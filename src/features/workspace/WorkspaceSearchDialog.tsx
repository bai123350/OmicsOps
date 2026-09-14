import { useEffect, useLayoutEffect, useMemo, useRef, useState, type KeyboardEvent as ReactKeyboardEvent } from "react";
import { ArrowUpRight, FileText, FolderKanban, LoaderCircle, MessageSquare, Paperclip, RefreshCw, Search, Sparkles, X, Zap } from "lucide-react";

import { filterWorkspaceSearchEntries, type WorkspaceSearchEntry } from "../../workspace-search";
import { useWindowEscapeLayer } from "../settings/BrowserSettings";
import "./workspace-search.css";

export type WorkspaceSearchDialogProps = {
  entries: WorkspaceSearchEntry[];
  zh: boolean;
  loading?: boolean;
  failedProjects?: number;
  error?: string;
  busy?: boolean;
  onRetry: () => void;
  onClose: () => void;
  onOpen: (entry: WorkspaceSearchEntry) => Promise<boolean | void> | boolean | void;
  onAttach?: (entry: WorkspaceSearchEntry) => Promise<boolean | void> | boolean | void;
  canAttach?: (entry: WorkspaceSearchEntry) => boolean;
};

const RESULT_KINDS: WorkspaceSearchEntry["kind"][] = ["project", "session", "artifact", "skill", "action"];

const KIND_ICONS = {
  project: FolderKanban,
  session: MessageSquare,
  artifact: FileText,
  skill: Sparkles,
  action: Zap,
} as const;

function kindLabel(kind: WorkspaceSearchEntry["kind"], zh: boolean): string {
  const labels = zh
    ? { project: "项目", session: "会话", artifact: "产物", skill: "技能", action: "操作" }
    : { project: "Projects", session: "Sessions", artifact: "Artifacts", skill: "Skills", action: "Actions" };
  return labels[kind];
}

function isImeEvent(event: ReactKeyboardEvent<HTMLInputElement>): boolean {
  const nativeEvent = event.nativeEvent as KeyboardEvent;
  return nativeEvent.isComposing || nativeEvent.keyCode === 229 || event.keyCode === 229;
}

function isNativeImeEvent(event: KeyboardEvent): boolean {
  return event.isComposing || event.keyCode === 229;
}

function genericActionError(zh: boolean): string {
  return zh ? "操作失败，请重试。" : "Action failed. Please try again.";
}

function actionLabel(action: "open" | "attach", entry: WorkspaceSearchEntry, zh: boolean): string {
  if (action === "attach") return zh ? `附加 ${entry.label}` : `Attach ${entry.label}`;
  return zh ? `打开 ${entry.label}` : `Open ${entry.label}`;
}

