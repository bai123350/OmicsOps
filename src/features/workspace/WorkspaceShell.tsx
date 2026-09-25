import { ContextUsagePanel, type ContextUsageView } from "./ContextUsagePanel";
import { SideChatPanel } from "./SideChatPanel";
import type { SideChatController } from "./useSideChat";
import { listLocalComposerFiles, parseClipboardFilePaths, parseWorkspaceFileDrag, resolveComposerClipboardPaths, WORKSPACE_FILE_DRAG_TYPE, type WorkspaceFileReference } from "../../composer-file-api";
import type { WorkspaceSearchRequest } from "../../workspace-search";
import type { ComposerPickerCommand } from "./ComposerReferences";
import type { ComposerReference, ComposerCatalogItem, SubmitGuidanceV4Request, ConversationBranchV4, ComposerQueueItemV4, ComposerQueueActionRequestV4, UpdateComposerQueueRequestV4 } from "../../types";
import { composerReferenceCatalog } from "../../composer-reference-api";
import { ComposerReferencePicker, ComposerReferenceChips, parseComposerTrigger, referenceKey } from "./ComposerReferences";
import { ComposerFilePreviewDialog } from "./ComposerFilePreviewDialog";
import { WorkflowLibraryDialog } from "./WorkflowLibraryDialog";
import { ProjectTemplatePicker } from "./ProjectTemplatePicker";
import type { SpecialistTemplate } from "../../project-template-types";
import { useConversationBranch } from "./useConversationBranch";
import { ConversationBranchBanner } from "./ConversationBranchBanner";
import { GuidanceDialog } from "./GuidanceDialog";
import { ReviewerSettingsDialog } from "./ReviewerSettingsDialog";
import { SessionReviewDialog } from "./SessionReviewDialog";
import { ComposerAttachments } from "./ComposerAttachments";
import { useComposerAttachments } from "./useComposerAttachments";
import { useConversationAgentPreferences, type ConversationAgentPreferenceKey } from "./useConversationAgentPreferences";
import { useSessionReviews } from "./useSessionReviews";
import { supportsFastMode } from "../../fast-mode";
import { Fragment, useCallback, useEffect, useLayoutEffect, useRef, useState, type ReactNode } from "react";
import { WorkspaceNavigation, type WorkspacePage } from "./WorkspaceNavigation";
import { WorkspaceResearchPages } from "./WorkspaceResearchPages";
import { CollectSourceButton } from "./CollectSourceButton";
import { messageLibrarySource } from "./librarySources";
import { saveWorkspaceLibraryItem } from "../../workspace-navigation-api";
import type { SaveWorkspaceLibraryItemRequest } from "../../workspace-navigation-types";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import {
  Activity, Bot, Check, ChevronRight, ClipboardList, Copy, Expand, FileBarChart, FileText,
  Folder, Hand, NotebookPen, Plus,
  Search, Settings, Shield, ShieldAlert, ShieldCheck, Square, X, Monitor, ChevronDown, Gauge, PanelRight, Zap,
} from "lucide-react";
import { copy, type Locale } from "./copy";
import type { AgentRunEventV4, ApprovalPolicyV4, AutonomyModeV4, BrowserApprovalScopeV4, ComposerWorkflowTemplate, ComputeBackendAvailabilityV4, ConversationCapabilitiesV4, FormalStepProposal, KernelEvent, KernelLanguage, KernelSession, MemoryFact, ModelProfile, NotebookEntry, ProposedPlanRevisionV4, ProjectArtifact, ProjectImagePreview, RunSummaryV4, SessionAgentModeV4, SyncEntry, WorkspaceConversation } from "../../types";
import { FollowUpQuestions } from "./FollowUpQuestions";
import { ConversationCapabilities } from "./ConversationCapabilities";
import { RemoteFileTree } from "./RemoteFileTree";
import { KernelPanel } from "./KernelPanel";
import { ComposeActions } from "./ComposeActions";
import { ShareConversationDialog } from "./ShareConversationDialog";
import { RuntimeDialog } from "./RuntimeDialog";
import { useWindowEscapeLayer } from "../settings/BrowserSettings";
import { useComposerSendPreference } from "../settings/useComposerSendPreference";
import { useGeneralPreferences } from "../../use-general-preferences";
import { MessageSelectionActions, type MessageSelectionQuote } from "./MessageSelectionActions";
import { collectNotebookCells, collectDelegatedTasks, collectProvenance, isSidebarPreviewImage } from "./sidebarData";
import { ArtifactCatalog, CodeNotebook, DelegatedAgents, EnvironmentContexts, ProvenancePanel } from "./SidebarPanels";
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
import "./sidebar.css";

