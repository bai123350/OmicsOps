import { useEffect, useRef, useState } from "react";
import {
  Activity, ArrowLeft, Bot, Check, ChevronRight, Database, Expand, FileBarChart, FileText,
  FlaskConical, Folder, Languages, MessageSquarePlus, NotebookPen, Play,
  Search, Send, Settings, Sparkles, X,
} from "lucide-react";
import { copy, type Locale } from "./copy";
import type { FormalStepProposal, KernelEvent, KernelLanguage, KernelSession, PlanProposal } from "../../types";
import { RemoteFileTree } from "./RemoteFileTree";
import { KernelPanel } from "./KernelPanel";
import "./workspace.css";
import "./approval.css";
import "./file-actions.css";
import "./navigation.css";
import "./kernel.css";
import "./agent.css";

export interface WorkspaceProject {
  id: string;
  name: string;
  status: "ready" | "running" | "waiting_for_input" | "needs_attention" | "archived";
  template: "blank" | "single_cell_rna_seq" | "bulk_rna_seq" | "literature_review";
}

interface Props {
  project: WorkspaceProject;
  locale: Locale;
  onLocaleChange: (locale: Locale) => void;
  onOpenSettings?: () => void;
  onOpenHistory?: () => void;
  onBackToProjects?: () => void;
  onSend?: (message: string) => Promise<boolean | void> | boolean | void;
  messages?: Array<{ id: string; role: "user" | "assistant" | "tool" | "system"; markdown: string }>;
  streamingAssistant?: string;
  agentBusy?: boolean;
  agentNotice?: string;
  modelLabel?: string;
  planProposal?: PlanProposal | null;
  planLoading?: boolean;
  planApproved?: boolean;
  onRequestPlan?: () => Promise<void> | void;
  onApprovePlan?: () => Promise<void> | void;
  onStartRun?: () => Promise<void> | void;
  canStartRun?: boolean;
  runStarted?: boolean;
  remoteFiles?: import("../../types").RemoteFileEntry[];
  filesBusy?: boolean;
  onUploadFiles?: () => Promise<void> | void;
  onRefreshFiles?: () => Promise<void> | void;
  onDownloadFile?: (relativePath: string) => Promise<void> | void;
  fileNotice?: string;
  kernelSessions?: KernelSession[];
  kernelEvents?: KernelEvent[];
  kernelBusy?: boolean;
  kernelNotice?: string;
  onStartKernel?: (language: KernelLanguage, rebuildSessionId?: string) => Promise<void> | void;
  onExecuteKernel?: (sessionId: string, code: string, save: boolean, capturePaths: string[]) => Promise<number | null>;
  onInterruptKernel?: (sessionId: string) => Promise<void> | void;
  onStopKernel?: (sessionId: string) => Promise<void> | void;
  onPromoteKernelCell?: (sessionId: string, cellIndex: number, name: string) => Promise<FormalStepProposal>;
}

type ContextTab = "files" | "preview" | "notebook" | "explore" | "runs";

