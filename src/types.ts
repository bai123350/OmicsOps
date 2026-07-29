export type AuthenticationMethod = "password" | "private_key";

export interface ConnectionProfile {
  id: string;
  label: string;
  host: string;
  port: number;
  username: string;
  authentication: AuthenticationMethod;
  authentication_reference: string;
  host_key_fingerprint: string | null;
}

export interface ResourceLimits {
  max_cpu_cores: number;
  max_memory_gib: number;
  max_disk_gib: number;
  max_step_seconds: number;
}

export interface ProjectSpec {
  id: string;
  connection_id: string;
  remote_root: string;
  plan_summary: string;
  data_sources: Array<{ label: string; url: string; checksum: string | null }>;
  resource_limits: ResourceLimits;
  allowed_network_domains: string[];
}

export interface CompletionCondition {
  kind: "exit_code" | "file_exists" | "sha256" | "json_field" | "command_succeeds";
  target: string;
  expected: string | null;
}

export interface StepSpec {
  id: string;
  title: string;
  rationale: string;
  command: string;
  working_directory: string;
  timeout_seconds: number;
  risk: "low" | "medium" | "high" | "destructive";
  completion_conditions: CompletionCondition[];
  expected_artifacts: string[];
}

export interface StageSpec {
  id: string;
  goal: string;
  dependencies: string[];
  steps: StepSpec[];
  expected_artifacts: string[];
  completion_conditions: CompletionCondition[];
}

export interface AnalysisPlan {
  id: string;
  title: string;
  summary: string;
  stages: StageSpec[];
  resource_budget: ResourceLimits;
  highest_risk: "low" | "medium" | "high" | "destructive";
  approved: boolean;
}

export interface ServerInspection {
  os: string;
  cpuCores: number;
  memoryKib: number;
  diskAvailableKib: number;
  home: string;
  micromamba: string | null;
  remoteUser: string | null;
  projectRealPath: string | null;
  projectExists: boolean;
  projectEmpty: boolean;
  ownerMatches: boolean;
}

export interface RunEvent {
  sequence: number;
  timestamp: string;
  run_id: string;
  stage_id: string | null;
  step_id: string | null;
  attempt: number;
  action: string;
  state:
    | "draft"
    | "inspecting"
    | "awaiting_plan_approval"
    | "preparing"
    | "running"
    | "paused_for_approval"
    | "succeeded"
    | "failed"
    | "canceled";
  log_reference: string | null;
  reason: string;
}

export interface Artifact {
  remote_path: string;
  kind:
    | "h5ad"
    | "seurat_rds"
    | "html_report"
    | "figure"
    | "environment_lock"
    | "audit_log"
    | "other";
  size_bytes: number;
  sha256: string;
  previewable: boolean;
  downloadable: boolean;
}
