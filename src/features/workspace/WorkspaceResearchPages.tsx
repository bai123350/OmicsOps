import { useState } from "react";
import { BookMarked, Clock3, FilePenLine } from "lucide-react";
import { deleteWorkspaceLibraryItem, getWorkspaceJourney, getWorkspaceLibraryItem, listWorkspaceLibrary } from "../../workspace-navigation-api";
import type { JourneyEntry, LibraryDetail, LibrarySourceProject, LibrarySummary, SourceKind, WorkspaceSourceRef } from "../../workspace-navigation-types";
import { ResearchDialog, ResearchError, SnapshotView, SourceDialog, errorText, sourceKey, useAlive, useResearchList } from "./ResearchPagePrimitives";
import { WorkspacePublicationPage } from "./WorkspacePublicationPage";
import "./workspace-research-pages.css";

export interface WorkspaceResearchPagesProps {
  page: "journey" | "publication" | "library";
  projectId: string;
  locale: string;
  onOpenConversation: (projectId: string, conversationId: string) => void;
  onInsert: (text: string) => void;
  registerBeforeLeave?: (guard: (next: () => void, onCancel?: () => void) => void) => void;
}

export function WorkspaceResearchPages(props: WorkspaceResearchPagesProps) {
  const zh = props.locale.startsWith("zh");
  const titles = { journey: zh ? "研究历程" : "Research journey", publication: zh ? "发表工作区" : "Publication workspace", library: zh ? "资料库" : "Personal library" };
  const Icon = { journey: Clock3, publication: FilePenLine, library: BookMarked }[props.page];
  return <section className="workspace-research-page" aria-label={titles[props.page]}>
    <header className="research-page-header"><span className="research-page-icon"><Icon size={22} /></span><div><small>OMICSOPS / {zh ? "科研工作区" : "RESEARCH WORKSPACE"}</small><h1>{titles[props.page]}</h1></div></header>
    {props.page === "journey" && <JourneyBrowser key={`journey:${props.projectId}`} {...props} zh={zh} />}
    {props.page === "publication" && <WorkspacePublicationPage key={`publication:${props.projectId}`} {...props} zh={zh} />}
    {props.page === "library" && <LibraryPage key={`library:${props.projectId}`} {...props} zh={zh} />}
  </section>;
}

const SOURCE_KINDS: SourceKind[] = ["run", "dataset", "analysis", "artifact", "evidence", "provenance", "legacy_artifact"];
const KIND_LABELS: Record<SourceKind, [string, string]> = { run: ["运行", "Runs"], dataset: ["数据集", "Datasets"], analysis: ["分析", "Analyses"], artifact: ["产物", "Artifacts"], evidence: ["证据", "Evidence"], provenance: ["溯源", "Provenance"], legacy_artifact: ["旧版产物", "Legacy artifacts"], message: ["消息", "Messages"], tool: ["工具", "Tools"] };

