import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { open, save } from "@tauri-apps/plugin-dialog";

import type {
  AnalysisPlan,
  Artifact,
  ConnectionProfile,
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
  StepAttempt,
  ArtifactRecordV2,
  EnvironmentLock,
  WorkspaceConversation,
  WorkspaceProject,
  WorkspaceTemplate,
  WorkspaceMessage,
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

export async function createProject(request: { name: string; description: string; local_root: string; template: WorkspaceTemplate }): Promise<WorkspaceProject> {
  if (!isTauri()) {
    const now = new Date().toISOString();
    return { id: crypto.randomUUID(), ...request, remote_root: null, connection_id: null, status: "ready", ollama_only: false, created_at: now, updated_at: now };
  }
  return invoke("create_project", { request });
}

export async function listConversations(projectId: string): Promise<WorkspaceConversation[]> {
  return isTauri() ? invoke("list_conversations", { projectId }) : [];
}

export async function createConversation(projectId: string, title: string): Promise<WorkspaceConversation> {
  if (!isTauri()) {
    const now = new Date().toISOString();
    return { id: crypto.randomUUID(), project_id: projectId, title, status: "idle", model_profile_id: null, created_at: now, updated_at: now };
  }
  return invoke("create_conversation", { request: { project_id: projectId, title } });
}

export async function listMessages(conversationId: string): Promise<WorkspaceMessage[]> {
  return isTauri() ? invoke("list_messages", { conversationId }) : [];
}

export async function submitMessage(request: { project_id: string; conversation_id: string; markdown: string; sequence: number }): Promise<WorkspaceMessage> {
  if (!isTauri()) return { id: crypto.randomUUID(), ...request, role: "user", created_at: new Date().toISOString() };
  return invoke("submit_message", { request });
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

export async function testConnection(
  profileId: string,
): Promise<{ fingerprint: string; trusted: boolean }> {
  if (!isTauri()) {
    return { fingerprint: "SHA256:demo-host-key", trusted: false };
  }
  return invoke("test_connection", { profileId });
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

export async function listStepAttemptsV2(runId: string): Promise<StepAttempt[]> {
  return invoke("list_step_attempts_v2", { runId });
}

export async function listArtifactsV2(runId: string): Promise<ArtifactRecordV2[]> {
  return invoke("list_artifacts_v2", { runId });
}

export async function getEnvironmentLockV2(runId: string): Promise<EnvironmentLock | null> {
  return invoke("get_environment_lock_v2", { runId });
}
