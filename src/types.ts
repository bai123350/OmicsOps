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
  context_window_tokens?: number | null;
  delegated_model_profile_id?: string | null;
}

export interface ExecutionPlanV4 {
  schema_version: 4;
  objective: string;
  steps: string[];
  completion_criteria: string[];
  requested_capabilities: string[];
}

export type ComputeBackendKindV4 = "ssh" | "local" | "docker" | "podman";
export type AutonomyModeV4 = "supervised" | "full_auto";
export type ApprovalPolicyV4 = "request_approval" | "risk_based" | "full_access";
export type NetworkPolicyV4 = "host_inherited" | "none";

export interface ComputeSelectionV4 {
  schema_version: 4;
  backend_id: string;
  backend_kind: ComputeBackendKindV4;
  autonomy_mode: AutonomyModeV4;
  approval_policy?: ApprovalPolicyV4;
  environment: string;
  network_policy: NetworkPolicyV4;
  container_image?: { reference: string; image_id: string } | null;
}

export interface ComputeBackendAvailabilityV4 {
  descriptor: {
    schema_version: 4;
    backend_id: string;
    kind: ComputeBackendKindV4;
    isolation: "process" | "container";
    available: boolean;
    supports_python: boolean;
    supports_r: boolean;
    supports_network_policy: boolean;
  };
  selectable: boolean;
  reason: string | null;
  python_status: "available" | "unavailable" | "unverified";
  r_status: "available" | "unavailable" | "unverified";
  resolved_image_id: string | null;
}

export interface RunSummaryV4 {
  run_id: string;
  status: string;
  plan: ExecutionPlanV4 | null;
  plan_hash: string | null;
  compute_selection: ComputeSelectionV4 | null;
  approval_hash: string | null;
  plan_revision?: number | null;
  session_mode?: SessionAgentModeV4 | null;
}

export type SessionAgentModeV4 = "agent" | "plan";

export interface GetConversationAgentModeRequestV4 {
  project_id: string;
  conversation_id: string;
}

export interface GetConversationAgentModeResponseV4 extends GetConversationAgentModeRequestV4 {
  mode: SessionAgentModeV4;
}

export interface SetConversationAgentModeRequestV4 extends GetConversationAgentModeRequestV4 {
  mode: SessionAgentModeV4;
}

export interface SetConversationAgentModeResponseV4 extends SetConversationAgentModeRequestV4 {}

export type PlanRevisionStatusV4 = "generating" | "revising" | "pending" | "approved" | "superseded" | "cancelled";
export interface ProposedPlanRevisionV4 {
  id: string;
  project_id: string;
  conversation_id: string;
  run_id: string;
  revision: number;
  plan: ExecutionPlanV4;
  markdown: string;
  plan_hash: string;
  status: PlanRevisionStatusV4;
  feedback?: string | null;
  created_at: string;
  updated_at: string;
}
export interface AgentV4RequestPlanRevisionRequest {
  run_id: string;
  plan_hash: string;
  feedback: string;
}
export interface RequestPlanRevisionResponseV4 {
  run_id: string;
  revision: number;
  plan_hash: string;
  status: PlanRevisionStatusV4;
  feedback?: string | null;
}

/**
 * Durable, conversation-scoped Agent V4 snapshot.
 *
 * The snapshot is the UI hydration boundary: mode and lock state are read
 * together with the latest plan revision/run so a reconnect cannot briefly
 * expose a stale local mode or an unlocked pending plan.
 */
export interface ConversationAgentStateV4 {
  project_id: string;
  conversation_id: string;
  mode: SessionAgentModeV4;
  locked: boolean;
  latest_plan_revision: ProposedPlanRevisionV4 | null;
  latest_run: RunSummaryV4 | null;
}