export interface WorkspaceProject {
  connection_id?: string | null;
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
  onOpenSourceConversation?: (projectId: string, conversationId: string) => Promise<void> | void;
  onRegisterNavigationGuard?: (guard: (next: () => void, onCancel?: () => void) => void) => void;
  conversations?: WorkspaceConversation[];
  activeConversationId?: string | null;
  capabilitySummary?: ConversationCapabilitiesV4 | null;
  capabilitiesLoading?: boolean;
  capabilitiesError?: string;
  onRefreshCapabilities?: () => void;
  onSelectConversation?: (conversationId: string) => Promise<void> | void;
  onBranchSend?: (sourceMessageId: string, message: string, mode: "chat" | "plan", references: ComposerReference[], attachments: string[]) => Promise<boolean>;
  branchSendOriginalMarkdown?: string;
  branchSendBusy?: boolean;
  branchSendPending?: boolean;
  branchSendError?: boolean;
  onRetryBranchSend?: () => Promise<boolean>;
  onOpenBranch?: (branch: ConversationBranchV4) => Promise<boolean>;
  onNewConversation?: () => Promise<void> | void;
  onDeleteConversation?: (conversationId: string) => Promise<void> | void;
  onSuggestFollowUps?: (runId: string) => Promise<string[]>;
  referenceCatalog?: ComposerCatalogItem[];
  workflowCatalogVersion?: number;
  onOpenSearch?: () => void;
  searchRequest?: WorkspaceSearchRequest | null;
  onSearchRequestHandled?: (key: string) => void;
  contextUsage?: ContextUsageView | null;
  contextUsageError?: boolean;
  sideChat?: SideChatController;
  queueItems?: ComposerQueueItemV4[];
  queueError?: boolean;
  queueLoading?: boolean;
  onQueueRefresh?: () => Promise<void>;
  onQueueUpdate?: (request: UpdateComposerQueueRequestV4) => Promise<unknown>;
  onQueueAction?: (request: ComposerQueueActionRequestV4) => Promise<unknown>;
  onQueue?: (message: string, mode: "chat" | "plan", references?: ComposerReference[], attachments?: string[]) => Promise<boolean>;
  replacement?: {
    busy: boolean; pending: boolean; error: boolean; originalMarkdown?: string;
    send: (message: string, mode: "chat" | "plan", target: { run_id: string; sequence: number; event_hash: string }, references: ComposerReference[], attachments: string[]) => Promise<boolean>;
    retry: () => Promise<boolean>;
  };
  onSend?: (message: string, mode: "chat" | "plan", references?: ComposerReference[], attachments?: string[]) => Promise<boolean | void> | boolean | void;
  messages?: Array<{ id: string; role: "user" | "assistant" | "tool" | "system"; markdown: string; created_at?: string }>;
  streamingAssistant?: string;
  agentBusy?: boolean;
  agentNotice?: string;
  conversationLoadError?: string;
  onRetryConversationLoad?: () => void;
  agentRetryNotice?: string;
  modelLabel?: string;
  activeModelProfile?: ModelProfile | null;
  modelProfiles?: ModelProfile[];
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
  agentTextPreview?: import("../../types").AgentTextPreviewV4 | null;
  agentReasoningPreview?: import("../../types").AgentReasoningPreviewV4 | null;
  agentModelActivity?: import("../../types").AgentModelActivityReceiptV4 | null;
  guidanceAvailable?: boolean;
  onAnswerAgentQuestionV4?: (runId: string, questionId: string, answer: string) => Promise<void> | void;
  onDecideToolApprovalV4?: (runId: string, approvalId: string, callHash: string, decision: "approved" | "denied", browserScope?: BrowserApprovalScopeV4) => Promise<void> | void;
  onResolveUncertainV4?: (runId: string, callId: string, resolution: "side_effect_observed" | "side_effect_not_observed" | "compensated", evidence: string) => Promise<void> | void;
  onCancelRuntimeRecoveryV4?: (runId: string) => Promise<void> | void;
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

type ContextTab = "files" | "plan" | "artifacts" | "notebook" | "environment" | "provenance" | "agents" | "records" | "side-chat";
const DEFAULT_CONTEXT_TABS: ContextTab[] = ["artifacts", "agents", "files", "environment"];
const AGENT_STALL_THRESHOLD_MS = 90_000;
type ComposerCommandInvocation = { id: string; args: string; raw: string };
function parseComposerCommand(text: string): ComposerCommandInvocation | null {
  const raw = text.trim();
  const match = /^\/([a-z][a-z0-9-]*)(?:\s+([\s\S]*))?$/i.exec(raw);
  if (!match) return null;
  return { id: match[1].toLowerCase(), args: match[2] ?? "", raw };
}

export function WorkspaceShell({ project, locale, onLocaleChange, onOpenSettings, onBackToProjects, onOpenSourceConversation, onRegisterNavigationGuard, conversations = [], activeConversationId, capabilitySummary, capabilitiesLoading, capabilitiesError, onRefreshCapabilities, onSelectConversation, onNewConversation, onDeleteConversation, onOpenBranch, onBranchSend, branchSendOriginalMarkdown, branchSendBusy = false, branchSendPending = false, branchSendError = false, onRetryBranchSend, onSend, onQueue, replacement, sideChat, contextUsage = null, contextUsageError = false, queueItems = [], queueError = false, queueLoading = false, onQueueRefresh, referenceCatalog, workflowCatalogVersion = 0, onOpenSearch, searchRequest, onSearchRequestHandled, onSuggestFollowUps, messages = [], streamingAssistant = "", agentBusy = false, agentNotice = "", conversationLoadError = "", onRetryConversationLoad, agentRetryNotice = "", modelLabel, activeModelProfile, modelProfiles = [], modelPicker, composerBusy = false, modelOptions = [], modelId, onModelChange, agentMode, onAgentModeChange, conversationLocked = false, conversationHydrating = false, planActionBusy = false, v4Plan, latestPlanRevision, computeBackends = [], computeBackendId = "", containerImage = "", autonomyMode = "supervised", approvalPolicy = "risk_based", computeEnvironment = "system", computeBusy = false, onComputeBackendChange, onContainerImageChange, onAutonomyModeChange, onApprovalPolicyChange, onComputeEnvironmentChange, planLoading = false, planApproved = false, onRequestPlan, onRequestPlanRevision, onApprovePlan, onStartRun, onCancelRun, runStopping = false, canStartRun = false, runStarted = false, activeRunId, activeRunLastActivityAt, agentRunEventsV4 = [], agentTextPreview, agentReasoningPreview, agentModelActivity, guidanceAvailable = false, onAnswerAgentQuestionV4, onDecideToolApprovalV4, onResolveUncertainV4, onResumeAgentRunV4, onCancelRuntimeRecoveryV4, onCloseBrowserRunTabsV4, remoteFiles, filesBusy = false, onUploadFiles, onRefreshFiles, onDownloadFile, onPreviewImage, fileNotice, kernelSessions = [], kernelEvents = [], kernelBusy = false, kernelNotice, onStartKernel, onExecuteKernel, onInterruptKernel, onStopKernel, onPromoteKernelCell, memoryFacts = [], notebookEntries = [], projectArtifacts = [], onSearchMemory, onExportNotebook, syncEntries = [], onPauseSync, onCancelSync, onRetrySync }: Props) {
  const t = copy[locale];
  const zh = locale === "zh-CN";
  const { selectionActionsEnabled } = useGeneralPreferences();
  const [workspacePage, setWorkspacePage] = useState<WorkspacePage>("conversation");
  const [navigationCollapsed, setNavigationCollapsed] = useState(() => { try { return localStorage.getItem("omicsops.workspaceNavigationCollapsed") === "true"; } catch { return false; } });
  const leaveGuard = useRef<(next: () => void, onCancel?: () => void) => void>((next) => next());
  const pendingExcerpt = useRef<{ key: string; request: SaveWorkspaceLibraryItemRequest } | null>(null);
  const registerBeforeLeave = useCallback((guard: (next: () => void, onCancel?: () => void) => void) => { leaveGuard.current = guard; }, []);
  useEffect(() => { onRegisterNavigationGuard?.((next, onCancel) => leaveGuard.current(next, onCancel)); return () => onRegisterNavigationGuard?.((next) => next()); }, [onRegisterNavigationGuard]);
  useEffect(() => { setWorkspacePage("conversation"); }, [project.id]);
  function navigateWorkspace(page: WorkspacePage) {
    leaveGuard.current(() => {
      closeComposerMenus();
      setWorkspacePage(page === "files" ? "conversation" : page);
      if (page === "files") openSidebarSection("files");
    });
  }
  const [tab, setTab] = useState<ContextTab>("artifacts");
  const [openTabs, setOpenTabs] = useState<ContextTab[]>(DEFAULT_CONTEXT_TABS);
  const [draggedTab, setDraggedTab] = useState<ContextTab | null>(null);
  const [sidebarOpen, setSidebarOpen] = useState(false);
  const [sectionMenuOpen, setSectionMenuOpen] = useState(false);
  const sidebarEvents = agentRunEventsV4.filter((event) => event.project_id === project.id && (!activeConversationId || event.conversation_id === activeConversationId));
  const notebookCells = collectNotebookCells(messages, sidebarEvents);
  const delegatedTasks = collectDelegatedTasks(sidebarEvents);
  const provenanceRows = collectProvenance(sidebarEvents);
  const sidebarSections: Array<{ id: ContextTab | "highlights"; label: string; unavailable?: string }> = [
    { id: "artifacts", label: `Artifacts (${projectArtifacts.length})` },
    { id: "agents", label: "Agents" },
    { id: "notebook", label: `Notebook (${notebookCells.length})` },
    { id: "highlights", label: "Highlights", unavailable: zh ? "需要已收藏的对话摘录；当前尚无收藏接口。" : "Requires saved conversation excerpts; the highlight API is not available." },
    { id: "files", label: "Files" },
    { id: "provenance", label: `Provenance (${provenanceRows.length})` },
    { id: "environment", label: "Environment" },
    { id: "side-chat", label: "Side chat", unavailable: sideChat ? undefined : (zh ? "独立旁聊需要桌面宿主。" : "Side chat requires the desktop host.") },
  ];
  const tabLabel = (id: ContextTab) => id === "plan" ? "Plan" : id === "records" ? (zh ? "研究记录" : "Research records") : sidebarSections.find((section) => section.id === id)!.label;
  function openSidebarSection(id: ContextTab, reveal = true) {
    if (reveal) setWorkspacePage("conversation");
    setOpenTabs((current) => current.includes(id) ? current : [...current, id]);
    setTab(id);
    setSidebarOpen(true);
    setSectionMenuOpen(false);
  }
  function closeSidebarTab(id: ContextTab) {
    const remaining = openTabs.filter((item) => item !== id);
    setOpenTabs(remaining);
    if (!remaining.length) closeSidebar();
    else if (tab === id) setTab(remaining[Math.max(0, openTabs.indexOf(id) - 1)] ?? remaining[0]);
  }
  function closeSidebar() {
    setSidebarOpen(false);
    setSectionMenuOpen(false);
  }
  useWindowEscapeLayer(sidebarOpen && workspacePage === "conversation", closeSidebar);
  useWindowEscapeLayer(sidebarOpen && workspacePage === "conversation" && sectionMenuOpen, () => setSectionMenuOpen(false));
  const [expanded, setExpanded] = useState(false);
  const [draft, setDraft] = useState("");
  const branchDrafts = useRef(new Map<string, string>());
  const branching = useConversationBranch(project.id, activeConversationId, locale, onOpenBranch ? async (branch) => {
    const opened = await onOpenBranch(branch);
    if (opened) { const key = `${branch.project_id}:${branch.source_conversation_id}`; const sourceDraft = branchDrafts.current.get(key); if (sourceDraft !== undefined) { branchDrafts.current.delete(key); setDraft(sourceDraft); } }
    return opened;
  } : undefined);
  const branchDisabled = branchSendBusy || branchSendPending || !onOpenBranch || agentBusy || conversationLocked || conversationHydrating || composerBusy || branching.busy || branching.retryAvailable;
  const lastBranchAnchor = [...messages].reverse().find((message) => message.role === "user");
  async function branchAt(sourceMessageId: string, kind: "before_user" | "after_response", title?: string): Promise<boolean> {
    if (branchDisabled) return false;
    setSendMenuOpen(false);
    if (title !== undefined && onBranchSend && (title.trim() || attachments.receipts.length)) {
      const generation = sendGenerationRef.current;
      const text = title.trim() || (zh ? "请查看附件。" : "Please inspect the attached files.");
      const references = selectedReferences;
      const ids = attachments.receipts.map((receipt) => receipt.id);
      if (attachmentsBlocked || fileReferenceOperation.current || !computeReady) return false;
      try {
        const accepted = await onBranchSend(sourceMessageId, text, planModeEnabled ? "plan" : "chat", references.map((item) => item.reference), ids);
        if (!accepted || generation !== sendGenerationRef.current) return accepted;
        setDraft((current) => current === title ? "" : current);
        setSelectedReferences((current) => current === references ? [] : current);
        attachments.clearAccepted(ids);
        return true;
      } catch {
        if (generation === sendGenerationRef.current) setSendError(true);
        return false;
      }
    }
    const sourceText = title === undefined ? messages.find((message) => message.id === sourceMessageId && message.role === "user")?.markdown : undefined;
    if (sourceText !== undefined) branchDrafts.current.set(`${project.id}:${activeConversationId}`, sourceText);
    return branching.start(sourceMessageId, kind, title?.trim() || (zh ? "会话分支" : "Conversation branch"));
  }
  const draftRef = useRef<HTMLTextAreaElement>(null);
  const handledSearchRequest = useRef<string | null>(null);
  const [referenceNotice, setReferenceNotice] = useState("");
  const [openedSearchArtifact, setOpenedSearchArtifact] = useState<ComposerCatalogItem | null>(null);
  const [selectedReferences, setSelectedReferences] = useState<ComposerCatalogItem[]>([]);
  const [fileSource, setFileSource] = useState<"local" | "remote">(() => project.connection_id || (project.connection_id === undefined && (remoteFiles?.length || onRefreshFiles)) ? "remote" : "local");
  const [localFiles, setLocalFiles] = useState<import("../../types").RemoteFileEntry[]>([]);
  const [localFilesBusy, setLocalFilesBusy] = useState(false);
  const [localFilesError, setLocalFilesError] = useState("");
  const [fileRefreshVersion, setFileRefreshVersion] = useState(0);
  const [fileReferenceBusy, setFileReferenceBusy] = useState(false);
  const fileReferenceOperation = useRef<object | null>(null);
  const remoteRefreshRef = useRef(onRefreshFiles);
  remoteRefreshRef.current = onRefreshFiles;
  useEffect(() => {
    setFileSource(project.connection_id || (project.connection_id === undefined && (remoteFiles?.length || onRefreshFiles)) ? "remote" : "local");
    setLocalFiles([]);
  }, [project.id, project.connection_id]);
  useEffect(() => {
    fileReferenceOperation.current = null;
    setFileReferenceBusy(false);
    return () => { fileReferenceOperation.current = null; };
  }, [project.id, activeConversationId]);
  useEffect(() => {
    if (!sidebarOpen || tab !== "files") return;
    if (fileSource === "remote") { void remoteRefreshRef.current?.(); return; }
    let active = true;
    setLocalFiles([]); setLocalFilesBusy(true); setLocalFilesError("");
    listLocalComposerFiles(project.id).then((files) => { if (active) setLocalFiles(files); })
      .catch(() => { if (active) setLocalFilesError(locale === "zh-CN" ? "本地目录加载失败，请刷新重试。" : "Could not load local files. Refresh to retry."); })
      .finally(() => { if (active) setLocalFilesBusy(false); });
    return () => { active = false; };
  }, [sidebarOpen, tab, fileSource, project.id, fileRefreshVersion, locale]);

  const attachments = useComposerAttachments(project.id, activeConversationId);
  const attachmentsBlocked = attachments.busy || attachments.items.some((item) => item.status !== "ready");
  const [referenceTrigger, setReferenceTrigger] = useState<ReturnType<typeof parseComposerTrigger>>(null);
  const [fileTextPreview, setFileTextPreview] = useState<WorkspaceFileReference | null>(null);
  const [workflowLibraryOpen, setWorkflowLibraryOpen] = useState(false);
  const [projectTemplatePickerOpen, setProjectTemplatePickerOpen] = useState(false);
  const [guidanceDialogOpen, setGuidanceDialogOpen] = useState(false);
  const guidanceDraftAtOpenRef = useRef<string | null>(null);
  const guidancePendingRequestsRef = useRef(new Map<string, { current: SubmitGuidanceV4Request | null }>());
  const [reviewDialogOpen, setReviewDialogOpen] = useState(false);
  const [reviewerSettingsOpen, setReviewerSettingsOpen] = useState(false);
  const [trajectoryOpen, setTrajectoryOpen] = useState(false);
  const [localWorkflowCatalogVersion, setLocalWorkflowCatalogVersion] = useState(0);
  const [catalog, setCatalog] = useState<ComposerCatalogItem[]>([]);
  const [catalogLoading, setCatalogLoading] = useState(false);
  const [catalogError, setCatalogError] = useState("");
  const [composerCommandNotice, setComposerCommandNotice] = useState("");
  const referencesOpen = referenceTrigger !== null;
  useEffect(() => {
    if (!referencesOpen || referenceCatalog) return;
    let current = true;
    setCatalog([]);
    setCatalogLoading(true);
    setCatalogError("");
    composerReferenceCatalog(project.id).then((items) => {
      if (current) setCatalog(items);
    }).catch(() => {
      if (current) setCatalogError(locale === "zh-CN" ? "引用加载失败，请重新打开重试。" : "Could not load references. Reopen to retry.");
    }).finally(() => { if (current) setCatalogLoading(false); });
    return () => { current = false; };
  }, [referencesOpen, project.id, referenceCatalog, locale, workflowCatalogVersion, localWorkflowCatalogVersion]);
  function selectReference(item: ComposerCatalogItem) {
    if (!referenceTrigger) return false;
    if (selectedReferences.length >= 12 && !selectedReferences.some((entry) => referenceKey(entry.reference) === referenceKey(item.reference))) {
      setCatalogError(locale === "zh-CN" ? "每条消息最多引用 12 项。请先移除一项。" : "A message can reference up to 12 items. Remove one first.");
      return false;
    }
    setSelectedReferences((items) => items.some((entry) => referenceKey(entry.reference) === referenceKey(item.reference)) || items.length >= 12 ? items : [...items, item]);
    const { start, end } = referenceTrigger;
    setDraft((text) => text.slice(0, start) + text.slice(end));
    setReferenceTrigger(null);
    setCatalogError("");
    requestAnimationFrame(() => { draftRef.current?.focus(); draftRef.current?.setSelectionRange(start, start); });
  }
  const manualDraftHeight = useRef(false);
  const resizeStartHeight = useRef<number | null>(null);
  const [sendError, setSendError] = useState(false);
  const [sharing, setSharing] = useState(false);
  const [contextUsageOpen, setContextUsageOpen] = useState(false);
  useEffect(() => setContextUsageOpen(false), [project.id, activeConversationId]);
  const [modifierSend, setModifierSend] = useComposerSendPreference();
  useLayoutEffect(() => {
    const input = draftRef.current;
    if (!input) return;
    if (!draft) manualDraftHeight.current = false;
    if (manualDraftHeight.current) return;
    input.style.height = "0px";
    input.style.height = `${input.scrollHeight}px`;
  }, [draft]);
  const [localMode, setLocalMode] = useState<SessionAgentModeV4>("agent");
  const [planSessionActive, setPlanSessionActive] = useState(false);
  const [composerMenuOpen, setComposerMenuOpen] = useState(false);
  const [computeMenuOpen, setComputeMenuOpen] = useState(false);
  const [permissionMenuOpen, setPermissionMenuOpen] = useState(false);
  const [runtimeLanguage, setRuntimeLanguage] = useState<KernelLanguage | null>(null);
  const [sendMenuOpen, setSendMenuOpen] = useState(false);
  const sendOptionsRef = useRef<HTMLButtonElement>(null);
  const [modelMenuOpen, setModelMenuOpen] = useState(false);
  useWindowEscapeLayer(expanded, () => setExpanded(false));
  useWindowEscapeLayer(composerMenuOpen, () => setComposerMenuOpen(false));
  useWindowEscapeLayer(permissionMenuOpen, () => setPermissionMenuOpen(false));
  useWindowEscapeLayer(computeMenuOpen, () => setComputeMenuOpen(false));
  useWindowEscapeLayer(sendMenuOpen, () => setSendMenuOpen(false));
  useWindowEscapeLayer(modelMenuOpen, () => setModelMenuOpen(false));
  useWindowEscapeLayer(runtimeLanguage !== null, () => setRuntimeLanguage(null));
  useWindowEscapeLayer(trajectoryOpen, () => { setTrajectoryOpen(false); draftRef.current?.focus(); });
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
  const imageFiles = [...new Map([
    ...(remoteFiles ?? []).filter((entry) => !entry.directory && isSidebarPreviewImage(entry.relative_path)),
    ...projectArtifacts.filter((artifact) => /^image\//.test(artifact.media_type) && isSidebarPreviewImage(artifact.relative_path)).map((artifact) => ({ relative_path: artifact.relative_path, directory: false, size_bytes: artifact.size_bytes, modified_unix_seconds: 0 })),
  ].map((entry) => [entry.relative_path, entry])).values()];
  const preview = <ArtifactPreview title={t.artifactPreview} locale={locale} images={imageFiles} selectedPath={selectedImagePath} preview={imagePreview} busy={previewBusy} error={previewError} onSelect={setSelectedImagePath} onLoad={loadImagePreview} />;
  const effectiveActiveRunId = activeRunId ?? (runStarted ? agentRunEventsV4.at(-1)?.run_id ?? null : null);
  const activeRunEventsV4 = effectiveActiveRunId
    ? agentRunEventsV4
      .filter((event) => event.run_id === effectiveActiveRunId)
      .sort((left, right) => left.sequence - right.sequence)
    : [];
  const currentScopeRunEventsV4 = activeConversationId
    ? agentRunEventsV4.filter((event) => event.project_id === project.id && event.conversation_id === activeConversationId)
    : [];
  const trajectoryRuns = groupAgentRunEventsV4(currentScopeRunEventsV4);
  const runFinished = Boolean(effectiveTerminalAgentEventV4(activeRunEventsV4));
  const runPaused = getV4PauseReason(activeRunEventsV4) !== null;
  const runActive = runStarted && !runFinished && !runPaused;
  const stopAvailable = Boolean(onCancelRun && (runActive || runStopping));
  const showStopButton = stopAvailable && !(onQueue && (draft.trim() || attachments.receipts.length));
  const stopLabel = runStopping ? (zh ? "终止中…" : "Stopping…") : onQueue ? (zh ? "停止当前运行" : "Stop current run") : (zh ? "终止运行" : "Stop run");
  const [watchdogNow, setWatchdogNow] = useState(() => Date.now());
  const pendingActiveModel = pendingModelRequest(activeRunEventsV4);
  const currentModelActivity = agentModelActivity?.run_id === effectiveActiveRunId
    && agentModelActivity.attempt_id === pendingActiveModel?.attemptId ? agentModelActivity : null;
  const persistedActivityMs = activeRunLastActivityAt ? Date.parse(activeRunLastActivityAt) : Number.NaN;
  const transientActivityMs = currentModelActivity ? Date.parse(currentModelActivity.received_at) : Number.NaN;
  const lastActivityMs = Number.isFinite(transientActivityMs) ? Math.max(persistedActivityMs || 0, transientActivityMs) : persistedActivityMs;
  const runStalled = runActive && Number.isFinite(lastActivityMs) && watchdogNow - lastActivityMs > AGENT_STALL_THRESHOLD_MS;
  // A paused run still owns the conversation sequence. Keep the composer
  // locked while approval/input cards remain usable inside the run trace.
  const existingComposerDisabled = fileReferenceBusy || composerBusy || conversationHydrating || sendBusy || agentBusy || conversationLocked || (runStarted && !runFinished);
  const agentPreferences = useConversationAgentPreferences(project.id, activeConversationId ?? null, existingComposerDisabled);
  const sessionReviews = useSessionReviews(project.id, activeConversationId ?? null, activeModelProfile);
  const reviewBusy = sessionReviews.busy;
  const fastMode = agentPreferences.preferences.fast_mode ?? null;
  const profileFastMode = activeModelProfile?.fast_mode ?? null;
  const effectiveFastMode = fastMode ?? profileFastMode;
  const fastModeAvailable = activeModelProfile ? supportsFastMode(activeModelProfile) : false;
  // Keep a stale enabled override visible after switching models so it can be
  // turned off. The host remains the authority and will reject unsupported
  // enabled requests when a new run is started.
  const showFastMode = fastModeAvailable || fastMode === true || profileFastMode === true;
  const fastModeLocked = existingComposerDisabled || agentPreferences.busy || Boolean(agentPreferences.loadError) || !activeConversationId;
  const fastModeButtonDisabled = fastModeLocked || (!fastModeAvailable && fastMode !== true && profileFastMode !== true);
  const fastModeOption = fastMode === true ? "fast" : fastMode === false ? "standard" : "default";
  const queueBacklog = queueItems.some((item) => item.project_id === project.id && item.conversation_id === activeConversationId && !["completed", "failed", "cancelled"].includes(item.status));
  const queuedSend = Boolean(onQueue && (agentBusy || conversationLocked || (runStarted && !runFinished) || reviewBusy || planLoading || queueBacklog));
  const composerDisabled = onQueue ? fileReferenceBusy || composerBusy || conversationHydrating || sendBusy || queueLoading : existingComposerDisabled;
  const preferenceError = agentPreferences.loadError
    ? (zh ? "会话偏好加载失败，请重试。" : agentPreferences.loadError)
    : agentPreferences.saveError
      ? (zh ? "会话偏好保存失败，请重试。" : agentPreferences.saveError)
      : "";
  const sendLabel = fileReferenceBusy ? (zh ? "校验路径中…" : "Checking paths…") : queuedSend ? (zh ? "加入队列" : "Add to queue") : reviewBusy ? (zh ? "审核中…" : "Reviewing…") : composerDisabled || planLoading ? (planModeEnabled ? (zh ? "规划中…" : "Planning…") : (zh ? "执行中…" : "Running…")) : queuedSend ? (zh ? "加入队列" : "Add to queue") : t.send;
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
  function openProjectFiles() { openSidebarSection("files"); setFileRefreshVersion((value) => value + 1); }
  function appendFileReferences(items: ComposerCatalogItem[]) {
    const allowed = items.every((item) => item.reference.kind === "workspace_file" && item.reference.project_id === project.id &&
      (item.reference.backend_id === "local" || item.reference.backend_id === `ssh:${project.connection_id}`));
    if (!allowed) { setReferenceNotice(zh ? "只能附加当前项目的文件引用。" : "Only files from this project can be attached."); return; }
    setSelectedReferences((current) => {
      const added = items.filter((item, index) => !current.some((entry) => referenceKey(entry.reference) === referenceKey(item.reference)) && items.findIndex((entry) => referenceKey(entry.reference) === referenceKey(item.reference)) === index);
      if (current.length + added.length > 12) { setReferenceNotice(zh ? "每条消息最多引用 12 项。" : "A message can reference up to 12 items."); return current; }
      return [...current, ...added];
    });
    setReferenceTrigger(null);
    draftRef.current?.focus();
  }
  function attachWorkspaceFile(reference: WorkspaceFileReference) {
    if (composerDisabled) return;
    appendFileReferences([{ reference, label: reference.relative_path, description: reference.backend_id === "local" ? (zh ? "本地文件引用；未上传" : "Local file reference; not uploaded") : (zh ? "SSH 文件引用；未下载" : "SSH file reference; not downloaded") }]);
  }
  async function attachClipboardPaths(paths: string[]) {
    if (composerDisabled || fileReferenceOperation.current) return;
    const operation = {};
    fileReferenceOperation.current = operation; setFileReferenceBusy(true); setReferenceNotice("");
    try {
      const items = await resolveComposerClipboardPaths(project.id, paths);
      if (fileReferenceOperation.current !== operation) return;
      appendFileReferences(items);
    } catch {
      if (fileReferenceOperation.current === operation) setReferenceNotice(zh ? "路径无法附加。请确认文件存在于当前项目内。" : "Could not attach paths. Check that the files exist inside this project.");
    } finally {
      if (fileReferenceOperation.current === operation) { fileReferenceOperation.current = null; setFileReferenceBusy(false); }
    }
  }
  const fileBackendId = fileSource === "local" ? "local" : project.connection_id ? `ssh:${project.connection_id}` : null;
  const currentFileReference = (relativePath: string): WorkspaceFileReference => ({ kind: "workspace_file", project_id: project.id, backend_id: fileBackendId!, relative_path: relativePath });

  function closeComposerMenus() {
    setReferenceTrigger(null);
    setComposerMenuOpen(false); setPermissionMenuOpen(false); setComputeMenuOpen(false); setModelMenuOpen(false); setSendMenuOpen(false);
  }
  function insertQuickActionWorkflow(workflow: ComposerWorkflowTemplate) {
    if (composerDisabled || workflow.project_id !== project.id || !workflow.enabled) return;
    const item: ComposerCatalogItem = {
      reference: { kind: "workflow", project_id: project.id, id: workflow.id },
      label: workflow.name,
      description: workflow.description,
    };
    if (selectedReferences.some((entry) => referenceKey(entry.reference) === referenceKey(item.reference))) {
      requestAnimationFrame(() => draftRef.current?.focus());
      return;
    }
    if (selectedReferences.length >= 12) {
      setReferenceNotice(zh ? "每条消息最多引用 12 项。请先移除一项。" : "A message can reference up to 12 items. Remove one first.");
      return;
    }
    setSelectedReferences((current) => [...current, item]);
    setReferenceNotice("");
    requestAnimationFrame(() => draftRef.current?.focus());
  }
  function insertSpecialistDraft(template: SpecialistTemplate) {
    if (composerDisabled || template.project_id !== project.id || !template.enabled) return;
    const block = `${zh ? "[专家角色：" : "[Specialist: "}${template.name}]\n${template.instructions}`;
    setDraft((current) => current ? `${current}\n\n${block}` : block);
    setSendError(false);
    requestAnimationFrame(() => draftRef.current?.focus());
  }
  function insertSelectionQuote(selection: MessageSelectionQuote) {
    const source = selection.role === "user"
      ? (zh ? "你的消息" : "You")
      : (zh ? "Agent 回复" : "Agent response");
    const quotedText = selection.text.split("\n").map((line) => `> ${line}`).join("\n");
    const block = `${zh ? "[引用自" : "[Quoted from"} ${source}]\n${quotedText}`;
    setDraft((current) => current ? `${current}\n\n${block}` : block);
    setSendError(false);
    requestAnimationFrame(() => draftRef.current?.focus());
  }
  function guidancePendingRequestForScope(runId: string) {
    const key = `${project.id}:${activeConversationId}:${runId}`;
    let pending = guidancePendingRequestsRef.current.get(key);
    if (!pending) {
      for (const [scope, value] of guidancePendingRequestsRef.current) {
        if (value.current === null) guidancePendingRequestsRef.current.delete(scope);
      }
      pending = { current: null };
      guidancePendingRequestsRef.current.set(key, pending);
    }
    return pending;
  }
  function openGuidanceDialog() {
    if (!guidanceDialogAvailable) return;
    const hasComposerContext = attachments.items.length > 0 || attachments.receipts.length > 0 || selectedReferences.length > 0;
    guidanceDraftAtOpenRef.current = !hasComposerContext && draft ? draft : null;
    // The menu item is removed as the dialog mounts; focus the stable launch
    // control first so the dialog can restore to a connected element.
    sendOptionsRef.current?.focus();
    closeComposerMenus();
    setGuidanceDialogOpen(true);
  }
  function closeGuidanceDialog() {
    setGuidanceDialogOpen(false);
    guidanceDraftAtOpenRef.current = null;
  }
  function handleGuidanceAccepted(markdown: string) {
    const openedDraft = guidanceDraftAtOpenRef.current;
    if (openedDraft === null || openedDraft.trim() !== markdown) return;
    guidanceDraftAtOpenRef.current = null;
    if (attachments.items.length > 0 || attachments.receipts.length > 0 || selectedReferences.length > 0) return;
    setDraft((current) => current === openedDraft ? "" : current);
  }
  // Older hosts may return the ordinary run's internal contract. Only expose an actual Plan.
  const hasApprovalPlan = Boolean(v4Plan?.plan && (
    v4Plan.session_mode !== "agent" || v4Plan.plan_revision != null
    || latestPlanRevision?.run_id === v4Plan.run_id
    || activeRunEventsV4.some((event) => event.event.kind === "plan_proposed")
  ));
  const visiblePlan = hasApprovalPlan ? v4Plan : null;
  const planReady = hasApprovalPlan && !runStarted && (
    latestPlanRevision ? latestPlanRevision.run_id === v4Plan?.run_id && latestPlanRevision.status === "pending"
      : v4Plan?.status === "awaiting_approval"
  );
  const showPlanPanel = planModeEnabled || planLoading || (!controlledMode && (planSessionActive || hasApprovalPlan));
  const executeStart = activeRunEventsV4.findIndex((event) => event.event.kind === "mode_changed" && event.event.mode === "execute");
  const visibleActiveRunEventsV4 = hasApprovalPlan && executeStart >= 0 ? activeRunEventsV4.slice(executeStart) : hasApprovalPlan ? [] : activeRunEventsV4;
  const guidanceRunId = effectiveActiveRunId ?? (guidanceAvailable && !planModeEnabled && !hasApprovalPlan ? v4Plan?.run_id ?? null : null);
  // History remains readable after a run pauses or finishes, while new
  // guidance is accepted only by an ordinary Agent run that is still active.
  const guidanceDialogAvailable = Boolean(
      guidanceAvailable
        && activeConversationId
        && guidanceRunId
      && !planModeEnabled
      && !hasApprovalPlan,
  );
  const guidanceEnabled = guidanceDialogAvailable && runStarted && !runFinished && !runPaused && !runStopping;

  useEffect(() => { if (!controlledMode) setLocalMode("agent"); setPlanSessionActive(false); }, [activeConversationId, controlledMode]);
  useEffect(() => {
    sendGenerationRef.current += 1;
    sendBusyRef.current = false;
    setSendBusy(false);
    setSendError(false);
    setSharing(false);
    setWorkflowLibraryOpen(false);
    setProjectTemplatePickerOpen(false);
    setGuidanceDialogOpen(false);
    guidanceDraftAtOpenRef.current = null;
    setReviewDialogOpen(false);
    setReviewerSettingsOpen(false);
    setTrajectoryOpen(false);
    setFileTextPreview(null);
    setReferenceTrigger(null);
    setSelectedReferences([]);
    setReferenceNotice("");
    setComposerCommandNotice("");
    followingLatestRef.current = true;
    setFollowingLatest(true);
  }, [project.id, activeConversationId]);
  useEffect(() => {
    if (!searchRequest || handledSearchRequest.current === searchRequest.key || searchRequest.projectId !== project.id) return;
    handledSearchRequest.current = searchRequest.key;
    onSearchRequestHandled?.(searchRequest.key);
    setSharing(false);
    setGuidanceDialogOpen(false);
    guidanceDraftAtOpenRef.current = null;
    setWorkflowLibraryOpen(false);
    setProjectTemplatePickerOpen(false);
    setFileTextPreview(null);
    setRuntimeLanguage(null);
    setExpanded(false);
    closeComposerMenus();
    if (searchRequest.kind === "reveal") { setWorkspacePage("conversation"); return; }
    if (searchRequest.kind === "files") { openProjectFiles(); return; }
    if (searchRequest.kind === "artifact") {
      setOpenedSearchArtifact(searchRequest.item);
      openSidebarSection("artifacts");
      return;
    }
    if (searchRequest.conversationId !== activeConversationId) return;
    if (composerDisabled) {
      setReferenceNotice(zh ? "当前消息暂不可编辑，请稍后重新附加引用。" : "The message is currently locked. Attach the reference again when it is editable.");
      return;
    }
    const item = searchRequest.item;
    const reference = item.reference;
    if (reference.kind !== "skill" && (reference.project_id !== project.id || (reference.kind === "session" && reference.id === activeConversationId))) return;
    if (selectedReferences.some((entry) => referenceKey(entry.reference) === referenceKey(reference))) { draftRef.current?.focus(); return; }
    if (selectedReferences.length >= 12) {
      setReferenceNotice(zh ? "每条消息最多引用 12 项。请先移除一项。" : "A message can reference up to 12 items. Remove one first.");
      return;
    }
    setSelectedReferences((items) => [...items, item]);
    setReferenceNotice("");
    draftRef.current?.focus();
  }, [searchRequest, project.id, activeConversationId, composerDisabled, selectedReferences, zh, onSearchRequestHandled]);
  useEffect(() => { if (showPlanPanel) openSidebarSection("plan", false); }, [showPlanPanel]);
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
  }, [activeConversationId, messages.length, agentTextPreview?.text, agentReasoningPreview?.text, streamingAssistant, agentBusy, agentNotice, planLoading, runStarted, latestAgentRunEventV4?.run_id, latestAgentRunEventV4?.sequence]);

