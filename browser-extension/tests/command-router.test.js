import test from "node:test";
import assert from "node:assert/strict";
import { CommandRouter } from "../command-router.js";
import { TabLedger } from "../tab-ledger.js";

const RUN_ID = "123e4567-e89b-42d3-a456-426614174000";

function makeChrome() {
  const tabs = new Map();
  let nextTabId = 10;
  const removed = [];
  const downloads = new Map();
  const downloadRequests = [];
  const chrome = {
    tabs: {
      create: async (options) => {
        const tab = { id: nextTabId++, windowId: options.windowId || 1, url: options.url, active: options.active };
        tabs.set(tab.id, tab);
        return tab;
      },
      get: async (id) => tabs.get(id),
      sendMessage: async () => ({ ok: true, result: { kind: "structured_page_scan", captcha: { detected: false }, text: "text" } }),
      captureVisibleTab: async (windowId) => windowId === 1 ? "data:image/png;base64,AAAA" : "bad",
      remove: async (id) => { tabs.delete(id); removed.push(id); },
    },
    debugger: {
      attach: async () => {},
      sendCommand: async (_target, method) => {
        if (method === "Page.getFrameTree") return { frameTree: { frame: { id: "main-frame" } } };
        if (method === "Page.createIsolatedWorld") return { executionContextId: 17 };
        if (method === "Runtime.evaluate") return { result: { value: 42 } };
        return { data: "AAAA" };
      },
      detach: async () => {},
    },
    downloads: {
      download: async (options) => {
        downloadRequests.push(options);
        const id = downloads.size + 1;
        downloads.set(id, { id, state: "complete", filename: `C:\\Users\\Researcher\\Downloads\\${options.filename}`, fileSize: 123 });
        return id;
      },
      search: async ({ id }) => id === undefined ? [...downloads.values()] : [downloads.get(id)].filter(Boolean),
    },
  };
  return { chrome, tabs, removed, downloadRequests };
}

test("router supports open, scan, CDP execute, screenshot, staged assets, and run cleanup", async () => {
  const { chrome, removed, downloadRequests } = makeChrome();
  const router = new CommandRouter({ chromeApi: chrome, ledger: new TabLedger({ sessionId: "shared" }) });
  const opened = await router.execute({ requestId: "open", runId: RUN_ID, method: "web_open_tab", payload: { url: "https://example.org" } });
  assert.equal(opened.ok, true);
  const tabId = opened.data.tab.tab_id;
  const scan = await router.execute({ requestId: "scan", runId: RUN_ID, method: "web_scan", payload: { tab_id: tabId, page_kind: "source" } });
  assert.equal(scan.ok, true);
  const execution = await router.execute({ requestId: "exec", runId: RUN_ID, method: "web_execute_js", payload: { tab_id: tabId, script: "return 6 * 7;" } });
  assert.equal(execution.data.executed, true);
  assert.equal(execution.data.result_policy, "discarded_rescan_required");
  assert.equal("value" in execution.data, false);
  const shot = await router.execute({ requestId: "shot", runId: RUN_ID, method: "web_screenshot", payload: { tab_id: tabId, relative_path: "shot.png" } });
  assert.match(shot.data.data_url, /^data:image\/png/);
  const staged = await router.execute({ requestId: "assets", runId: RUN_ID, method: "web_save_assets", payload: { target_host: "example.org", assets: [{ relative_path: "paper.pdf", source_url: "https://example.org/paper.pdf" }] } });
  assert.match(staged.data.assets[0].staged_id, /^asset_/);
  assert.equal(staged.data.assets[0].state, "download_complete");
  assert.equal(downloadRequests[0].conflictAction, "uniquify");
  const closed = await router.execute({ requestId: "close", runId: RUN_ID, method: "close_run_tabs", payload: {} });
  assert.deepEqual(closed.data.closed_tab_ids, [tabId]);
  assert.deepEqual(removed, [tabId]);
});

