import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { open, save } from "@tauri-apps/plugin-dialog";

import type {
  AnalysisPlan,
  Artifact,
  ConnectionProfile,
  ConnectionTestResult,
  ProjectSpec,
  RunCheckpoint,
  RunEvent,
  ServerInspection,
  AnalysisPlanV2,
  ApprovedPlan,
  PlanValidation,
  PlanningTurn,
  PolicyEnvelope,
  ToolSummary,
  RunCheckpointV2,
  RunEventV2,
  AgentRunStreamEvent,
  AgentRunEventV3,
  AgentRunEventV4,
  RunSummaryV4,
  StepAttempt,
  ArtifactRecordV2,
  EnvironmentLock,
  WorkspaceConversation,
  WorkspaceProject,
  WorkspaceTemplate,
  WorkspaceMessage,
  ModelProfile,
  ModelProbeResult,
  AgentEvent,
  PlanProposal,
  RemoteFileEntry,
  ProjectImagePreview,
  SyncEntry,
  SkillPackage,
  ResearchSearchResult,
  KernelLanguage,
  KernelSession,
  KernelEvent,
  KernelCellResult,
  FormalStepProposal,
  MemoryFact,
  NotebookEntry,
  ProjectArtifact,
  McpResult,
  McpServerProfile,
} from "./types";

export const isTauri = () => "__TAURI_INTERNALS__" in window;

export async function listProjects(): Promise<WorkspaceProject[]> {
  return isTauri() ? invoke("list_projects") : [];
}

export async function chooseProjectDirectory(): Promise<string | null> {
  if (!isTauri()) return "E:/Science/omicsops-demo";
  const selected = await open({ directory: true, multiple: false });
  return typeof selected === "string" ? selected : null;
}

export async function createProject(request: { name: string; description: string; local_root: string; template: WorkspaceTemplate; connection_id?: string | null; remote_root?: string | null }): Promise<WorkspaceProject> {
  if (!isTauri()) {
    const now = new Date().toISOString();
    return { id: crypto.randomUUID(), ...request, remote_root: request.remote_root ?? null, connection_id: request.connection_id ?? null, status: "ready", ollama_only: false, created_at: now, updated_at: now };
  }
  return invoke("create_project", { request });
}

export async function deleteProject(projectId: string): Promise<void> {
  if (!isTauri()) return;
  await invoke("delete_project", { projectId });
}

export async function listConversations(projectId: string): Promise<WorkspaceConversation[]> {
  return isTauri() ? invoke("list_conversations", { projectId }) : [];
}

export async function createConversation(projectId: string, title?: string): Promise<WorkspaceConversation> {
  if (!isTauri()) {
    const now = new Date().toISOString();
    return { id: crypto.randomUUID(), project_id: projectId, title: title ?? "", status: "idle", model_profile_id: null, created_at: now, updated_at: now };
  }
  return invoke("create_conversation", { request: { project_id: projectId, title: title ?? null } });
}

export async function deleteConversation(projectId: string, conversationId: string): Promise<void> {
  if (!isTauri()) return;
  await invoke("delete_conversation", { projectId, conversationId });
}

export async function listMessages(conversationId: string): Promise<WorkspaceMessage[]> {
  return isTauri() ? invoke("list_messages", { conversationId }) : [];
}

export async function submitMessage(request: { project_id: string; conversation_id: string; markdown: string; sequence: number }): Promise<WorkspaceMessage> {
  if (!isTauri()) return { id: crypto.randomUUID(), ...request, role: "user", created_at: new Date().toISOString() };
  return invoke("submit_message", { request });
}

export async function listModelProfiles(): Promise<ModelProfile[]> {
  return isTauri() ? invoke("list_model_profiles") : [];
}

export async function saveModelProfile(request: { id?: string; label: string; provider: ModelProfile["provider"]; base_url: string; model: string; credential?: string }): Promise<ModelProfile> {
  if (!isTauri()) return { id: request.id ?? crypto.randomUUID(), label: request.label, provider: request.provider, base_url: request.base_url, model: request.model, credential_reference: request.provider === "ollama" ? null : "model/demo", supports_tools: true, supports_vision: false };
  return invoke("save_model_profile", { request });
}

