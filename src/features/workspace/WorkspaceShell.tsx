import { Fragment, useEffect, useRef, useState } from "react";
import {
  Activity, ArrowLeft, Bot, Check, ChevronRight, ClipboardList, Database, Expand, FileBarChart, FileText,
  FlaskConical, Folder, Languages, MessageSquarePlus, NotebookPen, Plus,
  Search, Send, Settings, Sparkles, Square, Trash2, X,
} from "lucide-react";
import { copy, type Locale } from "./copy";
import type { AgentRunEventV3, AgentRunEventV4, AgentRunStreamEvent, AutonomyModeV4, ComputeBackendAvailabilityV4, FormalStepProposal, KernelEvent, KernelLanguage, KernelSession, MemoryFact, NotebookEntry, PlanProposal, ProjectArtifact, ProjectImagePreview, RunSummaryV4, SyncEntry, WorkspaceConversation } from "../../types";
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
  onDeleteConversation?: (conversationId: string) => Promise<void> | void;
  onSend?: (message: string, mode: "chat" | "plan") => Promise<boolean | void> | boolean | void;
  messages?: Array<{ id: string; role: "user" | "assistant" | "tool" | "system"; markdown: string; created_at?: string }>;
  streamingAssistant?: string;
  agentBusy?: boolean;
  agentNotice?: string;
  agentRetryNotice?: string;
  modelLabel?: string;
  planProposal?: PlanProposal | null;
  v4Plan?: RunSummaryV4 | null;
  computeBackends?: ComputeBackendAvailabilityV4[];
  computeBackendId?: string;
  containerImage?: string;
  autonomyMode?: AutonomyModeV4;
  computeEnvironment?: string;
  computeBusy?: boolean;
  onComputeBackendChange?: (backendId: string) => void;
  onContainerImageChange?: (image: string) => void;
  onAutonomyModeChange?: (mode: AutonomyModeV4) => void;
  onComputeEnvironmentChange?: (environment: string) => void;
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
  agentRunEventsV3?: AgentRunEventV3[];
  agentRunEventsV4?: AgentRunEventV4[];
  onAnswerAgentQuestionV3?: (runId: string, questionId: string, answer: string) => Promise<void> | void;
  onAnswerAgentQuestionV4?: (runId: string, questionId: string, answer: string) => Promise<void> | void;
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

type ContextTab = "files" | "plan" | "preview" | "notebook" | "explore" | "runs";

