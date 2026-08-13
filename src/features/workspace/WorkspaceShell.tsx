import { Fragment, useEffect, useRef, useState } from "react";
import {
  Activity, ArrowLeft, Bot, Check, ChevronRight, Database, Expand, FileBarChart, FileText,
  FlaskConical, Folder, Languages, MessageSquarePlus, NotebookPen, Play,
  Search, Send, Settings, Sparkles, Square, X,
} from "lucide-react";
import { copy, type Locale } from "./copy";
import type { AgentRunStreamEvent, FormalStepProposal, KernelEvent, KernelLanguage, KernelSession, MemoryFact, NotebookEntry, PlanProposal, ProjectArtifact, ProjectImagePreview, SyncEntry, WorkspaceConversation } from "../../types";
import { RemoteFileTree } from "./RemoteFileTree";
import { KernelPanel } from "./KernelPanel";
import "./workspace.css";
import "./approval.css";
import "./file-actions.css";
import "./navigation.css";
import "./kernel.css";
import "./agent.css";
import "./preview.css";
import "./notebook.css";

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
  conversations?: WorkspaceConversation[];
  activeConversationId?: string | null;
  onSelectConversation?: (conversationId: string) => Promise<void> | void;
  onNewConversation?: () => Promise<void> | void;
  onSend?: (message: string) => Promise<boolean | void> | boolean | void;
  messages?: Array<{ id: string; role: "user" | "assistant" | "tool" | "system"; markdown: string; created_at?: string }>;
  streamingAssistant?: string;
  agentBusy?: boolean;
  agentNotice?: string;
  agentRetryNotice?: string;
  modelLabel?: string;
  planProposal?: PlanProposal | null;
  planLoading?: boolean;
  planApproved?: boolean;
  onRequestPlan?: () => Promise<void> | void;
  onApprovePlan?: () => Promise<void> | void;
  onStartRun?: () => Promise<void> | void;
  onCancelRun?: () => Promise<void> | void;
  runStopping?: boolean;
  canStartRun?: boolean;
  runStarted?: boolean;
  activeRunId?: string | null;
  agentRunEvents?: AgentRunStreamEvent[];
  remoteFiles?: import("../../types").RemoteFileEntry[];
  filesBusy?: boolean;
  onUploadFiles?: () => Promise<void> | void;
  onRefreshFiles?: () => Promise<void> | void;
  onDownloadFile?: (relativePath: string) => Promise<void> | void;
  onPreviewImage?: (relativePath: string) => Promise<ProjectImagePreview>;
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
  memoryFacts?: MemoryFact[];
  notebookEntries?: NotebookEntry[];
  projectArtifacts?: ProjectArtifact[];
  onSearchMemory?: (query: string, dimension?: string) => Promise<void> | void;
  onExportNotebook?: (format: "markdown" | "json" | "bundle") => Promise<void> | void;
  syncEntries?: SyncEntry[];
  onPauseSync?: (id: string) => Promise<void> | void;
  onCancelSync?: (id: string) => Promise<void> | void;
  onRetrySync?: (id: string) => Promise<void> | void;
}

type ContextTab = "files" | "preview" | "notebook" | "explore" | "runs";