export async function probeModelProfile(profileId: string): Promise<ModelProbeResult> {
  if (!isTauri()) return { endpoint: "https://models.example/v1/chat/completions", protocol: "OpenAiCompatible", model: "demo", latency_ms: 25, response_preview: "OK" };
  return invoke("probe_model_profile", { profileId });
}

export async function listModelProfileModels(profileId: string): Promise<string[]> {
  return isTauri() ? invoke("list_model_profile_models", { profileId }) : ["demo-model"];
}

export async function runAgentTurn(request: { project_id: string; conversation_id: string; model_profile_id: string; markdown: string; message_sequence: number; remote_context?: string | null }): Promise<string> {
  if (!isTauri()) return crypto.randomUUID();
  return invoke("run_agent_turn", { request });
}

export async function proposeAnalysisPlan(request: { project_id: string; conversation_id: string; model_profile_id: string; goal: string; environment_summary: string }): Promise<PlanProposal> {
  return invoke("propose_analysis_plan", { request });
}

export async function onAgentEvent(callback: (event: AgentEvent) => void): Promise<UnlistenFn> {
  if (!isTauri()) return () => undefined;
  return listen<AgentEvent>("agent-event", ({ payload }) => callback(payload));
}

export async function onConversationEvent(callback: (event: { project_id: string; conversation_id: string; message: WorkspaceMessage }) => void): Promise<UnlistenFn> {
  if (!isTauri()) return () => undefined;
  return listen("conversation-event", ({ payload }) => callback(payload as { project_id: string; conversation_id: string; message: WorkspaceMessage }));
}

export async function onConversationUpdated(callback: (event: { project_id: string; conversation: WorkspaceConversation }) => void): Promise<UnlistenFn> {
  if (!isTauri()) return () => undefined;
  return listen("conversation-updated", ({ payload }) => callback(payload as { project_id: string; conversation: WorkspaceConversation }));
}

export async function listRemoteFiles(projectId: string): Promise<RemoteFileEntry[]> {
  return isTauri() ? invoke("list_remote_files", { projectId }) : [];
}

export async function chooseProjectFiles(localRoot: string): Promise<string[]> {
  if (!isTauri()) return [];
  const selected = await open({ multiple: true, directory: false });
  const paths = Array.isArray(selected) ? selected : selected ? [selected] : [];
  const normalizedRoot = localRoot.replaceAll("\\", "/").replace(/\/$/, "");
  return paths.map((path) => path.replaceAll("\\", "/")).map((path) => {
    const sameRoot = path.toLocaleLowerCase().startsWith(`${normalizedRoot.toLocaleLowerCase()}/`);
    if (!sameRoot) throw new Error("Only files inside the project workspace can be uploaded");
    return path.slice(normalizedRoot.length + 1);
  });
}

export async function uploadSelectedFiles(projectId: string, relativePaths: string[]): Promise<SyncEntry[]> {
  return invoke("upload_selected_files", { request: { project_id: projectId, relative_paths: relativePaths } });
}

export async function downloadProjectFile(projectId: string, relativePath: string): Promise<{ entry: SyncEntry; conflict: boolean }> {
  return invoke("download_project_file", { request: { project_id: projectId, relative_path: relativePath } });
}

export async function listSyncEntries(projectId: string): Promise<SyncEntry[]> { return isTauri() ? invoke("list_sync_entries", { projectId }) : []; }
export async function pauseSyncTransfer(transferId: string): Promise<void> { await invoke("pause_sync_transfer", { transferId }); }
export async function cancelSyncTransfer(transferId: string): Promise<void> { await invoke("cancel_sync_transfer", { transferId }); }
export async function retrySyncTransfer(transferId: string): Promise<SyncEntry> { return invoke("retry_sync_transfer", { transferId }); }
export async function onSyncEvent(callback: (entry: SyncEntry) => void): Promise<UnlistenFn> {
  if (!isTauri()) return () => undefined;
  return listen<SyncEntry>("artifact-event", ({ payload }) => callback(payload));
}

