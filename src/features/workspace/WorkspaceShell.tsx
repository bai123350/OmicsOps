import { Fragment, useEffect, useRef, useState, type ReactNode } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import {
  Activity, ArrowLeft, Bot, Check, ChevronRight, ClipboardList, Database, Expand, FileBarChart, FileText,
  FlaskConical, Folder, Hand, Languages, MessageSquarePlus, NotebookPen, Plus,
  Search, Send, Settings, Shield, ShieldAlert, ShieldCheck, Sparkles, Square, Trash2, X, Orbit, Monitor, ChevronDown, Gauge, Zap,
} from "lucide-react";
import { copy, type Locale } from "./copy";
import type { AgentRunEventV4, AgentV4Phase, AgentV4Task, AgentV4TaskShape, ApprovalPolicyV4, AutonomyModeV4, BrowserApprovalScopeV4, ComputeBackendAvailabilityV4, FormalStepProposal, KernelEvent, KernelLanguage, KernelSession, MemoryFact, NotebookEntry, ProposedPlanRevisionV4, ProjectArtifact, ProjectImagePreview, RunSummaryV4, SessionAgentModeV4, SyncEntry, WorkspaceConversation } from "../../types";
import { RemoteFileTree } from "./RemoteFileTree";
import { KernelPanel } from "./KernelPanel";
import { ComposeActions } from "./ComposeActions";
import { RuntimeDialog } from "./RuntimeDialog";
import { useWindowEscapeLayer } from "../settings/BrowserSettings";
import { V4PlanPanel } from "./V4PlanPanel";
import "./workspace.css";
import "./approval.css";
import "./file-actions.css";
import "./navigation.css";
import "./kernel.css";
import "./agent.css";
import "./preview.css";
import "./notebook.css";
import "./composer.css";

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
  onOpenSettings?: (section?: "models" | "remote" | "skills") => void;
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
  modelPicker?: ReactNode;
  composerBusy?: boolean;
  modelOptions?: Array<{ id: string; label: string }>;
  modelId?: string;
  onModelChange?: (id: string) => void;
  /** Durable conversation mode owned by DesktopApp. */
  agentMode?: SessionAgentModeV4;
  onAgentModeChange?: (mode: SessionAgentModeV4) => Promise<void> | void;
  /** Snapshot-derived lock for the active conversation only. */
  conversationLocked?: boolean;
  /** Initial conversation hydration is still restoring the durable mode/state. */
  conversationHydrating?: boolean;
  /** Mutually excludes approve/request-changes/cancel plan actions. */
  planActionBusy?: boolean;
  v4Plan?: RunSummaryV4 | null;
  latestPlanRevision?: ProposedPlanRevisionV4 | null;
  computeBackends?: ComputeBackendAvailabilityV4[];
  computeBackendId?: string;
  containerImage?: string;
  autonomyMode?: AutonomyModeV4;
  approvalPolicy?: ApprovalPolicyV4;
  computeEnvironment?: string;
  computeBusy?: boolean;
  onComputeBackendChange?: (backendId: string) => void;
  onContainerImageChange?: (image: string) => void;
  onAutonomyModeChange?: (mode: AutonomyModeV4) => void;
  onApprovalPolicyChange?: (policy: ApprovalPolicyV4) => void;
  onComputeEnvironmentChange?: (environment: string) => void;
  planLoading?: boolean;
  planApproved?: boolean;
  onRequestPlan?: () => Promise<void> | void;
  onRequestPlanRevision?: (feedback: string) => Promise<void> | void;
  onApprovePlan?: () => Promise<void> | void;
  onStartRun?: () => Promise<void> | void;
  onCancelRun?: () => Promise<void> | void;
  runStopping?: boolean;
  canStartRun?: boolean;
  runStarted?: boolean;
  activeRunId?: string | null;
  activeRunLastActivityAt?: string | null;
  agentRunEventsV4?: AgentRunEventV4[];
  onAnswerAgentQuestionV4?: (runId: string, questionId: string, answer: string) => Promise<void> | void;
  onDecideToolApprovalV4?: (runId: string, approvalId: string, callHash: string, decision: "approved" | "denied", browserScope?: BrowserApprovalScopeV4) => Promise<void> | void;
  onResolveUncertainV4?: (runId: string, callId: string, resolution: "side_effect_observed" | "side_effect_not_observed" | "compensated", evidence: string) => Promise<void> | void;
  onResumeAgentRunV4?: (runId: string) => Promise<void> | void;
  onCloseBrowserRunTabsV4?: (runId: string, sessions: Array<"shared" | "workspace">) => Promise<void> | void;
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
const AGENT_STALL_THRESHOLD_MS = 90_000;