export function WorkspaceShell({ project, locale, onLocaleChange, onOpenSettings, onBackToProjects, onSend, messages = [], streamingAssistant = "", agentBusy = false, agentNotice = "", modelLabel, planProposal, planLoading = false, planApproved = false, onRequestPlan, onApprovePlan, onStartRun, canStartRun = false, runStarted = false, remoteFiles, filesBusy = false, onUploadFiles, onRefreshFiles, onDownloadFile, fileNotice, kernelSessions = [], kernelEvents = [], kernelBusy = false, kernelNotice, onStartKernel, onExecuteKernel, onInterruptKernel, onStopKernel, onPromoteKernelCell }: Props) {
  const t = copy[locale];
  const zh = locale === "zh-CN";
  const [tab, setTab] = useState<ContextTab>("files");
  const [expanded, setExpanded] = useState(false);
  const [draft, setDraft] = useState("");
  const [sentMessages, setSentMessages] = useState<string[]>([]);
  const [approved, setApproved] = useState(false);
  const messageStreamRef = useRef<HTMLElement>(null);
  const preview = <ArtifactPreview title={t.overview} />;

  useEffect(() => {
    const stream = messageStreamRef.current;
    if (!stream) return;
    const frame = requestAnimationFrame(() => {
      stream.scrollTop = stream.scrollHeight;
    });
    return () => cancelAnimationFrame(frame);
  }, [messages.length, streamingAssistant, agentBusy, agentNotice, planProposal, planLoading]);

  async function send() {
    const message = draft.trim();
    if (!message || agentBusy) return;
    setDraft("");
    if (onSend) {
      const accepted = await onSend(message);
      if (accepted === false) setDraft((current) => current || message);
    } else {
      setSentMessages((current) => [...current, message]);
    }
  }

  return <div className="science-shell">
    <nav className="project-rail" aria-label={t.projects}>
      <div className="science-brand"><span className="brand-orbit"><FlaskConical size={20} /></span><div><strong>OmicsOps</strong><small>Life Science Workspace</small></div></div>
      <button className="rail-home" onClick={onBackToProjects} aria-label={zh ? "返回项目主页" : "Back to project home"}><ArrowLeft size={15} />{zh ? "返回项目主页" : "Project home"}</button>
      <button className="rail-search"><Search size={15} />{zh ? "搜索项目" : "Search projects"}</button>
      <div className="rail-section"><span>{zh ? "项目" : "Projects"}</span><button className="project-row active"><span className="project-glyph"><Database size={16} /></span><span><strong>{project.name}</strong><small>{t.status}</small></span><ChevronRight size={14} /></button></div>
      <div className="rail-section sessions"><span>{zh ? "会话" : "Sessions"}</span><button className="session-row active"><Sparkles size={15} /><span>{zh ? "QC 与聚类" : "QC and clustering"}</span></button><button className="session-row"><FileText size={15} /><span>{zh ? "文献证据" : "Literature evidence"}</span></button><button className="session-row"><FileBarChart size={15} /><span>{zh ? "报告生成" : "Report drafting"}</span></button><button className="new-session"><MessageSquarePlus size={15} />{t.newConversation}</button></div>
      <div className="rail-footer"><button onClick={() => onLocaleChange(zh ? "en-US" : "zh-CN")}><Languages size={16} />{zh ? "English" : "简体中文"}</button><button onClick={onOpenSettings}><Settings size={16} />{t.settings}</button></div>
    </nav>

    <main className="conversation-pane" aria-label={t.research}>
      <header className="conversation-header"><div><small>{project.name}</small><h1>{zh ? "QC 与聚类" : "QC and clustering"}</h1></div><span className="live-status"><i />{t.status}</span></header>
      <section className="message-stream" aria-live="polite" ref={messageStreamRef}>
        {!onSend && <article className="message user-message"><p>{zh ? "比较两批 PBMC，检查批次效应并生成可复现的分析报告。" : "Compare two PBMC batches, assess batch effects, and generate a reproducible report."}</p></article>}
        {messages.length === 0 && <article className="message assistant-message"><div className="assistant-avatar"><Bot size={17} /></div><div><strong>OmicsOps Agent</strong><p>{zh ? "描述你的研究目标。我会先核对数据与假设，并在任何正式执行前展示计划供你审批。" : "Describe your research goal. I will first check the data and assumptions, then show a plan for approval before any formal execution."}</p></div></article>}
        {sentMessages.map((message, index) => <article className="message user-message" key={`${index}-${message}`}><p>{message}</p></article>)}
        {messages.map((message) => message.role === "user" ? <article className="message user-message" key={message.id}><p>{message.markdown}</p></article> : message.role === "assistant" ? <article className="message assistant-message" key={message.id}><div className="assistant-avatar"><Bot size={17} /></div><div><strong>OmicsOps Agent</strong><p>{message.markdown}</p></div></article> : null)}
        {streamingAssistant && <article className="message assistant-message"><div className="assistant-avatar"><Bot size={17} /></div><div><strong>OmicsOps Agent · {zh ? "生成中" : "streaming"}</strong><p>{streamingAssistant}</p></div></article>}
        {agentBusy && !streamingAssistant && <article className="message assistant-message agent-pending" role="status"><div className="assistant-avatar"><Bot size={17} /></div><div><strong>OmicsOps Agent</strong><p>{zh ? "正在等待模型响应…" : "Waiting for the model…"}</p></div></article>}
        {agentNotice && <div className="agent-notice" role="alert"><strong>{zh ? "对话未完成" : "Conversation did not complete"}</strong><span>{agentNotice}</span></div>}
        {(planProposal || !onRequestPlan) && <article className="approval-card"><div className="task-icon"><Check size={18} /></div><div className="task-body"><div><strong>{planProposal?.plan.title ?? (zh ? "正式计划等待审批" : "Formal plan awaiting approval")}</strong><span>{runStarted ? (zh ? "运行已启动" : "Run started") : (planApproved || approved) ? (zh ? "已批准" : "Approved") : (planProposal?.validation.valid === false ? (zh ? "验证失败" : "Invalid") : (zh ? "需确认" : "Review"))}</span></div><p>{planProposal ? `${planProposal.plan.stages.reduce((count, stage) => count + stage.steps.length, 0)} ${zh ? "个版本化步骤" : "versioned steps"} · SHA-256 ${planProposal.plan_hash.slice(0, 12)}` : (zh ? "新增 5 个版本化步骤；将上传 2 个选定文件，不会同步整个工作区。" : "Adds 5 versioned steps; uploads 2 selected files and never mirrors the whole workspace.")}</p><div className="task-actions"><button>{zh ? "查看差异" : "View diff"}</button><button disabled={planApproved || approved || planProposal?.validation.valid === false} onClick={() => { if (onApprovePlan) void onApprovePlan(); else setApproved(true); }}>{(planApproved || approved) ? (zh ? "已批准" : "Approved") : onStartRun ? (zh ? "批准并开始远端运行" : "Approve and run remotely") : (zh ? "批准计划" : "Approve plan")}</button>{(planApproved || approved) && onStartRun && !runStarted && <button disabled={!canStartRun} onClick={() => void onStartRun()}>{canStartRun ? (zh ? "重新开始远端运行" : "Start remote run") : (zh ? "正在启动…" : "Starting…")}</button>}</div></div></article>}
        {planProposal?.validation.valid === false && <div className="plan-validation" role="alert"><strong>{zh ? "计划未通过本地执行契约" : "Plan failed the local execution contract"}</strong><ul>{planProposal.validation.issues.map((issue) => <li key={`${issue.path}:${issue.code}`}><code>{issue.path}</code><span>{issue.message}</span></li>)}</ul><button disabled={planLoading} onClick={() => void onRequestPlan?.()}>{planLoading ? (zh ? "重新生成中…" : "Regenerating…") : (zh ? "按当前工具契约重新生成" : "Regenerate with current tool contract")}</button></div>}
        {!onSend && <article className="task-card"><div className="task-icon"><Activity size={18} /></div><div className="task-body"><div><strong>{t.task}</strong><span>65%</span></div><p>{zh ? "远端 Linux · 8 CPU · 32 GiB · 低风险" : "Remote Linux · 8 CPU · 32 GiB · low risk"}</p><div className="task-progress"><i /></div><div className="task-actions"><button>{zh ? "查看日志" : "View logs"}</button><button>{zh ? "查看计划" : "View plan"}</button></div></div></article>}
        {onSend && !planProposal && <article className="task-card planning-card"><div className="task-icon"><Activity size={18} /></div><div className="task-body"><div><strong>{zh ? "分析计划" : "Analysis plan"}</strong><span>{planLoading ? (zh ? "生成中" : "Generating") : (zh ? "尚未生成" : "Not generated")}</span></div><p>{zh ? "对话明确目标后，生成版本化计划并在远端执行前审批。" : "After the goal is clear, generate a versioned plan for approval before remote execution."}</p><div className="task-actions"><button disabled={planLoading || agentBusy} onClick={() => void onRequestPlan?.()}>{planLoading ? (zh ? "生成中…" : "Generating…") : (zh ? "生成分析计划" : "Generate plan")}</button></div></div></article>}
      </section>
      <footer className="composer"><div className="composer-input"><textarea aria-label={t.composer} placeholder={t.composer} value={draft} disabled={agentBusy} onChange={(event) => setDraft(event.target.value)} /><div><button className="composer-tool"><Folder size={16} /></button><button className="composer-tool"><Play size={16} /></button><button className="send-button" disabled={agentBusy || !draft.trim()} onClick={send}><Send size={16} />{agentBusy ? (zh ? "响应中…" : "Responding…") : t.send}</button></div></div><small>{modelLabel ? `${zh ? "当前模型" : "Model"}: ${modelLabel}` : (zh ? "发送前请在设置中配置模型提供方" : "Configure a model provider in Settings before sending")}</small></footer>
    </main>

    <aside className="context-pane" aria-label={t.context}>
      <div className="context-tabs" role="tablist">{(["files", "preview", "notebook", "explore", "runs"] as ContextTab[]).map((id) => <button key={id} role="tab" aria-selected={tab === id} onClick={() => setTab(id)}>{id === "files" ? t.files : id === "preview" ? t.preview : id === "notebook" ? t.notebook : id === "explore" ? t.explore : t.runs}</button>)}</div>
      <div className="context-content">{tab === "files" && <RemoteFileTree locale={locale} remoteFiles={remoteFiles} busy={filesBusy} notice={fileNotice} onUpload={onUploadFiles} onRefresh={onRefreshFiles} onDownload={onDownloadFile} />}{tab === "preview" && <><div className="context-toolbar"><span>{t.overview}</span><button aria-label={t.expand} onClick={() => setExpanded(true)}><Expand size={16} /></button></div>{preview}</>}{tab === "notebook" && <Notebook locale={locale} />}{tab === "explore" && <KernelPanel locale={locale} sessions={kernelSessions} events={kernelEvents} busy={kernelBusy} notice={kernelNotice} onStart={onStartKernel} onExecute={onExecuteKernel} onInterrupt={onInterruptKernel} onStop={onStopKernel} onPromote={onPromoteKernelCell} />}{tab === "runs" && <RunSummary locale={locale} />}</div>
    </aside>
    {expanded && <div className="preview-overlay" role="dialog" aria-modal="true" aria-label={t.artifactPreview}><header><div><small>{project.name}</small><h2>{t.overview}</h2></div><button aria-label="Close" onClick={() => setExpanded(false)}><X /></button></header>{preview}</div>}
  </div>;
}