export function JourneyBrowser({ projectId, zh, onOpenConversation, onSelect, selected = [] }: { projectId: string; zh: boolean; onOpenConversation: (projectId: string, conversationId: string) => void; onSelect?: (source: WorkspaceSourceRef) => void; selected?: WorkspaceSourceRef[] }) {
  const [query, setQuery] = useState("");
  const [kind, setKind] = useState<SourceKind | "">("");
  const [status, setStatus] = useState("");
  const [source, setSource] = useState<WorkspaceSourceRef | null>(null);
  const list = useResearchList<JourneyEntry>(JSON.stringify([projectId, query, kind, status]), async (offset) => {
    const page = await getWorkspaceJourney({ project_id: projectId, query, kind: kind || null, status: status.trim() || null, offset, limit: 25 });
    return { items: page.entries, next: page.next_offset };
  });
  const entries = list.items;
  return <div className="research-page-body">
    <p className="research-description">{zh ? "浏览持久保存的运行、分析和证据。记录状态来自原始来源，不代表科研结论已验证。" : "Explore saved runs, analyses and evidence. Recorded statuses do not imply a verified scientific conclusion."}</p>
    <div className="research-filters">
      <input type="search" aria-label={zh ? "搜索研究历程" : "Search journey"} placeholder={zh ? "搜索记录…" : "Search records…"} value={query} onChange={(event) => setQuery(event.target.value)} />
      <select aria-label={zh ? "记录类型" : "Record type"} value={kind} onChange={(event) => { setKind(event.target.value as SourceKind | ""); setStatus(""); }}><option value="">{zh ? "全部类型" : "All types"}</option>{SOURCE_KINDS.map((value) => <option key={value} value={value}>{KIND_LABELS[value][zh ? 0 : 1]}</option>)}</select>
      <input aria-label={zh ? "记录状态" : "Record status"} placeholder={zh ? "完整状态，如 failed" : "Exact status, e.g. failed"} title={zh ? "按完整状态筛选全部记录，不区分大小写；留空显示全部状态。" : "Filter all records by exact status, ignoring case. Leave empty for all statuses."} value={status} onChange={(event) => setStatus(event.target.value)} />
    </div>
    <ResearchError error={list.error} retry={list.retry} zh={zh} />
    <div className="research-timeline">{entries.map((entry) => <article className="research-entry" key={sourceKey(entry.source)}>
      <span className="research-timeline-dot" />
      <div className="research-entry-main"><div className="research-badges"><span>{KIND_LABELS[entry.source.kind][zh ? 0 : 1]}</span><span>{entry.status}</span><time dateTime={entry.occurred_at}>{formatResearchDate(entry.occurred_at, zh)}</time></div>
        <button type="button" className="research-entry-open" onClick={() => setSource(entry.source)}><strong>{entry.title}</strong><span>{entry.summary}</span></button>
        <small>{entry.source.conversation_id ? `${zh ? "会话" : "Conversation"}: ${entry.source.conversation_id}` : (zh ? "来源会话未记录" : "Source conversation not recorded")}{entry.source.run_id ? ` · ${zh ? "运行" : "Run"}: ${entry.source.run_id}` : ""}</small>
      </div>
      {onSelect && <button type="button" disabled={selected.some((value) => sourceKey(value) === sourceKey(entry.source))} onClick={() => onSelect(entry.source)}>{selected.some((value) => sourceKey(value) === sourceKey(entry.source)) ? (zh ? "已选择" : "Selected") : (zh ? "添加引用" : "Add reference")}</button>}
    </article>)}</div>
    {!list.loading && !list.error && entries.length === 0 && <p className="research-empty">{zh ? "没有符合筛选条件的科研记录。" : "No research records match these filters."}</p>}
    {list.loading && <p role="status">{zh ? "正在加载记录…" : "Loading records…"}</p>}
    {list.next !== null && <button type="button" className="research-more" disabled={list.loading} onClick={list.more}>{zh ? "加载更多" : "Load more"}</button>}
    {source && <SourceDialog key={sourceKey(source)} source={source} zh={zh} onClose={() => setSource(null)} onOpenConversation={onOpenConversation} />}
  </div>;
}

function LibraryPage({ projectId, zh, onOpenConversation, onInsert }: WorkspaceResearchPagesProps & { zh: boolean }) {
  const [query, setQuery] = useState("");
  const [kind, setKind] = useState<LibrarySummary["kind"] | "">("");
  const [project, setProject] = useState("");
  const [selected, setSelected] = useState<string | null>(null);
  const list = useResearchList<LibrarySummary, LibrarySourceProject[]>(JSON.stringify([query, kind, project]), async (offset) => {
    const page = await listWorkspaceLibrary({ query, kind: kind || null, project_id: project || null, offset, limit: 25 });
    return { items: page.items, next: page.next_offset, meta: page.source_projects ?? [] };
  });
  const projects = list.meta ?? [];
  return <div className="research-page-body">
    <p className="research-description">{zh ? "跨项目保存的摘录、代码和产物引用。移除收藏不会删除来源，插入仅填入会话草稿。" : "Saved excerpts, code and artifact references across projects. Removing an item preserves its source; inserting only fills the conversation draft."}</p>
    <div className="research-filters">
      <input type="search" aria-label={zh ? "搜索资料库" : "Search library"} placeholder={zh ? "搜索收藏…" : "Search saved items…"} value={query} onChange={(event) => setQuery(event.target.value)} />
      <select aria-label={zh ? "收藏类型" : "Library type"} value={kind} onChange={(event) => setKind(event.target.value as typeof kind)}><option value="">{zh ? "全部类型" : "All types"}</option><option value="code">{zh ? "代码" : "Code"}</option><option value="excerpt">{zh ? "摘录" : "Excerpts"}</option><option value="artifact">{zh ? "产物引用" : "Artifact references"}</option></select>
      <select aria-label={zh ? "来源项目" : "Source project"} value={project} onChange={(event) => setProject(event.target.value)}><option value="">{zh ? "全部项目" : "All projects"}</option><option value={projectId}>{projects.find((item) => item.id === projectId)?.name || (zh ? "当前项目" : "Current project")}</option>{projects.filter((item) => item.id !== projectId).map((item) => <option value={item.id} key={item.id}>{item.name}</option>)}</select>
    </div>
    <ResearchError error={list.error} retry={list.retry} zh={zh} />
    <div className="research-library-grid">{list.items.map((item) => <button type="button" className="research-library-card" key={item.id} onClick={() => setSelected(item.id)}><span className="research-badges"><span>{item.kind === "artifact" ? (zh ? "产物引用" : "Artifact reference") : item.kind}</span></span><strong>{item.title}</strong><p>{item.text_preview}</p><small>{item.source_project_name} {item.source_conversation_title && ` / ${item.source_conversation_title}`}</small><time>{formatResearchDate(item.created_at, zh)}</time></button>)}</div>
    {!list.loading && !list.error && list.items.length === 0 && <p className="research-empty">{zh ? "没有符合筛选条件的收藏。" : "No saved items match these filters."}</p>}
    {list.loading && <p role="status">{zh ? "正在加载收藏…" : "Loading saved items…"}</p>}
    {list.next !== null && <button type="button" className="research-more" disabled={list.loading} onClick={list.more}>{zh ? "加载更多" : "Load more"}</button>}
    {selected && <LibraryItemDialog key={selected} itemId={selected} zh={zh} onClose={() => setSelected(null)} onRemoved={() => { setSelected(null); list.reload(); }} onInsert={onInsert} onOpenConversation={onOpenConversation} />}
  </div>;
}

