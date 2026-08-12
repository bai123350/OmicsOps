import { useEffect, useState } from "react";
import * as api from "./tauri-api";
import type { AgentEvent, AgentRunStreamEvent, ConnectionProfile, KernelEvent, KernelLanguage, KernelSession, ModelProfile, PlanProposal, RemoteFileEntry, SkillPackage, WorkspaceConversation, WorkspaceMessage, WorkspaceProject } from "./types";
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
  const [kernelSessions, setKernelSessions] = useState<KernelSession[]>([]);
  const [kernelEvents, setKernelEvents] = useState<KernelEvent[]>([]);
  const [kernelBusy, setKernelBusy] = useState(false);
  const [kernelNotice, setKernelNotice] = useState("");
  const [connections, setConnections] = useState<ConnectionProfile[]>([]);

  useEffect(() => {
    Promise.all([api.listProjects(), api.listModelProfiles(), api.listSkillPackages(), api.listConnections()]).then(([items, profiles, skills, savedConnections]) => {
      setProjects(items); setSelected(items[0] ?? null);
      setModelProfiles(profiles); setActiveModelProfileId(profiles[0]?.id ?? null);
      setSkillPackages(skills);
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
    if (!selected) return () => { disposed = true; };
    api.listAgentRunEvents(selected.id)
      .then((events) => {
        if (disposed) return;
        setAgentRunEvents((current) => {
          if (events.length === 0) return current;
          const latestRunId = current.at(-1)?.run_id ?? events.at(-1)?.run_id;
          const merged = [...events, ...current]
            .filter((event) => event.run_id === latestRunId)
            .filter((event, index, all) => all.findIndex((item) => item.run_id === event.run_id && item.sequence === event.sequence) === index)
            .sort((left, right) => left.sequence - right.sequence);
          return merged;
        });
        setRunId((current) => current ?? events.at(-1)?.run_id ?? null);
      })
      .catch((error) => {
        if (!disposed) setAgentNotice(error instanceof Error ? error.message : String(error));
      });
    return () => { disposed = true; };
  }, [selected?.id]);
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
    api.onAgentRunEvent((event) => {
      if (event.project_id !== selected?.id) return;
      setRunId((current) => current ?? event.run_id);
      if (event.kind === "agent_canceled" || event.kind === "agent_completed" || event.kind === "agent_failed") setRunStopping(false);
      if (event.kind === "agent_completed") void refreshRemoteFiles(event.project_id);
      setAgentRunEvents((current) => {
        if (current.some((item) => item.run_id === event.run_id && item.sequence === event.sequence)) return current;
        return [...current.filter((item) => item.run_id === event.run_id).slice(-399), event];
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
      setAgentRunEvents(await api.listAgentRunEvents(selected!.id, id));
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
  if (loading) return <div className="desktop-loading">OmicsOps</div>;
  const settings = settingsOpen ? <SettingsPanel locale={locale} onClose={() => setSettingsOpen(false)} modelProfiles={modelProfiles} skillPackages={skillPackages} connections={connections} selectedProject={selected} onSaveConnection={async (profile, secret) => { await api.saveConnection(profile, secret); setConnections(await api.listConnections()); }} onTestConnection={api.testConnection} onConfirmHostKey={async (profileId, fingerprint) => { await api.confirmHostKey(profileId, fingerprint); setConnections(await api.listConnections()); }} onBindProjectRemote={async (connectionId, remoteRoot) => { if (!selected) return; const updated = await api.updateProjectRemote(selected.id, connectionId, remoteRoot); setSelected(updated); setProjects((current) => current.map((project) => project.id === updated.id ? updated : project)); }} onSaveModel={async (request) => { const profile = await api.saveModelProfile(request); setModelProfiles((current) => [profile, ...current.filter((item) => item.id !== profile.id)]); setActiveModelProfileId(profile.id); }} onProbeModel={api.probeModelProfile} onListModels={api.listModelProfileModels} onImportSkill={async () => { const sourcePath = await api.chooseSkillDirectory(); if (!sourcePath) return; const skill = await api.importSkillDirectory(sourcePath); setSkillPackages((current) => [skill, ...current.filter((item) => item.id !== skill.id)]); }} onSetSkillEnabled={async (skillId, enabled) => { const updated = await api.setSkillEnabled(skillId, enabled); setSkillPackages(await api.listSkillPackages()); return updated; }} /> : null;
  if (!selected) return <><ProjectLibrary projects={projects} connections={connections} locale={locale} onLocaleChange={setLocale} onSettings={() => setSettingsOpen(true)} onOpen={setSelected} onChooseLocalRoot={api.chooseProjectDirectory} onCreate={async ({ template, name, localRoot, connectionId, remoteRoot }) => { const project = await api.createProject({ name, description: "", local_root: localRoot, template, connection_id: connectionId, remote_root: remoteRoot }); setProjects((current) => [project, ...current]); setSelected(project); }} />{settings}</>;
  const activeModel = modelProfiles.find((profile) => profile.id === activeModelProfileId) ?? null;
  return <><WorkspaceShell
    project={{ id: selected.id, name: selected.name, status: selected.status, template: selected.template }}
    locale={locale} onLocaleChange={setLocale} onOpenSettings={() => setSettingsOpen(true)} onBackToProjects={() => setSelected(null)}
    conversations={conversations} activeConversationId={conversation?.id} onSelectConversation={selectConversation} onNewConversation={newConversation}
    messages={messages} streamingAssistant={streamingAssistant} agentBusy={agentBusy} agentNotice={agentNotice} agentRetryNotice={agentRetryNotice} modelLabel={activeModel?.label}
    planProposal={planProposal} planLoading={planLoading} planApproved={planApproved} canStartRun={Boolean(selected.connection_id && approvedPlanId)} runStarted={Boolean(runId)} agentRunEvents={agentRunEvents}
    runStopping={runStopping}
    remoteFiles={remoteFiles} filesBusy={filesBusy} fileNotice={fileNotice}
    kernelSessions={kernelSessions} kernelEvents={kernelEvents} kernelBusy={kernelBusy} kernelNotice={kernelNotice}
    onStartKernel={selected.connection_id && selected.remote_root ? startKernel : undefined}
    onExecuteKernel={async (sessionId, code, save, capturePaths) => { let savedIndex: number | null = null; await withKernelBusy(async () => { const result = await api.executeKernelCell(sessionId, code, save, capturePaths); savedIndex = result.saved_cell_index; setKernelEvents((current) => { const known = new Set(current.map((event) => `${event.request_id}:${event.sequence}`)); return [...current, ...result.events.filter((event) => !known.has(`${event.request_id}:${event.sequence}`))].slice(-200); }); }); return savedIndex; }}
    onInterruptKernel={async (sessionId) => withKernelBusy(async () => replaceKernelSession(await api.interruptKernel(sessionId)))}
    onStopKernel={async (sessionId) => withKernelBusy(async () => replaceKernelSession(await api.stopKernel(sessionId)))} onPromoteKernelCell={api.promoteKernelCell}
    onUploadFiles={selected.connection_id && selected.remote_root ? uploadFiles : undefined} onRefreshFiles={selected.connection_id && selected.remote_root ? () => refreshRemoteFiles() : undefined} onDownloadFile={selected.connection_id && selected.remote_root ? downloadFile : undefined}
    onPreviewImage={selected.connection_id && selected.remote_root ? (relativePath) => api.previewProjectImage(selected.id, relativePath) : undefined}
    onSend={async (markdown) => {
      if (!conversation || !activeModel) { setSettingsOpen(true); return false; }
      setLastGoal(markdown); setPlanProposal(null); setPlanApproved(false); setApprovedPlanId(null); setRunId(null); setAgentRunEvents([]); setAgentBusy(true); setAgentNotice("");
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
        setAgentRunEvents(await api.listAgentRunEvents(selected.id, runId));
      } catch (error) {
        setAgentNotice(error instanceof Error ? error.message : String(error));
        setRunStopping(false);
      }
    } : undefined}
  />{settings}</>;
}