export function WorkspaceShell({ project, locale, onLocaleChange, onOpenSettings, onBackToProjects, conversations = [], activeConversationId, onSelectConversation, onNewConversation, onDeleteConversation, onSend, messages = [], streamingAssistant = "", agentBusy = false, agentNotice = "", agentRetryNotice = "", modelLabel, planProposal, v4Plan, computeBackends = [], computeBackendId = "", containerImage = "", autonomyMode = "supervised", computeEnvironment = "system", computeBusy = false, onComputeBackendChange, onContainerImageChange, onAutonomyModeChange, onComputeEnvironmentChange, planLoading = false, planApproved = false, onRequestPlan, onApprovePlan, onStartRun, onCancelRun, runStopping = false, canStartRun = false, runStarted = false, activeRunId, agentRunEvents = [], agentRunEventsV3 = [], agentRunEventsV4 = [], onAnswerAgentQuestionV3, onAnswerAgentQuestionV4, remoteFiles, filesBusy = false, onUploadFiles, onRefreshFiles, onDownloadFile, onPreviewImage, fileNotice, kernelSessions = [], kernelEvents = [], kernelBusy = false, kernelNotice, onStartKernel, onExecuteKernel, onInterruptKernel, onStopKernel, onPromoteKernelCell, memoryFacts = [], notebookEntries = [], projectArtifacts = [], onSearchMemory, onExportNotebook, syncEntries = [], onPauseSync, onCancelSync, onRetrySync }: Props) {
  const t = copy[locale];
  const zh = locale === "zh-CN";
  const [tab, setTab] = useState<ContextTab>("files");
  const [expanded, setExpanded] = useState(false);
  const [draft, setDraft] = useState("");
  const [planModeEnabled, setPlanModeEnabled] = useState(false);
  const [planSessionActive, setPlanSessionActive] = useState(false);
  const [composerMenuOpen, setComposerMenuOpen] = useState(false);
  const [sentMessages, setSentMessages] = useState<string[]>([]);
  const [approved, setApproved] = useState(false);
  const [selectedImagePath, setSelectedImagePath] = useState("");
  const [imagePreview, setImagePreview] = useState<ProjectImagePreview | null>(null);
  const [previewBusy, setPreviewBusy] = useState(false);
  const [previewError, setPreviewError] = useState("");
  const [deletingConversationId, setDeletingConversationId] = useState<string | null>(null);
  const messageStreamRef = useRef<HTMLElement>(null);
  const imageFiles = (remoteFiles ?? []).filter((entry) => !entry.directory && isPreviewImage(entry.relative_path));
  const preview = <ArtifactPreview title={t.overview} locale={locale} images={imageFiles} selectedPath={selectedImagePath} preview={imagePreview} busy={previewBusy} error={previewError} onSelect={setSelectedImagePath} onLoad={loadImagePreview} />;
  const effectiveActiveRunId = activeRunId ?? (runStarted ? agentRunEventsV4.at(-1)?.run_id ?? agentRunEventsV3.at(-1)?.run_id ?? agentRunEvents.at(-1)?.run_id ?? null : null);
  const activeRunEvents = effectiveActiveRunId ? agentRunEvents.filter((event) => event.run_id === effectiveActiveRunId) : [];
  const activeRunEventsV3 = effectiveActiveRunId ? agentRunEventsV3.filter((event) => event.run_id === effectiveActiveRunId) : [];
  const activeRunEventsV4 = effectiveActiveRunId ? agentRunEventsV4.filter((event) => event.run_id === effectiveActiveRunId) : [];
  const runFinished = activeRunEvents.some((event) => event.kind === "agent_completed" || event.kind === "agent_failed" || event.kind === "agent_canceled") || activeRunEventsV3.some(isTerminalAgentEventV3) || activeRunEventsV4.some(isTerminalAgentEventV4);
  const runActive = runStarted && !runFinished;
  const activeConversation = conversations.find((item) => item.id === activeConversationId);
  const conversationTitle = activeConversation?.title || (zh ? "新会话" : "New conversation");
  const plannedSkills = planProposal?.plan.metadata.skill_citations ? parseSkillCitationSummary(planProposal.plan.metadata.skill_citations) : [];
  const historicalAgentRunEvents = effectiveActiveRunId
    ? agentRunEvents.filter((event) => event.run_id !== effectiveActiveRunId)
    : agentRunEvents;
  const historicalAgentRunEventsV3 = effectiveActiveRunId
    ? agentRunEventsV3.filter((event) => event.run_id !== effectiveActiveRunId)
    : agentRunEventsV3;
  const historicalAgentRunEventsV4 = effectiveActiveRunId
    ? agentRunEventsV4.filter((event) => event.run_id !== effectiveActiveRunId)
    : agentRunEventsV4;
  const activeRun = effectiveActiveRunId
    ? groupAgentRunEvents(activeRunEvents).find((run) => run.runId === effectiveActiveRunId) ?? null
    : null;
  const activeRunV3 = effectiveActiveRunId
    ? groupAgentRunEventsV3(activeRunEventsV3).find((run) => run.runId === effectiveActiveRunId) ?? null
    : null;
  const runTimeline = placeAgentRunsAfterMessages(messages, historicalAgentRunEvents);
  const runTimelineV3 = placeHarnessV3RunsAfterMessages(messages, historicalAgentRunEventsV3);
  const runTimelineV4 = placeV4RunsAfterMessages(messages, historicalAgentRunEventsV4);
  const latestAgentRunEvent = agentRunEvents.at(-1);
  const latestAgentRunEventV3 = agentRunEventsV3.at(-1);
  const latestAgentRunEventV4 = agentRunEventsV4.at(-1);
  const computeReady = computeBackends.some((item) => item.descriptor.backend_id === computeBackendId && item.selectable);
  const showPlanPanel = planModeEnabled || planSessionActive || planLoading || Boolean(v4Plan?.plan) || Boolean(planProposal);
  const executeStart = activeRunEventsV4.findIndex((event) => event.event.kind === "mode_changed" && event.event.mode === "execute");
  const visibleActiveRunEventsV4 = v4Plan?.plan && executeStart >= 0 ? activeRunEventsV4.slice(executeStart) : v4Plan?.plan ? [] : activeRunEventsV4;

  useEffect(() => { setPlanModeEnabled(false); setPlanSessionActive(false); }, [activeConversationId]);
  useEffect(() => { if (showPlanPanel) setTab("plan"); }, [showPlanPanel]);

  useEffect(() => {
    const stream = messageStreamRef.current;
    if (!stream) return;
    const frame = requestAnimationFrame(() => {
      stream.scrollTop = stream.scrollHeight;
    });
    return () => cancelAnimationFrame(frame);
  }, [messages.length, streamingAssistant, agentBusy, agentNotice, planProposal, planLoading, runStarted, latestAgentRunEvent?.run_id, latestAgentRunEvent?.sequence, latestAgentRunEventV3?.run_id, latestAgentRunEventV3?.sequence, latestAgentRunEventV4?.run_id, latestAgentRunEventV4?.sequence]);

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
      const mode = planModeEnabled ? "plan" : "chat";
      setPlanSessionActive(mode === "plan");
      const accepted = await onSend(message, mode);
      if (accepted === false) setDraft((current) => current || message);
      else if (planModeEnabled) setPlanModeEnabled(false);
    } else {
      setSentMessages((current) => [...current, message]);
    }
  }

  async function deleteConversation(item: WorkspaceConversation) {
    if (!onDeleteConversation) return;
    const title = item.title || (zh ? "新会话" : "New conversation");
    const confirmed = window.confirm(zh ? `确定删除会话“${title}”吗？此操作无法撤销。` : `Delete “${title}”? This cannot be undone.`);
    if (!confirmed) return;
    setDeletingConversationId(item.id);
    try {
      await onDeleteConversation(item.id);
    } finally {
      setDeletingConversationId(null);
    }
  }

  return <div className="science-shell">
    <nav className="project-rail" aria-label={t.projects}>
      <div className="science-brand"><span className="brand-orbit"><FlaskConical size={20} /></span><div><strong>OmicsOps</strong><small>Life Science Workspace</small></div></div>
      <button className="rail-home" onClick={onBackToProjects} aria-label={zh ? "返回项目主页" : "Back to project home"}><ArrowLeft size={15} />{zh ? "返回项目主页" : "Project home"}</button>
      <button className="rail-search"><Search size={15} />{zh ? "搜索项目" : "Search projects"}</button>
      <div className="rail-section"><span>{zh ? "项目" : "Projects"}</span><button className="project-row active"><span className="project-glyph"><Database size={16} /></span><span><strong>{project.name}</strong><small>{t.status}</small></span><ChevronRight size={14} /></button></div>
      <div className="rail-section sessions"><span>{zh ? "会话" : "Sessions"}</span><div className="session-list">{conversations.map((item) => { const title = item.title || (zh ? "新会话" : "New conversation"); const active = item.id === activeConversationId; const deleteDisabled = deletingConversationId !== null || (active && (agentBusy || runActive)); return <div className={`session-entry ${active ? "active" : ""}`} key={item.id}><button className="session-row" aria-current={active ? "page" : undefined} onClick={() => void onSelectConversation?.(item.id)}><Sparkles size={15} /><span title={item.title}>{title}</span></button><button className="session-delete" aria-label={zh ? `删除会话：${title}` : `Delete conversation: ${title}`} title={zh ? "删除会话" : "Delete conversation"} disabled={deleteDisabled || !onDeleteConversation} onClick={() => void deleteConversation(item)}><Trash2 size={14} /></button></div>; })}</div><button className="new-session" disabled={agentBusy} onClick={() => void onNewConversation?.()}><MessageSquarePlus size={15} />{t.newConversation}</button></div>
      <div className="rail-footer"><button onClick={() => onLocaleChange(zh ? "en-US" : "zh-CN")}><Languages size={16} />{zh ? "English" : "简体中文"}</button><button onClick={onOpenSettings}><Settings size={16} />{t.settings}</button></div>
    </nav>

    <main className="conversation-pane" aria-label={t.research}>
      <header className="conversation-header"><div><small>{project.name}</small><h1>{conversationTitle}</h1></div><span className="live-status"><i />{t.status}</span></header>
      <section className="message-stream" aria-live="polite" ref={messageStreamRef}>
        {!onSend && <article className="message user-message"><p>{zh ? "比较两批 PBMC，检查批次效应并生成可复现的分析报告。" : "Compare two PBMC batches, assess batch effects, and generate a reproducible report."}</p></article>}
        {messages.length === 0 && <article className="message assistant-message"><div className="assistant-avatar"><Bot size={17} /></div><div><strong>OmicsOps Agent</strong><p>{zh ? "描述你的研究目标。我会先核对数据与假设，并在任何正式执行前展示计划供你审批。" : "Describe your research goal. I will first check the data and assumptions, then show a plan for approval before any formal execution."}</p></div></article>}
        {sentMessages.map((message, index) => <article className="message user-message" key={`${index}-${message}`}><p>{message}</p></article>)}
        {messages.map((message) => <Fragment key={message.id}>
          {message.role === "user" ? <article className="message user-message"><p>{message.markdown}</p></article> : message.role === "assistant" ? <article className="message assistant-message"><div className="assistant-avatar"><Bot size={17} /></div><div><strong>OmicsOps Agent</strong><p>{message.markdown}</p></div></article> : null}
          {runTimeline.afterMessage.get(message.id)?.map((run) => <AgentRunFold locale={locale} run={run} activeRunId={effectiveActiveRunId} key={run.runId} />)}
          {runTimelineV3.afterMessage.get(message.id)?.map((run) => <HarnessV3RunFold locale={locale} run={run} activeRunId={effectiveActiveRunId} onAnswer={onAnswerAgentQuestionV3} key={run.runId} />)}
          {runTimelineV4.afterMessage.get(message.id)?.map((run) => <V4RunTrace locale={locale} events={run.events} onAnswer={onAnswerAgentQuestionV4} historical key={run.runId} />)}
        </Fragment>)}
        {streamingAssistant && <article className="message assistant-message"><div className="assistant-avatar"><Bot size={17} /></div><div><strong>OmicsOps Agent · {zh ? "生成中" : "streaming"}</strong><p>{streamingAssistant}</p></div></article>}
        {agentBusy && !streamingAssistant && <article className="message assistant-message agent-pending" role="status"><div className="assistant-avatar"><Bot size={17} /></div><div><strong>OmicsOps Agent</strong><p>{zh ? "正在等待模型响应…" : "Waiting for the model…"}</p></div></article>}
        {agentRetryNotice && <div className="agent-retry-notice" role="status"><span className="agent-working"><i />{agentRetryNotice}</span></div>}
        {agentNotice && <div className="agent-notice" role="alert"><strong>{zh ? "对话未完成" : "Conversation did not complete"}</strong><span>{agentNotice}</span></div>}
        {v4Plan?.plan && !runStarted && <article className="message assistant-message plan-ready-message"><div className="assistant-avatar"><ClipboardList size={17} /></div><div><strong>OmicsOps Agent</strong><p>{zh ? "计划已生成，请在右侧 Plan 面板审核并决定是否运行。" : "The plan is ready. Review it in the Plan panel and decide whether to run it."}</p></div></article>}
        {runTimeline.unanchored.length > 0 && <AgentRunHistory locale={locale} runs={runTimeline.unanchored} activeRunId={effectiveActiveRunId} />}
        {runTimelineV3.unanchored.length > 0 && <HarnessV3History locale={locale} runs={runTimelineV3.unanchored} activeRunId={effectiveActiveRunId} onAnswer={onAnswerAgentQuestionV3} />}
        {runTimelineV4.unanchored.map((run) => <V4RunTrace locale={locale} events={run.events} onAnswer={onAnswerAgentQuestionV4} historical key={run.runId} />)}
        {activeRun && <AgentRunFold locale={locale} run={activeRun} activeRunId={effectiveActiveRunId} />}
        {activeRunV3 && <HarnessV3RunFold locale={locale} run={activeRunV3} activeRunId={effectiveActiveRunId} onAnswer={onAnswerAgentQuestionV3} />}
        {visibleActiveRunEventsV4.length > 0 && <V4RunTrace locale={locale} events={visibleActiveRunEventsV4} onAnswer={onAnswerAgentQuestionV4} />}
        {runStarted && agentRunEvents.length === 0 && agentRunEventsV3.length === 0 && agentRunEventsV4.length === 0 && <AgentConversationUpdates locale={locale} events={[]} />}
        {runActive && onCancelRun && <div className="agent-run-controls" role="region" aria-label={zh ? "远程 Agent 运行控制" : "Remote agent run controls"}><div><span className="agent-working"><i />{runStopping ? (zh ? "正在终止当前操作…" : "Stopping current operation…") : (zh ? "远程 Agent 正在运行" : "Remote agent is running")}</span><small>{zh ? "将中断模型请求、当前 SSH 命令及后续操作" : "Stops the model request, current SSH command, and all subsequent actions"}</small></div><button className="stop-agent-button" disabled={runStopping} onClick={() => void onCancelRun()}><Square size={14} fill="currentColor" />{runStopping ? (zh ? "终止中…" : "Stopping…") : (zh ? "终止运行" : "Stop run")}</button></div>}
        {!onSend && <article className="task-card"><div className="task-icon"><Activity size={18} /></div><div className="task-body"><div><strong>{t.task}</strong><span>65%</span></div><p>{zh ? "远端 Linux · 8 CPU · 32 GiB · 低风险" : "Remote Linux · 8 CPU · 32 GiB · low risk"}</p><div className="task-progress"><i /></div><div className="task-actions"><button>{zh ? "查看日志" : "View logs"}</button><button>{zh ? "查看计划" : "View plan"}</button></div></div></article>}
      </section>
      <footer className="composer"><div className={`composer-input ${planModeEnabled ? "is-plan-mode" : ""}`}><textarea aria-label={t.composer} placeholder={planModeEnabled ? (zh ? "描述需要规划和执行的任务" : "Describe the task to plan and execute") : t.composer} value={draft} disabled={agentBusy} onChange={(event) => setDraft(event.target.value)} /><div><div className="composer-menu-anchor"><button className="composer-tool" aria-label={zh ? "添加上下文或选择模式" : "Add context or choose mode"} aria-expanded={composerMenuOpen} onClick={() => setComposerMenuOpen((open) => !open)}><Plus size={17} /></button>{composerMenuOpen && <div className="composer-add-menu" role="menu"><button role="menuitem" disabled={!onUploadFiles} onClick={() => { setComposerMenuOpen(false); void onUploadFiles?.(); }}><Folder size={16} /><span><b>{zh ? "添加文件" : "Add files"}</b><small>{zh ? "选择项目输入文件" : "Select project input files"}</small></span></button><button role="menuitem" className={planModeEnabled ? "active" : ""} onClick={() => { setPlanModeEnabled(!planModeEnabled); setComposerMenuOpen(false); }}><Activity size={16} /><span><b>{zh ? "Plan 模式" : "Plan mode"}</b><small>{zh ? "先规划并在右侧审核，再批准执行" : "Plan first, review on the right, then approve execution"}</small></span>{planModeEnabled && <Check size={15} />}</button></div>}</div>{planModeEnabled && <button className="composer-mode-chip" onClick={() => setPlanModeEnabled(false)}><Activity size={14} />Plan<X size={13} /></button>}<button className="send-button" disabled={agentBusy || planLoading || !draft.trim() || Boolean(onSend && !computeReady)} onClick={send}><Send size={16} />{agentBusy || planLoading ? (planModeEnabled ? (zh ? "规划中…" : "Planning…") : (zh ? "执行中…" : "Running…")) : t.send}</button></div></div><small>{planModeEnabled ? (zh ? "Plan 模式：计划显示在右侧，批准后才执行" : "Plan mode: review the plan on the right before execution") : computeReady ? (zh ? "Agent 模式：发送后直接执行任务" : "Agent mode: send to execute directly") : onSend ? (zh ? "没有可用的计算后端" : "No compute backend is available") : modelLabel ? `${zh ? "当前模型" : "Model"}: ${modelLabel}` : ""}</small></footer>
    </main>

    <aside className="context-pane" aria-label={t.context}>
      <div className="context-tabs" role="tablist">{(["files", "plan", "preview", "notebook", "explore", "runs"] as ContextTab[]).map((id) => <button key={id} role="tab" aria-selected={tab === id} onClick={() => setTab(id)}>{id === "files" ? t.files : id === "plan" ? (zh ? "Plan" : "Plan") : id === "preview" ? t.preview : id === "notebook" ? t.notebook : id === "explore" ? t.explore : t.runs}</button>)}</div>
      <div className="context-content">{tab === "files" && <RemoteFileTree locale={locale} remoteFiles={remoteFiles} busy={filesBusy} notice={fileNotice} onUpload={onUploadFiles} onRefresh={onRefreshFiles} onDownload={onDownloadFile} syncEntries={syncEntries} onPauseSync={onPauseSync} onCancelSync={onCancelSync} onRetrySync={onRetrySync} />}{tab === "plan" && <PlanPanel locale={locale} active={showPlanPanel} planLoading={planLoading} v4Plan={v4Plan} planProposal={planProposal} planApproved={planApproved || approved} runStarted={runStarted} backends={computeBackends} backendId={computeBackendId} containerImage={containerImage} autonomyMode={autonomyMode} environment={computeEnvironment} computeBusy={computeBusy} plannedSkills={plannedSkills} events={activeRunEventsV4} onBackendChange={onComputeBackendChange} onImageChange={onContainerImageChange} onAutonomyChange={onAutonomyModeChange} onEnvironmentChange={onComputeEnvironmentChange} onApprove={() => { if (onApprovePlan) return onApprovePlan(); setApproved(true); }} onRegenerate={onRequestPlan} onAnswer={onAnswerAgentQuestionV4} />}{tab === "preview" && <><div className="context-toolbar"><span>{t.overview}</span><button aria-label={t.expand} onClick={() => setExpanded(true)}><Expand size={16} /></button></div>{preview}</>}{tab === "notebook" && <Notebook locale={locale} entries={notebookEntries} artifacts={projectArtifacts} facts={memoryFacts} onSearch={onSearchMemory} onExport={onExportNotebook} />}{tab === "explore" && <KernelPanel locale={locale} sessions={kernelSessions} events={kernelEvents} busy={kernelBusy} notice={kernelNotice} onStart={onStartKernel} onExecute={onExecuteKernel} onInterrupt={onInterruptKernel} onStop={onStopKernel} onPromote={onPromoteKernelCell} />}{tab === "runs" && <RunSummary locale={locale} />}</div>
    </aside>
    {expanded && <div className="preview-overlay" role="dialog" aria-modal="true" aria-label={t.artifactPreview}><header><div><small>{project.name}</small><h2>{t.overview}</h2></div><button aria-label="Close" onClick={() => setExpanded(false)}><X /></button></header>{preview}</div>}
  </div>;
}

