import { callChrome } from "./chrome-api.js";
import { AssetStager } from "./asset-stager.js";
import { assertExecuteJsAllowed, evaluateInIsolatedWorld } from "./execute-js-policy.js";
import { makeReply } from "./protocol.js";
import { buildSearchUrl } from "./search.js";
import { assertAllowedUrl, isAllowedUrl, sanitizeUrlForReport } from "./url-policy.js";
import { TabLedger } from "./tab-ledger.js";

const MAX_SCAN_OPTIONS_BYTES = 16 * 1024;
const MAX_SCREENSHOT_BYTES = 16 * 1024 * 1024;
const DOWNLOAD_WAIT_ATTEMPTS = 80;
const DOWNLOAD_WAIT_MS = 250;

function commandError(code, message, details = undefined) {
  const error = new Error(message);
  error.code = code;
  error.details = details;
  return error;
}

function tabIdFrom(payload) {
  const tabId = payload.tab_id;
  if (!Number.isInteger(tabId) || tabId < 0) throw commandError("INVALID_PAYLOAD", "tab_id must be a non-negative integer");
  return tabId;
}

function responseTab(tab) {
  return {
    tab_id: tab?.id,
    window_id: tab?.windowId,
    title: typeof tab?.title === "string" ? tab.title.slice(0, 500) : undefined,
    url: typeof tab?.url === "string" && isAllowedUrl(tab.url) ? sanitizeUrlForReport(tab.url) : undefined,
    active: Boolean(tab?.active),
  };
}

function validateObjectOptions(value) {
  if (value === undefined) return {};
  if (value === null || typeof value !== "object" || Array.isArray(value)) {
    throw commandError("INVALID_PAYLOAD", "options must be an object");
  }
  if (JSON.stringify(value).length > MAX_SCAN_OPTIONS_BYTES) {
    throw commandError("INVALID_PAYLOAD", "options exceed the size limit");
  }
  return value;
}

function normalizedHost(value) {
  if (typeof value !== "string" || value.length === 0 || value.length > 253 || value.trim() !== value || value.includes("*") || value.includes("/")) {
    throw commandError("INVALID_PAYLOAD", "target_host must be a concrete host");
  }
  const host = value.trim().toLowerCase().replace(/\.$/, "");
  if (!host || !/^[a-z0-9.-]+$/.test(host)) throw commandError("INVALID_PAYLOAD", "target_host is invalid");
  return host;
}

function hostOfUrl(value) {
  try { return new URL(value).hostname.toLowerCase().replace(/\.$/, ""); } catch { return null; }
}

function policyDomains(value) {
  if (value === undefined) return [];
  if (!Array.isArray(value) || value.length > 200) throw commandError("INVALID_PAYLOAD", "domain policy must be a bounded array");
  return value.map(normalizedHost);
}

function domainMatches(host, domain) {
  return host === domain || host.endsWith(`.${domain}`);
}

function applyLinkPolicy(links, payload) {
  const disabled = policyDomains(payload.disabled_domains);
  const preferred = policyDomains(payload.preferred_domains);
  return (Array.isArray(links) ? links : [])
    .filter((link) => {
      const host = hostOfUrl(link?.url);
      return host && !disabled.some((domain) => domainMatches(host, domain));
    })
    .sort((left, right) => {
      const leftHost = hostOfUrl(left.url);
      const rightHost = hostOfUrl(right.url);
      const leftPreferred = preferred.some((domain) => domainMatches(leftHost, domain));
      const rightPreferred = preferred.some((domain) => domainMatches(rightHost, domain));
      return Number(rightPreferred) - Number(leftPreferred);
    });
}

function assertDomainAllowed(url, payload) {
  const host = hostOfUrl(url);
  if (!host) throw commandError("URL_BLOCKED", "URL has no valid host");
  if (policyDomains(payload.disabled_domains).some((domain) => domainMatches(host, domain))) {
    throw commandError("DOMAIN_DISABLED", `browser policy disables ${host}`);
  }
}

function assertTargetHost(payload, url) {
  if (payload.target_host === undefined) return;
  const expected = normalizedHost(payload.target_host);
  const actual = hostOfUrl(url);
  if (!actual || actual !== expected) throw commandError("TARGET_HOST_MISMATCH", "tab host does not match target_host");
}

