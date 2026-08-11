import { useMemo, useState } from "react";
import { BookOpen, ChevronRight, Dna, FilePlus2, FlaskConical, FolderOpen, HardDrive, Languages, Library, Plus, Server, Settings, Sparkles, X } from "lucide-react";
import type { ConnectionProfile, WorkspaceProject, WorkspaceTemplate } from "../../types";
import type { Locale } from "../workspace/copy";
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
  onChooseLocalRoot: () => Promise<string | null>;
  onCreate: (options: CreateProjectOptions) => Promise<void>;
  onOpen: (project: WorkspaceProject) => void;
  onSettings: () => void;
}

const templates = [
  { id: "single_cell_rna_seq", zh: "单细胞 RNA 测序", en: "Single-cell RNA-seq", zhDescription: "QC、整合、聚类、标记基因与报告", enDescription: "QC, integration, clustering, markers, and report", icon: Dna },
  { id: "bulk_rna_seq", zh: "Bulk RNA 测序", en: "Bulk RNA-seq", zhDescription: "设计矩阵、差异表达、富集与图表", enDescription: "Design, differential expression, enrichment, and figures", icon: FlaskConical },
  { id: "literature_review", zh: "文献综述", en: "Literature review", zhDescription: "可溯源检索、证据提取与结构化写作", enDescription: "Traceable search, evidence extraction, and writing", icon: BookOpen },
  { id: "blank", zh: "空白研究项目", en: "Blank research project", zhDescription: "从自由对话、文件和远端环境开始", enDescription: "Start from conversation, files, and remote compute", icon: FilePlus2 },
] satisfies Array<{ id: WorkspaceTemplate; zh: string; en: string; zhDescription: string; enDescription: string; icon: typeof Dna }>;

