import { useRef, useState } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import type { SideChatTurnStatusV4, SideChatTurnV4 } from "../../types";
import type { Locale } from "./copy";
import "./side-chat.css";

interface Props {
  projectId: string; conversationId: string; locale: Locale; records: SideChatTurnV4[];
  models: Array<{ id: string; label: string }>; modelId?: string; onModelChange: (id: string) => void;
  loading?: boolean; busy?: boolean; pending?: boolean; error?: boolean; disabled?: boolean; originalQuestion?: string;
  onSend: (question: string) => Promise<boolean>; onRefresh: () => unknown;
  onRetry?: () => Promise<boolean>; onRetryTurn?: (turn: SideChatTurnV4) => unknown;
  onOpenSource?: (messageId: string) => void;
  draftValue?: string; onDraftChange?: (draft: string) => void;
  hasMore?: boolean; loadingOlder?: boolean; onLoadOlder?: () => unknown;
}
export function SideChatPanel({ projectId, conversationId, locale, records, models, modelId, onModelChange, loading = false, busy = false, pending = false, error = false, disabled = false, originalQuestion, onSend, onRefresh, onRetry, onRetryTurn, onOpenSource, draftValue, onDraftChange, hasMore, loadingOlder, onLoadOlder }: Props) {
  const zh = locale === "zh-CN";
  const [localDraft, setLocalDraft] = useState(originalQuestion ?? "");
  const draft = draftValue ?? localDraft;
  const latestDraft = useRef(draft); latestDraft.current = draft;
  const setDraft = (value: string | ((previous: string) => string)) => {
    const next = typeof value === "function" ? value(latestDraft.current) : value;
    latestDraft.current = next;
    if (onDraftChange) onDraftChange(next); else setLocalDraft(next);
  };
  const [working, setWorking] = useState(false);
  const [failed, setFailed] = useState(false);
  const operation = useRef(false);
  const composing = useRef(false);
  const input = useRef<HTMLTextAreaElement>(null);
  const scope = `${projectId}:${conversationId}`;
  const current = useRef(scope); current.current = scope;
  const question = draft.trim();
  const tooLong = new TextEncoder().encode(question).length > 16 * 1024;
  const canSend = Boolean(question && modelId) && !disabled && !loading && !busy && !pending && !working && !tooLong;
  const labels: Record<SideChatTurnStatusV4, string> = zh
    ? { queued: "等待回答", running: "回答中", completed: "已回答", no_evidence: "未找到相关证据", failed: "回答失败", interrupted: "已中断" }
    : { queued: "Queued", running: "Answering", completed: "Answered", no_evidence: "No matching evidence", failed: "Failed", interrupted: "Interrupted" };
  async function submit(reconcile = false) {
    if (operation.current || (!reconcile && !canSend) || (reconcile && !onRetry)) return;
    operation.current = true; setWorking(true); setFailed(false);
    const original = reconcile ? originalQuestion : question;
    try {
      const accepted = reconcile ? await onRetry!() : await onSend(question);
      if (current.current !== scope) return;
      if (accepted) { setDraft((value) => value.trim() === original ? "" : value); input.current?.focus(); }
    } catch { if (current.current === scope) setFailed(true); }
    finally { operation.current = false; if (current.current === scope) setWorking(false); }
  }
  return <section className="side-chat-panel" aria-label={zh ? "独立旁聊" : "Side chat"}>
    <div className="side-chat-heading"><strong>{zh ? "独立旁聊" : "Side chat"}</strong><button type="button" onClick={() => void onRefresh()}>{zh ? "刷新" : "Refresh"}</button></div>
    <p className="side-chat-note">{zh ? "根据已保存的会话证据回答；可与主 Agent 同时使用。" : "Ask about saved conversation evidence while the main Agent works."}</p>
    <label>{zh ? "旁聊模型" : "Side-chat model"}<select aria-label={zh ? "旁聊模型" : "Side-chat model"} value={modelId ?? ""} disabled={working || busy || pending} onChange={(event) => onModelChange(event.target.value)}><option value="">{zh ? "选择模型" : "Choose a model"}</option>{models.map((model) => <option key={model.id} value={model.id}>{model.label}</option>)}</select></label>
    {loading && <p role="status">{zh ? "正在读取旁聊历史…" : "Loading side-chat history…"}</p>}
    {(error || failed) && <p role="alert">{zh ? "操作尚未确认，请刷新或核对原问题。" : "The operation was not confirmed. Refresh or reconcile the original question."}</p>}
    {pending && originalQuestion && <div className="side-chat-question"><strong>{zh ? "等待确认的问题" : "Question awaiting confirmation"}</strong><p>{originalQuestion}</p></div>}
    {hasMore && onLoadOlder && <button type="button" disabled={loadingOlder} onClick={() => void onLoadOlder()}>{zh ? "读取更早的旁聊" : "Load older side chats"}</button>}
    <div className="side-chat-history" aria-live="polite">{records.filter((turn) => turn.project_id === projectId && turn.conversation_id === conversationId).map((turn) => <article key={turn.request_id} className="side-chat-turn">
      <div className="side-chat-question"><ReactMarkdown remarkPlugins={[remarkGfm]}>{turn.question_markdown}</ReactMarkdown></div>
      {Boolean(turn.attachments.length || turn.references.length) && <small>{turn.attachments.length} {zh ? "个附件" : "attachments"} · {turn.references.length} {zh ? "个引用" : "references"}</small>}
      <p className="side-chat-turn-meta">{labels[turn.status]} · {turn.model_label || (zh ? "已保存的模型" : "Saved model")}</p>
      {turn.answer_markdown && <div className="side-chat-answer"><ReactMarkdown remarkPlugins={[remarkGfm]}>{turn.answer_markdown}</ReactMarkdown></div>}
      {turn.status === "no_evidence" && <p>{zh ? "当前保存的资料不足以回答这个问题。" : "The saved material does not contain enough evidence to answer this question."}</p>}
      {turn.sources.length > 0 && <details className="side-chat-sources"><summary>{zh ? `来源（${turn.sources.length}）` : `Sources (${turn.sources.length})`}</summary>{turn.sources.map((source) => <div key={source.source_id}><strong>{source.label}</strong><blockquote>{source.excerpt}</blockquote>{source.message_id && onOpenSource && <button type="button" onClick={() => onOpenSource(source.message_id!)}>{zh ? "定位来源消息" : "Go to source message"}</button>}</div>)}</details>}
      {(turn.status === "failed" || turn.status === "interrupted") && onRetryTurn && <button type="button" disabled={busy || pending || working || disabled} onClick={() => void onRetryTurn(turn)}>{zh ? "重新提问" : "Ask again"}</button>}
    </article>)}</div>
    <form onSubmit={(event) => { event.preventDefault(); void submit(); }}>
      <textarea ref={input} aria-label={zh ? "旁聊问题" : "Side question"} value={draft} onChange={(event) => setDraft(event.target.value)} placeholder={zh ? "询问方法、结果或证据…" : "Ask about methods, results, or evidence…"} onCompositionStart={() => { composing.current = true; }} onCompositionEnd={() => { composing.current = false; }} onKeyDown={(event) => { if (event.key === "Enter" && !event.shiftKey && !event.nativeEvent.isComposing && !composing.current && event.keyCode !== 229) { event.preventDefault(); void submit(); } }} />
      {tooLong && <p role="alert">{zh ? "问题超过 16 KiB，请缩短后再发送。" : "The question exceeds 16 KiB. Shorten it before sending."}</p>}
      <div className="side-chat-actions">{pending && onRetry && <button type="button" disabled={working} onClick={() => void submit(true)}>{zh ? "核对原问题" : "Reconcile original question"}</button>}<button type="submit" disabled={!canSend}>{working || busy ? (zh ? "回答中…" : "Answering…") : (zh ? "提问" : "Ask")}</button></div>
    </form>
  </section>;
}