function isProhibitedAiHost(url) {
  const host = hostOfUrl(url);
  if (!host) return false;
  return ["chatgpt.com", "chat.openai.com", "gemini.google.com", "claude.ai", "copilot.microsoft.com"]
    .some((blocked) => host === blocked || host.endsWith(`.${blocked}`));
}

/**
 * Dispatches one Rust BrowserCommand.  The router has no model or network
 * client and all tab operations are explicit and recorded in the run ledger.
 */
export class CommandRouter {
  constructor({
    chromeApi = globalThis.chrome,
    ledger = new TabLedger(),
    stager = new AssetStager(),
    sendEvent = () => {},
    clock = Date.now,
    wait = (milliseconds) => new Promise((resolve) => setTimeout(resolve, milliseconds)),
  } = {}) {
    this.chrome = chromeApi;
    this.ledger = ledger;
    this.stager = stager;
    this.sendEvent = sendEvent;
    this.clock = clock;
    this.wait = wait;
  }

  setSession(session) {
    this.ledger.setSession(session);
    this.stager.clear({ sessionId: session });
  }

  async execute(command) {
    const turnId = command.runId;
    this.ledger.beginTurn(turnId);
    try {
      let data;
      switch (command.method) {
        case "web_search":
          data = await this.webSearch(command);
          break;
        case "web_open_tab":
          data = await this.openTab(command);
          break;
        case "web_scan":
          data = await this.scan(command);
          break;
        case "web_execute_js":
          data = await this.executeJs(command);
          break;
        case "web_screenshot":
          data = await this.screenshot(command);
          break;
        case "web_save_assets":
          data = await this.saveAssets(command);
          break;
        case "close_run_tabs":
          data = await this.closeRunTabs(command);
          break;
        default:
          throw commandError("UNKNOWN_METHOD", `unsupported browser method: ${command.method}`);
      }
      return makeReply(command.requestId, true, {
        ...data,
        tab_accounting: this.ledger.snapshot(turnId),
      }, null);
    } catch (error) {
      const message = error instanceof Error ? error.message : "browser command failed";
      return makeReply(command.requestId, false, {
        tab_accounting: this.ledger.snapshot(turnId),
      }, `${error?.code ? `${error.code}: ` : ""}${message}`);
    }
  }

  async getTab(tabId) {
    const fn = this.chrome?.tabs?.get;
    if (typeof fn !== "function") throw commandError("API_UNAVAILABLE", "tabs.get is unavailable");
    const tab = await callChrome(fn.bind(this.chrome.tabs), [tabId]);
    if (!tab || !Number.isInteger(tab.id)) throw commandError("TAB_NOT_FOUND", "tab does not exist");
    if (typeof tab.url === "string" && !isAllowedUrl(tab.url)) {
      throw commandError("URL_BLOCKED", "controlled tab is not on an HTTP(S) URL");
    }
    return tab;
  }

  async requireControlledTab(command, { adopt = false } = {}) {
    const tabId = tabIdFrom(command.payload);
    const tab = await this.getTab(tabId);
    assertTargetHost(command.payload, tab.url);
    if (!this.ledger.has(command.runId, tabId)) {
      if (!adopt) throw commandError("TAB_NOT_CONTROLLED", "tab must be opened or explicitly adopted in this run");
      this.ledger.touch(command.runId, tabId, {
        url: tab.url,
        title: tab.title,
        windowId: tab.windowId,
        reason: "explicit_adopt",
      });
    } else {
      this.ledger.touch(command.runId, tabId, {
        url: tab.url,
        title: tab.title,
        windowId: tab.windowId,
        reason: command.method,
      });
    }
    return tab;
  }

  async webSearch(command) {
    const payload = command.payload;
    const provider = payload.provider ?? "default";
    const url = buildSearchUrl(payload.query, provider);
    assertDomainAllowed(url, payload);
    assertTargetHost(payload, url);
    const tab = await this.createTab(command, url, {
      active: payload.active === undefined ? true : Boolean(payload.active),
    });
    return {
      tab_id: tab.id,
      target_host: hostOfUrl(tab.url ?? url),
      provider,
      query: payload.query,
      tab: responseTab(tab),
    };
  }