export async function previewProjectImage(projectId: string, relativePath: string): Promise<ProjectImagePreview> {
  return invoke("preview_project_image", { request: { project_id: projectId, relative_path: relativePath } });
}

export async function searchAgentMemory(projectId: string, query = "", dimension?: string, conversationId?: string): Promise<MemoryFact[]> {
  return isTauri() ? invoke("search_agent_memory", { request: { project_id: projectId, conversation_id: conversationId ?? null, query, dimension: dimension ?? null } }) : [];
}

export async function listNotebookEntries(projectId: string): Promise<NotebookEntry[]> {
  return isTauri() ? invoke("list_notebook_entries", { projectId }) : [];
}

export async function listProjectArtifacts(projectId: string): Promise<ProjectArtifact[]> {
  return isTauri() ? invoke("list_project_artifacts", { projectId }) : [];
}

export async function exportProjectNotebook(projectId: string, format: "markdown" | "json" | "bundle", localPath: string): Promise<void> {
  await invoke("export_project_notebook", { request: { project_id: projectId, format, local_path: localPath } });
}

export async function inspectMcpServer(request: { project_id: string; name: string; command: string; args?: string[]; approved: boolean }): Promise<McpResult> {
  return invoke("inspect_mcp_server", { request: { ...request, args: request.args ?? [], tool: null, arguments: null } });
}

export async function callMcpTool(request: { project_id: string; name: string; command: string; args?: string[]; approved: boolean; tool: string; arguments?: Record<string, unknown> }): Promise<McpResult> {
  return invoke("call_mcp_tool", { request: { ...request, args: request.args ?? [], arguments: request.arguments ?? {} } });
}

export async function listMcpServers(): Promise<McpServerProfile[]> {
  return isTauri() ? invoke("list_mcp_servers") : [];
}

export async function saveMcpServer(request: { id?: string; name: string; command: string; args?: string[] }): Promise<McpServerProfile> {
  return invoke("save_mcp_server", { request: { ...request, args: request.args ?? [] } });
}

export async function setMcpServerEnabled(serverId: string, enabled: boolean): Promise<McpServerProfile> {
  return invoke("set_mcp_server_enabled", { request: { server_id: serverId, enabled } });
}

export async function inspectConfiguredMcpServer(projectId: string, serverId: string): Promise<McpResult> {
  return invoke("inspect_configured_mcp_server", { request: { project_id: projectId, server_id: serverId, approved: true } });
}

export async function setMcpToolApproval(serverId: string, tool: string, approved: boolean): Promise<McpServerProfile> {
  return invoke("set_mcp_tool_approval", { request: { server_id: serverId, tool, approved } });
}

export async function callConfiguredMcpTool(request: { project_id: string; server_id: string; tool: string; arguments?: Record<string, unknown>; approved: boolean }): Promise<McpResult> {
  return invoke("call_configured_mcp_tool", { request: { ...request, arguments: request.arguments ?? {} } });
}

export async function listSkillPackages(): Promise<SkillPackage[]> {
  return isTauri() ? invoke("list_skill_packages") : [];
}

export async function chooseSkillDirectory(): Promise<string | null> {
  if (!isTauri()) return null;
  const selected = await open({ directory: true, multiple: false });
  return typeof selected === "string" ? selected : null;
}

export async function importSkillDirectory(sourcePath: string): Promise<SkillPackage> {
  return invoke("import_skill_directory", { request: { source_path: sourcePath } });
}

export async function setSkillEnabled(skillId: string, enabled: boolean): Promise<SkillPackage> {
  return invoke("set_skill_enabled", { request: { skill_id: skillId, enabled } });
}

export async function searchResearch(request: { source: ResearchSearchResult["source"]; query: string; limit?: number; cursor?: string | null; refresh?: boolean }): Promise<ResearchSearchResult> {
  return invoke("search_research", { request: { source: request.source, query: request.query, limit: request.limit ?? 20, cursor: request.cursor ?? null, refresh: request.refresh ?? false } });
}

export async function listKernelSessions(): Promise<KernelSession[]> {
  return isTauri() ? invoke("list_kernel_sessions") : [];
}

