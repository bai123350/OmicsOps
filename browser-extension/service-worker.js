import { CommandRouter } from "./command-router.js";
import { TabLedger } from "./tab-ledger.js";
import { AssetStager } from "./asset-stager.js";
import { BRIDGE_ENDPOINTS, SESSION_NAMES } from "./protocol.js";
import { OmicsOpsWebSocketClient } from "./websocket-client.js";
import { isAllowedUrl, sanitizeUrlForReport } from "./url-policy.js";

/**
 * One extension instance owns exactly one explicitly selected local session.
 * Shared and workspace browser profiles therefore never share a socket,
 * ledger, or staged metadata.
 */
export class OmicsOpsBrowserBridge {
  constructor({
    session,
    chromeApi = globalThis.chrome,
    WebSocketImpl = globalThis.WebSocket,
    reconnect = true,
    onStatus = () => {},
    initialLedgerState = undefined,
    persistLedger = () => {},
  } = {}) {
    if (!SESSION_NAMES.includes(session)) throw new TypeError("session must be shared or workspace");
    this.chrome = chromeApi;
    this.session = session;
    this.onStatus = onStatus;
    this.routers = {};
    this.clients = {};
    this.humanNotices = [];
    for (const sessionName of [session]) {
      const ledger = new TabLedger({
        sessionId: sessionName,
        initialState: initialLedgerState,
        onChange: (state) => persistLedger(sessionName, state),
      });
      const router = new CommandRouter({
        chromeApi,
        ledger,
        stager: new AssetStager(),
        sendEvent: (event) => this.recordEvent(sessionName, event),
      });
      this.routers[sessionName] = router;
      this.clients[sessionName] = new OmicsOpsWebSocketClient({
        session: sessionName,
        endpoint: BRIDGE_ENDPOINTS[sessionName],
        WebSocketImpl,
        reconnect,
        onCommand: async (command) => {
          const result = await router.execute(command);
          this.publishTabState(sessionName);
          return result;
        },
        getTabSummaries: () => router.ledger.tabSummaries(),
        onStatus: (status) => this.onStatus(status),
      });
    }
  }

  async start() {
    return this.clients[this.session].connect();
  }

  stop() {
    this.clients[this.session].stop();
  }

  recordEvent(session, event) {
    if (event?.type === "human_required") {
      this.humanNotices.push({ session, ...event, observed_at: Date.now() });
      if (this.humanNotices.length > 100) this.humanNotices.shift();
    }
    try { this.onStatus({ session, event }); } catch { /* status observers are non-critical */ }
  }

  handleRuntimeMessage(message, sender, sendResponse) {
    if (!message || message.type !== "omicsops.human_required") return false;
    if (sender?.id && this.chrome?.runtime?.id && sender.id !== this.chrome.runtime.id) return false;
    const notice = message.notice && typeof message.notice === "object" ? message.notice : {};
    this.recordEvent("unknown", {
      type: "human_required",
      reason: "captcha_detected",
      url: isAllowedUrl(notice.url) ? sanitizeUrlForReport(notice.url) : undefined,
      signals: Array.isArray(notice.signals) ? notice.signals.slice(0, 20) : [],
    });
    if (typeof sendResponse === "function") sendResponse({ ok: true, paused: true });
    return true;
  }

  handleTabUpdated(tabId, changeInfo, tab) {
    if (!changeInfo?.url && !changeInfo?.title && changeInfo?.windowId === undefined) return;
    for (const session of [this.session]) {
      const ledger = this.routers[session].ledger;
      if (!ledger.hasAnyTab(tabId)) continue;
      if (changeInfo.url) {
        if (isAllowedUrl(changeInfo.url)) ledger.observeNavigation(tabId, changeInfo.url, tab?.windowId);
        else ledger.observeBlockedNavigation(tabId, changeInfo.url);
      }
      ledger.observeTab(tabId, {
        url: tab?.url && isAllowedUrl(tab.url) ? tab.url : undefined,
        title: tab?.title,
        windowId: tab?.windowId,
      });
      this.publishTabState(session);
    }
  }