  async openTab(command) {
    const url = assertAllowedUrl(command.payload.url);
    assertDomainAllowed(url, command.payload);
    assertTargetHost(command.payload, url);
    const tab = await this.createTab(command, url, {
      active: command.payload.active === undefined ? true : Boolean(command.payload.active),
      windowId: command.payload.window_id,
    });
    return { tab_id: tab.id, target_host: hostOfUrl(tab.url ?? url), tab: responseTab(tab) };
  }

  async createTab(command, url, options = {}) {
    const create = this.chrome?.tabs?.create;
    if (typeof create !== "function") throw commandError("API_UNAVAILABLE", "tabs.create is unavailable");
    const createOptions = { url, active: Boolean(options.active) };
    if (options.windowId !== undefined) {
      if (!Number.isInteger(options.windowId) || options.windowId < 0) throw commandError("INVALID_PAYLOAD", "window_id must be a non-negative integer");
      createOptions.windowId = options.windowId;
    }
    const tab = await callChrome(create.bind(this.chrome.tabs), [createOptions]);
    if (!tab || !Number.isInteger(tab.id)) throw commandError("TAB_CREATE_FAILED", "browser did not return a tab ID");
    if (typeof tab.url === "string" && !isAllowedUrl(tab.url)) {
      throw commandError("URL_BLOCKED", "browser returned a non-HTTP(S) tab URL");
    }
    this.ledger.touch(command.runId, tab.id, {
      url: tab.url ?? url,
      title: tab.title,
      created: true,
      windowId: tab.windowId,
      reason: command.method,
    });
    return tab;
  }

  async scan(command) {
    const tab = await this.requireControlledTab(command, { adopt: command.payload.adopt === true });
    const record = this.ledger.assertControlled(command.runId, tab.id);
    const pageKind = command.payload.page_kind;
    if (pageKind === "search_results" && !record.reasons.includes("web_search")) {
      throw commandError("PAGE_KIND_MISMATCH", "search_results scans must target the run's search tab");
    }
    if (pageKind === "source" && !record.reasons.includes("web_open_tab") && command.payload.adopt !== true) {
      throw commandError("PAGE_KIND_MISMATCH", "source scans must target a run-opened or explicitly adopted source tab");
    }
    if (pageKind !== "search_results" && pageKind !== "source") {
      throw commandError("INVALID_PAYLOAD", "page_kind must be search_results or source");
    }
    const sendMessage = this.chrome?.tabs?.sendMessage;
    if (typeof sendMessage !== "function") throw commandError("API_UNAVAILABLE", "tabs.sendMessage is unavailable");
    const options = validateObjectOptions(command.payload.options);
    const result = await callChrome(sendMessage.bind(this.chrome.tabs), [tab.id, {
      type: "omicsops.scan",
      options,
    }]);
    if (!result || result.ok !== true) {
      throw commandError("SCAN_FAILED", result?.error || "content script did not return a scan");
    }
    const scan = result.result ?? result.data ?? result;
    if (scan?.captcha?.detected || scan?.requires_human) {
      this.sendEvent({
        type: "human_required",
        reason: "captcha_detected",
        run_id: command.runId,
        tab_id: tab.id,
      });
      throw commandError("CAPTCHA_HUMAN_REQUIRED", "captcha detected; human intervention is required");
    }
    const results = pageKind === "search_results" ? applyLinkPolicy(scan.links, command.payload) : [];
    return {
      tab_id: tab.id,
      target_host: hostOfUrl(tab.url),
      page_kind: pageKind,
      result_count: results.length,
      results,
      tab: responseTab(tab),
      scan,
    };
  }

