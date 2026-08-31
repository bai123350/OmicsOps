import test from "node:test";
import assert from "node:assert/strict";
import { TabLedger } from "../tab-ledger.js";

test("ledger records only run-owned tabs and separates sessions/turns", () => {
  const ledger = new TabLedger({ sessionId: "workspace", clock: (() => { let value = 100; return () => value++; })() });
  ledger.touch("run-a", 3, { url: "https://example.org/a?token=hidden", created: true, reason: "web_open_tab" });
  ledger.touch("run-a", 3, { url: "https://example.org/b", reason: "web_scan" });
  ledger.touch("run-b", 4, { url: "http://example.net", reason: "web_open_tab" });
  assert.throws(() => ledger.assertControlled("run-a", 4), (error) => error.code === "TAB_NOT_CONTROLLED");
  const snapshot = ledger.snapshot("run-a");
  assert.deepEqual(snapshot.created_tab_ids, [3]);
  assert.deepEqual(snapshot.touched_tab_ids, [3]);
  assert.equal(snapshot.tabs[0].url, "https://example.org/b");
  assert.equal(ledger.tabSummaries()[0].origin, "https://example.org");
  ledger.markClosed("run-a", 3);
  assert.deepEqual(ledger.snapshot("run-a").closed_tab_ids, [3]);
});

test("blocked navigation remains auditable without retaining a blocked URL", () => {
  const ledger = new TabLedger({ sessionId: "shared" });
  ledger.touch("run", 1, { url: "https://example.org", created: true });
  ledger.observeBlockedNavigation(1, "chrome://settings");
  const record = ledger.snapshot("run").tabs[0];
  assert.equal(record.blocked, true);
  assert.equal(record.url, "https://example.org/");
});

test("session persistence restores ownership without storing URL paths or titles", () => {
  const ledger = new TabLedger({ sessionId: "workspace" });
  ledger.touch("run-persist", 7, { url: "https://example.org/private/paper?token=secret", title: "Sensitive title", created: true });
  const persisted = ledger.exportState();
  assert.equal(persisted.turns[0].tabs[0].origin, "https://example.org");
  assert.equal(JSON.stringify(persisted).includes("private"), false);
  assert.equal(JSON.stringify(persisted).includes("Sensitive title"), false);

  const restored = new TabLedger({ sessionId: "workspace", initialState: persisted });
  assert.equal(restored.has("run-persist", 7), true);
  assert.equal(restored.tabSummaries()[0].created_by_run, true);
  assert.throws(() => new TabLedger({ sessionId: "shared", initialState: persisted }), /invalid/);
});