function ComputeBackendSelector({ locale, backends, backendId, containerImage, autonomyMode, environment, busy, onBackendChange, onImageChange, onAutonomyChange, onEnvironmentChange }: { locale: Locale; backends: ComputeBackendAvailabilityV4[]; backendId: string; containerImage: string; autonomyMode: AutonomyModeV4; environment: string; busy: boolean; onBackendChange?: (value: string) => void; onImageChange?: (value: string) => void; onAutonomyChange?: (value: AutonomyModeV4) => void; onEnvironmentChange?: (value: string) => void }) {
  const zh = locale === "zh-CN";
  const selected = backends.find((item) => item.descriptor.backend_id === backendId);
  const container = selected?.descriptor.isolation === "container";
  return <section className="compute-selector" aria-label={zh ? "V4 计算后端" : "V4 compute backend"}><header><div><b>{zh ? "冻结计算配置" : "Frozen compute selection"}</b><small>{busy ? (zh ? "正在探测…" : "Probing…") : (zh ? "探测不会拉取镜像或启动容器" : "Discovery never pulls images or starts containers")}</small></div><span>{selected?.descriptor.isolation ?? "—"}</span></header><div className="compute-backend-grid">{backends.map((backend) => <label className={backendId === backend.descriptor.backend_id ? "active" : ""} key={backend.descriptor.backend_id}><input type="radio" name="v4-backend" value={backend.descriptor.backend_id} checked={backendId === backend.descriptor.backend_id} disabled={!backend.selectable} onChange={() => onBackendChange?.(backend.descriptor.backend_id)} /><span><b>{backend.descriptor.kind.toUpperCase()}</b><small>Python: {backend.python_status} · R: {backend.r_status}</small>{backend.reason && <em>{backend.reason}</em>}</span></label>)}</div>{container && <label>{zh ? "本地已有容器镜像" : "Existing local image"}<input aria-label={zh ? "容器镜像" : "Container image"} value={containerImage} placeholder="omicsops/science:latest" onChange={(event) => onImageChange?.(event.target.value)} /><small>{selected?.resolved_image_id ? `image ID: ${selected.resolved_image_id}` : (zh ? "输入后只执行 image inspect" : "Only image inspect runs after entry")}</small></label>}{selected?.descriptor.kind === "ssh" && <label>{zh ? "SSH 环境" : "SSH environment"}<input aria-label={zh ? "SSH 环境" : "SSH environment"} value={environment} onChange={(event) => onEnvironmentChange?.(event.target.value)} /><small>{zh ? "system 或安全的 Micromamba 环境名" : "system or a safe Micromamba environment name"}</small></label>}<div className="compute-policy"><label><input type="radio" name="autonomy" checked={autonomyMode === "supervised"} onChange={() => onAutonomyChange?.("supervised")} />supervised</label><label><input type="radio" name="autonomy" checked={autonomyMode === "full_auto"} disabled={!container || !selected?.selectable} onChange={() => onAutonomyChange?.("full_auto")} />full auto</label><span>{container ? "network=none" : "network=host_inherited"}</span></div></section>;
}
function PlanPanel({ locale, active, planLoading, v4Plan, planProposal, planApproved, runStarted, backends, backendId, containerImage, autonomyMode, environment, computeBusy, plannedSkills, events, onBackendChange, onImageChange, onAutonomyChange, onEnvironmentChange, onApprove, onRegenerate, onAnswer }: { locale: Locale; active: boolean; planLoading: boolean; v4Plan?: RunSummaryV4 | null; planProposal?: PlanProposal | null; planApproved: boolean; runStarted: boolean; backends: ComputeBackendAvailabilityV4[]; backendId: string; containerImage: string; autonomyMode: AutonomyModeV4; environment: string; computeBusy: boolean; plannedSkills: Array<{ name: string; version: string; hash: string; sections: number }>; events: AgentRunEventV4[]; onBackendChange?: (value: string) => void; onImageChange?: (value: string) => void; onAutonomyChange?: (value: AutonomyModeV4) => void; onEnvironmentChange?: (value: string) => void; onApprove: () => Promise<void> | void; onRegenerate?: () => Promise<void> | void; onAnswer?: (runId: string, questionId: string, answer: string) => Promise<void> | void }) {
  const zh = locale === "zh-CN";
  const input = [...events].reverse().find((event) => event.event.kind === "input_requested");
  if (!active) return <div className="plan-panel-empty"><ClipboardList size={28} /><b>{zh ? "尚未进入 Plan 模式" : "Plan mode is not active"}</b><p>{zh ? "从输入框左侧的 + 菜单选择 Plan。默认 Agent 模式会直接执行任务。" : "Choose Plan from the + menu. Default Agent mode executes tasks directly."}</p></div>;
  if (!v4Plan?.plan && !planProposal) return <div className="plan-panel"><header><div><small>AGENT RUNTIME V4</small><h2>{planLoading ? (zh ? "正在生成计划" : "Generating plan") : (zh ? "Plan 模式配置" : "Plan mode setup")}</h2></div><span>{planLoading ? "…" : (zh ? "待发送" : "Ready")}</span></header><p className="plan-panel-lead">{zh ? "确认计算后端，然后在主输入框发送任务。规划阶段只会读取和检查，不会执行分析。" : "Confirm the compute backend, then send the task from the main composer. Planning may inspect but cannot execute analysis."}</p><ComputeBackendSelector locale={locale} backends={backends} backendId={backendId} containerImage={containerImage} autonomyMode={autonomyMode} environment={environment} busy={computeBusy} onBackendChange={onBackendChange} onImageChange={onImageChange} onAutonomyChange={onAutonomyChange} onEnvironmentChange={onEnvironmentChange} />{input?.event.kind === "input_requested" && onAnswer && <section className="plan-question"><b>{zh ? "需要补充信息" : "More information required"}</b><p>{input.event.question}</p><V4AnswerForm locale={locale} onSubmit={(answer) => onAnswer(input.run_id, input.event.kind === "input_requested" ? input.event.question_id : "", answer)} /></section>}</div>;
  const title = v4Plan?.plan?.objective ?? planProposal?.plan.title ?? (zh ? "分析计划" : "Analysis plan");
  const steps = v4Plan?.plan?.steps ?? planProposal?.plan.stages.flatMap((stage) => stage.steps.map((step) => step.title)) ?? [];
  const criteria = v4Plan?.plan?.completion_criteria ?? [];
  const invalid = planProposal?.validation.valid === false;
  return <div className="plan-panel"><header><div><small>AGENT RUNTIME V4 · PLAN</small><h2>{title}</h2></div><span>{runStarted ? (zh ? "运行中" : "Running") : planApproved ? (zh ? "已批准" : "Approved") : invalid ? (zh ? "无效" : "Invalid") : (zh ? "待审批" : "Review")}</span></header><section><h3>{zh ? "执行步骤" : "Execution steps"}</h3><ol>{steps.map((step, index) => <li key={`${index}-${step}`}>{step}</li>)}</ol></section>{criteria.length > 0 && <section><h3>{zh ? "完成标准" : "Completion criteria"}</h3><ul>{criteria.map((criterion) => <li key={criterion}>{criterion}</li>)}</ul></section>}{v4Plan?.compute_selection && <section className="plan-frozen"><h3>{zh ? "冻结配置" : "Frozen configuration"}</h3><code>{v4Plan.compute_selection.backend_kind}:{v4Plan.compute_selection.backend_id}</code><span>{v4Plan.compute_selection.autonomy_mode} · {v4Plan.compute_selection.environment} · network={v4Plan.compute_selection.network_policy}</span>{v4Plan.compute_selection.container_image && <small>{v4Plan.compute_selection.container_image.reference}<br />{v4Plan.compute_selection.container_image.image_id}</small>}</section>}{plannedSkills.length > 0 && <section><h3>Skills</h3>{plannedSkills.map((skill) => <small className="plan-skill" key={`${skill.name}:${skill.version}:${skill.hash}`}>{skill.name}@{skill.version} · {skill.sections} {zh ? "段" : "sections"} · {skill.hash.slice(0, 12)}</small>)}</section>}{invalid && <section className="plan-panel-errors" role="alert"><h3>{zh ? "计划验证失败" : "Plan validation failed"}</h3><ul>{planProposal!.validation.issues.map((issue) => <li key={`${issue.path}:${issue.code}`}><code>{issue.path}</code> {issue.message}</li>)}</ul><button disabled={planLoading} onClick={() => void onRegenerate?.()}>{zh ? "重新生成" : "Regenerate"}</button></section>}<footer><small>{v4Plan?.approval_hash ? `approval SHA-256 ${v4Plan.approval_hash}` : planProposal ? `plan SHA-256 ${planProposal.plan_hash}` : ""}</small><button disabled={planApproved || runStarted || invalid || Boolean(v4Plan && !v4Plan.approval_hash)} onClick={() => void onApprove()}>{runStarted ? (zh ? "正在运行" : "Running") : planApproved ? (zh ? "已批准" : "Approved") : (zh ? "批准并运行" : "Approve and run")}</button></footer></div>;
}
function FileTree({ locale }: { locale: Locale }) { const zh = locale === "zh-CN"; return <div className="file-tree"><div className="context-heading"><b>{zh ? "项目文件" : "Project files"}</b><small>{zh ? "选择性同步" : "Selective sync"}</small></div><div className="tree-folder"><Folder size={15} />data <span>{zh ? "远端" : "remote"}</span></div><div className="tree-folder"><Folder size={15} />analysis</div><div className="tree-file"><FileBarChart size={15} />umap.png <em>1.2 MB</em></div><div className="tree-file"><FileText size={15} />markers.csv <em>84 KB</em></div><div className="tree-file"><NotebookPen size={15} />report.md <em>12 KB</em></div></div>; }
function V4RunTrace({ locale, events, onAnswer, historical = false }: { locale: Locale; events: AgentRunEventV4[]; onAnswer?: (runId: string, questionId: string, answer: string) => Promise<void> | void; historical?: boolean }) {
  const zh = locale === "zh-CN";
  const entries = coalesceV4ModelText(events);
  const terminal = [...events].reverse().find(isTerminalAgentEventV4);
  const status = terminal?.event.kind === "run_completed" ? (zh ? "已完成" : "Completed") : terminal?.event.kind === "run_cancelled" ? (zh ? "已终止" : "Cancelled") : terminal?.event.kind === "run_needs_attention" ? (zh ? "需要处理" : "Needs attention") : terminal?.event.kind === "run_failed" ? (zh ? "失败" : "Failed") : (zh ? "运行中" : "Running");
  return <details className={`agent-run-fold harness-v3-run ${historical ? "" : "is-active"}`} open={historical ? undefined : true}><summary><span className="agent-run-fold-title"><span><b>Agent Runtime V4</b><small>{zh ? "Plan/Execute 硬隔离 · 持久 Kernel" : "Hard Plan/Execute boundary · persistent kernel"}</small></span><ChevronRight size={15} /></span><span>{status} · {events.length} {zh ? "条哈希事件" : "hash-chained events"}</span></summary><div className="agent-run-fold-body">{entries.map(({ event, lastEvent, modelText }) => <article aria-label={modelText === undefined ? undefined : (zh ? "模型输出" : "Model output")} className="message assistant-message agent-work-update" key={`${event.run_id}-${event.sequence}`}><div className="assistant-avatar"><Bot size={17} /></div><div><div className="agent-work-heading"><strong>{modelText === undefined ? v4EventLabel(event, zh) : (zh ? "模型输出" : "Model output")}</strong><small>{lastEvent.sequence === event.sequence ? `#${event.sequence}` : `#${event.sequence}–#${lastEvent.sequence}`} · {new Date(lastEvent.occurred_at).toLocaleTimeString()}</small></div><p>{modelText ?? v4EventContent(event, zh)}</p>{event.event.kind === "input_requested" && onAnswer && <V4AnswerForm locale={locale} onSubmit={(answer) => onAnswer(event.run_id, (event.event as Extract<AgentRunEventV4["event"], { kind: "input_requested" }>).question_id, answer)} />}</div></article>)}</div></details>;
}
function coalesceV4ModelText(events: AgentRunEventV4[]): Array<{ event: AgentRunEventV4; lastEvent: AgentRunEventV4; modelText?: string }> {
  const entries: Array<{ event: AgentRunEventV4; lastEvent: AgentRunEventV4; modelText?: string }> = [];
  for (let index = 0; index < events.length;) {
    const event = events[index];
    if (event.event.kind !== "model_text") {
      entries.push({ event, lastEvent: event });
      index += 1;
      continue;
    }
    let lastEvent = event;
    let modelText = "";
    while (index < events.length && events[index].event.kind === "model_text") {
      const textEvent = events[index];
      modelText += (textEvent.event as Extract<AgentRunEventV4["event"], { kind: "model_text" }>).text;
      lastEvent = textEvent;
      index += 1;
    }
    entries.push({ event, lastEvent, modelText });
  }
  return entries;
}
function V4AnswerForm({ locale, onSubmit }: { locale: Locale; onSubmit: (answer: string) => Promise<void> | void }) { const [answer, setAnswer] = useState(""); const zh = locale === "zh-CN"; return <div className="v4-answer"><input aria-label={zh ? "回答 V4 问题" : "Answer V4 question"} value={answer} onChange={(event) => setAnswer(event.target.value)} /><button disabled={!answer.trim()} onClick={() => void onSubmit(answer.trim())}>{zh ? "回答并恢复" : "Answer and resume"}</button></div>; }
function v4EventLabel(event: AgentRunEventV4, zh: boolean) { if (event.event.kind === "run_created") return event.event.mode === "execute" ? (zh ? "任务启动" : "Task started") : (zh ? "规划启动" : "Planning started"); const labels: Record<string, string> = { plan_proposed: zh ? "计划已冻结" : "Plan frozen", plan_approved: zh ? "计划获批" : "Plan approved", mode_changed: zh ? "执行模式" : "Execution mode", tool_requested: zh ? "工具请求" : "Tool request", tool_finished: zh ? "工具结果" : "Tool result", completion_proposed: zh ? "完成提案" : "Completion proposed", run_completed: zh ? "运行完成" : "Run completed", run_failed: zh ? "运行失败" : "Run failed", run_cancelled: zh ? "运行取消" : "Run cancelled" }; return labels[event.event.kind] ?? event.event.kind; }
function v4EventContent(event: AgentRunEventV4, zh: boolean) { if (event.event.kind === "tool_requested") return event.event.call.tool_id; if (event.event.kind === "tool_finished") return event.event.outcome.model_content; if (event.event.kind === "plan_proposed") return `${event.event.plan.steps.length} ${zh ? "个步骤" : "steps"} · SHA-256 ${event.event.plan_hash.slice(0, 12)}`; if (event.event.kind === "model_text") return event.event.text; if (event.event.kind === "run_failed") return event.event.message; return zh ? "状态已写入可验证事件链" : "State persisted to the verified event chain"; }
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