  async executeJs(command) {
    const tab = await this.requireControlledTab(command, { adopt: command.payload.adopt === true });
    if (isProhibitedAiHost(tab.url)) throw commandError("EXECUTE_JS_BLOCKED", "execute_js is not permitted on web AI hosts");
    const source = assertExecuteJsAllowed(command.payload.script, { maxLength: 16_000 });
    const transport = command.payload.transport ?? "cdp";
    if (transport === "cdp") {
      await this.executeJsCdp(tab.id, source);
    } else if (transport === "scripting") {
      await this.executeJsScripting(tab.id, source);
    } else {
      throw commandError("INVALID_PAYLOAD", "transport must be cdp or scripting");
    }
    return {
      tab: responseTab(tab),
      transport,
      executed: true,
      result_policy: "discarded_rescan_required",
      policy: "no_page_ai_prompting_or_sensitive_browser_apis",
    };
  }

  async executeJsScripting(tabId, source) {
    const execute = this.chrome?.scripting?.executeScript;
    if (typeof execute !== "function") throw commandError("API_UNAVAILABLE", "scripting.executeScript is unavailable");
    const results = await callChrome(execute.bind(this.chrome.scripting), [{
      target: { tabId },
      world: "ISOLATED",
      func: evaluateInIsolatedWorld,
      args: [source],
    }]);
    const first = Array.isArray(results) ? results[0] : results;
    return first?.result?.value ?? first?.result ?? first;
  }

  async executeJsCdp(tabId, source) {
    const debuggerApi = this.chrome?.debugger;
    if (!debuggerApi || typeof debuggerApi.attach !== "function" || typeof debuggerApi.sendCommand !== "function") {
      throw commandError("API_UNAVAILABLE", "debugger CDP is unavailable for execute_js");
    }
    const target = { tabId };
    let attached = false;
    try {
      await callChrome(debuggerApi.attach.bind(debuggerApi), [target, "1.3"]);
      attached = true;
      await callChrome(debuggerApi.sendCommand.bind(debuggerApi), [target, "Page.enable", {}]);
      const frameTree = await callChrome(debuggerApi.sendCommand.bind(debuggerApi), [target, "Page.getFrameTree", {}]);
      const frameId = frameTree?.frameTree?.frame?.id;
      if (typeof frameId !== "string" || frameId.length === 0) {
        throw commandError("EXECUTE_JS_FAILED", "page main frame is unavailable");
      }
      const isolated = await callChrome(debuggerApi.sendCommand.bind(debuggerApi), [
        target,
        "Page.createIsolatedWorld",
        { frameId, worldName: "omicsops-controlled", grantUniveralAccess: false },
      ]);
      if (!Number.isInteger(isolated?.executionContextId)) {
        throw commandError("EXECUTE_JS_FAILED", "isolated execution context is unavailable");
      }
      const response = await callChrome(debuggerApi.sendCommand.bind(debuggerApi), [
        target,
        "Runtime.evaluate",
        {
          expression: `(async () => {\n${source}\n})()`,
          contextId: isolated.executionContextId,
          awaitPromise: true,
          returnByValue: true,
          userGesture: false,
          objectGroup: "omicsops",
        },
      ]);
      if (response?.exceptionDetails) throw commandError("EXECUTE_JS_FAILED", "page script raised an exception");
      return undefined;
    } finally {
      if (attached && typeof debuggerApi.detach === "function") {
        try {
          await callChrome(debuggerApi.detach.bind(debuggerApi), [target]);
        } catch {
          this.sendEvent({ type: "debugger_cleanup_failed", tab_id: tabId });
          throw commandError("DEBUGGER_CLEANUP_FAILED", "CDP debugger could not be detached safely");
        }
      }
    }
  }

  async screenshot(command) {
    const tab = await this.requireControlledTab(command, { adopt: command.payload.adopt === true });
    const capture = this.chrome?.tabs?.captureVisibleTab;
    if (typeof capture === "function" && tab.active === true) {
      const dataUrl = await callChrome(capture.bind(this.chrome.tabs), [tab.windowId, { format: "png" }]);
      if (typeof dataUrl !== "string" || !dataUrl.startsWith("data:image/png;base64,")) {
        throw commandError("SCREENSHOT_FAILED", "browser did not return a PNG data URL");
      }
      if (dataUrl.length > MAX_SCREENSHOT_BYTES) throw commandError("SCREENSHOT_TOO_LARGE", "screenshot exceeds the size limit");
      return { tab: responseTab(tab), data_url: dataUrl };
    }
    if (tab.active !== true && (!this.chrome?.debugger || typeof this.chrome.debugger.sendCommand !== "function")) {
      throw commandError("SCREENSHOT_TARGET_NOT_VISIBLE", "target tab is not active and CDP screenshot is unavailable");
    }
    const response = await this.executeCdpScreenshot(tab.id);
    return { tab: responseTab(tab), data_url: `data:image/png;base64,${response.data}` };
  }