export interface ToolCallV4 { call_id: string; tool_id: string; arguments: Record<string, unknown> }
export interface SubmitGuidanceV4Request {
  message_id: string;
  project_id: string;
  conversation_id: string;
  run_id: string;
  markdown: string;
}
export interface GuidanceRecordV4 extends SubmitGuidanceV4Request {
  ordinal: number;
  accepted_at: string;
  consumed_at: string | null;
}
export interface ToolOutcomeV4 { call_id: string; tool_id: string; succeeded: boolean; model_content: string; data: unknown; provenance: string[] }
export type ToolEffectV4 = "read_only" | "mutating" | "runtime" | "network" | "delegation";
export interface ToolApprovalRequestV4 { approval_id: string; call: ToolCallV4; effect: ToolEffectV4; reason: string; call_hash: string }
export type BrowserSessionKindV4 = "shared" | "workspace";
export type BrowserApprovalScopeV4 = "once" | "conversation" | "project" | "global";
export interface BrowserApprovalBindingV4 { capability: string; target_host: string; session: BrowserSessionKindV4; protocol_version: number }
export interface BrowserAuthorizationV4 { id: string; scope: BrowserApprovalScopeV4; binding: BrowserApprovalBindingV4; project_id?: string; conversation_id?: string; created_at_ms: number }
export interface BrowserConfigV4 { auto_launch: boolean; auto_close_turn_tabs: boolean; browser_path: string | null; default_search_provider: "default" | "google" | "bing" | "duckduckgo"; disabled_domains: string[]; preferred_domains: string[] }
export interface BrowserStatusV4 { session: BrowserSessionKindV4; port: number; listening: boolean; connected: boolean; protocol_version: number; extension_id: string; capabilities: string[]; tab_summaries: BrowserTabSummaryV4[] }
export interface BrowserTabSummaryV4 { session: BrowserSessionKindV4; tab_id: number; run_id?: string | null; title: string; origin: string; created_by_run: boolean }
export interface BrowserSettingsResponseV4 { config: BrowserConfigV4; shared: BrowserStatusV4; workspace: BrowserStatusV4; extension_path: string }
export interface CompletionProposalV4 {
  schema_version: number;
  summary: string;
  answer_markdown: string;
  criteria: Array<{ criterion: string; evidence: Array<Record<string, unknown>> }>;
}
export type AgentV4Phase = "routing" | "discovery" | "clarification" | "organizing" | "executing" | "verifying";
export type AgentV4TaskShape = "fast" | "multi_step";
export type AgentV4TaskStatus = "pending" | "in_progress" | "completed" | "blocked";
export interface AgentV4Task {
  id: string;
  title: string;
  status: AgentV4TaskStatus;
  blocked_reason?: string | null;
}
export type AgentEventKindV4 =
  | { kind: "run_created"; mode: "plan" | "execute" }
  | { kind: "model_text"; text: string }
  | { kind: "model_retrying"; attempt: number; class: string; message: string }
  | { kind: "tool_requested"; call: ToolCallV4 }
  | { kind: "tool_approval_requested"; request: ToolApprovalRequestV4 }
  | { kind: "tool_approval_decided"; approval_id: string; call_hash: string; decision: "approved" | "denied" }
  | { kind: "request_routed"; route: "research_retrieval" | "adaptive" }
  | { kind: "browser_connection_required"; session: BrowserSessionKindV4; protocol_version: number; message: string }
  | { kind: "browser_human_intervention_required"; session: BrowserSessionKindV4; reason: string; message: string }
  | { kind: "browser_tab_cleanup_required"; sessions: BrowserSessionKindV4[]; tabs: BrowserTabSummaryV4[]; message: string }
  | { kind: "tool_dispatch_started"; call_id: string; tool_id: string; effect: string; idempotency_key: string }
  | { kind: "tool_finished"; outcome: ToolOutcomeV4 }
  | { kind: "tool_outcome_reused"; idempotency_key: string; outcome: ToolOutcomeV4 }
  | { kind: "tool_dispatch_uncertain"; call_id: string; tool_id: string }
  | { kind: "tool_dispatch_resolved"; call_id: string; resolution: "side_effect_observed" | "side_effect_not_observed" | "compensated"; evidence: string }
  | { kind: "plan_proposed"; plan: ExecutionPlanV4; plan_hash: string }
  | { kind: "plan_approved"; plan_hash: string }
  | { kind: "plan_revision_requested"; plan_hash: string; feedback: string }
  | { kind: "run_spec_frozen"; approval_hash: string; spec_hash: string }
  | { kind: "mode_changed"; mode: "plan" | "execute" }
  | { kind: "task_shape_selected"; task_shape: AgentV4TaskShape; source: "model" | "host"; reason: string }
  | { kind: "phase_changed"; phase: AgentV4Phase }
  | { kind: "cycle_started"; cycle_id: number }
  | { kind: "cycle_finished"; cycle_id: number }
  | { kind: "task_list_updated"; revision: number; change_summary: string; tasks: AgentV4Task[] }
  | { kind: "tool_batch_started"; batch_id: number; cycle_id: number; phase: AgentV4Phase; tool_names: string[]; call_ids: string[] }
  | { kind: "tool_batch_finished"; batch_id: number; cycle_id: number; phase: AgentV4Phase; tool_names: string[]; call_ids: string[]; duration_ms: number; succeeded: number; failed: number }
  | { kind: "input_requested"; question_id: string; question: string; reason?: "scope" | "decision" | "missing_data" | "blocker" }
  | { kind: "user_input_answered"; question_id: string; answer: string }
  | { kind: "guidance_consumed"; message_id: string; markdown: string }
  | { kind: "context_archived"; archive: { archive_id: string; through_sequence: number; size_bytes: number; sha256: string } }
  | { kind: "context_checkpointed"; checkpoint: { schema_version: 4; through_sequence: number; completion_criteria: string[]; unresolved_errors: string[]; recent_steps: string[]; scientific_state: unknown; task_shape?: AgentV4TaskShape | null; phase?: AgentV4Phase | null; task_revision?: number | null; tasks?: AgentV4Task[]; cycle_id?: number | null } }
  | { kind: "scientific_state_changed"; revision: number; state_sha256: string; changes: string[] }
  | { kind: "completion_proposed" | "run_completed" | "run_cancelled" }
  | { kind: "completion_proposal_submitted"; proposal: CompletionProposalV4 }
  | { kind: "deterministic_verification_finished"; report: unknown }
  | { kind: "reviewer_finished"; report: unknown }
  | { kind: "reviewer_correction_requested"; correction: number; findings: unknown[] }
  | { kind: "delegation_graph_started"; call_id: string; graph: unknown }
  | { kind: "delegation_node_finished"; call_id: string; outcome: unknown }
  | { kind: "delegation_graph_finished"; call_id: string; outcome: unknown }
  | { kind: "run_failed"; message: string }
  | { kind: "run_needs_attention"; message: string };

