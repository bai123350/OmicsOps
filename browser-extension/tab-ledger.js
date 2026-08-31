import { originForReport, sanitizeUrlForReport } from "./url-policy.js";

const MAX_TURNS = 64;
const MAX_TABS_PER_TURN = 256;

function nowValue(clock) {
  return typeof clock === "function" ? clock() : Date.now();
}

function asTabId(value) {
  if (!Number.isInteger(value) || value < 0) {
    throw new TypeError("tab_id must be a non-negative integer");
  }
  return value;
}

function asTurnId(value) {
  if (typeof value !== "string" || value.length === 0 || value.length > 128) {
    throw new TypeError("turn_id must be a non-empty string");
  }
  return value;
}

/**
 * In-memory, per-session accounting.  It intentionally stores only a
 * redacted URL and operation metadata; page contents and credentials never
 * enter the ledger.
 */
export class TabLedger {
  constructor({ sessionId = null, clock = Date.now, onChange = () => {}, initialState = undefined } = {}) {
    this.sessionId = sessionId;
    this.clock = clock;
    this.onChange = onChange;
    this.turns = new Map();
    if (initialState !== undefined) this.restoreState(initialState);
  }

  changed() {
    try { this.onChange(this.exportState()); } catch { /* persistence is best effort */ }
  }

  setSession(sessionId) {
    this.sessionId = sessionId;
    this.turns.clear();
    this.changed();
  }

  clear() {
    this.turns.clear();
    this.changed();
  }

  beginTurn(turnId) {
    const normalizedTurnId = asTurnId(turnId);
    if (!this.turns.has(normalizedTurnId)) {
      if (this.turns.size >= MAX_TURNS) {
        const oldest = this.turns.keys().next().value;
        this.turns.delete(oldest);
      }
      this.turns.set(normalizedTurnId, {
        turnId: normalizedTurnId,
        openedAt: nowValue(this.clock),
        tabs: new Map(),
      });
      this.changed();
    }
    return this.turns.get(normalizedTurnId);
  }

  touch(turnId, tabId, {
    url = undefined,
    created = false,
    reason = "command",
    windowId = undefined,
    title = undefined,
  } = {}) {
    const turn = this.beginTurn(turnId);
    const normalizedTabId = asTabId(tabId);
    if (!turn.tabs.has(normalizedTabId) && turn.tabs.size >= MAX_TABS_PER_TURN) {
      throw new Error("tab accounting limit exceeded");
    }
    const current = turn.tabs.get(normalizedTabId);
    const timestamp = nowValue(this.clock);
    const next = current ?? {
      tabId: normalizedTabId,
      firstSeenAt: timestamp,
      createdByExtension: false,
      touched: 0,
      closedAt: null,
      reasons: [],
    };
    next.lastSeenAt = timestamp;
    next.touched += 1;
    next.createdByExtension ||= Boolean(created);
    if (windowId !== undefined && Number.isInteger(windowId)) {
      next.windowId = windowId;
    }
    if (typeof url === "string" && url.length > 0) {
      next.url = sanitizeUrlForReport(url);
    }
    if (typeof title === "string" && title.length > 0) {
      next.title = title.slice(0, 500);
    }
    if (typeof reason === "string" && reason.length > 0 && !next.reasons.includes(reason)) {
      next.reasons.push(reason);
    }
    turn.tabs.set(normalizedTabId, next);
    this.changed();
    return next;
  }

  get(turnId, tabId) {
    const turn = this.turns.get(turnId);
    return turn?.tabs.get(tabId) ?? null;
  }

  has(turnId, tabId) {
    return Boolean(this.get(turnId, tabId));
  }

  hasAnyTab(tabId) {
    return [...this.turns.values()].some((turn) => turn.tabs.has(tabId));
  }

  observeNavigation(tabId, url, windowId = undefined) {
    const normalizedTabId = asTabId(tabId);
    for (const turn of this.turns.values()) {
      const record = turn.tabs.get(normalizedTabId);
      if (!record || record.closedAt !== null) continue;
      this.touch(turn.turnId, normalizedTabId, {
        url,
        windowId,
        reason: "navigation",
      });
      record.blocked = false;
    }
  }

  observeTab(tabId, { url = undefined, title = undefined, windowId = undefined } = {}) {
    const normalizedTabId = asTabId(tabId);
    for (const turn of this.turns.values()) {
      const record = turn.tabs.get(normalizedTabId);
      if (!record || record.closedAt !== null) continue;
      this.touch(turn.turnId, normalizedTabId, { url, title, windowId, reason: "tab_state" });
    }
  }

  observeBlockedNavigation(tabId, url) {
    const normalizedTabId = asTabId(tabId);
    for (const turn of this.turns.values()) {
      const record = turn.tabs.get(normalizedTabId);
      if (!record || record.closedAt !== null) continue;
      record.blocked = true;
      record.lastSeenAt = nowValue(this.clock);
      record.reasons = [...new Set([...record.reasons, "blocked_navigation"])]
        .slice(0, 16);
      // Keep the previous safe URL in the report; never retain a chrome://,
      // file://, javascript:, or data: URL supplied by a page navigation.
      void url;
      this.changed();
    }
  }