function FileTree({ locale }: { locale: Locale }) { const zh = locale === "zh-CN"; return <div className="file-tree"><div className="context-heading"><b>{zh ? "项目文件" : "Project files"}</b><small>{zh ? "选择性同步" : "Selective sync"}</small></div><div className="tree-folder"><Folder size={15} />data <span>{zh ? "远端" : "remote"}</span></div><div className="tree-folder"><Folder size={15} />analysis</div><div className="tree-file"><FileBarChart size={15} />umap.png <em>1.2 MB</em></div><div className="tree-file"><FileText size={15} />markers.csv <em>84 KB</em></div><div className="tree-file"><NotebookPen size={15} />report.md <em>12 KB</em></div></div>; }
function ArtifactPreview({ title }: { title: string }) { return <div className="artifact-preview"><div className="umap-plot" aria-label={title}>{Array.from({ length: 32 }, (_, index) => <i key={index} style={{ "--x": `${12 + ((index * 29) % 75)}%`, "--y": `${14 + ((index * 43) % 68)}%`, "--c": index % 4 } as React.CSSProperties} />)}</div><div className="artifact-meta"><b>{title}</b><span>12 clusters · 5,842 cells</span><small>results/figures/umap.png · SHA-256 verified</small></div></div>; }
function Notebook({ locale }: { locale: Locale }) { const zh = locale === "zh-CN"; return <div className="notebook-list"><div><span>{zh ? "方法" : "Method"}</span><b>Scanpy QC</b><p>{zh ? "过滤低质量细胞并保留原始计数层。" : "Filtered low-quality cells while preserving raw counts."}</p></div><div><span>{zh ? "决策" : "Decision"}</span><b>Harmony</b><p>{zh ? "在 PCA 后进行批次校正。" : "Apply batch correction after PCA."}</p></div></div>; }
function RunSummary({ locale }: { locale: Locale }) { const zh = locale === "zh-CN"; return <div className="run-summary"><Activity size={24} /><b>{zh ? "运行中" : "Running"}</b><span>3 / 5 {zh ? "步骤已验证" : "steps verified"}</span><div className="task-progress"><i style={{ width: "60%" }} /></div></div>; }