  useEffect(() => {
    if (selectedImagePath && imageFiles.some((entry) => entry.relative_path === selectedImagePath)) return;
    setSelectedImagePath(imageFiles[0]?.relative_path ?? "");
    setImagePreview(null);
    setPreviewError("");
  }, [remoteFiles, projectArtifacts, selectedImagePath]);

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

  const replacementDraft = useRef<{ generation: number; draft: string; references: typeof selectedReferences; attachments: string[] } | null>(null);
  const replacementHead = activeRunEventsV4.at(-1);
  const replacementDisabled = !replacement || replacement.busy || replacement.pending || composerDisabled || attachmentsBlocked || !computeReady || agentPreferences.busy || Boolean(agentPreferences.loadError) || runStopping || runFinished || !replacementHead || !activeRunId || (!draft.trim() && !attachments.receipts.length);
  function clearReplacementDraft() {
    const submitted = replacementDraft.current;
    if (submitted && submitted.generation === sendGenerationRef.current) {
      setDraft((current) => current === submitted.draft ? "" : current);
      setSelectedReferences((current) => current === submitted.references ? [] : current);
      attachments.clearAccepted(submitted.attachments);
    }
    replacementDraft.current = null;
  }
  async function replaceCurrentTurn() {
    if (!replacement || replacementDisabled || !replacementHead || !activeRunId || fileReferenceOperation.current) return;
    const submitted = { generation: sendGenerationRef.current, draft, references: selectedReferences, attachments: attachments.receipts.map((receipt) => receipt.id) };
    replacementDraft.current = submitted;
    setSendMenuOpen(false);
    const accepted = await replacement.send(draft.trim() || (zh ? "请查看附件。" : "Please inspect the attached files."), planModeEnabled ? "plan" : "chat", { run_id: activeRunId, sequence: replacementHead.sequence, event_hash: replacementHead.event_hash }, selectedReferences.map((item) => item.reference), submitted.attachments);
    if (accepted && replacementDraft.current === submitted) clearReplacementDraft();
  }
  async function retryReplacement() {
    if (!replacement) return;
    const submitted = replacementDraft.current;
    if (await replacement.retry()) {
      if (replacementDraft.current === submitted) clearReplacementDraft();
    }
  }
  async function send() {
    const submittedDraft = draft;
    const message = submittedDraft.trim() || (attachments.receipts.length ? (zh ? "请查看附件。" : "Please inspect the attached files.") : "");
    if (typedComposerCommand) { typedComposerCommand.onSelect(); return; }
    if (!message || fileReferenceOperation.current || attachmentsBlocked || composerDisabled || (!onQueue && reviewBusy) || agentPreferences.busy || Boolean(agentPreferences.loadError) || (!onQueue && planLoading) || ((onSend || onQueue) && !computeReady) || sendBusyRef.current) return;
    const sendGeneration = sendGenerationRef.current;
    sendBusyRef.current = true;
    setSendBusy(true);
    setSendError(false);
    try {
      const submit = onQueue ?? onSend;
      if (submit) {
        const mode = planModeEnabled ? "plan" : "chat";
        setPlanSessionActive(mode === "plan");
        const attachmentIds = attachments.receipts.map((receipt) => receipt.id);
        const accepted = attachmentIds.length
          ? await submit(message, mode, selectedReferences.map((item) => item.reference), attachmentIds)
          : selectedReferences.length ? await submit(message, mode, selectedReferences.map((item) => item.reference)) : await submit(message, mode);
        if (sendGeneration === sendGenerationRef.current) {
          if (accepted === false) setSendError(true);
          else {
            setDraft((current) => current === submittedDraft ? "" : current);
            setSelectedReferences((current) => current.filter((item) => !selectedReferences.some((sent) => referenceKey(sent.reference) === referenceKey(item.reference))));
            setReferenceTrigger(null);
            attachments.clearAccepted(attachmentIds);
          }
        }
      } else {
        setSentMessages((current) => [...current, message]);
        setDraft("");
        setSelectedReferences([]);
        setReferenceTrigger(null);
      }
    } catch {
      if (sendGeneration === sendGenerationRef.current) setSendError(true);
    } finally {
      if (sendGeneration === sendGenerationRef.current) {
        sendBusyRef.current = false;
        setSendBusy(false);
      }
    }
  }

  const sideComposerAttempts = useRef(new Map<string, { draft: string; references: ComposerCatalogItem[]; ids: string[]; generation: number }>());
  const sideScope = `${project.id}:${activeConversationId ?? ""}`;
  function clearAcceptedSideDraft(scope: string) {
    const attempt = sideComposerAttempts.current.get(scope);
    sideComposerAttempts.current.delete(scope);
    if (!attempt || attempt.generation !== sendGenerationRef.current) return;
    setDraft((current) => current === attempt.draft ? "" : current);
    // Only clear the submitted selection instance; edits can re-add the same reference.
    setSelectedReferences((current) => current === attempt.references ? [] : current);
    attachments.clearAccepted(attempt.ids);
  }
  async function sendSideQuestion(question?: string, originalDraft = draft, fromCommand = false): Promise<boolean> {
    if (!sideChat || !activeConversationId) {
      if (fromCommand) setComposerCommandNotice(zh ? "独立旁聊需要桌面宿主。" : "Side chat requires the desktop host.");
      return false;
    }
    closeComposerMenus(); openSidebarSection("side-chat");
    const hasMaterial = attachments.receipts.length > 0 || selectedReferences.length > 0;
    const requestedQuestion = question === undefined ? draft.trim() : question.trim();
    if (!requestedQuestion && !hasMaterial) return true;
    if (!sideChat.ready || !sideChat.modelId || sideChat.busy || sideChat.pending || attachmentsBlocked || fileReferenceOperation.current) {
      if (fromCommand) setComposerCommandNotice(zh ? "独立旁聊当前不可用，请稍后重试。" : "Side chat is unavailable right now. Retry in a moment.");
      return false;
    }
    const attempt = { draft: originalDraft, references: selectedReferences, ids: attachments.receipts.map((receipt) => receipt.id), generation: sendGenerationRef.current };
    sideComposerAttempts.current.set(sideScope, attempt);
    try {
      const accepted = await sideChat.send({ question_markdown: requestedQuestion || (zh ? "请解释所选资料。" : "Please explain the selected material."), references: attempt.references.map((item) => item.reference), attachments: attempt.ids });
      if (accepted) clearAcceptedSideDraft(sideScope);
      return accepted;
    } catch {
      if (fromCommand) setComposerCommandNotice(zh ? "独立旁聊状态未能确认，原始命令已保留。请重试。" : "Side chat status could not be confirmed; the original command is preserved. Retry.");
      return false;
    }
  }
  async function retrySideQuestion() {
    if (!sideChat) return false;
    const accepted = await sideChat.retry();
    if (accepted) clearAcceptedSideDraft(sideScope);
    return accepted;
  }
  function revealSideChatSource(messageId: string) {
    if (!messages.some((message) => message.id === messageId)) return;
    const node = [...(messageStreamRef.current?.querySelectorAll<HTMLElement>("[data-message-id]") ?? [])].find((element) => element.dataset.messageId === messageId);
    if (node) { followingLatestRef.current = false; setFollowingLatest(false); node.scrollIntoView({ block: "center" }); node.focus(); }
  }

  function prepareSkill() {
    const request = zh
      ? "请根据当前会话中已验证的方法和证据，整理可复用的技能：生成 SKILL.md，说明适用条件、输入输出、环境依赖、执行步骤和验证方法。区分已执行结果与建议；保存前检查内容和路径，并遵守当前审批策略。"
      : "Prepare a reusable skill from the verified methods and evidence in this conversation. Generate SKILL.md with applicability, inputs, outputs, environment dependencies, execution steps and validation. Distinguish executed results from suggestions; check the content and path before saving under the current approval policy.";
    setDraft((current) => [current, request].filter(Boolean).join("\n\n"));
    draftRef.current?.focus();
  }

  function runComposerCommand(action: () => void, options: { preserveDraft?: boolean; allowLocked?: boolean } = {}): boolean {
    if (!options.allowLocked && composerDisabled) return false;
    setComposerCommandNotice("");
    const trigger = referenceTrigger ?? parseComposerTrigger(draft, draft.length);
    if (!options.preserveDraft) {
      if (trigger?.kind === "skill") setDraft((text) => text.slice(0, trigger.start) + text.slice(trigger.end));
      else if (/^\/[a-z-]+$/i.test(draft.trim())) setDraft("");
    }
    setReferenceTrigger(null);
    closeComposerMenus();
    action();
    return true;
  }
  function openTrajectory(): boolean {
    if (!activeConversationId) {
      setComposerCommandNotice(zh ? "当前没有可查看运行轨迹的会话。" : "There is no conversation to show a run trajectory for.");
      return false;
    }
    closeComposerMenus();
    setTrajectoryOpen(true);
    return true;
  }
  function runBtwCommand(invocation: ComposerCommandInvocation): boolean {
    const originalDraft = draft;
    setComposerCommandNotice("");
    if (!invocation.args.trim()) {
      if (!sideChat || !activeConversationId) {
        setComposerCommandNotice(zh ? "独立旁聊需要桌面宿主。" : "Side chat requires the desktop host.");
        return false;
      }
      closeComposerMenus();
      openSidebarSection("side-chat");
      return true;
    }
    void sendSideQuestion(invocation.args, originalDraft, true);
    return true;
  }
  function runForkCommand(invocation: ComposerCommandInvocation): boolean {
    const originalDraft = draft;
    const request = invocation.args.trim();
    setComposerCommandNotice("");
    if (!request) {
      setComposerCommandNotice(zh ? "用法：/fork <请求>" : "Usage: /fork <request>");
      closeComposerMenus();
      return false;
    }
    if (!onBranchSend) {
      setComposerCommandNotice(zh ? "分支发送需要桌面宿主。" : "Branch sending requires the desktop host.");
      closeComposerMenus();
      return false;
    }
    if (branchDisabled || !lastBranchAnchor) {
      setComposerCommandNotice(!onOpenBranch ? (zh ? "当前宿主不支持创建分支。" : "Branching is unavailable in this host.") : (zh ? "当前会话已锁定，暂时不能创建分支。" : "Branching is locked while this conversation is active."));
      closeComposerMenus();
      return false;
    }
    if (!computeReady) {
      setComposerCommandNotice(zh ? "请选择可用的计算配置后再创建分支。" : "Choose an available compute configuration before branching.");
      closeComposerMenus();
      return false;
    }
    const generation = sendGenerationRef.current;
    closeComposerMenus();
    void branchAt(lastBranchAnchor.id, "after_response", request).then((accepted) => {
      if (accepted && generation === sendGenerationRef.current) setDraft((current) => current === originalDraft ? "" : current);
    });
    return true;
  }
  const composerCommands: ComposerPickerCommand[] = [
    { id: "btw", label: "/btw", description: zh ? "不中断主任务，询问独立旁聊" : "Ask side chat without interrupting the main run", onSelect: () => runBtwCommand(parseComposerCommand(draft) ?? { id: "btw", args: "", raw: draft }) },
    { id: "fork", label: "/fork", description: zh ? "创建分支并发送请求" : "Create a branch and send a request", onSelect: () => runForkCommand(parseComposerCommand(draft) ?? { id: "fork", args: "", raw: draft }) },
    { id: "trajectory", label: "/trajectory", description: zh ? "查看当前会话的运行轨迹" : "View the current conversation's run trajectory", onSelect: () => activeConversationId ? runComposerCommand(openTrajectory, { allowLocked: true }) : openTrajectory() },
    { id: "context", label: "/context", description: zh ? "查看上下文用量" : "Inspect context usage", onSelect: () => runComposerCommand(() => setContextUsageOpen(true)) },
    { id: "workflows", label: "/workflows", description: zh ? "管理项目工作流" : "Manage project workflows", onSelect: () => runComposerCommand(() => setWorkflowLibraryOpen(true)) },
    ...(!modeLocked ? [{ id: "plan", label: "/plan", description: zh ? "切换先做计划模式" : "Toggle plan-first mode", onSelect: () => runComposerCommand(() => chooseMode(planModeEnabled ? "agent" : "plan")) }] : []),
    { id: "permission", label: "/permission", description: zh ? "打开权限选项" : "Open permission options", onSelect: () => runComposerCommand(() => setPermissionMenuOpen(true)) },
    ...(activeConversationId ? [{ id: "review", label: "/review", description: zh ? "回看并审核当前会话" : "Review the current conversation", onSelect: () => runComposerCommand(() => setReviewDialogOpen(true)) }] : []),
    { id: "files", label: "/files", description: zh ? "浏览项目文件" : "Browse project files", onSelect: () => runComposerCommand(openProjectFiles) },
    { id: "save-as-skill", label: "/save-as-skill", description: zh ? "准备可复用技能草稿" : "Prepare a reusable skill draft", onSelect: () => runComposerCommand(prepareSkill) },
    ...(onOpenSettings ? [{ id: "skills", label: "/skills", description: zh ? "管理技能" : "Manage skills", onSelect: () => runComposerCommand(() => onOpenSettings("skills")) }] : []),
    ...(activeConversationId ? [{ id: "upload", label: "/upload", description: zh ? "添加本地附件" : "Attach local files", onSelect: () => runComposerCommand(() => { void attachments.chooseFiles(); }) }] : []),
    ...(messages.length ? [{ id: "share", label: "/share", description: zh ? "预览并导出会话" : "Preview and export conversation", onSelect: () => runComposerCommand(() => setSharing(true)) }] : []),
  ];
  const composerCommandInvocation = parseComposerCommand(draft);
  const typedComposerCommand = composerCommandInvocation && composerCommands.find((command) => command.id === composerCommandInvocation.id && (command.id === "btw" || command.id === "fork" || !composerCommandInvocation.args.trim()));

