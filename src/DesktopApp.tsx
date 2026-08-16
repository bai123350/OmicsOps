import { useEffect, useState } from "react";
import * as api from "./tauri-api";
import type { AgentEvent, AgentRunStreamEvent, ConnectionProfile, KernelEvent, KernelLanguage, KernelSession, McpServerProfile, MemoryFact, ModelProfile, NotebookEntry, PlanProposal, ProjectArtifact, RemoteFileEntry, SkillPackage, SyncEntry, WorkspaceConversation, WorkspaceMessage, WorkspaceProject } from "./types";
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
  const [streamingAssistant, setStreamingAssistant] = useState("");
  const [agentBusy, setAgentBusy] = useState(false);
  const [agentNotice, setAgentNotice] = useState("");
  const [agentRetryNotice, setAgentRetryNotice] = useState("");
  const [modelProfiles, setModelProfiles] = useState<ModelProfile[]>([]);
  const [activeModelProfileId, setActiveModelProfileId] = useState<string | null>(null);
  const [lastGoal, setLastGoal] = useState("");
  const [planProposal, setPlanProposal] = useState<PlanProposal | null>(null);
  const [planLoading, setPlanLoading] = useState(false);
  const [planApproved, setPlanApproved] = useState(false);
  const [approvedPlanId, setApprovedPlanId] = useState<string | null>(null);
  const [runId, setRunId] = useState<string | null>(null);
  const [runStopping, setRunStopping] = useState(false);
  const [agentRunEvents, setAgentRunEvents] = useState<AgentRunStreamEvent[]>([]);
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
    if (!selected) { setMemoryFacts([]); setNotebookEntries([]); setProjectArtifacts([]); return; }
    void Promise.all([api.searchAgentMemory(selected.id), api.listNotebookEntries(selected.id), api.listProjectArtifacts(selected.id), api.listSyncEntries(selected.id)])
      .then(([facts, notebook, artifacts, transfers]) => { setMemoryFacts(facts); setNotebookEntries(notebook); setProjectArtifacts(artifacts); setSyncEntries(transfers); })
      .catch((error) => setAgentNotice(error instanceof Error ? error.message : String(error)));
  }, [selected?.id, agentRunEvents.at(-1)?.kind]);
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
    setAgentRunEvents([]);
    setRunId(null);
    setRunStopping(false);
    if (!selected || !conversation) return () => { disposed = true; };
    api.listAgentRunEvents(selected.id)
      .then((events) => {
        if (disposed) return;
        const conversationEvents = events.filter((event) => event.conversation_id === conversation.id);
        setAgentRunEvents(conversationEvents);
        const latestRunId = conversationEvents.at(-1)?.run_id;
        const latestRunEvents = latestRunId ? conversationEvents.filter((event) => event.run_id === latestRunId) : [];
        const latestRunFinished = latestRunEvents.some(isTerminalAgentEvent);
        setRunId(latestRunFinished ? null : latestRunId ?? null);
      })
      .catch((error) => {
        if (!disposed) setAgentNotice(error instanceof Error ? error.message : String(error));
      });
    return () => { disposed = true; };
  }, [selected?.id, conversation?.id]);
  useEffect(() => {
    let disposed = false;
    const unlisten: Array<() => void> = [];
    api.onAgentEvent((event: AgentEvent) => {
      if (event.conversation_id !== conversation?.id) return;
      if (event.event.kind === "turn-started") {
        setStreamingAssistant("");
        setAgentBusy(true);
        setAgentNotice("");
      }
      if (event.event.kind === "text-delta") {
        const delta = event.event.payload;
        setStreamingAssistant((value) => value + delta);
        setAgentRetryNotice("");
      }
      if (event.event.kind === "provider-retrying") {
        const seconds = Math.max(1, Math.ceil(event.event.payload.delay_ms / 1000));
        setAgentBusy(true);
        setAgentRetryNotice(locale === "zh-CN"
          ? `模型服务暂时不可用，${seconds} 秒后自动重试（第 ${event.event.payload.attempt} 次）`
          : `The model service is temporarily unavailable. Retrying in ${seconds}s (attempt ${event.event.payload.attempt}).`);
      }
      if (event.event.kind === "turn-completed" || event.event.kind === "turn-failed") {
        setStreamingAssistant("");
        setAgentBusy(false);
        setAgentRetryNotice("");
      }
      if (event.event.kind === "turn-completed") setAgentNotice("");
      if (event.event.kind === "turn-failed") setAgentNotice(event.event.payload.message);
    }).then((fn) => disposed ? fn() : unlisten.push(fn));
    api.onConversationEvent((event) => {
      if (event.conversation_id !== conversation?.id) return;
      setMessages((current) => current.some((message) => message.id === event.message.id) ? current : [...current, event.message]);
      setMessageSequence((value) => Math.max(value, event.message.sequence + 1));
    }).then((fn) => disposed ? fn() : unlisten.push(fn));
    api.onConversationUpdated((event) => {
      if (event.project_id !== selected?.id) return;
      setConversations((current) => [event.conversation, ...current.filter((item) => item.id !== event.conversation.id)]);
      setConversation((current) => current?.id === event.conversation.id ? event.conversation : current);
    }).then((fn) => disposed ? fn() : unlisten.push(fn));
    api.onKernelEvent((event) => {
      if (event.project_id !== selected?.id) return;
      setKernelEvents((current) => [...current.slice(-199), event]);
    }).then((fn) => disposed ? fn() : unlisten.push(fn));
    api.onSyncEvent((entry) => {
      if (entry.project_id !== selected?.id) return;
      setSyncEntries((current) => [entry, ...current.filter((item) => item.id !== entry.id)]);
    }).then((fn) => disposed ? fn() : unlisten.push(fn));
    api.onAgentRunEvent((event) => {
      if (event.project_id !== selected?.id) return;
      if (event.conversation_id !== conversation?.id) return;
      setRunId((current) => current ?? event.run_id);
      if (event.kind === "agent_canceled" || event.kind === "agent_completed" || event.kind === "agent_failed") {
        setRunStopping(false);
        setRunId((current) => current === event.run_id ? null : current);
      }
      if (event.kind === "agent_completed") void refreshRemoteFiles(event.project_id);
      setAgentRunEvents((current) => {
        if (current.some((item) => item.run_id === event.run_id && item.sequence === event.sequence)) return current;
        return [...current, event];
      });
    }).then((fn) => disposed ? fn() : unlisten.push(fn));
    return () => { disposed = true; unlisten.forEach((fn) => fn()); };
  }, [conversation?.id, selected?.id]);

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
  async function attachRun(start: () => Promise<string>) {
    setAgentNotice("");
    try {
      const id = await start();
      setRunId(id);
      const events = await api.listAgentRunEvents(selected!.id, id, conversation?.id);
      setAgentRunEvents((current) => mergeAgentRunEvents(current, events));
    } catch (error) {
      setAgentNotice(error instanceof Error ? error.message : String(error));
    }
  }

  function resetConversationWork() {
    setMessages([]); setMessageSequence(1); setStreamingAssistant(""); setAgentBusy(false); setAgentNotice(""); setAgentRetryNotice("");
    setLastGoal(""); setPlanProposal(null); setPlanApproved(false); setApprovedPlanId(null); setRunId(null); setRunStopping(false); setAgentRunEvents([]);
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
  const settings = settingsOpen ? <SettingsPanel locale={locale} onClose={() => setSettingsOpen(false)} modelProfiles={modelProfiles} skillPackages={skillPackages} mcpServers={mcpServers} connections={connections} selectedProject={selected} onSaveConnection={async (profile, secret) => { await api.saveConnection(profile, secret); setConnections(await api.listConnections()); }} onTestConnection={api.testConnection} onConfirmHostKey={async (profileId, fingerprint) => { await api.confirmHostKey(profileId, fingerprint); setConnections(await api.listConnections()); }} onBindProjectRemote={async (connectionId, remoteRoot) => { if (!selected) return; const updated = await api.updateProjectRemote(selected.id, connectionId, remoteRoot); setSelected(updated); setProjects((current) => current.map((project) => project.id === updated.id ? updated : project)); }} onSaveModel={async (request) => { const profile = await api.saveModelProfile(request); setModelProfiles((current) => [profile, ...current.filter((item) => item.id !== profile.id)]); setActiveModelProfileId(profile.id); }} onProbeModel={api.probeModelProfile} onListModels={api.listModelProfileModels} onImportSkill={async () => { const sourcePath = await api.chooseSkillDirectory(); if (!sourcePath) return; const skill = await api.importSkillDirectory(sourcePath); setSkillPackages((current) => [skill, ...current.filter((item) => item.id !== skill.id)]); }} onSetSkillEnabled={async (skillId, enabled) => { const updated = await api.setSkillEnabled(skillId, enabled); setSkillPackages(await api.listSkillPackages()); return updated; }} onSaveMcpServer={async (request) => { const updated = await api.saveMcpServer(request); setMcpServers(await api.listMcpServers()); return updated; }} onInspectMcpServer={async (serverId) => { if (!selected) throw new Error(locale === "zh-CN" ? "请先打开一个项目，再检查 MCP server。" : "Open a project before inspecting an MCP server."); await api.inspectConfiguredMcpServer(selected.id, serverId); setMcpServers(await api.listMcpServers()); }} onSetMcpServerEnabled={async (serverId, enabled) => { const updated = await api.setMcpServerEnabled(serverId, enabled); setMcpServers(await api.listMcpServers()); return updated; }} onSetMcpToolApproval={async (serverId, tool, approved) => { const updated = await api.setMcpToolApproval(serverId, tool, approved); setMcpServers(await api.listMcpServers()); return updated; }} /> : null;
  if (!selected) return <><ProjectLibrary projects={projects} connections={connections} locale={locale} onLocaleChange={setLocale} onSettings={() => setSettingsOpen(true)} onOpen={setSelected} onDelete={async (projectId) => { await api.deleteProject(projectId); setProjects((current) => current.filter((project) => project.id !== projectId)); }} onChooseLocalRoot={api.chooseProjectDirectory} onCreate={async ({ template, name, localRoot, connectionId, remoteRoot }) => { const project = await api.createProject({ name, description: "", local_root: localRoot, template, connection_id: connectionId, remote_root: remoteRoot }); setProjects((current) => [project, ...current]); setSelected(project); }} />{settings}</>;
  const activeModel = modelProfiles.find((profile) => profile.id === activeModelProfileId) ?? null;
  return <><WorkspaceShell
    project={{ id: selected.id, name: selected.name, status: selected.status, template: selected.template }}
    locale={locale} onLocaleChange={setLocale} onOpenSettings={() => setSettingsOpen(true)} onBackToProjects={() => setSelected(null)}
    conversations={conversations} activeConversationId={conversation?.id} onSelectConversation={selectConversation} onNewConversation={newConversation} onDeleteConversation={deleteConversation}
    messages={messages} streamingAssistant={streamingAssistant} agentBusy={agentBusy} agentNotice={agentNotice} agentRetryNotice={agentRetryNotice} modelLabel={activeModel?.label}
    planProposal={planProposal} planLoading={planLoading} planApproved={planApproved} canStartRun={Boolean(selected.connection_id && approvedPlanId)} runStarted={Boolean(runId)} activeRunId={runId} agentRunEvents={agentRunEvents}
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
    onSend={async (markdown) => {
      if (!conversation || !activeModel) { setSettingsOpen(true); return false; }
      setLastGoal(markdown); setPlanProposal(null); setPlanApproved(false); setApprovedPlanId(null); setRunId(null); setAgentBusy(true); setAgentNotice("");
      const remoteContext = selected.connection_id && selected.remote_root
        ? [
            `Remote root: ${selected.remote_root}`,
            `Indexed entries: ${remoteFiles.length}`,
            ...remoteFiles.slice(0, 200).map((entry) => `${entry.directory ? "directory" : "file"}\t${entry.relative_path}\t${entry.size_bytes} bytes`),
            ...(remoteFiles.length > 200 ? [`${remoteFiles.length - 200} additional entries omitted`] : []),
          ].join("\n")
        : null;
      try {
        await api.runAgentTurn({ project_id: selected.id, conversation_id: conversation.id, model_profile_id: activeModel.id, markdown, message_sequence: messageSequence, remote_context: remoteContext });
        const storedMessages = await api.listMessages(conversation.id);
        setMessages(storedMessages);
        setMessageSequence((storedMessages.at(-1)?.sequence ?? 0) + 1);
        if (selected.connection_id && selected.remote_root) {
          setAgentBusy(true);
          setPlanLoading(true);
          try {
            setPlanProposal(await api.proposeAnalysisPlan({
              project_id: selected.id,
              conversation_id: conversation.id,
              model_profile_id: activeModel.id,
              goal: markdown,
              environment_summary: remoteContext ?? `Remote Linux project at ${selected.remote_root}`,
            }));
          } catch (error) {
            setAgentNotice(`${locale === "zh-CN" ? "对话已完成，但远程计划生成失败" : "Conversation completed, but remote plan generation failed"}: ${error instanceof Error ? error.message : String(error)}`);
          } finally { setPlanLoading(false); setAgentBusy(false); }
        }
        return true;
      } catch (error) {
        setAgentNotice(error instanceof Error ? error.message : String(error));
        return false;
      } finally { setAgentBusy(false); }
    }}
    onRequestPlan={async () => {
      if (!activeModel || !lastGoal) { if (!activeModel) setSettingsOpen(true); return; }
      setPlanLoading(true); setAgentNotice("");
      try { if (!conversation) return; setPlanProposal(await api.proposeAnalysisPlan({ project_id: selected.id, conversation_id: conversation.id, model_profile_id: activeModel.id, goal: lastGoal, environment_summary: selected.remote_root ? `Remote Linux project at ${selected.remote_root}` : "Remote Linux environment not inspected yet" })); }
      catch (error) { setAgentNotice(error instanceof Error ? error.message : String(error)); }
      finally { setPlanLoading(false); }
    }}
    onApprovePlan={async () => {
      if (!planProposal) return;
      setAgentNotice("");
      try {
        const approved = await api.approvePlanV2(planProposal.plan, planProposal.plan.policy);
        setApprovedPlanId(approved.id);
        setPlanApproved(true);
        if (selected.connection_id) await attachRun(() => api.startRunV2(selected.connection_id!, selected.id, approved.id));
      } catch (error) {
        setAgentNotice(error instanceof Error ? error.message : String(error));
      }
    }}
    onStartRun={selected.connection_id ? async () => { if (!approvedPlanId) return; await attachRun(() => api.startRunV2(selected.connection_id!, selected.id, approvedPlanId)); } : undefined}
    onCancelRun={runId ? async () => {
      setRunStopping(true);
      setAgentNotice("");
      try {
        await api.cancelRun(runId);
        const events = await api.listAgentRunEvents(selected.id, runId, conversation?.id);
        setAgentRunEvents((current) => mergeAgentRunEvents(current, events));
      } catch (error) {
        setAgentNotice(error instanceof Error ? error.message : String(error));
        setRunStopping(false);
      }
    } : undefined}
  />{settings}</>;
}

function mergeAgentRunEvents(current: AgentRunStreamEvent[], incoming: AgentRunStreamEvent[]) {
  return [...current, ...incoming]
    .filter((event, index, all) => all.findIndex((item) => item.run_id === event.run_id && item.sequence === event.sequence) === index)
    .sort((left, right) => new Date(left.timestamp).getTime() - new Date(right.timestamp).getTime() || left.sequence - right.sequence);
}

function isTerminalAgentEvent(event: AgentRunStreamEvent) {
  return event.kind === "agent_completed" || event.kind === "agent_failed" || event.kind === "agent_canceled";
}