export function ProjectLibrary({ projects, connections = [], locale, onLocaleChange, onChooseLocalRoot, onCreate, onOpen, onSettings }: Props) {
  const zh = locale === "zh-CN";
  const trustedConnections = useMemo(() => connections.filter((connection) => connection.host_key_fingerprint), [connections]);
  const [template, setTemplate] = useState<WorkspaceTemplate | null>(null);
  const [name, setName] = useState("");
  const [localRoot, setLocalRoot] = useState("");
  const [compute, setCompute] = useState<"local" | "remote">("local");
  const [connectionId, setConnectionId] = useState("");
  const [remoteRoot, setRemoteRoot] = useState("");
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState("");
  const selectedConnection = connections.find((connection) => connection.id === connectionId);

  function beginCreate(id: WorkspaceTemplate, initialName: string) {
    setTemplate(id); setName(initialName); setLocalRoot(""); setRemoteRoot(""); setError("");
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

  return <div className="library-page">
    <header className="library-topbar"><div className="library-brand"><span><Sparkles size={19} /></span><b>OmicsOps</b></div><div><button onClick={() => onLocaleChange(zh ? "en-US" : "zh-CN")}><Languages size={16} />{zh ? "English" : "简体中文"}</button><button onClick={onSettings}><Settings size={16} />{zh ? "设置" : "Settings"}</button></div></header>
    <main className="library-main">
      <section className="library-hero"><span>LOCAL WORKSPACE · OPTIONAL REMOTE COMPUTE</span><h1>{zh ? "生命科学项目" : "Life science projects"}</h1><p>{zh ? "每个项目都保存在你选择的本地目录；大型分析可显式绑定可信远端 Linux。" : "Every project lives in a local folder you choose; large analyses can explicitly use a trusted remote Linux host."}</p></section>
      {projects.length > 0 && <section><div className="section-title"><h2>{zh ? "最近项目" : "Recent projects"}</h2></div><div className="recent-projects">{projects.map((project) => { const connection = connections.find((item) => item.id === project.connection_id); return <button key={project.id} onClick={() => onOpen(project)}><span className="recent-icon"><Library size={19} /></span><span><b>{project.name}</b><small><HardDrive size={11} />{project.local_root}</small><small className={connection ? "remote-target" : "local-target"}>{connection ? <><Server size={11} />{connection.username}@{connection.host}:{connection.port} · {project.remote_root}</> : <><HardDrive size={11} />{zh ? "仅本地，未启用远端计算" : "Local only; remote compute disabled"}</>}</small></span><ChevronRight size={17} /></button>; })}</div></section>}
      <section><div className="section-title"><h2>{zh ? "创建新项目" : "Create a project"}</h2><p>{zh ? "模板选择后会确认本地存储位置和计算位置" : "After choosing a template, confirm local storage and compute location"}</p></div><div className="template-grid">{templates.map(({ id, zh: nameZh, en, zhDescription, enDescription, icon: Icon }) => <button key={id} className="template-card" onClick={() => beginCreate(id, zh ? nameZh : en)}><span className="template-icon"><Icon size={22} /></span><span><b>{zh ? nameZh : en}</b><small>{zh ? zhDescription : enDescription}</small></span><span className="template-arrow"><Plus size={17} /></span></button>)}</div></section>
    </main>
    {template && <div className="create-project-backdrop"><section className="create-project-dialog" role="dialog" aria-modal="true" aria-label={zh ? "创建新项目" : "Create project"}><header><div><small>{zh ? "本地工作区与计算位置" : "Workspace and compute location"}</small><h2>{zh ? "创建新项目" : "Create project"}</h2></div><button aria-label="Close" onClick={() => setTemplate(null)}><X size={18} /></button></header><div className="create-project-body">
      <label>{zh ? "项目名称" : "Project name"}<input aria-label={zh ? "项目名称" : "Project name"} value={name} onChange={(event) => setName(event.target.value)} /></label>
      <div className="location-card"><HardDrive size={20} /><div><b>{zh ? "本地工作区（必选）" : "Local workspace (required)"}</b><small>{zh ? "文献、笔记、脚本及选定产物保存在这里，不会自动全量上传。" : "Literature, notes, scripts, and selected artifacts live here and are never fully uploaded implicitly."}</small><code>{localRoot || (zh ? "尚未选择本地目录" : "No local folder selected")}</code></div><button onClick={async () => { const root = await onChooseLocalRoot(); if (root) setLocalRoot(root); }}><FolderOpen size={14} />{zh ? "选择目录" : "Choose"}</button></div>
      <fieldset className="compute-choice"><legend>{zh ? "分析在哪里运行？" : "Where should analyses run?"}</legend><label className={compute === "local" ? "active" : ""}><input type="radio" name="compute" checked={compute === "local"} onChange={() => setCompute("local")} /><HardDrive size={18} /><span><b>{zh ? "仅本地" : "Local only"}</b><small>{zh ? "创建本地工作区；SSH 分析和远端探索暂不可用。" : "Creates a local workspace; SSH analyses and remote exploration remain unavailable."}</small></span></label><label className={compute === "remote" ? "active" : ""}><input type="radio" name="compute" checked={compute === "remote"} onChange={() => setCompute("remote")} disabled={trustedConnections.length === 0} /><Server size={18} /><span><b>{zh ? "本地工作区 + 远端 Linux 计算" : "Local workspace + remote Linux compute"}</b><small>{trustedConnections.length ? (zh ? "只上传明确选择的文件，大型计算在服务器执行。" : "Only explicitly selected files are uploaded; large analyses run on the server.") : (zh ? "尚无已确认主机指纹的服务器，请先前往设置。" : "No trusted server is available; configure one in Settings first.")}</small></span></label></fieldset>
      {compute === "remote" && <div className="remote-create-fields"><label>{zh ? "远端服务器" : "Remote server"}<select aria-label={zh ? "远端服务器" : "Remote server"} value={connectionId} onChange={(event) => setConnectionId(event.target.value)}>{trustedConnections.map((connection) => <option key={connection.id} value={connection.id}>{connection.label} · {connection.username}@{connection.host}:{connection.port}</option>)}</select></label><label>{zh ? "服务器项目目录" : "Remote project directory"}<input aria-label={zh ? "服务器项目目录" : "Remote project directory"} placeholder="/home/user/omicsops/project" value={remoteRoot} onChange={(event) => setRemoteRoot(event.target.value)} /></label></div>}
      <div className="creation-summary"><b>{zh ? "创建摘要" : "Creation summary"}</b><span><HardDrive size={13} />{zh ? "存储" : "Storage"}: {localRoot || "—"}</span><span>{compute === "remote" && selectedConnection ? <><Server size={13} />{zh ? "计算" : "Compute"}: {selectedConnection.username}@{selectedConnection.host}:{selectedConnection.port}</> : <><HardDrive size={13} />{zh ? "计算" : "Compute"}: {zh ? "本机（不启用 SSH）" : "This computer (SSH disabled)"}</>}</span></div>
      {error && <p className="create-error" role="alert">{error}</p>}
    </div><footer><button onClick={() => setTemplate(null)}>{zh ? "取消" : "Cancel"}</button>{compute === "remote" && trustedConnections.length === 0 && <button onClick={onSettings}>{zh ? "配置服务器" : "Configure server"}</button>}<button className="primary" disabled={submitting || !name.trim() || !localRoot || (compute === "remote" && (!connectionId || !remoteRoot.startsWith("/")))} onClick={() => void create()}>{submitting ? "…" : (zh ? "创建项目" : "Create project")}</button></footer></section></div>}
  </div>;
}
