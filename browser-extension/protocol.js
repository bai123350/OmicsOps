/**
 * Clean-room wire contract for the OmicsOps browser bridge.
 *
 * The Rust bridge is the protocol authority. This file intentionally keeps
 * the contract small: hello frames identify the packaged extension and one of
 * the two sessions; commands and replies use the exact fields consumed by the
 * Rust side.
 */

export const PROTOCOL_VERSION = 1;
export const EXTENSION_VERSION = "0.1.0";
export const EXTENSION_ID = "joifljknpalpoppceknociillogolbnb";

export const SESSION_NAMES = Object.freeze(["shared", "workspace"]);
export const REQUIRED_CAPABILITIES = Object.freeze([
  "tabs",
  "scan",
  "search",
  "screenshot",
  "downloads",
  "debugger",
]);

export const METHODS = Object.freeze([
  "web_search",
  "web_open_tab",
  "web_scan",
  "web_execute_js",
  "web_screenshot",
  "web_save_assets",
  "close_run_tabs",
]);

export const BRIDGE_ENDPOINTS = Object.freeze({
  shared: "ws://127.0.0.1:18775/v1/session/shared",
  workspace: "ws://127.0.0.1:18776/v1/session/workspace",
});

export const LIMITS = Object.freeze({
  maxFrameBytes: 2 * 1024 * 1024,
  maxRequestIdLength: 128,
  maxRunIdLength: 64,
});

export class ProtocolError extends Error {
  constructor(code, message, details = undefined) {
    super(message);
    this.name = "ProtocolError";
    this.code = code;
    this.details = details;
  }
}