  handleTabRemoved(tabId) {
    for (const session of [this.session]) {
      this.routers[session].ledger.forgetTab(tabId);
      this.publishTabState(session);
    }
  }

  publishTabState(session) {
    this.clients[session]?.sendTabState();
  }
}

export function installServiceWorker(chromeApi = globalThis.chrome, WebSocketImpl = globalThis.WebSocket) {
  if (!chromeApi) return null;
  const controller = {
    bridge: null,
    async selectSession(session, persist = true) {
      if (!SESSION_NAMES.includes(session)) throw new TypeError("session must be shared or workspace");
      if (this.bridge?.session !== session) {
        this.bridge?.stop();
        const storageKey = `omicsops_tab_ledger_${session}`;
        let initialLedgerState;
        if (chromeApi.storage?.session?.get) {
          const stored = await chromeApi.storage.session.get(storageKey);
          initialLedgerState = stored?.[storageKey];
        }
        try {
          this.bridge = new OmicsOpsBrowserBridge({
            session,
            chromeApi,
            WebSocketImpl,
            initialLedgerState,
            persistLedger: (_sessionName, state) => {
              if (chromeApi.storage?.session?.set) void chromeApi.storage.session.set({ [storageKey]: state });
            },
          });
        } catch {
          // Corrupt/stale session metadata can never expand authority. Drop it
          // and reconnect with an empty ledger, which makes cleanup fail closed.
          if (chromeApi.storage?.session?.remove) await chromeApi.storage.session.remove(storageKey);
          this.bridge = new OmicsOpsBrowserBridge({
            session,
            chromeApi,
            WebSocketImpl,
            persistLedger: (_sessionName, state) => {
              if (chromeApi.storage?.session?.set) void chromeApi.storage.session.set({ [storageKey]: state });
            },
          });
        }
      }
      if (persist && chromeApi.storage?.local?.set) {
        await chromeApi.storage.local.set({ omicsops_browser_session: session });
      }
      return this.bridge.start();
    },
    async restoreSession() {
      if (!chromeApi.storage?.local?.get) return false;
      const stored = await chromeApi.storage.local.get("omicsops_browser_session");
      const session = stored?.omicsops_browser_session;
      return SESSION_NAMES.includes(session) ? this.selectSession(session, false) : false;
    },
  };
  chromeApi.runtime?.onStartup?.addListener(() => { void controller.restoreSession(); });
  chromeApi.runtime?.onMessage?.addListener((message, sender, sendResponse) => {
    if (message?.type === "omicsops.select_session") {
      if (sender?.id && chromeApi.runtime?.id && sender.id !== chromeApi.runtime.id) return false;
      void controller.selectSession(message.session).then(
        (connected) => sendResponse?.({ ok: true, connected, session: message.session }),
        (error) => sendResponse?.({ ok: false, error: String(error?.message || error) }),
      );
      return true;
    }
    return controller.bridge?.handleRuntimeMessage(message, sender, sendResponse) ?? false;
  });
  chromeApi.action?.onClicked?.addListener(() => { void controller.selectSession("shared"); });
  chromeApi.tabs?.onUpdated?.addListener((tabId, changeInfo, tab) => controller.bridge?.handleTabUpdated(tabId, changeInfo, tab));
  chromeApi.tabs?.onRemoved?.addListener((tabId) => controller.bridge?.handleTabRemoved(tabId));
  // Restore only an explicitly selected session. A fresh isolated workspace
  // profile waits for session.html, so it can never race onto the shared port.
  void controller.restoreSession();
  return controller;
}

let installedBridge = null;
if (globalThis.chrome?.runtime) installedBridge = installServiceWorker(globalThis.chrome, globalThis.WebSocket);

export { installedBridge };
