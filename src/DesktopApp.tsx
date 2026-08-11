import { useEffect, useState } from "react";
import * as api from "./tauri-api";
import type { AgentEvent, ConnectionProfile, KernelEvent, KernelLanguage, KernelSession, ModelProfile, PlanProposal, RemoteFileEntry, SkillPackage, WorkspaceConversation, WorkspaceMessage, WorkspaceProject } from "./types";
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
  const [conversation, setConversation] = useState<WorkspaceConversation | null>(null);
  const [messageSequence, setMessageSequence] = useState(1);
  const [messages, setMessages] = useState<WorkspaceMessage[]>([]);
  const [streamingAssistant, setStreamingAssistant] = useState("");
  const [modelProfiles, setModelProfiles] = useState<ModelProfile[]>([]);
  const [activeModelProfileId, setActiveModelProfileId] = useState<string | null>(null);
  const [lastGoal, setLastGoal] = useState("");
  const [planProposal, setPlanProposal] = useState<PlanProposal | null>(null);
  const [planLoading, setPlanLoading] = useState(false);
  const [planApproved, setPlanApproved] = useState(false);
  const [approvedPlanId, setApprovedPlanId] = useState<string | null>(null);
  const [runId, setRunId] = useState<string | null>(null);
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
    if (!selected) { setConversation(null); return; }
    api.listConversations(selected.id).then(async (items) => {
      const active = items[0] ?? await api.createConversation(selected.id, locale === "zh-CN" ? "QC 与聚类" : "QC and clustering");
      setConversation(active);
      const storedMessages = await api.listMessages(active.id);
      setMessages(storedMessages);
      setMessageSequence((storedMessages.at(-1)?.sequence ?? 0) + 1);
    });
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
    const unlisten: Array<() => void> = [];
    api.onAgentEvent((event: AgentEvent) => {
      if (event.conversation_id !== conversation?.id) return;
      if (event.event.kind === "turn-started") setStreamingAssistant("");
      if (event.event.kind === "text-delta") {
        const delta = event.event.payload;
        setStreamingAssistant((value) => value + delta);
      }
      if (event.event.kind === "turn-completed" || event.event.kind === "turn-failed") setStreamingAssistant("");
    }).then((fn) => disposed ? fn() : unlisten.push(fn));
    api.onConversationEvent((event) => {
      if (event.conversation_id !== conversation?.id) return;
      setMessages((current) => current.some((message) => message.id === event.message.id) ? current : [...current, event.message]);
      setMessageSequence((value) => Math.max(value, event.message.sequence + 1));
    }).then((fn) => disposed ? fn() : unlisten.push(fn));
    api.onKernelEvent((event) => {
      if (event.project_id !== selected?.id) return;
      setKernelEvents((current) => [...current.slice(-199), event]);
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
  if (loading) return <div className="desktop-loading">OmicsOps</div>;
  const settings = settingsOpen ? <SettingsPanel locale={locale} onClose={() => setSettingsOpen(false)} modelProfiles={modelProfiles} skillPackages={skillPackages} connections={connections} selectedProject={selected} onSaveConnection={async (profile, secret) => { await api.saveConnection(profile, secret); setConnections(await api.listConnections()); }} onTestConnection={api.testConnection} onConfirmHostKey={async (profileId, fingerprint) => { await api.confirmHostKey(profileId, fingerprint); setConnections(await api.listConnections()); }} onBindProjectRemote={async (connectionId, remoteRoot) => { if (!selected) return; const updated = await api.updateProjectRemote(selected.id, connectionId, remoteRoot); setSelected(updated); setProjects((current) => current.map((project) => project.id === updated.id ? updated : project)); }} onSaveModel={async (request) => { const profile = await api.saveModelProfile(request); setModelProfiles((current) => [profile, ...current.filter((item) => item.id !== profile.id)]); setActiveModelProfileId(profile.id); }} onProbeModel={api.probeModelProfile} onImportSkill={async () => { const sourcePath = await api.chooseSkillDirectory(); if (!sourcePath) return; const skill = await api.importSkillDirectory(sourcePath); setSkillPackages((current) => [skill, ...current.filter((item) => item.id !== skill.id)]); }} onSetSkillEnabled={async (skillId, enabled) => { const updated = await api.setSkillEnabled(skillId, enabled); setSkillPackages(await api.listSkillPackages()); return updated; }} /> : null;
  if (!selected) return <><ProjectLibrary projects={projects} connections={connections} locale={locale} onLocaleChange={setLocale} onSettings={() => setSettingsOpen(true)} onOpen={setSelected} onChooseLocalRoot={api.chooseProjectDirectory} onCreate={async ({ template, name, localRoot, connectionId, remoteRoot }) => { const project = await api.createProject({ name, description: "", local_root: localRoot, template, connection_id: connectionId, remote_root: remoteRoot }); setProjects((current) => [project, ...current]); setSelected(project); }} />{settings}</>;
  const activeModel = modelProfiles.find((profile) => profile.id === activeModelProfileId) ?? null;
  return <><WorkspaceShell project={{ id: selected.id, name: selected.name, status: selected.status, template: selected.template }} locale={locale} onLocaleChange={setLocale} onOpenSettings={() => setSettingsOpen(true)} onBackToProjects={() => setSelected(null)} messages={messages} streamingAssistant={streamingAssistant} modelLabel={activeModel?.label} planProposal={planProposal} planLoading={planLoading} planApproved={planApproved} canStartRun={Boolean(selected.connection_id && approvedPlanId)} runStarted={Boolean(runId)} remoteFiles={remoteFiles} filesBusy={filesBusy} fileNotice={fileNotice} kernelSessions={kernelSessions} kernelEvents={kernelEvents} kernelBusy={kernelBusy} kernelNotice={kernelNotice} onStartKernel={selected.connection_id && selected.remote_root ? startKernel : undefined} onExecuteKernel={async (sessionId, code, save, capturePaths) => { let savedIndex: number | null = null; await withKernelBusy(async () => { const result = await api.executeKernelCell(sessionId, code, save, capturePaths); savedIndex = result.saved_cell_index; setKernelEvents((current) => { const known = new Set(current.map((event) => `${event.request_id}:${event.sequence}`)); return [...current, ...result.events.filter((event) => !known.has(`${event.request_id}:${event.sequence}`))].slice(-200); }); }); return savedIndex; }} onInterruptKernel={async (sessionId) => withKernelBusy(async () => replaceKernelSession(await api.interruptKernel(sessionId)))} onStopKernel={async (sessionId) => withKernelBusy(async () => replaceKernelSession(await api.stopKernel(sessionId)))} onPromoteKernelCell={api.promoteKernelCell} onUploadFiles={selected.connection_id && selected.remote_root ? uploadFiles : undefined} onRefreshFiles={selected.connection_id && selected.remote_root ? () => refreshRemoteFiles() : undefined} onDownloadFile={selected.connection_id && selected.remote_root ? downloadFile : undefined} onSend={async (markdown) => { if (!conversation || !activeModel) { setSettingsOpen(true); return false; } setLastGoal(markdown); setPlanProposal(null); setPlanApproved(false); setApprovedPlanId(null); setRunId(null); await api.runAgentTurn({ project_id: selected.id, conversation_id: conversation.id, model_profile_id: activeModel.id, markdown, message_sequence: messageSequence }); return true; }} onRequestPlan={async () => { if (!activeModel || !lastGoal) { if (!activeModel) setSettingsOpen(true); return; } setPlanLoading(true); try { setPlanProposal(await api.proposeAnalysisPlan({ project_id: selected.id, model_profile_id: activeModel.id, goal: lastGoal, environment_summary: selected.remote_root ? `Remote Linux project at ${selected.remote_root}` : "Remote Linux environment not inspected yet" })); } finally { setPlanLoading(false); } }} onApprovePlan={async () => { if (!planProposal) return; const approved = await api.approvePlanV2(planProposal.plan, planProposal.plan.policy); setApprovedPlanId(approved.id); setPlanApproved(true); }} onStartRun={async () => { if (!selected.connection_id || !approvedPlanId) return; setRunId(await api.startRunV2(selected.connection_id, selected.id, approvedPlanId)); }} />{settings}</>;
}