function isTerminalAgentEventV3(event: AgentRunEventV3) {
  return event.event.kind === "run_completed" || event.event.kind === "run_failed" || event.event.kind === "run_cancelled" || event.event.kind === "needs_attention";
}

function isTerminalAgentEventV4(event: AgentRunEventV4) {
  return event.event.kind === "run_completed" || event.event.kind === "run_failed" || event.event.kind === "run_cancelled" || event.event.kind === "run_needs_attention";
}

function groupAgentRunEventsV4(events: AgentRunEventV4[]) {
  const byRun = new Map<string, AgentRunEventV4[]>();
  for (const event of events) byRun.set(event.run_id, [...(byRun.get(event.run_id) ?? []), event]);
  return [...byRun].map(([runId, runEvents]) => ({ runId, events: runEvents.sort((left, right) => left.sequence - right.sequence) }));
}

function placeV4RunsAfterMessages(messages: NonNullable<Props["messages"]>, events: AgentRunEventV4[]) {
  const timedMessages = messages.filter((message) => (message.role === "user" || message.role === "assistant") && message.created_at && Number.isFinite(Date.parse(message.created_at)));
  const afterMessage = new Map<string, ReturnType<typeof groupAgentRunEventsV4>>();
  const unanchored: ReturnType<typeof groupAgentRunEventsV4> = [];
  groupAgentRunEventsV4(events).forEach((run) => {
    const startedAt = Date.parse(run.events[0]?.occurred_at ?? "");
    const anchor = Number.isFinite(startedAt) ? timedMessages.filter((message) => Date.parse(message.created_at!) <= startedAt).at(-1) : undefined;
    if (!anchor) unanchored.push(run);
    else afterMessage.set(anchor.id, [...(afterMessage.get(anchor.id) ?? []), run]);
  });
  return { afterMessage, unanchored };
}

