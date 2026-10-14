import { useEffect, useRef, useState } from "react";
import { listConversationVisibility } from "../../workspace-navigation-api";
import type { ConversationVisibilityErrorCodeV1, ConversationVisibilityListItemV1, ConversationVisibilityPageV1, ConversationVisibilityV1 } from "../../workspace-navigation-types";
import { ResearchDialog, ResearchError } from "./ResearchPagePrimitives";
import type { Locale } from "./copy";
import "./workspace-research-pages.css";
import "./shelved-conversations.css";

interface Props {
  projectId: string;
  locale: Locale;
  onClose: () => void;
  onOpenConversation: (id: string) => Promise<void> | void;
  onSetVisibility: (id: string, visibility: ConversationVisibilityV1, expectedRevision: number) => Promise<void>;
}
const errors: Record<ConversationVisibilityErrorCodeV1 | "unknown", [string, string]> = {
  invalid_input: ["查询无效，请检查输入。", "Invalid query. Check the input."],
  not_found: ["会话不可用。", "Conversation unavailable."],
  ownership_mismatch: ["会话身份不匹配。", "Conversation identity mismatch."],
  revision_conflict: ["会话可见性已变化，请刷新后重试。", "Conversation visibility changed. Refresh before retrying."],
  request_conflict: ["请求身份冲突，请刷新后重试。", "Request identity conflict. Refresh before retrying."],
  storage_failed: ["无法完成请求，请刷新当前状态。", "Could not complete the request. Refresh the current state."],
  unknown: ["无法完成请求，请刷新当前状态。", "Could not complete the request. Refresh the current state."],
};
function safeError(cause: unknown, zh: boolean): string {
  const code = cause && typeof cause === "object" && "code" in cause ? cause.code : null;
  const key = typeof code === "string" && Object.hasOwn(errors, code) ? code as ConversationVisibilityErrorCodeV1 : "unknown";
  return errors[key][zh ? 0 : 1];
}