export function WorkspaceShell({ project, locale, onLocaleChange, onOpenSettings, onBackToProjects, conversations = [], activeConversationId, onSelectConversation, onNewConversation, onSend, messages = [], streamingAssistant = "", agentBusy = false, agentNotice = "", agentRetryNotice = "", modelLabel, planProposal, planLoading = false, planApproved = false, onRequestPlan, onApprovePlan, onStartRun, onCancelRun, runStopping = false, canStartRun = false, runStarted = false, activeRunId, agentRunEvents = [], remoteFiles, filesBusy = false, onUploadFiles, onRefreshFiles, onDownloadFile, onPreviewImage, fileNotice, kernelSessions = [], kernelEvents = [], kernelBusy = false, kernelNotice, onStartKernel, onExecuteKernel, onInterruptKernel, onStopKernel, onPromoteKernelCell, memoryFacts = [], notebookEntries = [], projectArtifacts = [], onSearchMemory, onExportNotebook, syncEntries = [], onPauseSync, onCancelSync, onRetrySync }: Props) {
  const t = copy[locale];
  const zh = locale === "zh-CN";
  const [tab, setTab] = useState<ContextTab>("files");
  const [expanded, setExpanded] = useState(false);
  const [draft, setDraft] = useState("");
  const [sentMessages, setSentMessages] = useState<string[]>([]);
  const [approved, setApproved] = useState(false);
  const [selectedImagePath, setSelectedImagePath] = useState("");
  const [imagePreview, setImagePreview] = useState<ProjectImagePreview | null>(null);
  const [previewBusy, setPreviewBusy] = useState(false);
  const [previewError, setPreviewError] = useState("");
  const messageStreamRef = useRef<HTMLElement>(null);
  const imageFiles = (remoteFiles ?? []).filter((entry) => !entry.directory && isPreviewImage(entry.relative_path));
  const preview = <ArtifactPreview title={t.overview} locale={locale} images={imageFiles} selectedPath={selectedImagePath} preview={imagePreview} busy={previewBusy} error={previewError} onSelect={setSelectedImagePath} onLoad={loadImagePreview} />;
  const effectiveActiveRunId = activeRunId ?? (runStarted ? agentRunEvents.at(-1)?.run_id ?? null : null);
  const activeRunEvents = effectiveActiveRunId ? agentRunEvents.filter((event) => event.run_id === effectiveActiveRunId) : [];
  const runFinished = activeRunEvents.some((event) => event.kind === "agent_completed" || event.kind === "agent_failed" || event.kind === "agent_canceled");
  const runActive = runStarted && !runFinished;
  const activeConversation = conversations.find((item) => item.id === activeConversationId);
  const conversationTitle = activeConversation?.title || (zh ? "新会话" : "New conversation");
  const plannedSkills = planProposal?.plan.metadata.skill_citations ? parseSkillCitationSummary(planProposal.plan.metadata.skill_citations) : [];
  const historicalAgentRunEvents = effectiveActiveRunId
    ? agentRunEvents.filter((event) => event.run_id !== effectiveActiveRunId)
    : agentRunEvents;
  const activeRun = effectiveActiveRunId
    ? groupAgentRunEvents(activeRunEvents).find((run) => run.runId === effectiveActiveRunId) ?? null
    : null;
  const runTimeline = placeAgentRunsAfterMessages(messages, historicalAgentRunEvents);
  const latestAgentRunEvent = agentRunEvents.at(-1);

  useEffect(() => {
    const stream = messageStreamRef.current;
    if (!stream) return;
    const frame = requestAnimationFrame(() => {
      stream.scrollTop = stream.scrollHeight;
    });
    return () => cancelAnimationFrame(frame);
  }, [messages.length, streamingAssistant, agentBusy, agentNotice, planProposal, planLoading, runStarted, latestAgentRunEvent?.run_id, latestAgentRunEvent?.sequence]);

  useEffect(() => {
    if (selectedImagePath && imageFiles.some((entry) => entry.relative_path === selectedImagePath)) return;
    setSelectedImagePath(imageFiles[0]?.relative_path ?? "");
    setImagePreview(null);
    setPreviewError("");
  }, [remoteFiles, selectedImagePath]);

  async function loadImagePreview() {
    if (!selectedImagePath || !onPreviewImage) return;
    setPreviewBusy(true);
    setPreviewError("");
    try {
      setImagePreview(await onPreviewImage(selectedImagePath));
    } catch (error) {
      setImagePreview(null);
      setPreviewError(error instanceof Error ? error.message : String(error));
    } finally {
      setPreviewBusy(false);
    }
  }

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
      <div className="rail-section sessions"><span>{zh ? "会话" : "Sessions"}</span><div className="session-list">{conversations.map((item) => <button className={`session-row ${item.id === activeConversationId ? "active" : ""}`} key={item.id} aria-current={item.id === activeConversationId ? "page" : undefined} onClick={() => void onSelectConversation?.(item.id)}><Sparkles size={15} /><span title={item.title}>{item.title || (zh ? "新会话" : "New conversation")}</span></button>)}</div><button className="new-session" disabled={agentBusy} onClick={() => void onNewConversation?.()}><MessageSquarePlus size={15} />{t.newConversation}</button></div>
      <div className="rail-footer"><button onClick={() => onLocaleChange(zh ? "en-US" : "zh-CN")}><Languages size={16} />{zh ? "English" : "简体中文"}</button><button onClick={onOpenSettings}><Settings size={16} />{t.settings}</button></div>
    </nav>

    <main className="conversation-pane" aria-label={t.research}>
      <header className="conversation-header"><div><small>{project.name}</small><h1>{conversationTitle}</h1></div><span className="live-status"><i />{t.status}</span></header>
      <section className="message-stream" aria-live="polite" ref={messageStreamRef}>
        {!onSend && <article className="message user-message"><p>{zh ? "比较两批 PBMC，检查批次效应并生成可复现的分析报告。" : "Compare two PBMC batches, assess batch effects, and generate a reproducible report."}</p></article>}
        {messages.length === 0 && <article className="message assistant-message"><div className="assistant-avatar"><Bot size={17} /></div><div><strong>OmicsOps Agent</strong><p>{zh ? "描述你的研究目标。我会先核对数据与假设，并在任何正式执行前展示计划供你审批。" : "Describe your research goal. I will first check the data and assumptions, then show a plan for approval before any formal execution."}</p></div></article>}
        {sentMessages.map((message, index) => <article className="message user-message" key={`${index}-${message}`}><p>{message}</p></article>)}
        {messages.map((message) => <Fragment key={message.id}>{message.role === "user" ? <article className="message user-message"><p>{message.markdown}</p></article> : message.role === "assistant" ? <article className="message assistant-message"><div className="assistant-avatar"><Bot size={17} /></div><div><strong>OmicsOps Agent</strong><p>{message.markdown}</p></div></article> : null}{runTimeline.afterMessage.get(message.id)?.map((run) => <AgentRunFold locale={locale} run={run} activeRunId={effectiveActiveRunId} key={run.runId} />)}</Fragment>)}
        {streamingAssistant && <article className="message assistant-message"><div className="assistant-avatar"><Bot size={17} /></div><div><strong>OmicsOps Agent · {zh ? "生成中" : "streaming"}</strong><p>{streamingAssistant}</p></div></article>}
        {agentBusy && !streamingAssistant && <article className="message assistant-message agent-pending" role="status"><div className="assistant-avatar"><Bot size={17} /></div><div><strong>OmicsOps Agent</strong><p>{zh ? "正在等待模型响应…" : "Waiting for the model…"}</p></div></article>}
        {agentRetryNotice && <div className="agent-retry-notice" role="status"><span className="agent-working"><i />{agentRetryNotice}</span></div>}
        {agentNotice && <div className="agent-notice" role="alert"><strong>{zh ? "对话未完成" : "Conversation did not complete"}</strong><span>{agentNotice}</span></div>}
        {(planProposal || !onRequestPlan) && <article className="approval-card"><div className="task-icon"><Check size={18} /></div><div className="task-body"><div><strong>{planProposal?.plan.title ?? (zh ? "正式计划等待审批" : "Formal plan awaiting approval")}</strong><span>{runStarted ? (zh ? "运行已启动" : "Run started") : (planApproved || approved) ? (zh ? "已批准" : "Approved") : (planProposal?.validation.valid === false ? (zh ? "验证失败" : "Invalid") : (zh ? "需确认" : "Review"))}</span></div><p>{planProposal ? `${planProposal.plan.stages.reduce((count, stage) => count + stage.steps.length, 0)} ${zh ? "个版本化步骤" : "versioned steps"} · SHA-256 ${planProposal.plan_hash.slice(0, 12)}` : (zh ? "新增 5 个版本化步骤；将上传 2 个选定文件，不会同步整个工作区。" : "Adds 5 versioned steps; uploads 2 selected files and never mirrors the whole workspace.")}</p><div className="task-actions"><button>{zh ? "查看差异" : "View diff"}</button><button disabled={planApproved || approved || planProposal?.validation.valid === false} onClick={() => { if (onApprovePlan) void onApprovePlan(); else setApproved(true); }}>{(planApproved || approved) ? (zh ? "已批准" : "Approved") : onStartRun ? (zh ? "批准并开始远端运行" : "Approve and run remotely") : (zh ? "批准计划" : "Approve plan")}</button>{(planApproved || approved) && onStartRun && !runStarted && <button disabled={!canStartRun} onClick={() => void onStartRun()}>{canStartRun ? (zh ? "重新开始远端运行" : "Start remote run") : (zh ? "正在启动…" : "Starting…")}</button>}</div></div></article>}
        {plannedSkills.length > 0 && <div className="plan-skills"><b>{zh ? "计划采用的 Skills" : "Skills applied by this plan"}</b>{plannedSkills.map((skill) => <small key={`${skill.name}:${skill.version}:${skill.hash}`}>{skill.name}@{skill.version} · SHA-256 {skill.hash.slice(0, 12)} · {skill.sections} {zh ? "个引用段" : "cited sections"}</small>)}</div>}
        {runTimeline.unanchored.length > 0 && <AgentRunHistory locale={locale} runs={runTimeline.unanchored} activeRunId={effectiveActiveRunId} />}
        {activeRun && <AgentRunFold locale={locale} run={activeRun} activeRunId={effectiveActiveRunId} />}
        {runStarted && agentRunEvents.length === 0 && <AgentConversationUpdates locale={locale} events={[]} />}
        {runActive && onCancelRun && <div className="agent-run-controls" role="region" aria-label={zh ? "远程 Agent 运行控制" : "Remote agent run controls"}><div><span className="agent-working"><i />{runStopping ? (zh ? "正在终止当前操作…" : "Stopping current operation…") : (zh ? "远程 Agent 正在运行" : "Remote agent is running")}</span><small>{zh ? "将中断模型请求、当前 SSH 命令及后续操作" : "Stops the model request, current SSH command, and all subsequent actions"}</small></div><button className="stop-agent-button" disabled={runStopping} onClick={() => void onCancelRun()}><Square size={14} fill="currentColor" />{runStopping ? (zh ? "终止中…" : "Stopping…") : (zh ? "终止运行" : "Stop run")}</button></div>}
        {planProposal?.validation.valid === false && <div className="plan-validation" role="alert"><strong>{zh ? "计划未通过本地执行契约" : "Plan failed the local execution contract"}</strong><ul>{planProposal.validation.issues.map((issue) => <li key={`${issue.path}:${issue.code}`}><code>{issue.path}</code><span>{issue.message}</span></li>)}</ul><button disabled={planLoading} onClick={() => void onRequestPlan?.()}>{planLoading ? (zh ? "重新生成中…" : "Regenerating…") : (zh ? "按当前工具契约重新生成" : "Regenerate with current tool contract")}</button></div>}
        {!onSend && <article className="task-card"><div className="task-icon"><Activity size={18} /></div><div className="task-body"><div><strong>{t.task}</strong><span>65%</span></div><p>{zh ? "远端 Linux · 8 CPU · 32 GiB · 低风险" : "Remote Linux · 8 CPU · 32 GiB · low risk"}</p><div className="task-progress"><i /></div><div className="task-actions"><button>{zh ? "查看日志" : "View logs"}</button><button>{zh ? "查看计划" : "View plan"}</button></div></div></article>}
        {onSend && !planProposal && <article className="task-card planning-card"><div className="task-icon"><Activity size={18} /></div><div className="task-body"><div><strong>{zh ? "分析计划" : "Analysis plan"}</strong><span>{planLoading ? (zh ? "生成中" : "Generating") : (zh ? "尚未生成" : "Not generated")}</span></div><p>{zh ? "对话明确目标后，生成版本化计划并在远端执行前审批。" : "After the goal is clear, generate a versioned plan for approval before remote execution."}</p><div className="task-actions"><button disabled={planLoading || agentBusy} onClick={() => void onRequestPlan?.()}>{planLoading ? (zh ? "生成中…" : "Generating…") : (zh ? "生成分析计划" : "Generate plan")}</button></div></div></article>}
      </section>
      <footer className="composer"><div className="composer-input"><textarea aria-label={t.composer} placeholder={t.composer} value={draft} disabled={agentBusy} onChange={(event) => setDraft(event.target.value)} /><div><button className="composer-tool"><Folder size={16} /></button><button className="composer-tool"><Play size={16} /></button><button className="send-button" disabled={agentBusy || !draft.trim()} onClick={send}><Send size={16} />{agentBusy ? (zh ? "响应中…" : "Responding…") : t.send}</button></div></div><small>{modelLabel ? `${zh ? "当前模型" : "Model"}: ${modelLabel}` : (zh ? "发送前请在设置中配置模型提供方" : "Configure a model provider in Settings before sending")}</small></footer>
    </main>

    <aside className="context-pane" aria-label={t.context}>
      <div className="context-tabs" role="tablist">{(["files", "preview", "notebook", "explore", "runs"] as ContextTab[]).map((id) => <button key={id} role="tab" aria-selected={tab === id} onClick={() => setTab(id)}>{id === "files" ? t.files : id === "preview" ? t.preview : id === "notebook" ? t.notebook : id === "explore" ? t.explore : t.runs}</button>)}</div>
      <div className="context-content">{tab === "files" && <RemoteFileTree locale={locale} remoteFiles={remoteFiles} busy={filesBusy} notice={fileNotice} onUpload={onUploadFiles} onRefresh={onRefreshFiles} onDownload={onDownloadFile} syncEntries={syncEntries} onPauseSync={onPauseSync} onCancelSync={onCancelSync} onRetrySync={onRetrySync} />}{tab === "preview" && <><div className="context-toolbar"><span>{t.overview}</span><button aria-label={t.expand} onClick={() => setExpanded(true)}><Expand size={16} /></button></div>{preview}</>}{tab === "notebook" && <Notebook locale={locale} entries={notebookEntries} artifacts={projectArtifacts} facts={memoryFacts} onSearch={onSearchMemory} onExport={onExportNotebook} />}{tab === "explore" && <KernelPanel locale={locale} sessions={kernelSessions} events={kernelEvents} busy={kernelBusy} notice={kernelNotice} onStart={onStartKernel} onExecute={onExecuteKernel} onInterrupt={onInterruptKernel} onStop={onStopKernel} onPromote={onPromoteKernelCell} />}{tab === "runs" && <RunSummary locale={locale} />}</div>
    </aside>
    {expanded && <div className="preview-overlay" role="dialog" aria-modal="true" aria-label={t.artifactPreview}><header><div><small>{project.name}</small><h2>{t.overview}</h2></div><button aria-label="Close" onClick={() => setExpanded(false)}><X /></button></header>{preview}</div>}
  </div>;
}

