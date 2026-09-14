import { useEffect, useRef, useState } from "react";
import { ArrowDown, ArrowUp, Pencil, X } from "lucide-react";
import type { Locale } from "./copy";
import type { ComposerQueueActionRequestV4, ComposerQueueActionV4, ComposerQueueItemV4, ComposerQueueStatusV4, UpdateComposerQueueRequestV4 } from "../../types";
import { useWindowEscapeLayer } from "../settings/BrowserSettings";
import "./ComposerQueuePanel.css";

interface Props {
  projectId: string;
  conversationId: string;
  locale: Locale;
  items: ComposerQueueItemV4[];
  onRestore?: (item: ComposerQueueItemV4) => Promise<void>;
  restoreDisabled?: boolean;
  cutInAvailable?: boolean;
  onRefresh?: () => Promise<unknown>;
  onUpdate: (request: UpdateComposerQueueRequestV4) => Promise<unknown>;
  onAction: (request: ComposerQueueActionRequestV4) => Promise<unknown>;
}

export function ComposerQueuePanel({ projectId, conversationId, locale, items, onUpdate, onAction, onRefresh, onRestore, restoreDisabled = false, cutInAvailable = false }: Props) {
  const zh = locale === "zh-CN";
  const [editing, setEditing] = useState<ComposerQueueItemV4 | null>(null);
  const [draft, setDraft] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const generation = useRef(0);
  const retainedDrafts = useRef(new Map<string, string>());
  const draftKey = (item: ComposerQueueItemV4) => `${item.project_id}:${item.conversation_id}:${item.request_id}`;
  const editTrigger = useRef<HTMLButtonElement | null>(null);
  const restoreEditFocus = useRef(false);
  useEffect(() => { if (!editing && !busy && restoreEditFocus.current) { restoreEditFocus.current = false; if (editTrigger.current?.isConnected) editTrigger.current.focus(); } }, [editing, busy]);
  const operation = useRef<object | null>(null);
  useEffect(() => {
    generation.current += 1;
    operation.current = null;
    setEditing(null); setDraft(""); setError(""); setBusy(false);
    return () => { generation.current += 1; operation.current = null; };
  }, [projectId, conversationId]);
  function closeEditor() {
    setEditing(null);
    restoreEditFocus.current = true;
    editTrigger.current?.focus();
  }
  useWindowEscapeLayer(editing !== null, closeEditor);
  const scoped = items.filter((item) => item.project_id === projectId && item.conversation_id === conversationId).sort((a, b) => a.position - b.position);
  const pending = scoped.filter((item) => item.status === "pending");
  const labels: Record<ComposerQueueStatusV4, string> = zh
    ? { pending: "等待发送", dispatching: "正在准备", running: "运行中", completed: "已完成", failed: "失败", cancelled: "已取消", uncertain: "待核对" }
    : { pending: "Queued", dispatching: "Preparing", running: "Running", completed: "Completed", failed: "Failed", cancelled: "Cancelled", uncertain: "Needs reconciliation" };
  async function perform(action: () => Promise<unknown>, closesEditor = false) {
    if (operation.current) return;
    const token = {}; operation.current = token;
    const captured = generation.current;
    setBusy(true); setError("");
    try {
      await action();
      if (captured === generation.current && closesEditor) closeEditor();
    } catch {
      if (captured === generation.current) setError(zh ? "操作未确认。请刷新队列后重试；草稿和附件已保留。" : "The change was not confirmed. Refresh the queue and retry; the draft and attachments are retained.");
    } finally {
      if (operation.current === token) { operation.current = null; setBusy(false); }
    }
  }
  function act(item: ComposerQueueItemV4, action: ComposerQueueActionV4) {
    void perform(() => onAction({ project_id: projectId, conversation_id: conversationId, request_id: item.request_id, expected_revision: item.revision, action }));
  }
  if (!scoped.length) return null;
  return <section className="composer-queue" aria-label={zh ? "发送队列" : "Send queue"}>
    <header><strong>{zh ? "发送队列" : "Send queue"}</strong><span>{pending.length} {zh ? "条等待发送" : "waiting"}</span></header>
    {pending.length > 0 && <p className="composer-queue-material">{zh ? "停止当前运行不会清空队列；当前运行安全结束后继续发送。" : "Stopping the current run keeps queued messages; sending continues once that run safely settles."}</p>}
    {error && <div><p role="alert">{error}</p>{onRefresh && <button type="button" disabled={busy} onClick={() => void perform(onRefresh)}>{zh ? "刷新队列" : "Refresh queue"}</button>}</div>}
    <ol>{scoped.map((item) => <li key={item.request_id}>
      <div className="composer-queue-meta"><span>{item.cut_in_message_id ? (zh ? "已插入当前运行" : "Delivered as guidance") : labels[item.status]}</span><small>{item.mode === "plan" ? "Plan" : "Agent"} · {item.frozen.compute_selection.backend_id}</small></div>
      {editing?.request_id === item.request_id ? <form onSubmit={(event) => {
        event.preventDefault();
        const original = editing;
        if (!draft.trim() || busy) return;
        const submitted = draft;
        void perform(async () => { await onUpdate({ project_id: projectId, conversation_id: conversationId, request_id: original.request_id, expected_revision: original.revision, message_markdown: submitted, references: original.references, attachments: original.attachments }); if (retainedDrafts.current.get(draftKey(original)) === submitted) retainedDrafts.current.delete(draftKey(original)); }, true);
      }}><textarea aria-label={zh ? "排队消息" : "Queued message"} value={draft} onChange={(event) => { setDraft(event.target.value); retainedDrafts.current.set(draftKey(editing), event.target.value); }} disabled={busy} /><div className="composer-queue-actions"><button type="submit" disabled={busy || !draft.trim()}>{zh ? "保存修改" : "Save changes"}</button><button type="button" onClick={closeEditor}>{zh ? "返回" : "Back"}</button></div></form> : <p className="composer-queue-text">{item.message_markdown}</p>}
      {item.failure_code && <p className="composer-queue-failure">{({ configuration_changed: zh ? "模型或会话设置已变化" : "Model or conversation settings changed", material_changed: zh ? "引用或附件已变化" : "A reference or attachment changed", dispatch_failed: zh ? "未能启动运行" : "The run could not be started", run_failed: zh ? "运行失败，请查看运行记录" : "The run failed; inspect its history", cancelled_by_user: zh ? "已取消" : "Cancelled", lease_uncertain: zh ? "派发状态需要核对" : "Dispatch requires reconciliation" })[item.failure_code]}</p>}
      {item.replacement_target_run_id && <small className="composer-queue-material">{zh ? "替换请求：保留旧记录，安全结束后优先发送。取消此消息不会撤回已发出的停止请求。" : "Replacement: keeps prior history and sends first after safe settlement. Cancelling this message does not undo Stop."}</small>}
      {item.replacement_receipt && <details className="composer-replacement-receipt"><summary>{zh ? "已接收的替换回执" : "Accepted replacement receipt"}</summary><p>{zh ? "旧运行" : "Prior run"}: {item.replacement_receipt.target_run_id}<br />Stop ID: {item.replacement_receipt.stop.request_id}<br />{zh ? "来源事件" : "Source event"}: {item.replacement_receipt.source_event_sequence}<br />SHA-256: {item.replacement_receipt.source_event_hash}{item.replacement_receipt.source_message_id && <><br />{zh ? "来源消息" : "Source message"}: {item.replacement_receipt.source_message_id}</>}</p></details>}
      {(item.attachments.length > 0 || item.references.length > 0) && <small className="composer-queue-material">{item.attachments.length} {zh ? "个附件" : "attachments"} · {item.references.length} {zh ? "个引用" : "references"}</small>}
      {onRestore && !item.cut_in_message_id && (item.status === "pending" || item.status === "cancelled" || item.status === "failed") && <button type="button" disabled={busy || restoreDisabled} onClick={() => void perform(() => onRestore(item))}>{zh ? "恢复到输入框" : "Restore to composer"}</button>}
      {item.status === "pending" && <div className="composer-queue-actions">
        {cutInAvailable && !item.replacement_target_run_id && <button type="button" disabled={busy || item.mode !== "agent" || item.attachments.length > 0 || item.references.length > 0 || new TextEncoder().encode(item.message_markdown.trim()).length > 2048} title={new TextEncoder().encode(item.message_markdown.trim()).length > 2048 ? (zh ? "消息过长，请先编辑缩短" : "Shorten this message before sending guidance") : (zh ? "仅可插入不带附件或引用的纯文本消息" : "Plain text without attachments or references only")} onClick={() => act(item, "cut_in")}>{zh ? "插入当前运行" : "Send as guidance"}</button>}
        <button type="button" disabled={busy || Boolean(item.replacement_target_run_id)} aria-label={zh ? "编辑排队消息" : "Edit queued message"} onClick={(event) => { editTrigger.current = event.currentTarget; setEditing(item); setDraft(retainedDrafts.current.get(draftKey(item)) ?? item.message_markdown); setError(""); }}><Pencil size={14} /></button>
        <button type="button" disabled={busy || Boolean(item.replacement_target_run_id) || Boolean(pending[pending.findIndex((row) => row.request_id === item.request_id) - 1]?.replacement_target_run_id) || pending[0]?.request_id === item.request_id} aria-label={zh ? "上移排队消息" : "Move queued message up"} onClick={() => act(item, "move_up")}><ArrowUp size={14} /></button>
        <button type="button" disabled={busy || Boolean(item.replacement_target_run_id) || Boolean(pending[pending.findIndex((row) => row.request_id === item.request_id) + 1]?.replacement_target_run_id) || pending.at(-1)?.request_id === item.request_id} aria-label={zh ? "下移排队消息" : "Move queued message down"} onClick={() => act(item, "move_down")}><ArrowDown size={14} /></button>
        <button type="button" disabled={busy} aria-label={zh ? "取消排队消息" : "Cancel queued message"} onClick={() => act(item, "cancel")}><X size={14} /></button>
      </div>}
    </li>)}</ol>
  </section>;
}
