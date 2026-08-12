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

export interface ApprovalRequest {
  id: string;
  run_id: string;
  reason: string;
  proposed_action: string;
  impact: string;
  alternatives: string[];
}

export interface RunCheckpoint {
  run_id: string;
  profile_id: string;
  project_id: string;
  plan_id: string;
  state: RunEvent["state"];
  stage_index: number;
  step_index: number;
  pending_approval: ApprovalRequest | null;
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

export type StepAction =
  | { kind: "tool"; tool_id: string; version: string; arguments: Record<string, unknown> }
  | { kind: "legacy_shell"; command: string };

export type VerificationSpec =
  | { kind: "exit_code"; expected: number }
  | { kind: "file"; path: string; min_bytes: number; sha256: string | null }
  | { kind: "json_field"; path: string; pointer: string; expected: string }
  | { kind: "table"; path: string; delimiter: string; required_columns: string[]; min_rows: number }
  | { kind: "domain_report"; path: string; validator: string };

export interface PolicyEnvelope {
  allowed_tools: string[];
  allowed_domains: string[];
  max_risk: StepSpec["risk"];
  allow_legacy_shell: boolean;
}

export interface StepSpecV2 {
  id: string;
  title: string;
  rationale: string;
  dependencies: string[];
  action: StepAction;
  working_directory: string;
  resources: ResourceLimits;
  risk: StepSpec["risk"];
  verifications: VerificationSpec[];
  expected_artifacts: string[];
}

export interface AnalysisPlanV2 {
  schema_version: 2;
  id: string;
  title: string;
  summary: string;
  environment: { kind: "micromamba"; channels: string[]; dependencies: string[] };
  stages: Array<{ id: string; goal: string; dependencies: string[]; steps: StepSpecV2[] }>;
  resource_budget: ResourceLimits;
  policy: PolicyEnvelope;
  metadata: Record<string, string>;
}

export interface ApprovedPlan {
  id: string;
  plan_id: string;
  plan_hash: string;
  policy: PolicyEnvelope;
  approved_at: string;
}

export interface PlanValidation {
  valid: boolean;
  issues: Array<{ code: string; path: string; message: string }>;
}

export interface ToolSummary {
  id: string;
  version: string;
  title: string;
  description: string;
  risk: StepSpec["risk"];
}

export type PlanningTurn =
  | { kind: "clarification"; questions: string[] }
  | { kind: "draft"; plan: AnalysisPlanV2 };

export interface RunCheckpointV2 {
  run_id: string;
  profile_id: string;
  project_id: string;
  approved_plan_id: string;
  state: "preparing" | "running" | "paused_for_approval" | "needs_attention" | "succeeded" | "failed" | "canceled";
  completed_steps: string[];
  action_hashes: Record<string, string>;
  attention_reason: string | null;
}

export interface RunEventV2 {
  sequence: number;
  timestamp: string;
  run_id: string;
  kind: "run_started" | "run_resumed" | "step_started" | "step_succeeded" | "step_failed" | "approval_required" | "verification_failed" | "needs_attention" | "run_succeeded" | "run_canceled";
  message: string;
  prev_hash: string | null;
  details: Record<string, string>;
  event_hash: string;
}

export interface VerificationResult {
  specification: VerificationSpec;
  passed: boolean;
  observed: string;
}

export interface StepAttempt {
  run_id: string;
  step_id: string;
  attempt: number;
  action_hash: string;
  process_group_id: number | null;
  started_at: string;
  finished_at: string | null;
  exit_code: number | null;
  log_path: string;
  manifest_path: string;
  verifications: VerificationResult[];
}

export interface ArtifactRecordV2 {
  run_id: string;
  source_step_id: string;
  remote_path: string;
  size_bytes: number;
  sha256: string;
  verified: boolean;
}

export interface EnvironmentLock {
  run_id: string;
  backend: string;
  remote_path: string;
  sha256: string;
  declared_dependencies: string[];
}

export interface AgentRunStreamEvent {
  run_id: string;
  sequence: number;
  timestamp: string;
  kind: "agent_started" | "model_started" | "model_action" | "tool_started" | "stdout" | "stderr" | "tool_completed" | "agent_completed" | "agent_failed";
  title: string;
  content: string;
  iteration: number | null;
}

export interface ConnectionTestResult {
  fingerprint: string;
  trusted: boolean;
  authenticated: boolean;
  latencyMs: number;
  serverOs: string | null;
  remoteUsername: string | null;
  home: string | null;
  sftpAvailable: boolean;
  pythonAvailable: boolean;
  rAvailable: boolean;
}

export type WorkspaceTemplate = "blank" | "single_cell_rna_seq" | "bulk_rna_seq" | "literature_review";
export type WorkspaceProjectStatus = "ready" | "running" | "waiting_for_input" | "needs_attention" | "archived";

export interface WorkspaceProject {
  id: string;
  name: string;
  description: string;
  local_root: string;
  remote_root: string | null;
  connection_id: string | null;
  template: WorkspaceTemplate;
  status: WorkspaceProjectStatus;
  ollama_only: boolean;
  created_at: string;
  updated_at: string;
}

export interface WorkspaceConversation {
  id: string;
  project_id: string;
  title: string;
  status: "idle" | "running" | "waiting_for_input" | "needs_attention" | "completed" | "archived";
  model_profile_id: string | null;
  created_at: string;
  updated_at: string;
}

export interface WorkspaceMessage {
  id: string;
  project_id: string;
  conversation_id: string;
  sequence: number;
  role: "user" | "assistant" | "tool" | "system";
  markdown: string;
  created_at: string;
}

export interface ModelProfile {
  id: string;
  label: string;
  provider: "anthropic" | "open_ai_compatible" | "ollama";
  base_url: string;
  model: string;
  credential_reference: string | null;
  supports_tools: boolean;
  supports_vision: boolean;
}

export interface ModelProbeResult {
  endpoint: string;
  protocol: string;
  model: string;
  latency_ms: number;
  response_preview: string;
}

export type AgentEventPayload =
  | { kind: "turn-started" }
  | { kind: "text-delta"; payload: string }
  | { kind: "tool-arguments-delta"; payload: { name: string; json_fragment: string } }
  | { kind: "approval-required"; payload: { approval_id: string; summary: string } }
  | { kind: "plan-ready"; payload: { plan_id: string; plan_hash: string } }
  | { kind: "turn-completed" }
  | { kind: "turn-failed"; payload: { message: string } };

export interface AgentEvent {
  project_id: string;
  conversation_id: string;
  turn_id: string;
  sequence: number;
  occurred_at: string;
  event: AgentEventPayload;
}

export interface PlanProposal {
  plan: AnalysisPlanV2;
  validation: PlanValidation;
  plan_hash: string;
}

export interface RemoteFileEntry {
  relative_path: string;
  directory: boolean;
  size_bytes: number;
  modified_unix_seconds: number;
}

export interface SyncEntry {
  id: string;
  project_id: string;
  relative_path: string;
  remote_path: string | null;
  direction: "local_to_remote" | "remote_to_local";
  size_bytes: number;
  sha256: string;
  state: "pending" | "transferring" | "synced" | "conflict" | "failed";
  updated_at: string;
}

export interface SkillPackage {
  id: string;
  name: string;
  version: string;
  source_path: string;
  sha256: string;
  enabled: boolean;
  capabilities: string[];
}

export interface ResearchItem {
  identifier: string;
  title: string;
  abstract_text: string | null;
  authors: string[];
  year: number | null;
  doi: string | null;
  url: string | null;
  provenance: { source: string; query: string; identifier: string; retrieved_at: string };
}

export interface ResearchSearchResult {
  source: "pubmed" | "europe-pmc" | "crossref";
  query: string;
  retrieved_at: string;
  next_cursor: string | null;
  items: ResearchItem[];
  cached: boolean;
}

export type KernelLanguage = "python" | "r";
export type KernelState = "created" | "running" | "interrupted" | "stopped";

export interface KernelSession {
  id: string;
  project_id: string;
  language: KernelLanguage;
  state: KernelState;
}

export type KernelEventPayload =
  | { kind: "started" }
  | { kind: "stdout"; payload: string }
  | { kind: "stderr"; payload: string }
  | { kind: "artifact"; payload: { relative_path: string; size_bytes: number; sha256: string } }
  | { kind: "completed" }
  | { kind: "failed"; payload: { message: string } }
  | { kind: "stopped" };

export interface KernelEvent {
  project_id: string;
  session_id: string;
  request_id: string;
  sequence: number;
  occurred_at: string;
  event: KernelEventPayload;
}

export interface KernelCellResult {
  request_id: string;
  saved_cell_index: number | null;
  events: KernelEvent[];
}

export interface FormalStepProposal {
  name: string;
  version: number;
  language: KernelLanguage;
  code: string;
  code_sha256: string;
}
