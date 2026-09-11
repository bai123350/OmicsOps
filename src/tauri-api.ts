import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { open, save } from "@tauri-apps/plugin-dialog";

import type {
  ConnectionProfile,
  ConnectionTestResult,
  ProjectSpec,
  ServerInspection,
  AgentRunEventV4,
  RunSummaryV4,
  ConversationAgentStateV4,
  GetConversationAgentModeResponseV4,
  SetConversationAgentModeRequestV4,
  SetConversationAgentModeResponseV4,
  AgentV4RequestPlanRevisionRequest,
  RequestPlanRevisionResponseV4,
  ComputeSelectionV4,
  ComputeBackendAvailabilityV4,
  WorkspaceConversation,
  WorkspaceProject,
  WorkspaceTemplate,
  WorkspaceMessage,
  ModelProfile,
  ModelProbeResult,
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
  McpEnvBinding,
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

export async function getConversationAgentMode(
  projectId: string,
  conversationId: string,
): Promise<GetConversationAgentModeResponseV4> {
  if (!isTauri()) {
    return { project_id: projectId, conversation_id: conversationId, mode: "agent" };
  }
  return invoke("get_conversation_agent_mode", { projectId, conversationId });
}

export async function setConversationAgentMode(
  request: SetConversationAgentModeRequestV4,
): Promise<SetConversationAgentModeResponseV4> {
  if (!isTauri()) return request;
  return invoke("set_conversation_agent_mode", { request });
}

/**
 * Read the complete durable Agent/Plan state for one conversation.
 *
 * Browser tests use the same shape as the native command so hydration and
 * reconnect behavior can be exercised without a Tauri host.
 */
export async function agentV4ConversationState(
  projectId: string,
  conversationId: string,
): Promise<ConversationAgentStateV4> {
  if (!isTauri()) {
    return {
      project_id: projectId,
      conversation_id: conversationId,
      mode: "agent",
      locked: false,
      latest_plan_revision: null,
      latest_run: null,
    };
  }
  return invoke("agent_v4_conversation_state", { projectId, conversationId });
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

export async function saveModelProfile(request: { id?: string; label: string; provider: ModelProfile["provider"]; base_url: string; model: string; credential?: string; refresh_catalog?: boolean; reasoning_effort?: ModelProfile["reasoning_effort"]; delegated_model_profile_id?: string | null }): Promise<ModelProfile> {
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

export interface SaveMcpServerRequest {
  id?: string;
  name: string;
  command: string;
  args?: string[];
  cwd?: string | null;
  timeout_secs?: number | null;
  env_bindings?: McpEnvBinding[];
}

export async function saveMcpServer(request: SaveMcpServerRequest): Promise<McpServerProfile> {
  return invoke("save_mcp_server", { request: { ...request, args: request.args ?? [] } });
}

/**
 * Create or update the native PubMed MCP preset.  The key is sent only for
 * this command so the desktop host can put it in the system credential vault;
 * it is never part of the persisted MCP profile returned to the UI.
 *
 * The command is intentionally kept small so older hosts can implement the
 * preset without exposing the MCP runtime to the webview.
 */
export async function addPubMedMcpServer(request: { api_key?: string; admin_email?: string } = {}): Promise<McpServerProfile> {
  return invoke("add_pubmed_mcp_server", {
    request: {
      api_key: request.api_key?.trim() || null,
      admin_email: request.admin_email?.trim() || null,
    },
  });
}

export async function setMcpServerEnabled(serverId: string, enabled: boolean): Promise<McpServerProfile> {
  return invoke("set_mcp_server_enabled", { request: { server_id: serverId, enabled } });
}

export async function setMcpLaunchApproval(serverId: string, approved: boolean): Promise<McpServerProfile> {
  return invoke("set_mcp_launch_approval", { request: { server_id: serverId, approved } });
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

export async function agentV4ComputeBackends(projectId: string, containerImage?: string): Promise<ComputeBackendAvailabilityV4[]> {
  if (!isTauri()) return [];
  return invoke("agent_v4_compute_backends", { request: { project_id: projectId, container_image: containerImage || null } });
}

export async function agentV4StartPlanning(request: { project_id: string; conversation_id: string; model_profile_id: string; objective: string; compute_selection: ComputeSelectionV4 }): Promise<RunSummaryV4> {
  return invoke("agent_v4_start_planning", { request });
}

export async function agentV4StartDirect(request: { project_id: string; conversation_id: string; model_profile_id: string; objective: string; compute_selection: ComputeSelectionV4 }): Promise<RunSummaryV4> {
  return invoke("agent_v4_start_direct", { request });
}

export async function agentV4ApprovePlan(runId: string, approvalHash: string, revision?: number): Promise<RunSummaryV4> {
  return invoke("agent_v4_approve_plan", { request: { run_id: runId, approval_hash: approvalHash, ...(revision === undefined ? {} : { revision }) } });
}

export async function agentV4RequestPlanRevision(request: AgentV4RequestPlanRevisionRequest): Promise<RequestPlanRevisionResponseV4> {
  return invoke("agent_v4_request_plan_revision", { request });
}

export async function agentV4Resume(runId: string): Promise<void> { await invoke("agent_v4_resume", { runId }); }
export async function agentV4Cancel(runId: string): Promise<void> { await invoke("agent_v4_cancel", { runId }); }
export async function agentV4Answer(runId: string, questionId: string, answer: string): Promise<void> { await invoke("agent_v4_answer", { request: { run_id: runId, question_id: questionId, answer } }); }
export async function agentV4DecideToolApproval(runId: string, approvalId: string, callHash: string, decision: "approved" | "denied", browserScope?: import("./types").BrowserApprovalScopeV4): Promise<void> { await invoke("agent_v4_decide_tool_approval", { request: { run_id: runId, approval_id: approvalId, call_hash: callHash, decision, ...(browserScope ? { browser_scope: browserScope } : {}) } }); }

export async function browserGetSettings(): Promise<import("./types").BrowserSettingsResponseV4> { return invoke("browser_get_settings"); }
export async function browserSaveSettings(config: import("./types").BrowserConfigV4): Promise<import("./types").BrowserSettingsResponseV4> { return invoke("browser_save_settings", { config }); }
export async function browserStatus(session: import("./types").BrowserSessionKindV4): Promise<import("./types").BrowserStatusV4> { return invoke("browser_status", { session }); }
export async function browserSetup(session: import("./types").BrowserSessionKindV4, launchIfNeeded = true): Promise<import("./types").BrowserStatusV4> { return invoke("browser_setup", { session, launchIfNeeded }); }
export async function browserCloseRunTabs(session: import("./types").BrowserSessionKindV4, runId: string): Promise<void> { await invoke("browser_close_run_tabs", { session, runId }); }
export async function browserListAuthorizations(): Promise<import("./types").BrowserAuthorizationV4[]> { return invoke("browser_list_authorizations"); }
export async function browserRevokeAuthorization(id: string): Promise<boolean> { return invoke("browser_revoke_authorization", { id }); }
export async function agentV4ResolveUncertain(runId: string, callId: string, resolution: "side_effect_observed" | "side_effect_not_observed" | "compensated", evidence: string): Promise<void> { await invoke("agent_v4_resolve_uncertain", { request: { run_id: runId, call_id: callId, resolution, evidence } }); }
export async function agentV4Events(runId: string): Promise<AgentRunEventV4[]> { return isTauri() ? invoke("agent_v4_events", { runId }) : []; }
export async function agentV4SubmitGuidance(request: import("./types").SubmitGuidanceV4Request): Promise<import("./types").GuidanceRecordV4> {
  return invoke("agent_v4_submit_guidance", { request });
}
export async function agentV4ListGuidance(projectId: string, conversationId: string, runId: string): Promise<import("./types").GuidanceRecordV4[]> {
  return isTauri() ? invoke("agent_v4_list_guidance", { projectId, conversationId, runId }) : [];
}
export async function agentV4EventsForConversation(projectId: string, conversationId: string): Promise<AgentRunEventV4[]> {
  return isTauri() ? invoke("agent_v4_events_for_conversation", { projectId, conversationId }) : [];
}
export async function onAgentV4Event(callback: (event: AgentRunEventV4) => void): Promise<UnlistenFn> {
  if (!isTauri()) return () => undefined;
  return listen<AgentRunEventV4>("agent-v4-event", ({ payload }) => callback(payload));
}

export async function agentV4CancelRuntimeRecovery(runId: string): Promise<void> { await invoke("agent_v4_cancel_runtime_recovery", { runId }); }

export async function onAgentV4TextPreview(callback: (event: import("./types").AgentTextPreviewV4) => void): Promise<UnlistenFn> {
  if (!isTauri()) return () => {};
  return listen<import("./types").AgentTextPreviewV4>("agent-v4-text-preview", ({ payload }) => callback(payload));
}