function FileTree({ locale }: { locale: Locale }) { const zh = locale === "zh-CN"; return <div className="file-tree"><div className="context-heading"><b>{zh ? "项目文件" : "Project files"}</b><small>{zh ? "选择性同步" : "Selective sync"}</small></div><div className="tree-folder"><Folder size={15} />data <span>{zh ? "远端" : "remote"}</span></div><div className="tree-folder"><Folder size={15} />analysis</div><div className="tree-file"><FileBarChart size={15} />umap.png <em>1.2 MB</em></div><div className="tree-file"><FileText size={15} />markers.csv <em>84 KB</em></div><div className="tree-file"><NotebookPen size={15} />report.md <em>12 KB</em></div></div>; }
function isPreviewImage(path: string) { return /\.(png|jpe?g|gif|webp|bmp)$/i.test(path); }
function ArtifactPreview({ title, locale, images, selectedPath, preview, busy, error, onSelect, onLoad }: { title: string; locale: Locale; images: import("../../types").RemoteFileEntry[]; selectedPath: string; preview: ProjectImagePreview | null; busy: boolean; error: string; onSelect: (path: string) => void; onLoad: () => Promise<void> | void }) {
  const zh = locale === "zh-CN";
  return <div className="artifact-preview"><div className="preview-picker"><label>{zh ? "选择项目图片" : "Select project image"}<select aria-label={zh ? "选择项目图片" : "Select project image"} value={selectedPath} onChange={(event) => onSelect(event.target.value)}><option value="">{images.length ? (zh ? "请选择图片" : "Choose an image") : (zh ? "未发现图片文件" : "No image files found")}</option>{images.map((entry) => <option value={entry.relative_path} key={entry.relative_path}>{entry.relative_path}</option>)}</select></label><button disabled={!selectedPath || busy} onClick={() => void onLoad()}>{busy ? (zh ? "加载中…" : "Loading…") : (zh ? "显示图片" : "Show image")}</button></div>{error && <div className="preview-error" role="alert">{error}</div>}<div className="project-image-stage" aria-label={title}>{preview ? <img src={preview.data_url} alt={preview.relative_path} /> : <div className="preview-empty">{images.length ? (zh ? "选择图片后点击“显示图片”" : "Choose an image and click Show image") : (zh ? "项目中暂未发现 PNG、JPEG、GIF、WebP 或 BMP 图片" : "No PNG, JPEG, GIF, WebP, or BMP images were found")}</div>}</div><div className="artifact-meta"><b>{preview?.relative_path ?? title}</b>{preview && <><span>{preview.mime_type} · {formatPreviewBytes(preview.size_bytes)}</span><small>SHA-256 {preview.sha256}</small></>}</div></div>;
}
function formatPreviewBytes(bytes: number) { return bytes < 1024 * 1024 ? `${Math.max(1, Math.round(bytes / 1024))} KB` : `${(bytes / 1024 / 1024).toFixed(1)} MB`; }
function parseSkillCitationSummary(raw: string): Array<{ name: string; version: string; hash: string; sections: number }> { try { const citations = JSON.parse(raw) as Array<{ name: string; version: string; package_sha256: string }>; const grouped = new Map<string, { name: string; version: string; hash: string; sections: number }>(); citations.forEach((citation) => { const key = `${citation.name}:${citation.version}:${citation.package_sha256}`; const current = grouped.get(key); if (current) current.sections += 1; else grouped.set(key, { name: citation.name, version: citation.version, hash: citation.package_sha256, sections: 1 }); }); return [...grouped.values()]; } catch { return []; } }
interface AgentRunGroup { runId: string; events: AgentRunStreamEvent[] }

