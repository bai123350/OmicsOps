import { useConversationBranchSend } from "./features/workspace/useConversationBranchSend";
import { useContextUsage } from "./features/workspace/useContextUsage";
import { useSideChat } from "./features/workspace/useSideChat";
import { useWorkspaceSearch } from "./use-workspace-search";
import { validateComposerReferences } from "./composer-reference-api";
import { validateComposerAttachments } from "./composer-attachment-api";
import { canAttachSearchEntry, type WorkspaceSearchEntry, type WorkspaceSearchRequest } from "./workspace-search";
import { WorkspaceSearchDialog } from "./features/workspace/WorkspaceSearchDialog";
import { referenceKey } from "./features/workspace/ComposerReferences";
import type { ComposerReference } from "./types";
import { useEffect, useRef, useState } from "react";
import * as api from "./tauri-api";
import type { AgentRunEventV4, ApprovalPolicyV4, AutonomyModeV4, ComputeBackendAvailabilityV4, ComputeSelectionV4, ConnectionProfile, ConversationAgentStateV4, KernelEvent, KernelLanguage, KernelSession, McpServerProfile, MemoryFact, ModelProfile, NotebookEntry, ProjectArtifact, ProposedPlanRevisionV4, RemoteFileEntry, RunSummaryV4, SessionAgentModeV4, SkillPackage, SyncEntry, WorkspaceConversation, WorkspaceMessage, WorkspaceProject } from "./types";
import { ProjectLibrary } from "./features/projects/ProjectLibrary";
import { WorkspaceShell } from "./features/workspace/WorkspaceShell";
import { ApiModelPicker } from "./features/workspace/ApiModelPicker";
import { SettingsPanel, type SettingsSection } from "./features/settings/SettingsPanel";
import { usePersistentLocale } from "./use-persistent-locale";
import { useConversationCapabilities } from "./features/workspace/useConversationCapabilities";
import { useComposerQueue } from "./features/workspace/useComposerQueue";
import { useComposerReplacement } from "./features/workspace/useComposerReplacement";
import { useAgentStop } from "./features/workspace/useAgentStop";
import { useResumeLastSessionPreference } from "./features/settings/useResumeLastSessionPreference";
import { useRunNotifications } from "./use-run-notifications";

type SendMode = "chat" | "plan";

interface PendingSubmission {
  projectId: string;
  conversationId: string;
  markdown: string;
  mode: SendMode;
  references: ComposerReference[];
  attachments: string[];
  message: WorkspaceMessage;
}

export function samePendingSubmission(
  pending: PendingSubmission,
  projectId: string,
  conversationId: string,
  markdown: string,
  mode: SendMode,
  references: ComposerReference[],
  attachments: string[],
) {
  const pendingReferenceKeys = pending.references.map(referenceKey);
  const referenceKeys = references.map(referenceKey);
  return pending.projectId === projectId
    && pending.conversationId === conversationId
    && pending.markdown === markdown
    && pending.mode === mode
    && JSON.stringify(pendingReferenceKeys) === JSON.stringify(referenceKeys)
    && JSON.stringify(pending.attachments) === JSON.stringify(attachments);
}