  async executeCdpScreenshot(tabId) {
    const debuggerApi = this.chrome?.debugger;
    if (!debuggerApi || typeof debuggerApi.attach !== "function" || typeof debuggerApi.sendCommand !== "function") {
      throw commandError("API_UNAVAILABLE", "tabs.captureVisibleTab and debugger are unavailable");
    }
    const target = { tabId };
    let attached = false;
    try {
      await callChrome(debuggerApi.attach.bind(debuggerApi), [target, "1.3"]);
      attached = true;
      const response = await callChrome(debuggerApi.sendCommand.bind(debuggerApi), [target, "Page.captureScreenshot", {
        format: "png",
        fromSurface: true,
      }]);
      if (!response?.data || typeof response.data !== "string") throw commandError("SCREENSHOT_FAILED", "CDP did not return PNG bytes");
      if (response.data.length > MAX_SCREENSHOT_BYTES) throw commandError("SCREENSHOT_TOO_LARGE", "screenshot exceeds the size limit");
      return response;
    } finally {
      if (attached && typeof debuggerApi.detach === "function") {
        try {
          await callChrome(debuggerApi.detach.bind(debuggerApi), [target]);
        } catch {
          this.sendEvent({ type: "debugger_cleanup_failed", tab_id: tabId });
          throw commandError("DEBUGGER_CLEANUP_FAILED", "CDP debugger could not be detached safely");
        }
      }
    }
  }

  async saveAssets(command) {
    const payload = command.payload;
    let tabId;
    if (payload.tab_id !== undefined) {
      const tab = await this.requireControlledTab(command, { adopt: payload.adopt === true });
      tabId = tab.id;
    }
    const targetHost = normalizedHost(payload.target_host);
    if (!targetHost) throw commandError("TARGET_HOST_REQUIRED", "target_host is required for browser downloads");
    const assets = payload.assets;
    if (!Array.isArray(assets) || assets.length === 0) throw commandError("INVALID_PAYLOAD", "assets must be a non-empty array");
    const approvedSources = assets.map((asset) => {
      const sourceUrl = asset?.source_url ?? asset?.url;
      if (typeof sourceUrl !== "string") {
        throw commandError("INVALID_PAYLOAD", "source_url is required for deterministic browser staging");
      }
      const url = assertAllowedUrl(sourceUrl);
      if (new URL(url).hostname.toLowerCase() !== targetHost) {
        throw commandError("TARGET_HOST_MISMATCH", "asset source_url does not match the approved target_host");
      }
      assertDomainAllowed(url, payload);
      return url;
    });
    const staged = this.stager.stage({
      sessionId: this.ledger.sessionId,
      turnId: command.runId,
      tabId,
      assets: assets.map((asset) => ({
        // Rust later consumes staged_id and relative_path.  The extension only
        // stages/records metadata; it never copies bytes into project storage.
        relative_path: asset.relative_path,
        url: asset.url ?? asset.source_url,
        filename: asset.filename,
        mime_type: asset.mime_type,
        bytes: asset.bytes,
        sha256: asset.sha256,
        title: asset.title,
      })),
    });
    const completed = [];
    for (let index = 0; index < staged.length; index += 1) {
      const record = staged[index];
      const url = approvedSources[index];
      const download = this.chrome?.downloads?.download;
      const search = this.chrome?.downloads?.search;
      if (typeof download !== "function" || typeof search !== "function") {
        throw commandError("API_UNAVAILABLE", "downloads API is unavailable");
      }
      const downloadId = await callChrome(download.bind(this.chrome.downloads), [{
        url,
        filename: `OmicsOps-Staging/${record.staged_id}`,
        conflictAction: "uniquify",
        saveAs: false,
      }]);
      if (!Number.isInteger(downloadId)) throw commandError("DOWNLOAD_FAILED", "browser did not return a download ID");
      const item = await this.waitForStagedDownload(downloadId, record.staged_id);
      completed.push({
        ...record,
        state: "download_complete",
        download_id: downloadId,
        size_bytes: Number.isSafeInteger(item.fileSize) ? item.fileSize : record.size_bytes,
      });
    }
    return { assets: completed };
  }