export interface AgentRunEventV4 {
  schema_version: 4;
  run_id: string;
  project_id: string;
  conversation_id: string;
  sequence: number;
  occurred_at: string;
  previous_hash: string;
  event_hash: string;
  event: AgentEventKindV4;
}

export interface ModelProbeResult {
  endpoint: string;
  protocol: string;
  model: string;
  latency_ms: number;
  response_preview: string;
}

export interface RemoteFileEntry {
  relative_path: string;
  directory: boolean;
  size_bytes: number;
  modified_unix_seconds: number;
}

export interface ProjectImagePreview {
  relative_path: string;
  mime_type: string;
  size_bytes: number;
  sha256: string;
  data_url: string;
}

export interface SyncEntry {
  id: string;
  project_id: string;
  relative_path: string;
  local_relative_path: string | null;
  remote_path: string | null;
  direction: "local_to_remote" | "remote_to_local";
  size_bytes: number;
  sha256: string;
  state: "pending" | "transferring" | "paused" | "canceled" | "synced" | "conflict" | "failed";
  transferred_bytes: number;
  retry_count: number;
  error: string | null;
  updated_at: string;
}

export interface EvidenceReference { source_kind: string; source_id: string; excerpt: string; }
export interface MemoryFact { id: string; project_id: string; conversation_id: string | null; run_id: string | null; dimension: string; key: string; value: string; statement: string; evidence: EvidenceReference[]; conflicted_with: string[]; created_at: string; }
export interface ProjectArtifact { id: string; project_id: string; run_id: string | null; relative_path: string; remote_path: string | null; media_type: string; size_bytes: number; sha256: string; verified: boolean; created_at: string; }
export interface NotebookEntry { id: string; project_id: string; conversation_id: string | null; turn_id: string | null; kind: "goal" | "hypothesis" | "method" | "observation" | "decision" | "evidence" | "code" | "environment" | "command"; title: string; markdown: string; confidence: number | null; evidence_ids: string[]; artifact_ids: string[]; created_at: string; updated_at: string; }
export interface McpResult { server_name: string; capabilities: Record<string, unknown>; tools: Array<Record<string, unknown>>; result: unknown | null; audit_id: string; }

export interface McpToolDefinition {
  name: string;
  description?: string;
  inputSchema?: Record<string, unknown>;
  [key: string]: unknown;
}

/**
 * Environment variables supplied to an MCP process.
 *
 * Only one of `value` and `credential_reference` should be populated.  The
 * latter is intentionally a reference rather than the secret itself; the
 * desktop host resolves it immediately before spawning the process.
 */
export interface McpEnvBinding {
  name: string;
  value?: string | null;
  credential_reference?: string | null;
}

export interface McpServerProfile {
  id: string;
  name: string;
  command: string;
  args: string[];
  /** Optional working directory for the stdio process. */
  cwd?: string | null;
  /** MCP call timeout in seconds; the host defaults to 60 when omitted. */
  timeout_secs?: number | null;
  /** Persisted environment declarations; literal values are never rendered in the UI. */
  env_bindings?: McpEnvBinding[];
  enabled: boolean;
  launch_approved: boolean;
  approved_tools: string[];
  tools: McpToolDefinition[];
  capabilities: Record<string, unknown>;
  /** Configuration/schema state supplied by newer desktop runtimes. */
  config_version?: number;
  tool_catalog_sha256?: string | null;
  status?: "disconnected" | "connecting" | "ready" | "stale" | "failed" | "stopping" | string;
  last_error?: string | null;
  stderr_tail?: string | null;
  last_inspected_at: string | null;
  created_at: string;
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
  category?: string | null;
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