export function ShelvedConversationsDialog(props: Props) {
  // Project switches discard all local query, page and in-flight operation state.
  return <ShelvedCollection key={props.projectId} {...props} />;
}
function ShelvedCollection({ projectId, locale, onClose, onOpenConversation, onSetVisibility }: Props) {
  const zh = locale === "zh-CN";
  const [selection, setSelection] = useState({ query: "", offset: 0, refresh: 0 });
  const scope = JSON.stringify([projectId, selection.query, selection.offset, selection.refresh]);
  const currentScope = useRef(scope); currentScope.current = scope;
  const generation = useRef(0);
  const closed = useRef(false);
  const operationSequence = useRef(0);
  const pending = useRef<{ scopeGeneration: number; operationId: number } | null>(null);
  const [page, setPage] = useState<ConversationVisibilityPageV1 | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState<string | null>(null);
  const validQuery = [...selection.query].length <= 200 && new TextEncoder().encode(selection.query).length <= 1024;
  useEffect(() => {
    const ticket = ++generation.current;
    pending.current = null;
    setPage(null); setError(""); setBusy(null); setLoading(validQuery);
    if (!validQuery) return;
    void listConversationVisibility({ project_id: projectId, visibility: "shelved", query: selection.query, offset: selection.offset, limit: 50 }).then((value) => {
      if (closed.current || ticket !== generation.current || scope !== currentScope.current) return;
      if (!value || !Array.isArray(value.items)) throw { code: "storage_failed" };
      if (value.items.some((item) => item.state?.project_id !== projectId || item.state.visibility !== "shelved" || !item.state.conversation_id || !Number.isSafeInteger(item.state.revision) || item.state.revision < 0)) throw { code: "ownership_mismatch" };
      if (value.next_offset !== null && (!Number.isInteger(value.next_offset) || value.next_offset <= selection.offset)) throw { code: "storage_failed" };
      setPage(value);
    }).catch((cause: unknown) => {
      if (!closed.current && ticket === generation.current && scope === currentScope.current) setError(safeError(cause, zh));
    }).finally(() => {
      if (!closed.current && ticket === generation.current && scope === currentScope.current) setLoading(false);
    });
    return () => { generation.current++; };
  }, [projectId, scope, validQuery, zh]);
  function refresh() { setSelection((value) => ({ ...value, offset: 0, refresh: value.refresh + 1 })); }
  function close() { closed.current = true; generation.current++; onClose(); }
  async function action(item: ConversationVisibilityListItemV1, restore: boolean) {
    if (closed.current || pending.current?.scopeGeneration === generation.current) return;
    const operation = { scopeGeneration: generation.current, operationId: ++operationSequence.current };
    pending.current = operation; setBusy(item.state.conversation_id); setError("");
    // Query/page values can recur. Both the monotonic generation and this
    // unique operation must still own the UI before any completion applies.
    const isCurrentOperation = () => !closed.current && operation.scopeGeneration === generation.current && pending.current?.operationId === operation.operationId && scope === currentScope.current;
    try {
      if (restore) await onSetVisibility(item.state.conversation_id, "visible", item.state.revision);
      else await onOpenConversation(item.state.conversation_id);
      // Do not apply historical receipts to rows; requery the first current page.
      if (restore && isCurrentOperation()) refresh();
    } catch (cause) {
      if (isCurrentOperation()) setError(safeError(cause, zh));
    } finally {
      if (isCurrentOperation()) { pending.current = null; setBusy(null); }
    }
  }
  const queryError = validQuery ? "" : zh ? "搜索最多 200 个字符 / 1 KiB。" : "Search is limited to 200 characters / 1 KiB.";
  const beyondLimit = page?.next_offset !== null && page?.next_offset !== undefined && page.next_offset > 10000;
  return <ResearchDialog title={zh ? "收起的会话" : "Shelved conversations"} zh={zh} onClose={close}>
    <div className="shelved-conversations">
      <p>{zh ? "打开会话不会恢复其可见性。关闭列表不会取消正在运行的计算。" : "Opening a conversation keeps it shelved. Closing this list does not cancel running computation."}</p>
      <label>{zh ? "搜索收起的会话" : "Search shelved conversations"}<input value={selection.query} onChange={(event) => setSelection((value) => ({ ...value, query: event.target.value, offset: 0 }))} /></label>
      <button type="button" onClick={refresh}>{zh ? "刷新" : "Refresh"}</button>
      <ResearchError error={queryError || error} zh={zh} />
      {loading && <p role="status">{zh ? "正在加载…" : "Loading…"}</p>}
      {!loading && !error && !queryError && page?.items.length === 0 && <p>{zh ? "暂无收起的会话。" : "No shelved conversations."}</p>}
      <ul>{page?.items.map((item) => <li key={item.state.conversation_id}>
        <div><strong>{item.title || (zh ? "未命名会话" : "Untitled conversation")}</strong><small>{item.conversation_status} · <time dateTime={item.conversation_updated_at}>{item.conversation_updated_at}</time></small><code>{item.state.conversation_id}</code></div>
        <div className="shelved-conversation-actions"><button type="button" disabled={busy !== null} onClick={() => void action(item, false)}>{zh ? "打开" : "Open"}</button><button type="button" disabled={busy !== null} onClick={() => void action(item, true)}>{busy === item.state.conversation_id ? (zh ? "处理中…" : "Working…") : (zh ? "恢复" : "Restore")}</button></div>
      </li>)}</ul>
      <div className="shelved-conversation-actions">
        {selection.offset > 0 && <button type="button" disabled={loading} onClick={() => setSelection((value) => ({ ...value, offset: Math.max(0, value.offset - 50) }))}>{zh ? "上一页" : "Previous page"}</button>}
        {page?.next_offset !== null && page?.next_offset !== undefined && !beyondLimit && <button type="button" disabled={loading} onClick={() => setSelection((value) => ({ ...value, offset: page.next_offset! }))}>{zh ? "下一页" : "Next page"}</button>}
      </div>
      {beyondLimit && <p>{zh ? "已达到分页限制，请缩小搜索范围。" : "Pagination limit reached. Narrow the search."}</p>}
    </div>
  </ResearchDialog>;
}
