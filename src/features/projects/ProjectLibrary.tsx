import { useId, useMemo, useState } from "react";
import { BookOpen, ChevronRight, Dna, FilePlus2, FlaskConical, FolderOpen, HardDrive, Languages, Library, Plus, Search, Server, Settings, Sparkles, Trash2, X } from "lucide-react";
import type { ConnectionProfile, WorkspaceProject, WorkspaceTemplate } from "../../types";
import type { Locale } from "../workspace/copy";
import { useWindowEscapeLayer } from "../settings/BrowserSettings";
import type { ProjectDirectoryChoice } from "../../general-settings-api";
import "./project-library.css";
import "./create-project.css";

interface CreateProjectOptions {
  template: WorkspaceTemplate;
  name: string;
  localRoot: string;
  connectionId: string | null;
  remoteRoot: string | null;
}

interface Props {
  projects: WorkspaceProject[];
  connections?: ConnectionProfile[];
  locale: Locale;
  onLocaleChange: (locale: Locale) => void;
  onChooseLocalRoot: () => Promise<ProjectDirectoryChoice>;
  onCreate: (options: CreateProjectOptions) => Promise<void>;
  onOpen: (project: WorkspaceProject) => void;
  onDelete: (projectId: string) => Promise<void>;
  onSettings: () => void;
  onOpenSearch?: () => void;
}

const templates = [
  { id: "single_cell_rna_seq", zh: "单细胞 RNA 测序", en: "Single-cell RNA-seq", zhDescription: "QC、整合、聚类、标记基因与报告", enDescription: "QC, integration, clustering, markers, and report", icon: Dna },
  { id: "bulk_rna_seq", zh: "Bulk RNA 测序", en: "Bulk RNA-seq", zhDescription: "设计矩阵、差异表达、富集与图表", enDescription: "Design, differential expression, enrichment, and figures", icon: FlaskConical },
  { id: "literature_review", zh: "文献综述", en: "Literature review", zhDescription: "可溯源检索、证据提取与结构化写作", enDescription: "Traceable search, evidence extraction, and writing", icon: BookOpen },
  { id: "blank", zh: "空白研究项目", en: "Blank research project", zhDescription: "从自由对话、文件和远端环境开始", enDescription: "Start from conversation, files, and remote compute", icon: FilePlus2 },
] satisfies Array<{ id: WorkspaceTemplate; zh: string; en: string; zhDescription: string; enDescription: string; icon: typeof Dna }>;

