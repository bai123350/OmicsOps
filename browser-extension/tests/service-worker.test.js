import test from "node:test";
import assert from "node:assert/strict";
import { OmicsOpsBrowserBridge } from "../service-worker.js";

test("one extension instance owns only its explicitly selected session", () => {
  const shared = new OmicsOpsBrowserBridge({ session: "shared", chromeApi: {}, reconnect: false });
  assert.deepEqual(Object.keys(shared.clients), ["shared"]);
  assert.deepEqual(Object.keys(shared.routers), ["shared"]);
  const workspace = new OmicsOpsBrowserBridge({ session: "workspace", chromeApi: {}, reconnect: false });
  assert.deepEqual(Object.keys(workspace.clients), ["workspace"]);
  assert.throws(() => new OmicsOpsBrowserBridge({ session: "other", chromeApi: {} }), /session/);
});