export async function startKernel(projectId: string, language: KernelLanguage, rebuildSessionId?: string): Promise<KernelSession> {
  return invoke("start_kernel", { request: { project_id: projectId, language, rebuild_session_id: rebuildSessionId ?? null } });
}

export async function executeKernelCell(sessionId: string, code: string, saveCell: boolean, capturePaths: string[]): Promise<KernelCellResult> {
  return invoke("execute_kernel_cell", { request: { session_id: sessionId, code, save: saveCell, capture_paths: capturePaths } });
}

export async function interruptKernel(sessionId: string): Promise<KernelSession> {
  return invoke("interrupt_kernel", { sessionId });
}

export async function stopKernel(sessionId: string): Promise<KernelSession> {
  return invoke("stop_kernel", { sessionId });
}

export async function promoteKernelCell(sessionId: string, cellIndex: number, name: string, version = 1): Promise<FormalStepProposal> {
  return invoke("promote_kernel_cell", { request: { session_id: sessionId, cell_index: cellIndex, name, version } });
}

export async function onKernelEvent(callback: (event: KernelEvent) => void): Promise<UnlistenFn> {
  if (!isTauri()) return () => undefined;
  return listen<KernelEvent>("kernel-event", ({ payload }) => callback(payload));
}

export async function choosePlan(): Promise<string | null> {
  if (!isTauri()) return "example-plan.md";
  const selected = await open({
    multiple: false,
    filters: [{ name: "分析方案", extensions: ["md", "markdown", "docx", "pdf", "txt"] }],
  });
  return typeof selected === "string" ? selected : null;
}

export async function chooseDownloadPath(defaultName: string): Promise<string | null> {
  if (!isTauri()) return defaultName;
  return save({ defaultPath: defaultName });
}

export async function saveConnection(
  profile: ConnectionProfile,
  secret: string,
): Promise<void> {
  if (!isTauri()) return;
  await invoke("save_connection", { profile, secret });
}

export async function listConnections(): Promise<ConnectionProfile[]> {
  return isTauri() ? invoke("list_connections") : [];
}

export async function testConnection(
  profileId: string,
): Promise<ConnectionTestResult> {
  if (!isTauri()) {
    return { fingerprint: "SHA256:demo-host-key", trusted: false, authenticated: false, latencyMs: 25, serverOs: null, remoteUsername: null, home: null, sftpAvailable: false, pythonAvailable: false, rAvailable: false };
  }
  return invoke("test_connection", { profileId });
}

export async function updateProjectRemote(projectId: string, connectionId: string | null, remoteRoot: string | null): Promise<WorkspaceProject> {
  return invoke("update_project_remote", { request: { project_id: projectId, connection_id: connectionId, remote_root: remoteRoot } });
}

export async function confirmHostKey(
  profileId: string,
  fingerprint: string,
): Promise<void> {
  if (!isTauri()) return;
  await invoke("confirm_host_key", { profileId, fingerprint });
}

export async function saveLlm(
  baseUrl: string,
  model: string,
  apiKey: string,
): Promise<void> {
  if (!isTauri()) return;
  await invoke("save_llm_config", { baseUrl, model, apiKey });
  await invoke("probe_llm");
}

export async function inspectProject(
  profileId: string,
  remoteRoot: string,
): Promise<ServerInspection> {
  if (!isTauri()) {
    return {
      os: "Linux 6.8",
      cpuCores: 16,
      memoryKib: 67_108_864,
      diskAvailableKib: 524_288_000,
      home: "/home/omicsops",
      micromamba: "/usr/local/bin/micromamba",
      remoteUser: "omicsops",
      projectRealPath: remoteRoot,
      projectExists: false,
      projectEmpty: true,
      ownerMatches: true,
    };
  }
  return invoke("inspect_project", { profileId, remoteRoot });
}

export async function initializeProject(
  profileId: string,
  project: ProjectSpec,
): Promise<ServerInspection> {
  if (!isTauri()) return inspectProject(profileId, project.remote_root);
  return invoke("initialize_project", { profileId, project });
}