function groupAgentRunEventsV3(events: AgentRunEventV3[]) {
  const byRun = new Map<string, AgentRunEventV3[]>();
  for (const event of events) byRun.set(event.run_id, [...(byRun.get(event.run_id) ?? []), event]);
  return [...byRun].map(([runId, runEvents]) => ({ runId, events: runEvents.sort((left, right) => left.sequence - right.sequence) }));
}

function placeHarnessV3RunsAfterMessages(messages: NonNullable<Props["messages"]>, events: AgentRunEventV3[]) {
  const timedMessages = messages.filter((message) => (message.role === "user" || message.role === "assistant") && message.created_at && Number.isFinite(Date.parse(message.created_at)));
  const afterMessage = new Map<string, ReturnType<typeof groupAgentRunEventsV3>>();
  const unanchored: ReturnType<typeof groupAgentRunEventsV3> = [];
  groupAgentRunEventsV3(events).forEach((run) => {
    const startedAt = Date.parse(run.events[0]?.occurred_at ?? "");
    const anchor = Number.isFinite(startedAt) ? timedMessages.filter((message) => Date.parse(message.created_at!) <= startedAt).at(-1) : undefined;
    if (!anchor) unanchored.push(run);
    else afterMessage.set(anchor.id, [...(afterMessage.get(anchor.id) ?? []), run]);
  });
  return { afterMessage, unanchored };
}

