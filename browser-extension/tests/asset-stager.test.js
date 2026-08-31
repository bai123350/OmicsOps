import test from "node:test";
import assert from "node:assert/strict";
import { AssetStager } from "../asset-stager.js";

test("asset staging stores metadata and no bytes", () => {
  const stager = new AssetStager({ clock: () => 100, idFactory: () => "stage-1" });
  const [asset] = stager.stage({
    sessionId: "shared",
    turnId: "run-1",
    assets: [{ relative_path: "paper.pdf", size_bytes: 123, sha256: "A".repeat(64) }],
  });
  assert.deepEqual(asset, {
    staged_id: "stage-1",
    relative_path: "paper.pdf",
    session_id: "shared",
    turn_id: "run-1",
    size_bytes: 123,
    sha256: "a".repeat(64),
    staged_at: 100,
    state: "staged_metadata",
  });
  assert.equal("bytes" in asset, false);
  assert.throws(() => stager.stage({ sessionId: "shared", turnId: "run-1", assets: [{ relative_path: "../escape" }] }), /relative_path/);
  assert.throws(() => stager.stage({ sessionId: "shared", turnId: "run-1", assets: [{ staged_id: "chosen", relative_path: "paper.pdf" }] }), /extension-generated/);
});