export async function extractPlan(path: string): Promise<{ format: string; text: string }> {
  if (!isTauri()) {
    return {
      format: "markdown",
      text: "# PBMC scRNA-seq\n下载公开 PBMC 数据，完成 Scanpy 分析并转换为 Seurat RDS。",
    };
  }
  return invoke("extract_plan", { path });
}

export async function generatePlan(
  documentText: string,
  inspection: ServerInspection,
): Promise<AnalysisPlan> {
  if (!isTauri()) {
    const limits = {
      max_cpu_cores: 8,
      max_memory_gib: 32,
      max_disk_gib: 50,
      max_step_seconds: 86_400,
    };
    return {
      id: crypto.randomUUID(),
      title: "PBMC scRNA-seq 自主分析",
      summary: "下载、质控、聚类、注释并转换到 Seurat。",
      stages: ["数据下载", "Scanpy 质控与聚类", "Seurat 转换", "报告"].map(
        (goal, index) => ({
          id: `stage-${index + 1}`,
          goal,
          dependencies: index ? [`stage-${index}`] : [],
          steps: [],
          expected_artifacts: [],
          completion_conditions: [],
        }),
      ),
      resource_budget: limits,
      highest_risk: "low",
      approved: false,
    };
  }
  return invoke("generate_plan", { documentText, inspection });
}

export async function approvePlan(plan: AnalysisPlan): Promise<AnalysisPlan> {
  if (!isTauri()) return { ...plan, approved: true };
  return invoke("approve_legacy_plan", { plan });
}

export async function startRun(
  profileId: string,
  project: ProjectSpec,
  plan: AnalysisPlan,
): Promise<string> {
  if (!isTauri()) return "RUN-2026-0001";
  return invoke("legacy_start_run", { profileId, project, plan });
}

export async function listRuns(): Promise<RunCheckpoint[]> {
  if (!isTauri()) return [];
  return invoke("list_runs");
}

export async function listRunEvents(runId: string): Promise<RunEvent[]> {
  if (!isTauri()) return [];
  return invoke("list_run_events", { runId });
}

export async function resumeRun(runId: string): Promise<void> {
  if (!isTauri()) return;
  await invoke("resume_run", { runId });
}

export async function approveRun(runId: string, approvalId: string): Promise<void> {
  if (!isTauri()) return;
  await invoke("approve_run", { runId, approvalId });
}

export async function cancelRun(runId: string): Promise<void> {
  if (!isTauri()) return;
  await invoke("cancel_run", { runId });
}

export async function listenRunEvents(
  callback: (event: RunEvent) => void,
): Promise<UnlistenFn> {
  if (!isTauri()) return () => undefined;
  return listen<RunEvent>("run-event", ({ payload }) => callback(payload));
}

export async function listArtifacts(
  profileId: string,
  remoteRoot: string,
): Promise<Artifact[]> {
  if (!isTauri()) {
    return [
      {
        remote_path: `${remoteRoot}/results/pbmc3k.h5ad`,
        kind: "h5ad",
        size_bytes: 85_432_109,
        sha256: "8f5f6f32-demo",
        previewable: false,
        downloadable: true,
      },
      {
        remote_path: `${remoteRoot}/results/pbmc3k.seurat.rds`,
        kind: "seurat_rds",
        size_bytes: 68_200_410,
        sha256: "7b2414ab-demo",
        previewable: false,
        downloadable: true,
      },
      {
        remote_path: `${remoteRoot}/results/report.html`,
        kind: "html_report",
        size_bytes: 152_140,
        sha256: "f312ce91-demo",
        previewable: true,
        downloadable: true,
      },
    ];
  }
  return invoke("list_artifacts", { profileId, remoteRoot });
}

export async function downloadArtifact(
  profileId: string,
  remotePath: string,
  localPath: string,
): Promise<void> {
  if (!isTauri()) return;
  await invoke("download_artifact", { profileId, remotePath, localPath });
}

export async function listTools(): Promise<ToolSummary[]> {
  return isTauri() ? invoke("list_tools") : [];
}