function HarnessV3History({ locale, runs, activeRunId, onAnswer }: { locale: Locale; runs: ReturnType<typeof groupAgentRunEventsV3>; activeRunId: string | null; onAnswer?: Props["onAnswerAgentQuestionV3"] }) {
  return <section className="harness-v3-history" aria-label={locale === "zh-CN" ? "Harness v3 运行轨迹" : "Harness v3 run trace"}>
    {runs.map((run) => <HarnessV3RunFold locale={locale} run={run} activeRunId={activeRunId} onAnswer={onAnswer} key={run.runId} />)}
  </section>;
}

function HarnessV3RunFold({ locale, run, activeRunId, onAnswer }: { locale: Locale; run: { runId: string; events: AgentRunEventV3[] }; activeRunId: string | null; onAnswer?: Props["onAnswerAgentQuestionV3"] }) {
  const zh = locale === "zh-CN";
  const terminal = [...run.events].reverse().find(isTerminalAgentEventV3);
  const active = run.runId === activeRunId && !terminal;
  const status = terminal?.event.kind === "run_completed" ? (zh ? "已完成" : "Completed")
    : terminal?.event.kind === "run_cancelled" ? (zh ? "已终止" : "Canceled")
    : terminal?.event.kind === "needs_attention" ? (zh ? "需要处理" : "Needs attention")
    : terminal?.event.kind === "run_failed" ? (zh ? "失败" : "Failed")
    : (zh ? "运行中" : "Running");
  return <details className={`agent-run-fold harness-v3-fold ${active ? "is-active" : ""}`} open={active ? true : undefined} aria-label={zh ? "Harness v3 运行" : "Harness v3 run"}>
    <summary><span className="agent-run-fold-title"><span><b>Harness v3</b><small>{zh ? "可恢复结构化轨迹" : "Recoverable structured trace"}</small></span><ChevronRight size={15} /></span><span>{status} · {run.events.length} {zh ? "条事件" : "events"}</span></summary>
    <div className="harness-v3-events">{run.events.map((event, eventIndex) => <HarnessV3EventCard locale={locale} event={event} eventIndex={eventIndex} runEvents={run.events} onAnswer={onAnswer} key={`${event.run_id}-${event.sequence}`} />)}</div>
  </details>;
}