function groupAgentRunEvents(events: AgentRunStreamEvent[]): AgentRunGroup[] {
  const byRun = new Map<string, AgentRunStreamEvent[]>();
  events.forEach((event) => byRun.set(event.run_id, [...(byRun.get(event.run_id) ?? []), event]));
  return [...byRun].map(([runId, runEvents]) => ({ runId, events: runEvents.sort((left, right) => left.sequence - right.sequence) }));
}

function placeAgentRunsAfterMessages(messages: NonNullable<Props["messages"]>, events: AgentRunStreamEvent[]) {
  const timedMessages = messages.filter((message) => (message.role === "user" || message.role === "assistant") && message.created_at && Number.isFinite(Date.parse(message.created_at)));
  const afterMessage = new Map<string, AgentRunGroup[]>();
  const unanchored: AgentRunGroup[] = [];
  groupAgentRunEvents(events).forEach((run) => {
    const startedAt = Date.parse(run.events[0]?.timestamp ?? "");
    const anchor = Number.isFinite(startedAt) ? timedMessages.filter((message) => Date.parse(message.created_at!) <= startedAt).at(-1) : undefined;
    if (!anchor) unanchored.push(run);
    else afterMessage.set(anchor.id, [...(afterMessage.get(anchor.id) ?? []), run]);
  });
  return { afterMessage, unanchored };
}

