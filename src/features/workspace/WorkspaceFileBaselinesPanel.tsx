import { useEffect, useRef, useState } from "react";
import { createWorkspaceFileBaseline, diffWorkspaceFileBaseline, getWorkspaceFileBaseline, listWorkspaceFileBaselines, readWorkspaceBaselineText } from "../../workspace-navigation-api";
import type { BaselineDiffStateV1, BaselineOmissionCodeV1, BaselineTextSnapshotV1, CreateFileBaselineRequestV1, FileBaselineDiffV1, FileBaselineEntryV1, FileBaselineErrorCodeV1, FileBaselineV1, FileReferenceHashV1, ProjectFileSelectionV1 } from "../../workspace-navigation-types";
import { useWindowEscapeLayer } from "../settings/BrowserSettings";
import type { Locale } from "./copy";
import "./workspace-file-baselines.css";

// Shell owns this record for its lifetime. Dismissing the UI does not cancel a
// native write or justify issuing a new request ID for an uncertain receipt.
export interface PendingFileBaseline {
  scope: string;
  request: CreateFileBaselineRequestV1 | null;
  busy: boolean;
  receipt: FileBaselineV1 | null;
  error: FileBaselineErrorCodeV1 | "unknown" | null;
  listeners: Set<() => void>;
}
export function newPendingFileBaseline(scope: string): PendingFileBaseline {
  return { scope, request: null, busy: false, receipt: null, error: null, listeners: new Set() };
}
interface Props {
  projectId: string;
  branchConversationId: string | null;
  locale: Locale;
  selectedFiles: ProjectFileSelectionV1[];
  onClose: () => void;
  pendingRequest?: PendingFileBaseline;
}
const errors: Record<FileBaselineErrorCodeV1 | "unknown", [string, string]> = {
  invalid_selection: ["选择无效，请检查文件和标题。", "Invalid selection. Check files and title."],
  not_found: ["基线不可用。", "Baseline unavailable."],
  ownership_mismatch: ["基线身份不匹配。", "Baseline identity mismatch."],
  request_conflict: ["请求身份冲突。", "Request identity conflict."],
  source_changed: ["源文件发生变化，捕获已拒绝。", "Source changed; capture rejected."],
  manifest_changed: ["已保存基线校验失败。", "Saved baseline verification failed."],
  limit_exceeded: ["超过基线容量限制。", "Baseline capacity limit exceeded."],
  storage_failed: ["保存回执未确认，可重试原请求。", "Save receipt unconfirmed. Retry the same request."],
  unknown: ["请求结果未确认，请重试。", "Request result unconfirmed. Please retry."],
};
const omissions: Record<BaselineOmissionCodeV1, [string, string]> = {
  remote_reference: ["远端引用，未读取远端正文", "Remote reference; remote text not read"],
  too_large: ["超过单文件预算", "Per-file budget exceeded"], unsupported_content: ["不支持的正文", "Unsupported content"],
  sensitive_content: ["敏感内容已省略", "Sensitive content omitted"], missing: ["捕获时源文件缺失", "Source missing at capture"],
  unreadable: ["捕获时不可读取", "Unreadable at capture"], unsafe_file: ["不安全文件已省略", "Unsafe file omitted"],
  aggregate_budget: ["总读取预算耗尽", "Aggregate read budget exhausted"],
};
const states: Record<BaselineDiffStateV1, [string, string]> = {
  unchanged: ["未变化", "Unchanged"], changed: ["已变化", "Changed"], missing: ["缺失", "Missing"], unreadable: ["不可读取", "Unreadable"],
  unsafe_file: ["不安全文件", "Unsafe file"], reference_only: ["仅引用，未比较正文", "Reference only; text not compared"],
  binding_changed: ["项目根绑定已变化，未比较当前文件", "Root binding changed; current files not compared"],
  current_too_large: ["当前文件过大", "Current file too large"], sensitive_current_content: ["当前敏感正文已省略", "Sensitive current content omitted"],
};
function errorCode(error: unknown): FileBaselineErrorCodeV1 | "unknown" {
  const code = error && typeof error === "object" && "code" in error ? error.code : null;
  return typeof code === "string" && Object.hasOwn(errors, code) ? code as FileBaselineErrorCodeV1 : "unknown";
}
function matches(baseline: FileBaselineV1, projectId: string, id?: string) {
  return baseline?.project_id === projectId && (!id || baseline.id === id);
}

