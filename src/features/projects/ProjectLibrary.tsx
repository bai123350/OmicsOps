import { useState } from "react";
import { BookOpen, ChevronRight, Dna, FilePlus2, FlaskConical, Languages, Library, Plus, Settings, Sparkles } from "lucide-react";
import type { WorkspaceProject, WorkspaceTemplate } from "../../types";
import type { Locale } from "../workspace/copy";
import "./project-library.css";

interface Props {
  projects: WorkspaceProject[];
  locale: Locale;
  onLocaleChange: (locale: Locale) => void;
  onCreate: (template: WorkspaceTemplate, name: string) => Promise<void>;
  onOpen: (project: WorkspaceProject) => void;
  onSettings: () => void;
}

const templates = [
  { id: "single_cell_rna_seq", zh: "单细胞 RNA 测序", en: "Single-cell RNA-seq", zhDescription: "QC、整合、聚类、标记基因与报告", enDescription: "QC, integration, clustering, markers, and report", icon: Dna },
  { id: "bulk_rna_seq", zh: "Bulk RNA 测序", en: "Bulk RNA-seq", zhDescription: "设计矩阵、差异表达、富集与图表", enDescription: "Design, differential expression, enrichment, and figures", icon: FlaskConical },
  { id: "literature_review", zh: "文献综述", en: "Literature review", zhDescription: "可溯源检索、证据提取与结构化写作", enDescription: "Traceable search, evidence extraction, and writing", icon: BookOpen },
  { id: "blank", zh: "空白研究项目", en: "Blank research project", zhDescription: "从自由对话、文件和远端环境开始", enDescription: "Start from conversation, files, and remote compute", icon: FilePlus2 },
] satisfies Array<{ id: WorkspaceTemplate; zh: string; en: string; zhDescription: string; enDescription: string; icon: typeof Dna }>;

export function ProjectLibrary({ projects, locale, onLocaleChange, onCreate, onOpen, onSettings }: Props) {
  const [creating, setCreating] = useState<WorkspaceTemplate | null>(null);
  const zh = locale === "zh-CN";
  return <div className="library-page">
    <header className="library-topbar"><div className="library-brand"><span><Sparkles size={19} /></span><b>OmicsOps</b></div><div><button onClick={() => onLocaleChange(zh ? "en-US" : "zh-CN")}><Languages size={16} />{zh ? "English" : "简体中文"}</button><button onClick={onSettings}><Settings size={16} />{zh ? "设置" : "Settings"}</button></div></header>
    <main className="library-main">
      <section className="library-hero"><span>LOCAL-FIRST · REMOTE COMPUTE</span><h1>{zh ? "生命科学项目" : "Life science projects"}</h1><p>{zh ? "在一个桌面工作区中组织研究问题、数据、分析、证据与报告。" : "Organize research questions, data, analysis, evidence, and reports in one desktop workspace."}</p></section>
      {projects.length > 0 && <section><div className="section-title"><h2>{zh ? "最近项目" : "Recent projects"}</h2></div><div className="recent-projects">{projects.map((project) => <button key={project.id} onClick={() => onOpen(project)}><span className="recent-icon"><Library size={19} /></span><span><b>{project.name}</b><small>{project.local_root}</small></span><ChevronRight size={17} /></button>)}</div></section>}
      <section><div className="section-title"><h2>{zh ? "创建新项目" : "Create a project"}</h2><p>{zh ? "选择引导模板，或从空白对话开始" : "Choose a guided template or start from a blank conversation"}</p></div><div className="template-grid">{templates.map(({ id, zh: nameZh, en, zhDescription, enDescription, icon: Icon }) => <button key={id} className="template-card" disabled={creating !== null} onClick={async () => { setCreating(id); await onCreate(id, zh ? nameZh : en); setCreating(null); }}><span className="template-icon"><Icon size={22} /></span><span><b>{zh ? nameZh : en}</b><small>{zh ? zhDescription : enDescription}</small></span><span className="template-arrow">{creating === id ? "…" : <Plus size={17} />}</span></button>)}</div></section>
    </main>
  </div>;
}