export function WorkspaceSearchDialog({
  entries,
  zh,
  loading = false,
  failedProjects = 0,
  error,
  busy = false,
  onRetry,
  onClose,
  onOpen,
  onAttach,
  canAttach,
}: WorkspaceSearchDialogProps) {
  const [query, setQuery] = useState("");
  const [activeIndex, setActiveIndex] = useState(0);
  const [pendingKey, setPendingKey] = useState<string | null>(null);
  const [localError, setLocalError] = useState("");
  const searchInputRef = useRef<HTMLInputElement>(null);
  const resultRefs = useRef<Array<HTMLDivElement | null>>([]);
  const pendingRef = useRef<string | null>(null);
  const mountedRef = useRef(false);
  const dialogRef = useRef<HTMLElement>(null);
  const previousFocusRef = useRef<HTMLElement | null>(null);
  const onCloseRef = useRef(onClose);
  const onOpenRef = useRef(onOpen);
  const onAttachRef = useRef(onAttach);

  onCloseRef.current = onClose;
  onOpenRef.current = onOpen;
  onAttachRef.current = onAttach;

  if (previousFocusRef.current === null && typeof document !== "undefined") {
    const active = document.activeElement;
    previousFocusRef.current = active instanceof HTMLElement ? active : null;
  }

  useLayoutEffect(() => {
    mountedRef.current = true;
    searchInputRef.current?.focus();

    return () => {
      mountedRef.current = false;
      const previous = previousFocusRef.current;
      if (previous?.isConnected) previous.focus();
    };
  }, []);

  useWindowEscapeLayer(true, () => {
    if (mountedRef.current) onCloseRef.current();
  });

  useEffect(() => {
    const stopComposingEscape = (event: KeyboardEvent) => {
      if (event.key !== "Escape" || !isNativeImeEvent(event)) return;
      if (!(event.target instanceof HTMLElement) || !dialogRef.current?.contains(event.target)) return;
      event.preventDefault();
      event.stopImmediatePropagation();
    };
    window.addEventListener("keydown", stopComposingEscape, true);
    return () => window.removeEventListener("keydown", stopComposingEscape, true);
  }, []);

  const matchingEntries = useMemo(() => filterWorkspaceSearchEntries(entries, query), [entries, query]);
  const groupedEntries = useMemo(
    () => RESULT_KINDS.map((kind) => ({ kind, entries: matchingEntries.filter((entry) => entry.kind === kind) })).filter((group) => group.entries.length > 0),
    [matchingEntries],
  );
  const visibleEntries = useMemo(() => groupedEntries.flatMap((group) => group.entries), [groupedEntries]);
  const matchingSignature = useMemo(
    () => visibleEntries.map((entry) => `${entry.key}\u0000${entry.label}\u0000${entry.description}`).join("\u0001"),
    [visibleEntries],
  );
  const selectedIndex = visibleEntries.length === 0 ? -1 : Math.min(activeIndex, visibleEntries.length - 1);
  const actionPending = pendingKey !== null;
  const actionDisabled = busy || actionPending;
  const dialogLabel = zh ? "搜索工作区" : "Search workspace";
  const searchLabel = zh ? "搜索工作区内容" : "Search workspace content";
  const displayedError = localError;

  useEffect(() => {
    if (selectedIndex < 0) return;
    resultRefs.current[selectedIndex]?.scrollIntoView?.({ block: "nearest" });
  }, [selectedIndex, matchingSignature]);

  useEffect(() => {
    setActiveIndex(0);
  }, [query, matchingSignature]);

  function canAttachEntry(entry: WorkspaceSearchEntry): boolean {
    if (!onAttach || !canAttach) return false;
    try {
      return canAttach(entry);
    } catch {
      return false;
    }
  }

  async function runAction(entry: WorkspaceSearchEntry, action: "open" | "attach") {
    if (busy || pendingRef.current !== null) return;
    const callback = action === "attach" ? onAttachRef.current : onOpenRef.current;
    if (!callback) return;

    const key = `${action}:${entry.key}`;
    pendingRef.current = key;
    setPendingKey(key);
    setLocalError("");

    try {
      const accepted = await callback(entry);
      if (!mountedRef.current) return;
      if (accepted !== false) onCloseRef.current();
    } catch {
      if (mountedRef.current) setLocalError(genericActionError(zh));
    } finally {
      if (mountedRef.current) {
        pendingRef.current = null;
        setPendingKey(null);
      }
    }
  }

  function handleSearchKeyDown(event: ReactKeyboardEvent<HTMLInputElement>) {
    if (isImeEvent(event)) return;
    if (event.key === "ArrowDown" && visibleEntries.length > 0) {
      event.preventDefault();
      setActiveIndex((index) => (index + 1) % visibleEntries.length);
      return;
    }
    if (event.key === "ArrowUp" && visibleEntries.length > 0) {
      event.preventDefault();
      setActiveIndex((index) => (index - 1 + visibleEntries.length) % visibleEntries.length);
      return;
    }
    if (event.key === "Enter" && selectedIndex >= 0 && !actionDisabled) {
      event.preventDefault();
      void runAction(visibleEntries[selectedIndex], "open");
    }
  }

  return (
    <div className="workspace-search-backdrop">
      <section ref={dialogRef} className="workspace-search-dialog" role="dialog" aria-modal="true" aria-label={dialogLabel} aria-busy={busy || undefined}>
        <header className="workspace-search-header">
          <div>
            <span className="workspace-search-kicker">OmicsOps</span>
            <h2>{dialogLabel}</h2>
            <p>{zh ? "查找项目、会话、产物、技能和工作区操作" : "Find projects, sessions, artifacts, skills, and workspace actions"}</p>
          </div>
          <button type="button" className="workspace-search-close" aria-label={zh ? "关闭搜索" : "Close search"} onClick={onClose}>
            <X size={18} />
          </button>
        </header>

        <div className="workspace-search-body">
          <label className="workspace-search-input-wrap" htmlFor="workspace-search-input">
            <Search size={17} aria-hidden="true" />
            <input
              ref={searchInputRef}
              id="workspace-search-input"
              type="search"
              role="searchbox"
              aria-label={searchLabel}
              aria-controls="workspace-search-results"
              placeholder={zh ? "搜索项目、产物或操作…" : "Search projects, artifacts, or actions…"}
              autoComplete="off"
              spellCheck={false}
              value={query}
              disabled={busy}
              onChange={(event) => { setQuery(event.target.value); setLocalError(""); }}
              onKeyDown={handleSearchKeyDown}
            />
            <kbd>Esc</kbd>
          </label>

          {loading && <div className="workspace-search-status" role="status"><LoaderCircle size={15} className="workspace-search-spin" />{zh ? "正在加载工作区结果…" : "Loading workspace results…"}</div>}

          {(failedProjects > 0 || error) && (
            <div className="workspace-search-error" role="alert">
              <span>{error ?? (zh ? "部分项目结果无法加载。" : "Some project results could not be loaded.")}</span>
              {failedProjects > 0 && <button type="button" onClick={onRetry} disabled={busy}><RefreshCw size={13} />{zh ? "重试" : "Retry"}</button>}
            </div>
          )}

          <div id="workspace-search-results" className="workspace-search-results" role="listbox" aria-label={zh ? "工作区搜索结果" : "Workspace search results"} aria-activedescendant={selectedIndex >= 0 ? `workspace-search-result-${selectedIndex}` : undefined}>
            {groupedEntries.map(({ kind, entries: groupEntries }) => {
              const Icon = KIND_ICONS[kind];
              const headingId = `workspace-search-heading-${kind}`;
              return (
                <section className="workspace-search-group" key={kind} aria-labelledby={headingId}>
                  <h3 id={headingId}>{kindLabel(kind, zh)}</h3>
                  {groupEntries.map((entry) => {
                    const index = visibleEntries.indexOf(entry);
                    const selected = index === selectedIndex;
                    const attachable = canAttachEntry(entry);
                    return (
                      <div
                        className={`workspace-search-result${selected ? " is-selected" : ""}`}
                        key={entry.key}
                        id={`workspace-search-result-${index}`}
                        role="option"
                        aria-selected={selected}
                        ref={(node) => { resultRefs.current[index] = node; }}
                      >
                        <span className="workspace-search-result-icon"><Icon size={16} aria-hidden="true" /></span>
                        <button type="button" className="workspace-search-result-open" aria-label={actionLabel("open", entry, zh)} onClick={() => void runAction(entry, "open")} disabled={actionDisabled}>
                          <strong>{entry.label}</strong>
                          <small>{entry.description || (zh ? "无描述" : "No description")}</small>
                          <ArrowUpRight size={15} aria-hidden="true" />
                        </button>
                        <div className="workspace-search-result-actions">
                          {attachable && <button type="button" className="workspace-search-attach" aria-label={actionLabel("attach", entry, zh)} title={actionLabel("attach", entry, zh)} onClick={() => void runAction(entry, "attach")} disabled={actionDisabled}>
                            <Paperclip size={14} aria-hidden="true" />
                          </button>}
                        </div>
                      </div>
                    );
                  })}
                </section>
              );
            })}
            {visibleEntries.length === 0 && !loading && <p className="workspace-search-empty">{zh ? "没有匹配的工作区结果。" : "No matching workspace results."}</p>}
          </div>
        </div>

        <footer className="workspace-search-footer">
          {displayedError ? <span className="workspace-search-local-error" role="alert">{displayedError}</span> : <span>{zh ? "使用 ↑ ↓ 选择，Enter 打开，Esc 关闭" : "Use ↑ ↓ to select, Enter to open, Esc to close"}</span>}
          {actionPending && <span className="workspace-search-pending" role="status">{zh ? "处理中…" : "Working…"}</span>}
        </footer>
      </section>
    </div>
  );
}