  assertControlled(turnId, tabId) {
    const record = this.get(asTurnId(turnId), asTabId(tabId));
    if (!record || record.closedAt !== null) {
      const error = new Error("tab is not controlled in this turn");
      error.code = "TAB_NOT_CONTROLLED";
      throw error;
    }
    return record;
  }

  markClosed(turnId, tabId) {
    const record = this.assertControlled(turnId, tabId);
    record.closedAt = nowValue(this.clock);
    this.changed();
    return record;
  }

  forgetTab(tabId) {
    const normalizedTabId = asTabId(tabId);
    for (const turn of this.turns.values()) {
      const record = turn.tabs.get(normalizedTabId);
      if (record && record.closedAt === null) {
        record.closedAt = nowValue(this.clock);
      }
    }
    this.changed();
  }

  markBlocked(turnId, tabId, url) {
    const record = this.touch(turnId, tabId, { url, reason: "blocked_navigation" });
    record.blocked = true;
    this.changed();
    return record;
  }

  tabIds(turnId, { includeClosed = false, onlyCreated = false } = {}) {
    const turn = this.turns.get(turnId);
    if (!turn) return [];
    return [...turn.tabs.values()]
      .filter((record) => (includeClosed || record.closedAt === null) && (!onlyCreated || record.createdByExtension))
      .map((record) => record.tabId);
  }

  snapshot(turnId) {
    const normalizedTurnId = asTurnId(turnId);
    const turn = this.beginTurn(normalizedTurnId);
    const tabs = [...turn.tabs.values()].map((record) => ({
      tab_id: record.tabId,
      window_id: record.windowId,
      title: record.title,
      url: record.url,
      created_by_extension: record.createdByExtension,
      first_seen_at: record.firstSeenAt,
      last_seen_at: record.lastSeenAt,
      touched: record.touched,
      closed_at: record.closedAt,
      blocked: Boolean(record.blocked),
      reasons: [...record.reasons],
    }));
    return {
      session_id: this.sessionId,
      turn_id: normalizedTurnId,
      tabs,
      created_tab_ids: tabs.filter((tab) => tab.created_by_extension).map((tab) => tab.tab_id),
      touched_tab_ids: tabs.map((tab) => tab.tab_id),
      closed_tab_ids: tabs.filter((tab) => tab.closed_at !== null).map((tab) => tab.tab_id),
    };
  }

  tabSummaries() {
    const summaries = [];
    for (const turn of this.turns.values()) {
      for (const record of turn.tabs.values()) {
        if (record.closedAt !== null) continue;
        summaries.push({
          tab_id: record.tabId,
          run_id: turn.turnId,
          title: (record.title || "").slice(0, 200),
          origin: originForReport(record.url),
          created_by_run: Boolean(record.createdByExtension),
        });
      }
    }
    return summaries;
  }

  /**
   * Minimal ownership state for chrome.storage.session. It intentionally
   * excludes URL paths/query, titles, page text, and credentials.
   */
  exportState() {
    return {
      schema_version: 1,
      session_id: this.sessionId,
      turns: [...this.turns.values()].map((turn) => ({
        run_id: turn.turnId,
        tabs: [...turn.tabs.values()]
          .filter((record) => record.closedAt === null && record.url)
          .map((record) => ({
            tab_id: record.tabId,
            origin: originForReport(record.url),
            created_by_run: Boolean(record.createdByExtension),
          })),
      })).filter((turn) => turn.tabs.length > 0),
    };
  }

  restoreState(state) {
    if (!state || state.schema_version !== 1 || state.session_id !== this.sessionId || !Array.isArray(state.turns) || state.turns.length > MAX_TURNS) {
      throw new TypeError("persisted tab ledger is invalid");
    }
    const restored = new Map();
    for (const turn of state.turns) {
      const runId = asTurnId(turn?.run_id);
      if (!Array.isArray(turn.tabs) || turn.tabs.length > MAX_TABS_PER_TURN) throw new TypeError("persisted tab ledger is invalid");
      const tabs = new Map();
      for (const tab of turn.tabs) {
        const tabId = asTabId(tab?.tab_id);
        if (typeof tab.origin !== "string" || originForReport(tab.origin) !== tab.origin || typeof tab.created_by_run !== "boolean") {
          throw new TypeError("persisted tab ledger is invalid");
        }
        tabs.set(tabId, {
          tabId,
          url: tab.origin,
          createdByExtension: tab.created_by_run,
          firstSeenAt: nowValue(this.clock),
          lastSeenAt: nowValue(this.clock),
          touched: 0,
          closedAt: null,
          reasons: ["restored_session_ledger"],
        });
      }
      restored.set(runId, { turnId: runId, openedAt: nowValue(this.clock), tabs });
    }
    this.turns = restored;
    return true;
  }
}

export function tabSummary(record) {
  if (!record) return null;
  return {
    tab_id: record.tabId,
    window_id: record.windowId,
    url: record.url,
    created_by_extension: record.createdByExtension,
    closed_at: record.closedAt,
  };
}