export async function planningTurn(request: {
  goal: string;
  environment_summary: string;
  answers: Record<string, string>;
}): Promise<PlanningTurn> {
  return invoke("planning_turn", { request });
}

export async function validatePlanV2(plan: AnalysisPlanV2): Promise<PlanValidation> {
  return invoke("validate_plan", { plan });
}

export async function approvePlanV2(plan: AnalysisPlanV2, envelope: PolicyEnvelope): Promise<ApprovedPlan> {
  return invoke("approve_plan", { plan, envelope });
}

export async function startRunV2(profileId: string, projectId: string, approvedPlanId: string): Promise<string> {
  return invoke("start_run", { profileId, projectId, approvedPlanId });
}

export async function exportRunBundle(runId: string, localPath: string): Promise<void> {
  await invoke("export_run_bundle", { runId, localPath });
}

export async function listRunsV2(): Promise<RunCheckpointV2[]> {
  return isTauri() ? invoke("list_runs_v2") : [];
}

export async function resumeRunV2(runId: string): Promise<void> {
  await invoke("resume_run_v2", { runId });
}

export async function listRunEventsV2(runId: string): Promise<RunEventV2[]> {
  return invoke("list_run_events_v2", { runId });
}

export async function onAgentRunEvent(callback: (event: AgentRunStreamEvent) => void): Promise<UnlistenFn> {
  if (!isTauri()) return () => undefined;
  return listen<AgentRunStreamEvent>("agent-run-event", ({ payload }) => callback(payload));
}

export async function listAgentRunEvents(projectId: string, runId?: string, conversationId?: string): Promise<AgentRunStreamEvent[]> {
  return isTauri() ? invoke("list_agent_run_events", { projectId, runId, conversationId }) : [];
}

export async function onAgentRunEventV3(callback: (event: AgentRunEventV3) => void): Promise<UnlistenFn> {
  if (!isTauri()) return () => undefined;
  return listen<AgentRunEventV3>("agent-run-v3-event", ({ payload }) => callback(payload));
}

export async function listAgentRunEventsV3(options: { runId?: string; projectId?: string; conversationId?: string }): Promise<AgentRunEventV3[]> {
  return isTauri() ? invoke("list_agent_run_events_v3", options) : [];
}

export async function answerAgentRunQuestionV3(runId: string, questionId: string, answer: string): Promise<void> {
  await invoke("answer_agent_run_question_v3", { runId, questionId, answer });
}

export async function agentV4StartPlanning(request: { project_id: string; conversation_id: string; model_profile_id: string; objective: string }): Promise<RunSummaryV4> {
  return invoke("agent_v4_start_planning", { request });
}

export async function agentV4ApprovePlan(runId: string, planHash: string): Promise<RunSummaryV4> {
  return invoke("agent_v4_approve_plan", { request: { run_id: runId, plan_hash: planHash } });
}

export async function agentV4Resume(runId: string): Promise<void> { await invoke("agent_v4_resume", { runId }); }
export async function agentV4Cancel(runId: string): Promise<void> { await invoke("agent_v4_cancel", { runId }); }
export async function agentV4Answer(runId: string, questionId: string, answer: string): Promise<void> { await invoke("agent_v4_answer", { request: { run_id: runId, question_id: questionId, answer } }); }
export async function agentV4Events(runId: string): Promise<AgentRunEventV4[]> { return isTauri() ? invoke("agent_v4_events", { runId }) : []; }
export async function onAgentV4Event(callback: (event: AgentRunEventV4) => void): Promise<UnlistenFn> {
  if (!isTauri()) return () => undefined;
  return listen<AgentRunEventV4>("agent-v4-event", ({ payload }) => callback(payload));
}

export async function listStepAttemptsV2(runId: string): Promise<StepAttempt[]> {
  return invoke("list_step_attempts_v2", { runId });
}

export async function listArtifactsV2(runId: string): Promise<ArtifactRecordV2[]> {
  return invoke("list_artifacts_v2", { runId });
}

export async function getEnvironmentLockV2(runId: string): Promise<EnvironmentLock | null> {
  return invoke("get_environment_lock_v2", { runId });
}