export function WorkspaceFileBaselinesPanel(props: Props) {
  return <FileBaselinesView key={JSON.stringify([props.projectId, props.pendingRequest?.scope ?? null])} {...props} />;
}
function FileBaselinesView({ projectId, branchConversationId, locale, selectedFiles, onClose, pendingRequest }: Props) {
  const zh = locale === "zh-CN"; const language = zh ? 0 : 1;
  const fallback = useRef<PendingFileBaseline>(newPendingFileBaseline(projectId));
  const pending = pendingRequest ?? fallback.current;
  const [, redraw] = useState(0);
  const [title, setTitle] = useState(zh ? "探索基线" : "Exploration baseline");
  const [associate, setAssociate] = useState(false);
  const [branchFilter, setBranchFilter] = useState(false);
  const [items, setItems] = useState<FileBaselineV1[]>([]);
  const [nextOffset, setNextOffset] = useState<number | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<FileBaselineErrorCodeV1 | "unknown" | null>(null);
  const [selected, setSelected] = useState<FileBaselineV1 | null>(null);
  const [child, setChild] = useState<{ kind: "text" | "diff"; text?: BaselineTextSnapshotV1; diff?: FileBaselineDiffV1; error?: FileBaselineErrorCodeV1 | "unknown" } | null>(null);
  const alive = useRef(true); const listGeneration = useRef(0); const selectionGeneration = useRef(0); const childGeneration = useRef(0);
  const refreshRef = useRef<() => void>(() => {});
  useEffect(() => {
    alive.current = true;
    const listener = () => redraw((value) => value + 1); pending.listeners.add(listener);
    return () => { alive.current = false; listGeneration.current++; selectionGeneration.current++; childGeneration.current++; pending.listeners.delete(listener); };
  }, [pending]);
  function closeChild() { childGeneration.current++; setChild(null); }
  useWindowEscapeLayer(true, onClose);
  useWindowEscapeLayer(child !== null, closeChild);

  async function load(offset = 0) {
    const generation = ++listGeneration.current; setLoading(true); setError(null);
    try {
      const page = await listWorkspaceFileBaselines({ project_id: projectId, branch_conversation_id: branchFilter ? branchConversationId : null, offset, limit: 20 });
      if (!alive.current || generation !== listGeneration.current) return;
      if (!page || !Array.isArray(page.items) || page.items.some((item) => !matches(item, projectId))) throw { code: "ownership_mismatch" };
      setItems((old) => offset ? [...old, ...page.items.filter((item) => !old.some((value) => value.id === item.id))] : page.items); setNextOffset(page.next_offset);
    } catch (caught) { if (alive.current && generation === listGeneration.current) setError(errorCode(caught)); }
    finally { if (alive.current && generation === listGeneration.current) setLoading(false); }
  }
  refreshRef.current = () => { void load(); };
  useEffect(() => { setItems([]); setNextOffset(null); void load(); return () => { listGeneration.current++; }; }, [projectId, branchFilter, branchConversationId]);
  async function open(id: string) {
    const generation = ++selectionGeneration.current; closeChild(); setSelected(null); setError(null);
    try {
      const baseline = await getWorkspaceFileBaseline(projectId, id);
      if (!alive.current || generation !== selectionGeneration.current) return;
      if (!matches(baseline, projectId, id)) throw { code: "ownership_mismatch" };
      setSelected(baseline);
    } catch (caught) { if (alive.current && generation === selectionGeneration.current) setError(errorCode(caught)); }
  }
  async function save() {
    if (pending.busy) return;
    const selection = ++selectionGeneration.current; const childTicket = childGeneration.current;
    if (!pending.request) {
      const files = selectedFiles.map((file) => Object.freeze({ ...file })).sort((a, b) => a.backend_id.localeCompare(b.backend_id) || a.relative_path.localeCompare(b.relative_path));
      pending.request = Object.freeze({ request_id: crypto.randomUUID(), project_id: projectId, branch_conversation_id: associate ? branchConversationId : null, title: title.trim(), files: Object.freeze(files) as unknown as ProjectFileSelectionV1[] });
    }
    const request = pending.request; pending.busy = true; pending.error = null; pending.receipt = null;
    pending.listeners.forEach((listener) => listener());
    try {
      const baseline = await createWorkspaceFileBaseline(request);
      if (!matches(baseline, request.project_id)) throw { code: "ownership_mismatch" };
      pending.receipt = baseline; pending.request = null;
      if (alive.current) {
        if (selection === selectionGeneration.current && childTicket === childGeneration.current) { closeChild(); setSelected(baseline); }
        refreshRef.current();
      }
    } catch (caught) { pending.error = errorCode(caught); }
    finally { pending.busy = false; pending.listeners.forEach((listener) => listener()); }
  }
  // Only these two native create errors prove no baseline commit. Other codes
  // can also arise while reading a previously committed replay receipt.
  function releaseRejectedRequest() {
    if (pending.busy || !pending.request || (pending.error !== "source_changed" && pending.error !== "limit_exceeded")) return;
    setTitle(pending.request.title); pending.request = null; pending.error = null;
    pending.listeners.forEach((listener) => listener());
  }
  async function view(kind: "text" | "diff", entryIndex = 0) {
    if (!selected) return;
    const baseline = selected; const generation = ++childGeneration.current; setChild({ kind });
    const request = { project_id: projectId, baseline_id: baseline.id, expected_manifest_sha256: baseline.manifest_sha256 };
    try {
      if (kind === "text") {
        const text = await readWorkspaceBaselineText({ ...request, entry_index: entryIndex });
        if (!alive.current || generation !== childGeneration.current) return;
        const entry = baseline.entries[entryIndex];
        if (text?.project_id !== projectId || text.baseline_id !== baseline.id || text.entry_index !== entryIndex || text.relative_path !== entry.relative_path || text.full_sha256 !== entry.hash?.sha256) throw { code: "ownership_mismatch" };
        setChild({ kind, text });
      } else {
        const diff = await diffWorkspaceFileBaseline(request);
        if (!alive.current || generation !== childGeneration.current) return;
        if (diff?.project_id !== projectId || diff.baseline_id !== baseline.id || diff.manifest_sha256 !== baseline.manifest_sha256) throw { code: "manifest_changed" };
        setChild({ kind, diff });
      }
    } catch (caught) { if (alive.current && generation === childGeneration.current) setChild({ kind, error: errorCode(caught) }); }
  }
  const validTitle = [...title.trim()].length > 0 && [...title.trim()].length <= 80 && new TextEncoder().encode(title.trim()).length <= 320;
  return <div className="file-baselines-backdrop"><section className="file-baselines-panel" role="dialog" aria-modal="true" aria-label={zh ? "文件基线" : "File baselines"}>
    <header><h2>{zh ? "文件基线" : "File baselines"}</h2><button type="button" onClick={onClose}>{zh ? "关闭基线" : "Close baselines"}</button></header>
    <p>{zh ? "仅保存明确选择的文件。逐项顺序读取，不是原子项目快照。安全 UTF-8 小文本可保存正文；远端和其他内容仅引用。" : "Only explicitly selected files are captured. Files are read sequentially, not as an atomic project snapshot. Safe small UTF-8 text may be saved; remote and other content remain references."}</p>
    <p>{zh ? "最多 32 项；每项 512 KiB，总正文 4 MiB。关闭面板不取消已派发的保存。" : "Up to 32 files; 512 KiB per file and 4 MiB total text. Closing does not cancel a dispatched save."}</p>
    <ul aria-label={zh ? "已选文件" : "Selected files"}>{selectedFiles.map((file) => <li key={`${file.backend_id}:${file.relative_path}`}><span>{file.relative_path}</span> · <code>{file.backend_id}</code></li>)}</ul>
    <label>{zh ? "基线标题" : "Baseline title"}<input aria-label={zh ? "基线标题" : "Baseline title"} value={pending.request?.title ?? title} disabled={pending.request !== null || pending.busy} onChange={(event) => setTitle(event.target.value)} /></label>
    {branchConversationId && <label><input type="checkbox" checked={associate} disabled={pending.request !== null || pending.busy} onChange={(event) => setAssociate(event.target.checked)} />{zh ? "关联当前已验证分支" : "Associate verified branch"}</label>}
    {pending.request && <p>{zh ? "待确认原请求（重试使用以下选择）：" : "Unconfirmed original request (retry uses these files):"} {pending.request.title} · {pending.request.branch_conversation_id ?? (zh ? "无分支" : "No branch")}{pending.request.files.map((file) => <span key={`${file.backend_id}:${file.relative_path}`}> · {file.backend_id}:{file.relative_path}</span>)}</p>}
    <button type="button" disabled={pending.busy || (!pending.request && (!validTitle || selectedFiles.length < 1 || selectedFiles.length > 32))} onClick={() => void save()}>{pending.busy ? (zh ? "保存中…" : "Saving…") : pending.request ? (zh ? "重试原请求" : "Retry same request") : (zh ? "保存基线" : "Save baseline")}</button>
    {pending.error && <p role="alert">{errors[pending.error][language]}</p>}
    {!pending.busy && pending.request && (pending.error === "source_changed" || pending.error === "limit_exceeded") && <button type="button" onClick={releaseRejectedRequest}>{zh ? "编辑选择 / 新请求" : "Edit selection / New request"}</button>}
    {pending.receipt && <p role="status">{zh ? "已保存基线：" : "Saved baseline: "}{pending.receipt.title} · {pending.receipt.id}</p>}
    <div className="file-baselines-actions"><button type="button" onClick={() => void load()}>{zh ? "刷新基线" : "Refresh baselines"}</button>{branchConversationId && <label><input type="checkbox" checked={branchFilter} onChange={(event) => setBranchFilter(event.target.checked)} />{zh ? "仅当前分支基线" : "Only verified branch baselines"}</label>}</div>
    {loading && <p role="status">{zh ? "读取基线列表…" : "Loading baselines…"}</p>}{error && <p role="alert">{errors[error][language]}</p>}
    <ul className="file-baselines-list">{items.map((item) => <li key={item.id}><button type="button" aria-label={`${zh ? "打开基线" : "Open baseline"} ${item.title}`} onClick={() => void open(item.id)}>{item.title}</button><small>{item.created_at} · {item.entries.length} {zh ? "项" : "files"}</small></li>)}</ul>
    {nextOffset !== null && <button type="button" disabled={loading} onClick={() => void load(nextOffset)}>{zh ? "更多基线" : "More baselines"}</button>}
    {selected && <section aria-label={zh ? "保存的基线详情" : "Saved baseline details"}><h3>{selected.title}</h3><p>{zh ? "保存的 manifest SHA-256" : "Saved manifest SHA-256"}: <code>{selected.manifest_sha256}</code></p><p>{zh ? "当前分支关联" : "Current branch association"}: {selected.branch_conversation_id ?? (zh ? "无" : "None")}</p>
      {selected.entries.map((entry, index) => <article key={index}><EntryMetadata entry={entry} zh={zh} />{entry.content_mode === "text_snapshot" && <button type="button" aria-label={`${zh ? "查看保存正文" : "View saved text"} ${entry.relative_path}`} onClick={() => void view("text", index)}>{zh ? "查看保存正文" : "View saved text"}</button>}</article>)}
      <button type="button" onClick={() => void view("diff")}>{zh ? "比较当前文件" : "Compare current files"}</button>
    </section>}
    {child && <div className="file-baselines-child"><section className="file-baselines-panel" role="dialog" aria-modal="true" aria-label={child.kind === "text" ? (zh ? "基线保存正文" : "Saved baseline text") : (zh ? "基线差异" : "Baseline diff")}><header><h2>{child.kind === "text" ? (zh ? "基线保存正文" : "Saved baseline text") : (zh ? "基线差异" : "Baseline diff")}</h2><button type="button" onClick={closeChild}>{zh ? "关闭查看" : "Close view"}</button></header>
      {child.error ? <p role="alert">{errors[child.error][language]}</p> : !child.text && !child.diff ? <p role="status">{zh ? "读取中…" : "Loading…"}</p> : null}
      {child.text && <><p>{child.text.relative_path} · {zh ? "保存的完整文件 SHA-256" : "Saved full file SHA-256"}: {child.text.full_sha256}</p><pre>{child.text.text}</pre></>}
      {child.diff && <><p>{zh ? "观察时间" : "Observed"}: {child.diff.observed_at}</p><p>{zh ? "比较有读取、行数、工作量和输出预算；截断不代表完整比较。远端仅引用未校验。" : "Comparison has read, line, work and output budgets; truncation is not a complete comparison. Remote references are unverified."}</p>{child.diff.entries.map((entry, index) => <article key={index}><EntryMetadata entry={entry.baseline} zh={zh} /><b>{states[entry.state][language]}</b><p>{zh ? "当前大小" : "Current size"}: {entry.current_size_bytes === null ? (zh ? "未知" : "Unknown") : `${entry.current_size_bytes} B`}</p><HashLabel hash={entry.current_hash} zh={zh} current /><p>{zh ? "行尾" : "Line endings"}: {flag(entry.line_endings_changed, zh)} · {zh ? "末尾换行" : "Final newline"}: {flag(entry.final_newline_changed, zh)}</p>{entry.hunks_truncated && <p role="status">{zh ? "差异已截断；比较不完整。" : "Diff truncated; comparison is incomplete."}</p>}{entry.hunks.map((hunk, h) => <pre key={h}>{`@@ -${hunk.old_start},${hunk.old_lines} +${hunk.new_start},${hunk.new_lines} @@\n`}{hunk.lines.map((line, i) => <span className={`baseline-line-${line.kind}`} key={i}>{`${line.kind === "added" ? "+" : line.kind === "removed" ? "-" : " "}${line.text}\n`}</span>)}</pre>)}</article>)}</>}
    </section></div>}
  </section></div>;
}
function flag(value: boolean | null, zh: boolean) { return value === null ? (zh ? "未知" : "Unknown") : value ? (zh ? "已变化" : "Changed") : (zh ? "未变化" : "Unchanged"); }
function HashLabel({ hash, zh, current = false }: { hash: FileReferenceHashV1 | null; zh: boolean; current?: boolean }) {
  const prefix = current ? (zh ? "当前" : "Current ") : (zh ? "保存的" : "Saved ");
  return <p>{hash ? `${hash.scope === "recorded_file" ? (zh ? "历史记录文件 SHA-256（未验证当前文件）" : "Recorded file SHA-256 (current file unverified)") : `${prefix}${zh ? "完整文件 SHA-256" : "full file SHA-256"}`}: ${hash.sha256}` : `${prefix}${zh ? "哈希：未知" : "hash: Unknown"}`}</p>;
}
function EntryMetadata({ entry, zh }: { entry: FileBaselineEntryV1; zh: boolean }) {
  return <><b>{entry.relative_path ?? (zh ? "路径已省略" : "Path withheld")}</b><p>{entry.backend_id} · {entry.captured_at}</p><p>{entry.content_mode === "text_snapshot" ? (zh ? "保存的正文" : "Text snapshot") : (zh ? "仅引用" : "Reference only")}</p><p>{zh ? "大小" : "Size"}: {entry.size_bytes === null ? (zh ? "未知" : "Unknown") : `${entry.size_bytes} B`}</p><HashLabel hash={entry.hash} zh={zh} />{entry.omission && <p>{omissions[entry.omission][zh ? 0 : 1]}</p>}</>;
}