function LibraryItemDialog({ itemId, zh, onClose, onRemoved, onInsert, onOpenConversation }: { itemId: string; zh: boolean; onClose: () => void; onRemoved: () => void; onInsert: (text: string) => void; onOpenConversation: (projectId: string, conversationId: string) => void }) {
  const list = useResearchList<LibraryDetail>(itemId, async () => ({ items: [await getWorkspaceLibraryItem(itemId)], next: null }));
  const detail = list.items[0];
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const [inserted, setInserted] = useState(false);
  const alive = useAlive();
  async function remove() {
    if (busy) return;
    setBusy(true); setError("");
    try { await deleteWorkspaceLibraryItem(itemId); if (alive.current) onRemoved(); }
    catch (cause) { if (alive.current) setError(errorText(cause)); }
    finally { if (alive.current) setBusy(false); }
  }
  return <ResearchDialog title={zh ? "收藏详情" : "Library details"} zh={zh} onClose={onClose}>
    {list.loading && <p role="status">{zh ? "正在加载…" : "Loading…"}</p>}
    <ResearchError error={list.error} retry={list.retry} zh={zh} />
    {detail && <><SnapshotView snapshot={detail.snapshot} zh={zh} /><p className="research-note">{detail.item.source_project_name}{detail.item.source_conversation_title && ` / ${detail.item.source_conversation_title}`}</p><div className="research-actions">
      <button type="button" disabled={!detail.item.source_conversation_id || detail.snapshot.availability === "missing" || detail.snapshot.availability === "unavailable"} onClick={() => { if (detail.item.source_conversation_id) onOpenConversation(detail.item.source_project_id, detail.item.source_conversation_id); }}>{zh ? "打开来源会话" : "Open source conversation"}</button>
      <button type="button" onClick={() => { onInsert(detail.snapshot.text); setInserted(true); }}>{zh ? "插入会话草稿" : "Insert into conversation draft"}</button>
      <button type="button" className="research-danger" disabled={busy} onClick={() => void remove()}>{zh ? "移除收藏" : "Remove from library"}</button>
    </div><p className="research-note">{zh ? "移除仅删除此收藏，原始消息与文件保持不变。" : "Removal affects this saved item only; original messages and files remain."}</p></>}
    <ResearchError error={error} retry={() => void remove()} zh={zh} />
    {inserted && <p role="status">{zh ? "已插入草稿，尚未发送。" : "Inserted into draft; no message was sent."}</p>}
  </ResearchDialog>;
}

export function formatResearchDate(value: string, zh: boolean): string {
  const date = new Date(value);
  return Number.isNaN(date.getTime()) ? (zh ? "时间未记录" : "Time not recorded") : date.toLocaleString(zh ? "zh-CN" : "en-US");
}
