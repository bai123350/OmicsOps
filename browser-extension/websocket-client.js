import {
  BRIDGE_ENDPOINTS,
  EXTENSION_ID,
  METHODS,
  PROTOCOL_VERSION,
  decodeFrame,
  makeHello,
  makeReply,
  makeTabState,
  parseCommand,
  protocolErrorText,
  validateHelloAck,
} from "./protocol.js";

const OPEN = 1;
const CONNECTING = 0;
const DEFAULT_HANDSHAKE_TIMEOUT_MS = 5_000;
const DEFAULT_RECONNECT_DELAY_MS = 1_000;
const MAX_RECONNECT_DELAY_MS = 30_000;

function wait(milliseconds) {
  return new Promise((resolve) => setTimeout(resolve, milliseconds));
}

function socketData(event) {
  return event && Object.prototype.hasOwnProperty.call(event, "data") ? event.data : event;
}

function closeSocket(socket, code = 1002, reason = "protocol error") {
  try { socket.close(code, reason.slice(0, 123)); } catch { /* socket may already be gone */ }
}

/**
 * One session connection.  The caller owns one instance for `shared` and one
 * for `workspace`; no endpoint fallback is allowed across session names.
 */
export class OmicsOpsWebSocketClient {
  constructor({
    session,
    WebSocketImpl = globalThis.WebSocket,
    endpoint = BRIDGE_ENDPOINTS[session],
    onCommand = async () => makeReply("unknown", false, {}, "command handler unavailable"),
    onStatus = () => {},
    getTabSummaries = () => [],
    handshakeTimeoutMs = DEFAULT_HANDSHAKE_TIMEOUT_MS,
    reconnect = true,
  } = {}) {
    if (!Object.prototype.hasOwnProperty.call(BRIDGE_ENDPOINTS, session)) throw new TypeError("session must be shared or workspace");
    if (endpoint !== BRIDGE_ENDPOINTS[session]) throw new TypeError("browser bridge endpoint is pinned to loopback");
    this.session = session;
    this.endpoint = endpoint;
    this.WebSocketImpl = WebSocketImpl;
    this.onCommand = onCommand;
    this.onStatus = onStatus;
    this.getTabSummaries = getTabSummaries;
    this.handshakeTimeoutMs = handshakeTimeoutMs;
    this.reconnect = reconnect;
    this.socket = null;
    this.state = "idle";
    this.reconnectDelay = DEFAULT_RECONNECT_DELAY_MS;
    this.reconnectTimer = null;
    this.stopped = false;
    this.handling = new Set();
  }

  async connect() {
    if (this.stopped) return false;
    if (this.state === "ready" || this.state === "connecting" || this.state === "handshaking") return true;
    if (typeof this.WebSocketImpl !== "function") {
      this.setStatus("unavailable", { error: "WebSocket is unavailable" });
      this.scheduleReconnect();
      return false;
    }
    this.state = "connecting";
    this.setStatus("connecting");
    let socket;
    try {
      socket = new this.WebSocketImpl(this.endpoint);
    } catch (error) {
      this.setStatus("disconnected", { error: String(error?.message || error) });
      this.scheduleReconnect();
      return false;
    }
    this.socket = socket;
    const opened = await new Promise((resolve) => {
      let settled = false;
      const finish = (value) => {
        if (settled) return;
        settled = true;
        clearTimeout(timer);
        resolve(value);
      };
      const timer = setTimeout(() => {
        closeSocket(socket, 1000, "connection timeout");
        finish(false);
      }, this.handshakeTimeoutMs);
      socket.onopen = () => finish(true);
      socket.onerror = () => finish(false);
      socket.onclose = () => finish(false);
    });
    if (!opened || this.socket !== socket || socket.readyState === 3) {
      if (this.socket === socket) this.socket = null;
      this.state = "disconnected";
      this.setStatus("disconnected");
      this.scheduleReconnect();
      return false;
    }
    this.state = "handshaking";
    this.setStatus("handshaking");
    socket.onmessage = (event) => { void this.handleMessage(socketData(event), socket); };
    socket.onerror = () => {
      if (this.socket === socket) this.setStatus("transport_error");
    };
    socket.onclose = () => this.handleClose(socket);
    try {
      this.sendRaw(makeHello(this.session, {
        extensionId: EXTENSION_ID,
        tabSummaries: this.getTabSummaries(),
      }));
    } catch (error) {
      closeSocket(socket, 1002, protocolErrorText(error));
      return false;
    }
    return this.waitUntilReady(socket);
  }

