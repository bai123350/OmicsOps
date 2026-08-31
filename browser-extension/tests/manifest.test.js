import test from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import crypto from "node:crypto";
import path from "node:path";
import { EXTENSION_ID, BRIDGE_ENDPOINTS, REQUIRED_CAPABILITIES } from "../protocol.js";

const root = path.resolve(import.meta.dirname, "..");

test("manifest has pinned key, valid entry points, endpoints, and required capabilities", () => {
  const manifest = JSON.parse(fs.readFileSync(path.join(root, "manifest.json"), "utf8"));
  assert.equal(manifest.manifest_version, 3);
  assert.equal(manifest.background.service_worker, "service-worker.js");
  assert.equal(fs.existsSync(path.join(root, manifest.background.service_worker)), true);
  assert.equal(fs.existsSync(path.join(root, manifest.content_scripts[0].js[0])), true);
  assert.equal(fs.existsSync(path.join(root, "session.html")), true);
  assert.equal(fs.existsSync(path.join(root, "session.js")), true);
  assert.equal(manifest.permissions.includes("downloads"), true);
  assert.equal(manifest.permissions.includes("debugger"), true);
  assert.equal(manifest.permissions.includes("storage"), true);
  const digest = crypto.createHash("sha256").update(Buffer.from(manifest.key, "base64")).digest();
  const extensionId = [...digest.subarray(0, 16)].map((byte) => String.fromCharCode(97 + (byte >> 4), 97 + (byte & 15))).join("");
  assert.equal(extensionId, EXTENSION_ID);
  assert.deepEqual(Object.keys(BRIDGE_ENDPOINTS).sort(), ["shared", "workspace"]);
  assert.deepEqual(REQUIRED_CAPABILITIES, ["tabs", "scan", "search", "screenshot", "downloads", "debugger"]);
});