function AgentRunHistory({ locale, runs, activeRunId }: { locale: Locale; runs: AgentRunGroup[]; activeRunId: string | null }) {
  return <section className="agent-run-history" aria-label={locale === "zh-CN" ? "Agent 运行历史" : "Agent run history"}>
    {runs.map((run) => <AgentRunFold locale={locale} run={run} activeRunId={activeRunId} key={run.runId} />)}
  </section>;
}

function AgentRunFold({ locale, run, activeRunId }: { locale: Locale; run: AgentRunGroup; activeRunId: string | null }) {
  const zh = locale === "zh-CN";
  const bodyRef = useRef<HTMLDivElement>(null);
  const terminal = [...run.events].reverse().find((event) => event.kind === "agent_completed" || event.kind === "agent_failed" || event.kind === "agent_canceled");
  const isActive = run.runId === activeRunId && !terminal;
  const status = terminal?.kind === "agent_completed" ? (zh ? "已完成" : "Completed") : terminal?.kind === "agent_failed" ? (zh ? "失败" : "Failed") : terminal?.kind === "agent_canceled" ? (zh ? "已终止" : "Canceled") : (zh ? "运行中" : "Running");
  const duration = formatAgentRunDuration(run.events, zh);
  const latestSequence = run.events.at(-1)?.sequence;
  useEffect(() => {
    if (!isActive || !bodyRef.current) return;
    const frame = requestAnimationFrame(() => {
      const body = bodyRef.current;
      if (body) body.scrollTop = body.scrollHeight;
    });
    return () => cancelAnimationFrame(frame);
  }, [isActive, latestSequence]);
  return <details id={`agent-run-${run.runId}`} className={`agent-run-fold ${isActive ? "is-active" : ""}`} aria-label={zh ? "Agent 运行" : "Agent run"} open={isActive ? true : undefined}>
    <summary><span className="agent-run-fold-title"><span><b>{isActive ? (zh ? "处理中" : "Processing") : (zh ? "已处理" : "Processed")} {duration}</b><small>{zh ? "Agent 与服务器交互" : "Agent and server interaction"}</small></span><ChevronRight size={15} /></span><span>{status} · {run.events.length} {zh ? "条流式事件" : "stream events"}</span></summary>
    <div className="agent-run-fold-body" ref={bodyRef}><AgentConversationUpdates locale={locale} events={run.events} /></div>
  </details>;
}