  waitUntilReady(socket) {
    return new Promise((resolve) => {
      const started = Date.now();
      const check = () => {
        if (this.socket !== socket || this.state === "disconnected" || this.state === "closed") {
          resolve(false);
          return;
        }
        if (this.state === "ready") {
          this.reconnectDelay = DEFAULT_RECONNECT_DELAY_MS;
          resolve(true);
          return;
        }
        if (Date.now() - started >= this.handshakeTimeoutMs) {
          closeSocket(socket, 1002, "hello_ack timeout");
          resolve(false);
          return;
        }
        setTimeout(check, 10);
      };
      check();
    });
  }

  sendRaw(frame) {
    if (!this.socket || this.socket.readyState !== OPEN) throw new Error("browser bridge socket is not open");
    this.socket.send(JSON.stringify(frame));
  }

  async handleMessage(raw, socket) {
    if (this.socket !== socket) return;
    let message;
    try {
      message = decodeFrame(raw);
      if (this.state === "handshaking") {
        const ack = validateHelloAck(message, this.session);
        this.state = "ready";
        this.setStatus("ready", { protocol_version: PROTOCOL_VERSION, methods: METHODS });
        this.sendTabState();
        return;
      }
      if (this.state !== "ready") throw new Error("message received before handshake completed");
      const command = parseCommand(message, { session: this.session });
      const key = command.requestId;
      if (this.handling.has(key)) throw new Error("duplicate request_id is in flight");
      this.handling.add(key);
      try {
        const reply = await this.onCommand(command);
        if (!reply || reply.request_id !== key || typeof reply.ok !== "boolean") {
          throw new Error("command handler returned an invalid reply");
        }
        this.sendRaw(makeReply(key, reply.ok, reply.data, reply.error));
      } catch (error) {
        this.sendRaw(makeReply(key, false, {}, protocolErrorText(error)));
      } finally {
        this.handling.delete(key);
      }
    } catch (error) {
      closeSocket(socket, 1002, protocolErrorText(error));
      this.setStatus("protocol_error", { error: protocolErrorText(error) });
    }
  }

  sendTabState() {
    if (this.state !== "ready" || !this.socket || this.socket.readyState !== OPEN) return false;
    try {
      this.sendRaw(makeTabState(this.session, this.getTabSummaries()));
      return true;
    } catch {
      return false;
    }
  }

  handleClose(socket) {
    if (this.socket !== socket) return;
    this.socket = null;
    if (this.state !== "closed") {
      this.state = "disconnected";
      this.setStatus("disconnected");
      this.scheduleReconnect();
    }
  }

  setStatus(state, details = {}) {
    this.state = state;
    try { this.onStatus({ session: this.session, state, endpoint: this.endpoint, ...details }); } catch { /* status observers are non-critical */ }
  }

  scheduleReconnect() {
    if (!this.reconnect || this.stopped || this.reconnectTimer !== null) return;
    const delay = this.reconnectDelay;
    this.reconnectDelay = Math.min(MAX_RECONNECT_DELAY_MS, this.reconnectDelay * 2);
    this.reconnectTimer = setTimeout(() => {
      this.reconnectTimer = null;
      void this.connect();
    }, delay);
  }

  stop() {
    this.stopped = true;
    if (this.reconnectTimer !== null) {
      clearTimeout(this.reconnectTimer);
      this.reconnectTimer = null;
    }
    if (this.socket) closeSocket(this.socket, 1000, "bridge stopped");
    this.socket = null;
    this.setStatus("closed");
  }
}

export function endpointForSession(session) {
  return BRIDGE_ENDPOINTS[session];
}