  async waitForStagedDownload(downloadId, stagedId) {
    const search = this.chrome.downloads.search.bind(this.chrome.downloads);
    for (let attempt = 0; attempt < DOWNLOAD_WAIT_ATTEMPTS; attempt += 1) {
      const matches = await callChrome(search, [{ id: downloadId }]);
      const item = Array.isArray(matches) ? matches[0] : undefined;
      if (item?.state === "interrupted") throw commandError("DOWNLOAD_FAILED", "browser download was interrupted");
      if (item?.state === "complete") {
        const normalized = String(item.filename || "").replaceAll("\\", "/");
        if (!normalized.endsWith(`/OmicsOps-Staging/${stagedId}`)) {
          throw commandError("DOWNLOAD_PATH_MISMATCH", "browser did not stage the download at the pinned path");
        }
        return item;
      }
      await this.wait(DOWNLOAD_WAIT_MS);
    }
    throw commandError("DOWNLOAD_TIMEOUT", "browser download did not finish before the staging deadline");
  }

  async closeRunTabs(command) {
    const remove = this.chrome?.tabs?.remove;
    if (typeof remove !== "function") throw commandError("API_UNAVAILABLE", "tabs.remove is unavailable");
    const explicit = command.payload.tab_targets;
    let targets;
    if (explicit === undefined) {
      targets = this.ledger.tabIds(command.runId, { onlyCreated: true })
        .map((tabId) => ({ tabId, origin: null, ledgerRequired: true }));
    } else {
      if (!Array.isArray(explicit) || explicit.length > 256) {
        throw commandError("INVALID_PAYLOAD", "tab_targets must be an array within the size limit");
      }
      targets = explicit.map((target) => {
        const tabId = target?.tab_id;
        if (!Number.isInteger(tabId) || tabId < 0) {
          throw commandError("INVALID_PAYLOAD", "tab target has an invalid tab ID");
        }
        const allowed = assertAllowedUrl(target?.origin);
        const origin = new URL(allowed).origin;
        if (allowed !== `${origin}/` && allowed !== origin) {
          throw commandError("INVALID_PAYLOAD", "tab target must contain an origin only");
        }
        return { tabId, origin, ledgerRequired: false };
      });
    }
    const closed = [];
    const alreadyClosed = [];
    const failures = [];
    for (const target of targets) {
      const tabId = target.tabId;
      try {
        if (target.ledgerRequired) this.ledger.assertControlled(command.runId, tabId);
        let tab;
        try {
          tab = await this.getTab(tabId);
        } catch (error) {
          if (error?.code === "TAB_NOT_FOUND") {
            alreadyClosed.push(tabId);
            continue;
          }
          throw error;
        }
        if (!target.ledgerRequired && !this.ledger.has(command.runId, tabId)) {
          throw commandError("TAB_NOT_CONTROLLED", "persisted session ledger does not prove ownership of this tab");
        }
        if (target.origin !== null && new URL(tab.url).origin !== target.origin) {
          throw commandError("TAB_ORIGIN_MISMATCH", "recorded tab ID now belongs to another origin");
        }
        if (command.payload.target_host !== undefined) {
          assertTargetHost(command.payload, tab.url);
        }
        await callChrome(remove.bind(this.chrome.tabs), [tabId]);
        if (this.ledger.has(command.runId, tabId)) this.ledger.markClosed(command.runId, tabId);
        closed.push(tabId);
      } catch (error) {
        failures.push({ tab_id: tabId, error: error instanceof Error ? error.message : "tab close failed" });
      }
    }
    return { closed_tab_ids: closed, already_closed_tab_ids: alreadyClosed, failures };
  }
}