function isRecord(value) {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

function requiredString(value, field, maxLength) {
  if (typeof value !== "string" || value.length === 0 || value.length > maxLength) {
    throw new ProtocolError("INVALID_FIELD", `${field} must be a non-empty string`, { field });
  }
  return value;
}

function exactSession(value, field = "session") {
  const session = requiredString(value, field, 32);
  if (!SESSION_NAMES.includes(session)) {
    throw new ProtocolError("INVALID_SESSION", `unsupported ${field}`, { value: session });
  }
  return session;
}

function exactCapabilities(value) {
  if (!Array.isArray(value)) {
    throw new ProtocolError("INVALID_FIELD", "capabilities must be an array");
  }
  const seen = new Set();
  for (const capability of value) {
    requiredString(capability, "capabilities[]", 64);
    if (seen.has(capability)) {
      throw new ProtocolError("INVALID_FIELD", "capabilities must not contain duplicates");
    }
    seen.add(capability);
  }
  return [...value];
}

function assertRequiredCapabilities(value) {
  const capabilities = exactCapabilities(value);
  const allowed = new Set(REQUIRED_CAPABILITIES);
  if (capabilities.some((capability) => !allowed.has(capability))) {
    throw new ProtocolError("CAPABILITY_UNKNOWN", "hello contains an unknown capability");
  }
  for (const required of REQUIRED_CAPABILITIES) {
    if (!capabilities.includes(required)) {
      throw new ProtocolError("CAPABILITY_MISSING", `required capability is missing: ${required}`);
    }
  }
  return capabilities;
}

export function makeHello(session, {
  extensionId = EXTENSION_ID,
  capabilities = REQUIRED_CAPABILITIES,
  tabSummaries = undefined,
} = {}) {
  const selectedSession = exactSession(session);
  const id = requiredString(extensionId, "extension_id", 64);
  if (id !== EXTENSION_ID) {
    throw new ProtocolError("EXTENSION_ID_MISMATCH", "manifest extension ID does not match the bridge contract");
  }
  const offered = assertRequiredCapabilities(capabilities);
  const hello = {
    type: "hello",
    protocol_version: PROTOCOL_VERSION,
    extension_id: id,
    session: selectedSession,
    capabilities: offered,
  };
  if (tabSummaries !== undefined) hello.tab_summaries = validateTabSummaries(tabSummaries);
  return hello;
}

function isUuid(value) {
  return typeof value === "string"
    && /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i.test(value);
}

function isHttpUrl(value) {
  try {
    const protocol = new URL(value).protocol;
    return protocol === "http:" || protocol === "https:";
  } catch {
    return false;
  }
}

export function validateTabSummaries(value) {
  if (!Array.isArray(value) || value.length > 100) {
    throw new ProtocolError("INVALID_FIELD", "tab_summaries must be an array within the size limit");
  }
  return value.map((summary) => {
    if (!isRecord(summary)) throw new ProtocolError("INVALID_FIELD", "tab summary must be an object");
    if (!Number.isInteger(summary.tab_id) || summary.tab_id <= 0) throw new ProtocolError("INVALID_FIELD", "tab summary tab_id is invalid");
    if (!isUuid(summary.run_id)) throw new ProtocolError("INVALID_FIELD", "tab summary run_id must be a UUID");
    if (typeof summary.title !== "string" || summary.title.length > 200) throw new ProtocolError("INVALID_FIELD", "tab summary title is invalid");
    if (typeof summary.origin !== "string" || !isHttpUrl(summary.origin) || new URL(summary.origin).origin !== summary.origin) throw new ProtocolError("INVALID_FIELD", "tab summary origin is invalid");
    if (typeof summary.created_by_run !== "boolean") throw new ProtocolError("INVALID_FIELD", "tab summary ownership is invalid");
    return {
      tab_id: summary.tab_id,
      run_id: summary.run_id,
      title: summary.title,
      origin: summary.origin,
      created_by_run: summary.created_by_run,
    };
  });
}

export function makeTabState(session, tabSummaries) {
  return {
    type: "tab_state",
    protocol_version: PROTOCOL_VERSION,
    session: exactSession(session),
    tab_summaries: validateTabSummaries(tabSummaries),
  };
}

export function validateHelloAck(message, expectedSession) {
  if (!isRecord(message)) throw new ProtocolError("INVALID_FRAME", "hello_ack must be an object");
  if (message.type !== "hello_ack") {
    throw new ProtocolError("HANDSHAKE_REQUIRED", "expected hello_ack during handshake");
  }
  if (message.protocol_version !== PROTOCOL_VERSION) {
    throw new ProtocolError("VERSION_MISMATCH", "unsupported bridge protocol version");
  }
  const session = exactSession(message.session);
  if (session !== expectedSession) {
    throw new ProtocolError("SESSION_MISMATCH", "hello_ack belongs to another session");
  }
  return { session, protocolVersion: message.protocol_version };
}

export function parseCommand(message, { session = undefined } = {}) {
  if (!isRecord(message)) throw new ProtocolError("INVALID_FRAME", "command must be an object");
  if (message.type !== "command") throw new ProtocolError("INVALID_COMMAND", "expected command frame");
  if (message.protocol_version !== PROTOCOL_VERSION) {
    throw new ProtocolError("VERSION_MISMATCH", "unsupported bridge protocol version");
  }
  const requestId = requiredString(message.request_id, "request_id", LIMITS.maxRequestIdLength);
  if (typeof message.run_id !== "string" || message.run_id.length > LIMITS.maxRunIdLength || !isUuid(message.run_id)) {
    throw new ProtocolError("INVALID_FIELD", "run_id must be a UUID", { field: "run_id" });
  }
  if (!METHODS.includes(message.method)) {
    throw new ProtocolError("UNKNOWN_METHOD", `unsupported browser method: ${String(message.method)}`);
  }
  if (!isRecord(message.payload)) {
    throw new ProtocolError("INVALID_FIELD", "payload must be an object", { field: "payload" });
  }
  if (session !== undefined && message.payload.session !== undefined && message.payload.session !== session) {
    throw new ProtocolError("SESSION_MISMATCH", "command payload belongs to another session");
  }
  return {
    requestId,
    runId: message.run_id,
    method: message.method,
    payload: message.payload,
    session,
  };
}

export function makeReply(requestId, ok, data = {}, error = null) {
  const id = requiredString(requestId, "request_id", LIMITS.maxRequestIdLength);
  if (typeof ok !== "boolean") throw new ProtocolError("INVALID_FIELD", "ok must be a boolean");
  if (error !== null && typeof error !== "string") {
    throw new ProtocolError("INVALID_FIELD", "error must be a string or null");
  }
  return {
    request_id: id,
    ok,
    data: data === undefined ? {} : data,
    error,
  };
}

export function decodeFrame(raw, maxBytes = LIMITS.maxFrameBytes) {
  if (typeof raw !== "string") {
    if (raw instanceof ArrayBuffer) raw = new TextDecoder().decode(raw);
    else if (ArrayBuffer.isView(raw)) raw = new TextDecoder().decode(raw);
    else throw new ProtocolError("INVALID_FRAME", "WebSocket frame must be text JSON");
  }
  if (new TextEncoder().encode(raw).byteLength > maxBytes) {
    throw new ProtocolError("FRAME_TOO_LARGE", "WebSocket frame exceeds the size limit");
  }
  let decoded;
  try {
    decoded = JSON.parse(raw);
  } catch {
    throw new ProtocolError("INVALID_JSON", "WebSocket frame is not valid JSON");
  }
  if (!isRecord(decoded)) throw new ProtocolError("INVALID_FRAME", "decoded frame must be an object");
  return decoded;
}

export function protocolErrorText(error) {
  if (error instanceof ProtocolError) return `${error.code}: ${error.message}`;
  return "INTERNAL_ERROR: browser bridge request failed";
}

export function isProtocolRecord(value) {
  return isRecord(value);
}