  function toggleModifierSend() {
    setModifierSend(!modifierSend);
  }

  function toggleAgentPreference(key: ConversationAgentPreferenceKey) {
    if (existingComposerDisabled || agentPreferences.busy || agentPreferences.loadError || !activeConversationId) return;
    agentPreferences.toggle(key);
  }

  function chooseFastMode(option: string) {
    if (option === "fast" && !fastModeAvailable && fastMode !== true) return;
    agentPreferences.setFastMode(option === "fast" ? true : option === "standard" ? false : null);
  }

  function toggleFastMode() {
    const next = effectiveFastMode === true
      ? (fastMode === null && profileFastMode === true ? false : null)
      : true;
    agentPreferences.setFastMode(next);
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

  return <div className={`science-shell ${sidebarOpen ? "sidebar-open" : "sidebar-collapsed"} ${navigationCollapsed ? "navigation-collapsed" : ""} ${workspacePage !== "conversation" ? "research-page-open" : ""}`}>
    <WorkspaceNavigation key={`navigation:${project.id}`} projectId={project.id} projectName={project.name} locale={locale} conversations={conversations} activeConversationId={activeConversationId} collapsed={navigationCollapsed} onToggleCollapsed={() => setNavigationCollapsed((value) => { try { localStorage.setItem("omicsops.workspaceNavigationCollapsed", String(!value)); } catch { /* Keep usable in restricted storage. */ } return !value; })} page={workspacePage === "conversation" && sidebarOpen && tab === "files" ? "files" : workspacePage} onNavigate={navigateWorkspace} onNewConversation={onNewConversation ? () => leaveGuard.current(() => { setWorkspacePage("conversation"); void onNewConversation(); }) : undefined} newConversationDisabled={conversationHydrating || agentBusy || conversationLocked} onSelectConversation={(id) => leaveGuard.current(() => { setWorkspacePage("conversation"); void onSelectConversation?.(id); })} onDeleteConversation={onDeleteConversation ? (item) => leaveGuard.current(() => { void deleteConversation(item); }) : undefined} deleteDisabled={(item) => deletingConversationId !== null || conversationHydrating || conversationLocked || (item.id === activeConversationId && (agentBusy || runActive))} onOpenSearch={onOpenSearch} onBack={() => leaveGuard.current(() => onBackToProjects?.())}>
      <ConversationCapabilities projectId={project.id} conversationId={activeConversationId} summary={capabilitySummary} loading={capabilitiesLoading} error={capabilitiesError} onRefresh={onRefreshCapabilities} locale={locale} onLocaleChange={onLocaleChange} onOpenSettings={onOpenSettings} />
    </WorkspaceNavigation>
    {workspacePage !== "conversation" && workspacePage !== "files" && <WorkspaceResearchPages key={`research:${project.id}`} page={workspacePage} projectId={project.id} locale={locale} registerBeforeLeave={registerBeforeLeave} onOpenConversation={(projectId, conversationId) => leaveGuard.current(() => { setWorkspacePage("conversation"); if (onOpenSourceConversation) void onOpenSourceConversation(projectId, conversationId); else if (projectId === project.id) void onSelectConversation?.(conversationId); })} onInsert={(text) => leaveGuard.current(() => { setDraft((current) => current ? `${current}\n\n${text}` : text); setWorkspacePage("conversation"); requestAnimationFrame(() => draftRef.current?.focus()); })} />}

    <main className={`conversation-pane ${workspacePage !== "conversation" ? "workspace-content-hidden" : ""}`} hidden={workspacePage !== "conversation"} aria-label={t.research}>
      <header className="conversation-header"><div><small>{project.name}</small><h1>{conversationTitle}</h1></div><div className="workspace-header-actions">{showPlanPanel && <button className="sidebar-plan-link" onClick={() => openSidebarSection("plan")}>{zh ? "查看 Plan" : "Review Plan"}</button>}<span className="live-status"><i />{t.status}</span><button className="sidebar-toggle" aria-label={sidebarOpen ? (zh ? "收起侧栏" : "Collapse sidebar") : (zh ? "展开侧栏" : "Expand sidebar")} aria-expanded={sidebarOpen} aria-controls="workspace-sidebar" onClick={() => { if (sidebarOpen) closeSidebar(); else { if (!openTabs.length) { setOpenTabs(["artifacts"]); setTab("artifacts"); } setSidebarOpen(true); } }}><PanelRight size={19} /></button></div></header>
      <section className="message-stream" aria-live="polite" ref={messageStreamRef} onScroll={(event) => {
        const stream = event.currentTarget;
        const following = stream.scrollHeight - stream.scrollTop - stream.clientHeight < 64;
        followingLatestRef.current = following;
        setFollowingLatest(following);
      }}>
        {!onSend && <article className="message user-message"><MarkdownContent markdown={zh ? "比较两批 PBMC，检查批次效应并生成可复现的分析报告。" : "Compare two PBMC batches, assess batch effects, and generate a reproducible report."} /></article>}
        {messages.length === 0 && <article className="message assistant-message"><div className="assistant-avatar"><Bot size={17} /></div><div><strong>OmicsOps Agent</strong><MarkdownContent markdown={zh ? "描述你的研究目标。我会按需查阅资料、调用工具并核验结果；需要先讨论方案时可切换到 Plan。" : "Describe your research goal. I will inspect relevant evidence, use tools, and verify results. Choose Plan when you want to discuss the approach first."} /></div></article>}
        {sentMessages.map((message, index) => <article className="message user-message" key={`${index}-${message}`}><MarkdownContent markdown={message} /></article>)}
        {onOpenBranch && activeConversationId && <ConversationBranchBanner projectId={project.id} conversationId={activeConversationId} locale={locale} onSelect={onSelectConversation} />}
        {branching.error && <div className="agent-notice" role="alert"><span>{branching.error}</span>{branching.retryAvailable && <button disabled={branching.busy} onClick={() => void branching.retry()}>{zh ? "重试分支" : "Retry branch"}</button>}</div>}
        {messages.map((message, messageIndex) => <Fragment key={message.id}>
          {activeConversationId && (message.role === "user" || message.role === "assistant") && message.markdown.trim() && <CollectSourceButton key={`collect:${project.id}:${activeConversationId}:${message.id}`} source={() => messageLibrarySource(project.id, activeConversationId, message.id, message.markdown)} title={message.markdown.slice(0, 60)} kind="excerpt" zh={zh} />}
          {message.role === "user" ? <article className="message user-message" data-message-id={message.id} tabIndex={-1}><MarkdownContent markdown={message.markdown} selectionScope={{ messageId: message.id, role: "user", projectId: project.id, conversationId: activeConversationId ?? "" }} /></article> : message.role === "assistant" && message.markdown.trim() ? <article className="message assistant-message response-message" aria-label={zh ? "Agent 回复" : "Agent response"} data-message-id={message.id} tabIndex={-1}><ResponseBody markdown={message.markdown} zh={zh} createdAt={message.created_at} selectionScope={{ messageId: message.id, role: "assistant", projectId: project.id, conversationId: activeConversationId ?? "" }} /></article> : null}
          {onOpenBranch && (message.role === "user" || (message.role === "assistant" && message.markdown.trim())) && <div className="message-branch-actions"><button type="button" disabled={branchDisabled || !messages.slice(0, messageIndex + 1).some((entry) => entry.role === "user")} onClick={() => {
            const anchor = messages.slice(0, messageIndex + 1).reverse().find((entry) => entry.role === "user");
            if (anchor) branchAt(anchor.id, message.role === "user" ? "before_user" : "after_response");
          }}>{message.role === "user" ? (zh ? "从此消息前分叉" : "Branch before this message") : (zh ? "从此回复后分叉" : "Branch after this response")}</button></div>}
          {runTimelineV4.afterMessage.get(message.id)?.map((run) => <V4RunTrace locale={locale} events={run.events} onAnswer={onAnswerAgentQuestionV4} onDecideApproval={onDecideToolApprovalV4} onResolveUncertain={onResolveUncertainV4} onResume={onResumeAgentRunV4} onCancelRecovery={onCancelRuntimeRecoveryV4} onCloseBrowserTabs={onCloseBrowserRunTabsV4} historical key={run.runId} />)}
        </Fragment>)}
        {streamingAssistant && <article className="message assistant-message response-streaming"><div className="assistant-avatar"><Bot size={17} /></div><div><strong>OmicsOps Agent · {zh ? "生成中" : "streaming"}</strong><MarkdownContent markdown={streamingAssistant} /></div></article>}
        {agentBusy && !streamingAssistant && <article className="message assistant-message agent-pending" role="status"><div className="assistant-avatar"><Bot size={17} /></div><div><strong>OmicsOps Agent</strong><p>{zh ? "正在等待模型响应…" : "Waiting for the model…"}</p></div></article>}
        {agentRetryNotice && <div className="agent-retry-notice" role="status"><span className="agent-working"><i />{agentRetryNotice}</span></div>}
        {conversationLoadError && <div className="agent-notice" role="alert"><strong>{zh ? "会话恢复失败" : "Session restore failed"}</strong><span>{conversationLoadError}</span><button type="button" aria-label={zh ? "重试会话恢复" : "Retry session restore"} onClick={onRetryConversationLoad}>{zh ? "重试" : "Retry"}</button></div>}
        {agentNotice && <div className="agent-notice" role="alert"><strong>{zh ? "对话未完成" : "Conversation did not complete"}</strong><span>{agentNotice}</span></div>}
        {planReady && <article className="message assistant-message plan-ready-message"><div className="assistant-avatar"><ClipboardList size={17} /></div><div><strong>OmicsOps Agent</strong><p>{zh ? "计划已生成，请在右侧 Plan 面板审核并决定是否运行。" : "The plan is ready. Review it in the Plan panel and decide whether to run it."}</p></div></article>}
        {runTimelineV4.unanchored.map((run) => <V4RunTrace locale={locale} events={run.events} onAnswer={onAnswerAgentQuestionV4} onDecideApproval={onDecideToolApprovalV4} onResolveUncertain={onResolveUncertainV4} onResume={onResumeAgentRunV4} onCancelRecovery={onCancelRuntimeRecoveryV4} onCloseBrowserTabs={onCloseBrowserRunTabsV4} historical key={run.runId} />)}
        {visibleActiveRunEventsV4.length > 0 && <V4RunTrace locale={locale} events={visibleActiveRunEventsV4} previewText={agentTextPreview?.run_id === effectiveActiveRunId ? agentTextPreview.text : null} reasoningPreview={agentReasoningPreview?.run_id === effectiveActiveRunId ? agentReasoningPreview : null} modelActivity={currentModelActivity} onAnswer={onAnswerAgentQuestionV4} onDecideApproval={onDecideToolApprovalV4} onResolveUncertain={onResolveUncertainV4} onResume={onResumeAgentRunV4} onCancelRecovery={onCancelRuntimeRecoveryV4} onCloseBrowserTabs={onCloseBrowserRunTabsV4} />}
        {!composerDisabled && effectiveActiveRunId && onSuggestFollowUps && effectiveTerminalAgentEventV4(activeRunEventsV4)?.event.kind === "run_completed" && <FollowUpQuestions key={`${project.id}:${activeConversationId}:${effectiveActiveRunId}`} runId={effectiveActiveRunId} generate={onSuggestFollowUps} onChoose={(question) => { setDraft(question); }} locale={locale} />}
        {runStarted && agentRunEventsV4.length === 0 && <article className="message assistant-message agent-pending" role="status"><div className="assistant-avatar"><Bot size={17} /></div><div><strong>OmicsOps Agent</strong><p>{zh ? "V4 运行正在启动…" : "Starting the V4 run…"}</p></div></article>}
        {runStalled && <div className="agent-retry-notice" role="status"><span>{zh ? "超过 90 秒未收到模型数据或 Agent 事件，任务可能卡住；仍可终止运行。" : "No model data or Agent event has arrived for 90 seconds; the run may be stuck. You can still stop it."}</span></div>}
        {!onSend && <article className="task-card"><div className="task-icon"><Activity size={18} /></div><div className="task-body"><div><strong>{t.task}</strong><span>65%</span></div><p>{zh ? "远端 Linux · 8 CPU · 32 GiB · 低风险" : "Remote Linux · 8 CPU · 32 GiB · low risk"}</p><div className="task-progress"><i /></div><div className="task-actions"><button>{zh ? "查看日志" : "View logs"}</button><button>{zh ? "查看计划" : "View plan"}</button></div></div></article>}
      {!followingLatest && <button className="back-to-latest" onClick={() => {
        const stream = messageStreamRef.current;
        if (stream) stream.scrollTop = stream.scrollHeight;
        followingLatestRef.current = true;
        setFollowingLatest(true);
      }}>↓ {zh ? "回到最新" : "Back to latest"}</button>}
      </section>
      <MessageSelectionActions enabled={workspacePage === "conversation" && selectionActionsEnabled && Boolean(activeConversationId)} locale={locale} projectId={project.id} conversationId={activeConversationId ?? ""} onQuote={insertSelectionQuote} onSave={activeConversationId ? async (selection) => {
        const message = messages.find((item) => item.id === selection.messageId);
        if (!message) throw new Error("Source unavailable");
        const start = message.markdown.indexOf(selection.text);
        if (start < 0 || message.markdown.indexOf(selection.text, start + 1) !== -1) throw new Error("Selection does not identify one exact source range");
        const key = JSON.stringify([project.id, activeConversationId, message.id, message.markdown, start, selection.text]);
        if (pendingExcerpt.current?.key !== key) pendingExcerpt.current = { key, request: { request_id: crypto.randomUUID(), title: selection.text.slice(0, 60), kind: "excerpt", source: await messageLibrarySource(project.id, activeConversationId, message.id, message.markdown, start, start + selection.text.length) } };
        await saveWorkspaceLibraryItem(pendingExcerpt.current.request);
        if (pendingExcerpt.current?.key === key) pendingExcerpt.current = null;
      } : undefined} />
      <footer className="composer">
        {branchSendError && <p role="alert">{zh ? "分支发送或打开未确认，原始消息和材料已保留。" : "Branch sending or opening was not confirmed; the original message and material are retained."}</p>}
        {branchSendPending && onRetryBranchSend && <button type="button" disabled={branchSendBusy} onClick={() => { const original = branchSendOriginalMarkdown; void onRetryBranchSend().then((accepted) => { if (accepted && original) setDraft((current) => current.trim() === original ? "" : current); }); }}>{zh ? "重试原分支发送" : "Retry original branch send"}</button>}
        {replacement?.error && <p role="alert" className="composer-error">{replacement.pending ? (zh ? "替换请求状态待核对，原草稿已保留。" : "Replacement status is unconfirmed; the original draft is retained.") : (zh ? "替换请求未接收，请刷新运行状态后重试。" : "Replacement was not accepted. Refresh the run before trying again.")}{replacement.pending && <><span>{replacement.originalMarkdown}</span><button type="button" disabled={replacement.busy || conversationHydrating} onClick={() => void retryReplacement()}>{zh ? "核对原替换请求" : "Reconcile original replacement"}</button></>}</p>}
         {queueError && <p role="alert" className="composer-error">{zh ? "队列状态未能确认，请刷新后重试。" : "Queue state could not be confirmed. Refresh and retry."}<button type="button" onClick={() => void onQueueRefresh?.()}>{zh ? "刷新队列" : "Refresh queue"}</button></p>}
         {sendError && <p role="alert" className="composer-error">{zh ? "消息未能发送，草稿已保留。请重试。" : "Your message could not be sent. The draft is preserved; please retry."}</p>}
         {composerCommandNotice && <p role="alert" className="composer-error">{composerCommandNotice}</p>}
         {(agentPreferences.loading || agentPreferences.saving) && <p className="agent-preferences-status" role="status">{agentPreferences.loading ? (zh ? "正在加载会话偏好…" : "Loading conversation preferences…") : (zh ? "正在保存会话偏好…" : "Saving conversation preferences…")}</p>}
         {preferenceError && <p className="agent-preferences-error" role="alert"><span>{preferenceError}</span><button type="button" aria-label={zh ? "重试会话偏好" : "Retry conversation preferences"} onClick={() => agentPreferences.retry()} disabled={agentPreferences.saving}>{zh ? "重试" : "Retry"}</button></p>}

         <div className="composer-runtime-bar">
          <div className="composer-menu-anchor compute-anchor">
            <button className="runtime-host" aria-label={zh ? "选择计算后端" : "Choose compute backend"} aria-expanded={computeMenuOpen} onClick={() => { const next = !computeMenuOpen; closeComposerMenus(); setComputeMenuOpen(next); }}><Monitor size={17} /><b>{backendLabel}</b><ChevronDown size={13} /></button>
            {computeMenuOpen && <div className="composer-compute-menu"><ComputeBackendSelector locale={locale} backends={computeBackends} backendId={computeBackendId} containerImage={containerImage} autonomyMode={autonomyMode} environment={computeEnvironment} busy={computeBusy || existingComposerDisabled} onBackendChange={onComputeBackendChange} onImageChange={onContainerImageChange} onAutonomyChange={onAutonomyModeChange} onEnvironmentChange={onComputeEnvironmentChange} /><button className="compute-settings-link" disabled={!onOpenSettings} onClick={() => { setComputeMenuOpen(false); onOpenSettings?.("remote"); }}>{zh ? "添加 SSH 主机 / 管理环境" : "Add SSH host / Manage environments"}<ChevronRight size={14} /></button></div>}
          </div>
          {(["python", "r"] as const).map((language) => <button key={language} className="runtime-pill" aria-label={`${language === "python" ? "Python" : "R"} ${zh ? "环境" : "environment"}`} onClick={() => { closeComposerMenus(); setRuntimeLanguage(language); }}><b>{language === "python" ? "Python" : "R"}</b><span>{runtimeStatus(language)}</span></button>)}
        </div>
        <div className={`composer-input ${planModeEnabled ? "is-plan-mode" : ""}`} onDragOver={(event) => { if (event.dataTransfer.types.includes(WORKSPACE_FILE_DRAG_TYPE) || event.dataTransfer.types.includes("Files")) { event.preventDefault(); event.dataTransfer.dropEffect = composerDisabled ? "none" : "copy"; } }} onDrop={(event) => {
            if (event.dataTransfer.types.includes(WORKSPACE_FILE_DRAG_TYPE)) {
              event.preventDefault(); const reference = parseWorkspaceFileDrag(event.dataTransfer.getData(WORKSPACE_FILE_DRAG_TYPE));
              if (reference) attachWorkspaceFile(reference); else setReferenceNotice(zh ? "文件引用无效。" : "Invalid file reference.");
              return;
            }
            if (!event.dataTransfer.files.length) return;
            event.preventDefault(); if (!composerDisabled) void attachments.addFiles(Array.from(event.dataTransfer.files));
          }}>
          {referenceNotice && <p className="composer-error" role="alert">{referenceNotice}</p>}
          <ComposerAttachments items={attachments.items} zh={zh} disabled={composerDisabled} onRemove={attachments.remove} onRetry={attachments.retry} />
          {attachments.pickerBusyNotice && <p role="alert">{zh ? "请等待当前附件选择或处理完成，再添加其他附件。" : "Wait for the current file selection or upload to finish, then add more attachments."}</p>}
          {attachments.limitReached && <p role="alert">{zh ? "部分附件未添加：每次最多 8 个、单个 20 MiB、合计 40 MiB。请移除附件后重试。" : "Some attachments were not added: maximum 8 files, 20 MiB each and 40 MiB total. Remove attachments and try again."}</p>}
          <ComposerReferenceChips references={selectedReferences.map((item) => item.reference)} items={selectedReferences} disabled={composerDisabled} zh={zh} onRemove={(reference) => { setSelectedReferences((items) => items.filter((entry) => referenceKey(entry.reference) !== referenceKey(reference))); setCatalogError(""); setReferenceNotice(""); }} />
          {referenceTrigger && <ComposerReferencePicker commands={composerCommands} inputRef={draftRef} items={(referenceCatalog ?? catalog).filter((item) => !(item.reference.kind === "session" && item.reference.id === activeConversationId))} trigger={referenceTrigger} onSelect={selectReference} onClose={() => setReferenceTrigger(null)} zh={zh} loading={catalogLoading} error={catalogError} />}
          <textarea ref={draftRef} onPaste={(event) => {
            const images = Array.from(event.clipboardData.files).filter((file) => file.type.startsWith("image/"));
            if (images.length) { event.preventDefault(); if (!composerDisabled) void attachments.addFiles(images); return; }
            const paths = parseClipboardFilePaths(event.clipboardData.getData?.("text/plain") ?? "");
            if (paths) { event.preventDefault(); void attachClipboardPaths(paths); }
          }} onPointerDown={(event) => { resizeStartHeight.current = event.currentTarget.getBoundingClientRect().height; }} onPointerUp={(event) => { if (resizeStartHeight.current !== null && Math.abs(event.currentTarget.getBoundingClientRect().height - resizeStartHeight.current) > 1) manualDraftHeight.current = true; resizeStartHeight.current = null; }} aria-label={t.composer} placeholder={`${planModeEnabled ? (zh ? "描述需要规划和执行的任务" : "Describe the task to plan and execute") : t.composer} — ${zh ? "@ 产物与环境，# 项目与会话，/ 命令、工作流与技能" : "@ artifacts and environments, # projects and sessions, / commands, workflows and skills"}`} value={draft} disabled={composerDisabled} onChange={(event) => { setDraft(event.target.value); setReferenceTrigger(parseComposerTrigger(event.target.value, event.target.selectionStart)); setSendError(false); setComposerCommandNotice(""); }} onSelect={(event) => { const input = event.currentTarget; setReferenceTrigger(parseComposerTrigger(input.value, input.selectionStart)); }} onKeyDown={(event) => { if (event.defaultPrevented) return; if (event.key === "Enter" && !event.shiftKey && !event.nativeEvent.isComposing && event.keyCode !== 229 && (!modifierSend || event.ctrlKey || event.metaKey)) { event.preventDefault(); void send(); } }} />
          <div className="composer-toolbar">
            <div className="composer-menu-anchor"><button className="composer-tool" aria-label={zh ? "添加上下文或选择模式" : "Add context or choose mode"} aria-expanded={composerMenuOpen} onClick={() => { const next = !composerMenuOpen; closeComposerMenus(); setComposerMenuOpen(next); }}><Plus size={20} /></button>{composerMenuOpen && <ComposeActions zh={zh} onClose={() => setComposerMenuOpen(false)} onAttach={activeConversationId && !composerDisabled ? () => { void attachments.chooseFiles(); } : undefined} onFiles={openProjectFiles} onReview={() => setReviewDialogOpen(true)} onShare={messages.length ? () => setSharing(true) : undefined} onSaveSkill={composerDisabled ? undefined : prepareSkill} onManageWorkflows={() => setWorkflowLibraryOpen(true)} onManageSkills={onOpenSettings ? () => onOpenSettings("skills") : undefined} />}</div>
            <div className="composer-menu-anchor permission-anchor">
              <button className="composer-tool composer-orbit" title={zh ? "Agent 控制" : "Agent controls"} aria-label={zh ? "Agent 权限" : "Agent permissions"} aria-expanded={permissionMenuOpen} onClick={() => { const next = !permissionMenuOpen; closeComposerMenus(); setPermissionMenuOpen(next); }}><Settings size={20} strokeWidth={2.1} /></button>
              {permissionMenuOpen && <div className="permission-menu" role="menu" aria-label={zh ? "Agent 权限选项" : "Agent permission options"}>
                <button className="agent-control-row" role="menuitemcheckbox" aria-checked={planModeEnabled} disabled={modeLocked} onClick={() => chooseMode(planModeEnabled ? "agent" : "plan")}><span>{zh ? "先做计划" : "Plan first"}</span><i className={`control-switch ${planModeEnabled ? "is-on" : ""}`} /></button>
                <header><b>{zh ? "应如何批准 Agent 操作？" : "How should Agent actions be approved?"}</b><small>{selectedBackend?.descriptor.kind.toUpperCase() ?? "—"}</small></header>
                <PermissionOption icon={<Hand size={17} />} active={approvalPolicy === "request_approval"} title={zh ? "请求批准" : "Ask approval"} description={zh ? "写入、命令和网络操作前请求确认" : "Ask before writes, commands, and network operations"} onClick={() => { onApprovalPolicyChange?.("request_approval"); onAutonomyModeChange?.("supervised"); setPermissionMenuOpen(false); }} />
                <PermissionOption icon={<ShieldCheck size={17} />} active={approvalPolicy === "risk_based"} title={zh ? "帮我批准" : "Risk based"} description={zh ? "仅对检测到的风险操作请求批准" : "Ask only for operations detected as risky"} onClick={() => { onApprovalPolicyChange?.("risk_based"); onAutonomyModeChange?.("supervised"); setPermissionMenuOpen(false); }} />
                <PermissionOption icon={<ShieldAlert size={17} />} active={approvalPolicy === "full_access"} danger disabled={!selectedBackendIsContainer} title={zh ? "完全访问权限" : "Full access"} description={selectedBackendIsContainer ? (zh ? "仅限离线 Docker/Podman 容器" : "Offline Docker/Podman containers only") : (zh ? "需要可用的 Docker/Podman 隔离" : "Requires available Docker/Podman isolation")} onClick={() => { onApprovalPolicyChange?.("full_access"); onAutonomyModeChange?.("full_auto"); setPermissionMenuOpen(false); }} />
                <div className="agent-control-divider" />
                <button className="agent-control-row" role="menuitemcheckbox" aria-checked={modifierSend} onClick={toggleModifierSend}><span>{zh ? "使用 Ctrl/Cmd+Enter 发送" : "Send with Ctrl/Cmd+Enter"}</span><i className={`control-switch ${modifierSend ? "is-on" : ""}`} /></button>
                <button role="menuitem" className="agent-control-row" disabled><span>{zh ? "完成方式" : "Completion"}</span><small>{zh ? "会话内" : "Inline"}</small></button>
                <PreferenceToggle active={agentPreferences.preferences.delegation_enabled} disabled={existingComposerDisabled || agentPreferences.busy || Boolean(agentPreferences.loadError) || !activeConversationId} title={zh ? "子任务委派" : "Delegation"} description={zh ? "允许 Agent 在批准范围内委派子任务" : "Allow Agent to delegate within the approved scope"} onClick={() => toggleAgentPreference("delegation_enabled")} />
                <PreferenceToggle active={agentPreferences.preferences.auto_review} disabled={existingComposerDisabled || agentPreferences.busy || Boolean(agentPreferences.loadError) || !activeConversationId} title={zh ? "自动审查" : "Auto-review"} description={zh ? "可选的模型审查；证据和结果核验仍然必需" : "Run an optional model review; evidence and result checks remain required"} onClick={() => toggleAgentPreference("auto_review")} />
                <PreferenceToggle active={agentPreferences.preferences.memory_enabled} disabled={existingComposerDisabled || agentPreferences.busy || Boolean(agentPreferences.loadError) || !activeConversationId} title={zh ? "使用记忆" : "Use memory"} description={zh ? "允许 Agent 检索项目记忆" : "Allow Agent to retrieve project memory"} onClick={() => toggleAgentPreference("memory_enabled")} />
                {showFastMode && <label className="fast-mode-select"><span><b>{zh ? "Fast 模式" : "Fast mode"}</b><small>{zh ? "请求 Fast 处理；可用性取决于提供方" : "Requests Fast processing; availability depends on the provider"}</small></span><select aria-label={zh ? "Fast 模式" : "Fast mode"} value={fastModeOption} disabled={fastModeLocked} onChange={(event) => chooseFastMode(event.target.value)}>
                  <option value="default">{zh ? "模型默认" : "Model default"}</option>
                  <option value="standard">{zh ? "标准" : "Standard"}</option>
                  <option value="fast" disabled={!fastModeAvailable && fastMode !== true}>Fast</option>
                </select></label>}
                 <button role="menuitem" className="agent-control-row" onClick={() => { setPermissionMenuOpen(false); setReviewerSettingsOpen(true); }}><span>{zh ? "审查模型" : "Reviewer model"}</span><small>{zh ? "配置只读审核模型" : "Configure read-only reviewer"}</small><ChevronRight size={14} /></button>
                 <button role="menuitem" className="agent-control-row" disabled><span>{zh ? "分析工具失败" : "Analyze tool failures"}</span><small>{zh ? "暂未支持" : "Unavailable"}</small></button>
                 <button role="menuitem" className="agent-control-row" disabled={composerDisabled} onClick={() => { setPermissionMenuOpen(false); setProjectTemplatePickerOpen(true); }}><span>{zh ? "快捷操作与专家角色" : "Quick actions & roles"}</span><small>{zh ? "追加工作流引用或可见角色草稿" : "Insert a workflow reference or visible role draft"}</small><ChevronRight size={14} /></button>
                <button role="menuitem" className="agent-control-row" onClick={() => { openSidebarSection("records"); setPermissionMenuOpen(false); }}><span>{zh ? "记忆与研究记录" : "Memory & notebook"}</span><ChevronRight size={14} /></button>
                <button role="menuitem" className="agent-control-row" onClick={() => setComputeMenuOpen(true)}><span>{zh ? "计算环境" : "Compute"}</span><span>{backendLabel}<ChevronRight size={14} /></span></button>
              </div>}
            </div>
            {planModeEnabled && <button className="composer-mode-chip" disabled={modeLocked} onClick={() => chooseMode("agent")}><Activity size={14} />Plan<X size={13} /></button>}
             <div className="composer-send-controls">
             <button type="button" className="context-meter" aria-label={zh ? "查看上下文用量" : "Inspect context usage"} onClick={() => setContextUsageOpen(true)}><Gauge size={19} /><span>{contextUsage?.contextTokens != null && contextUsage.contextLimit != null && contextUsage.contextLimit > 0 ? `${contextUsage.estimated ? "≈" : ""}${(100 * contextUsage.contextTokens / contextUsage.contextLimit).toFixed(0)}%` : "—"}</span></button>
             {showFastMode && <button className={`composer-tool fast-mode-toggle ${effectiveFastMode === true ? "is-active" : ""}`} disabled={fastModeButtonDisabled} aria-label={zh ? "Fast 模式" : "Fast mode"} aria-pressed={effectiveFastMode === true} title={zh ? "请求 Fast 处理；可用性取决于提供方" : "Request Fast processing; availability depends on the provider"} onClick={toggleFastMode}><Zap size={19} /></button>}
             {modelPicker ? <div onClickCapture={closeComposerMenus}>{modelPicker}</div> : <div className="composer-menu-anchor model-anchor"><button className="composer-model" disabled={composerDisabled} aria-label={zh ? "选择模型" : "Choose model"} aria-expanded={modelMenuOpen} onClick={() => { const next = !modelMenuOpen; closeComposerMenus(); setModelMenuOpen(next); }}><span>{modelLabel || (zh ? "选择模型" : "Choose model")}</span><ChevronDown size={12} /></button>{modelMenuOpen && <div className="model-menu" role="menu">{modelOptions.map((model) => <button role="menuitemradio" aria-checked={model.id === modelId} key={model.id} disabled={!onModelChange} onClick={() => { onModelChange?.(model.id); setModelMenuOpen(false); }}>{model.label}{model.id === modelId && <Check size={14} />}</button>)}<button role="menuitem" disabled={!onOpenSettings} onClick={() => { setModelMenuOpen(false); onOpenSettings?.(); }}>{zh ? "管理模型" : "Manage models"}<Settings size={14} /></button></div>}</div>}
            <div className="composer-send-group">
             {stopAvailable && !showStopButton && <button type="button" className="composer-tool" aria-label={stopLabel} title={stopLabel} disabled={runStopping} onClick={() => void onCancelRun?.()}><Square size={16} fill="currentColor" /></button>}
             <button className={`send-button ${showStopButton ? "is-stop" : ""}`} aria-label={showStopButton ? stopLabel : sendLabel} title={showStopButton ? stopLabel : sendLabel} aria-busy={showStopButton && runStopping} disabled={showStopButton ? runStopping : composerDisabled || (!onQueue && reviewBusy) || agentPreferences.busy || Boolean(agentPreferences.loadError) || (!onQueue && planLoading) || attachmentsBlocked || (!draft.trim() && !attachments.receipts.length) || Boolean((onSend || onQueue) && !computeReady && !typedComposerCommand)} onClick={showStopButton ? () => { void onCancelRun?.(); } : send}>{showStopButton ? <Square size={16} fill="currentColor" /> : <span>{queuedSend ? (zh ? "加入队列" : "Add to queue") : t.send}</span>}</button>
            <div className="composer-menu-anchor"><button ref={sendOptionsRef} className="send-options" aria-label={zh ? "发送选项" : "Send options"} aria-expanded={sendMenuOpen} onClick={() => { const next = !sendMenuOpen; closeComposerMenus(); setSendMenuOpen(next); }}><ChevronDown size={17} /></button>{sendMenuOpen && <div className="model-menu send-menu" role="menu">{replacement && <button type="button" role="menuitem" disabled={replacementDisabled} onClick={() => void replaceCurrentTurn()}><span>{zh ? "中断并替换" : "Interrupt and replace"}</span><small>{zh ? "停止当前运行，安全结束后优先发送此草稿" : "Stop the current run, then send this draft first after safe settlement"}</small></button>}{sideChat && <button type="button" role="menuitem" onClick={() => void sendSideQuestion()}><span>{zh ? "独立旁聊" : "Side chat"}</span><small>{zh ? "根据证据回答，不打断主任务" : "Ask about evidence while the main task continues"}</small></button>}{onOpenBranch && <button type="button" role="menuitem" disabled={branchDisabled || !lastBranchAnchor} onClick={() => { if (lastBranchAnchor) branchAt(lastBranchAnchor.id, "after_response", draft); }}><span>{zh ? "分叉会话" : "Branch conversation"}</span><small>{onBranchSend && (draft.trim() || attachments.receipts.length) ? (zh ? "创建分支并发送完整草稿" : "Create a branch and send the complete draft") : (zh ? "从最近一轮创建分支" : "Branch from the latest turn")}</small></button>}{guidanceDialogAvailable && <button type="button" role="menuitem" className="guidance-menu-item" onClick={openGuidanceDialog}><span>{zh ? "追加指导" : "Add guidance"}</span><small>{guidanceEnabled ? (zh ? "追加到当前运行" : "Append to the current run") : (zh ? "仅查看已接收历史" : "View received history only")}</small></button>}{(["agent", "plan"] as const).map((mode) => <button key={mode} role="menuitemradio" aria-checked={effectiveMode === mode} disabled={modeLocked} onClick={() => { chooseMode(mode); setSendMenuOpen(false); }}>{mode === "plan" ? (zh ? "先做计划" : "Plan first") : (zh ? "直接执行" : "Execute directly")}{effectiveMode === mode && <Check size={14} />}</button>)}</div>}</div>
            </div>
            </div>
          </div>
        </div>
        <small>{conversationHydrating ? (zh ? "正在恢复会话模式和运行状态…" : "Restoring conversation mode and run state…") : planModeEnabled ? (zh ? "Plan 模式：计划显示在右侧，批准后才执行" : "Plan mode: review the plan on the right before execution") : computeReady ? (zh ? `Agent 模式：${selectedBackend?.descriptor.kind.toUpperCase()} · ${approvalPolicy === "request_approval" ? "请求批准" : approvalPolicy === "full_access" ? "完全访问" : "风险审批"}` : `Agent mode: ${selectedBackend?.descriptor.kind.toUpperCase()} · ${approvalPolicy}`) : onSend ? (zh ? "请选择计算配置" : "Choose a compute configuration") : modelLabel ? `${zh ? "当前模型" : "Model"}: ${modelLabel}` : ""}</small>
      </footer>
    </main>

    {fileTextPreview && activeConversationId && <ComposerFilePreviewDialog key={`${project.id}:${activeConversationId}:${referenceKey(fileTextPreview)}`} reference={fileTextPreview} conversationId={activeConversationId} zh={zh} disabled={composerDisabled || selectedReferences.length >= 12} onClose={() => setFileTextPreview(null)} onAttach={(item) => {
      if (composerDisabled || item.reference.kind !== "quote" || item.reference.project_id !== project.id) return;
      setSelectedReferences((current) => current.length >= 12 || current.some((entry) => referenceKey(entry.reference) === referenceKey(item.reference)) ? current : [...current, item]);
      setFileTextPreview(null); draftRef.current?.focus();
    }} />}
    {reviewDialogOpen && activeConversationId && <SessionReviewDialog key={`${project.id}:${activeConversationId}`} locale={locale} records={sessionReviews.records} loading={sessionReviews.loading} busy={sessionReviews.busy} startDisabled={!messages.some((message) => message.role !== "system" && message.markdown.trim()) || existingComposerDisabled || agentPreferences.busy || Boolean(agentPreferences.loadError)} startDisabledReason={!messages.some((message) => message.role !== "system" && message.markdown.trim()) ? (zh ? "先发送一条消息后再发起审核。" : "Send a message before requesting a review.") : undefined} error={sessionReviews.error} modelProfiles={modelProfiles} onStartReview={sessionReviews.startReview} onRetry={sessionReviews.retry} onOpenReviewerSettings={() => setReviewerSettingsOpen(true)} onClose={() => setReviewDialogOpen(false)} />}
    {guidanceDialogOpen && guidanceDialogAvailable && activeConversationId && guidanceRunId && <GuidanceDialog key={`${project.id}:${activeConversationId}:${guidanceRunId}`} runId={guidanceRunId} projectId={project.id} conversationId={activeConversationId} enabled={guidanceEnabled} locale={locale} initialDraft={guidanceDraftAtOpenRef.current ?? undefined} pendingRequest={guidancePendingRequestForScope(guidanceRunId)} composerHasAttachments={attachments.items.length > 0 || attachments.receipts.length > 0} composerHasReferences={selectedReferences.length > 0} onAccepted={handleGuidanceAccepted} onClose={closeGuidanceDialog} />}
    {reviewerSettingsOpen && <ReviewerSettingsDialog zh={zh} modelProfiles={modelProfiles} onClose={() => setReviewerSettingsOpen(false)} />}
    {workflowLibraryOpen && <WorkflowLibraryDialog key={project.id} projectId={project.id} zh={zh} onClose={() => { setWorkflowLibraryOpen(false); draftRef.current?.focus(); }} onChanged={() => setLocalWorkflowCatalogVersion((value) => value + 1)} />}
    {projectTemplatePickerOpen && <ProjectTemplatePicker key={project.id} selectedProject={project} locale={locale} onSelectWorkflow={insertQuickActionWorkflow} onSelectSpecialist={insertSpecialistDraft} onClose={() => { setProjectTemplatePickerOpen(false); draftRef.current?.focus(); }} />}
    {sharing && <ShareConversationDialog key={`${project.id}:${activeConversationId}`} messages={messages} locale={locale} onClose={() => { setSharing(false); draftRef.current?.focus(); }} />}
    {contextUsageOpen && <ContextUsagePanel error={contextUsageError} value={contextUsage} locale={locale} onClose={() => setContextUsageOpen(false)} onNewConversation={!conversationHydrating && !agentBusy && !conversationLocked && onNewConversation ? () => { setContextUsageOpen(false); void onNewConversation(); } : undefined} />}
    {sidebarOpen && <aside id="workspace-sidebar" className={`context-pane workspace-sidebar ${workspacePage !== "conversation" ? "workspace-content-hidden" : ""}`} hidden={workspacePage !== "conversation"} aria-label={t.context}>
      <div className="sidebar-heading">
        <div className="sidebar-tab-scroll" role="tablist" aria-label={zh ? "已打开的侧栏标签" : "Open sidebar tabs"}>
          {openTabs.map((id) => <div className="sidebar-tab-wrap" key={id} draggable onDragStart={() => setDraggedTab(id)} onDragEnd={() => setDraggedTab(null)} onDragOver={(event) => { if (draggedTab) event.preventDefault(); }} onDrop={(event) => { event.preventDefault(); if (draggedTab && draggedTab !== id) { const reordered = openTabs.filter((item) => item !== draggedTab); reordered.splice(openTabs.indexOf(id), 0, draggedTab); setOpenTabs(reordered); } setDraggedTab(null); }}>
            <button id={`sidebar-tab-${id}`} role="tab" aria-controls="sidebar-tab-content" aria-selected={tab === id} tabIndex={tab === id ? 0 : -1} title={tabLabel(id)} onClick={() => { setTab(id); setSectionMenuOpen(false); }} onKeyDown={(event) => { const index = openTabs.indexOf(id); const next = event.key === "ArrowRight" ? openTabs[(index + 1) % openTabs.length] : event.key === "ArrowLeft" ? openTabs[(index + openTabs.length - 1) % openTabs.length] : event.key === "Home" ? openTabs[0] : event.key === "End" ? openTabs.at(-1) : undefined; if (next) { event.preventDefault(); setTab(next); document.getElementById(`sidebar-tab-${next}`)?.focus(); } }}>{tabLabel(id)}</button>
            <button className="sidebar-tab-close" aria-label={zh ? `关闭标签：${tabLabel(id)}` : `Close tab: ${tabLabel(id)}`} onClick={() => closeSidebarTab(id)}><X size={12} /></button>
          </div>)}
        </div>
        <button className="sidebar-tab-add" aria-label={zh ? "添加侧栏标签" : "Add sidebar tab"} aria-haspopup="menu" aria-expanded={sectionMenuOpen} onClick={() => setSectionMenuOpen((open) => !open)}><Plus size={16} /></button>
        {sectionMenuOpen && <><button className="sidebar-menu-backdrop" aria-label={zh ? "关闭侧栏菜单" : "Close sidebar menu"} onClick={() => setSectionMenuOpen(false)} /><div className="sidebar-section-menu" role="menu" aria-label={zh ? "侧栏内容" : "Sidebar sections"}>{sidebarSections.map((section) => { const isOpen = openTabs.some((id) => id === section.id); return <button key={section.id} role="menuitemcheckbox" aria-checked={isOpen} disabled={Boolean(section.unavailable)} title={section.unavailable} onClick={() => { if (section.id !== "highlights") openSidebarSection(section.id); }}><span>{section.label}</span>{isOpen && <Check size={18} />}</button>; })}</div></>}
      </div>
      <div className="context-content" id="sidebar-tab-content" role="tabpanel" aria-labelledby={`sidebar-tab-${tab}`}>{tab === "side-chat" && sideChat && activeConversationId && <SideChatPanel key={`${project.id}:${activeConversationId}`} projectId={project.id} conversationId={activeConversationId} locale={locale} records={sideChat.records} models={modelProfiles} modelId={sideChat.modelId} onModelChange={sideChat.setModelId} draftValue={sideChat.draft} onDraftChange={sideChat.setDraft} loading={sideChat.loading} disabled={!sideChat.ready} busy={sideChat.busy} pending={sideChat.pending} error={sideChat.error} originalQuestion={sideChat.originalQuestion} onSend={(question) => sideChat.send({ question_markdown: question, references: [], attachments: [] })} onRefresh={sideChat.refresh} hasMore={sideChat.hasMore} loadingOlder={sideChat.loadingOlder} onLoadOlder={sideChat.loadOlder} onRetry={retrySideQuestion} onRetryTurn={(turn) => sideChat.send({ question_markdown: turn.question_markdown, references: turn.references, attachments: turn.attachments, parent_request_id: turn.request_id })} onOpenSource={revealSideChatSource} />}{tab === "files" && <><div className="file-source-options" role="group" aria-label={zh ? "文件来源" : "File source"}><button aria-pressed={fileSource === "local"} onClick={() => setFileSource("local")}>{zh ? "本地" : "Local"}</button><button aria-pressed={fileSource === "remote"} disabled={!project.connection_id && !onRefreshFiles && !remoteFiles?.length} onClick={() => setFileSource("remote")}>SSH</button></div><RemoteFileTree locale={locale} source={fileSource} remoteFiles={fileSource === "local" ? localFiles : remoteFiles ?? []} busy={fileSource === "local" ? localFilesBusy : filesBusy} notice={fileSource === "local" ? localFilesError : fileNotice} onUpload={fileSource === "remote" ? onUploadFiles : undefined} onRefresh={() => setFileRefreshVersion((value) => value + 1)} onDownload={fileSource === "remote" ? onDownloadFile : undefined} onPreviewText={activeConversationId && !composerDisabled && fileBackendId ? (path) => setFileTextPreview(currentFileReference(path)) : undefined} onAttach={!composerDisabled && fileBackendId ? (path) => attachWorkspaceFile(currentFileReference(path)) : undefined} dragReference={!composerDisabled && fileBackendId ? currentFileReference : undefined} syncEntries={fileSource === "remote" ? syncEntries : []} onPauseSync={onPauseSync} onCancelSync={onCancelSync} onRetrySync={onRetrySync} /></>}{tab === "plan" && <V4PlanPanel locale={locale} active={showPlanPanel} planLoading={planLoading} v4Plan={visiblePlan} latestPlanRevision={latestPlanRevision} conversationLocked={conversationLocked} planActionBusy={effectivePlanActionBusy} planApproved={planApproved || approved || approvePlanBusy} runStarted={runStarted} events={activeRunEventsV4} onApprove={approvePlan} onRequestPlanRevision={onRequestPlanRevision} onCancel={onCancelRun} />}{tab === "artifacts" && <>{openedSearchArtifact?.reference.kind === "artifact" && openedSearchArtifact.reference.project_id === project.id && <section className="sidebar-data-card" aria-label={zh ? "选中的产物" : "Selected artifact"}><b>{openedSearchArtifact.label}</b><p>{openedSearchArtifact.description}</p><small>ID: {openedSearchArtifact.reference.id}</small><button onClick={() => setOpenedSearchArtifact(null)}>{zh ? "关闭详情" : "Close details"}</button></section>}<ArtifactCatalog artifacts={projectArtifacts} locale={locale} onSelect={(path) => { setSelectedImagePath(path); setExpanded(true); }} />{imageFiles.length > 0 && <><div className="context-toolbar"><span>{t.artifactPreview}</span><button aria-label={t.expand} onClick={() => setExpanded(true)}><Expand size={16} /></button></div>{preview}</>}</>}{tab === "notebook" && <CodeNotebook cells={notebookCells} locale={locale} collectionSource={activeConversationId ? async (cell) => { if (cell.toolSource) return cell.toolSource; const range = cell.messageRange; const message = messages.find((item) => item.id === range?.messageId); if (!range || !message) throw new Error("Source unavailable"); return messageLibrarySource(project.id, activeConversationId, message.id, message.markdown, range.start, range.end); } : undefined} />}{tab === "agents" && <DelegatedAgents tasks={delegatedTasks} locale={locale} />}{tab === "records" && <Notebook locale={locale} entries={notebookEntries} artifacts={projectArtifacts} facts={memoryFacts} onSearch={onSearchMemory} onExport={onExportNotebook} />}{tab === "environment" && <><EnvironmentContexts backends={computeBackends} selectedId={computeBackendId} environment={computeEnvironment} locale={locale} /><KernelPanel locale={locale} sessions={kernelSessions} events={kernelEvents} busy={kernelBusy} notice={kernelNotice} onStart={onStartKernel} onExecute={onExecuteKernel} onInterrupt={onInterruptKernel} onStop={onStopKernel} onPromote={onPromoteKernelCell} /></>}{tab === "provenance" && <ProvenancePanel rows={provenanceRows} locale={locale} />}</div>
    </aside>}
    {runtimeLanguage && <RuntimeDialog zh={zh} language={runtimeLanguage} onLanguageChange={setRuntimeLanguage} backend={selectedBackend} environment={computeEnvironment} onClose={() => setRuntimeLanguage(null)} onSettings={onOpenSettings ? () => { setRuntimeLanguage(null); onOpenSettings("remote"); } : undefined} onPrepare={(language) => {
      const name = language === "python" ? "Python" : "R";
      const request = zh ? `请检查当前 ${backendLabel} 计算环境中的 ${name} 解释器和科研依赖，报告版本与缺失项，并按当前审批策略准备环境。使用项目隔离环境，避免修改系统环境；先验证最小示例再报告结果。` : `Check the ${name} interpreter and research dependencies in the current ${backendLabel} compute environment. Report versions and missing dependencies, and prepare a project-isolated environment under the current approval policy without modifying the system environment. Verify a minimal example before reporting the result.`;
      setDraft((current) => current ? `${current}\n\n${request}` : request);
      setRuntimeLanguage(null);
    }} />}
    {expanded && <div className="preview-overlay" role="dialog" aria-modal="true" aria-label={t.artifactPreview}><header><div><small>{project.name}</small><h2>{t.artifactPreview}</h2></div><button aria-label="Close" onClick={() => setExpanded(false)}><X /></button></header>{preview}</div>}
    {trajectoryOpen && <div className="preview-overlay" role="dialog" aria-modal="true" aria-label={zh ? "运行轨迹" : "Run trajectory"}><header><div><small>{project.name}</small><h2>{zh ? "运行轨迹" : "Run trajectory"}</h2></div><button aria-label={zh ? "关闭运行轨迹" : "Close run trajectory"} onClick={() => { setTrajectoryOpen(false); draftRef.current?.focus(); }}><X /></button></header><div style={{ minHeight: 0, overflow: "auto", padding: "16px 20px" }}>{trajectoryRuns.length > 0 ? trajectoryRuns.map((run) => <V4RunTrace key={run.runId} locale={locale} events={run.events} historical />) : <p role="status">{zh ? "当前会话暂无已记录的运行轨迹。" : "No recorded run trajectory is available for this conversation."}</p>}</div></div>}
  </div>;
}

function ComputeBackendSelector({ locale, backends, backendId, containerImage, autonomyMode: _autonomyMode, environment, busy, onBackendChange, onImageChange, onAutonomyChange: _onAutonomyChange, onEnvironmentChange }: { locale: Locale; backends: ComputeBackendAvailabilityV4[]; backendId: string; containerImage: string; autonomyMode: AutonomyModeV4; environment: string; busy: boolean; onBackendChange?: (value: string) => void; onImageChange?: (value: string) => void; onAutonomyChange?: (value: AutonomyModeV4) => void; onEnvironmentChange?: (value: string) => void }) {
  const zh = locale === "zh-CN";
  const selected = backends.find((item) => item.descriptor.backend_id === backendId);
  const container = selected?.descriptor.isolation === "container";
  return <section className="compute-selector" aria-label={zh ? "V4 计算后端" : "V4 compute backend"}><header><div><b>{zh ? "冻结计算配置" : "Frozen compute selection"}</b><small>{busy ? (zh ? "正在读取配置…" : "Loading configuration…") : (zh ? "本地与 SSH 按需连接；选择配置不代表依赖已安装" : "Local and SSH initialize on demand; selection does not verify dependencies")}</small></div><span>{selected?.descriptor.isolation ?? "—"}</span></header><div className="compute-backend-grid">{backends.map((backend) => <label className={backendId === backend.descriptor.backend_id ? "active" : ""} key={backend.descriptor.backend_id}><input type="radio" name="v4-backend" value={backend.descriptor.backend_id} checked={backendId === backend.descriptor.backend_id} disabled={busy || !backend.selectable || !onBackendChange} onChange={() => onBackendChange?.(backend.descriptor.backend_id)} /><span><b>{backend.descriptor.kind.toUpperCase()}</b><small>Python: {backend.python_status} · R: {backend.r_status}</small>{backend.reason && <em>{backend.reason}</em>}</span></label>)}</div>{container && <label>{zh ? "本地已有容器镜像" : "Existing local image"}<input disabled={busy || !onImageChange} aria-label={zh ? "容器镜像" : "Container image"} value={containerImage} placeholder="omicsops/science:latest" onChange={(event) => onImageChange?.(event.target.value)} /><small>{selected?.resolved_image_id ? `image ID: ${selected.resolved_image_id}` : (zh ? "输入后只执行 image inspect" : "Only image inspect runs after entry")}</small></label>}{selected?.descriptor.kind === "ssh" && <label>{zh ? "SSH 环境" : "SSH environment"}<input disabled={busy || !onEnvironmentChange} aria-label={zh ? "SSH 环境" : "SSH environment"} value={environment} onChange={(event) => onEnvironmentChange?.(event.target.value)} /><small>{zh ? "system 或安全的 Micromamba 环境名" : "system or a safe Micromamba environment name"}</small></label>}<div className="compute-policy"><span>{container ? "network=none" : "network=host_inherited"}</span></div></section>;
}

function PermissionOption({ icon, active, danger = false, disabled = false, title, description, onClick }: { icon: ReactNode; active: boolean; danger?: boolean; disabled?: boolean; title: string; description: string; onClick: () => void }) {
  return <button role="menuitemradio" aria-checked={active} disabled={disabled} className={`${active ? "active" : ""} ${danger ? "danger" : ""}`} onClick={onClick}><span className="permission-icon">{icon}</span><span><b>{title}</b><small>{description}</small></span>{active && <Check size={15} />}</button>;
}
function PreferenceToggle({ active, disabled, title, description, onClick }: { active: boolean; disabled: boolean; title: string; description: string; onClick: () => void }) {
  return <button className={`agent-control-row preference-toggle ${active ? "active" : ""}`} role="menuitemcheckbox" aria-checked={active} disabled={disabled} onClick={onClick}><span><b>{title}</b><small>{description}</small></span><i className={`control-switch ${active ? "is-on" : ""}`} aria-hidden="true" /></button>;
}
function FileTree({ locale }: { locale: Locale }) { const zh = locale === "zh-CN"; return <div className="file-tree"><div className="context-heading"><b>{zh ? "项目文件" : "Project files"}</b><small>{zh ? "选择性同步" : "Selective sync"}</small></div><div className="tree-folder"><Folder size={15} />data <span>{zh ? "远端" : "remote"}</span></div><div className="tree-folder"><Folder size={15} />analysis</div><div className="tree-file"><FileBarChart size={15} />umap.png <em>1.2 MB</em></div><div className="tree-file"><FileText size={15} />markers.csv <em>84 KB</em></div><div className="tree-file"><NotebookPen size={15} />report.md <em>12 KB</em></div></div>; }
function V4RunTrace({ locale, events, previewText, reasoningPreview, modelActivity, onAnswer, onDecideApproval, onResolveUncertain, onResume, onCancelRecovery, onCloseBrowserTabs, historical = false }: { locale: Locale; events: AgentRunEventV4[]; previewText?: string | null; reasoningPreview?: import("../../types").AgentReasoningPreviewV4 | null; modelActivity?: import("../../types").AgentModelActivityReceiptV4 | null; onAnswer?: (runId: string, questionId: string, answer: string) => Promise<void> | void; onDecideApproval?: (runId: string, approvalId: string, callHash: string, decision: "approved" | "denied", browserScope?: BrowserApprovalScopeV4) => Promise<void> | void; onResolveUncertain?: (runId: string, callId: string, resolution: "side_effect_observed" | "side_effect_not_observed" | "compensated", evidence: string) => Promise<void> | void; onResume?: (runId: string) => Promise<void> | void; onCancelRecovery?: (runId: string) => Promise<void> | void; onCloseBrowserTabs?: (runId: string, sessions: Array<"shared" | "workspace">) => Promise<void> | void; historical?: boolean }) {
  const zh = locale === "zh-CN";
  const [resumeBusy, setResumeBusy] = useState(false);
  const resumeBusyRef = useRef(false);
  const [cancelRecoveryBusy, setCancelRecoveryBusy] = useState(false);
  const [recoveryError, setRecoveryError] = useState("");
  const entries = coalesceV4ModelText(events);
  const progress = entries.filter(({ modelText }) => modelText !== undefined && modelText.trim());
  const technicalEntries = entries.filter(({ event, modelText }) => modelText === undefined && isConversationEvent(event)
    && event.event.kind !== "tool_approval_decided"
    && !(event.event.kind === "tool_approval_requested" && isV4ApprovalDecided(events, event.event.request.approval_id))
    && !(event.event.kind === "user_input_answered" && events.some((item) => item.event.kind === "input_requested" && event.event.kind === "user_input_answered" && item.event.question_id === event.event.question_id)));
  const tools = mergeV4ToolCalls(events);
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
  const pendingModel = !historical && !terminal && !pauseReason ? pendingModelRequest(events) : null;
  const activeTool = !historical && !terminal && !pauseReason
    ? tools.filter((tool) => tool.status === "requested" || tool.status === "running").at(-1) : null;
  const reasoningText = pendingModel && reasoningPreview?.run_id === events[0]?.run_id
    && reasoningPreview.attempt_id === pendingModel.attemptId ? reasoningPreview.text : null;
  const modelRequests = events.filter((item) => item.event.kind === "model_request_started");
  const [modelWaitNow, setModelWaitNow] = useState(() => Date.now());
  const runClockActive = !historical && !terminal && !pauseReason;
  useEffect(() => {
    setModelWaitNow(Date.now());
    if (!runClockActive) return;
    const timer = window.setInterval(() => setModelWaitNow(Date.now()), 1_000);
    return () => window.clearInterval(timer);
  }, [events[0]?.run_id, runClockActive]);
  const modelWaitDuration = pendingModel?.startedAtMs === null || pendingModel === null
    ? ""
    : formatDuration(Math.max(0, modelWaitNow - pendingModel.startedAtMs));
  const toolStartedAtMs = activeTool?.startedAt ? Date.parse(activeTool.startedAt) : Number.NaN;
  const toolWaitDuration = Number.isFinite(toolStartedAtMs)
    ? formatDuration(Math.max(0, modelWaitNow - toolStartedAtMs)) : "";
  const recentActivity = pendingModel && modelActivity?.attempt_id === pendingModel.attemptId
    && modelWaitNow - Date.parse(modelActivity.received_at) < 15_000 ? modelActivity.phase : null;
  const runStartedAtMs = Date.parse(events[0]?.occurred_at ?? "");
  const runEndedAtMs = runClockActive ? modelWaitNow : Date.parse((terminal ?? latest)?.occurred_at ?? "");
  const duration = Number.isFinite(runStartedAtMs) && Number.isFinite(runEndedAtMs)
    ? formatDuration(Math.max(0, runEndedAtMs - runStartedAtMs)) : "";
  const unresolvedDispatchFailure = (!terminal || terminal.event.kind === "run_needs_attention" || terminal.event.kind === "run_failed") && events.some(({ event }) => event.kind === "tool_dispatch_uncertain" && !isV4UncertainResolved(events, event.call_id));
  const status = unresolvedDispatchFailure ? (zh ? "失败" : "Failed") : terminal?.event.kind === "run_completed" ? (zh ? "已完成" : "Completed") : terminal?.event.kind === "run_cancelled" ? (zh ? "已终止" : "Cancelled") : terminal?.event.kind === "run_needs_attention" ? (zh ? "需要处理" : "Needs attention") : terminal?.event.kind === "run_failed" ? (zh ? "失败" : "Failed") : pauseReason === "approval" ? (zh ? "等待工具审批" : "Waiting for approval") : pauseReason === "input" ? (zh ? "等待回答" : "Waiting for input") : pauseReason === "browser_connection" ? (zh ? "等待连接浏览器" : "Waiting for browser") : pauseReason === "browser_human" ? (zh ? "等待人工处理浏览器" : "Waiting for browser intervention") : pauseReason === "runtime_recovery" ? (zh ? "结果待恢复" : "Results ready to resume") : pendingModel && previewText ? (zh ? "正在生成回复" : "Generating response") : recentActivity === "reasoning" ? (zh ? "模型正在推理" : "Model reasoning") : recentActivity === "tool_call" ? (zh ? "模型正在准备工具" : "Preparing tools") : recentActivity === "retrying" ? (zh ? "模型正在重试" : "Retrying model") : pendingModel && reasoningText ? (zh ? "等待模型继续输出" : "Waiting for more model output") : pendingModel ? (zh ? "等待模型响应" : "Waiting for model") : (zh ? "运行中" : "Running");
  const shouldExpand = !historical && !terminal;
  async function resumeRun() {
    if (!onResume || !events[0] || resumeBusyRef.current) return;
    resumeBusyRef.current = true;
    setResumeBusy(true);
    setRecoveryError("");
    try {
      await onResume(events[0].run_id);
    } finally {
      resumeBusyRef.current = false;
      setResumeBusy(false);
    }
  }
  async function cancelRecovery() {
    if (!onCancelRecovery || !events[0] || resumeBusyRef.current) return;
    resumeBusyRef.current = true;
    setResumeBusy(true);
    setCancelRecoveryBusy(true);
    setRecoveryError("");
    try { await onCancelRecovery(events[0].run_id); }
    catch { setRecoveryError(zh ? "取消失败，请重试。" : "Cancellation failed. Retry."); }
    finally { resumeBusyRef.current = false; setResumeBusy(false); setCancelRecoveryBusy(false); }
  }
  return <>
    {completionPending && <div className="agent-completion-pending" role="status"><span className="agent-working"><i />{zh ? "正在核验最终结果…" : "Verifying the final result…"}</span></div>}
    <section className="v4-conversation-run" aria-label={zh ? "分析对话" : "Analysis conversation"}>
    {pendingModel && <div className="v4-live-activity" role="status" aria-label={zh ? "当前模型活动" : "Current model activity"}><span className="agent-working"><i />{status}</span><small role="timer" aria-live="off">{zh ? "本次请求" : "This request"} {modelWaitDuration}</small></div>}
    {!pendingModel && activeTool && <div className="v4-live-activity" role="status" aria-label={zh ? "当前工具活动" : "Current tool activity"}><span className="agent-working"><i />{zh ? "正在运行工具" : "Running tool"} · {activeTool.toolId === "use_skill" ? skillDisplayName(activeTool, zh) : compactToolLabel(activeTool.toolId)}</span>{toolWaitDuration && <small role="timer" aria-live="off">{toolWaitDuration}</small>}</div>}
    <details className={`agent-run-fold agent-v4-run ${historical ? "" : "is-active"}`} open={shouldExpand}>
      <summary><span className="agent-run-fold-title"><span className="v4-process-mark" aria-hidden="true" /><span><b>{zh ? "执行过程" : terminal ? "Processed" : "Processing"}</b><small>{terminal ? (zh ? "工具调用与验证记录" : "Tool calls and verification") : (zh ? "Agent 正在处理任务" : "Agent is working")}</small></span></span><span>{status} · {tools.length} {zh ? "个步骤" : tools.length === 1 ? "step" : "steps"}{duration && ` · ${duration}`}</span></summary>
      <div className="agent-run-fold-body">

      <section className="v4-process-timeline" aria-label={zh ? "工具调用详情" : "Tool call details"}><ActivityWindow zh={zh}>
      {[
        ...modelRequests.map((item) => ({ sequence: item.sequence, node: reasoningText && item.event.kind === "model_request_started" && item.event.request.attempt_id === pendingModel?.attemptId
          ? <ReasoningPreview key={`request-${item.sequence}`} zh={zh} text={reasoningText} />
          : <div className="v4-model-request-row v4-timeline-row" key={`request-${item.sequence}`}><span className="v4-row-mark" aria-hidden="true">○</span><strong>MODEL REQUEST</strong><small>{zh ? "请求已发送" : "Request sent"}</small></div> })),
        ...progress.map(({ event, modelText }) => ({ sequence: event.sequence, node: <PublicProgress key={`progress-${event.sequence}`} markdown={modelText ?? ""} zh={zh} /> })),
        ...tools.map((tool) => ({ sequence: tool.firstSequence, node: <details className="v4-tool-trace v4-timeline-row" key={tool.callId}>
          <summary className="v4-tool-trace-heading"><span className={`v4-tool-mark ${tool.status}`} aria-label={toolStatusLabel(tool.status, zh)}>{tool.status === "failed" ? "×" : tool.status === "succeeded" || tool.status === "reused" ? "✓" : "○"}</span>{tool.toolId === "use_skill" && <span className="v4-skill-badge">SKILL</span>}<strong title={toolDisplayLabel(tool.toolId, zh)}>{tool.toolId === "use_skill" ? skillDisplayName(tool, zh) : compactToolLabel(tool.toolId)}</strong>{tool.subject && tool.toolId !== "use_skill" && <span className="v4-tool-subject" title={tool.subject}>{Array.from(tool.subject).slice(0, 80).join("")}{Array.from(tool.subject).length > 80 ? "…" : ""}</span>}<small className="v4-tool-metrics">{toolMetrics(tool, zh, runClockActive ? modelWaitNow : null)}</small><ChevronRight size={13} /></summary>
          <div className="v4-tool-trace-body">
            <small>{tool.toolId} · #{tool.firstSequence}–#{tool.lastSequence}</small>
            {tool.argumentsPreview && <><b>{zh ? "输入" : "Input"}</b><code>{tool.argumentsPreview}</code></>}
            {tool.outcome !== undefined && <><b>{zh ? "结果" : "Result"}</b><pre>{tool.outcome}</pre></>}
          </div>
        </details> })),
      ].sort((a, b) => a.sequence - b.sequence).map(({ node }) => node)}</ActivityWindow>

      </section>
        <details className="v4-process-context"><summary>{zh ? "诊断记录" : "Diagnostic records"}</summary><ol className="v4-diagnostic-events">{events.filter(({ event }) => event.kind !== "model_text").map((event) => <li key={event.sequence}><span>#{event.sequence}</span><code>{event.event.kind}</code><time dateTime={event.occurred_at}>{new Date(event.occurred_at).toLocaleTimeString()}</time></li>)}</ol></details>
      </div>
    </details>
      {!terminal && previewText && <article aria-label={zh ? "模型实时输出" : "Live model output"} className="message assistant-message v4-live-response"><div><small className="response-eyebrow">{zh ? "正在回复" : "Responding"}</small><MarkdownContent markdown={previewText} /><span className="v4-streaming-cursor" aria-hidden="true">▍</span></div></article>}
      {technicalEntries.map(({ event }) => (<article className={`message assistant-message agent-work-update ${event.event.kind === "input_requested" ? "v4-input-decision-entry" : ""}`} key={`${event.run_id}-${event.sequence}`}>
        <div><div className="agent-work-heading"><strong>{v4EventLabel(event, zh)}</strong></div>
          {v4EventContent(event, zh) && <MarkdownContent markdown={v4EventContent(event, zh)} />}
          {event.event.kind === "input_requested" && isV4QuestionAnswered(events, event.event.question_id) && <footer className="v4-decision-sent">{zh ? "回答已发送给 Agent" : "Answer sent to the agent"}{events.filter((answer) => answer.event.kind === "user_input_answered" && event.event.kind === "input_requested" && answer.event.question_id === event.event.question_id).map((answer) => <div key={answer.sequence}>{v4EventContent(answer, zh)}</div>)}</footer>}
          {event.event.kind === "input_requested" && onAnswer && !isV4QuestionAnswered(events, event.event.question_id) && <V4AnswerForm locale={locale} onSubmit={(answer) => onAnswer(event.run_id, event.event.kind === "input_requested" ? event.event.question_id : "", answer)} />}
          {event.event.kind === "tool_approval_requested" && onDecideApproval && !isV4ApprovalDecided(events, event.event.request.approval_id) && <V4ApprovalCard locale={locale} request={event.event.request} onDecide={(decision, scope) => {
            if (event.event.kind !== "tool_approval_requested") return;
            const request = event.event.request;
            return scope === undefined
              ? onDecideApproval(event.run_id, request.approval_id, request.call_hash, decision)
              : onDecideApproval(event.run_id, request.approval_id, request.call_hash, decision, scope);
          }} />}
          {event.event.kind === "browser_tab_cleanup_required" && onCloseBrowserTabs && <BrowserTabCleanupCard locale={locale} tabs={event.event.tabs} onClose={() => onCloseBrowserTabs(event.run_id, event.event.kind === "browser_tab_cleanup_required" ? event.event.sessions : [])} />}
        </div>
      </article>))}

      {!terminal && pauseReason === "runtime_recovery" && (onResume || onCancelRecovery) && <div className="v4-resume-run"><span>{zh ? "计算结果已保存，可继续核验。" : "Saved computation results are ready for verification."}</span>{onResume && <button disabled={resumeBusy} onClick={() => void resumeRun()}>{resumeBusy && !cancelRecoveryBusy ? (zh ? "恢复中…" : "Resuming…") : (zh ? "恢复已保存结果" : "Resume saved results")}</button>}{onCancelRecovery && <button disabled={resumeBusy} onClick={() => void cancelRecovery()}>{cancelRecoveryBusy ? (zh ? "取消中…" : "Cancelling…") : (zh ? "取消此运行" : "Cancel this run")}</button>}{recoveryError && <span role="alert">{recoveryError}</span>}</div>}
      {pauseReason === "browser_connection" && onResume && <div className="v4-resume-run"><span>{zh ? "请在设置 → Browser 安装或启用 OmicsOps 扩展并连接相应会话，然后原地继续此任务。" : "Open Settings → Browser, install or enable the OmicsOps extension, connect the requested session, then resume this same task."}</span><button disabled={resumeBusy} onClick={() => void resumeRun()}>{resumeBusy ? (zh ? "恢复中…" : "Resuming…") : (zh ? "已连接，继续" : "Connected, resume")}</button></div>}
      {pauseReason === "browser_human" && onResume && <div className="v4-resume-run"><span>{zh ? "请在真实浏览器中完成人机验证或其他人工步骤；OmicsOps 不会自动求解 CAPTCHA。处理完成后原地继续。" : "Complete the CAPTCHA or other manual step in the real browser. OmicsOps never solves CAPTCHA automatically; resume this same run when finished."}</span><button disabled={resumeBusy} onClick={() => void resumeRun()}>{resumeBusy ? (zh ? "恢复中…" : "Resuming…") : (zh ? "已人工处理，继续" : "Handled, resume")}</button></div>}

    </section>
  </>;
}
function ReasoningPreview({ zh, text }: { zh: boolean; text: string }) {
  const contentRef = useRef<HTMLPreElement>(null);
  const followingRef = useRef(true);
  useEffect(() => {
    const content = contentRef.current;
    if (content && followingRef.current) content.scrollTop = content.scrollHeight;
  }, [text]);
  return <article aria-label={zh ? "模型思考" : "Model thinking"} className="v4-reasoning-preview v4-timeline-row"><details><summary><span className="v4-row-mark" aria-hidden="true">○</span><strong>THINKING</strong><ChevronRight size={14} /></summary><pre ref={contentRef} onScroll={(event) => { const content = event.currentTarget; followingRef.current = content.scrollHeight - content.scrollTop - content.clientHeight < 32; }}>{text}</pre></details></article>;
}
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
function isV4ApprovalDecided(events: AgentRunEventV4[], approvalId: string) {
  return events.some((event) => event.event.kind === "tool_approval_decided" && event.event.approval_id === approvalId);
}
function isV4UncertainResolved(events: AgentRunEventV4[], callId: string) {
  return events.some((event) => event.event.kind === "tool_dispatch_resolved" && event.event.call_id === callId);
}
function isV4QuestionAnswered(events: AgentRunEventV4[], questionId: string) {
  return events.some((event) => event.event.kind === "user_input_answered" && event.event.question_id === questionId);
}
type MessageSelectionScope = { messageId: string; role: "user" | "assistant"; projectId: string; conversationId: string };

function ResponseBody({ markdown, zh, createdAt, selectionScope }: { markdown: string; zh: boolean; createdAt?: string | null; selectionScope?: MessageSelectionScope }) {
  const [copied, setCopied] = useState<string | null>(null);
  const [copyFailed, setCopyFailed] = useState(false);
  async function copyResponse() {
    setCopyFailed(false);
    try { await navigator.clipboard.writeText(markdown); setCopied(markdown); }
    catch { setCopied(null); setCopyFailed(true); }
  }
  const label = copied === markdown ? (zh ? "已复制" : "Copied") : (zh ? "复制回复" : "Copy response");
  const timestamp = createdAt && Number.isFinite(Date.parse(createdAt)) ? new Date(createdAt) : null;
  const displayTime = timestamp ? `${String(timestamp.getMonth() + 1).padStart(2, "0")}-${String(timestamp.getDate()).padStart(2, "0")} ${String(timestamp.getHours()).padStart(2, "0")}:${String(timestamp.getMinutes()).padStart(2, "0")}` : null;
  return <div className="response-body">
    <header className="response-identity"><Bot size={15} aria-hidden="true" /><strong>OMICSOPS</strong>{displayTime && <time dateTime={createdAt!}>{displayTime}</time>}</header>
    <MarkdownContent markdown={markdown} selectionScope={selectionScope} />
    <footer className="response-actions"><button type="button" aria-label={label} title={label} onClick={() => void copyResponse()}>{copied === markdown ? <Check size={14} /> : <Copy size={14} />}</button>
      {copyFailed && <span role="status">{zh ? "复制失败，请手动选择正文。" : "Copy failed. Select the response manually."}</span>}
    </footer>
  </div>;
}
function ActivityWindow({ children, zh }: { children: ReactNode[]; zh: boolean }) {
  const earlierCount = Math.max(0, children.length - 6);
  return <>
    {earlierCount > 0 && <details className="v4-earlier-activity">
      <summary>{zh ? `较早活动 · ${earlierCount} 项` : `Earlier activity · ${earlierCount} items`}</summary>
      {children.slice(0, earlierCount)}
    </details>}
    {children.slice(earlierCount)}
  </>;
}
function PublicProgress({ markdown, zh }: { markdown: string; zh: boolean }) {
  const [open, setOpen] = useState(false);
  const firstLine = markdown.split(/\r?\n/).find((line) => line.trim())?.trim() ?? "";
  const summary = Array.from(firstLine);
  const preview = summary.slice(0, 100).join("") + (summary.length > 100 ? "…" : "");
  return <article aria-label={zh ? "模型输出" : "Model output"} className="v4-progress-disclosure v4-timeline-row">
    <button type="button" className="v4-progress-heading" aria-expanded={open} onClick={() => setOpen((value) => !value)}>
      <span className="v4-row-mark" aria-hidden="true">−</span><strong>PROGRESS</strong>{!open && <span className="v4-progress-preview">{preview}</span>}<ChevronRight size={13} />
    </button>
    {open && <MarkdownContent markdown={markdown} />}
  </article>;
}
function MarkdownContent({ markdown, selectionScope }: { markdown: string; selectionScope?: MessageSelectionScope }) {
  return <div className="markdown-content"
    data-message-selection-body={selectionScope ? true : undefined}
    data-message-id={selectionScope?.messageId}
    data-message-role={selectionScope?.role}
    data-project-id={selectionScope?.projectId}
    data-conversation-id={selectionScope?.conversationId}
  ><ReactMarkdown remarkPlugins={[remarkGfm]} components={{ table: ({ children }) => <div className="markdown-table-scroll" role="region" aria-label="表格 / Table" tabIndex={0}><table>{children}</table></div> }}>{markdown}</ReactMarkdown></div>;
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
      byCall.set(callId, { callId, toolId, argumentsPreview: args ? JSON.stringify(redactToolArguments(args), null, 2) : "", firstSequence: sequence, lastSequence: sequence, outcome, subject: toolSubject(args ? redactToolArguments(args) : undefined), status: status ?? "requested" });
      return;
    }
    current.lastSequence = Math.max(current.lastSequence, sequence);
    if (toolId) current.toolId = toolId;
    if (args) {
      current.argumentsPreview = JSON.stringify(redactToolArguments(args), null, 2);
      current.subject = toolSubject(redactToolArguments(args));
    }
    if (outcome !== undefined) current.outcome = outcome;
    if (status) current.status = status;
  };
  for (const item of events) {
    const event = item.event;
    if (event.kind === "tool_requested") update(event.call.call_id, item.sequence, event.call.tool_id, event.call.arguments, "requested");
    else if (event.kind === "tool_dispatch_started") update(event.call_id, item.sequence, event.tool_id, undefined, "running");
    else if (event.kind === "tool_dispatch_uncertain") update(event.call_id, item.sequence, event.tool_id, undefined, "failed", "Tool call failed; see diagnostic details. Automatic retry disabled.");
    else if (event.kind === "tool_finished") update(event.outcome.call_id, item.sequence, event.outcome.tool_id, undefined, event.outcome.succeeded ? "succeeded" : "failed", formatToolOutput(event.outcome.model_content));
    else if (event.kind === "tool_outcome_reused") update(event.outcome.call_id, item.sequence, event.outcome.tool_id, undefined, "reused", formatToolOutput(event.outcome.model_content));
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
function formatToolOutput(content: string) {
  try {
    const value: unknown = JSON.parse(content);
    return value !== null && typeof value === "object" ? JSON.stringify(value, null, 2) : content;
  } catch { return content; }
}
function isInternalAgentTool(toolId: string) {
  return toolId === "agent.complete" || toolId === "agent.request_input" || toolId === "agent.propose_plan" || toolId === "agent.update_tasks" || toolId === "agent.route" || toolId === "agent.route_request";
}
function toolSubject(value: unknown): string | undefined {
  if (!value || typeof value !== "object" || Array.isArray(value)) return undefined;
  const args = value as Record<string, unknown>;
  if (typeof args.tool === "string" && args.tool.trim()) {
    const nested = args.arguments;
    const query = nested && typeof nested === "object" && !Array.isArray(nested)
      ? toolSubject(nested) : undefined;
    return query ? `${args.tool.trim()} · ${query}` : args.tool.trim();
  }
  for (const key of ["path", "file_path", "relative_path", "command", "cmd", "skill_name", "skill_id", "name", "artifact_id", "query"]) {
    const value = args[key];
    if (typeof value === "string" && value.trim()) return value.trim();
  }
  return undefined;
}
function skillDisplayName(tool: MergedV4ToolCall, zh: boolean): string {
  const fallback = zh ? "加载技能" : "Load skill";
  try {
    const args = JSON.parse(tool.argumentsPreview || "{}");
    if (tool.status === "succeeded" || tool.status === "reused") {
      try {
        const frozen = JSON.parse(tool.outcome ?? "null");
        if (frozen && typeof frozen.skill_id === "string" && frozen.skill_id === args.skill_id && typeof frozen.name === "string" && frozen.name.trim()) return frozen.name.trim();
      } catch { /* Legacy non-JSON outcomes keep the request label. */ }
    }
    const name = args.skill_name ?? args.name;
    return typeof name === "string" && name.trim() ? name.trim() : fallback;
  } catch { return fallback; }
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
function toolMetrics(tool: MergedV4ToolCall, zh: boolean, now: number | null) {
  const end = tool.finishedAt ?? (now !== null && (tool.status === "requested" || tool.status === "running") ? new Date(now).toISOString() : undefined);
  const duration = tool.status === "reused" ? "" : eventDuration(tool.startedAt, end);
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
function isConversationEvent(event: AgentRunEventV4) {
  // Only user-facing decisions and problems belong in the conversation. New
  // runtime events stay in diagnostics until they have an explicit presentation.
  return event.event.kind === "input_requested"
    || event.event.kind === "user_input_answered"
    || event.event.kind === "tool_approval_requested"
    || event.event.kind === "tool_approval_decided"
    || event.event.kind === "tool_dispatch_uncertain"
    || event.event.kind === "tool_dispatch_resolved"
    || event.event.kind === "browser_connection_required"
    || event.event.kind === "browser_human_intervention_required"
    || event.event.kind === "browser_tab_cleanup_required"
    || event.event.kind === "run_failed"
    || event.event.kind === "run_needs_attention";
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
function v4EventLabel(event: AgentRunEventV4, zh: boolean) { if (event.event.kind === "tool_dispatch_uncertain") return zh ? "工具调用失败" : "Tool call failed"; if (event.event.kind === "run_created") return event.event.mode === "execute" ? (zh ? "任务启动" : "Task started") : (zh ? "规划启动" : "Planning started"); const labels: Record<string, string> = { request_routed: zh ? "请求已分类" : "Request routed", browser_connection_required: zh ? "需要连接浏览器" : "Browser connection required", browser_human_intervention_required: zh ? "浏览器需要人工处理" : "Browser intervention required", browser_tab_cleanup_required: zh ? "浏览器标签待清理" : "Browser tabs need cleanup", plan_proposed: zh ? "计划已冻结" : "Plan frozen", plan_approved: zh ? "计划获批" : "Plan approved", mode_changed: zh ? "执行模式" : "Execution mode", tool_requested: zh ? "工具请求" : "Tool request", tool_approval_requested: zh ? "等待工具审批" : "Tool approval required", tool_approval_decided: zh ? "工具审批已决定" : "Tool approval decided", tool_dispatch_uncertain: zh ? "工具调用失败" : "Tool call failed", tool_dispatch_resolved: zh ? "不确定状态已核实" : "Uncertain dispatch resolved", tool_finished: zh ? "工具结果" : "Tool result", input_requested: zh ? "需要补充信息" : "Input required", user_input_answered: zh ? "用户已回答" : "User answered", completion_proposed: zh ? "完成提案" : "Completion proposed", run_completed: zh ? "运行完成" : "Run completed", run_needs_attention: zh ? "需要处理" : "Needs attention", run_failed: zh ? "运行失败" : "Run failed", run_cancelled: zh ? "运行取消" : "Run cancelled" }; return labels[event.event.kind] ?? event.event.kind; }
function v4EventContent(event: AgentRunEventV4, zh: boolean) { if (event.event.kind === "tool_dispatch_uncertain") return zh ? `${event.event.tool_id} 调用未正常完成，请查看失败详情。此记录不会自动重试。` : `${event.event.tool_id} did not complete normally. See the failure details. This call will not be retried automatically.`; if (event.event.kind === "request_routed") return event.event.route === "research_retrieval" ? (zh ? "科研检索流水线" : "Research retrieval workflow") : (zh ? "自适应执行" : "Adaptive execution"); if (event.event.kind === "browser_connection_required" || event.event.kind === "browser_human_intervention_required" || event.event.kind === "browser_tab_cleanup_required") return event.event.message; if (event.event.kind === "tool_requested") return event.event.call.tool_id; if (event.event.kind === "tool_approval_requested") return `${event.event.request.call.tool_id}: ${event.event.request.reason}`; if (event.event.kind === "tool_approval_decided") return event.event.decision === "approved" ? (zh ? "用户已批准" : "Approved by user") : (zh ? "用户已拒绝" : "Denied by user"); if (event.event.kind === "tool_dispatch_resolved") return event.event.evidence; if (event.event.kind === "tool_finished") return event.event.outcome.model_content; if (event.event.kind === "plan_proposed") return `${event.event.plan.steps.length} ${zh ? "个步骤" : "steps"} · SHA-256 ${event.event.plan_hash.slice(0, 12)}`; if (event.event.kind === "model_text") return event.event.text; if (event.event.kind === "input_requested") return event.event.question; if (event.event.kind === "user_input_answered") return zh ? `已提交回答：${event.event.answer}` : `Answer submitted: ${event.event.answer}`; if (event.event.kind === "run_failed" || event.event.kind === "run_needs_attention") return event.event.message; return ""; }
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

function pendingModelRequest(events: AgentRunEventV4[]): { attemptId: string; startedAtMs: number | null } | null {
  let pending: { attemptId: string; startedAtMs: number | null } | null = null;
  for (const item of events) {
    const event = item.event;
    if (event.kind === "model_request_started") {
      const startedAtMs = Date.parse(item.occurred_at);
      pending = {
        attemptId: event.request.attempt_id,
        startedAtMs: Number.isFinite(startedAtMs) ? startedAtMs : null,
      };
      continue;
    }
    if (event.kind === "model_usage_observed") {
      if (pending && event.observation.attempt_id === pending.attemptId
        && (event.observation.state === "final" || event.observation.state === "interrupted")) pending = null;
      continue;
    }
    if (pending && (event.kind === "model_text" || event.kind === "cycle_finished"
      || event.kind === "tool_requested" || event.kind === "plan_proposed"
      || event.kind === "tool_approval_requested" || event.kind === "input_requested"
      || event.kind === "browser_connection_required" || event.kind === "browser_human_intervention_required"
      || event.kind === "runtime_recovery_available"
      || isTerminalAgentEventV4(item))) pending = null;
  }
  return pending;
}

function getV4PauseReason(events: AgentRunEventV4[]): "approval" | "input" | "browser_connection" | "browser_human" | "uncertain" | "runtime_recovery" | null {
  for (let index = events.length - 1; index >= 0; index -= 1) {
    const event = events[index].event;
    if (event.kind === "runtime_recovery_available") return event.call_ids.some((id) => !events.slice(index + 1).some((item) => item.event.kind === "tool_finished" && item.event.outcome.call_id === id)) ? "runtime_recovery" : null;
    if (event.kind === "tool_approval_requested") return isV4ApprovalDecided(events, event.request.approval_id) ? null : "approval";
    if (event.kind === "input_requested") return isV4QuestionAnswered(events, event.question_id) ? null : "input";
    if (event.kind === "browser_connection_required") return "browser_connection";
    if (event.kind === "browser_human_intervention_required") return "browser_human";
    if (event.kind === "tool_finished" && event.outcome.succeeded
      && (event.outcome.tool_id.startsWith("browser_") || event.outcome.tool_id.startsWith("web_"))) return null;
    if (event.kind === "tool_dispatch_uncertain") return null;
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

function Notebook({ locale, entries, artifacts, facts, onSearch, onExport }: { locale: Locale; entries: NotebookEntry[]; artifacts: ProjectArtifact[]; facts: MemoryFact[]; onSearch?: (query: string, dimension?: string) => Promise<void> | void; onExport?: (format: "markdown" | "json" | "bundle") => Promise<void> | void }) { const zh = locale === "zh-CN"; const [query, setQuery] = useState(""); const [dimension, setDimension] = useState(""); return <div className="notebook-panel"><div className="notebook-actions"><input aria-label={zh ? "检索记忆" : "Search memory"} value={query} onChange={(event) => setQuery(event.target.value)} placeholder={zh ? "检索已保存的记忆文件" : "Search saved memory files"} /><select aria-label={zh ? "记忆维度" : "Memory dimension"} value={dimension} onChange={(event) => setDimension(event.target.value)}><option value="">{zh ? "全部事实" : "All facts"}</option><option value="memory">{zh ? "记忆文件" : "Memory files"}</option></select><button onClick={() => void onSearch?.(query, dimension || undefined)}>{zh ? "检索" : "Search"}</button></div><div className="notebook-export"><button onClick={() => void onExport?.("markdown")}>Markdown</button><button onClick={() => void onExport?.("json")}>JSON</button><button onClick={() => void onExport?.("bundle")}>{zh ? "项目包" : "Bundle"}</button></div><div className="notebook-list">{entries.length === 0 && <div><span>{zh ? "研究记录" : "Notebook"}</span><b>{zh ? "暂无正式条目" : "No formal entries yet"}</b><p>{zh ? "Agent 完成并验证产物后会自动登记目标、方法、观察、决策与证据。" : "Verified Agent runs automatically register goals, methods, observations, decisions, and evidence."}</p></div>}{entries.map((entry) => <div key={entry.id}><span>{entry.kind}</span><b>{entry.title}</b><p>{entry.markdown}</p><small>{entry.evidence_ids.length} {zh ? "条可追溯引用" : "traceable references"}</small></div>)}</div><div className="artifact-register"><b>{zh ? "已登记产物" : "Registered artifacts"} · {artifacts.length}</b>{artifacts.map((artifact) => <small key={artifact.id}>{artifact.relative_path} · SHA-256 {artifact.sha256.slice(0, 12)}</small>)}</div><div className="memory-results"><b>{zh ? "事实记忆" : "Fact memory"} · {facts.length}</b>{facts.slice(0, 20).map((fact) => <article className={fact.conflicted_with.length ? "conflicted" : ""} key={fact.id}><span>{fact.dimension}</span><p>{fact.statement}</p><small>{fact.evidence.map((evidence) => `${evidence.source_kind}:${evidence.source_id}`).join(" · ")}</small>{fact.conflicted_with.length > 0 && <em>{zh ? "存在冲突事实，已保留双方来源" : "Conflicting fact retained with both sources"}</em>}</article>)}</div></div>; }
function RunSummary({ locale }: { locale: Locale }) { const zh = locale === "zh-CN"; return <div className="run-summary"><Activity size={24} /><b>{zh ? "运行中" : "Running"}</b><span>3 / 5 {zh ? "步骤已验证" : "steps verified"}</span><div className="task-progress"><i style={{ width: "60%" }} /></div></div>; }