export default function DesktopApp() {
  const [projects, setProjects] = useState<WorkspaceProject[]>([]);
  const [selected, setSelected] = useState<WorkspaceProject | null>(null);
  const [searchOpen, setSearchOpen] = useState(false);
  const searchOpenRef = useRef(searchOpen);
  searchOpenRef.current = searchOpen;
  const searchActionGeneration = useRef(0);
  const requestedConversation = useRef<{ projectId: string; conversationId: string } | null>(null);
  const [searchRequest, setSearchRequest] = useState<WorkspaceSearchRequest | null>(null);
  const workspaceSearch = useWorkspaceSearch(searchOpen, projects);
  function openWorkspaceSearch() {
    if (!searchOpenRef.current) searchActionGeneration.current += 1;
    setSearchOpen(true);
  }
  function closeWorkspaceSearch() {
    searchActionGeneration.current += 1;
    setSearchOpen(false);
  }
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.defaultPrevented || event.isComposing || event.keyCode === 229 || event.altKey || !(event.ctrlKey || event.metaKey) || event.key.toLowerCase() !== "k") return;
      event.preventDefault();
      openWorkspaceSearch();
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, []);

  const [locale, setLocale] = usePersistentLocale();
  const [loading, setLoading] = useState(true);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const settingsOpenRef = useRef(settingsOpen);
  settingsOpenRef.current = settingsOpen;
  const usageOpenGeneration = useRef(0);
  const [settingsNavigationKey, setSettingsNavigationKey] = useState(0);
  function closeSettings() {
    usageOpenGeneration.current += 1;
    setSettingsOpen(false);
  }
  const [conversations, setConversations] = useState<WorkspaceConversation[]>([]);
  const [conversation, setConversation] = useState<WorkspaceConversation | null>(null);
  const [conversationMode, setConversationMode] = useState<SessionAgentModeV4>("agent");
  const [conversationState, setConversationState] = useState<ConversationAgentStateV4 | null>(null);
  const [messageSequence, setMessageSequence] = useState(1);
  const [messages, setMessages] = useState<WorkspaceMessage[]>([]);
  const [agentBusy, setAgentBusy] = useState(false);
  const [agentNotice, setAgentNotice] = useState("");
  useRunNotifications((message) => setAgentNotice(message));
  const [modelProfiles, setModelProfiles] = useState<ModelProfile[]>([]);
  const [settingsSection, setSettingsSection] = useState<SettingsSection>("general");
  const [workflowCatalogVersion, setWorkflowCatalogVersion] = useState(0);
  const [activeModelProfileId, setActiveModelProfileId] = useState<string | null>(null);
  const [modelSelectionBusy, setModelSelectionBusy] = useState(false);
  const modelSelectionInFlight = useRef(false);
  const [lastGoal, setLastGoal] = useState("");
  const [v4Plan, setV4Plan] = useState<RunSummaryV4 | null>(null);
  const [computeBackends, setComputeBackends] = useState<ComputeBackendAvailabilityV4[]>([]);
  const [computeBackendId, setComputeBackendId] = useState("local");
  const [containerImage, setContainerImage] = useState("");
  const [autonomyMode, setAutonomyMode] = useState<AutonomyModeV4>("supervised");
  const [approvalPolicy, setApprovalPolicy] = useState<ApprovalPolicyV4>("risk_based");
  const [computeEnvironment, setComputeEnvironment] = useState("system");
  const [computeBusy, setComputeBusy] = useState(false);
  const [planLoading, setPlanLoading] = useState(false);
  const [planApproved, setPlanApproved] = useState(false);
  const [runId, setRunId] = useState<string | null>(null);
  const [runStartedAt, setRunStartedAt] = useState<string | null>(null);
  const [agentTextPreview, setAgentTextPreview] = useState<import("./types").AgentTextPreviewV4 | null>(null);
  const [agentRunEventsV4, setAgentRunEventsV4] = useState<AgentRunEventV4[]>([]);
  const [conversationHydrating, setConversationHydrating] = useState(false);
  const [conversationLoadError, setConversationLoadError] = useState("");
  const [conversationLoadRetry, setConversationLoadRetry] = useState(0);
  const [resumeLastSession] = useResumeLastSessionPreference();
  const nativeQueueAvailable = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
  const queue = useComposerQueue(selected?.id, conversation?.id, nativeQueueAvailable && !conversationHydrating, refreshQueuedConversation);
  const replacement = useComposerReplacement(selected?.id, conversation?.id, nativeQueueAvailable && !conversationHydrating, async () => { await queue.refresh(); await refreshQueuedConversation(); });
  const contextUsage = useContextUsage(selected?.id, conversation?.id, nativeQueueAvailable && !conversationHydrating);
  const sideChat = useSideChat(selected?.id, conversation?.id, nativeQueueAvailable && !conversationHydrating, activeModelProfileId);
  const branchSend = useConversationBranchSend(selected?.id, conversation?.id, openCreatedBranch);
  const [remoteFiles, setRemoteFiles] = useState<RemoteFileEntry[]>([]);
  const [filesBusy, setFilesBusy] = useState(false);
  const [fileNotice, setFileNotice] = useState("");
  const [skillPackages, setSkillPackages] = useState<SkillPackage[]>([]);
  const [mcpServers, setMcpServers] = useState<McpServerProfile[]>([]);
  const [kernelSessions, setKernelSessions] = useState<KernelSession[]>([]);
  const [kernelEvents, setKernelEvents] = useState<KernelEvent[]>([]);
  const [kernelBusy, setKernelBusy] = useState(false);
  const [kernelNotice, setKernelNotice] = useState("");
  const [connections, setConnections] = useState<ConnectionProfile[]>([]);
  const [memoryFacts, setMemoryFacts] = useState<MemoryFact[]>([]);
  const [notebookEntries, setNotebookEntries] = useState<NotebookEntry[]>([]);
  const [projectArtifacts, setProjectArtifacts] = useState<ProjectArtifact[]>([]);
  const [syncEntries, setSyncEntries] = useState<SyncEntry[]>([]);
  const capabilities = useConversationCapabilities(
    selected?.id ?? null,
    conversation?.project_id === selected?.id ? conversation?.id ?? null : null,
    JSON.stringify([
      messages.length, agentBusy, settingsOpen, notebookEntries.length, projectArtifacts.length,
      conversations.map(({ id }) => id),
      skillPackages.map(({ id, name, enabled, sha256 }) => [id, name, enabled, sha256]),
      mcpServers.map(({ id, name, enabled, updated_at }) => [id, name, enabled, updated_at]),
    ]),
  );
  const runActionGuards = useRef(new Set<string>());
  const runPollingNotice = useRef("");
  // Every asynchronous conversation read/action captures both this token and
  // the ids below. A late response from a previous session must never hydrate
  // the currently selected conversation.
  const conversationRequestToken = useRef(0);
  const conversationSnapshotRequestSequence = useRef(0);
  const conversationModeRequestSequence = useRef(0);
  const projectRequestToken = useRef(0);
  const blankConversationRequests = useRef(new Map<string, Promise<WorkspaceConversation>>());
  const blankConversationProject = useRef<string | null>(null);
  const currentConversationIdentity = useRef<{ projectId: string | null; conversationId: string | null }>({ projectId: null, conversationId: null });
  const messagesRef = useRef<WorkspaceMessage[]>([]);
  const messageSequenceRef = useRef(1);
  const pendingSubmissionRef = useRef<PendingSubmission | null>(null);
  const lastGoalRef = useRef("");
  const agentRunEventsRef = useRef<AgentRunEventV4[]>([]);
  const agentEventGeneration = useRef(0);
  const conversationHydratingRef = useRef(false);
  currentConversationIdentity.current = { projectId: selected?.id ?? null, conversationId: conversation?.id ?? null };

  useEffect(() => {
    const pending = pendingSubmissionRef.current;
    if (pending && (pending.projectId !== selected?.id || pending.conversationId !== conversation?.id)) {
      pendingSubmissionRef.current = null;
    }
  }, [selected?.id, conversation?.id]);

  function reportRunPollingFailure(error: unknown) {
    const message = error instanceof Error ? error.message : String(error);
    runPollingNotice.current = message;
    setAgentNotice(message);
  }

  function clearRecoveredRunPollingFailure() {
    const recovered = runPollingNotice.current;
    if (!recovered) return;
    runPollingNotice.current = "";
    setAgentNotice((current) => current === recovered ? "" : current);
  }

  function isCurrentConversation(projectId: string, conversationId: string, token: number) {
    const identity = currentConversationIdentity.current;
    return token === conversationRequestToken.current
      && identity.projectId === projectId
      && identity.conversationId === conversationId;
  }

  function isCurrentConversationIdentity(projectId: string, conversationId: string) {
    const identity = currentConversationIdentity.current;
    return identity.projectId === projectId && identity.conversationId === conversationId;
  }

  function captureConversationAction() {
    const projectId = selected?.id;
    const conversationId = conversation?.id;
    if (!projectId || !conversationId) return null;
    return { projectId, conversationId, token: conversationRequestToken.current };
  }

  function isCurrentConversationAction(action: { projectId: string; conversationId: string; token: number }) {
    return isCurrentConversation(action.projectId, action.conversationId, action.token);
  }

  function isCurrentConversationProject(projectId: string, token: number) {
    return token === projectRequestToken.current
      && (currentConversationIdentity.current.projectId === projectId || selected?.id === projectId);
  }

  function captureConversationGeneration() {
    return {
      projectId: selected?.id ?? null,
      conversationId: conversation?.id ?? null,
      projectToken: projectRequestToken.current,
      conversationToken: conversationRequestToken.current,
    };
  }

  function isCurrentConversationGeneration(generation: ReturnType<typeof captureConversationGeneration>) {
    if (!generation.projectId || generation.projectId !== selected?.id
      || generation.projectToken !== projectRequestToken.current) return false;
    if (generation.conversationToken !== conversationRequestToken.current) return false;
    const currentConversationId = conversation?.id ?? null;
    if (generation.conversationId !== currentConversationId) return false;
    return generation.conversationId === null
      || isCurrentConversationIdentity(generation.projectId, generation.conversationId);
  }

  function setHydrationState(value: boolean) {
    conversationHydratingRef.current = value;
    setConversationHydrating(value);
  }

  function createBlankConversation(projectId: string): Promise<WorkspaceConversation> {
    const existing = blankConversationRequests.current.get(projectId);
    if (existing) return existing;
    const request = api.createConversation(projectId);
    blankConversationRequests.current.set(projectId, request);
    void request.then(
      () => undefined,
      () => { if (blankConversationRequests.current.get(projectId) === request) blankConversationRequests.current.delete(projectId); },
    );
    return request;
  }

  function mergeAgentRunEvents(incoming: AgentRunEventV4[]) {
    const merged = mergeAgentRunEventsV4(agentRunEventsRef.current, incoming);
    agentRunEventsRef.current = merged;
    setAgentRunEventsV4(merged);
  }

  function applyConversationState(snapshot: ConversationAgentStateV4) {
    const revision = snapshot.latest_plan_revision;
    const run = snapshot.latest_run;
    // A conversation can retain older cancelled/superseded revisions while a
    // later direct run is active. Only use revision fields to fill a run when
    // both records identify the same run; otherwise the durable run remains
    // authoritative and the old plan must not leak into the UI.
    const revisionForRun = run && revision?.run_id === run.run_id ? revision : null;
    const summary = run
      ? {
          ...run,
          plan: run.plan ?? revisionForRun?.plan ?? null,
          plan_hash: run.plan_hash ?? revisionForRun?.plan_hash ?? null,
          plan_revision: run.plan_revision ?? revisionForRun?.revision ?? null,
          session_mode: run.session_mode ?? snapshot.mode,
        }
      : revision
        ? {
            run_id: revision.run_id,
            status: revision.status,
            plan: revision.plan,
            plan_hash: revision.plan_hash,
            compute_selection: null,
            approval_hash: null,
            plan_revision: revision.revision,
            session_mode: snapshot.mode,
          }
        : null;
    setConversationState(snapshot);
    setConversationMode(snapshot.mode);
    setV4Plan(summary);
    const effectiveRevision = run ? revisionForRun : revision;
    setPlanApproved(effectiveRevision?.status === "approved" || summary?.status === "approved");

    const active = Boolean(
      run
      && !isTerminalRunStatus(run.status)
      && (snapshot.locked || !revisionForRun || revisionForRun.status === "approved" || run.status === "running"),
    );
    setRunId(active ? run!.run_id : null);
    setRunStartedAt(active ? new Date().toISOString() : null);
  }

  async function refreshConversationState(
    projectId = selected?.id,
    conversationId = conversation?.id,
    token = conversationRequestToken.current,
    preserveTransient = false,
  ) {
    if (!projectId || !conversationId) return null;
    const requestSequence = ++conversationSnapshotRequestSequence.current;
    const eventGeneration = agentEventGeneration.current;
    const snapshot = await api.agentV4ConversationState(projectId, conversationId);
    if (!isCurrentConversation(projectId, conversationId, token)
      || requestSequence !== conversationSnapshotRequestSequence.current
      || eventGeneration !== agentEventGeneration.current) return null;
    // The browser fallback and a freshly-created native run may briefly
    // return an empty durable snapshot. Keep the optimistic run/plan and mode
    // until a non-empty snapshot arrives. A later ordinary refresh remains
    // authoritative and will clear the transient state if nothing was saved.
    if (preserveTransient && !snapshot.latest_run && !snapshot.latest_plan_revision) {
      return snapshot;
    }
    applyConversationState(snapshot);
    return snapshot;
  }

  useEffect(() => {
    Promise.all([api.listProjects(), api.listModelProfiles(), api.listSkillPackages(), api.listMcpServers(), api.listConnections()]).then(([items, profiles, skills, servers, savedConnections]) => {
      setProjects(items); setSelected(items[0] ?? null);
      setModelProfiles(profiles); setActiveModelProfileId(profiles[0]?.id ?? null);
      setSkillPackages(skills);
      setMcpServers(servers);
      setConnections(savedConnections);
    }).finally(() => setLoading(false));
  }, []);
  useEffect(() => {
    const token = ++projectRequestToken.current;
    let disposed = false;
    const selectedProjectId = selected?.id ?? null;
    if (blankConversationProject.current !== selectedProjectId) {
      blankConversationProject.current = selectedProjectId;
      blankConversationRequests.current.clear();
    }
    const requested = requestedConversation.current;
    requestedConversation.current = null;
    const shouldResume = resumeLastSession;
    if (!selected) {
      setConversations([]);
      setConversation(null);
      setConversationLoadError("");
      setHydrationState(false);
      return () => { disposed = true; };
    }
    setConversationLoadError("");
    setHydrationState(true);
    setConversations([]);
    setConversation(null);
    const requestedForProject = requested?.projectId === selected.id ? requested : null;
    const loadConversation = async () => {
      const [items, latest] = await Promise.all([
        api.listConversations(selected.id),
        requestedForProject || !shouldResume ? Promise.resolve(null) : api.latestUsedConversation(selected.id),
      ]);
      if (disposed || !isCurrentConversationProject(selected.id, token)) return;
      let active = requestedForProject ? items.find((item) => item.id === requestedForProject.conversationId) : latest;
      if (requestedForProject && !active) throw new Error("The requested saved conversation is no longer available.");
      if (active && active.project_id !== selected.id) throw new Error("The restored conversation belongs to another project.");
      if (!active) {
        active = await createBlankConversation(selected.id);
        if (disposed || !isCurrentConversationProject(selected.id, token)) return;
      }
      setConversations(items.some((item) => item.id === active.id) ? items : [active, ...items]);
      setConversation(active);
    };
    void loadConversation().catch(() => {
      if (!disposed && isCurrentConversationProject(selected.id, token)) {
        if (requestedForProject) requestedConversation.current = requestedForProject;
        setHydrationState(false);
        setConversationLoadError(locale === "zh-CN" ? "无法恢复此项目的会话。请重试。" : "Could not restore this project's sessions. Please retry.");
      }
    });
    return () => { disposed = true; };
  }, [selected?.id, conversationLoadRetry]);
  useEffect(() => {
    let disposed = false;
    if (!selected) { setComputeBackends([]); return () => { disposed = true; }; }
    const timer = window.setTimeout(() => {
      setComputeBusy(true);
      api.agentV4ComputeBackends(selected.id, containerImage)
        .then((items) => {
          if (disposed) return;
          setComputeBackends(items);
          setComputeBackendId((current) => {
            if (items.some((item) => item.descriptor.backend_id === current && item.selectable)) return current;
            const preferred = selected.connection_id
              ? items.find((item) => item.descriptor.kind === "ssh" && item.selectable)
              : items.find((item) => item.descriptor.kind === "local" && item.selectable);
            return preferred?.descriptor.backend_id ?? items.find((item) => item.selectable)?.descriptor.backend_id ?? current;
          });
        })
        .catch((error) => { if (!disposed) setAgentNotice(error instanceof Error ? error.message : String(error)); })
        .finally(() => { if (!disposed) setComputeBusy(false); });
    }, 250);
    return () => { disposed = true; window.clearTimeout(timer); };
  }, [selected?.id, selected?.connection_id, containerImage]);
  useEffect(() => {
    const backend = computeBackends.find((item) => item.descriptor.backend_id === computeBackendId);
    if (!backend) return;
    if (backend.descriptor.kind !== "ssh") setComputeEnvironment("system");
    if (backend.descriptor.isolation !== "container") {
      setAutonomyMode("supervised");
      setApprovalPolicy((current) => current === "full_access" ? "risk_based" : current);
    }
  }, [computeBackendId, computeBackends]);
  useEffect(() => {
    if (!selected) { setMemoryFacts([]); setNotebookEntries([]); setProjectArtifacts([]); return; }
    void Promise.all([api.searchAgentMemory(selected.id), api.listNotebookEntries(selected.id), api.listProjectArtifacts(selected.id), api.listSyncEntries(selected.id)])
      .then(([facts, notebook, artifacts, transfers]) => { setMemoryFacts(facts); setNotebookEntries(notebook); setProjectArtifacts(artifacts); setSyncEntries(transfers); })
      .catch((error) => setAgentNotice(error instanceof Error ? error.message : String(error)));
  }, [selected?.id, agentRunEventsV4.at(-1)?.sequence]);
  useEffect(() => {
    setFileNotice("");
    if (!selected?.connection_id || !selected.remote_root) {
      setRemoteFiles([]);
      return;
    }
    void refreshRemoteFiles(selected.id);
  }, [selected?.id, selected?.connection_id, selected?.remote_root]);
  useEffect(() => {
    setKernelEvents([]);
    setKernelNotice("");
    if (!selected) { setKernelSessions([]); return; }
    const subscriptionProjectId = selected.id;
    const subscriptionProjectToken = projectRequestToken.current;
    let disposed = false;
    api.listKernelSessions()
      .then((sessions) => {
        if (disposed || subscriptionProjectToken !== projectRequestToken.current
          || currentConversationIdentity.current.projectId !== subscriptionProjectId) return;
        setKernelSessions(sessions.filter((session) => session.project_id === subscriptionProjectId));
      })
      .catch((error) => {
        if (!disposed && subscriptionProjectToken === projectRequestToken.current
          && currentConversationIdentity.current.projectId === subscriptionProjectId) {
          setKernelNotice(error instanceof Error ? error.message : String(error));
        }
      });
    return () => { disposed = true; };
  }, [selected?.id]);
  useEffect(() => {
    const projectId = selected?.id;
    const conversationId = conversation?.id;
    const token = ++conversationRequestToken.current;
    const snapshotRequestSequence = ++conversationSnapshotRequestSequence.current;
    const modeRequestSequence = conversationModeRequestSequence.current;
    const hydrationEventGeneration = agentEventGeneration.current;
    let disposed = false;
    setHydrationState(true);
    setConversationState(null);
    messagesRef.current = [];
    messageSequenceRef.current = 1;
    lastGoalRef.current = "";
    setMessages([]);
    setMessageSequence(1);
    setLastGoal("");
    // Keep the previous durable mode hidden behind the hydration lock until
    // the snapshot arrives. Setting Agent here briefly exposed the wrong mode
    // for conversations that were persisted in Plan mode.
    setAgentRunEventsV4([]);
    agentRunEventsRef.current = [];
    setV4Plan(null);
    setPlanApproved(false);
    setRunId(null);
    setRunStartedAt(null);
    agentStop.clear();
    if (!projectId) {
      setHydrationState(false);
      return () => { disposed = true; };
    }
    if (!conversationId || conversation?.project_id !== projectId) {
      // A project with no selected conversation is still restoring its
      // initial session. Keep all ordinary session actions locked until the
      // list/create request selects a conversation and this effect hydrates it.
      setHydrationState(true);
      return () => { disposed = true; };
    }

    const storedMessages = api.listMessages(conversationId);
    const eventsThenState = api.agentV4EventsForConversation(projectId, conversationId).then(async (eventsV4) => {
      // Native event hydration reconciles an uncertain stop before returning.
      // Read the durable snapshot only after that reconciliation has finished,
      // otherwise an older running/locked snapshot can resurrect the run.
      if (disposed || !isCurrentConversation(projectId, conversationId, token)) return null;
      const snapshot = await api.agentV4ConversationState(projectId, conversationId);
      return { snapshot, eventsV4 };
    });
    Promise.all([storedMessages, eventsThenState])
      .then(([storedMessages, hydrated]) => {
        if (!hydrated) return;
        const { snapshot, eventsV4 } = hydrated;
        if (disposed || !isCurrentConversation(projectId, conversationId, token)) return;
        clearRecoveredRunPollingFailure();
        const mergedMessages = mergeWorkspaceMessages(messagesRef.current, storedMessages);
        messagesRef.current = mergedMessages;
        setMessages((current) => {
          const merged = mergeWorkspaceMessages(current, mergedMessages);
          messagesRef.current = merged;
          return merged;
        });
        const storedNextSequence = (storedMessages.reduce((maximum, message) => Math.max(maximum, message.sequence), 0) + 1);
        messageSequenceRef.current = Math.max(messageSequenceRef.current, storedNextSequence);
        setMessageSequence((current) => {
          const next = Math.max(current, messageSequenceRef.current, storedNextSequence);
          messageSequenceRef.current = next;
          return next;
        });
        const hydratedGoal = lastGoalRef.current || [...storedMessages].reverse().find((message) => message.role === "user")?.markdown || "";
        lastGoalRef.current = hydratedGoal;
        setLastGoal((current) => {
          const next = lastGoalRef.current || current || hydratedGoal;
          lastGoalRef.current = next;
          return next;
        });
        // A mode write can complete while this initial read is in flight.
        // Keep the independently loaded messages/events, but do not let the
        // older snapshot roll back that explicit mode transition. The mode
        // action performs its own authoritative snapshot refresh.
        if (snapshotRequestSequence === conversationSnapshotRequestSequence.current
          && modeRequestSequence === conversationModeRequestSequence.current
          && hydrationEventGeneration === agentEventGeneration.current) {
          applyConversationState(snapshot);
        }
        // Keep events delivered while hydration was in flight. Replacing the
        // list here would erase a newer live event (and the snapshot refresh
        // it triggered), while merging still hydrates an otherwise empty UI.
        const mergedEvents = mergeAgentRunEventsV4(agentRunEventsRef.current, eventsV4);
        agentRunEventsRef.current = mergedEvents;
        setAgentRunEventsV4(() => mergedEvents);
        // A historical event list may be older than an event received through
        // the live subscription while this request was pending. In that case
        // the live run state remains authoritative and the historical list is
        // used only to fill missing trace entries.
        const stateEvents = hydrationEventGeneration === agentEventGeneration.current
          ? mergedEvents
          : agentRunEventsRef.current;
        const latest = stateEvents
          .map((event) => ({ runId: event.run_id, timestamp: event.occurred_at }))
          .sort((left, right) => new Date(left.timestamp).getTime() - new Date(right.timestamp).getTime())
          .at(-1);
        const latestRunEventsV4 = latest ? stateEvents.filter((event) => event.run_id === latest.runId) : [];
        // Events can be absent from a test/native reconnect while the durable
        // snapshot still contains a planning run. Prefer the snapshot in that
        // case; event history only narrows a run when it proves termination.
        if (hydrationEventGeneration === agentEventGeneration.current
          && snapshotRequestSequence === conversationSnapshotRequestSequence.current
          && latest && !latestRunEventsV4.some(isTerminalAgentEventV4)) {
          setRunId(latest.runId);
          setRunStartedAt(latestAgentRunEventV4(latestRunEventsV4)?.occurred_at ?? latest.timestamp);
        }
        setHydrationState(false);
      })
      .catch((error) => {
        if (!disposed && isCurrentConversation(projectId, conversationId, token)) {
          setHydrationState(false);
          reportRunPollingFailure(error);
        }
      });
    return () => { disposed = true; };
  }, [selected?.id, conversation?.id]);
  useEffect(() => {
    let disposed = false;
    const unlisten: Array<() => void> = [];
    const subscriptionProjectId = selected?.id;
    const subscriptionConversationId = conversation?.id;
    const subscriptionProjectToken = projectRequestToken.current;
    const isCurrentSubscriptionProject = () => !disposed
      && Boolean(subscriptionProjectId)
      && subscriptionProjectToken === projectRequestToken.current
      && currentConversationIdentity.current.projectId === subscriptionProjectId;
    api.onConversationEvent((event) => {
      if (!isCurrentSubscriptionProject() || !subscriptionConversationId
        || event.project_id !== subscriptionProjectId
        || event.conversation_id !== subscriptionConversationId
        || !isCurrentConversationIdentity(subscriptionProjectId, subscriptionConversationId)) return;
      if (!messagesRef.current.some((message) => message.id === event.message.id)) {
        messagesRef.current = [...messagesRef.current, event.message];
      }
      setMessages((current) => {
        const next = mergeWorkspaceMessages(current, messagesRef.current);
        messagesRef.current = next;
        return next;
      });
      messageSequenceRef.current = Math.max(messageSequenceRef.current, event.message.sequence + 1);
      setMessageSequence((value) => {
        const next = Math.max(value, messageSequenceRef.current, event.message.sequence + 1);
        messageSequenceRef.current = next;
        return next;
      });
      if (event.message.role === "user") lastGoalRef.current = event.message.markdown;
    }).then((fn) => disposed ? fn() : unlisten.push(fn)).catch((error) => {
      if (!disposed && isCurrentSubscriptionProject()) setAgentNotice(subscriptionError("conversation", error));
    });
    api.onConversationUpdated((event) => {
      if (!isCurrentSubscriptionProject() || event.project_id !== subscriptionProjectId) return;
      setConversations((current) => [event.conversation, ...current.filter((item) => item.id !== event.conversation.id)]);
      setConversation((current) => current?.id === event.conversation.id ? event.conversation : current);
    }).then((fn) => disposed ? fn() : unlisten.push(fn)).catch((error) => {
      if (!disposed && isCurrentSubscriptionProject()) setAgentNotice(subscriptionError("conversation updates", error));
    });
    api.onKernelEvent((event) => {
      if (!isCurrentSubscriptionProject() || event.project_id !== subscriptionProjectId) return;
      setKernelEvents((current) => [...current.slice(-199), event]);
    }).then((fn) => disposed ? fn() : unlisten.push(fn)).catch((error) => {
      if (!disposed && isCurrentSubscriptionProject()) setAgentNotice(subscriptionError("kernel", error));
    });
    api.onSyncEvent((entry) => {
      if (!isCurrentSubscriptionProject() || entry.project_id !== subscriptionProjectId) return;
      setSyncEntries((current) => [entry, ...current.filter((item) => item.id !== entry.id)]);
    }).then((fn) => disposed ? fn() : unlisten.push(fn)).catch((error) => {
      if (!disposed && isCurrentSubscriptionProject()) setAgentNotice(subscriptionError("sync", error));
    });
    setAgentTextPreview(null);
    api.onAgentV4TextPreview((preview) => {
      if (!isCurrentSubscriptionProject() || !subscriptionProjectId || !subscriptionConversationId
        || !isCurrentConversationIdentity(subscriptionProjectId, subscriptionConversationId)
        || !agentRunEventsRef.current.some((event) => event.run_id === preview.run_id && event.project_id === subscriptionProjectId && event.conversation_id === subscriptionConversationId)) return;
      setAgentTextPreview(preview.text === null ? null : preview);
    }).then((fn) => disposed ? fn() : unlisten.push(fn)).catch((error) => {
      if (!disposed) setAgentNotice(subscriptionError("Agent streaming", error));
    });
    api.onAgentV4Event((event) => {
      if (!isCurrentSubscriptionProject() || !subscriptionConversationId
        || event.project_id !== subscriptionProjectId
        || event.conversation_id !== subscriptionConversationId
        || !isCurrentConversationIdentity(subscriptionProjectId, subscriptionConversationId)) return;
      if (event.event.kind === "model_text" || isTerminalAgentEventV4(event)) setAgentTextPreview(null);
      agentEventGeneration.current += 1;
      const eventToken = conversationRequestToken.current;
      if (isTerminalAgentEventV4(event)) {
        agentStop.markTerminal(event.run_id);
        setRunStartedAt(null);
        setRunId((current) => current === event.run_id ? null : current);
        if (event.event.kind === "run_completed") void refreshRemoteFiles(event.project_id);
      } else {
        setRunId((current) => current ?? event.run_id);
        setRunStartedAt((current) => current ?? event.occurred_at);
      }
      agentRunEventsRef.current = mergeAgentRunEventsV4(agentRunEventsRef.current, [event]);
      setAgentRunEventsV4(agentRunEventsRef.current);
      if (eventChangesConversationState(event.event.kind)) {
        // Event rendering above remains immediate. Snapshot reconciliation is
        // best-effort and deliberately cannot replace the primary event
        // error/notice when the read fails.
        void refreshConversationState(event.project_id, event.conversation_id, eventToken).catch(() => undefined);
      }
    }).then((fn) => disposed ? fn() : unlisten.push(fn)).catch((error) => {
      if (!disposed) setAgentNotice(subscriptionError("Agent V4", error));
    });
    return () => { disposed = true; unlisten.forEach((fn) => fn()); };
  }, [conversation?.id, selected?.id]);

  useEffect(() => {
    const activeRunId = runId;
    if (!activeRunId) return;
    let disposed = false;
    let inFlight = false;
    const reconcile = async () => {
      if (disposed || inFlight) return;
      inFlight = true;
      try {
        const events = await api.agentV4Events(activeRunId);
        if (disposed) return;
        const runEvents = events.filter((event) => event.run_id === activeRunId);
        clearRecoveredRunPollingFailure();
        mergeAgentRunEvents(runEvents);
        if (runEvents.some(isTerminalAgentEventV4)) {
          agentStop.markTerminal(activeRunId);
          setRunStartedAt(null);
          setRunId((current) => current === activeRunId ? null : current);
        }
      } catch (error) {
        if (!disposed) reportRunPollingFailure(error);
      } finally {
        inFlight = false;
      }
    };
    void reconcile();
    const timer = window.setInterval(() => { void reconcile(); }, 3_000);
    return () => { disposed = true; window.clearInterval(timer); };
  }, [runId]);

  function replaceKernelSession(session: KernelSession) {
    setKernelSessions((current) => [session, ...current.filter((item) => item.id !== session.id)]);
  }

  async function withKernelBusy(action: () => Promise<void>) {
    setKernelBusy(true); setKernelNotice("");
    try { await action(); }
    catch (error) { setKernelNotice(error instanceof Error ? error.message : String(error)); }
    finally { setKernelBusy(false); }
  }

  async function startKernel(language: KernelLanguage, rebuildSessionId?: string) {
    if (!selected) return;
    await withKernelBusy(async () => replaceKernelSession(await api.startKernel(selected.id, language, rebuildSessionId)));
  }

  async function refreshRemoteFiles(projectId = selected?.id) {
    if (!projectId) return;
    setFilesBusy(true);
    try {
      setRemoteFiles(await api.listRemoteFiles(projectId));
    } catch (error) {
      setFileNotice(error instanceof Error ? error.message : String(error));
    } finally {
      setFilesBusy(false);
    }
  }

  async function uploadFiles() {
    if (!selected) return;
    try {
      const relativePaths = await api.chooseProjectFiles(selected.local_root);
      if (relativePaths.length === 0) return;
      setFilesBusy(true);
      const entries = await api.uploadSelectedFiles(selected.id, relativePaths);
      setSyncEntries(await api.listSyncEntries(selected.id));
      setFileNotice(locale === "zh-CN" ? `已校验上传 ${entries.length} 个文件` : `${entries.length} uploaded files verified`);
      setRemoteFiles(await api.listRemoteFiles(selected.id));
    } catch (error) {
      setFileNotice(error instanceof Error ? error.message : String(error));
    } finally {
      setFilesBusy(false);
    }
  }

  async function downloadFile(relativePath: string) {
    if (!selected) return;
    setFilesBusy(true);
    try {
      const result = await api.downloadProjectFile(selected.id, relativePath);
      setSyncEntries(await api.listSyncEntries(selected.id));
      setFileNotice(result.conflict
        ? (locale === "zh-CN" ? `本地文件不同，已保存冲突副本：${result.entry.relative_path}` : `Local file differed; saved conflict copy: ${result.entry.relative_path}`)
        : (locale === "zh-CN" ? `下载完成并通过 SHA-256 校验：${result.entry.relative_path}` : `Downloaded and SHA-256 verified: ${result.entry.relative_path}`));
    } catch (error) {
      setFileNotice(error instanceof Error ? error.message : String(error));
    } finally {
      setFilesBusy(false);
    }
  }
  function resetConversationWork(hydrating = false) {
    pendingSubmissionRef.current = null;
    messagesRef.current = [];
    messageSequenceRef.current = 1;
    lastGoalRef.current = "";
    agentRunEventsRef.current = [];
    setMessages([]); setMessageSequence(1); setAgentBusy(false); setAgentNotice("");
    setLastGoal("");
    if (!hydrating) setConversationMode("agent");
    setHydrationState(hydrating);
    setConversationState(null); setV4Plan(null); setPlanApproved(false); setRunId(null); setRunStartedAt(null); agentStop.clear(); setAgentRunEventsV4([]);
  }

  function currentComputeSelection(): ComputeSelectionV4 {
    const backend = computeBackends.find((item) => item.descriptor.backend_id === computeBackendId);
    if (!backend?.selectable) throw new Error(locale === "zh-CN" ? "请选择一个有效的 V4 计算配置。" : "Select a valid V4 compute configuration.");
    const container = backend.descriptor.kind === "docker" || backend.descriptor.kind === "podman";
    if (container && !backend.resolved_image_id) throw new Error(locale === "zh-CN" ? "容器镜像尚未在本机验证。" : "The container image has not been verified locally.");
    return {
      schema_version: 4,
      backend_id: backend.descriptor.backend_id,
      backend_kind: backend.descriptor.kind,
      autonomy_mode: container && approvalPolicy === "full_access" ? "full_auto" : "supervised",
      approval_policy: approvalPolicy,
      environment: backend.descriptor.kind === "ssh" ? (computeEnvironment.trim() || "system") : "system",
      network_policy: container ? "none" : "host_inherited",
      container_image: container ? { reference: containerImage.trim(), image_id: backend.resolved_image_id! } : null,
    };
  }

  async function startV4Planning(goal: string, token = ++conversationRequestToken.current, references: ComposerReference[] = [], attachments: string[] = []) {
    if (!selected || !conversation || !activeModel) throw new Error(locale === "zh-CN" ? "请先选择会话和模型。" : "Select a conversation and model first.");
    const projectId = selected.id;
    const conversationId = conversation.id;
    const summary = await api.agentV4StartPlanning({
      project_id: projectId,
      conversation_id: conversationId,
      model_profile_id: activeModel.id,
      objective: goal,
      ...(references.length ? { references } : {}),
      ...(attachments.length ? { attachments } : {}),
      compute_selection: currentComputeSelection(),
    });
    if (!isCurrentConversation(projectId, conversationId, token)) return summary;
    setV4Plan(summary);
    setRunId(summary.run_id);
    setRunStartedAt(new Date().toISOString());
    setPlanApproved(false);
    let events: AgentRunEventV4[] = [];
    try {
      events = await api.agentV4Events(summary.run_id);
      if (isCurrentConversation(projectId, conversationId, token)) clearRecoveredRunPollingFailure();
    } catch (error) {
      // The native start has already been accepted. Keep its summary and let
      // the polling effect retry the event read instead of turning this into
      // a failed send.
      if (isCurrentConversation(projectId, conversationId, token)) reportRunPollingFailure(error);
    }
    if (!isCurrentConversation(projectId, conversationId, token)) return summary;
    mergeAgentRunEvents(events);
    // Reconcile the planning result with the durable conversation snapshot.
    // A temporary read failure must not turn a successfully-created run into
    // a failed send or overwrite the local plan with an empty fallback.
    try {
      await refreshConversationState(projectId, conversationId, token, true);
    } catch {
      // The run summary/events above are still authoritative for this turn.
    }
    return summary;
  }

  async function startV4Direct(goal: string, token = ++conversationRequestToken.current, references: ComposerReference[] = [], attachments: string[] = []) {
    if (!selected || !conversation || !activeModel) throw new Error(locale === "zh-CN" ? "请先选择会话和模型。" : "Select a conversation and model first.");
    const projectId = selected.id;
    const conversationId = conversation.id;
    const summary = await api.agentV4StartDirect({
      project_id: projectId,
      conversation_id: conversationId,
      model_profile_id: activeModel.id,
      objective: goal,
      ...(references.length ? { references } : {}),
      ...(attachments.length ? { attachments } : {}),
      compute_selection: currentComputeSelection(),
    });
    if (!isCurrentConversation(projectId, conversationId, token)) return summary;
    setV4Plan(summary);
    setRunId(summary.run_id);
    setRunStartedAt(new Date().toISOString());
    setPlanApproved(false);
    let events: AgentRunEventV4[] = [];
    try {
      events = await api.agentV4Events(summary.run_id);
      if (isCurrentConversation(projectId, conversationId, token)) clearRecoveredRunPollingFailure();
    } catch (error) {
      // Event history is a reconciliation read. A failure here must not
      // cause the already-created native run to be submitted again.
      if (isCurrentConversation(projectId, conversationId, token)) reportRunPollingFailure(error);
    }
    if (!isCurrentConversation(projectId, conversationId, token)) return summary;
    mergeAgentRunEvents(events);
    return summary;
  }

  function activateConversation(next: WorkspaceConversation) {
    if (next.id === conversation?.id) return;
    ++conversationRequestToken.current;
    resetConversationWork(true);
    setConversation(next);
  }

  async function selectConversation(conversationId: string) {
    const next = conversations.find((item) => item.id === conversationId);
    if (next) activateConversation(next);
  }

  async function openSearchEntry(entry: WorkspaceSearchEntry): Promise<boolean> {
    if (entry.kind === "action") {
      if (entry.key === "action:files" && selected) {
        closeSettings();
        setSearchRequest({ key: crypto.randomUUID(), kind: "files", projectId: selected.id });
      }
      else {
        setSettingsSection(entry.key === "action:skills" ? "skills" : entry.key === "action:mcp" ? "connections" : "models");
        setSettingsNavigationKey((value) => value + 1);
        setSettingsOpen(true);
      }
      return true;
    }
    if (entry.kind === "skill") { setSettingsSection("skills"); setSettingsNavigationKey((value) => value + 1); setSettingsOpen(true); return true; }
    const sourceProject = projects.find((project) => project.id === entry.projectId);
    if (!sourceProject) throw new Error("The source project is no longer available.");
    if (entry.kind === "session") {
      const reference = entry.item?.reference;
      if (reference?.kind !== "session") return false;
      const generation = searchActionGeneration.current;
      const available = await api.listConversations(sourceProject.id);
      if (generation !== searchActionGeneration.current) return false;
      const target = available.find((item) => item.id === reference.id && item.project_id === sourceProject.id);
      if (!target) throw new Error("The saved conversation is no longer available.");
      closeSettings();
      setSearchRequest({ key: crypto.randomUUID(), kind: "reveal", projectId: sourceProject.id });
      if (selected?.id === sourceProject.id) { setConversations(available); activateConversation(target); }
      else {
        requestedConversation.current = { projectId: sourceProject.id, conversationId: target.id };
        setSelected(sourceProject);
      }
      return true;
    }
    requestedConversation.current = null;
    closeSettings();
    if (entry.kind === "artifact" && entry.item) setSearchRequest({ key: crypto.randomUUID(), kind: "artifact", projectId: sourceProject.id, item: entry.item });
    else setSearchRequest({ key: crypto.randomUUID(), kind: "reveal", projectId: sourceProject.id });
    setSelected(sourceProject);
    return true;
  }

  async function openUsageConversation(projectId: string, conversationId: string) {
    const operation = ++usageOpenGeneration.current;
    const sourceProject = projects.find((project) => project.id === projectId);
    if (!sourceProject) throw new Error("usage project unavailable");
    const available = selected?.id === projectId ? conversations : await api.listConversations(projectId);
    if (operation !== usageOpenGeneration.current || !settingsOpenRef.current) return;
    const target = available.find((item) => item.id === conversationId && item.project_id === projectId);
    if (!target) throw new Error("saved conversation unavailable");
    closeSettings();
    if (selected?.id === projectId) {
      setConversations(available);
      activateConversation(target);
    } else {
      requestedConversation.current = { projectId, conversationId };
      setSelected(sourceProject);
    }
  }

  async function refreshQueuedConversation() {
    const action = captureConversationAction();
    if (!action || conversationHydratingRef.current) return;
    const events = await api.agentV4EventsForConversation(action.projectId, action.conversationId);
    if (!isCurrentConversationAction(action)) return;
    agentRunEventsRef.current = mergeAgentRunEventsV4(agentRunEventsRef.current, events);
    setAgentRunEventsV4(agentRunEventsRef.current);
    const saved = await api.listMessages(action.conversationId);
    if (!isCurrentConversationAction(action)) return;
    const merged = mergeWorkspaceMessages(messagesRef.current, saved);
    messagesRef.current = merged; setMessages(merged);
    const next = merged.reduce((maximum, message) => Math.max(maximum, message.sequence + 1), 1);
    messageSequenceRef.current = Math.max(messageSequenceRef.current, next);
    setMessageSequence(messageSequenceRef.current);
    await refreshConversationState(action.projectId, action.conversationId, action.token);
  }

  async function openCreatedBranch(branch: import("./types").ConversationBranchV4): Promise<boolean> {
    const generation = captureConversationGeneration();
    if (generation.projectId !== branch.project_id || generation.conversationId !== branch.source_conversation_id) return false;
    const available = await api.listConversations(branch.project_id);
    if (!isCurrentConversationGeneration(generation)) return false;
    const target = available.find((item) => item.id === branch.branch_conversation_id && item.project_id === branch.project_id);
    if (!target) throw new Error("The saved branch could not be loaded.");
    setConversations(available);
    activateConversation(target);
    return true;
  }

  async function newConversation() {
    if (!selected || agentBusy || conversationHydrating || conversationLocked) return;
    const generation = captureConversationGeneration();
    setAgentNotice("");
    try {
      const created = await api.createConversation(generation.projectId!);
      if (!isCurrentConversationGeneration(generation)) return;
      setConversations((current) => [created, ...current]);
      ++conversationRequestToken.current;
      resetConversationWork(true);
      setConversation(created);
    } catch (error) {
      if (isCurrentConversationGeneration(generation)) {
        setAgentNotice(error instanceof Error ? error.message : String(error));
      }
    }
  }

  async function deleteConversation(conversationId: string) {
    if (!selected || agentBusy || conversationHydrating || conversationLocked || (conversation?.id === conversationId && runId)) return;
    const generation = captureConversationGeneration();
    setAgentNotice("");
    try {
      await api.deleteConversation(generation.projectId!, conversationId);
      if (!isCurrentConversationGeneration(generation)) return;
      const remaining = conversations.filter((item) => item.id !== conversationId);
      if (conversation?.id !== conversationId) {
        setConversations(remaining);
        return;
      }
      let next = remaining[0];
      if (!next) {
        next = await api.createConversation(generation.projectId!);
        if (!isCurrentConversationGeneration(generation)) return;
      }
      setConversations(remaining.length > 0 ? remaining : [next]);
      ++conversationRequestToken.current;
      resetConversationWork(true);
      setConversation(next);
    } catch (error) {
      if (isCurrentConversationGeneration(generation)) {
        setAgentNotice(error instanceof Error ? error.message : String(error));
      }
    }
  }
  const activeModel = modelProfiles.find((profile) => profile.id === activeModelProfileId) ?? null;
  const activeRunLastActivityAt = runId
    ? latestAgentRunEventV4(agentRunEventsV4.filter((event) => event.run_id === runId))?.occurred_at ?? runStartedAt
    : null;
  const currentRunEventsV4 = runId ? agentRunEventsV4.filter((event) => event.run_id === runId) : [];
  const latestRun = conversationState?.latest_run ?? v4Plan;
  const snapshotPlanRevision = conversationState?.latest_plan_revision ?? null;
  // Do not render a revision belonging to another run (for example a
  // cancelled planning run retained in the conversation history) alongside a
  // later direct run. A revision can supplement a run only when its run id
  // matches; with no run, it remains the current plan draft.
  const latestPlanRevision = snapshotPlanRevision && (!latestRun || snapshotPlanRevision.run_id === latestRun.run_id)
    ? snapshotPlanRevision
    : null;
  const currentRunAwaitsPlanApproval = isActivePlanRevisionStatus(latestPlanRevision?.status)
    || ["planning", "generating", "revising", "pending", "awaiting_approval"].includes(latestRun?.status ?? "")
    || (latestRun?.status === "waiting_for_approval" && latestRun.session_mode === "plan" && latestPlanRevision?.status !== "approved")
    || v4Plan?.status === "awaiting_approval"
    || (currentRunEventsV4.some((event) => event.event.kind === "plan_proposed")
      && !currentRunEventsV4.some((event) => event.event.kind === "mode_changed" && event.event.mode === "execute"));
  const conversationLocked = Boolean(
    conversationHydrating
    || conversationState?.locked
    || isActivePlanRevisionStatus(latestPlanRevision?.status)
    || planLoading
    || currentRunAwaitsPlanApproval,
  );
  const activePlanRunId = runId ?? latestPlanRevision?.run_id ?? latestRun?.run_id ?? v4Plan?.run_id ?? null;
  const stopRunId = runId
    ?? (latestRun && !isTerminalRunStatus(latestRun.status) ? latestRun.run_id : null)
    ?? (latestPlanRevision && isActivePlanRevisionStatus(latestPlanRevision.status) ? latestPlanRevision.run_id : null);
  const agentStop = useAgentStop(selected?.id && conversation?.id && stopRunId
    ? { projectId: selected.id, conversationId: conversation.id, runId: stopRunId }
    : null);
  const runStopping = agentStop.stopping;
  const stopNoticeRef = useRef("");
  useEffect(() => {
    if (!agentStop.error || !selected?.id || !conversation?.id) {
      const previous = stopNoticeRef.current;
      stopNoticeRef.current = "";
      if (previous) setAgentNotice((current) => current === previous ? "" : current);
      return;
    }
    const message = locale === "zh-CN" ? "停止状态更新失败，请重试。" : "Could not update the run stop state. Please retry.";
    stopNoticeRef.current = message;
    setAgentNotice(message);
  }, [agentStop.error, conversation?.id, locale, selected?.id]);

  if (loading) return <div className="desktop-loading">OmicsOps</div>;
  const searchEntries: WorkspaceSearchEntry[] = [
    ...workspaceSearch.entries,
    { key: "action:models", kind: "action", label: locale === "zh-CN" ? "管理模型" : "Manage models", description: locale === "zh-CN" ? "打开模型设置" : "Open model settings" },
    { key: "action:skills", kind: "action", label: locale === "zh-CN" ? "管理技能" : "Manage skills", description: locale === "zh-CN" ? "打开技能设置" : "Open skill settings" },
    { key: "action:mcp", kind: "action", label: locale === "zh-CN" ? "管理 MCP" : "Manage MCP", description: locale === "zh-CN" ? "打开 MCP 连接设置" : "Open MCP connection settings" },
    ...(selected ? [{ key: "action:files", kind: "action" as const, label: locale === "zh-CN" ? "项目文件" : "Project files", description: selected.name }] : []),
  ];
  const canAttachFromSearch = (entry: WorkspaceSearchEntry) => !agentBusy && !conversationLocked && !modelSelectionBusy && !runId && canAttachSearchEntry(entry, selected?.id, conversation?.project_id === selected?.id ? conversation?.id : undefined);
  const searchDialog = searchOpen ? <WorkspaceSearchDialog entries={searchEntries} zh={locale === "zh-CN"} loading={workspaceSearch.loading} failedProjects={workspaceSearch.failedProjects} onRetry={workspaceSearch.retry} onClose={closeWorkspaceSearch} onOpen={openSearchEntry} canAttach={canAttachFromSearch} onAttach={(entry) => {
    if (!selected || !conversation || !entry.item || !canAttachFromSearch(entry)) return false;
    setSearchRequest({ key: crypto.randomUUID(), kind: "attach", projectId: selected.id, conversationId: conversation.id, item: entry.item });
    return true;
  }} /> : null;
  const settings = settingsOpen ? <SettingsPanel key={settingsNavigationKey} initialSection={settingsSection} locale={locale} onLocaleChange={setLocale} onClose={closeSettings} modelProfiles={modelProfiles} skillPackages={skillPackages} mcpServers={mcpServers} connections={connections} projects={projects} selectedProject={selected} onOpenUsageConversation={openUsageConversation} onWorkflowsChanged={() => setWorkflowCatalogVersion((value) => value + 1)} onMemoryChanged={capabilities.refresh} onSaveConnection={async (profile, secret) => { await api.saveConnection(profile, secret); setConnections(await api.listConnections()); }} onTestConnection={api.testConnection} onConfirmHostKey={async (profileId, fingerprint) => { await api.confirmHostKey(profileId, fingerprint); setConnections(await api.listConnections()); }} onBindProjectRemote={async (connectionId, remoteRoot) => { if (!selected) return; const updated = await api.updateProjectRemote(selected.id, connectionId, remoteRoot); setSelected((current) => current?.id === updated.id ? updated : current); setProjects((current) => current.map((project) => project.id === updated.id ? updated : project)); }} onSaveModel={async (request) => {
    if (modelSelectionInFlight.current) throw new Error("Model selection is currently locked");
    modelSelectionInFlight.current = true;
    setModelSelectionBusy(true);
    try {
      const profile = await api.saveModelProfile(request);
      setModelProfiles((current) => [profile, ...current.filter((item) => item.id !== profile.id)]);
      setActiveModelProfileId(profile.id);
    } finally {
      modelSelectionInFlight.current = false;
      setModelSelectionBusy(false);
    }
  }} onProbeModel={api.probeModelProfile} onListModels={api.listModelProfileModels} onImportSkill={async () => { const sourcePath = await api.chooseSkillDirectory(); if (!sourcePath) return; const skill = await api.importSkillDirectory(sourcePath); setSkillPackages((current) => [skill, ...current.filter((item) => item.id !== skill.id)]); }} onSetSkillEnabled={async (skillId, enabled) => { const updated = await api.setSkillEnabled(skillId, enabled); setSkillPackages(await api.listSkillPackages()); return updated; }} onSkillsChanged={async () => setSkillPackages(await api.listSkillPackages())} onPluginsChanged={async () => setSkillPackages(await api.listSkillPackages())} onSaveMcpServer={async (request) => { const updated = await api.saveMcpServer(request); setMcpServers(await api.listMcpServers()); return updated; }} onConfigurePubMedMcp={async (request) => { const updated = await api.configurePubMedMcpCredentials(request); setMcpServers(await api.listMcpServers()); return updated; }} onListBundledMcpPresets={api.listBundledMcpPresets} onAddBundledMcp={async (request) => { const updated = await api.addBundledMcpServer(request); setMcpServers(await api.listMcpServers()); return updated; }} onInspectMcpServer={async (serverId) => { if (!selected) throw new Error(locale === "zh-CN" ? "请先打开一个项目，再检查 MCP server。" : "Open a project before inspecting an MCP server."); await api.inspectConfiguredMcpServer(selected.id, serverId); setMcpServers(await api.listMcpServers()); }} onSetMcpServerEnabled={async (serverId, enabled) => { const updated = await api.setMcpServerEnabled(serverId, enabled); setMcpServers(await api.listMcpServers()); return updated; }} onSetMcpLaunchApproval={async (serverId, approved) => { const updated = await api.setMcpLaunchApproval(serverId, approved); setMcpServers(await api.listMcpServers()); return updated; }} onSetMcpToolApproval={async (serverId, tool, approved) => { const updated = await api.setMcpToolApproval(serverId, tool, approved); setMcpServers(await api.listMcpServers()); return updated; }} /> : null;
  if (!selected) return <><ProjectLibrary onOpenSearch={openWorkspaceSearch} projects={projects} connections={connections} locale={locale} onLocaleChange={setLocale} onSettings={() => { setSettingsSection("general"); setSettingsNavigationKey((value) => value + 1); setSettingsOpen(true); }} onOpen={setSelected} onDelete={async (projectId) => { await api.deleteProject(projectId); setProjects((current) => current.filter((project) => project.id !== projectId)); }} onChooseLocalRoot={api.chooseProjectDirectory} onCreate={async ({ template, name, localRoot, connectionId, remoteRoot }) => { const project = await api.createProject({ name, description: "", local_root: localRoot, template, connection_id: connectionId, remote_root: remoteRoot }); setProjects((current) => [project, ...current]); setSelected(project); }} />{settings}{searchDialog}</>;
  async function changeConversationMode(nextMode: SessionAgentModeV4) {
    if (!selected || !conversation || nextMode === conversationMode) return;
    const projectId = selected.id;
    const conversationId = conversation.id;
    const actionKey = `mode:${projectId}:${conversationId}`;
    if (runActionGuards.current.has(actionKey)) return;
    if (!isCurrentConversation(projectId, conversationId, conversationRequestToken.current)) return;
    if (conversationLocked) {
      setAgentNotice(locale === "zh-CN" ? "当前计划正在处理中，请先批准、请求修改或取消计划。" : "The current plan is still active. Approve, request changes, or cancel it first.");
      return;
    }
    // Invalidate only older mode/snapshot reads. Conversation identity uses a
    // separate token so an in-flight hydration may still deliver messages and
    // events for this same conversation.
    const modeRequestToken = conversationRequestToken.current;
    const modeRequestSequence = ++conversationModeRequestSequence.current;
    ++conversationSnapshotRequestSequence.current;
    runActionGuards.current.add(actionKey);
    const previousMode = conversationMode;
    setConversationMode(nextMode);
    setConversationState((current) => current ? { ...current, mode: nextMode } : current);
    setAgentNotice("");
    try {
      const response = await api.setConversationAgentMode({ project_id: projectId, conversation_id: conversationId, mode: nextMode });
      if (!isCurrentConversation(projectId, conversationId, modeRequestToken)
        || modeRequestSequence !== conversationModeRequestSequence.current) return;
      setConversationMode(response.mode);
      setConversationState((current) => current ? { ...current, mode: response.mode } : current);
      try { await refreshConversationState(projectId, conversationId, modeRequestToken); } catch { /* the persisted response remains authoritative for mode */ }
    } catch (error) {
      if (!isCurrentConversation(projectId, conversationId, modeRequestToken)
        || modeRequestSequence !== conversationModeRequestSequence.current) return;
      setConversationMode(previousMode);
      setConversationState((current) => current ? { ...current, mode: previousMode } : current);
      setAgentNotice(error instanceof Error ? error.message : String(error));
      try { await refreshConversationState(projectId, conversationId, modeRequestToken); } catch { /* retain the rollback and original write error */ }
    } finally {
      runActionGuards.current.delete(actionKey);
    }
  }

  async function requestPlanRevision(feedback: string) {
    if (!selected || !conversation) return;
    const revision = latestPlanRevision;
    const targetRunId = revision?.run_id ?? v4Plan?.run_id ?? conversationState?.latest_run?.run_id;
    const targetPlanHash = revision?.plan_hash ?? v4Plan?.plan_hash;
    if (!targetRunId || !targetPlanHash || !feedback.trim()) return;
    if (revision && revision.status !== "pending") {
      setAgentNotice(locale === "zh-CN" ? "只能对最新的待审批计划请求修改。" : "Changes can only be requested for the latest pending plan.");
      return;
    }
    const projectId = selected.id;
    const conversationId = conversation.id;
    const token = conversationRequestToken.current;
    const revisionKey = revision?.revision ?? v4Plan?.plan_revision ?? "legacy";
    const actionKey = `request-revision:${targetRunId}:${revisionKey}`;
    if (runActionGuards.current.has(actionKey)) return;
    runActionGuards.current.add(actionKey);
    setAgentNotice("");
    try {
      await api.agentV4RequestPlanRevision({ run_id: targetRunId, plan_hash: targetPlanHash, feedback: feedback.trim() });
      // A request-change transition is one logical resume. Do not route this
      // through the old "regenerate" callback, which created a second run.
      await api.agentV4Resume(targetRunId);
      await refreshConversationState(projectId, conversationId, token);
    } catch (error) {
      if (isCurrentConversation(projectId, conversationId, token)) setAgentNotice(error instanceof Error ? error.message : String(error));
      try { await refreshConversationState(projectId, conversationId, token); } catch { /* preserve the original action error */ }
    } finally {
      runActionGuards.current.delete(actionKey);
    }
  }

  async function approvePlan() {
    const revision = latestPlanRevision;
    if (revision && revision.status !== "pending") {
      setAgentNotice(locale === "zh-CN" ? "只能批准最新的待审批计划。" : "Only the latest pending plan can be approved.");
      try { await refreshConversationState(); } catch { /* keep the current plan visible */ }
      return;
    }
    if (!v4Plan?.run_id || !v4Plan.approval_hash) return;
    const projectId = selected?.id;
    const conversationId = conversation?.id;
    const token = conversationRequestToken.current;
    const revisionNumber = revision?.revision ?? v4Plan.plan_revision ?? undefined;
    const actionKey = `approve:${v4Plan.run_id}:${revisionNumber ?? "legacy"}`;
    if (runActionGuards.current.has(actionKey)) return;
    runActionGuards.current.add(actionKey);
    setAgentNotice("");
    try {
      const approved = revisionNumber === undefined
        ? await api.agentV4ApprovePlan(v4Plan.run_id, v4Plan.approval_hash)
        : await api.agentV4ApprovePlan(v4Plan.run_id, v4Plan.approval_hash, revisionNumber);
      if (projectId && conversationId && !isCurrentConversation(projectId, conversationId, token)) return;
      setV4Plan(approved);
      setRunId(approved.run_id);
      setRunStartedAt(new Date().toISOString());
      setPlanApproved(true);
      setConversationMode("agent");
      setConversationState((current) => current ? {
        ...current,
        mode: "agent",
        locked: false,
        latest_plan_revision: current.latest_plan_revision ? { ...current.latest_plan_revision, status: "approved" } : current.latest_plan_revision,
        latest_run: { ...approved, session_mode: "agent" },
      } : current);
      const events = await api.agentV4Events(approved.run_id);
      if (!projectId || !conversationId || isCurrentConversation(projectId, conversationId, token)) {
        mergeAgentRunEvents(events);
      }
    } catch (error) {
      if (!projectId || !conversationId || isCurrentConversation(projectId, conversationId, token)) {
        setAgentNotice(error instanceof Error ? error.message : String(error));
        try { await refreshConversationState(projectId, conversationId, token); } catch { /* leave Plan/lock state intact */ }
      }
    } finally {
      runActionGuards.current.delete(actionKey);
    }
  }

  async function cancelRun() {
    const targetRunId = activePlanRunId;
    if (!targetRunId) return;
    const projectId = selected?.id;
    const conversationId = conversation?.id;
    const token = conversationRequestToken.current;
    const keepPlanMode = conversationMode === "plan" || conversationLocked || isActivePlanRevisionStatus(latestPlanRevision?.status);
    const actionKey = `cancel:${targetRunId}:${token}`;
    if (runActionGuards.current.has(actionKey)) return;
    runActionGuards.current.add(actionKey);
    setAgentNotice("");
    let cancelObservedTerminal = false;
    try {
      if (!projectId || !conversationId) return;
      const receipt = await agentStop.requestStop({ projectId, conversationId, runId: targetRunId });
      if (!receipt) throw new Error("Agent stop requires an active desktop conversation.");
      cancelObservedTerminal = receipt.status === "observed";
      if (isCurrentConversation(projectId, conversationId, token)) {
        if (cancelObservedTerminal) {
          agentStop.markTerminal(targetRunId);
          setRunStartedAt(null);
          setRunId((current) => current === targetRunId ? null : current);
        }
        // The receipt is the durable stop acknowledgement. Event polling is
        // only an immediate trace refresh; a transient read failure must not
        // turn an accepted stop into a second request with a new id.
        try {
          const events = await api.agentV4Events(targetRunId);
          if (!isCurrentConversation(projectId, conversationId, token)) return;
          const runEvents = events.filter((event) => event.run_id === targetRunId);
          mergeAgentRunEvents(runEvents);
          if (runEvents.some(isTerminalAgentEventV4)) {
            cancelObservedTerminal = true;
            agentStop.markTerminal(targetRunId);
            setRunStartedAt(null);
            setRunId((current) => current === targetRunId ? null : current);
          }
        } catch (error) {
          if (isCurrentConversation(projectId, conversationId, token)) reportRunPollingFailure(error);
        }
      }
    } catch (error) {
      if (!projectId || !conversationId || isCurrentConversation(projectId, conversationId, token)) setAgentNotice(error instanceof Error ? error.message : String(error));
    } finally {
      try {
        const snapshot = await refreshConversationState(projectId, conversationId, token);
        if (snapshot && keepPlanMode && isCurrentConversation(projectId!, conversationId!, token)) setConversationMode("plan");
        // Legacy/browser mocks may not expose a post-cancel run snapshot. Keep
        // the local direct-run controls until the persisted terminal event is
        // observed, while real snapshots remain authoritative.
        if (snapshot && !keepPlanMode && !cancelObservedTerminal && !snapshot.latest_run && !snapshot.latest_plan_revision) {
          setRunId(targetRunId);
          setRunStartedAt((current) => current ?? new Date().toISOString());
        }
      } catch (error) {
        if ((!projectId || !conversationId || isCurrentConversation(projectId, conversationId, token)) && !agentNotice) setAgentNotice(error instanceof Error ? error.message : String(error));
        if (keepPlanMode && (!projectId || !conversationId || isCurrentConversation(projectId, conversationId, token))) setConversationMode("plan");
      }
      runActionGuards.current.delete(actionKey);
    }
  }
  const currentConversationAction = captureConversationAction();
  return <><WorkspaceShell
    onOpenSearch={openWorkspaceSearch} searchRequest={searchRequest}
    onSearchRequestHandled={(key) => setSearchRequest((current) => current?.key === key ? null : current)}
    onSuggestFollowUps={api.agentV4SuggestFollowUps}
    capabilitySummary={capabilities.summary} capabilitiesLoading={capabilities.loading}
    capabilitiesError={capabilities.error} onRefreshCapabilities={capabilities.refresh}
    project={{ id: selected.id, name: selected.name, status: selected.status, template: selected.template }}
    workflowCatalogVersion={workflowCatalogVersion}
    locale={locale} onLocaleChange={setLocale} onOpenSettings={(section = "models") => { setSettingsSection(section); setSettingsOpen(true); }} onBackToProjects={() => setSelected(null)}
    conversations={conversations} activeConversationId={conversation?.id} onSelectConversation={selectConversation} onNewConversation={newConversation} onOpenBranch={openCreatedBranch} onDeleteConversation={deleteConversation}
    messages={messages} agentBusy={agentBusy} agentNotice={agentNotice} conversationLoadError={conversationLoadError} onRetryConversationLoad={() => setConversationLoadRetry((value) => value + 1)} modelLabel={activeModel?.model} activeModelProfile={activeModel} modelProfiles={modelProfiles}
    composerBusy={modelSelectionBusy || !conversation}
    modelPicker={<ApiModelPicker zh={locale === "zh-CN"} profiles={modelProfiles} activeProfileId={activeModelProfileId} disabled={agentBusy || conversationLocked || conversationHydrating || modelSelectionBusy || planLoading} onProfileChange={(id) => { if (!modelSelectionInFlight.current) setActiveModelProfileId(id); }} onManage={() => { setSettingsSection("models"); setSettingsOpen(true); }} onModelSelect={async (profile, model) => {
      if (modelSelectionInFlight.current || conversationLocked || conversationHydrating || agentBusy || profile.id !== activeModelProfileId) throw new Error("Model selection is currently locked");
      modelSelectionInFlight.current = true;
      setModelSelectionBusy(true);
      try {
        const updated = await api.saveModelProfile({ id: profile.id, label: profile.label, provider: profile.provider, base_url: profile.base_url, model });
        setModelProfiles((current) => current.map((item) => item.id === updated.id ? updated : item));
      } finally {
        modelSelectionInFlight.current = false;
        setModelSelectionBusy(false);
      }
    }} onReasoningEffortChange={async (profile, effort) => {
      if (modelSelectionInFlight.current || conversationLocked || conversationHydrating || agentBusy || planLoading || profile.id !== activeModelProfileId) throw new Error("Model selection is currently locked");
      modelSelectionInFlight.current = true;
      setModelSelectionBusy(true);
      try {
        const updated = await api.saveModelProfile({ id: profile.id, label: profile.label, provider: profile.provider, base_url: profile.base_url, model: profile.model, reasoning_effort: effort ?? null });
        setModelProfiles((current) => current.map((item) => item === profile ? updated : item));
      } finally {
        modelSelectionInFlight.current = false;
        setModelSelectionBusy(false);
      }
    }} />}
    agentMode={conversationMode} conversationLocked={conversationLocked} conversationHydrating={conversationHydrating} onAgentModeChange={changeConversationMode}
    latestPlanRevision={latestPlanRevision} v4Plan={v4Plan} planLoading={planLoading} planApproved={planApproved} canStartRun={false} runStarted={Boolean(runId && !currentRunAwaitsPlanApproval)} activeRunId={runId} activeRunLastActivityAt={activeRunLastActivityAt} agentRunEventsV4={agentRunEventsV4} agentTextPreview={agentTextPreview}
    guidanceAvailable={v4Plan?.session_mode === "agent" && !planApproved && latestPlanRevision?.run_id !== v4Plan?.run_id}
    computeBackends={computeBackends} computeBackendId={computeBackendId} containerImage={containerImage} autonomyMode={autonomyMode} approvalPolicy={approvalPolicy} computeEnvironment={computeEnvironment} computeBusy={computeBusy}
    onComputeBackendChange={setComputeBackendId} onContainerImageChange={setContainerImage} onAutonomyModeChange={setAutonomyMode} onApprovalPolicyChange={setApprovalPolicy} onComputeEnvironmentChange={setComputeEnvironment}
    onAnswerAgentQuestionV4={async (answerRunId, questionId, answer) => {
      const action = currentConversationAction;
      if (!action || !isCurrentConversationAction(action)) return;
      const actionKey = `answer:${action.token}:${answerRunId}:${questionId}`;
      if (runActionGuards.current.has(actionKey)) return;
      runActionGuards.current.add(actionKey);
      setAgentNotice("");
      try {
        await api.agentV4Answer(answerRunId, questionId, answer);
        await api.agentV4Resume(answerRunId);
        if (!isCurrentConversationAction(action)) return;
        setRunId(answerRunId);
        setRunStartedAt((current) => current ?? new Date().toISOString());
        const events = await api.agentV4Events(answerRunId);
        if (!isCurrentConversationAction(action)) return;
        mergeAgentRunEvents(events);
      } catch (error) {
        if (isCurrentConversationAction(action)) setAgentNotice(error instanceof Error ? error.message : String(error));
      } finally {
        runActionGuards.current.delete(actionKey);
      }
    }}
    onDecideToolApprovalV4={async (approvalRunId, approvalId, callHash, decision, browserScope) => {
      const action = currentConversationAction;
      if (!action || !isCurrentConversationAction(action)) return;
      const actionKey = `approval:${action.token}:${approvalRunId}:${approvalId}`;
      if (runActionGuards.current.has(actionKey)) return;
      runActionGuards.current.add(actionKey);
      setAgentNotice("");
      try {
        await api.agentV4DecideToolApproval(approvalRunId, approvalId, callHash, decision, browserScope);
        await api.agentV4Resume(approvalRunId);
        if (!isCurrentConversationAction(action)) return;
        setRunId(approvalRunId);
        setRunStartedAt((current) => current ?? new Date().toISOString());
        const events = await api.agentV4Events(approvalRunId);
        if (!isCurrentConversationAction(action)) return;
        mergeAgentRunEvents(events);
      } catch (error) {
        if (isCurrentConversationAction(action)) setAgentNotice(error instanceof Error ? error.message : String(error));
      } finally {
        runActionGuards.current.delete(actionKey);
      }
    }}
    onCloseBrowserRunTabsV4={async (browserRunId, sessions) => {
      setAgentNotice("");
      try {
        await Promise.all([...new Set(sessions)].map((session) => api.browserCloseRunTabs(session, browserRunId)));
      } catch (error) {
        setAgentNotice(error instanceof Error ? error.message : String(error));
        throw error;
      }
    }}
    onResolveUncertainV4={async (uncertainRunId, callId, resolution, evidence) => {
      const action = currentConversationAction;
      if (!action || !isCurrentConversationAction(action)) return;
      const actionKey = `uncertain:${action.token}:${uncertainRunId}:${callId}`;
      if (runActionGuards.current.has(actionKey)) return;
      runActionGuards.current.add(actionKey);
      setAgentNotice("");
      try {
        await api.agentV4ResolveUncertain(uncertainRunId, callId, resolution, evidence);
        await api.agentV4Resume(uncertainRunId);
        if (!isCurrentConversationAction(action)) return;
        setRunId(uncertainRunId);
        setRunStartedAt((current) => current ?? new Date().toISOString());
        const events = await api.agentV4Events(uncertainRunId);
        if (!isCurrentConversationAction(action)) return;
        mergeAgentRunEvents(events);
      } catch (error) {
        if (isCurrentConversationAction(action)) setAgentNotice(error instanceof Error ? error.message : String(error));
      } finally {
        runActionGuards.current.delete(actionKey);
      }
    }}
    onCancelRuntimeRecoveryV4={async (cancelRunId) => {
      const action = currentConversationAction;
      if (!action || !isCurrentConversationAction(action)) return;
      const actionKey = `resume:${action.token}:${cancelRunId}`;
      if (runActionGuards.current.has(actionKey)) return;
      runActionGuards.current.add(actionKey);
      setAgentNotice("");
      try {
        await api.agentV4CancelRuntimeRecovery(cancelRunId);
        if (!isCurrentConversationAction(action)) return;
        const events = await api.agentV4Events(cancelRunId);
        if (isCurrentConversationAction(action)) mergeAgentRunEvents(events);
      } catch (error) {
        if (isCurrentConversationAction(action)) setAgentNotice(error instanceof Error ? error.message : String(error));
        throw error;
      } finally { runActionGuards.current.delete(actionKey); }
    }}
    onResumeAgentRunV4={async (resumeRunId) => {
      const action = currentConversationAction;
      if (!action || !isCurrentConversationAction(action)) return;
      const actionKey = `resume:${action.token}:${resumeRunId}`;
      if (runActionGuards.current.has(actionKey)) return;
      runActionGuards.current.add(actionKey);
      setAgentNotice("");
      try {
        await api.agentV4Resume(resumeRunId);
        if (!isCurrentConversationAction(action)) return;
        setRunId(resumeRunId);
        setRunStartedAt((current) => current ?? new Date().toISOString());
        const events = await api.agentV4Events(resumeRunId);
        if (!isCurrentConversationAction(action)) return;
        mergeAgentRunEvents(events);
      } catch (error) {
        if (isCurrentConversationAction(action)) setAgentNotice(error instanceof Error ? error.message : String(error));
      } finally {
        runActionGuards.current.delete(actionKey);
      }
    }}
    runStopping={runStopping}
    remoteFiles={remoteFiles} filesBusy={filesBusy} fileNotice={fileNotice}
    syncEntries={syncEntries}
    onPauseSync={async (id) => { await api.pauseSyncTransfer(id); setSyncEntries(await api.listSyncEntries(selected.id)); }}
    onCancelSync={async (id) => { await api.cancelSyncTransfer(id); setSyncEntries(await api.listSyncEntries(selected.id)); }}
    onRetrySync={async (id) => { await api.retrySyncTransfer(id); setSyncEntries(await api.listSyncEntries(selected.id)); }}
    kernelSessions={kernelSessions} kernelEvents={kernelEvents} kernelBusy={kernelBusy} kernelNotice={kernelNotice}
    memoryFacts={memoryFacts} notebookEntries={notebookEntries} projectArtifacts={projectArtifacts}
    onSearchMemory={async (query, dimension) => { const facts = await api.searchAgentMemory(selected.id, query, dimension, conversation?.id); setMemoryFacts(facts); }}
    onExportNotebook={async (format) => { const extension = format === "bundle" ? "omicsops.zip" : format === "json" ? "json" : "md"; const path = await api.chooseDownloadPath(`${selected.name}.${extension}`); if (path) await api.exportProjectNotebook(selected.id, format, path); }}
    onStartKernel={selected.connection_id && selected.remote_root ? startKernel : undefined}
    onExecuteKernel={async (sessionId, code, save, capturePaths) => { let savedIndex: number | null = null; await withKernelBusy(async () => { const result = await api.executeKernelCell(sessionId, code, save, capturePaths); savedIndex = result.saved_cell_index; setKernelEvents((current) => { const known = new Set(current.map((event) => `${event.request_id}:${event.sequence}`)); return [...current, ...result.events.filter((event) => !known.has(`${event.request_id}:${event.sequence}`))].slice(-200); }); }); return savedIndex; }}
    onInterruptKernel={async (sessionId) => withKernelBusy(async () => replaceKernelSession(await api.interruptKernel(sessionId)))}
    onStopKernel={async (sessionId) => withKernelBusy(async () => replaceKernelSession(await api.stopKernel(sessionId)))} onPromoteKernelCell={api.promoteKernelCell}
    onUploadFiles={selected.connection_id && selected.remote_root ? uploadFiles : undefined} onRefreshFiles={selected.connection_id && selected.remote_root ? () => refreshRemoteFiles() : undefined} onDownloadFile={selected.connection_id && selected.remote_root ? downloadFile : undefined}
    onPreviewImage={selected.connection_id && selected.remote_root ? (relativePath) => api.previewProjectImage(selected.id, relativePath) : undefined}
    branchSendOriginalMarkdown={branchSend.originalMarkdown} branchSendBusy={branchSend.busy} branchSendPending={branchSend.pending} branchSendError={branchSend.error} onRetryBranchSend={branchSend.retry}
    onBranchSend={nativeQueueAvailable ? async (sourceMessageId, markdown, mode, references, attachments) => {
      if (!selected || !conversation || !activeModel || conversationHydratingRef.current) return false;
      return branchSend.submit({ sourceMessageId, message_markdown: markdown, mode: mode === "plan" ? "plan" : "agent", model_profile_id: activeModel.id, compute_selection: currentComputeSelection(), references, attachments });
    } : undefined}
    contextUsage={contextUsage.value} contextUsageError={contextUsage.error}
    sideChat={nativeQueueAvailable ? sideChat : undefined}
    queueItems={queue.items} queueError={queue.error} queueLoading={queue.loading} onQueueRefresh={queue.refresh} onQueueUpdate={queue.update} onQueueAction={queue.action}
    replacement={nativeQueueAvailable ? { busy: replacement.busy, pending: replacement.pending, error: replacement.error, originalMarkdown: replacement.originalTurn?.message_markdown, retry: replacement.retry, send: async (markdown, mode, target, references, attachments) => {
      if (!selected || !conversation || !activeModel) return false;
      return replacement.send({ project_id: selected.id, conversation_id: conversation.id, mode: mode === "plan" ? "plan" : "agent", message_markdown: markdown, model_profile_id: activeModel.id, compute_selection: currentComputeSelection(), references, attachments }, target);
    } } : undefined}
    onQueue={nativeQueueAvailable ? async (markdown, mode, references = [], attachments = []) => {
      if (!conversation || !activeModel || conversationHydratingRef.current || modelSelectionInFlight.current) return false;
      return queue.enqueue({ project_id: selected.id, conversation_id: conversation.id, mode: mode === "plan" ? "plan" : "agent", message_markdown: markdown, model_profile_id: activeModel.id, compute_selection: currentComputeSelection(), references, attachments });
    } : undefined}
    onSend={async (markdown, mode, references = [], attachments = []) => {
      if (!conversation || !activeModel) { setSettingsOpen(true); return false; }
      if (conversationHydratingRef.current || conversationLocked || modelSelectionInFlight.current) return false;
      const projectId = selected.id;
      const conversationId = conversation.id;
      // The send is a newer conversation transition than any reconnect read
      // that may still be in flight.
      ++conversationRequestToken.current;
      const token = conversationRequestToken.current;
      lastGoalRef.current = markdown;
      setLastGoal(markdown); setV4Plan(null); setPlanApproved(false); setRunId(null); setRunStartedAt(null); setAgentBusy(true); setAgentNotice("");
      if (mode === "plan") {
        setPlanLoading(true);
        try {
          if (references.length) await validateComposerReferences(projectId, conversationId, references);
          if (attachments.length) await validateComposerAttachments(projectId, conversationId, attachments, activeModel?.id);
          if (!isCurrentConversation(projectId, conversationId, token)) return false;
          const pending = pendingSubmissionRef.current;
          const reusableMessage = pending && samePendingSubmission(pending, projectId, conversationId, markdown, mode, references, attachments)
            ? pending.message
            : null;
          const message = reusableMessage ?? await api.submitMessage({ project_id: projectId, conversation_id: conversationId, markdown, sequence: messageSequence });
          if (!isCurrentConversation(projectId, conversationId, token)) return false;
          setMessages((current) => {
            const next = current.some((item) => item.id === message.id) ? current : [...current, message];
            messagesRef.current = next;
            return next;
          });
          setMessageSequence((value) => {
            const next = Math.max(value, messageSequenceRef.current, message.sequence + 1);
            messageSequenceRef.current = next;
            return next;
          });
          if (!reusableMessage) {
            pendingSubmissionRef.current = { projectId, conversationId, markdown, mode, references: [...references], attachments: [...attachments], message };
          }
          await startV4Planning(markdown, token, references, attachments);
          if (!isCurrentConversation(projectId, conversationId, token)) return false;
          if (pendingSubmissionRef.current && samePendingSubmission(pendingSubmissionRef.current, projectId, conversationId, markdown, mode, references, attachments)) {
            pendingSubmissionRef.current = null;
          }
          return true;
        } catch (error) {
          if (isCurrentConversation(projectId, conversationId, token)) setAgentNotice(error instanceof Error ? error.message : String(error));
          return false;
        } finally {
          if (isCurrentConversation(projectId, conversationId, token)) {
            setPlanLoading(false);
            setAgentBusy(false);
          }
        }
      }
      try {
        if (references.length) await validateComposerReferences(projectId, conversationId, references);
          if (attachments.length) await validateComposerAttachments(projectId, conversationId, attachments, activeModel?.id);
        if (!isCurrentConversation(projectId, conversationId, token)) return false;
        const pending = pendingSubmissionRef.current;
        const reusableMessage = pending && samePendingSubmission(pending, projectId, conversationId, markdown, mode, references, attachments)
          ? pending.message
          : null;
        const message = reusableMessage ?? await api.submitMessage({ project_id: projectId, conversation_id: conversationId, markdown, sequence: messageSequence });
        if (!isCurrentConversation(projectId, conversationId, token)) return false;
        setMessages((current) => {
          const next = current.some((item) => item.id === message.id) ? current : [...current, message];
          messagesRef.current = next;
          return next;
        });
        setMessageSequence((value) => {
          const next = Math.max(value, messageSequenceRef.current, message.sequence + 1);
          messageSequenceRef.current = next;
          return next;
        });
        if (!reusableMessage) {
          pendingSubmissionRef.current = { projectId, conversationId, markdown, mode, references: [...references], attachments: [...attachments], message };
        }
        await startV4Direct(markdown, token, references, attachments);
        if (!isCurrentConversation(projectId, conversationId, token)) return false;
        if (pendingSubmissionRef.current && samePendingSubmission(pendingSubmissionRef.current, projectId, conversationId, markdown, mode, references, attachments)) {
          pendingSubmissionRef.current = null;
        }
        return true;
      } catch (error) {
        if (isCurrentConversation(projectId, conversationId, token)) setAgentNotice(error instanceof Error ? error.message : String(error));
        return false;
      } finally {
        if (isCurrentConversation(projectId, conversationId, token)) setAgentBusy(false);
      }
    }}
    onApprovePlan={approvePlan}
    onRequestPlanRevision={requestPlanRevision}
    onCancelRun={activePlanRunId ? cancelRun : undefined}
  />{settings}{searchDialog}</>;
}
function mergeAgentRunEventsV4(current: AgentRunEventV4[], incoming: AgentRunEventV4[]) {
  return [...current, ...incoming]
    .filter((event, index, all) => all.findIndex((item) => item.run_id === event.run_id && item.sequence === event.sequence) === index)
    .sort((left, right) => new Date(left.occurred_at).getTime() - new Date(right.occurred_at).getTime() || left.sequence - right.sequence);
}

