import test from "node:test";
import assert from "node:assert/strict";
import { OmicsOpsWebSocketClient } from "../websocket-client.js";

const RUN_ID = "123e4567-e89b-42d3-a456-426614174000";

class FakeSocket {
  static instances = [];

  constructor(endpoint) {
    this.endpoint = endpoint;
    this.readyState = 0;
    this.sent = [];
    FakeSocket.instances.push(this);
    queueMicrotask(() => {
      this.readyState = 1;
      this.onopen?.();
    });
  }

  send(value) {
    this.sent.push(JSON.parse(value));
    const message = JSON.parse(value);
    if (message.type === "hello") queueMicrotask(() => this.onmessage?.({ data: JSON.stringify({ type: "hello_ack", protocol_version: 1, session: message.session }) }));
  }

  close(code = 1000, reason = "") {
    this.readyState = 3;
    this.onclose?.({ code, reason });
  }

  serverCommand(command) {
    this.onmessage?.({ data: JSON.stringify(command) });
  }
}

test("client performs strict session handshake and replies to commands", async () => {
  FakeSocket.instances.length = 0;
  const replies = [];
  const client = new OmicsOpsWebSocketClient({
    session: "shared",
    WebSocketImpl: FakeSocket,
    reconnect: false,
    onCommand: async (command) => {
      replies.push(command);
      return { request_id: command.requestId, ok: true, data: { accepted: true }, error: null };
    },
  });
  assert.equal(await client.connect(), true);
  const socket = FakeSocket.instances[0];
  assert.equal(socket.endpoint, "ws://127.0.0.1:18775/v1/session/shared");
  assert.equal(socket.sent[0].type, "hello");
  assert.equal(socket.sent[0].extension_id, "joifljknpalpoppceknociillogolbnb");
  socket.serverCommand({ type: "command", request_id: "r-1", protocol_version: 1, run_id: RUN_ID, method: "web_search", payload: { query: "paper" } });
  await new Promise((resolve) => setImmediate(resolve));
  assert.equal(replies[0].method, "web_search");
  assert.deepEqual(socket.sent.find((message) => message.request_id === "r-1"), { request_id: "r-1", ok: true, data: { accepted: true }, error: null });
  assert.deepEqual(socket.sent.find((message) => message.type === "tab_state").tab_summaries, []);
  client.stop();
});

test("client rejects a non-loopback or wrong-session endpoint", () => {
  assert.throws(() => new OmicsOpsWebSocketClient({ session: "shared", endpoint: "ws://example.org/v1/session/shared", WebSocketImpl: FakeSocket }), /endpoint is pinned/);
  assert.throws(() => new OmicsOpsWebSocketClient({ session: "shared", endpoint: "ws://127.0.0.1:18776/v1/session/workspace", WebSocketImpl: FakeSocket }), /endpoint is pinned/);
});