function formatAgentRunDuration(events: AgentRunStreamEvent[], zh: boolean) {
  const seconds = Math.max(0, Math.round((new Date(events.at(-1)?.timestamp ?? 0).getTime() - new Date(events[0]?.timestamp ?? 0).getTime()) / 1000));
  const minutes = Math.floor(seconds / 60);
  const rest = seconds % 60;
  if (zh) return minutes > 0 ? `${minutes}分 ${rest}秒` : `${rest}秒`;
  return minutes > 0 ? `${minutes}m ${rest}s` : `${rest}s`;
}

function AgentConversationUpdates({ locale, events }: { locale: Locale; events: AgentRunStreamEvent[] }) {
  const zh = locale === "zh-CN";
  const runInactive = events.some((event) => event.kind === "cancel_requested" || event.kind === "agent_canceled" || event.kind === "agent_completed" || event.kind === "agent_failed");
  if (events.length === 0) return <article className="message assistant-message agent-work-update" role="status">
    <div className="assistant-avatar"><Bot size={17} /></div><div><strong>OmicsOps Agent</strong><p>{zh ? "远程运行已启动。我正在连接服务器；模型的判断、执行的命令和服务器返回将持续显示在这里。" : "The remote run has started. I am connecting to the server; model decisions, commands, and server output will appear here as they happen."}</p><span className="agent-working"><i />{zh ? "正在连接" : "Connecting"}</span></div>
  </article>;
  return <>{events.map((event) => {
    const terminal = event.kind === "stdout" || event.kind === "stderr" || event.kind === "tool_started" || event.kind === "model_progress";
    const label = event.iteration ? (zh ? `第 ${event.iteration} 轮` : `Iteration ${event.iteration}`) : (zh ? "远程运行" : "Remote run");
    return <article className={`message assistant-message agent-work-update kind-${event.kind}`} aria-label={zh ? "远程 Agent 工作消息" : "Remote agent work update"} key={`${event.run_id}-${event.sequence}`}>
      <div className="assistant-avatar"><Bot size={17} /></div><div><div className="agent-work-heading"><strong>{agentEventSource(event.kind, zh)}</strong><small>{label} · #{event.sequence} · {new Date(event.timestamp).toLocaleTimeString()}</small></div><b className="agent-work-title">{event.title}</b>{terminal ? <pre>{event.content}</pre> : <p>{event.content}</p>}{!runInactive && (event.kind === "model_started" || event.kind === "model_waiting" || event.kind === "model_recovering" || event.kind === "tool_waiting" || event.kind === "ssh_reconnecting") && <span className="agent-working"><i />{event.kind === "ssh_reconnecting" ? (zh ? "正在恢复远程连接" : "Restoring the remote connection") : event.kind === "tool_waiting" ? (zh ? "远端命令仍在运行" : "The remote command is still running") : event.kind === "model_recovering" ? (zh ? "正在恢复模型请求" : "Recovering the model request") : (zh ? "模型仍在工作" : "The model is still working")}</span>}</div>
    </article>;
  })}</>;
}