export function ProjectLibrary({ projects, connections = [], locale, onLocaleChange, onChooseLocalRoot, onCreate, onOpen, onDelete, onSettings, onOpenSearch }: Props) {
  const zh = locale === "zh-CN";
  const projectDescriptionPrefix = useId();
  const trustedConnections = useMemo(() => connections.filter((connection) => connection.host_key_fingerprint), [connections]);
  const [template, setTemplate] = useState<WorkspaceTemplate | null>(null);
  const [name, setName] = useState("");
  const [localRoot, setLocalRoot] = useState("");
  const [compute, setCompute] = useState<"local" | "remote">("local");
  const [connectionId, setConnectionId] = useState("");
  const [remoteRoot, setRemoteRoot] = useState("");
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState("");
  const [directoryNotice, setDirectoryNotice] = useState("");
  const [deletingProjectId, setDeletingProjectId] = useState<string | null>(null);
  const [deleteError, setDeleteError] = useState("");
  useWindowEscapeLayer(template !== null, () => { if (!submitting) setTemplate(null); });
  const selectedConnection = connections.find((connection) => connection.id === connectionId);

  function beginCreate(id: WorkspaceTemplate, initialName: string) {
    setTemplate(id); setName(initialName); setLocalRoot(""); setRemoteRoot(""); setError(""); setDirectoryNotice("");
    const firstTrusted = trustedConnections[0];
    setCompute(firstTrusted ? "remote" : "local");
    setConnectionId(firstTrusted?.id ?? "");
  }

  async function create() {
    if (!template || !name.trim() || !localRoot) return;
    setSubmitting(true); setError("");
    try {
      await onCreate({ template, name: name.trim(), localRoot, connectionId: compute === "remote" ? connectionId : null, remoteRoot: compute === "remote" ? remoteRoot : null });
      setTemplate(null);
    } catch (reason) { setError(reason instanceof Error ? reason.message : String(reason)); }
    finally { setSubmitting(false); }
  }

  async function deleteProject(project: WorkspaceProject) {
    const confirmed = window.confirm(zh
      ? `确定删除项目“${project.name}”吗？\n\nOmicsOps 中的会话、运行记录、Notebook 和索引将被清理。\n本地目录 ${project.local_root} 及远端文件不会被删除。\n\n此操作无法撤销。`
      : `Delete project “${project.name}”?\n\nIts OmicsOps conversations, run history, notebook, and indexes will be removed.\nThe local folder ${project.local_root} and remote files will not be deleted.\n\nThis cannot be undone.`);
    if (!confirmed) return;
    setDeletingProjectId(project.id);
    setDeleteError("");
    try {
      await onDelete(project.id);
    } catch (reason) {
      setDeleteError(reason instanceof Error ? reason.message : String(reason));
    } finally {
      setDeletingProjectId(null);
    }
  }

  return <div className="library-page">
    <header className="library-topbar">
      <div className="library-topbar-inner">
        <div className="library-identity">
          <div className="library-brand"><span aria-hidden="true"><Sparkles size={18} /></span><b>OmicsOps</b></div>
          <p>{zh ? "面向可复现科研的本地工作台" : "A local workspace for reproducible research"}</p>
        </div>
        <nav className="library-actions" aria-label={zh ? "首页操作" : "Home actions"}>
          {onOpenSearch && <button type="button" className="library-search-action" aria-label={zh ? "搜索工作区" : "Search workspace"} onClick={onOpenSearch}><Search size={15} /><span>{zh ? "搜索工作区" : "Search workspace"}</span><kbd>Ctrl+K</kbd></button>}
          <button type="button" aria-label={zh ? "切换为 English" : "Switch to 简体中文"} onClick={() => onLocaleChange(zh ? "en-US" : "zh-CN")}><Languages size={15} /><span>{zh ? "English" : "简体中文"}</span></button>
          <button type="button" onClick={onSettings}><Settings size={15} /><span>{zh ? "设置" : "Settings"}</span></button>
        </nav>
      </div>
    </header>
    <main className="library-main">
      <div className="library-columns">
        <section className="library-column library-recent" aria-labelledby="recent-projects-heading">
          <div className="section-title"><div><span className="section-kicker">{zh ? "继续研究" : "Continue research"}</span><h1 id="recent-projects-heading">{zh ? "最近项目" : "Recent projects"}</h1></div>{projects.length > 0 && <span className="section-count" aria-label={zh ? `${projects.length} 个项目` : `${projects.length} projects`}>{projects.length}</span>}</div>
          {deleteError && <p className="project-delete-error" role="alert">{deleteError}</p>}
          {projects.length > 0 ? <div className="recent-projects">{projects.map((project, projectIndex) => {
            const connection = connections.find((item) => item.id === project.connection_id);
            const deleting = deletingProjectId === project.id;
            const localDescriptionId = `${projectDescriptionPrefix}-${projectIndex}-local`;
            const computeDescriptionId = `${projectDescriptionPrefix}-${projectIndex}-compute`;
            const projectDescriptionIds = `${localDescriptionId} ${computeDescriptionId}`;
            const remoteRootLabel = project.remote_root || (zh ? "未记录远端目录" : "Remote directory not recorded");
            const remoteLabel = connection ? `${connection.username}@${connection.host}:${connection.port} · ${remoteRootLabel}` : `${zh ? "远端连接未找到" : "Remote connection not found"} · ${remoteRootLabel}`;
            return <article className="recent-project-card" key={project.id}>
              <button type="button" className="recent-project-open" aria-label={zh ? `打开项目：${project.name}` : `Open project: ${project.name}`} aria-describedby={projectDescriptionIds} disabled={deletingProjectId !== null} onClick={() => onOpen(project)}>
                <span className="recent-icon" aria-hidden="true"><Library size={18} /></span>
                <span className="recent-project-copy"><b title={project.name}>{project.name}</b><small id={localDescriptionId} className="project-location local-path" title={project.local_root}><HardDrive size={11} aria-hidden="true" /><span>{project.local_root}</span></small>{project.connection_id ? <small id={computeDescriptionId} className={`project-location remote-target${connection ? "" : " remote-target-missing"}`} title={remoteLabel}><Server size={11} aria-hidden="true" /><span>{connection ? <>{connection.username}@{connection.host}:{connection.port} · {remoteRootLabel}</> : <><em>{zh ? "远端连接未找到" : "Remote connection not found"}</em> · {remoteRootLabel}</>}</span></small> : <small id={computeDescriptionId} className="project-location local-target"><HardDrive size={11} aria-hidden="true" /><span>{zh ? "仅本地，未启用远端计算" : "Local only; remote compute disabled"}</span></small>}</span>
                <ChevronRight className="recent-project-chevron" size={17} aria-hidden="true" />
              </button>
              <button type="button" className="recent-project-delete" aria-label={zh ? `删除项目：${project.name}` : `Delete project: ${project.name}`} aria-describedby={projectDescriptionIds} title={zh ? "删除项目" : "Delete project"} disabled={deletingProjectId !== null} onClick={() => void deleteProject(project)}>{deleting ? <span className="delete-spinner" aria-hidden="true">…</span> : <Trash2 size={16} aria-hidden="true" />}</button>
            </article>;
          })}</div> : <div className="library-empty-state"><span className="library-empty-icon" aria-hidden="true"><Library size={21} /></span><div><h2>{zh ? "还没有项目" : "No projects yet"}</h2><p>{zh ? "从空白项目开始，或选择一个研究模板。" : "Start from a blank project or choose a research template."}</p></div><button type="button" className="library-primary-action" onClick={() => beginCreate("blank", zh ? "空白研究项目" : "Blank research project")}><Plus size={15} aria-hidden="true" />{zh ? "创建空白项目" : "Create a blank project"}</button></div>}
        </section>
        <section className="library-column library-create" aria-labelledby="create-project-heading">
          <div className="section-title"><div><span className="section-kicker">{zh ? "开始研究" : "Start research"}</span><h2 id="create-project-heading">{zh ? "创建新项目" : "Create a project"}</h2><p>{zh ? "选择模板后确认本地存储位置和计算位置" : "Choose a template, then confirm storage and compute locations"}</p></div></div>
          <div className="template-grid">{templates.map(({ id, zh: nameZh, en, zhDescription, enDescription, icon: Icon }) => <button type="button" key={id} className="template-card" onClick={() => beginCreate(id, zh ? nameZh : en)}><span className="template-icon" aria-hidden="true"><Icon size={19} /></span><span className="template-copy"><b>{zh ? nameZh : en}</b><small>{zh ? zhDescription : enDescription}</small></span><span className="template-arrow" aria-hidden="true"><Plus size={16} /></span></button>)}</div>
        </section>
      </div>
    </main>
    {template && <div className="create-project-backdrop"><section className="create-project-dialog" role="dialog" aria-modal="true" aria-label={zh ? "创建新项目" : "Create project"}><header><div><small>{zh ? "本地工作区与计算位置" : "Workspace and compute location"}</small><h2>{zh ? "创建新项目" : "Create project"}</h2></div><button aria-label="Close" onClick={() => setTemplate(null)}><X size={18} /></button></header><div className="create-project-body">
      <label>{zh ? "项目名称" : "Project name"}<input aria-label={zh ? "项目名称" : "Project name"} value={name} onChange={(event) => setName(event.target.value)} /></label>
      <div className="location-card"><HardDrive size={20} /><div><b>{zh ? "本地工作区（必选）" : "Local workspace (required)"}</b><small>{zh ? "文献、笔记、脚本及选定产物保存在这里，不会自动全量上传。" : "Literature, notes, scripts, and selected artifacts live here and are never fully uploaded implicitly."}</small><code>{localRoot || (zh ? "尚未选择本地目录" : "No local folder selected")}</code>{directoryNotice && <small className="directory-fallback-notice" role="status">{directoryNotice}</small>}</div><button onClick={async () => { const choice = await onChooseLocalRoot(); if (choice.usedFallback) setDirectoryNotice(zh ? "保存的起始目录已不可用，文件夹选择器已使用系统默认位置。" : "The saved start folder is unavailable. The folder picker used the system default location."); if (choice.path) setLocalRoot(choice.path); }}><FolderOpen size={14} />{zh ? "选择目录" : "Choose"}</button></div>
      <fieldset className="compute-choice"><legend>{zh ? "分析在哪里运行？" : "Where should analyses run?"}</legend><label className={compute === "local" ? "active" : ""}><input type="radio" name="compute" checked={compute === "local"} onChange={() => setCompute("local")} /><HardDrive size={18} /><span><b>{zh ? "仅本地" : "Local only"}</b><small>{zh ? "创建本地工作区；SSH 分析和远端探索暂不可用。" : "Creates a local workspace; SSH analyses and remote exploration remain unavailable."}</small></span></label><label className={compute === "remote" ? "active" : ""}><input type="radio" name="compute" checked={compute === "remote"} onChange={() => setCompute("remote")} disabled={trustedConnections.length === 0} /><Server size={18} /><span><b>{zh ? "本地工作区 + 远端 Linux 计算" : "Local workspace + remote Linux compute"}</b><small>{trustedConnections.length ? (zh ? "只上传明确选择的文件，大型计算在服务器执行。" : "Only explicitly selected files are uploaded; large analyses run on the server.") : (zh ? "尚无已确认主机指纹的服务器，请先前往设置。" : "No trusted server is available; configure one in Settings first.")}</small></span></label></fieldset>
      {compute === "remote" && <div className="remote-create-fields"><label>{zh ? "远端服务器" : "Remote server"}<select aria-label={zh ? "远端服务器" : "Remote server"} value={connectionId} onChange={(event) => setConnectionId(event.target.value)}>{trustedConnections.map((connection) => <option key={connection.id} value={connection.id}>{connection.label} · {connection.username}@{connection.host}:{connection.port}</option>)}</select></label><label>{zh ? "服务器项目目录" : "Remote project directory"}<input aria-label={zh ? "服务器项目目录" : "Remote project directory"} placeholder="/home/user/omicsops/project" value={remoteRoot} onChange={(event) => setRemoteRoot(event.target.value)} /></label></div>}
      <div className="creation-summary"><b>{zh ? "创建摘要" : "Creation summary"}</b><span><HardDrive size={13} />{zh ? "存储" : "Storage"}: {localRoot || "—"}</span><span>{compute === "remote" && selectedConnection ? <><Server size={13} />{zh ? "计算" : "Compute"}: {selectedConnection.username}@{selectedConnection.host}:{selectedConnection.port}</> : <><HardDrive size={13} />{zh ? "计算" : "Compute"}: {zh ? "本机（不启用 SSH）" : "This computer (SSH disabled)"}</>}</span></div>
      {error && <p className="create-error" role="alert">{error}</p>}
    </div><footer><button onClick={() => setTemplate(null)}>{zh ? "取消" : "Cancel"}</button>{compute === "remote" && trustedConnections.length === 0 && <button onClick={onSettings}>{zh ? "配置服务器" : "Configure server"}</button>}<button className="primary" disabled={submitting || !name.trim() || !localRoot || (compute === "remote" && (!connectionId || !remoteRoot.startsWith("/")))} onClick={() => void create()}>{submitting ? "…" : (zh ? "创建项目" : "Create project")}</button></footer></section></div>}
  </div>;
}
