import { useEffect, useRef, useState } from "react";
import * as api from "../../tauri-api";
import type { GuidanceRecordV4, SubmitGuidanceV4Request } from "../../types";
import "./GuidancePanel.css";

export interface GuidancePanelProps { runId: string; projectId: string; conversationId: string; enabled: boolean; locale: "zh-CN" | "en-US" }

export function GuidancePanel({ runId, projectId, conversationId, enabled, locale }: GuidancePanelProps) {
  const zh = locale === "zh-CN";
  const [records, setRecords] = useState<GuidanceRecordV4[]>([]);
  const [draft, setDraft] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState(false);
  const generation = useRef(0);
  const attempt = useRef<SubmitGuidanceV4Request | null>(null);
  const submitting = useRef(false);
  const refresh = useRef<() => Promise<void>>(async () => undefined);
  const bytes = new TextEncoder().encode(draft.trim()).length;

  useEffect(() => {
    const token = ++generation.current;
    let disposed = false;
    let latest = 0;
    let unlisten: (() => void) | undefined;
    setRecords([]); setDraft(""); setError(false); setBusy(false);
    attempt.current = null; submitting.current = false;
    const current = () => !disposed && generation.current === token;
    const load = async () => {
      const serial = ++latest;
      try {
        const rows = await api.agentV4ListGuidance(projectId, conversationId, runId);
        if (!current() || serial !== latest) return;
        const scoped = rows.filter((row) => row.project_id === projectId && row.conversation_id === conversationId && row.run_id === runId);
        setRecords((previous) => scoped.map((row) => {
          const known = previous.find((item) => item.message_id === row.message_id);
          return known?.consumed_at && !row.consumed_at ? known : row;
        }).sort((a, b) => a.ordinal - b.ordinal));
        if (attempt.current && scoped.some((row) => row.message_id === attempt.current?.message_id && row.markdown === attempt.current?.markdown)) {
          const acceptedText = attempt.current.markdown;
          attempt.current = null; setDraft((value) => value.trim() === acceptedText ? "" : value); setError(false);
        }
      } catch { if (current() && serial === latest) setError(true); }
    };
    refresh.current = load;
    void load();
    const timer = window.setInterval(() => { void load(); }, 3000);
    void api.onAgentV4Event((event) => {
      if (event.run_id === runId && event.project_id === projectId && event.conversation_id === conversationId) void load();
    }).then((cleanup) => { if (disposed) cleanup(); else unlisten = cleanup; }).catch(() => { if (current()) setError(true); });
    return () => { disposed = true; ++generation.current; window.clearInterval(timer); unlisten?.(); };
  }, [runId, projectId, conversationId]);

  useEffect(() => { void refresh.current(); }, [enabled]);

  async function send() {
    if (submitting.current || !enabled || bytes === 0 || bytes > 2048) return;
    const token = generation.current;
    const markdown = draft.trim();
    // Reuse the id across an uncertain response; edited text is a new request.
    if (!attempt.current || attempt.current.markdown !== markdown) {
      attempt.current = { message_id: crypto.randomUUID(), run_id: runId, project_id: projectId, conversation_id: conversationId, markdown };
    }
    const request = attempt.current;
    submitting.current = true; setBusy(true); setError(false);
    try {
      const row = await api.agentV4SubmitGuidance(request);
      if (generation.current !== token) return;
      if (row.message_id !== request.message_id || row.run_id !== runId || row.project_id !== projectId || row.conversation_id !== conversationId || row.markdown !== markdown) throw new Error("guidance response mismatch");
      setRecords((previous) => {
        const known = previous.find((item) => item.message_id === row.message_id);
        return [...previous.filter((item) => item.message_id !== row.message_id), known?.consumed_at ? known : row].sort((a,b) => a.ordinal-b.ordinal);
      });
      attempt.current = null; setDraft("");
      void refresh.current();
    } catch { if (generation.current === token) setError(true); }
    finally { if (generation.current === token) { submitting.current = false; setBusy(false); } }
  }

  return <section className="guidance-panel" aria-label={zh ? "运行中指导" : "Run guidance"}>
    <strong>{zh ? "运行中指导" : "Run guidance"}</strong>
    <small>{zh ? "已接收的指导会在下一模型边界应用，不能扩大执行权限。" : "Accepted guidance is applied at the next model boundary and cannot expand execution permissions."}</small>
    {enabled && <div className="guidance-form">
      <textarea aria-label={zh ? "追加指导" : "Additional guidance"} value={draft} disabled={busy} onChange={(event) => setDraft(event.target.value)} rows={2} />
      <div className="guidance-form-footer"><small>{bytes}/2048 {zh ? "字节" : "bytes"}</small><button className="guidance-submit" disabled={busy || bytes === 0 || bytes > 2048} onClick={() => void send()}>{zh ? "发送指导" : "Send guidance"}</button></div>
    </div>}
    {error && <p role="alert">{zh ? "无法确认指导状态，原文已保留，可重试或刷新。" : "Could not confirm guidance status. Your text is retained; retry or refresh."}</p>}
    <div className="guidance-records"><ol>{records.map((row) => <li key={row.message_id}>
      <small>{row.consumed_at ? (zh ? "已应用" : "Applied") : enabled ? (zh ? "已接收，等待应用" : "Received, waiting to apply") : (zh ? "已接收，尚未应用" : "Received, not yet applied")}</small><p>{row.markdown}</p>
    </li>)}</ol></div>
  </section>;
}