export function WorkspaceShell({ project, locale, onLocaleChange, onOpenSettings, onBackToProjects, conversations = [], activeConversationId, onSelectConversation, onNewConversation, onDeleteConversation, onSend, messages = [], streamingAssistant = "", agentBusy = false, agentNotice = "", agentRetryNotice = "", modelLabel, modelPicker, composerBusy = false, modelOptions = [], modelId, onModelChange, agentMode, onAgentModeChange, conversationLocked = false, conversationHydrating = false, planActionBusy = false, v4Plan, latestPlanRevision, computeBackends = [], computeBackendId = "", containerImage = "", autonomyMode = "supervised", approvalPolicy = "risk_based", computeEnvironment = "system", computeBusy = false, onComputeBackendChange, onContainerImageChange, onAutonomyModeChange, onApprovalPolicyChange, onComputeEnvironmentChange, planLoading = false, planApproved = false, onRequestPlan, onRequestPlanRevision, onApprovePlan, onStartRun, onCancelRun, runStopping = false, canStartRun = false, runStarted = false, activeRunId, activeRunLastActivityAt, agentRunEventsV4 = [], onAnswerAgentQuestionV4, onDecideToolApprovalV4, onResolveUncertainV4, onResumeAgentRunV4, onCloseBrowserRunTabsV4, remoteFiles, filesBusy = false, onUploadFiles, onRefreshFiles, onDownloadFile, onPreviewImage, fileNotice, kernelSessions = [], kernelEvents = [], kernelBusy = false, kernelNotice, onStartKernel, onExecuteKernel, onInterruptKernel, onStopKernel, onPromoteKernelCell, memoryFacts = [], notebookEntries = [], projectArtifacts = [], onSearchMemory, onExportNotebook, syncEntries = [], onPauseSync, onCancelSync, onRetrySync }: Props) {
  const t = copy[locale];
  const zh = locale === "zh-CN";
  const [tab, setTab] = useState<ContextTab>("files");
  const [expanded, setExpanded] = useState(false);
  const [draft, setDraft] = useState("");
  const [localMode, setLocalMode] = useState<SessionAgentModeV4>("agent");
  const [planSessionActive, setPlanSessionActive] = useState(false);
  const [composerMenuOpen, setComposerMenuOpen] = useState(false);
  const [computeMenuOpen, setComputeMenuOpen] = useState(false);
  const [permissionMenuOpen, setPermissionMenuOpen] = useState(false);
  const [runtimeLanguage, setRuntimeLanguage] = useState<KernelLanguage | null>(null);
  const [sendMenuOpen, setSendMenuOpen] = useState(false);
  const [modelMenuOpen, setModelMenuOpen] = useState(false);
  useWindowEscapeLayer(expanded, () => setExpanded(false));
  useWindowEscapeLayer(composerMenuOpen, () => setComposerMenuOpen(false));
  useWindowEscapeLayer(permissionMenuOpen, () => setPermissionMenuOpen(false));
  useWindowEscapeLayer(computeMenuOpen, () => setComputeMenuOpen(false));
  useWindowEscapeLayer(sendMenuOpen, () => setSendMenuOpen(false));
  useWindowEscapeLayer(modelMenuOpen, () => setModelMenuOpen(false));
  useWindowEscapeLayer(runtimeLanguage !== null, () => setRuntimeLanguage(null));
  const [sentMessages, setSentMessages] = useState<string[]>([]);
  const [approved, setApproved] = useState(false);
  const [sendBusy, setSendBusy] = useState(false);
  const sendBusyRef = useRef(false);
  const sendGenerationRef = useRef(0);
  const [approvePlanBusy, setApprovePlanBusy] = useState(false);
  const approvePlanBusyRef = useRef(false);
  const [selectedImagePath, setSelectedImagePath] = useState("");
  const [imagePreview, setImagePreview] = useState<ProjectImagePreview | null>(null);
  const [previewBusy, setPreviewBusy] = useState(false);
  const [previewError, setPreviewError] = useState("");
  const [deletingConversationId, setDeletingConversationId] = useState<string | null>(null);
  const [followingLatest, setFollowingLatest] = useState(true);
  const followingLatestRef = useRef(true);
  const messageStreamRef = useRef<HTMLElement>(null);
  const effectivePlanActionBusy = Boolean(planActionBusy || approvePlanBusy || runStopping);
  const controlledMode = agentMode !== undefined;
  const effectiveMode = agentMode ?? localMode;
  const planModeEnabled = effectiveMode === "plan";
  const modeLocked = conversationHydrating || conversationLocked || planLoading || runStarted;
  const imageFiles = (remoteFiles ?? []).filter((entry) => !entry.directory && isPreviewImage(entry.relative_path));
  const preview = <ArtifactPreview title={t.overview} locale={locale} images={imageFiles} selectedPath={selectedImagePath} preview={imagePreview} busy={previewBusy} error={previewError} onSelect={setSelectedImagePath} onLoad={loadImagePreview} />;
  const effectiveActiveRunId = activeRunId ?? (runStarted ? agentRunEventsV4.at(-1)?.run_id ?? null : null);
  const activeRunEventsV4 = effectiveActiveRunId
    ? agentRunEventsV4
      .filter((event) => event.run_id === effectiveActiveRunId)
      .sort((left, right) => left.sequence - right.sequence)
    : [];
  const runFinished = Boolean(effectiveTerminalAgentEventV4(activeRunEventsV4));
  const runPaused = getV4PauseReason(activeRunEventsV4) !== null;
  const runActive = runStarted && !runFinished && !runPaused;
  const [watchdogNow, setWatchdogNow] = useState(() => Date.now());
  const lastActivityMs = activeRunLastActivityAt ? Date.parse(activeRunLastActivityAt) : Number.NaN;
  const runStalled = runActive && Number.isFinite(lastActivityMs) && watchdogNow - lastActivityMs > AGENT_STALL_THRESHOLD_MS;
  // A paused run still owns the conversation sequence. Keep the composer
  // locked while approval/input cards remain usable inside the run trace.
  const composerDisabled = composerBusy || conversationHydrating || sendBusy || agentBusy || conversationLocked || (runStarted && !runFinished);
  const activeConversation = conversations.find((item) => item.id === activeConversationId);
  const conversationTitle = activeConversation?.title || (zh ? "新会话" : "New conversation");
  const historicalAgentRunEventsV4 = effectiveActiveRunId
    ? agentRunEventsV4.filter((event) => event.run_id !== effectiveActiveRunId)
    : agentRunEventsV4;
  const runTimelineV4 = placeV4RunsAfterMessages(messages, historicalAgentRunEventsV4);
  const latestAgentRunEventV4 = agentRunEventsV4.at(-1);
  const selectedBackend = computeBackends.find((item) => item.descriptor.backend_id === computeBackendId);
  const computeReady = Boolean(selectedBackend?.selectable);
  const selectedBackendIsContainer = Boolean(selectedBackend?.selectable && selectedBackend.descriptor.isolation === "container");
  const backendLabel = selectedBackend?.descriptor.kind === "local" ? "Local" : selectedBackend?.descriptor.kind === "ssh" ? "SSH" : selectedBackend?.descriptor.kind.toUpperCase() ?? (zh ? "选择环境" : "Choose environment");
  function runtimeStatus(language: KernelLanguage) {
    const status = language === "python" ? selectedBackend?.python_status : selectedBackend?.r_status;
    return status === "available" ? (zh ? "可用" : "AVAILABLE") : status === "unavailable" ? (zh ? "不可用" : "UNAVAILABLE") : (zh ? "待检测" : "UNVERIFIED");
  }
  function closeComposerMenus() {
    setComposerMenuOpen(false); setPermissionMenuOpen(false); setComputeMenuOpen(false); setModelMenuOpen(false); setSendMenuOpen(false);
  }
  const showPlanPanel = planModeEnabled || planLoading || (!controlledMode && (planSessionActive || Boolean(v4Plan?.plan)));
  const executeStart = activeRunEventsV4.findIndex((event) => event.event.kind === "mode_changed" && event.event.mode === "execute");
  const visibleActiveRunEventsV4 = v4Plan?.plan && executeStart >= 0 ? activeRunEventsV4.slice(executeStart) : v4Plan?.plan ? [] : activeRunEventsV4;

  useEffect(() => { if (!controlledMode) setLocalMode("agent"); setPlanSessionActive(false); }, [activeConversationId, controlledMode]);
  useEffect(() => {
    sendGenerationRef.current += 1;
    sendBusyRef.current = false;
    setSendBusy(false);
    followingLatestRef.current = true;
    setFollowingLatest(true);
  }, [activeConversationId]);
  useEffect(() => { if (showPlanPanel) setTab("plan"); }, [showPlanPanel]);
  useEffect(() => {
    if (!runActive || !activeRunLastActivityAt) {
      setWatchdogNow(Date.now());
      return;
    }
    const timer = window.setInterval(() => setWatchdogNow(Date.now()), 1_000);
    return () => window.clearInterval(timer);
  }, [runActive, activeRunLastActivityAt]);

  useEffect(() => {
    const stream = messageStreamRef.current;
    if (!stream || !followingLatestRef.current) return;
    const frame = requestAnimationFrame(() => {
      if (followingLatestRef.current) stream.scrollTop = stream.scrollHeight;
    });
    return () => cancelAnimationFrame(frame);
  }, [activeConversationId, messages.length, streamingAssistant, agentBusy, agentNotice, planLoading, runStarted, latestAgentRunEventV4?.run_id, latestAgentRunEventV4?.sequence]);

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

  function chooseMode(nextMode: SessionAgentModeV4) {
    if (modeLocked) return;
    if (controlledMode) {
      void onAgentModeChange?.(nextMode);
      return;
    }
    setLocalMode(nextMode);
  }

  async function send() {
    const message = draft.trim();
    if (!message || composerDisabled || sendBusyRef.current) return;
    const sendGeneration = sendGenerationRef.current;
    sendBusyRef.current = true;
    setSendBusy(true);
    setDraft("");
    try {
      if (onSend) {
        const mode = planModeEnabled ? "plan" : "chat";
        setPlanSessionActive(mode === "plan");
        const accepted = await onSend(message, mode);
        if (accepted === false && sendGeneration === sendGenerationRef.current) setDraft((current) => current || message);
      } else {
        setSentMessages((current) => [...current, message]);
      }
    } finally {
      if (sendGeneration === sendGenerationRef.current) {
        sendBusyRef.current = false;
        setSendBusy(false);
      }
    }
  }

  async function approvePlan() {
    if (approvePlanBusyRef.current) return;
    approvePlanBusyRef.current = true;
    setApprovePlanBusy(true);
    try {
      if (onApprovePlan) await onApprovePlan();
      else setApproved(true);
    } finally {
      approvePlanBusyRef.current = false;
      setApprovePlanBusy(false);
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
      <div className="rail-section sessions"><span>{zh ? "会话" : "Sessions"}</span><div className="session-list">{conversations.map((item) => { const title = item.title || (zh ? "新会话" : "New conversation"); const active = item.id === activeConversationId; const deleteDisabled = deletingConversationId !== null || conversationHydrating || conversationLocked || (active && (agentBusy || runActive)); return <div className={`session-entry ${active ? "active" : ""}`} key={item.id}><button className="session-row" aria-current={active ? "page" : undefined} onClick={() => void onSelectConversation?.(item.id)}><Sparkles size={15} /><span title={item.title}>{title}</span></button><button className="session-delete" aria-label={zh ? `删除会话：${title}` : `Delete conversation: ${title}`} title={zh ? "删除会话" : "Delete conversation"} disabled={deleteDisabled || !onDeleteConversation} onClick={() => void deleteConversation(item)}><Trash2 size={14} /></button></div>; })}</div><button className="new-session" disabled={conversationHydrating || agentBusy || conversationLocked} onClick={() => void onNewConversation?.()}><MessageSquarePlus size={15} />{t.newConversation}</button></div>
      <div className="rail-footer"><button onClick={() => onLocaleChange(zh ? "en-US" : "zh-CN")}><Languages size={16} />{zh ? "English" : "简体中文"}</button><button onClick={() => onOpenSettings?.()}><Settings size={16} />{t.settings}</button></div>
    </nav>

    <main className="conversation-pane" aria-label={t.research}>
      <header className="conversation-header"><div><small>{project.name}</small><h1>{conversationTitle}</h1></div><span className="live-status"><i />{t.status}</span></header>
      <section className="message-stream" aria-live="polite" ref={messageStreamRef} onScroll={(event) => {
        const stream = event.currentTarget;
        const following = stream.scrollHeight - stream.scrollTop - stream.clientHeight < 64;
        followingLatestRef.current = following;
        setFollowingLatest(following);
      }}>
        {!onSend && <article className="message user-message"><MarkdownContent markdown={zh ? "比较两批 PBMC，检查批次效应并生成可复现的分析报告。" : "Compare two PBMC batches, assess batch effects, and generate a reproducible report."} /></article>}
        {messages.length === 0 && <article className="message assistant-message"><div className="assistant-avatar"><Bot size={17} /></div><div><strong>OmicsOps Agent</strong><MarkdownContent markdown={zh ? "描述你的研究目标。我会先核对数据与假设，并在任何正式执行前展示计划供你审批。" : "Describe your research goal. I will first check the data and assumptions, then show a plan for approval before any formal execution."} /></div></article>}
        {sentMessages.map((message, index) => <article className="message user-message" key={`${index}-${message}`}><MarkdownContent markdown={message} /></article>)}
        {messages.map((message) => <Fragment key={message.id}>
          {message.role === "user" ? <article className="message user-message"><MarkdownContent markdown={message.markdown} /></article> : message.role === "assistant" ? <article className="message assistant-message"><div className="assistant-avatar"><Bot size={17} /></div><div><strong>OmicsOps Agent</strong><MarkdownContent markdown={message.markdown} /></div></article> : null}
          {runTimelineV4.afterMessage.get(message.id)?.map((run) => <V4RunTrace locale={locale} events={run.events} onAnswer={onAnswerAgentQuestionV4} onDecideApproval={onDecideToolApprovalV4} onResolveUncertain={onResolveUncertainV4} onResume={onResumeAgentRunV4} onCloseBrowserTabs={onCloseBrowserRunTabsV4} historical key={run.runId} />)}
        </Fragment>)}
        {streamingAssistant && <article className="message assistant-message"><div className="assistant-avatar"><Bot size={17} /></div><div><strong>OmicsOps Agent · {zh ? "生成中" : "streaming"}</strong><MarkdownContent markdown={streamingAssistant} /></div></article>}
        {agentBusy && !streamingAssistant && <article className="message assistant-message agent-pending" role="status"><div className="assistant-avatar"><Bot size={17} /></div><div><strong>OmicsOps Agent</strong><p>{zh ? "正在等待模型响应…" : "Waiting for the model…"}</p></div></article>}
        {agentRetryNotice && <div className="agent-retry-notice" role="status"><span className="agent-working"><i />{agentRetryNotice}</span></div>}
        {agentNotice && <div className="agent-notice" role="alert"><strong>{zh ? "对话未完成" : "Conversation did not complete"}</strong><span>{agentNotice}</span></div>}
        {v4Plan?.plan && !runStarted && <article className="message assistant-message plan-ready-message"><div className="assistant-avatar"><ClipboardList size={17} /></div><div><strong>OmicsOps Agent</strong><p>{zh ? "计划已生成，请在右侧 Plan 面板审核并决定是否运行。" : "The plan is ready. Review it in the Plan panel and decide whether to run it."}</p></div></article>}
        {runTimelineV4.unanchored.map((run) => <V4RunTrace locale={locale} events={run.events} onAnswer={onAnswerAgentQuestionV4} onDecideApproval={onDecideToolApprovalV4} onResolveUncertain={onResolveUncertainV4} onResume={onResumeAgentRunV4} onCloseBrowserTabs={onCloseBrowserRunTabsV4} historical key={run.runId} />)}
        {visibleActiveRunEventsV4.length > 0 && <V4RunTrace locale={locale} events={visibleActiveRunEventsV4} onAnswer={onAnswerAgentQuestionV4} onDecideApproval={onDecideToolApprovalV4} onResolveUncertain={onResolveUncertainV4} onResume={onResumeAgentRunV4} onCloseBrowserTabs={onCloseBrowserRunTabsV4} />}
        {runStarted && agentRunEventsV4.length === 0 && <article className="message assistant-message agent-pending" role="status"><div className="assistant-avatar"><Bot size={17} /></div><div><strong>OmicsOps Agent</strong><p>{zh ? "V4 运行正在启动…" : "Starting the V4 run…"}</p></div></article>}
        {runStalled && <div className="agent-retry-notice" role="status"><span>{zh ? "超过 90 秒未收到新的 Agent 事件，任务可能卡住；仍可终止运行。" : "No new Agent event has arrived for 90 seconds; the run may be stuck. You can still stop it."}</span></div>}
        {runActive && onCancelRun && <div className="agent-run-controls" role="region" aria-label={zh ? "远程 Agent 运行控制" : "Remote agent run controls"}><div><span className="agent-working"><i />{runStopping ? (zh ? "正在终止当前操作…" : "Stopping current operation…") : (zh ? "远程 Agent 正在运行" : "Remote agent is running")}</span><small>{zh ? "将中断模型请求、当前 SSH 命令及后续操作" : "Stops the model request, current SSH command, and all subsequent actions"}</small></div><button className="stop-agent-button" disabled={runStopping} onClick={() => void onCancelRun()}><Square size={14} fill="currentColor" />{runStopping ? (zh ? "终止中…" : "Stopping…") : (zh ? "终止运行" : "Stop run")}</button></div>}
        {!onSend && <article className="task-card"><div className="task-icon"><Activity size={18} /></div><div className="task-body"><div><strong>{t.task}</strong><span>65%</span></div><p>{zh ? "远端 Linux · 8 CPU · 32 GiB · 低风险" : "Remote Linux · 8 CPU · 32 GiB · low risk"}</p><div className="task-progress"><i /></div><div className="task-actions"><button>{zh ? "查看日志" : "View logs"}</button><button>{zh ? "查看计划" : "View plan"}</button></div></div></article>}
      {!followingLatest && <button className="back-to-latest" onClick={() => {
        const stream = messageStreamRef.current;
        if (stream) stream.scrollTop = stream.scrollHeight;
        followingLatestRef.current = true;
        setFollowingLatest(true);
      }}>↓ {zh ? "回到最新" : "Back to latest"}</button>}
      </section>
      <footer className="composer">
        <div className="composer-runtime-bar">
          <div className="composer-menu-anchor compute-anchor">
            <button className="runtime-host" aria-label={zh ? "选择计算后端" : "Choose compute backend"} aria-expanded={computeMenuOpen} onClick={() => { const next = !computeMenuOpen; closeComposerMenus(); setComputeMenuOpen(next); }}><Monitor size={17} /><b>{backendLabel}</b><ChevronDown size={13} /></button>
            {computeMenuOpen && <div className="composer-compute-menu"><ComputeBackendSelector locale={locale} backends={computeBackends} backendId={computeBackendId} containerImage={containerImage} autonomyMode={autonomyMode} environment={computeEnvironment} busy={computeBusy || composerDisabled} onBackendChange={onComputeBackendChange} onImageChange={onContainerImageChange} onAutonomyChange={onAutonomyModeChange} onEnvironmentChange={onComputeEnvironmentChange} /><button className="compute-settings-link" disabled={!onOpenSettings} onClick={() => { setComputeMenuOpen(false); onOpenSettings?.("remote"); }}>{zh ? "添加 SSH 主机 / 管理环境" : "Add SSH host / Manage environments"}<ChevronRight size={14} /></button></div>}
          </div>
          {(["python", "r"] as const).map((language) => <button key={language} className="runtime-pill" aria-label={`${language === "python" ? "Python" : "R"} ${zh ? "环境" : "environment"}`} onClick={() => { closeComposerMenus(); setRuntimeLanguage(language); }}><b>{language === "python" ? "Python" : "R"}</b><span>{runtimeStatus(language)}</span></button>)}
        </div>
        <div className={`composer-input ${planModeEnabled ? "is-plan-mode" : ""}`}>
          <textarea aria-label={t.composer} placeholder={planModeEnabled ? (zh ? "描述需要规划和执行的任务" : "Describe the task to plan and execute") : t.composer} value={draft} disabled={composerDisabled} onChange={(event) => setDraft(event.target.value)} />
          <div className="composer-toolbar">
            <div className="composer-menu-anchor"><button className="composer-tool" aria-label={zh ? "添加上下文或选择模式" : "Add context or choose mode"} aria-expanded={composerMenuOpen} onClick={() => { const next = !composerMenuOpen; closeComposerMenus(); setComposerMenuOpen(next); }}><Plus size={20} /></button>{composerMenuOpen && <ComposeActions zh={zh} onClose={() => setComposerMenuOpen(false)} onAttach={onUploadFiles ? () => { void onUploadFiles(); } : undefined} onFiles={() => setTab("files")} onReview={() => setDraft((current) => [current, zh ? "请审查当前会话中的方法、证据和结论，指出潜在问题与需要补充的验证。" : "Review the methods, evidence, and conclusions in this conversation. Identify potential issues and missing validation."].filter(Boolean).join("\n\n"))} onManageSkills={onOpenSettings ? () => onOpenSettings("skills") : undefined} />}</div>
            <div className="composer-menu-anchor permission-anchor">
              <button className="composer-tool composer-orbit" title={zh ? "Agent 控制" : "Agent controls"} aria-label={zh ? "Agent 权限" : "Agent permissions"} aria-expanded={permissionMenuOpen} onClick={() => { const next = !permissionMenuOpen; closeComposerMenus(); setPermissionMenuOpen(next); }}><Orbit size={21} /></button>
              {permissionMenuOpen && <div className="permission-menu" role="menu" aria-label={zh ? "Agent 权限选项" : "Agent permission options"}>
                <button className="agent-control-row" role="menuitemcheckbox" aria-checked={planModeEnabled} disabled={modeLocked} onClick={() => chooseMode(planModeEnabled ? "agent" : "plan")}><span>{zh ? "先做计划" : "Plan first"}</span><i className={`control-switch ${planModeEnabled ? "is-on" : ""}`} /></button>
                <header><b>{zh ? "应如何批准 Agent 操作？" : "How should Agent actions be approved?"}</b><small>{selectedBackend?.descriptor.kind.toUpperCase() ?? "—"}</small></header>
                <PermissionOption icon={<Hand size={17} />} active={approvalPolicy === "request_approval"} title={zh ? "请求批准" : "Ask approval"} description={zh ? "写入、命令和网络操作前请求确认" : "Ask before writes, commands, and network operations"} onClick={() => { onApprovalPolicyChange?.("request_approval"); onAutonomyModeChange?.("supervised"); setPermissionMenuOpen(false); }} />
                <PermissionOption icon={<ShieldCheck size={17} />} active={approvalPolicy === "risk_based"} title={zh ? "帮我批准" : "Risk based"} description={zh ? "仅对检测到的风险操作请求批准" : "Ask only for operations detected as risky"} onClick={() => { onApprovalPolicyChange?.("risk_based"); onAutonomyModeChange?.("supervised"); setPermissionMenuOpen(false); }} />
                <PermissionOption icon={<ShieldAlert size={17} />} active={approvalPolicy === "full_access"} danger disabled={!selectedBackendIsContainer} title={zh ? "完全访问权限" : "Full access"} description={selectedBackendIsContainer ? (zh ? "仅限离线 Docker/Podman 容器" : "Offline Docker/Podman containers only") : (zh ? "需要可用的 Docker/Podman 隔离" : "Requires available Docker/Podman isolation")} onClick={() => { onApprovalPolicyChange?.("full_access"); onAutonomyModeChange?.("full_auto"); setPermissionMenuOpen(false); }} />
                <div className="agent-control-divider" />
                <button role="menuitem" className="agent-control-row" disabled><span>{zh ? "完成方式" : "Completion"}</span><small>{zh ? "会话内" : "Inline"}</small></button>
                {[(zh ? "子任务委派" : "Delegation"), (zh ? "自动审查" : "Auto-review"), (zh ? "分析工具失败" : "Analyze tool failures"), (zh ? "审查模型" : "Reviewer model"), (zh ? "专家代理" : "Specialist")].map((label) => <button key={label} role="menuitem" className="agent-control-row" disabled><span>{label}</span><small>{zh ? "暂未支持" : "Unavailable"}</small></button>)}
                <button role="menuitem" className="agent-control-row" onClick={() => { setTab("notebook"); setPermissionMenuOpen(false); }}><span>{zh ? "记忆与研究记录" : "Memory & notebook"}</span><ChevronRight size={14} /></button>
                <button role="menuitem" className="agent-control-row" onClick={() => setComputeMenuOpen(true)}><span>{zh ? "计算环境" : "Compute"}</span><span>{backendLabel}<ChevronRight size={14} /></span></button>
              </div>}
            </div>
            {planModeEnabled && <button className="composer-mode-chip" disabled={modeLocked} onClick={() => chooseMode("agent")}><Activity size={14} />Plan<X size={13} /></button>}
            <div className="composer-send-controls">
            <span className="context-meter" title={zh ? "当前接口尚未提供上下文用量" : "Context usage is not reported by the current API"} aria-label={zh ? "上下文用量未知" : "Context usage unknown"}><Gauge size={19} /><span>—</span></span>
            <button className={`composer-tool ${planModeEnabled ? "" : "is-active"}`} disabled={modeLocked} aria-label={zh ? "直接执行模式" : "Direct execution mode"} aria-pressed={!planModeEnabled} title={zh ? "直接执行 / 先做计划" : "Execute directly / Plan first"} onClick={() => chooseMode(planModeEnabled ? "agent" : "plan")}><Zap size={20} /></button>
            {modelPicker ? <div onClickCapture={closeComposerMenus}>{modelPicker}</div> : <div className="composer-menu-anchor model-anchor"><button className="composer-model" disabled={composerDisabled} aria-label={zh ? "选择模型" : "Choose model"} aria-expanded={modelMenuOpen} onClick={() => { const next = !modelMenuOpen; closeComposerMenus(); setModelMenuOpen(next); }}><span>{modelLabel || (zh ? "选择模型" : "Choose model")}</span><ChevronDown size={12} /></button>{modelMenuOpen && <div className="model-menu" role="menu">{modelOptions.map((model) => <button role="menuitemradio" aria-checked={model.id === modelId} key={model.id} disabled={!onModelChange} onClick={() => { onModelChange?.(model.id); setModelMenuOpen(false); }}>{model.label}{model.id === modelId && <Check size={14} />}</button>)}<button role="menuitem" disabled={!onOpenSettings} onClick={() => { setModelMenuOpen(false); onOpenSettings?.(); }}>{zh ? "管理模型" : "Manage models"}<Settings size={14} /></button></div>}</div>}
            <div className="composer-send-group">
            <button className="send-button" disabled={composerDisabled || planLoading || !draft.trim() || Boolean(onSend && !computeReady)} onClick={send}><Send size={16} />{composerDisabled || planLoading ? (planModeEnabled ? (zh ? "规划中…" : "Planning…") : (zh ? "执行中…" : "Running…")) : t.send}</button>
            <div className="composer-menu-anchor"><button className="send-options" aria-label={zh ? "发送选项" : "Send options"} aria-expanded={sendMenuOpen} onClick={() => { const next = !sendMenuOpen; closeComposerMenus(); setSendMenuOpen(next); }}><ChevronDown size={17} /></button>{sendMenuOpen && <div className="model-menu send-menu" role="menu">{(["agent", "plan"] as const).map((mode) => <button key={mode} role="menuitemradio" aria-checked={effectiveMode === mode} disabled={modeLocked} onClick={() => { chooseMode(mode); setSendMenuOpen(false); }}>{mode === "plan" ? (zh ? "先做计划" : "Plan first") : (zh ? "直接执行" : "Execute directly")}{effectiveMode === mode && <Check size={14} />}</button>)}</div>}</div>
            </div>
            </div>
          </div>
        </div>
        <small>{conversationHydrating ? (zh ? "正在恢复会话模式和运行状态…" : "Restoring conversation mode and run state…") : planModeEnabled ? (zh ? "Plan 模式：计划显示在右侧，批准后才执行" : "Plan mode: review the plan on the right before execution") : computeReady ? (zh ? `Agent 模式：${selectedBackend?.descriptor.kind.toUpperCase()} · ${approvalPolicy === "request_approval" ? "请求批准" : approvalPolicy === "full_access" ? "完全访问" : "风险审批"}` : `Agent mode: ${selectedBackend?.descriptor.kind.toUpperCase()} · ${approvalPolicy}`) : onSend ? (zh ? "请选择一个可用的计算后端" : "Choose an available compute backend") : modelLabel ? `${zh ? "当前模型" : "Model"}: ${modelLabel}` : ""}</small>
      </footer>
    </main>

    <aside className="context-pane" aria-label={t.context}>
      <div className="context-tabs" role="tablist">{(["files", "plan", "preview", "notebook", "explore", "runs"] as ContextTab[]).map((id) => <button key={id} role="tab" aria-selected={tab === id} onClick={() => setTab(id)}>{id === "files" ? t.files : id === "plan" ? (zh ? "Plan" : "Plan") : id === "preview" ? t.preview : id === "notebook" ? t.notebook : id === "explore" ? t.explore : t.runs}</button>)}</div>
      <div className="context-content">{tab === "files" && <RemoteFileTree locale={locale} remoteFiles={remoteFiles} busy={filesBusy} notice={fileNotice} onUpload={onUploadFiles} onRefresh={onRefreshFiles} onDownload={onDownloadFile} syncEntries={syncEntries} onPauseSync={onPauseSync} onCancelSync={onCancelSync} onRetrySync={onRetrySync} />}{tab === "plan" && <V4PlanPanel locale={locale} active={showPlanPanel} planLoading={planLoading} v4Plan={v4Plan} latestPlanRevision={latestPlanRevision} conversationLocked={conversationLocked} planActionBusy={effectivePlanActionBusy} planApproved={planApproved || approved || approvePlanBusy} runStarted={runStarted} events={activeRunEventsV4} onApprove={approvePlan} onRequestPlanRevision={onRequestPlanRevision} onCancel={onCancelRun} />}{tab === "preview" && <><div className="context-toolbar"><span>{t.overview}</span><button aria-label={t.expand} onClick={() => setExpanded(true)}><Expand size={16} /></button></div>{preview}</>}{tab === "notebook" && <Notebook locale={locale} entries={notebookEntries} artifacts={projectArtifacts} facts={memoryFacts} onSearch={onSearchMemory} onExport={onExportNotebook} />}{tab === "explore" && <KernelPanel locale={locale} sessions={kernelSessions} events={kernelEvents} busy={kernelBusy} notice={kernelNotice} onStart={onStartKernel} onExecute={onExecuteKernel} onInterrupt={onInterruptKernel} onStop={onStopKernel} onPromote={onPromoteKernelCell} />}{tab === "runs" && <RunSummary locale={locale} />}</div>
    </aside>
    {runtimeLanguage && <RuntimeDialog zh={zh} language={runtimeLanguage} onLanguageChange={setRuntimeLanguage} backend={selectedBackend} environment={computeEnvironment} onClose={() => setRuntimeLanguage(null)} onSettings={onOpenSettings ? () => { setRuntimeLanguage(null); onOpenSettings("remote"); } : undefined} onPrepare={(language) => {
      const name = language === "python" ? "Python" : "R";
      const request = zh ? `请检查当前 ${backendLabel} 计算环境中的 ${name} 解释器和科研依赖，报告版本与缺失项，并按当前审批策略准备环境。使用项目隔离环境，避免修改系统环境；先验证最小示例再报告结果。` : `Check the ${name} interpreter and research dependencies in the current ${backendLabel} compute environment. Report versions and missing dependencies, and prepare a project-isolated environment under the current approval policy without modifying the system environment. Verify a minimal example before reporting the result.`;
      setDraft((current) => current ? `${current}\n\n${request}` : request);
      setRuntimeLanguage(null);
    }} />}
    {expanded && <div className="preview-overlay" role="dialog" aria-modal="true" aria-label={t.artifactPreview}><header><div><small>{project.name}</small><h2>{t.overview}</h2></div><button aria-label="Close" onClick={() => setExpanded(false)}><X /></button></header>{preview}</div>}
  </div>;
}

function ComputeBackendSelector({ locale, backends, backendId, containerImage, autonomyMode: _autonomyMode, environment, busy, onBackendChange, onImageChange, onAutonomyChange: _onAutonomyChange, onEnvironmentChange }: { locale: Locale; backends: ComputeBackendAvailabilityV4[]; backendId: string; containerImage: string; autonomyMode: AutonomyModeV4; environment: string; busy: boolean; onBackendChange?: (value: string) => void; onImageChange?: (value: string) => void; onAutonomyChange?: (value: AutonomyModeV4) => void; onEnvironmentChange?: (value: string) => void }) {
  const zh = locale === "zh-CN";
  const selected = backends.find((item) => item.descriptor.backend_id === backendId);
  const container = selected?.descriptor.isolation === "container";
  return <section className="compute-selector" aria-label={zh ? "V4 计算后端" : "V4 compute backend"}><header><div><b>{zh ? "冻结计算配置" : "Frozen compute selection"}</b><small>{busy ? (zh ? "正在探测…" : "Probing…") : (zh ? "探测不会拉取镜像或启动容器" : "Discovery never pulls images or starts containers")}</small></div><span>{selected?.descriptor.isolation ?? "—"}</span></header><div className="compute-backend-grid">{backends.map((backend) => <label className={backendId === backend.descriptor.backend_id ? "active" : ""} key={backend.descriptor.backend_id}><input type="radio" name="v4-backend" value={backend.descriptor.backend_id} checked={backendId === backend.descriptor.backend_id} disabled={busy || !backend.selectable || !onBackendChange} onChange={() => onBackendChange?.(backend.descriptor.backend_id)} /><span><b>{backend.descriptor.kind.toUpperCase()}</b><small>Python: {backend.python_status} · R: {backend.r_status}</small>{backend.reason && <em>{backend.reason}</em>}</span></label>)}</div>{container && <label>{zh ? "本地已有容器镜像" : "Existing local image"}<input disabled={busy || !onImageChange} aria-label={zh ? "容器镜像" : "Container image"} value={containerImage} placeholder="omicsops/science:latest" onChange={(event) => onImageChange?.(event.target.value)} /><small>{selected?.resolved_image_id ? `image ID: ${selected.resolved_image_id}` : (zh ? "输入后只执行 image inspect" : "Only image inspect runs after entry")}</small></label>}{selected?.descriptor.kind === "ssh" && <label>{zh ? "SSH 环境" : "SSH environment"}<input disabled={busy || !onEnvironmentChange} aria-label={zh ? "SSH 环境" : "SSH environment"} value={environment} onChange={(event) => onEnvironmentChange?.(event.target.value)} /><small>{zh ? "system 或安全的 Micromamba 环境名" : "system or a safe Micromamba environment name"}</small></label>}<div className="compute-policy"><span>{container ? "network=none" : "network=host_inherited"}</span></div></section>;
}

function PermissionOption({ icon, active, danger = false, disabled = false, title, description, onClick }: { icon: ReactNode; active: boolean; danger?: boolean; disabled?: boolean; title: string; description: string; onClick: () => void }) {
  return <button role="menuitemradio" aria-checked={active} disabled={disabled} className={`${active ? "active" : ""} ${danger ? "danger" : ""}`} onClick={onClick}><span className="permission-icon">{icon}</span><span><b>{title}</b><small>{description}</small></span>{active && <Check size={15} />}</button>;
}
function FileTree({ locale }: { locale: Locale }) { const zh = locale === "zh-CN"; return <div className="file-tree"><div className="context-heading"><b>{zh ? "项目文件" : "Project files"}</b><small>{zh ? "选择性同步" : "Selective sync"}</small></div><div className="tree-folder"><Folder size={15} />data <span>{zh ? "远端" : "remote"}</span></div><div className="tree-folder"><Folder size={15} />analysis</div><div className="tree-file"><FileBarChart size={15} />umap.png <em>1.2 MB</em></div><div className="tree-file"><FileText size={15} />markers.csv <em>84 KB</em></div><div className="tree-file"><NotebookPen size={15} />report.md <em>12 KB</em></div></div>; }
function V4RunTrace({ locale, events, onAnswer, onDecideApproval, onResolveUncertain, onResume, onCloseBrowserTabs, historical = false }: { locale: Locale; events: AgentRunEventV4[]; onAnswer?: (runId: string, questionId: string, answer: string) => Promise<void> | void; onDecideApproval?: (runId: string, approvalId: string, callHash: string, decision: "approved" | "denied", browserScope?: BrowserApprovalScopeV4) => Promise<void> | void; onResolveUncertain?: (runId: string, callId: string, resolution: "side_effect_observed" | "side_effect_not_observed" | "compensated", evidence: string) => Promise<void> | void; onResume?: (runId: string) => Promise<void> | void; onCloseBrowserTabs?: (runId: string, sessions: Array<"shared" | "workspace">) => Promise<void> | void; historical?: boolean }) {
  const zh = locale === "zh-CN";
  const [resumeBusy, setResumeBusy] = useState(false);
  const resumeBusyRef = useRef(false);
  const entries = coalesceV4ModelText(events);
  const progress = entries.filter(({ modelText }) => modelText !== undefined && modelText.trim());
  const technicalEntries = entries.filter(({ event, modelText }) => modelText === undefined && !isToolTrajectoryEvent(event) && !isHiddenTrajectoryEvent(event));
  const tools = mergeV4ToolCalls(events);
  const guided = buildGuidedV4Overview(events);
  const latest = events.at(-1);
  const terminal = effectiveTerminalAgentEventV4(events);
  const completionPending = !terminal && Boolean(latest && (
    latest.event.kind === "completion_proposed"
    || latest.event.kind === "completion_proposal_submitted"
    || latest.event.kind === "deterministic_verification_finished"
    || latest.event.kind === "reviewer_finished"
    || (latest.event.kind === "tool_requested" && latest.event.call.tool_id === "agent.complete")
  ));
  const pauseReason = getV4PauseReason(events);
  const status = terminal?.event.kind === "run_completed" ? (zh ? "已完成" : "Completed") : terminal?.event.kind === "run_cancelled" ? (zh ? "已终止" : "Cancelled") : terminal?.event.kind === "run_needs_attention" ? (zh ? "需要处理" : "Needs attention") : terminal?.event.kind === "run_failed" ? (zh ? "失败" : "Failed") : pauseReason === "approval" ? (zh ? "等待工具审批" : "Waiting for approval") : pauseReason === "input" ? (zh ? "等待回答" : "Waiting for input") : pauseReason === "browser_connection" ? (zh ? "等待连接浏览器" : "Waiting for browser") : pauseReason === "browser_human" ? (zh ? "等待人工处理浏览器" : "Waiting for browser intervention") : pauseReason === "uncertain" ? (zh ? "等待副作用核验" : "Waiting for verification") : (zh ? "运行中" : "Running");
  const failed = terminal?.event.kind === "run_failed";
  const shouldExpand = !historical && (!terminal || Boolean(pauseReason) || terminal?.event.kind === "run_failed" || terminal?.event.kind === "run_needs_attention");
  async function resumeRun() {
    if (!onResume || !events[0] || resumeBusyRef.current) return;
    resumeBusyRef.current = true;
    setResumeBusy(true);
    try {
      await onResume(events[0].run_id);
    } finally {
      resumeBusyRef.current = false;
      setResumeBusy(false);
    }
  }
  return <>
    {completionPending && <div className="agent-completion-pending" role="status"><span className="agent-working"><i />{zh ? "正在核验最终结果…" : "Verifying the final result…"}</span></div>}
    <details className={`agent-run-fold agent-v4-run ${historical ? "" : "is-active"}`} open={shouldExpand}>
    <summary><span className="agent-run-fold-title"><span><b>{zh ? "执行过程" : terminal ? "Processed" : "Processing"}</b><small>{terminal ? (zh ? "工具调用与验证记录" : "Tool calls and verification") : (zh ? "Agent 正在处理任务" : "Agent is working")}</small></span><ChevronRight size={15} /></span><span>{status} · {tools.length} {zh ? "个步骤" : tools.length === 1 ? "step" : "steps"}{eventDuration(events[0]?.occurred_at, (terminal ?? latest)?.occurred_at) && ` · ${eventDuration(events[0]?.occurred_at, (terminal ?? latest)?.occurred_at)}`}</span></summary>
    <div className="agent-run-fold-body">
      <section className="v4-process-timeline" aria-label={zh ? "工具调用详情" : "Tool call details"}>
      {[
        ...progress.map(({ event, modelText }) => ({ sequence: event.sequence, node: <article aria-label={zh ? "模型输出" : "Model output"} className="v4-progress-row" key={`progress-${event.sequence}`}><span aria-hidden="true">–</span><strong>{zh ? "进度" : "PROGRESS"}</strong><MarkdownContent markdown={modelText ?? ""} /></article> })),
        ...events.filter(({ event }) => event.kind === "cycle_started").map((event) => ({ sequence: event.sequence, node: <div className="v4-thinking-row" key={`thinking-${event.sequence}`}><span aria-hidden="true">○</span><strong>{zh ? "思考中" : "THINKING"}</strong></div> })),
        ...tools.map((tool) => ({ sequence: tool.firstSequence, node: <details className="v4-tool-trace" key={tool.callId}>
          <summary className="v4-tool-trace-heading"><span className={`v4-tool-mark ${tool.status}`} aria-label={toolStatusLabel(tool.status, zh)}>{tool.status === "failed" ? "×" : tool.status === "succeeded" || tool.status === "reused" ? "✓" : "○"}</span><strong title={toolDisplayLabel(tool.toolId, zh)}>{compactToolLabel(tool.toolId)}</strong>{tool.subject && <span className="v4-tool-subject" title={tool.subject}>{tool.subject}</span>}<span className={`v4-tool-status ${tool.status}`}>{toolStatusLabel(tool.status, zh)}</span><small className="v4-tool-metrics">{toolMetrics(tool, zh)}</small><ChevronRight size={13} /></summary>
          <div className="v4-tool-trace-body">
            <small>{tool.toolId} · #{tool.firstSequence}–#{tool.lastSequence}</small>
            {tool.argumentsPreview && <><b>{zh ? "输入" : "Input"}</b><code>{tool.argumentsPreview}</code></>}
            {tool.outcome !== undefined && <><b>{zh ? "结果" : "Result"}</b><pre>{tool.outcome}</pre></>}
          </div>
        </details> })),
        ...technicalEntries.map(({ event, lastEvent }) => ({ sequence: event.sequence, node: <article className={`message assistant-message agent-work-update ${event.event.kind === "input_requested" ? "v4-input-decision-entry" : ""}`} key={`${event.run_id}-${event.sequence}`}>
        <div className="assistant-avatar"><Bot size={17} /></div>
        <div><div className="agent-work-heading"><strong>{v4EventLabel(event, zh)}</strong><small>{lastEvent.sequence === event.sequence ? `#${event.sequence}` : `#${event.sequence}–#${lastEvent.sequence}`} · {new Date(lastEvent.occurred_at).toLocaleTimeString()}</small></div>
          {v4EventContent(event, zh) && <MarkdownContent markdown={v4EventContent(event, zh)} />}
          {event.event.kind === "input_requested" && onAnswer && !isV4QuestionAnswered(events, event.event.question_id) && <V4AnswerForm locale={locale} onSubmit={(answer) => onAnswer(event.run_id, event.event.kind === "input_requested" ? event.event.question_id : "", answer)} />}
          {event.event.kind === "tool_approval_requested" && onDecideApproval && !isV4ApprovalDecided(events, event.event.request.approval_id) && <V4ApprovalCard locale={locale} request={event.event.request} onDecide={(decision, scope) => {
            if (event.event.kind !== "tool_approval_requested") return;
            const request = event.event.request;
            return scope === undefined
              ? onDecideApproval(event.run_id, request.approval_id, request.call_hash, decision)
              : onDecideApproval(event.run_id, request.approval_id, request.call_hash, decision, scope);
          }} />}
          {event.event.kind === "tool_dispatch_uncertain" && onResolveUncertain && !isV4UncertainResolved(events, event.event.call_id) && <V4UncertainCard locale={locale} onResolve={(resolution, evidence) => onResolveUncertain(event.run_id, event.event.kind === "tool_dispatch_uncertain" ? event.event.call_id : "", resolution, evidence)} />}
          {event.event.kind === "browser_tab_cleanup_required" && onCloseBrowserTabs && <BrowserTabCleanupCard locale={locale} tabs={event.event.tabs} onClose={() => onCloseBrowserTabs(event.run_id, event.event.kind === "browser_tab_cleanup_required" ? event.event.sessions : [])} />}
        </div>
      </article> })),
      ].sort((a, b) => a.sequence - b.sequence).map(({ node }) => node)}
      </section>
      {guided && <details className="v4-process-context"><summary>{zh ? "任务与阶段详情" : "Task and phase details"}</summary><GuidedV4Overview locale={locale} overview={guided} terminal={terminal} historical={historical} /></details>}
      {failed && onResume && <div className="v4-resume-run"><span>{zh ? "修正运行条件后可从已验证事件链继续。" : "Resume from the verified event chain after fixing the runtime condition."}</span><button disabled={resumeBusy} onClick={() => void resumeRun()}>{resumeBusy ? (zh ? "恢复中…" : "Resuming…") : (zh ? "继续运行" : "Resume run")}</button></div>}
      {pauseReason === "browser_connection" && onResume && <div className="v4-resume-run"><span>{zh ? "请在设置 → Browser 安装或启用 OmicsOps 扩展并连接相应会话，然后原地继续此任务。" : "Open Settings → Browser, install or enable the OmicsOps extension, connect the requested session, then resume this same task."}</span><button disabled={resumeBusy} onClick={() => void resumeRun()}>{resumeBusy ? (zh ? "恢复中…" : "Resuming…") : (zh ? "已连接，继续" : "Connected, resume")}</button></div>}
      {pauseReason === "browser_human" && onResume && <div className="v4-resume-run"><span>{zh ? "请在真实浏览器中完成人机验证或其他人工步骤；OmicsOps 不会自动求解 CAPTCHA。处理完成后原地继续。" : "Complete the CAPTCHA or other manual step in the real browser. OmicsOps never solves CAPTCHA automatically; resume this same run when finished."}</span><button disabled={resumeBusy} onClick={() => void resumeRun()}>{resumeBusy ? (zh ? "恢复中…" : "Resuming…") : (zh ? "已人工处理，继续" : "Handled, resume")}</button></div>}
    </div>
    </details>
  </>;
}
const GUIDED_V4_PHASES: AgentV4Phase[] = ["routing", "discovery", "clarification", "organizing", "executing", "verifying"];
type GuidedTaskList = { revision: number; changeSummary: string; tasks: AgentV4Task[] };
type GuidedCycle = { cycleId: number; startedAt?: string; finishedAt?: string };
type GuidedToolBatch = {
  batchId: number;
  cycleId: number;
  phase: AgentV4Phase;
  toolNames: string[];
  callIds: string[];
  startedAt?: string;
  finishedAt?: string;
  startedSequence?: number;
  finishedSequence?: number;
  durationMs?: number;
  succeeded?: number;
  failed?: number;
};
type GuidedV4OverviewData = {
  taskShape?: AgentV4TaskShape;
  taskShapeSource?: "model" | "host";
  taskShapeReason?: string;
  currentPhase: AgentV4Phase;
  taskList?: GuidedTaskList;
  cycles: Map<number, GuidedCycle>;
  batches: GuidedToolBatch[];
};

function buildGuidedV4Overview(events: AgentRunEventV4[]): GuidedV4OverviewData | null {
  const ordered = [...events].sort((left, right) => left.sequence - right.sequence);
  const hasGuidedEvent = ordered.some(({ event }) => event.kind === "task_shape_selected"
    || event.kind === "phase_changed"
    || event.kind === "cycle_started"
    || event.kind === "cycle_finished"
    || event.kind === "task_list_updated"
    || event.kind === "tool_batch_started"
    || event.kind === "tool_batch_finished");
  if (!hasGuidedEvent) return null;

  let taskShape: AgentV4TaskShape | undefined;
  let taskShapeSource: "model" | "host" | undefined;
  let taskShapeReason: string | undefined;
  let currentPhase: AgentV4Phase | undefined;
  let explicitPhaseSeen = false;
  let taskList: GuidedTaskList | undefined;
  const cycles = new Map<number, GuidedCycle>();
  const batches = new Map<number, GuidedToolBatch>();

  for (const item of ordered) {
    const event = item.event;
    if (event.kind === "task_shape_selected") {
      taskShape = event.task_shape;
      taskShapeSource = event.source;
      taskShapeReason = event.reason;
    } else if (event.kind === "phase_changed") {
      currentPhase = event.phase;
      explicitPhaseSeen = true;
    } else if (event.kind === "cycle_started" || event.kind === "cycle_finished") {
      const cycle = cycles.get(event.cycle_id) ?? { cycleId: event.cycle_id };
      if (event.kind === "cycle_started") cycle.startedAt = item.occurred_at;
      else cycle.finishedAt = item.occurred_at;
      cycles.set(event.cycle_id, cycle);
    } else if (event.kind === "task_list_updated") {
      taskList = { revision: event.revision, changeSummary: event.change_summary ?? "", tasks: event.tasks };
      if (!explicitPhaseSeen) currentPhase = "organizing";
    } else if (event.kind === "tool_batch_started" || event.kind === "tool_batch_finished") {
      const visibleBatch = visibleToolBatchEntries(event.tool_names, event.call_ids);
      if (visibleBatch === null) {
        batches.delete(event.batch_id);
        continue;
      }
      const batch = batches.get(event.batch_id) ?? {
        batchId: event.batch_id,
        cycleId: event.cycle_id,
        phase: event.phase,
        toolNames: [],
        callIds: [],
      };
      batch.cycleId = event.cycle_id;
      batch.phase = event.phase;
      batch.toolNames = visibleBatch.toolNames;
      batch.callIds = visibleBatch.callIds;
      if (event.kind === "tool_batch_started") {
        batch.startedAt = item.occurred_at;
        batch.startedSequence = item.sequence;
      } else {
        batch.finishedAt = item.occurred_at;
        batch.finishedSequence = item.sequence;
        batch.durationMs = event.duration_ms ?? elapsedMilliseconds(batch.startedAt, item.occurred_at);
        batch.succeeded = event.succeeded;
        batch.failed = event.failed;
      }
      batches.set(event.batch_id, batch);
      if (!explicitPhaseSeen) currentPhase = event.phase;
    }
  }

  return {
    taskShape,
    taskShapeSource,
    taskShapeReason,
    currentPhase: currentPhase ?? inferGuidedV4Phase(ordered),
    taskList,
    cycles,
    batches: [...batches.values()].sort((left, right) => (left.startedSequence ?? left.finishedSequence ?? 0) - (right.startedSequence ?? right.finishedSequence ?? 0)),
  };
}

function visibleToolBatchEntries(toolNames: string[], callIds: string[]) {
  if (toolNames.length === 0) return { toolNames: [], callIds };
  const visibleIndexes = toolNames
    .map((toolName, index) => ({ toolName, index }))
    .filter(({ toolName }) => toolName === "agent.update_tasks" || !isInternalAgentTool(toolName));
  if (visibleIndexes.length === 0) return null;
  return {
    toolNames: visibleIndexes.map(({ toolName }) => toolName),
    callIds: visibleIndexes.map(({ index }) => callIds[index]).filter((callId): callId is string => Boolean(callId)),
  };
}

function inferGuidedV4Phase(events: AgentRunEventV4[]): AgentV4Phase {
  for (let index = events.length - 1; index >= 0; index -= 1) {
    const event = events[index].event;
    if (event.kind === "deterministic_verification_finished" || event.kind === "reviewer_finished" || event.kind === "completion_proposed" || event.kind === "run_completed") return "verifying";
    if (event.kind === "input_requested") return "clarification";
    if (event.kind === "tool_batch_started" || event.kind === "tool_batch_finished" || event.kind === "tool_requested" || event.kind === "tool_dispatch_started" || event.kind === "tool_finished") return "executing";
    if (event.kind === "task_list_updated") return "organizing";
    if (event.kind === "request_routed") return "routing";
  }
  return "routing";
}

function elapsedMilliseconds(startedAt: string | undefined, finishedAt: string) {
  if (!startedAt) return undefined;
  const started = Date.parse(startedAt);
  const finished = Date.parse(finishedAt);
  return Number.isFinite(started) && Number.isFinite(finished) && finished >= started ? finished - started : undefined;
}

function GuidedV4Overview({ locale, overview, terminal, historical }: { locale: Locale; overview: GuidedV4OverviewData; terminal?: AgentRunEventV4; historical: boolean }) {
  const zh = locale === "zh-CN";
  const currentIndex = Math.max(0, GUIDED_V4_PHASES.indexOf(overview.currentPhase));
  const completedRun = terminal?.event.kind === "run_completed";
  const failedRun = terminal?.event.kind === "run_failed";
  const isFastPath = overview.taskShape === "fast";
  const shapeLabel = overview.taskShape === "fast" ? (zh ? "快速路径" : "Fast path") : overview.taskShape === "multi_step" ? (zh ? "多步骤路径" : "Multi-step path") : (zh ? "引导轨迹" : "Guided trajectory");
  return <section className={`v4-guided-overview ${isFastPath ? "is-fast" : ""} ${historical ? "is-historical" : ""}`} aria-label={zh ? "Agent 阶段轨迹" : "Agent guided trajectory"}>
    <header className="v4-guided-heading"><div><strong>{zh ? "Agent 轨迹" : "Agent trajectory"}</strong><small>{shapeLabel}</small>{overview.taskShapeSource && <small>{zh ? "来源" : "source"}: {overview.taskShapeSource}</small>}</div>{overview.taskShape && <span className="v4-task-shape">{overview.taskShape}</span>}</header>
    {overview.taskShapeReason && <p className="v4-task-shape-reason">{overview.taskShapeReason}</p>}
    <ol className="v4-phase-list">
      {GUIDED_V4_PHASES.map((phase, index) => {
        const state = index < currentIndex || completedRun ? "completed" : index === currentIndex ? (failedRun ? "failed" : "active") : "pending";
        return <li className={`v4-phase-item ${state}`} data-phase={phase} key={phase} aria-current={index === currentIndex ? "step" : undefined}><span className="v4-phase-dot" /><b>{phase}</b><small>{phaseStateLabel(state, zh)}</small></li>;
      })}
    </ol>
    {isFastPath && <small className="v4-fast-path-note">{zh ? "快速路径：紧凑轨迹。" : "Fast path: compact trajectory."}</small>}
    {overview.taskList && !isFastPath && overview.taskList.tasks.length > 0 && <V4TaskList locale={locale} taskList={overview.taskList} />}
    {overview.batches.length > 0 && <section className="v4-tool-batches" aria-label={zh ? "工具批次" : "Tool batches"}>{overview.batches.map((batch) => <V4ToolBatch locale={locale} batch={batch} cycle={overview.cycles.get(batch.cycleId)} key={batch.batchId} />)}</section>}
  </section>;
}

function V4TaskList({ locale, taskList }: { locale: Locale; taskList: GuidedTaskList }) {
  const zh = locale === "zh-CN";
  const statuses: AgentV4Task["status"][] = ["pending", "in_progress", "completed", "blocked"];
  const counts: Record<AgentV4Task["status"], number> = { pending: 0, in_progress: 0, completed: 0, blocked: 0 };
  taskList.tasks.forEach((task) => { counts[task.status] += 1; });
  return <section className="v4-task-list" aria-label={zh ? "任务列表" : "Task list"} aria-readonly="true">
    <header><b>{zh ? "任务列表" : "Task list"}</b><span>revision {taskList.revision}</span></header>
    {taskList.changeSummary && <p>{taskList.changeSummary}</p>}
    <div className="v4-task-counts" aria-label={statuses.map((status) => `${status} ${counts[status]}`).join(", ")}>
      {statuses.map((status) => <span data-status={status} key={status}><b>{counts[status]}</b><small>{status} · {taskStatusLabel(status, zh)}</small></span>)}
    </div>
    <ul>{taskList.tasks.map((task) => <li className={`v4-task-row ${task.status}`} data-task-id={task.id} key={task.id}><span className="v4-task-status-dot" /><div><b>{task.title}</b><small>{task.status} · {taskStatusLabel(task.status, zh)}</small>{task.status === "blocked" && task.blocked_reason && <em>{task.blocked_reason}</em>}</div></li>)}</ul>
  </section>;
}

function V4ToolBatch({ locale, batch, cycle }: { locale: Locale; batch: GuidedToolBatch; cycle?: GuidedCycle }) {
  const zh = locale === "zh-CN";
  const steps = Math.max(batch.callIds.length, batch.toolNames.length);
  const finished = batch.finishedSequence !== undefined;
  const duration = batch.durationMs === undefined ? "" : ` · ${zh ? "耗时" : "duration"} ${formatDuration(batch.durationMs)}`;
  const summary = finished
    ? `${zh ? "已运行" : "Ran"} ${steps} ${zh ? "步" : steps === 1 ? "step" : "steps"}${duration}`
    : `${zh ? "执行中" : "Running"} ${steps} ${zh ? "步" : steps === 1 ? "step" : "steps"}`;
  const hasFailure = typeof batch.failed === "number" && batch.failed > 0;
  const status = !finished ? "running" : hasFailure ? "failed" : "succeeded";
  return <details className={`v4-tool-batch ${status}`}>
    <summary className="v4-tool-batch-heading"><span><b>{zh ? "阶段" : "stage"} · {batch.phase}</b><small>cycle {cycle?.cycleId ?? batch.cycleId}</small></span><strong>{summary}</strong></summary>
    <div className="v4-tool-batch-body"><small>{zh ? "工具" : "Tools"}: {batch.toolNames.length ? batch.toolNames.map((toolName) => toolDisplayLabel(toolName, zh)).join(" · ") : (zh ? "未提供工具名称" : "No tool names provided")}</small>{batch.callIds.length > 0 && <small>{zh ? "调用" : "Calls"}: {batch.callIds.join(" · ")}</small>}{(batch.succeeded !== undefined || batch.failed !== undefined) && <small>{zh ? "结果" : "Outcome"}: {batch.succeeded !== undefined ? `${zh ? "成功" : "succeeded"} ${formatMetric(batch.succeeded)}` : ""}{batch.succeeded !== undefined && batch.failed !== undefined ? " · " : ""}{batch.failed !== undefined ? `${zh ? "失败" : "failed"} ${formatMetric(batch.failed)}` : ""}</small>}</div>
  </details>;
}

function phaseStateLabel(state: "pending" | "active" | "completed" | "failed", zh: boolean) {
  return state === "completed" ? (zh ? "已完成" : "Completed") : state === "failed" ? (zh ? "失败" : "Failed") : state === "active" ? (zh ? "进行中" : "Active") : (zh ? "待处理" : "Pending");
}
function taskStatusLabel(status: AgentV4Task["status"], zh: boolean) {
  return status === "pending" ? (zh ? "待处理" : "Pending") : status === "in_progress" ? (zh ? "进行中" : "In progress") : status === "completed" ? (zh ? "已完成" : "Completed") : (zh ? "已阻塞" : "Blocked");
}
function formatMetric(value: number) { return String(value); }
function formatDuration(milliseconds: number) {
  if (milliseconds < 1_000) return `${Math.max(0, Math.round(milliseconds))} ms`;
  if (milliseconds < 60_000) return `${(milliseconds / 1_000).toFixed(milliseconds % 1_000 === 0 ? 0 : 1)}s`;
  return `${Math.floor(milliseconds / 60_000)}m ${Math.round((milliseconds % 60_000) / 1_000)}s`;
}

function V4ApprovalCard({ locale, request, onDecide }: { locale: Locale; request: Extract<AgentRunEventV4["event"], { kind: "tool_approval_requested" }>['request']; onDecide: (decision: "approved" | "denied", browserScope?: BrowserApprovalScopeV4) => Promise<void> | void }) {
  const zh = locale === "zh-CN";
  const [busy, setBusy] = useState(false);
  const browser = request.call.tool_id === "browser_setup" || request.call.tool_id.startsWith("web_");
  const [browserScope, setBrowserScope] = useState<BrowserApprovalScopeV4>("once");
  const busyRef = useRef(false);
  const decide = async (decision: "approved" | "denied") => {
    if (busyRef.current) return;
    busyRef.current = true;
    setBusy(true);
    try { await onDecide(decision, decision === "approved" && browser ? browserScope : undefined); } finally { busyRef.current = false; setBusy(false); }
  };
  return <section className="v4-approval-card" aria-label={zh ? "工具审批" : "Tool approval"}><b>{browser ? (zh ? "宿主浏览器授权" : "Host browser authorization") : (zh ? "等待工具审批" : "Tool approval required")}</b><code>{request.call.tool_id}</code><p>{request.reason}</p>{browser && <label>{zh ? "授权范围" : "Authorization scope"}<select aria-label={zh ? "浏览器授权范围" : "Browser authorization scope"} disabled={busy} value={browserScope} onChange={(event) => setBrowserScope(event.target.value as BrowserApprovalScopeV4)}><option value="once">{zh ? "仅此一次" : "Once"}</option><option value="conversation">{zh ? "本次对话" : "Conversation"}</option><option value="project">{zh ? "本项目" : "Project"}</option><option value="global">{zh ? "全局" : "Global"}</option></select></label>}<small>{request.effect} · SHA-256 {request.call_hash.slice(0, 12)}</small><div><button disabled={busy} onClick={() => void decide("denied")}>{zh ? "拒绝" : "Deny"}</button><button disabled={busy} onClick={() => void decide("approved")}>{zh ? "批准并继续" : "Approve and continue"}</button></div></section>;
}
function BrowserTabCleanupCard({ locale, tabs, onClose }: { locale: Locale; tabs: import("../../types").BrowserTabSummaryV4[]; onClose: () => Promise<void> | void }) {
  const zh = locale === "zh-CN";
  const [state, setState] = useState<"idle" | "busy" | "closed" | "error">("idle");
  async function closeTabs() {
    if (state === "busy" || state === "closed") return;
    setState("busy");
    try { await onClose(); setState("closed"); } catch { setState("error"); }
  }
  return <section className="v4-approval-card" aria-label={zh ? "浏览器标签清理" : "Browser tab cleanup"}><b>{zh ? "确认清理本轮标签" : "Confirm run tab cleanup"}</b>{tabs.slice(0, 8).map((tab) => <small key={tab.tab_id}>{tab.title || tab.origin} · {tab.origin}</small>)}{tabs.length > 8 && <small>{zh ? `另有 ${tabs.length - 8} 个标签` : `${tabs.length - 8} more tabs`}</small>}{state === "error" && <p role="alert">{zh ? "无法确认标签已关闭；请检查浏览器连接。" : "Could not confirm tab closure; check the browser connection."}</p>}<div><button disabled={state === "busy" || state === "closed"} onClick={() => void closeTabs()}>{state === "closed" ? (zh ? "已关闭" : "Closed") : state === "busy" ? (zh ? "关闭中…" : "Closing…") : (zh ? "关闭本轮标签" : "Close run tabs")}</button></div></section>;
}
function V4UncertainCard({ locale, onResolve }: { locale: Locale; onResolve: (resolution: "side_effect_observed" | "side_effect_not_observed" | "compensated", evidence: string) => Promise<void> | void }) {
  const zh = locale === "zh-CN";
  const [resolution, setResolution] = useState<"side_effect_observed" | "side_effect_not_observed" | "compensated">("side_effect_not_observed");
  const [evidence, setEvidence] = useState("");
  const [busy, setBusy] = useState(false);
  const busyRef = useRef(false);
  async function resolve() {
    const trimmedEvidence = evidence.trim();
    if (!trimmedEvidence || busyRef.current) return;
    busyRef.current = true;
    setBusy(true);
    try { await onResolve(resolution, trimmedEvidence); } finally { busyRef.current = false; setBusy(false); }
  }
  return <section className="v4-uncertain-card" aria-label={zh ? "不确定副作用处理" : "Uncertain dispatch resolution"}><b>{zh ? "需要核实工具副作用" : "Verify the possible side effect"}</b><select disabled={busy} value={resolution} onChange={(event) => setResolution(event.target.value as typeof resolution)}><option value="side_effect_not_observed">{zh ? "未观察到副作用，可安全重试" : "No side effect observed"}</option><option value="side_effect_observed">{zh ? "已观察到副作用，不要重试" : "Side effect observed"}</option><option value="compensated">{zh ? "副作用已补偿" : "Side effect compensated"}</option></select><input disabled={busy} aria-label={zh ? "核验证据" : "Verification evidence"} value={evidence} onChange={(event) => setEvidence(event.target.value)} placeholder={zh ? "填写核验证据" : "Describe verification evidence"} /><button disabled={busy || !evidence.trim()} onClick={() => void resolve()}>{busy ? (zh ? "处理中…" : "Resolving…") : (zh ? "保存证据并继续" : "Save evidence and continue")}</button></section>;
}
function isV4ApprovalDecided(events: AgentRunEventV4[], approvalId: string) {
  return events.some((event) => event.event.kind === "tool_approval_decided" && event.event.approval_id === approvalId);
}
function isV4UncertainResolved(events: AgentRunEventV4[], callId: string) {
  return events.some((event) => event.event.kind === "tool_dispatch_resolved" && event.event.call_id === callId);
}
function isV4QuestionAnswered(events: AgentRunEventV4[], questionId: string) {
  return events.some((event) => event.event.kind === "user_input_answered" && event.event.question_id === questionId);
}
function MarkdownContent({ markdown }: { markdown: string }) {
  return <div className="markdown-content"><ReactMarkdown remarkPlugins={[remarkGfm]}>{markdown}</ReactMarkdown></div>;
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
    if (modelText.trim()) entries.push({ event, lastEvent, modelText });
  }
  return entries;
}

type MergedV4ToolCall = {
  callId: string;
  toolId: string;
  argumentsPreview: string;
  firstSequence: number;
  lastSequence: number;
  outcome?: string;
  startedAt?: string;
  finishedAt?: string;
  subject?: string;
  status: "requested" | "running" | "succeeded" | "failed" | "reused";
};
function mergeV4ToolCalls(events: AgentRunEventV4[]): MergedV4ToolCall[] {
  const byCall = new Map<string, MergedV4ToolCall>();
  const update = (callId: string, sequence: number, toolId: string, args?: Record<string, unknown>, status?: MergedV4ToolCall["status"], outcome?: string) => {
    if (isInternalAgentTool(toolId)) return;
    const current = byCall.get(callId);
    if (!current) {
      byCall.set(callId, { callId, toolId, argumentsPreview: args ? limitText(JSON.stringify(redactToolArguments(args)), 500) : "", firstSequence: sequence, lastSequence: sequence, outcome, subject: toolSubject(args), status: status ?? "requested" });
      return;
    }
    current.lastSequence = Math.max(current.lastSequence, sequence);
    if (toolId) current.toolId = toolId;
    if (args) {
      current.argumentsPreview = limitText(JSON.stringify(redactToolArguments(args)), 500);
      current.subject = toolSubject(args);
    }
    if (outcome !== undefined) current.outcome = outcome;
    if (status) current.status = status;
  };
  for (const item of events) {
    const event = item.event;
    if (event.kind === "tool_requested") update(event.call.call_id, item.sequence, event.call.tool_id, event.call.arguments, "requested");
    else if (event.kind === "tool_dispatch_started") update(event.call_id, item.sequence, event.tool_id, undefined, "running");
    else if (event.kind === "tool_finished") update(event.outcome.call_id, item.sequence, event.outcome.tool_id, undefined, event.outcome.succeeded ? "succeeded" : "failed", event.outcome.model_content);
    else if (event.kind === "tool_outcome_reused") update(event.outcome.call_id, item.sequence, event.outcome.tool_id, undefined, "reused", event.outcome.model_content);
  }
  for (const item of events) {
    const event = item.event;
    if (event.kind === "tool_requested" || event.kind === "tool_dispatch_started") {
      const tool = byCall.get(event.kind === "tool_requested" ? event.call.call_id : event.call_id);
      if (tool && (event.kind === "tool_dispatch_started" || !tool.startedAt)) tool.startedAt = item.occurred_at;
    } else if (event.kind === "tool_finished") {
      const tool = byCall.get(event.outcome.call_id);
      if (tool) tool.finishedAt = item.occurred_at;
    }
  }
  return [...byCall.values()];
}
function isInternalAgentTool(toolId: string) {
  return toolId === "agent.complete" || toolId === "agent.request_input" || toolId === "agent.propose_plan" || toolId === "agent.update_tasks" || toolId === "agent.route" || toolId === "agent.route_request";
}
function toolSubject(args?: Record<string, unknown>) {
  if (!args) return undefined;
  for (const key of ["path", "file_path", "relative_path", "command", "cmd", "skill_name", "name", "artifact_id", "query"]) {
    const value = args[key];
    if (typeof value === "string" && value.trim()) return value.trim();
  }
  return undefined;
}
function compactToolLabel(toolId: string) {
  const labels: Record<string, string> = { "project.read": "read", "project.write": "write", "project.edit": "edit", "project.list": "list", "use_skill": "SKILL", "runtime.execute": "execute", "runtime.python": "python", "runtime.r": "R", "agent.delegate": "agent" };
  return labels[toolId] ?? toolId;
}
function eventDuration(start?: string, end?: string) {
  if (!start || !end) return "";
  const duration = Date.parse(end) - Date.parse(start);
  return Number.isFinite(duration) && duration >= 0 ? `${Number((duration / 1000).toFixed(1))}s` : "";
}
function toolMetrics(tool: MergedV4ToolCall, zh: boolean) {
  const duration = tool.status === "reused" ? "" : eventDuration(tool.startedAt, tool.finishedAt);
  const lines = tool.outcome === undefined ? undefined : tool.outcome === "" ? 0 : tool.outcome.replace(/\r?\n$/, "").split(/\r?\n/).length;
  return [duration, lines === undefined ? "" : `${lines} ${zh ? "行" : lines === 1 ? "line" : "lines"}`].filter(Boolean).join(" · ");
}
function toolDisplayLabel(toolId: string, zh: boolean) {
  const labels: Record<string, [string, string]> = {
    "project.read": ["读取项目文件", "Read project file"],
    "project.list": ["浏览项目文件", "List project files"],
    "project.write": ["写入项目文件", "Write project file"],
    "artifact.verify": ["验证分析产物", "Verify artifact"],
    "runtime.execute": ["运行分析代码", "Run analysis code"],
    "runtime.python": ["运行 Python", "Run Python"],
    "runtime.r": ["运行 R", "Run R"],
    "agent.delegate": ["执行子任务", "Run delegated task"],
    "agent.update_tasks": ["更新任务列表", "Update task list"],
  };
  const label = labels[toolId];
  return label ? label[zh ? 0 : 1] : toolId;
}
function toolStatusLabel(status: MergedV4ToolCall["status"], zh: boolean) {
  return status === "requested" ? (zh ? "已请求" : "Requested") : status === "running" ? (zh ? "执行中" : "Running") : status === "succeeded" ? (zh ? "成功" : "Succeeded") : status === "failed" ? (zh ? "失败" : "Failed") : (zh ? "复用结果" : "Reused");
}
function limitText(value: string, max: number) {
  return value.length > max ? `${value.slice(0, max)}…` : value;
}
function redactToolArguments(value: unknown, key = ""): unknown {
  if (/(?:password|passwd|token|secret|credential|authorization|api[_-]?key|private[_-]?key)/i.test(key)) return "[REDACTED]";
  if (Array.isArray(value)) return value.map((item) => redactToolArguments(item));
  if (value && typeof value === "object") return Object.fromEntries(Object.entries(value).map(([childKey, child]) => [childKey, redactToolArguments(child, childKey)]));
  return value;
}
function isToolTrajectoryEvent(event: AgentRunEventV4) {
  return event.event.kind === "tool_requested" || event.event.kind === "tool_dispatch_started" || event.event.kind === "tool_finished" || event.event.kind === "tool_outcome_reused";
}
function isHiddenTrajectoryEvent(event: AgentRunEventV4) {
  return event.event.kind === "run_spec_frozen"
    || event.event.kind === "request_routed"
    || event.event.kind === "task_shape_selected"
    || event.event.kind === "phase_changed"
    || event.event.kind === "cycle_started"
    || event.event.kind === "cycle_finished"
    || event.event.kind === "task_list_updated"
    || event.event.kind === "tool_batch_started"
    || event.event.kind === "tool_batch_finished"
    || event.event.kind === "completion_proposed"
    || event.event.kind === "completion_proposal_submitted"
    || event.event.kind === "deterministic_verification_finished"
    || event.event.kind === "reviewer_finished"
    || event.event.kind === "reviewer_correction_requested"
    || event.event.kind === "run_completed"
    || event.event.kind === "run_cancelled";
}
function V4AnswerForm({ locale, onSubmit }: { locale: Locale; onSubmit: (answer: string) => Promise<void> | void }) {
  const [answer, setAnswer] = useState("");
  const [busy, setBusy] = useState(false);
  const busyRef = useRef(false);
  const zh = locale === "zh-CN";
  async function submit() {
    const trimmedAnswer = answer.trim();
    if (!trimmedAnswer || busyRef.current) return;
    busyRef.current = true;
    setBusy(true);
    try { await onSubmit(trimmedAnswer); } finally { busyRef.current = false; setBusy(false); }
  }
  return <div className="v4-answer"><input disabled={busy} aria-label={zh ? "回答 V4 问题" : "Answer V4 question"} value={answer} onChange={(event) => setAnswer(event.target.value)} /><button disabled={busy || !answer.trim()} onClick={() => void submit()}>{busy ? (zh ? "恢复中…" : "Resuming…") : (zh ? "回答并恢复" : "Answer and resume")}</button></div>;
}
function v4EventLabel(event: AgentRunEventV4, zh: boolean) { if (event.event.kind === "run_created") return event.event.mode === "execute" ? (zh ? "任务启动" : "Task started") : (zh ? "规划启动" : "Planning started"); const labels: Record<string, string> = { request_routed: zh ? "请求已分类" : "Request routed", browser_connection_required: zh ? "需要连接浏览器" : "Browser connection required", browser_human_intervention_required: zh ? "浏览器需要人工处理" : "Browser intervention required", browser_tab_cleanup_required: zh ? "浏览器标签待清理" : "Browser tabs need cleanup", plan_proposed: zh ? "计划已冻结" : "Plan frozen", plan_approved: zh ? "计划获批" : "Plan approved", mode_changed: zh ? "执行模式" : "Execution mode", tool_requested: zh ? "工具请求" : "Tool request", tool_approval_requested: zh ? "等待工具审批" : "Tool approval required", tool_approval_decided: zh ? "工具审批已决定" : "Tool approval decided", tool_dispatch_uncertain: zh ? "工具状态不确定" : "Tool dispatch uncertain", tool_dispatch_resolved: zh ? "不确定状态已核实" : "Uncertain dispatch resolved", tool_finished: zh ? "工具结果" : "Tool result", input_requested: zh ? "需要补充信息" : "Input required", user_input_answered: zh ? "用户已回答" : "User answered", completion_proposed: zh ? "完成提案" : "Completion proposed", run_completed: zh ? "运行完成" : "Run completed", run_failed: zh ? "运行失败" : "Run failed", run_cancelled: zh ? "运行取消" : "Run cancelled" }; return labels[event.event.kind] ?? event.event.kind; }
function v4EventContent(event: AgentRunEventV4, zh: boolean) { if (event.event.kind === "request_routed") return event.event.route === "research_retrieval" ? (zh ? "科研检索流水线" : "Research retrieval workflow") : (zh ? "自适应执行" : "Adaptive execution"); if (event.event.kind === "browser_connection_required" || event.event.kind === "browser_human_intervention_required" || event.event.kind === "browser_tab_cleanup_required") return event.event.message; if (event.event.kind === "tool_requested") return event.event.call.tool_id; if (event.event.kind === "tool_approval_requested") return `${event.event.request.call.tool_id}: ${event.event.request.reason}`; if (event.event.kind === "tool_approval_decided") return event.event.decision === "approved" ? (zh ? "用户已批准" : "Approved by user") : (zh ? "用户已拒绝" : "Denied by user"); if (event.event.kind === "tool_dispatch_uncertain") return zh ? `调用 ${event.event.tool_id} 的副作用尚未确认` : `The side effect of ${event.event.tool_id} is not yet known`; if (event.event.kind === "tool_dispatch_resolved") return event.event.evidence; if (event.event.kind === "tool_finished") return event.event.outcome.model_content; if (event.event.kind === "plan_proposed") return `${event.event.plan.steps.length} ${zh ? "个步骤" : "steps"} · SHA-256 ${event.event.plan_hash.slice(0, 12)}`; if (event.event.kind === "model_text") return event.event.text; if (event.event.kind === "input_requested") return event.event.question; if (event.event.kind === "user_input_answered") return zh ? `已提交回答：${event.event.answer}` : `Answer submitted: ${event.event.answer}`; if (event.event.kind === "run_failed") return event.event.message; return ""; }
function isPreviewImage(path: string) { return /\.(png|jpe?g|gif|webp|bmp)$/i.test(path); }
function ArtifactPreview({ title, locale, images, selectedPath, preview, busy, error, onSelect, onLoad }: { title: string; locale: Locale; images: import("../../types").RemoteFileEntry[]; selectedPath: string; preview: ProjectImagePreview | null; busy: boolean; error: string; onSelect: (path: string) => void; onLoad: () => Promise<void> | void }) {
  const zh = locale === "zh-CN";
  return <div className="artifact-preview"><div className="preview-picker"><label>{zh ? "选择项目图片" : "Select project image"}<select aria-label={zh ? "选择项目图片" : "Select project image"} value={selectedPath} onChange={(event) => onSelect(event.target.value)}><option value="">{images.length ? (zh ? "请选择图片" : "Choose an image") : (zh ? "未发现图片文件" : "No image files found")}</option>{images.map((entry) => <option value={entry.relative_path} key={entry.relative_path}>{entry.relative_path}</option>)}</select></label><button disabled={!selectedPath || busy} onClick={() => void onLoad()}>{busy ? (zh ? "加载中…" : "Loading…") : (zh ? "显示图片" : "Show image")}</button></div>{error && <div className="preview-error" role="alert">{error}</div>}<div className="project-image-stage" aria-label={title}>{preview ? <img src={preview.data_url} alt={preview.relative_path} /> : <div className="preview-empty">{images.length ? (zh ? "选择图片后点击“显示图片”" : "Choose an image and click Show image") : (zh ? "项目中暂未发现 PNG、JPEG、GIF、WebP 或 BMP 图片" : "No PNG, JPEG, GIF, WebP, or BMP images were found")}</div>}</div><div className="artifact-meta"><b>{preview?.relative_path ?? title}</b>{preview && <><span>{preview.mime_type} · {formatPreviewBytes(preview.size_bytes)}</span><small>SHA-256 {preview.sha256}</small></>}</div></div>;
}
function formatPreviewBytes(bytes: number) { return bytes < 1024 * 1024 ? `${Math.max(1, Math.round(bytes / 1024))} KB` : `${(bytes / 1024 / 1024).toFixed(1)} MB`; }
function isTerminalAgentEventV4(event: AgentRunEventV4) {
  return event.event.kind === "run_completed" || event.event.kind === "run_failed" || event.event.kind === "run_cancelled" || event.event.kind === "run_needs_attention";
}

function effectiveTerminalAgentEventV4(events: AgentRunEventV4[]): AgentRunEventV4 | undefined {
  for (let index = events.length - 1; index >= 0; index -= 1) {
    if (!isTerminalAgentEventV4(events[index])) continue;
    const laterEventsAreOnlyCleanup = events
      .slice(index + 1)
      .every((event) => event.event.kind === "browser_tab_cleanup_required");
    return laterEventsAreOnlyCleanup ? events[index] : undefined;
  }
  return undefined;
}

function getV4PauseReason(events: AgentRunEventV4[]): "approval" | "input" | "browser_connection" | "browser_human" | "uncertain" | null {
  for (let index = events.length - 1; index >= 0; index -= 1) {
    const event = events[index].event;
    if (event.kind === "tool_approval_requested") return isV4ApprovalDecided(events, event.request.approval_id) ? null : "approval";
    if (event.kind === "input_requested") return isV4QuestionAnswered(events, event.question_id) ? null : "input";
    if (event.kind === "browser_connection_required") return "browser_connection";
    if (event.kind === "browser_human_intervention_required") return "browser_human";
    if (event.kind === "tool_finished" && event.outcome.succeeded
      && (event.outcome.tool_id.startsWith("browser_") || event.outcome.tool_id.startsWith("web_"))) return null;
    if (event.kind === "tool_dispatch_uncertain") return isV4UncertainResolved(events, event.call_id) ? null : "uncertain";
    if (event.kind === "tool_approval_decided" || event.kind === "user_input_answered" || event.kind === "tool_dispatch_resolved" || isTerminalAgentEventV4(events[index])) return null;
  }
  return null;
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

function Notebook({ locale, entries, artifacts, facts, onSearch, onExport }: { locale: Locale; entries: NotebookEntry[]; artifacts: ProjectArtifact[]; facts: MemoryFact[]; onSearch?: (query: string, dimension?: string) => Promise<void> | void; onExport?: (format: "markdown" | "json" | "bundle") => Promise<void> | void }) { const zh = locale === "zh-CN"; const [query, setQuery] = useState(""); const [dimension, setDimension] = useState(""); return <div className="notebook-panel"><div className="notebook-actions"><input aria-label={zh ? "检索记忆" : "Search memory"} value={query} onChange={(event) => setQuery(event.target.value)} placeholder={zh ? "按任务、环境或产物检索" : "Search tasks, environments, artifacts"} /><select aria-label={zh ? "记忆维度" : "Memory dimension"} value={dimension} onChange={(event) => setDimension(event.target.value)}><option value="">{zh ? "全部事实" : "All facts"}</option><option value="task">{zh ? "任务" : "Task"}</option><option value="environment">{zh ? "环境" : "Environment"}</option><option value="artifact">{zh ? "产物" : "Artifact"}</option></select><button onClick={() => void onSearch?.(query, dimension || undefined)}>{zh ? "检索" : "Search"}</button></div><div className="notebook-export"><button onClick={() => void onExport?.("markdown")}>Markdown</button><button onClick={() => void onExport?.("json")}>JSON</button><button onClick={() => void onExport?.("bundle")}>{zh ? "项目包" : "Bundle"}</button></div><div className="notebook-list">{entries.length === 0 && <div><span>{zh ? "研究记录" : "Notebook"}</span><b>{zh ? "暂无正式条目" : "No formal entries yet"}</b><p>{zh ? "Agent 完成并验证产物后会自动登记目标、方法、观察、决策与证据。" : "Verified Agent runs automatically register goals, methods, observations, decisions, and evidence."}</p></div>}{entries.map((entry) => <div key={entry.id}><span>{entry.kind}</span><b>{entry.title}</b><p>{entry.markdown}</p><small>{entry.evidence_ids.length} {zh ? "条可追溯引用" : "traceable references"}</small></div>)}</div><div className="artifact-register"><b>{zh ? "已登记产物" : "Registered artifacts"} · {artifacts.length}</b>{artifacts.map((artifact) => <small key={artifact.id}>{artifact.relative_path} · SHA-256 {artifact.sha256.slice(0, 12)}</small>)}</div><div className="memory-results"><b>{zh ? "事实记忆" : "Fact memory"} · {facts.length}</b>{facts.slice(0, 20).map((fact) => <article className={fact.conflicted_with.length ? "conflicted" : ""} key={fact.id}><span>{fact.dimension}</span><p>{fact.statement}</p><small>{fact.evidence.map((evidence) => `${evidence.source_kind}:${evidence.source_id}`).join(" · ")}</small>{fact.conflicted_with.length > 0 && <em>{zh ? "存在冲突事实，已保留双方来源" : "Conflicting fact retained with both sources"}</em>}</article>)}</div></div>; }
function RunSummary({ locale }: { locale: Locale }) { const zh = locale === "zh-CN"; return <div className="run-summary"><Activity size={24} /><b>{zh ? "运行中" : "Running"}</b><span>3 / 5 {zh ? "步骤已验证" : "steps verified"}</span><div className="task-progress"><i style={{ width: "60%" }} /></div></div>; }
