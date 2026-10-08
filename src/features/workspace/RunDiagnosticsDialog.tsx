import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { getRunDiagnostics, saveRunDiagnostics } from "../../run-diagnostics-api";
import type { RunDiagnosticLocator, RunDiagnosticsExportFormatV4, RunDiagnosticsRequestV4, RunDiagnosticsSnapshotV4 } from "../../types";
import { useWindowEscapeLayer } from "../settings/BrowserSettings";
import type { Locale } from "./copy";

interface Props { projectId: string; conversationId: string; locale: Locale; onLocate: (target: RunDiagnosticLocator) => void; onClose: () => void }

// A scope change discards query text, preview, receipts and all asynchronous generations.
export function RunDiagnosticsDialog(props: Props) {
  return <ScopedRunDiagnosticsDialog key={JSON.stringify([props.projectId, props.conversationId])} {...props} />;
}

function ScopedRunDiagnosticsDialog({ projectId, conversationId, locale, onLocate, onClose }: Props) {
  const zh = locale === "zh-CN";
  const desktop = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
  const [runId, setRunId] = useState(""); const [toolId, setToolId] = useState("");
  const [keywords, setKeywords] = useState(""); const [failuresOnly, setFailuresOnly] = useState(false);
  const [preview, setPreview] = useState<{ snapshot: RunDiagnosticsSnapshotV4; query: RunDiagnosticsRequestV4 } | null>(null);
  const [busy, setBusy] = useState(false); const [error, setError] = useState(""); const [saved, setSaved] = useState(false);
  const generation = useRef(0); const pending = useRef(false); const alive = useRef(true);
  const dialog = useRef<HTMLElement>(null); const closeButton = useRef<HTMLButtonElement>(null);
  useWindowEscapeLayer(true, onClose);
  useLayoutEffect(() => {
    alive.current = true;
    const previous = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    const element = dialog.current; closeButton.current?.focus();
    return () => {
      alive.current = false; generation.current++;
      if (previous?.isConnected && (element?.contains(document.activeElement) || document.activeElement === document.body)) previous.focus();
    };
  }, []);
  useEffect(() => {
    function trap(event: KeyboardEvent) {
      if (event.key !== "Tab" || event.defaultPrevented) return;
      const items = Array.from(dialog.current?.querySelectorAll<HTMLElement>('button:not([disabled]), input:not([disabled]), textarea:not([disabled]), summary, [tabindex="0"]') ?? []);
      if (!items.length) return;
      event.preventDefault();
      const index = items.indexOf(document.activeElement as HTMLElement);
      items[index < 0 ? (event.shiftKey ? items.length - 1 : 0) : (index + (event.shiftKey ? -1 : 1) + items.length) % items.length].focus();
    }
    window.addEventListener("keydown", trap);
    return () => window.removeEventListener("keydown", trap);
  }, []);
  function invalidate() { generation.current++; setPreview(null); setSaved(false); setError(""); }
  async function refresh() {
    if (pending.current || !desktop) return;
    invalidate(); const version = generation.current;
    const query: RunDiagnosticsRequestV4 = { project_id: projectId, conversation_id: conversationId, filter: { run_id: runId || null, tool_id: toolId || null, failures_only: failuresOnly }, redaction_keywords: keywords === "" ? [] : keywords.split(/\r?\n/) };
    pending.current = true; setBusy(true); closeButton.current?.focus();
    try {
      const snapshot = await getRunDiagnostics(query);
      if (!alive.current || version !== generation.current) return;
      if (snapshot.project_id !== projectId || snapshot.conversation_id !== conversationId || !snapshot.source_chain_validated) throw "diagnostics_source_invalid";
      setPreview({ snapshot, query });
    } catch {
      if (alive.current && version === generation.current) setError(zh ? "无法读取诊断。请检查筛选输入或缩小会话范围后重试。" : "Diagnostics could not be read. Check filter inputs or use a smaller conversation and retry.");
    } finally { pending.current = false; if (alive.current) setBusy(false); }
  }
  async function save(format: RunDiagnosticsExportFormatV4) {
    if (pending.current || !preview || !desktop) return;
    const version = generation.current; pending.current = true; setBusy(true); setSaved(false); setError(""); closeButton.current?.focus();
    try {
      const path = await saveRunDiagnostics({ query: preview.query, expected_heads: preview.snapshot.source_heads, format });
      if (alive.current && version === generation.current && typeof path === "string" && path.length > 0) setSaved(true);
    } catch (reason) {
      if (!alive.current || version !== generation.current) return;
      if (reason === "diagnostics_snapshot_changed") {
        setPreview(null); setError(zh ? "源事件已改变，请刷新诊断后再次保存。" : "Source events changed. Refresh diagnostics before saving again.");
      } else setError(zh ? "诊断保存失败。" : "Diagnostics could not be saved.");
    } finally { pending.current = false; if (alive.current) setBusy(false); }
  }
  const snapshot = preview?.snapshot;
  const unknown = zh ? "未知" : "unknown";
  return <div className="run-diagnostics-backdrop"><section className="run-diagnostics-dialog" ref={dialog} role="dialog" aria-modal="true" aria-label={zh ? "运行诊断" : "Run diagnostics"}>
    <header><h2>{zh ? "运行诊断" : "Run diagnostics"}</h2><button ref={closeButton} aria-label={zh ? "关闭运行诊断" : "Close run diagnostics"} onClick={onClose}>×</button></header>
    <p>{zh ? "公开诊断投影，非完整科研证据。哈希仅引用原始事件，不能从投影重算。" : "Public diagnostic projection, not complete scientific evidence. Hashes reference original events and cannot be recomputed from this projection."}</p>
    {!desktop && <p role="status">{zh ? "需要桌面宿主进行验证和保存；浏览器预览不可用。" : "The desktop host is required for validation and saving; browser preview is unavailable."}</p>}
    <div className="run-diagnostics-filters">
      <label>{zh ? "运行 ID" : "Run ID"}<input value={runId} onChange={(e) => { invalidate(); setRunId(e.target.value); }} /></label>
      <label>{zh ? "工具 ID" : "Tool ID"}<input value={toolId} onChange={(e) => { invalidate(); setToolId(e.target.value); }} /></label>
      <label>{zh ? "脱敏词（每行一个）" : "Redaction keywords"}<textarea value={keywords} onChange={(e) => { invalidate(); setKeywords(e.target.value); }} /></label>
      <label><input type="checkbox" checked={failuresOnly} onChange={(e) => { invalidate(); setFailuresOnly(e.target.checked); }} />{zh ? "仅失败" : "Failures only"}</label>
    </div>
    <small>{zh ? "工具 ID 精确匹配；脱敏词仅本次窗口内保留，最多 32 个且不能为空。筛选变化后需刷新。" : "Tool ID is an exact match. Keywords remain in this window only: up to 32 nonempty lines. Refresh after changing filters."}</small>
    <nav><button disabled={!desktop || busy} onClick={() => void refresh()}>{zh ? "刷新诊断" : "Refresh diagnostics"}</button><button disabled={!preview || busy || !desktop} onClick={() => void save("html")}>{zh ? "保存 HTML" : "Save HTML"}</button><button disabled={!preview || busy || !desktop} onClick={() => void save("json")}>{zh ? "保存 JSON" : "Save JSON"}</button></nav>
    {busy && <p role="status">{zh ? "正在等待宿主…" : "Waiting for the host…"}</p>}{error && <p role="alert">{error}</p>}{saved && <p role="status">{zh ? "已成功保存固定快照；文件仅反映预览时的截点。" : "Saved successfully as a fixed snapshot at the preview cutoff."}</p>}
    {snapshot && <>
      <p>{zh ? "已验证源链；选中 / 原事件数" : "Source chain validated; selected / original events"}: {snapshot.selected_event_count} / {snapshot.total_event_count} · {snapshot.filtered ? (zh ? "已筛选" : "Filtered") : (zh ? "未筛选" : "Unfiltered")}</p>
      <details><summary>{zh ? "源截点" : "Source cutoff"}</summary><ul>{snapshot.source_heads.map((head) => <li key={head.run_id}><code>{head.run_id} #{head.last_sequence} {head.event_hash}</code> ({head.event_count})</li>)}</ul></details>
      <p>{zh ? "用量是选中完整运行的小计，不随工具或失败筛选重算。缺失值为未知，不补零。" : "Usage subtotals cover whole selected runs, independent of tool/failure filters. Missing values are unknown, never zero-filled."}</p>
      {snapshot.usage.map((usage) => <details key={usage.run_id}><summary>{usage.run_id} · {zh ? "缺少用量观察" : "Missing observations"}: {usage.missing_observation_attempts}</summary><dl>{(["input_tokens", "output_tokens", "reasoning_tokens", "cache_read_input_tokens", "cache_creation_input_tokens", "reported_total_tokens"] as const).map((key) => <div key={key}><dt>{key}</dt><dd>{usage.totals[key].known ?? unknown} · {zh ? "不完整次数" : "incomplete attempts"}: {usage.totals[key].incomplete_attempts}</dd></div>)}</dl></details>)}
      {snapshot.events.length === 0 && <p>{zh ? "没有匹配事件。" : "No matching events."}</p>}
      <ol className="run-diagnostics-events">{snapshot.events.map((event) => <li key={`${event.run_id}:${event.sequence}:${event.event_hash}`}><code>{event.run_id} #{event.sequence}</code><strong>{event.event_kind}</strong><time>{event.occurred_at}</time><span>{event.status ?? unknown} · {event.tool_label ?? "—"} · {event.duration_ms === null ? unknown : `${event.duration_ms} ms`}</span><code>{event.event_hash}</code><button disabled={busy} aria-label={zh ? `定位事件 ${event.sequence}` : `Locate event ${event.sequence}`} onClick={() => onLocate({ run_id: event.run_id, sequence: event.sequence, event_hash: event.event_hash })}>{zh ? "定位源事件" : "Locate source event"}</button></li>)}</ol>
    </>}
  </section></div>;
}