function mergeWorkspaceMessages(current: WorkspaceMessage[], incoming: WorkspaceMessage[]) {
  const byId = new Map<string, WorkspaceMessage>();
  for (const message of incoming) byId.set(message.id, message);
  // Live arrivals win over a stale hydration copy with the same id, while
  // messages that arrived during the read are retained in the merged list.
  for (const message of current) byId.set(message.id, message);
  return [...byId.values()].sort((left, right) =>
    left.sequence - right.sequence
    || new Date(left.created_at).getTime() - new Date(right.created_at).getTime()
    || left.id.localeCompare(right.id));
}

function latestAgentRunEventV4(events: AgentRunEventV4[]) {
  return [...events].sort((left, right) => left.sequence - right.sequence || new Date(left.occurred_at).getTime() - new Date(right.occurred_at).getTime()).at(-1);
}

function subscriptionError(channel: string, error: unknown) {
  const detail = error instanceof Error ? error.message : String(error);
  return `${channel} event subscription failed: ${detail}`;
}

function isTerminalAgentEventV4(event: AgentRunEventV4) {
  return event.event.kind === "run_completed" || event.event.kind === "run_failed" || event.event.kind === "run_cancelled" || event.event.kind === "run_needs_attention";
}

function eventChangesConversationState(kind: AgentRunEventV4["event"]["kind"]) {
  return kind === "run_created"
    || kind === "plan_proposed"
    || kind === "plan_approved"
    || kind === "plan_revision_requested"
    || kind === "mode_changed"
    || kind === "run_completed"
    || kind === "run_failed"
    || kind === "run_cancelled"
    || kind === "run_needs_attention";
}

function isActivePlanRevisionStatus(status: ProposedPlanRevisionV4["status"] | undefined) {
  return status === "generating" || status === "revising" || status === "pending";
}

function isTerminalRunStatus(status: string) {
  return status === "completed" || status === "cancelled" || status === "failed" || status === "needs_attention";
}