function HarnessV3EventCard({ locale, event, eventIndex, runEvents, onAnswer }: { locale: Locale; event: AgentRunEventV3; eventIndex: number; runEvents: AgentRunEventV3[]; onAnswer?: Props["onAnswerAgentQuestionV3"] }) {
  const zh = locale === "zh-CN";
  const kind = event.event.kind;
  const meta = <small>#{event.sequence} · {new Date(event.occurred_at).toLocaleTimeString()} · {event.event_hash.slice(0, 10)}</small>;
  if (kind === "model_text") {
    if (eventIndex > 0 && runEvents[eventIndex - 1]?.event.kind === "model_text") return null;
    const textEvents: AgentRunEventV3[] = [];
    for (let index = eventIndex; index < runEvents.length && runEvents[index]?.event.kind === "model_text"; index += 1) {
      textEvents.push(runEvents[index]);
    }
    const text = textEvents.map((item) => item.event.kind === "model_text" ? item.event.payload.text : "").join("");
    const lastEvent = textEvents[textEvents.length - 1] ?? event;
    const sequence = lastEvent.sequence === event.sequence ? `#${event.sequence}` : `#${event.sequence}–#${lastEvent.sequence}`;
    return <article className="harness-v3-card model-output-card" aria-label={zh ? "模型输出" : "Model output"}><header><b>{zh ? "模型输出" : "Model output"}</b><span>{sequence}</span></header><p>{text}</p><small>{new Date(lastEvent.occurred_at).toLocaleTimeString()} · {lastEvent.event_hash.slice(0, 10)}</small></article>;
  }
  if (kind === "tool_call_requested" || kind === "tool_call_dispatched") {
    const request = event.event.payload.request;
    return <article className="harness-v3-card tool-card"><header><b>{request.tool_id}</b><span>{kind === "tool_call_requested" ? (zh ? "已请求" : "Requested") : (zh ? "执行中" : "Dispatched")}</span></header>{meta}<pre>{JSON.stringify(request.arguments, null, 2)}</pre><small>{zh ? "幂等键" : "Idempotency key"}: {request.idempotency_key}</small></article>;
  }
  if (kind === "tool_call_finished") {
    const outcome = event.event.payload.outcome;
    return <article className={`harness-v3-card tool-outcome status-${outcome.status}`}><header><b>{zh ? "工具结果" : "Tool outcome"}</b><span>{outcome.status}{outcome.truncated ? ` · ${zh ? "预览已截断" : "preview truncated"}` : ""}</span></header>{meta}{outcome.model_content && <pre>{outcome.model_content}</pre>}{outcome.error && <p>{outcome.error}</p>}<div className="provenance-list">{outcome.provenance.map((item) => <code key={item}>{item}</code>)}</div></article>;
  }
  if (kind === "completion_ledger_updated") {
    if (runEvents.slice(eventIndex + 1).some((candidate) => candidate.event.kind === "completion_ledger_updated")) return null;
    const ledger = event.event.payload.ledger;
    return <article className="harness-v3-card ledger-card" aria-label={zh ? "完成账本" : "Completion ledger"}>
      <header><b>{zh ? "完成账本" : "Completion ledger"}</b><span>{ledger.criteria.filter((item) => item.evidence_sequences.length > 0).length}/{ledger.criteria.length}</span></header>
      {meta}
      {ledger.criteria.map((item) => <p key={item.id}>{item.evidence_sequences.length > 0 ? "✓" : "○"} {item.description} {item.evidence_sequences.length > 0 && <small>#{item.evidence_sequences.join(", #")}</small>}</p>)}
      {ledger.verified_artifacts.map((artifact) => <div className="verified-artifact" key={artifact.path}><b>{artifact.path}</b><small>{artifact.size_bytes} bytes · SHA-256 {artifact.sha256.slice(0, 12)} · #{artifact.evidence_sequence}</small></div>)}
      {[...ledger.unresolved_errors, ...ledger.uncertain_side_effects].map((error) => <p className="ledger-error" key={error}>{error}</p>)}
    </article>;
  }
  if (kind === "review_completed") {
    const report = event.event.payload.report;
    return <article className="harness-v3-card reviewer-card"><header><b>{zh ? "独立科学审查" : "Independent scientific review"}</b><span>{zh ? `第 ${report.cycle} 轮` : `Cycle ${report.cycle}`}</span></header>{meta}{report.findings.map((finding, index) => <div className={`review-finding severity-${finding.severity}`} key={`${finding.summary}-${index}`}><b>{finding.severity}</b><span>{finding.summary}</span>{finding.evidence.map((evidence) => <code key={evidence}>{evidence}</code>)}</div>)}</article>;
  }
  if (kind === "user_input_requested") {
    const requested = event.event as Extract<AgentRunEventV3["event"], { kind: "user_input_requested" }>;
    const answered = runEvents.some((candidate) => {
      const candidateEvent = candidate.event;
      return candidateEvent.kind === "user_input_answered" && candidateEvent.payload.question_id === requested.payload.question_id;
    });
    return <HarnessV3QuestionCard locale={locale} runId={event.run_id} questionId={requested.payload.question_id} question={requested.payload.question} answered={answered} onAnswer={onAnswer} />;
  }
  if (kind === "context_compacted") {
    return <article className="harness-v3-card context-card"><header><b>{zh ? "上下文已压缩" : "Context compacted"}</b><span>#{event.event.payload.first_sequence}–#{event.event.payload.last_sequence}</span></header>{meta}<code>SHA-256 {event.event.payload.last_event_hash.slice(0, 16)}</code></article>;
  }
  const message = kind === "model_step_started" ? `${zh ? "模型步骤" : "Model step"} ${event.event.payload.step}`
    : kind === "needs_attention" ? event.event.payload.reason
    : kind === "run_failed" ? event.event.payload.message
    : kind === "user_input_answered" ? `${zh ? "已回答" : "Answered"}: ${event.event.payload.answer}`
    : kind.replaceAll("_", " ");
  return <article className={`harness-v3-card event-${kind}`}><header><b>{message}</b></header>{meta}</article>;
}

function HarnessV3QuestionCard({ locale, runId, questionId, question, answered, onAnswer }: { locale: Locale; runId: string; questionId: string; question: string; answered: boolean; onAnswer?: Props["onAnswerAgentQuestionV3"] }) {
  const zh = locale === "zh-CN";
  const [answer, setAnswer] = useState("");
  return <article className="harness-v3-card question-card"><header><b>{zh ? "需要用户输入" : "User input required"}</b><span>{answered ? (zh ? "已回答" : "Answered") : (zh ? "等待中" : "Waiting")}</span></header><p>{question}</p>{!answered && <div><input aria-label={`${zh ? "回答" : "Answer"}: ${question}`} value={answer} onChange={(change) => setAnswer(change.target.value)} /><button disabled={!answer.trim()} onClick={() => void onAnswer?.(runId, questionId, answer.trim())}>{zh ? "提交回答" : "Submit answer"}</button></div>}</article>;
}

function Notebook({ locale, entries, artifacts, facts, onSearch, onExport }: { locale: Locale; entries: NotebookEntry[]; artifacts: ProjectArtifact[]; facts: MemoryFact[]; onSearch?: (query: string, dimension?: string) => Promise<void> | void; onExport?: (format: "markdown" | "json" | "bundle") => Promise<void> | void }) { const zh = locale === "zh-CN"; const [query, setQuery] = useState(""); const [dimension, setDimension] = useState(""); return <div className="notebook-panel"><div className="notebook-actions"><input aria-label={zh ? "检索记忆" : "Search memory"} value={query} onChange={(event) => setQuery(event.target.value)} placeholder={zh ? "按任务、环境或产物检索" : "Search tasks, environments, artifacts"} /><select aria-label={zh ? "记忆维度" : "Memory dimension"} value={dimension} onChange={(event) => setDimension(event.target.value)}><option value="">{zh ? "全部事实" : "All facts"}</option><option value="task">{zh ? "任务" : "Task"}</option><option value="environment">{zh ? "环境" : "Environment"}</option><option value="artifact">{zh ? "产物" : "Artifact"}</option></select><button onClick={() => void onSearch?.(query, dimension || undefined)}>{zh ? "检索" : "Search"}</button></div><div className="notebook-export"><button onClick={() => void onExport?.("markdown")}>Markdown</button><button onClick={() => void onExport?.("json")}>JSON</button><button onClick={() => void onExport?.("bundle")}>{zh ? "项目包" : "Bundle"}</button></div><div className="notebook-list">{entries.length === 0 && <div><span>{zh ? "研究记录" : "Notebook"}</span><b>{zh ? "暂无正式条目" : "No formal entries yet"}</b><p>{zh ? "Agent 完成并验证产物后会自动登记目标、方法、观察、决策与证据。" : "Verified Agent runs automatically register goals, methods, observations, decisions, and evidence."}</p></div>}{entries.map((entry) => <div key={entry.id}><span>{entry.kind}</span><b>{entry.title}</b><p>{entry.markdown}</p><small>{entry.evidence_ids.length} {zh ? "条可追溯引用" : "traceable references"}</small></div>)}</div><div className="artifact-register"><b>{zh ? "已登记产物" : "Registered artifacts"} · {artifacts.length}</b>{artifacts.map((artifact) => <small key={artifact.id}>{artifact.relative_path} · SHA-256 {artifact.sha256.slice(0, 12)}</small>)}</div><div className="memory-results"><b>{zh ? "事实记忆" : "Fact memory"} · {facts.length}</b>{facts.slice(0, 20).map((fact) => <article className={fact.conflicted_with.length ? "conflicted" : ""} key={fact.id}><span>{fact.dimension}</span><p>{fact.statement}</p><small>{fact.evidence.map((evidence) => `${evidence.source_kind}:${evidence.source_id}`).join(" · ")}</small>{fact.conflicted_with.length > 0 && <em>{zh ? "存在冲突事实，已保留双方来源" : "Conflicting fact retained with both sources"}</em>}</article>)}</div></div>; }
function RunSummary({ locale }: { locale: Locale }) { const zh = locale === "zh-CN"; return <div className="run-summary"><Activity size={24} /><b>{zh ? "运行中" : "Running"}</b><span>3 / 5 {zh ? "步骤已验证" : "steps verified"}</span><div className="task-progress"><i style={{ width: "60%" }} /></div></div>; }
