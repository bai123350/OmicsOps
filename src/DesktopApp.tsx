import { useEffect, useRef, useState } from "react";
import * as api from "./tauri-api";
import type { AgentRunEventV4, ApprovalPolicyV4, AutonomyModeV4, ComputeBackendAvailabilityV4, ComputeSelectionV4, ConnectionProfile, KernelEvent, KernelLanguage, KernelSession, McpServerProfile, MemoryFact, ModelProfile, NotebookEntry, ProjectArtifact, RemoteFileEntry, RunSummaryV4, SkillPackage, SyncEntry, WorkspaceConversation, WorkspaceMessage, WorkspaceProject } from "./types";
import { ProjectLibrary } from "./features/projects/ProjectLibrary";
import { WorkspaceShell } from "./features/workspace/WorkspaceShell";
import type { Locale } from "./features/workspace/copy";
import { SettingsPanel } from "./features/settings/SettingsPanel";

export default function DesktopApp() {
  const [projects, setProjects] = useState<WorkspaceProject[]>([]);
  const [selected, setSelected] = useState<WorkspaceProject | null>(null);
  const [locale, setLocale] = useState<Locale>("zh-CN");
  const [loading, setLoading] = useState(true);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [conversations, setConversations] = useState<WorkspaceConversation[]>([]);
  const [conversation, setConversation] = useState<WorkspaceConversation | null>(null);
  const [messageSequence, setMessageSequence] = useState(1);
  const [messages, setMessages] = useState<WorkspaceMessage[]>([]);
  const [agentBusy, setAgentBusy] = useState(false);
  const [agentNotice, setAgentNotice] = useState("");
  const [modelProfiles, setModelProfiles] = useState<ModelProfile[]>([]);
  const [activeModelProfileId, setActiveModelProfileId] = useState<string | null>(null);
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
  const [runStopping, setRunStopping] = useState(false);
  const [agentRunEventsV4, setAgentRunEventsV4] = useState<AgentRunEventV4[]>([]);
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
  const runActionGuards = useRef(new Set<string>());
  const runPollingNotice = useRef("");

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
    let disposed = false;
    if (!selected) { setConversations([]); setConversation(null); return () => { disposed = true; }; }
    api.listConversations(selected.id).then(async (items) => {
      const active = items[0] ?? await api.createConversation(selected.id);
      if (disposed) return;
      setConversations(items.length ? items : [active]);
      setConversation(active);
      const storedMessages = await api.listMessages(active.id);
      setMessages(storedMessages);
      setMessageSequence((storedMessages.at(-1)?.sequence ?? 0) + 1);
      setLastGoal([...storedMessages].reverse().find((message) => message.role === "user")?.markdown ?? "");
    }).catch((error) => { if (!disposed) setAgentNotice(error instanceof Error ? error.message : String(error)); });
    return () => { disposed = true; };
  }, [selected?.id]);
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
    api.listKernelSessions()
      .then((sessions) => setKernelSessions(sessions.filter((session) => session.project_id === selected.id)))
      .catch((error) => setKernelNotice(error instanceof Error ? error.message : String(error)));
  }, [selected?.id]);
  useEffect(() => {
    let disposed = false;
    setAgentRunEventsV4([]);
    setRunId(null);
    setRunStartedAt(null);
    setRunStopping(false);
    if (!selected || !conversation) return () => { disposed = true; };
    api.agentV4EventsForConversation(selected.id, conversation.id)
      .then((eventsV4) => {
        if (disposed) return;
        clearRecoveredRunPollingFailure();
        setAgentRunEventsV4(mergeAgentRunEventsV4([], eventsV4));
        const latest = eventsV4.map((event) => ({ runId: event.run_id, timestamp: event.occurred_at })).sort((left, right) => new Date(left.timestamp).getTime() - new Date(right.timestamp).getTime()).at(-1);
        const latestRunEventsV4 = latest ? eventsV4.filter((event) => event.run_id === latest.runId) : [];
        setRunId(latest && !latestRunEventsV4.some(isTerminalAgentEventV4) ? latest.runId : null);
        setRunStartedAt(latest && !latestRunEventsV4.some(isTerminalAgentEventV4) ? (latestAgentRunEventV4(latestRunEventsV4)?.occurred_at ?? latest.timestamp) : null);
      })
      .catch((error) => {
        if (!disposed) reportRunPollingFailure(error);
      });
    return () => { disposed = true; };
  }, [selected?.id, conversation?.id]);
  useEffect(() => {
    let disposed = false;
    const unlisten: Array<() => void> = [];
    api.onConversationEvent((event) => {
      if (event.conversation_id !== conversation?.id) return;
      setMessages((current) => current.some((message) => message.id === event.message.id) ? current : [...current, event.message]);
      setMessageSequence((value) => Math.max(value, event.message.sequence + 1));
    }).then((fn) => disposed ? fn() : unlisten.push(fn)).catch((error) => {
      if (!disposed) setAgentNotice(subscriptionError("conversation", error));
    });
    api.onConversationUpdated((event) => {
      if (event.project_id !== selected?.id) return;
      setConversations((current) => [event.conversation, ...current.filter((item) => item.id !== event.conversation.id)]);
      setConversation((current) => current?.id === event.conversation.id ? event.conversation : current);
    }).then((fn) => disposed ? fn() : unlisten.push(fn)).catch((error) => {
      if (!disposed) setAgentNotice(subscriptionError("conversation updates", error));
    });
    api.onKernelEvent((event) => {
      if (event.project_id !== selected?.id) return;
      setKernelEvents((current) => [...current.slice(-199), event]);
    }).then((fn) => disposed ? fn() : unlisten.push(fn)).catch((error) => {
      if (!disposed) setAgentNotice(subscriptionError("kernel", error));
    });
    api.onSyncEvent((entry) => {
      if (entry.project_id !== selected?.id) return;
      setSyncEntries((current) => [entry, ...current.filter((item) => item.id !== entry.id)]);
    }).then((fn) => disposed ? fn() : unlisten.push(fn)).catch((error) => {
      if (!disposed) setAgentNotice(subscriptionError("sync", error));
    });
    api.onAgentV4Event((event) => {
      if (event.project_id !== selected?.id || event.conversation_id !== conversation?.id) return;
      if (isTerminalAgentEventV4(event)) {
        setRunStopping(false);
        setRunStartedAt(null);
        setRunId((current) => current === event.run_id ? null : current);
        if (event.event.kind === "run_completed") void refreshRemoteFiles(event.project_id);
      } else {
        setRunId((current) => current ?? event.run_id);
        setRunStartedAt((current) => current ?? event.occurred_at);
      }
      setAgentRunEventsV4((current) => mergeAgentRunEventsV4(current, [event]));
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
        setAgentRunEventsV4((current) => mergeAgentRunEventsV4(current, runEvents));
        if (runEvents.some(isTerminalAgentEventV4)) {
          setRunStopping(false);
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
  function resetConversationWork() {
    setMessages([]); setMessageSequence(1); setAgentBusy(false); setAgentNotice("");
    setLastGoal(""); setV4Plan(null); setPlanApproved(false); setRunId(null); setRunStartedAt(null); setRunStopping(false); setAgentRunEventsV4([]);
  }

  function currentComputeSelection(): ComputeSelectionV4 {
    const backend = computeBackends.find((item) => item.descriptor.backend_id === computeBackendId);
    if (!backend?.selectable) throw new Error(locale === "zh-CN" ? "请选择一个可用的 V4 计算后端。" : "Select an available V4 compute backend.");
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

  async function startV4Planning(goal: string) {
    if (!selected || !conversation || !activeModel) throw new Error(locale === "zh-CN" ? "请先选择会话和模型。" : "Select a conversation and model first.");
    const summary = await api.agentV4StartPlanning({
      project_id: selected.id,
      conversation_id: conversation.id,
      model_profile_id: activeModel.id,
      objective: goal,
      compute_selection: currentComputeSelection(),
    });
    setV4Plan(summary);
    setRunId(summary.run_id);
    setRunStartedAt(new Date().toISOString());
    setPlanApproved(false);
    const events = await api.agentV4Events(summary.run_id);
    setAgentRunEventsV4((current) => mergeAgentRunEventsV4(current, events));
    return summary;
  }

  async function startV4Direct(goal: string) {
    if (!selected || !conversation || !activeModel) throw new Error(locale === "zh-CN" ? "请先选择会话和模型。" : "Select a conversation and model first.");
    const summary = await api.agentV4StartDirect({
      project_id: selected.id,
      conversation_id: conversation.id,
      model_profile_id: activeModel.id,
      objective: goal,
      compute_selection: currentComputeSelection(),
    });
    setV4Plan(summary);
    setRunId(summary.run_id);
    setRunStartedAt(new Date().toISOString());
    setPlanApproved(false);
    const events = await api.agentV4Events(summary.run_id);
    setAgentRunEventsV4((current) => mergeAgentRunEventsV4(current, events));
    return summary;
  }

  async function selectConversation(conversationId: string) {
    const next = conversations.find((item) => item.id === conversationId);
    if (!next || next.id === conversation?.id) return;
    resetConversationWork();
    setConversation(next);
    try {
      const storedMessages = await api.listMessages(next.id);
      setMessages(storedMessages);
      setMessageSequence((storedMessages.at(-1)?.sequence ?? 0) + 1);
      setLastGoal([...storedMessages].reverse().find((message) => message.role === "user")?.markdown ?? "");
    } catch (error) {
      setAgentNotice(error instanceof Error ? error.message : String(error));
    }
  }

  async function newConversation() {
    if (!selected || agentBusy) return;
    setAgentNotice("");
    try {
      const created = await api.createConversation(selected.id);
      setConversations((current) => [created, ...current]);
      resetConversationWork();
      setConversation(created);
    } catch (error) {
      setAgentNotice(error instanceof Error ? error.message : String(error));
    }
  }

  async function deleteConversation(conversationId: string) {
    if (!selected || agentBusy || (conversation?.id === conversationId && runId)) return;
    setAgentNotice("");
    try {
      await api.deleteConversation(selected.id, conversationId);
      const remaining = conversations.filter((item) => item.id !== conversationId);
      if (conversation?.id !== conversationId) {
        setConversations(remaining);
        return;
      }
      const next = remaining[0] ?? await api.createConversation(selected.id);
      setConversations(remaining.length > 0 ? remaining : [next]);
      resetConversationWork();
      setConversation(next);
      if (remaining.length > 0) {
        const storedMessages = await api.listMessages(next.id);
        setMessages(storedMessages);
        setMessageSequence((storedMessages.at(-1)?.sequence ?? 0) + 1);
        setLastGoal([...storedMessages].reverse().find((message) => message.role === "user")?.markdown ?? "");
      }
    } catch (error) {
      setAgentNotice(error instanceof Error ? error.message : String(error));
    }
  }
  if (loading) return <div className="desktop-loading">OmicsOps</div>;
  const settings = settingsOpen ? <SettingsPanel locale={locale} onClose={() => setSettingsOpen(false)} modelProfiles={modelProfiles} skillPackages={skillPackages} mcpServers={mcpServers} connections={connections} selectedProject={selected} onSaveConnection={async (profile, secret) => { await api.saveConnection(profile, secret); setConnections(await api.listConnections()); }} onTestConnection={api.testConnection} onConfirmHostKey={async (profileId, fingerprint) => { await api.confirmHostKey(profileId, fingerprint); setConnections(await api.listConnections()); }} onBindProjectRemote={async (connectionId, remoteRoot) => { if (!selected) return; const updated = await api.updateProjectRemote(selected.id, connectionId, remoteRoot); setSelected(updated); setProjects((current) => current.map((project) => project.id === updated.id ? updated : project)); }} onSaveModel={async (request) => { const profile = await api.saveModelProfile(request); setModelProfiles((current) => [profile, ...current.filter((item) => item.id !== profile.id)]); setActiveModelProfileId(profile.id); }} onProbeModel={api.probeModelProfile} onListModels={api.listModelProfileModels} onImportSkill={async () => { const sourcePath = await api.chooseSkillDirectory(); if (!sourcePath) return; const skill = await api.importSkillDirectory(sourcePath); setSkillPackages((current) => [skill, ...current.filter((item) => item.id !== skill.id)]); }} onSetSkillEnabled={async (skillId, enabled) => { const updated = await api.setSkillEnabled(skillId, enabled); setSkillPackages(await api.listSkillPackages()); return updated; }} onSaveMcpServer={async (request) => { const updated = await api.saveMcpServer(request); setMcpServers(await api.listMcpServers()); return updated; }} onAddPubMedMcp={async (request) => { const updated = await api.addPubMedMcpServer(request); setMcpServers(await api.listMcpServers()); return updated; }} onInspectMcpServer={async (serverId) => { if (!selected) throw new Error(locale === "zh-CN" ? "请先打开一个项目，再检查 MCP server。" : "Open a project before inspecting an MCP server."); await api.inspectConfiguredMcpServer(selected.id, serverId); setMcpServers(await api.listMcpServers()); }} onSetMcpServerEnabled={async (serverId, enabled) => { const updated = await api.setMcpServerEnabled(serverId, enabled); setMcpServers(await api.listMcpServers()); return updated; }} onSetMcpToolApproval={async (serverId, tool, approved) => { const updated = await api.setMcpToolApproval(serverId, tool, approved); setMcpServers(await api.listMcpServers()); return updated; }} /> : null;
  if (!selected) return <><ProjectLibrary projects={projects} connections={connections} locale={locale} onLocaleChange={setLocale} onSettings={() => setSettingsOpen(true)} onOpen={setSelected} onDelete={async (projectId) => { await api.deleteProject(projectId); setProjects((current) => current.filter((project) => project.id !== projectId)); }} onChooseLocalRoot={api.chooseProjectDirectory} onCreate={async ({ template, name, localRoot, connectionId, remoteRoot }) => { const project = await api.createProject({ name, description: "", local_root: localRoot, template, connection_id: connectionId, remote_root: remoteRoot }); setProjects((current) => [project, ...current]); setSelected(project); }} />{settings}</>;
  const activeModel = modelProfiles.find((profile) => profile.id === activeModelProfileId) ?? null;
  const activeRunLastActivityAt = runId
    ? latestAgentRunEventV4(agentRunEventsV4.filter((event) => event.run_id === runId))?.occurred_at ?? runStartedAt
    : null;
  const currentRunEventsV4 = runId ? agentRunEventsV4.filter((event) => event.run_id === runId) : [];
  const currentRunAwaitsPlanApproval = v4Plan?.status === "awaiting_approval"
    || (currentRunEventsV4.some((event) => event.event.kind === "plan_proposed")
      && !currentRunEventsV4.some((event) => event.event.kind === "mode_changed" && event.event.mode === "execute"));
  return <><WorkspaceShell
    project={{ id: selected.id, name: selected.name, status: selected.status, template: selected.template }}
    locale={locale} onLocaleChange={setLocale} onOpenSettings={() => setSettingsOpen(true)} onBackToProjects={() => setSelected(null)}
    conversations={conversations} activeConversationId={conversation?.id} onSelectConversation={selectConversation} onNewConversation={newConversation} onDeleteConversation={deleteConversation}
    messages={messages} agentBusy={agentBusy} agentNotice={agentNotice} modelLabel={activeModel?.label}
    v4Plan={v4Plan} planLoading={planLoading} planApproved={planApproved} canStartRun={false} runStarted={Boolean(runId && !currentRunAwaitsPlanApproval)} activeRunId={runId} activeRunLastActivityAt={activeRunLastActivityAt} agentRunEventsV4={agentRunEventsV4}
    computeBackends={computeBackends} computeBackendId={computeBackendId} containerImage={containerImage} autonomyMode={autonomyMode} approvalPolicy={approvalPolicy} computeEnvironment={computeEnvironment} computeBusy={computeBusy}
    onComputeBackendChange={setComputeBackendId} onContainerImageChange={setContainerImage} onAutonomyModeChange={setAutonomyMode} onApprovalPolicyChange={setApprovalPolicy} onComputeEnvironmentChange={setComputeEnvironment}
    onAnswerAgentQuestionV4={async (answerRunId, questionId, answer) => {
      const actionKey = `answer:${answerRunId}:${questionId}`;
      if (runActionGuards.current.has(actionKey)) return;
      runActionGuards.current.add(actionKey);
      setAgentNotice("");
      try {
        await api.agentV4Answer(answerRunId, questionId, answer);
        await api.agentV4Resume(answerRunId);
        setRunId(answerRunId);
        setRunStartedAt((current) => current ?? new Date().toISOString());
        const events = await api.agentV4Events(answerRunId);
        setAgentRunEventsV4((current) => mergeAgentRunEventsV4(current, events));
      } catch (error) {
        setAgentNotice(error instanceof Error ? error.message : String(error));
      } finally {
        runActionGuards.current.delete(actionKey);
      }
    }}
    onDecideToolApprovalV4={async (approvalRunId, approvalId, callHash, decision) => {
      const actionKey = `approval:${approvalRunId}:${approvalId}`;
      if (runActionGuards.current.has(actionKey)) return;
      runActionGuards.current.add(actionKey);
      setAgentNotice("");
      try {
        await api.agentV4DecideToolApproval(approvalRunId, approvalId, callHash, decision);
        await api.agentV4Resume(approvalRunId);
        setRunId(approvalRunId);
        setRunStartedAt((current) => current ?? new Date().toISOString());
        const events = await api.agentV4Events(approvalRunId);
        setAgentRunEventsV4((current) => mergeAgentRunEventsV4(current, events));
      } catch (error) {
        setAgentNotice(error instanceof Error ? error.message : String(error));
      } finally {
        runActionGuards.current.delete(actionKey);
      }
    }}
    onResolveUncertainV4={async (uncertainRunId, callId, resolution, evidence) => {
      const actionKey = `uncertain:${uncertainRunId}:${callId}`;
      if (runActionGuards.current.has(actionKey)) return;
      runActionGuards.current.add(actionKey);
      setAgentNotice("");
      try {
        await api.agentV4ResolveUncertain(uncertainRunId, callId, resolution, evidence);
        await api.agentV4Resume(uncertainRunId);
        setRunId(uncertainRunId);
        setRunStartedAt((current) => current ?? new Date().toISOString());
        const events = await api.agentV4Events(uncertainRunId);
        setAgentRunEventsV4((current) => mergeAgentRunEventsV4(current, events));
      } catch (error) {
        setAgentNotice(error instanceof Error ? error.message : String(error));
      } finally {
        runActionGuards.current.delete(actionKey);
      }
    }}
    onResumeAgentRunV4={async (resumeRunId) => {
      const actionKey = `resume:${resumeRunId}`;
      if (runActionGuards.current.has(actionKey)) return;
      runActionGuards.current.add(actionKey);
      setAgentNotice("");
      try {
        await api.agentV4Resume(resumeRunId);
        setRunId(resumeRunId);
        setRunStartedAt((current) => current ?? new Date().toISOString());
        const events = await api.agentV4Events(resumeRunId);
        setAgentRunEventsV4((current) => mergeAgentRunEventsV4(current, events));
      } catch (error) {
        setAgentNotice(error instanceof Error ? error.message : String(error));
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
    onSend={async (markdown, mode) => {
      if (!conversation || !activeModel) { setSettingsOpen(true); return false; }
      setLastGoal(markdown); setV4Plan(null); setPlanApproved(false); setRunId(null); setRunStartedAt(null); setAgentBusy(true); setAgentNotice("");
      if (mode === "plan") {
        setPlanLoading(true);
        try {
          const message = await api.submitMessage({ project_id: selected.id, conversation_id: conversation.id, markdown, sequence: messageSequence });
          setMessages((current) => current.some((item) => item.id === message.id) ? current : [...current, message]);
          setMessageSequence((value) => Math.max(value, message.sequence + 1));
          await startV4Planning(markdown);
          return true;
        } catch (error) {
          setAgentNotice(error instanceof Error ? error.message : String(error));
          return false;
        } finally {
          setPlanLoading(false);
          setAgentBusy(false);
        }
      }
      try {
        const message = await api.submitMessage({ project_id: selected.id, conversation_id: conversation.id, markdown, sequence: messageSequence });
        setMessages((current) => current.some((item) => item.id === message.id) ? current : [...current, message]);
        setMessageSequence((value) => Math.max(value, message.sequence + 1));
        await startV4Direct(markdown);
        return true;
      } catch (error) {
        setAgentNotice(error instanceof Error ? error.message : String(error));
        return false;
      } finally { setAgentBusy(false); }
    }}
    onRequestPlan={async () => {
      if (!activeModel || !lastGoal) { if (!activeModel) setSettingsOpen(true); return; }
      setPlanLoading(true); setAgentNotice("");
      try { await startV4Planning(lastGoal); }
      catch (error) { setAgentNotice(error instanceof Error ? error.message : String(error)); }
      finally { setPlanLoading(false); }
    }}
    onApprovePlan={async () => {
      if (!v4Plan?.approval_hash) return;
      const actionKey = `approve:${v4Plan.run_id}`;
      if (runActionGuards.current.has(actionKey)) return;
      runActionGuards.current.add(actionKey);
      setAgentNotice("");
      try {
        const approved = await api.agentV4ApprovePlan(v4Plan.run_id, v4Plan.approval_hash);
        setV4Plan(approved);
        setRunId(approved.run_id);
        setRunStartedAt(new Date().toISOString());
        setPlanApproved(true);
        const events = await api.agentV4Events(approved.run_id);
        setAgentRunEventsV4((current) => mergeAgentRunEventsV4(current, events));
      } catch (error) {
        setAgentNotice(error instanceof Error ? error.message : String(error));
      } finally {
        runActionGuards.current.delete(actionKey);
      }
    }}
    onCancelRun={runId ? async () => {
      const targetRunId = runId;
      setRunStopping(true);
      setAgentNotice("");
      try {
        await api.agentV4Cancel(targetRunId);
        const events = await api.agentV4Events(targetRunId);
        const runEvents = events.filter((event) => event.run_id === targetRunId);
        setAgentRunEventsV4((current) => mergeAgentRunEventsV4(current, runEvents));
        if (runEvents.some(isTerminalAgentEventV4)) {
          setRunStartedAt(null);
          setRunId((current) => current === targetRunId ? null : current);
        }
      } catch (error) {
        setAgentNotice(error instanceof Error ? error.message : String(error));
      } finally {
        // Cancellation is asynchronous in the host. The reconciliation loop
        // will clear the run when the terminal event is persisted; this
        // fallback keeps the control usable if that event is delayed/lost.
        setRunStopping(false);
      }
    } : undefined}
  />{settings}</>;
}
function mergeAgentRunEventsV4(current: AgentRunEventV4[], incoming: AgentRunEventV4[]) {
  return [...current, ...incoming]
    .filter((event, index, all) => all.findIndex((item) => item.run_id === event.run_id && item.sequence === event.sequence) === index)
    .sort((left, right) => new Date(left.occurred_at).getTime() - new Date(right.occurred_at).getTime() || left.sequence - right.sequence);
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
