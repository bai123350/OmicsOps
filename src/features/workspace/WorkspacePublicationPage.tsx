import { useCallback, useEffect, useRef, useState } from "react";
import { exportWorkspacePublication, getWorkspacePublication, listWorkspacePublications, restoreWorkspacePublication, saveWorkspacePublication } from "../../workspace-navigation-api";
import type { PublicationDetail, PublicationSummary, RestoreWorkspacePublicationRequest, SaveWorkspacePublicationRequest, WorkspaceSourceRef, WorkspaceSourceSnapshot } from "../../workspace-navigation-types";
import { JourneyBrowser, formatResearchDate, type WorkspaceResearchPagesProps } from "./WorkspaceResearchPages";
import { ResearchDialog, ResearchError, SnapshotView, SourceDialog, errorText, sourceKey, useAlive, useResearchList } from "./ResearchPagePrimitives";

type Draft = { title: string; markdown: string; sources: WorkspaceSourceRef[] };
type PendingWrite = { kind: "save"; request: SaveWorkspacePublicationRequest } | { kind: "restore"; request: RestoreWorkspacePublicationRequest };
const EMPTY_DRAFT: Draft = { title: "", markdown: "", sources: [] };

export function WorkspacePublicationPage({ projectId, zh, onOpenConversation, registerBeforeLeave }: WorkspaceResearchPagesProps & { zh: boolean }) {
  const publications = useResearchList<PublicationSummary>(projectId, async () => ({ items: await listWorkspacePublications(projectId), next: null }));
  const [detail, setDetail] = useState<PublicationDetail | null>(null);
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState<Draft>(EMPTY_DRAFT);
  const [baseline, setBaseline] = useState<Draft>(EMPTY_DRAFT);
  const [revision, setRevision] = useState<number | null>(null);
  const [error, setError] = useState("");
  const [loadError, setLoadError] = useState("");
  const failedPublication = useRef<string | null>(null);
  const [notice, setNotice] = useState("");
  const [loading, setLoading] = useState(false);
  const [busy, setBusy] = useState(false);
  const [picker, setPicker] = useState(false);
  const [snapshot, setSnapshot] = useState<WorkspaceSourceSnapshot | null>(null);
  const [source, setSource] = useState<WorkspaceSourceRef | null>(null);
  const [leave, setLeave] = useState<(() => void) | null>(null);
  const leaveNext = useRef<(() => void) | null>(null);
  const leaveCancellation = useRef<(() => void) | undefined>(undefined);
  const [pending, setPending] = useState<PendingWrite | null>(null);
  const pendingRef = useRef<PendingWrite | null>(null);
  const busyRef = useRef(false);
  const loadGeneration = useRef(0);
  const alive = useAlive();
  const dirty = editing && (JSON.stringify(draft) !== JSON.stringify(baseline) || pending !== null);
  const dirtyRef = useRef(dirty);
  dirtyRef.current = dirty;
  const guard = useCallback((next: () => void, onCancel?: () => void) => {
    const previousCancel = leaveCancellation.current;
    leaveCancellation.current = undefined;
    leaveNext.current = null;
    previousCancel?.();
    if (dirtyRef.current || busyRef.current) {
      leaveNext.current = next;
      leaveCancellation.current = onCancel;
      setLeave(() => next);
    }
    else next();
  }, []);
  useEffect(() => {
    registerBeforeLeave?.(guard);
    return () => { registerBeforeLeave?.((next) => next()); };
  }, [guard, registerBeforeLeave]);
  useEffect(() => () => {
    loadGeneration.current += 1;
    const cancel = leaveCancellation.current;
    leaveCancellation.current = undefined;
    cancel?.();
  }, []);

  const head = detail?.revisions.find((value) => value.revision === detail.publication.revision);
  const selected = detail?.revisions.find((value) => value.revision === revision);
  const readOnly = Boolean(selected && (selected.revision !== detail?.publication.revision || selected.legacy));
  const locked = busy || pending !== null;

  function accept(value: PublicationDetail, saved = false) {
    const current = value.revisions.find((item) => item.revision === value.publication.revision);
    const nextDraft = { title: current?.title ?? value.publication.title, markdown: current?.markdown ?? "", sources: current?.references.map((item) => item.source) ?? [] };
    setDetail(value); setEditing(true); setDraft(nextDraft); setBaseline(nextDraft); setRevision(value.publication.revision);
    dirtyRef.current = false;
    pendingRef.current = null; setPending(null); setError("");
    setNotice(saved ? (zh ? `版本 ${value.publication.revision} 已保存。` : `Revision ${value.publication.revision} saved.`) : "");
  }
  async function open(publicationId: string) {
    const ticket = ++loadGeneration.current;
    setLoading(true); setError(""); setLoadError(""); setNotice("");
    failedPublication.current = publicationId;
    try { const value = await getWorkspacePublication(projectId, publicationId); if (alive.current && ticket === loadGeneration.current) accept(value); }
    catch (cause) { if (alive.current && ticket === loadGeneration.current) setLoadError(errorText(cause)); }
    finally { if (alive.current && ticket === loadGeneration.current) setLoading(false); }
  }
  function create() {
    loadGeneration.current += 1;
    setDetail(null); setEditing(true); setDraft(EMPTY_DRAFT); setBaseline(EMPTY_DRAFT); setRevision(null); setError(""); setNotice(""); setLoading(false); pendingRef.current = null; setPending(null);
    setLoadError("");
  }
  function chooseRevision(value: number) {
    const version = detail?.revisions.find((item) => item.revision === value);
    if (!version) return;
    const next = { title: version.title, markdown: version.markdown, sources: version.references.map((item) => item.source) };
    setRevision(value); setDraft(next); setBaseline(next); setError(""); setNotice(""); pendingRef.current = null; setPending(null);
  }
  async function write(operation: "save" | "restore" = "save"): Promise<boolean> {
    if (busyRef.current) return false;
    let request = pendingRef.current;
    if (!request) {
      if (operation === "save") {
        if (!draft.title.trim()) { setError(zh ? "请填写稿件标题。" : "Enter a manuscript title."); return false; }
        request = { kind: "save", request: { request_id: crypto.randomUUID(), project_id: projectId, publication_id: detail?.publication.id ?? null, expected_revision: detail?.publication.revision ?? 0, title: draft.title.trim(), markdown: draft.markdown, sources: draft.sources } };
      } else {
        if (!detail || revision === null) return false;
        request = { kind: "restore", request: { request_id: crypto.randomUUID(), project_id: projectId, publication_id: detail.publication.id, expected_revision: detail.publication.revision, revision } };
      }
      pendingRef.current = request; setPending(request);
    }
    busyRef.current = true; setBusy(true); setError(""); setNotice("");
    try {
      const value = request.kind === "save" ? await saveWorkspacePublication(request.request) : await restoreWorkspacePublication(request.request);
      if (!alive.current) return false;
      accept(value, true); publications.reload(); return true;
    } catch (cause) {
      if (alive.current) {
        const message = errorText(cause);
        // These host validations run before a save can commit. Keep the draft
        // editable; every other error must replay the same uncertain write.
        const rejectedBeforeWrite = request.kind === "save" && (
          message === "Source no longer exists"
          || message === "Source content or identity has changed"
          || message.startsWith("Invalid source: ")
          || message === "At most 32 publication references may be saved"
          || message === "Publication reference belongs to another project"
          || /^invalid input: publication title (?:must contain 1\.\.200 characters|exceeds \d+ bytes)$/.test(message)
          || /^invalid input: publication text exceeds \d+ bytes$/.test(message)
        );
        if (rejectedBeforeWrite) { pendingRef.current = null; setPending(null); }
        setError(message);
      }
      return false;
    } finally { busyRef.current = false; if (alive.current) setBusy(false); }
  }
  async function exportRevision() {
    if (!detail || revision === null || busyRef.current) return;
    busyRef.current = true; setBusy(true); setError(""); setNotice("");
    try {
      const path = await exportWorkspacePublication(projectId, detail.publication.id, revision);
      if (alive.current) setNotice(path === null ? (zh ? "已取消导出。" : "Export cancelled.") : (zh ? `已导出至 ${path}` : `Exported to ${path}`));
    } catch (cause) { if (alive.current) setError(errorText(cause)); }
    finally { busyRef.current = false; if (alive.current) setBusy(false); }
  }
  function updateDraft(value: Partial<Draft>) { if (!locked && !readOnly) { setDraft((previous) => ({ ...previous, ...value })); setNotice(""); } }
  function discardAndLeave() {
    if (busy) return;
    const next = leaveNext.current;
    leaveNext.current = null; leaveCancellation.current = undefined;
    setDraft(baseline); pendingRef.current = null; dirtyRef.current = false; setPending(null); setLeave(null); next?.();
  }
  async function saveAndLeave() {
    if (await write()) { const next = leaveNext.current; leaveNext.current = null; leaveCancellation.current = undefined; setLeave(null); next?.(); }
  }
  function cancelLeave() {
    const cancel = leaveCancellation.current;
    leaveCancellation.current = undefined; leaveNext.current = null; setLeave(null); cancel?.();
  }

  return <div className="research-page-body">
    <p className="research-description">{zh ? "准备 Markdown 稿件，选择来源引用并保存独立版本。保存或导出不表示已投稿、已发表或已复现。" : "Prepare Markdown manuscripts, select source references and save independent revisions. Saving or exporting does not imply submission, publication or reproduction."}</p>
    <div className="research-publication-layout">
      <aside className="research-manuscript-list"><button type="button" className="research-primary" disabled={busy} onClick={() => guard(create)}>{zh ? "新建稿件" : "New manuscript"}</button>
        <ResearchError error={publications.error} retry={publications.retry} zh={zh} />
        {publications.loading && <p role="status">{zh ? "正在加载稿件…" : "Loading manuscripts…"}</p>}
        {publications.items.map((item) => <button type="button" key={item.id} aria-label={item.title} aria-current={detail?.publication.id === item.id ? "page" : undefined} disabled={busy} onClick={() => guard(() => void open(item.id))}><strong>{item.title}</strong><small>{zh ? "版本" : "Revision"} {item.revision} · {formatResearchDate(item.updated_at, zh)}</small></button>)}
        {!publications.loading && !publications.error && publications.items.length === 0 && <p className="research-note">{zh ? "还没有稿件。" : "No manuscripts yet."}</p>}
      </aside>
      <div className="research-manuscript-editor">
        {loading && <p role="status">{zh ? "正在加载稿件内容…" : "Loading manuscript…"}</p>}
        <ResearchError error={error} zh={zh} />
        <ResearchError error={loadError} retry={() => { const publicationId = failedPublication.current; if (publicationId) guard(() => void open(publicationId)); }} zh={zh} />
        {notice && <p className="research-notice" role="status">{notice}</p>}
        {!editing && !loading && <p className="research-empty">{zh ? "选择稿件或创建新稿件。" : "Select a manuscript or create a new one."}</p>}
        {editing && !loading && <>
          <div className="research-editor-toolbar">{detail && <label>{zh ? "版本历史" : "Version history"}<select aria-label={zh ? "版本历史" : "Version history"} value={revision ?? ""} disabled={busy} onChange={(event) => { const requestedRevision = Number(event.target.value); guard(() => chooseRevision(requestedRevision)); }}>{[...detail.revisions].sort((a, b) => b.revision - a.revision).map((item) => <option key={item.id} value={item.revision}>{zh ? "版本" : "Revision"} {item.revision} · {item.title}{item.legacy ? (zh ? "（旧版只读）" : " (legacy, read-only)") : ""}</option>)}</select></label>}
            <span className="research-note">{readOnly ? (zh ? "历史版本 · 只读" : "Historical revision · Read-only") : dirty ? (zh ? "有未保存修改" : "Unsaved changes") : (zh ? "稿件草稿" : "Manuscript draft")}</span>
          </div>
          {selected?.legacy && <p className="research-note">{zh ? "此版本为旧格式原文，以只读方式保留。恢复会生成新版本。" : "This legacy content is preserved as read-only text. Restoration creates a new revision."}</p>}
          <label className="research-editor-label">{zh ? "标题" : "Title"}<input aria-label={zh ? "标题" : "Title"} value={draft.title} maxLength={200} readOnly={readOnly || locked} onChange={(event) => updateDraft({ title: event.target.value })} /></label>
          <label className="research-editor-label research-markdown-label">Markdown<textarea aria-label="Markdown" spellCheck={false} value={draft.markdown} readOnly={readOnly || locked} onChange={(event) => updateDraft({ markdown: event.target.value })} placeholder={zh ? "在此编写研究背景、方法、结果和讨论…" : "Write background, methods, results and discussion…"} /></label>
          <section className="research-references"><div className="research-reference-header"><h3>{zh ? "来源引用" : "Source references"} ({draft.sources.length})</h3>{!readOnly && <button type="button" disabled={locked} onClick={() => setPicker(true)}>{zh ? "选择证据" : "Choose evidence"}</button>}</div>
            <p className="research-note">{zh ? "引用在保存时由宿主校验并生成快照。摘要不能替代原始执行证据。" : "The host validates references and creates snapshots when saved. Summaries do not replace original execution evidence."}</p>
            {draft.sources.map((ref) => {
              const saved = (readOnly ? selected : head)?.references.find((item) => sourceKey(item.source) === sourceKey(ref));
              return <div className="research-reference-row" key={sourceKey(ref)}><button type="button" onClick={() => saved ? setSnapshot(saved) : setSource(ref)}>{saved?.title ?? `${ref.kind}: ${ref.id}`}{saved && saved.availability !== "available" ? ` · ${saved.availability}` : ""}</button>{!readOnly && <button type="button" disabled={locked} aria-label={`${zh ? "移除引用" : "Remove reference"} ${ref.id}`} onClick={() => updateDraft({ sources: draft.sources.filter((item) => sourceKey(item) !== sourceKey(ref)) })}>×</button>}</div>;
            })}
          </section>
          {selected && <p className="research-hash">{zh ? "已保存版本 SHA-256" : "Saved revision SHA-256"}: {selected.sha256}</p>}
          {pending && !busy && <p className="research-note">{zh ? "保存结果尚未确认。重试使用同一请求和正文；版本冲突时可重新加载最新版本。" : "The save result is unconfirmed. Retry uses the same request and text; reload the latest revision to resolve a version conflict."}</p>}
          <div className="research-actions">
            {!readOnly && <button type="button" className="research-primary" disabled={busy} onClick={() => void write()}>{pending ? (zh ? "重试保存" : "Retry save") : (zh ? "保存版本" : "Save revision")}</button>}
            {readOnly && <button type="button" className="research-primary" disabled={busy} onClick={() => void write("restore")}>{pending ? (zh ? "重试恢复" : "Retry restore") : (zh ? "恢复此版本" : "Restore this revision")}</button>}
            {detail && <button type="button" disabled={busy} onClick={() => void exportRevision()}>{zh ? "导出 Markdown" : "Export Markdown"}</button>}
            {detail && <button type="button" disabled={busy} onClick={() => guard(() => void open(detail.publication.id))}>{zh ? "重新加载最新版本" : "Reload latest revision"}</button>}
          </div>
        </>}
      </div>
    </div>
    {picker && <ResearchDialog title={zh ? "选择证据" : "Choose evidence"} zh={zh} onClose={() => setPicker(false)}><JourneyBrowser projectId={projectId} zh={zh} onOpenConversation={(p, c) => guard(() => onOpenConversation(p, c))} selected={draft.sources} onSelect={(ref) => updateDraft({ sources: [...draft.sources, ref] })} /><div className="research-actions"><button type="button" onClick={() => setPicker(false)}>{zh ? "完成" : "Done"}</button></div></ResearchDialog>}
    {snapshot && <ResearchDialog title={zh ? "保存的引用" : "Saved reference"} zh={zh} onClose={() => setSnapshot(null)}><SnapshotView snapshot={snapshot} zh={zh} /></ResearchDialog>}
    {source && <SourceDialog key={sourceKey(source)} source={source} zh={zh} onClose={() => setSource(null)} onOpenConversation={(p, c) => guard(() => onOpenConversation(p, c))} />}
    {leave && <ResearchDialog title={zh ? "未保存的稿件" : "Unsaved manuscript"} zh={zh} onClose={cancelLeave} navigationDecision><p>{zh ? "稿件存在未保存或结果尚未确认的修改。离开前保存，或放弃这些修改。" : "The manuscript has unsaved or unconfirmed changes. Save before leaving, or discard these changes."}</p>{error && <ResearchError error={error} zh={zh} />}<div className="research-actions"><button type="button" className="research-primary" disabled={busy} onClick={() => void saveAndLeave()}>{zh ? "保存并离开" : "Save and leave"}</button><button type="button" disabled={busy} onClick={discardAndLeave}>{zh ? "放弃并离开" : "Discard and leave"}</button><button type="button" onClick={cancelLeave}>{zh ? "继续编辑" : "Keep editing"}</button></div></ResearchDialog>}
  </div>;
}
