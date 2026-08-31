import test from "node:test";
import assert from "node:assert/strict";
import {
  EXTENSION_ID,
  METHODS,
  PROTOCOL_VERSION,
  REQUIRED_CAPABILITIES,
  ProtocolError,
  decodeFrame,
  makeHello,
  makeReply,
  parseCommand,
  validateHelloAck,
} from "../protocol.js";

const RUN_ID = "123e4567-e89b-42d3-a456-426614174000";

test("hello matches the pinned Rust bridge contract", () => {
  assert.deepEqual(makeHello("shared"), {
    type: "hello",
    protocol_version: PROTOCOL_VERSION,
    extension_id: EXTENSION_ID,
    session: "shared",
    capabilities: [...REQUIRED_CAPABILITIES],
  });
  assert.equal(makeHello("shared", { tabSummaries: [] }).tab_summaries.length, 0);
  assert.throws(() => makeHello("shared", { tabSummaries: [{ tab_id: 1, run_id: "bad", title: "", origin: "https://example.org", created_by_run: true }] }), (error) => error.code === "INVALID_FIELD");
});

test("hello rejects a changed extension ID or missing required capability", () => {
  assert.throws(() => makeHello("workspace", { extensionId: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa" }), ProtocolError);
  assert.throws(() => makeHello("shared", { capabilities: METHODS }), (error) => ["CAPABILITY_MISSING", "CAPABILITY_UNKNOWN"].includes(error.code));
  assert.deepEqual(validateHelloAck({ type: "hello_ack", protocol_version: 1, session: "workspace" }, "workspace").session, "workspace");
  assert.throws(() => validateHelloAck({ type: "hello_ack", protocol_version: 2, session: "workspace" }, "workspace"), (error) => error.code === "VERSION_MISMATCH");
  assert.throws(() => validateHelloAck({ type: "hello_ack", protocol_version: 1, session: "shared" }, "workspace"), (error) => error.code === "SESSION_MISMATCH");
});

test("command and reply fields are exact and run-scoped", () => {
  const command = parseCommand({
    type: "command",
    request_id: "request-1",
    protocol_version: 1,
    run_id: RUN_ID,
    method: "web_scan",
    payload: { tab_id: 7, page_kind: "source" },
  }, { session: "shared" });
  assert.equal(command.requestId, "request-1");
  assert.equal(command.runId, RUN_ID);
  assert.equal(command.method, "web_scan");
  assert.deepEqual(makeReply("request-1", true, { ok_value: true }), {
    request_id: "request-1",
    ok: true,
    data: { ok_value: true },
    error: null,
  });
  assert.throws(() => parseCommand({
    type: "command", request_id: "bad", protocol_version: 1, run_id: RUN_ID,
    method: "web_unknown", payload: {},
  }), (error) => error.code === "UNKNOWN_METHOD");
  assert.throws(() => parseCommand({
    type: "command", request_id: "bad", protocol_version: 1, run_id: "not-a-uuid",
    method: "web_scan", payload: {},
  }), (error) => error.code === "INVALID_FIELD");
});

test("decoder rejects non-JSON and oversized frames", () => {
  assert.deepEqual(decodeFrame(JSON.stringify({ hello: "world" })), { hello: "world" });
  assert.throws(() => decodeFrame("not json"), (error) => error.code === "INVALID_JSON");
  assert.throws(() => decodeFrame(JSON.stringify({ x: "12345" }), 10), (error) => error.code === "FRAME_TOO_LARGE");
});