test("router rejects arbitrary tabs, disallowed URLs, and page AI prompting", async () => {
  const { chrome } = makeChrome();
  const router = new CommandRouter({ chromeApi: chrome, ledger: new TabLedger({ sessionId: "shared" }) });
  const unknown = await router.execute({ requestId: "scan", runId: RUN_ID, method: "web_scan", payload: { tab_id: 999, page_kind: "source" } });
  assert.equal(unknown.ok, false);
  assert.match(unknown.error, /TAB_NOT_FOUND/);
  const blocked = await router.execute({ requestId: "open", runId: RUN_ID, method: "web_open_tab", payload: { url: "file:///secret" } });
  assert.equal(blocked.ok, false);
  assert.match(blocked.error, /URL/);
  const opened = await router.execute({ requestId: "open2", runId: RUN_ID, method: "web_open_tab", payload: { url: "https://example.org" } });
  const ai = await router.execute({ requestId: "exec", runId: RUN_ID, method: "web_execute_js", payload: { tab_id: opened.data.tab.tab_id, script: "fetch('https://chatgpt.com/api/chat')" } });
  assert.equal(ai.ok, false);
  assert.match(ai.error, /EXECUTE_JS_BLOCKED/);
  const cookie = await router.execute({ requestId: "cookie", runId: RUN_ID, method: "web_execute_js", payload: { tab_id: opened.data.tab.tab_id, script: "return document['cookie'];" } });
  assert.equal(cookie.ok, false);
  assert.match(cookie.error, /EXECUTE_JS_BLOCKED/);
  const mismatch = await router.execute({ requestId: "host", runId: RUN_ID, method: "web_execute_js", payload: { tab_id: opened.data.tab.tab_id, target_host: "other.example", script: "return 1;" } });
  assert.equal(mismatch.ok, false);
  assert.match(mismatch.error, /TARGET_HOST_MISMATCH/);
  const assetMismatch = await router.execute({ requestId: "asset-host", runId: RUN_ID, method: "web_save_assets", payload: { target_host: "other.example", assets: [{ relative_path: "paper.pdf", source_url: "https://example.org/paper.pdf" }] } });
  assert.equal(assetMismatch.ok, false);
  assert.match(assetMismatch.error, /TARGET_HOST_MISMATCH/);
});

test("cleanup survives a worker ledger restart and refuses a reused tab ID at another origin", async () => {
  const { chrome, removed } = makeChrome();
  const first = new CommandRouter({ chromeApi: chrome, ledger: new TabLedger({ sessionId: "shared" }) });
  const opened = await first.execute({ requestId: "open", runId: RUN_ID, method: "web_open_tab", payload: { url: "https://example.org/paper" } });
  const tabId = opened.data.tab.tab_id;

  const emptyRestart = new CommandRouter({ chromeApi: chrome, ledger: new TabLedger({ sessionId: "shared" }) });
  const unproven = await emptyRestart.execute({ requestId: "unproven", runId: RUN_ID, method: "close_run_tabs", payload: { tab_targets: [{ tab_id: tabId, origin: "https://example.org" }] } });
  assert.match(unproven.data.failures[0].error, /does not prove ownership/);

  const restarted = new CommandRouter({ chromeApi: chrome, ledger: new TabLedger({ sessionId: "shared", initialState: first.ledger.exportState() }) });
  const closed = await restarted.execute({ requestId: "close", runId: RUN_ID, method: "close_run_tabs", payload: { tab_targets: [{ tab_id: tabId, origin: "https://example.org" }] } });
  assert.deepEqual(closed.data.closed_tab_ids, [tabId]);
  assert.deepEqual(closed.data.failures, []);
  assert.deepEqual(removed, [tabId]);

  const absent = await restarted.execute({ requestId: "close-again", runId: RUN_ID, method: "close_run_tabs", payload: { tab_targets: [{ tab_id: tabId, origin: "https://example.org" }] } });
  assert.deepEqual(absent.data.already_closed_tab_ids, [tabId]);

  const other = await first.execute({ requestId: "other", runId: RUN_ID, method: "web_open_tab", payload: { url: "https://other.example" } });
  const mismatchRouter = new CommandRouter({ chromeApi: chrome, ledger: new TabLedger({ sessionId: "shared", initialState: first.ledger.exportState() }) });
  const refused = await mismatchRouter.execute({ requestId: "refuse", runId: RUN_ID, method: "close_run_tabs", payload: { tab_targets: [{ tab_id: other.data.tab.tab_id, origin: "https://example.org" }] } });
  assert.deepEqual(refused.data.closed_tab_ids, []);
  assert.match(refused.data.failures[0].error, /another origin/);
});

test("execute_js blocks web AI hosts even with harmless source", async () => {
  const { chrome } = makeChrome();
  const router = new CommandRouter({ chromeApi: chrome, ledger: new TabLedger({ sessionId: "shared" }) });
  const opened = await router.execute({ requestId: "ai-open", runId: RUN_ID, method: "web_open_tab", payload: { url: "https://chatgpt.com/" } });
  const blocked = await router.execute({ requestId: "ai-exec", runId: RUN_ID, method: "web_execute_js", payload: { tab_id: opened.data.tab.tab_id, script: "return document.title;" } });
  assert.equal(blocked.ok, false);
  assert.match(blocked.error, /EXECUTE_JS_BLOCKED/);
});

test("CDP detach failure is reported instead of claiming the browser action succeeded", async () => {
  const { chrome } = makeChrome();
  chrome.debugger.detach = async () => { throw new Error("detach failed"); };
  const router = new CommandRouter({ chromeApi: chrome, ledger: new TabLedger({ sessionId: "shared" }) });
  const opened = await router.execute({ requestId: "open", runId: RUN_ID, method: "web_open_tab", payload: { url: "https://example.org" } });
  const execution = await router.execute({ requestId: "exec", runId: RUN_ID, method: "web_execute_js", payload: { tab_id: opened.data.tab.tab_id, script: "return 1;" } });
  assert.equal(execution.ok, false);
  assert.match(execution.error, /DEBUGGER_CLEANUP_FAILED/);
});