function agentEventSource(kind: AgentRunStreamEvent["kind"], zh: boolean) {
  if (kind === "stdout") return zh ? "服务器 stdout" : "Server stdout";
  if (kind === "stderr") return zh ? "服务器 stderr" : "Server stderr";
  if (kind === "tool_started" || kind === "tool_waiting" || kind === "tool_completed" || kind === "tool_stopping" || kind === "tool_stopped") return zh ? "SSH 命令" : "SSH command";
  if (kind.startsWith("model_")) return zh ? "Agent 流式判断" : "Agent stream";
  return "OmicsOps Agent";
}

function Notebook({ locale, entries, artifacts, facts, onSearch, onExport }: { locale: Locale; entries: NotebookEntry[]; artifacts: ProjectArtifact[]; facts: MemoryFact[]; onSearch?: (query: string, dimension?: string) => Promise<void> | void; onExport?: (format: "markdown" | "json" | "bundle") => Promise<void> | void }) { const zh = locale === "zh-CN"; const [query, setQuery] = useState(""); const [dimension, setDimension] = useState(""); return <div className="notebook-panel"><div className="notebook-actions"><input aria-label={zh ? "检索记忆" : "Search memory"} value={query} onChange={(event) => setQuery(event.target.value)} placeholder={zh ? "按任务、环境或产物检索" : "Search tasks, environments, artifacts"} /><select aria-label={zh ? "记忆维度" : "Memory dimension"} value={dimension} onChange={(event) => setDimension(event.target.value)}><option value="">{zh ? "全部事实" : "All facts"}</option><option value="task">{zh ? "任务" : "Task"}</option><option value="environment">{zh ? "环境" : "Environment"}</option><option value="artifact">{zh ? "产物" : "Artifact"}</option></select><button onClick={() => void onSearch?.(query, dimension || undefined)}>{zh ? "检索" : "Search"}</button></div><div className="notebook-export"><button onClick={() => void onExport?.("markdown")}>Markdown</button><button onClick={() => void onExport?.("json")}>JSON</button><button onClick={() => void onExport?.("bundle")}>{zh ? "项目包" : "Bundle"}</button></div><div className="notebook-list">{entries.length === 0 && <div><span>{zh ? "研究记录" : "Notebook"}</span><b>{zh ? "暂无正式条目" : "No formal entries yet"}</b><p>{zh ? "Agent 完成并验证产物后会自动登记目标、方法、观察、决策与证据。" : "Verified Agent runs automatically register goals, methods, observations, decisions, and evidence."}</p></div>}{entries.map((entry) => <div key={entry.id}><span>{entry.kind}</span><b>{entry.title}</b><p>{entry.markdown}</p><small>{entry.evidence_ids.length} {zh ? "条可追溯引用" : "traceable references"}</small></div>)}</div><div className="artifact-register"><b>{zh ? "已登记产物" : "Registered artifacts"} · {artifacts.length}</b>{artifacts.map((artifact) => <small key={artifact.id}>{artifact.relative_path} · SHA-256 {artifact.sha256.slice(0, 12)}</small>)}</div><div className="memory-results"><b>{zh ? "事实记忆" : "Fact memory"} · {facts.length}</b>{facts.slice(0, 20).map((fact) => <article className={fact.conflicted_with.length ? "conflicted" : ""} key={fact.id}><span>{fact.dimension}</span><p>{fact.statement}</p><small>{fact.evidence.map((evidence) => `${evidence.source_kind}:${evidence.source_id}`).join(" · ")}</small>{fact.conflicted_with.length > 0 && <em>{zh ? "存在冲突事实，已保留双方来源" : "Conflicting fact retained with both sources"}</em>}</article>)}</div></div>; }
function RunSummary({ locale }: { locale: Locale }) { const zh = locale === "zh-CN"; return <div className="run-summary"><Activity size={24} /><b>{zh ? "运行中" : "Running"}</b><span>3 / 5 {zh ? "步骤已验证" : "steps verified"}</span><div className="task-progress"><i style={{ width: "60%" }} /></div></div>; }
