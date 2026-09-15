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

export interface MemoryFileSummaryV4 {
  project_id: string;
  name: string;
  size_bytes: number;
  sha256: string;
}

export interface MemoryFileV4 extends MemoryFileSummaryV4 {
  content: string;
}

export type StorageUsageScopeV4 = "managed" | "project";
export type StorageUsageCategoryV4 = "database" | "skills" | "browser" | "project_metadata" | "project_root";
export type StorageUsageStatusV4 = "complete" | "partial";
export type StorageScanIssueV4 = "not_created" | "missing" | "unreadable" | "entry_limit" | "time_limit";

export interface StorageScanLimitsV4 {
  max_entries: number;
  max_duration_ms: number;
}

export interface StorageUsageEntryV4 {
  category: StorageUsageCategoryV4;
  project_id: string | null;
  path: string;
  known_logical_bytes: number | null;
  status: StorageUsageStatusV4;
  scanned_entries: number;
  skipped_links: number;
  issue: StorageScanIssueV4 | null;
}

export interface StorageUsageSnapshotV4 {
  scope: StorageUsageScopeV4;
  project_id: string | null;
  entries: StorageUsageEntryV4[];
  known_logical_bytes: number;
  status: StorageUsageStatusV4;
  scanned_entries: number;
  skipped_links: number;
  limits: StorageScanLimitsV4;
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
  catalog_capabilities?: {
    source_provider: string;
    source_sha256: string;
    context_limit: number;
    input_limit: number | null;
    output_limit: number;
    reasoning: boolean;
    reasoning_efforts: string[] | null;
  } | null;
  reasoning_effort?: "none" | "minimal" | "low" | "medium" | "high" | "xhigh" | "max" | "ultra" | null;
  /** Null or omission uses the provider/profile default; true requests Fast. */
  fast_mode?: boolean | null;
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

/** Local stop intent status. `observed` only describes a local terminal run. */
export type StopRunStatusV4 = "requested" | "observed";

export interface StopRunRequestV4 {
  request_id: string;
  project_id: string;
  conversation_id: string;
  run_id: string;
}

export interface StopRunReceiptV4 {
  request_id: string;
  project_id: string;
  conversation_id: string;
  run_id: string;
  status: StopRunStatusV4;
  created_at: string;
  updated_at: string;
}

/** Preferences snapshotted into a new Agent V4 run. */
export interface ConversationAgentPreferencesV4 {
  delegation_enabled: boolean;
  auto_review: boolean;
  memory_enabled: boolean;
  /** Null or omission inherits the selected model profile's service tier. */
  fast_mode?: boolean | null;
}

export type ReviewerBackendChoiceV4 =
  | { kind: "follow_session" }
  | { kind: "default_http" }
  | { kind: "http_profile"; profile_id: string };

export interface ReviewerSettingsV4 {
  backend: ReviewerBackendChoiceV4;
  default_http_profile_id?: string | null;
}

export interface RunServiceTierV4 {
  fast_mode?: boolean | null;
}

export interface SessionReviewRequestV4 {
  request_id: string;
  project_id: string;
  conversation_id: string;
  model_profile_id: string;
}

export interface SessionReviewSourceV4 {
  message_id: string;
  sequence: number;
  role: string;
  text: string;
}

export type SessionReviewSeverityV4 = "error" | "warn" | "ok";

export interface SessionReviewFindingV4 {
  severity: SessionReviewSeverityV4;
  code: string;
  message: string;
  source_ids: string[];
}

export interface SessionReviewReportV4 {
  summary: string;
  findings: SessionReviewFindingV4[];
}

export type SessionReviewStatusV4 = "running" | "completed" | "failed" | "abandoned";

export interface SessionReviewRecordV4 {
  id: string;
  project_id: string;
  conversation_id: string;
  reviewer_profile_id: string;
  reviewer_configuration_hash: string;
  source_snapshot_sha256: string;
  source_message_count: number;
  sources: SessionReviewSourceV4[];
  status: SessionReviewStatusV4;
  report: SessionReviewReportV4 | null;
  error: string | null;
  service_tier?: RunServiceTierV4 | null;
  created_at: string;
  updated_at: string;
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
  | { kind: "model_request_started"; request: ModelRequestStartedV4 }
  | { kind: "model_usage_observed"; observation: ModelUsageObservationV4 }
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
  | { kind: "runtime_recovery_available"; call_ids: string[] }
  | { kind: "context_archived"; archive: { archive_id: string; through_sequence: number; size_bytes: number; sha256: string } }
  | { kind: "context_compaction_started"; request_id: string; source_through_sequence: number; source_head_hash: string; frozen_spec_hash?: string | null }
  | { kind: "context_compaction_not_needed"; request_id: string; before_bytes: number }
  | { kind: "context_compaction_completed"; request_id: string; archive: { archive_id: string; through_sequence: number; size_bytes: number; sha256: string }; checkpoint_through_sequence: number; checkpoint_sha256: string; before_bytes: number; after_bytes: number }
  | { kind: "context_compaction_attention"; request_id: string; message: string }
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

export interface BundledMcpPreset {
  id: string;
  name: string;
  description: string;
  description_zh: string;
  tool_count: number;
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

export interface AgentTextPreviewV4 { run_id: string; text: string | null }
export interface ConversationCapabilitiesV4 {
  project_id: string;
  conversation_id: string;
  skills: Array<{ id: string; name: string; enabled: boolean }>;
  mcp_servers: Array<{ id: string; name: string; enabled: boolean; tool_count: number }>;
  memory_count: number;
}
export interface AgentIterationSettingsV4 {
  max_iterations: number;
  auto_continue: boolean;
  auto_continue_limit: number;
  auto_compact: boolean;
  follow_up_questions: boolean;
}

export type ComposerReference =
  | { kind: "artifact"; project_id: string; id: string }
  | { kind: "session"; project_id: string; id: string }
  | { kind: "project"; project_id: string; id: string }
  | { kind: "execution_context"; project_id: string; backend_id: string }
  | { kind: "runtime"; project_id: string; backend_id: string; language: "python" | "r" }
  | { kind: "workspace_file"; project_id: string; backend_id: string; relative_path: string }
  | { kind: "workflow"; project_id: string; id: string }
  | { kind: "quote"; project_id: string; id: string }
  | { kind: "skill"; id: string };
export type ComposerCatalogItem = { reference: ComposerReference; label: string; description: string };
export interface ComposerTextPreview { project_id: string; backend_id: string; relative_path: string; sha256: string; text: string }
export interface CreateComposerQuoteRequest { project_id: string; conversation_id: string; backend_id: string; relative_path: string; sha256: string; text: string }
export interface ComposerAttachmentReceipt {
  id: string;
  project_id: string;
  conversation_id: string;
  name: string;
  relative_path: string;
  size_bytes: number;
  sha256: string;
  media_type: string;
}

export interface ComposerWorkflowTemplate {
  id: string;
  project_id: string;
  name: string;
  description: string;
  steps: string[];
  enabled: boolean;
}
export interface SaveComposerWorkflowRequest {
  id?: string | null;
  project_id: string;
  name: string;
  description: string;
  steps: string[];
  enabled: boolean;
}

export interface SessionReviewStartErrorV4 { kind: "rejected" | "uncertain"; message: string }

// Shared wire contract: omicsops-dto/composer_queue.rs.
export type ComposerQueueModeV4 = "agent" | "plan";
export type ComposerQueueStatusV4 = "pending" | "dispatching" | "running" | "completed" | "failed" | "cancelled" | "uncertain";
export type ComposerQueueActionV4 = "cancel" | "move_up" | "move_down" | "cut_in";
export interface EnqueueComposerTurnRequestV4 {
  request_id: string;
  message_id: string;
  run_id: string;
  project_id: string;
  conversation_id: string;
  mode: ComposerQueueModeV4;
  message_markdown: string;
  model_profile_id: string;
  compute_selection: ComputeSelectionV4;
  references: ComposerReference[];
  attachments: string[];
}
export interface UpdateComposerQueueRequestV4 {
  project_id: string;
  conversation_id: string;
  request_id: string;
  expected_revision: number;
  message_markdown: string;
  references: ComposerReference[];
  attachments: string[];
}
export interface ComposerQueueActionRequestV4 {
  project_id: string;
  conversation_id: string;
  request_id: string;
  expected_revision: number;
  action: ComposerQueueActionV4;
}
export interface DelegatedModelBindingV4 {
  profile_id: string;
  configuration_hash: string;
}
export interface ReviewerModelBindingV4 extends DelegatedModelBindingV4 {
  service_tier: RunServiceTierV4;
}
export interface ComposerQueueFrozenConfigV4 {
  model_profile_id: string;
  model_configuration_hash: string;
  conversation_preferences: ConversationAgentPreferencesV4;
  service_tier: RunServiceTierV4;
  delegated_model: DelegatedModelBindingV4 | null;
  reviewer_model: ReviewerModelBindingV4 | null;
  compute_selection: ComputeSelectionV4;
}
export interface ComposerQueueItemV4 extends Omit<EnqueueComposerTurnRequestV4, "model_profile_id" | "compute_selection"> {
  replacement_target_run_id?: string;
  replacement_receipt?: ComposerReplacementReceiptV4;
  frozen: ComposerQueueFrozenConfigV4;
  attachment_receipts: ComposerAttachmentReceipt[];
  cut_in_message_id?: string | null;
  failure_code?: "configuration_changed" | "material_changed" | "dispatch_failed" | "run_failed" | "cancelled_by_user" | "lease_uncertain" | null;
  position: number;
  revision: number;
  status: ComposerQueueStatusV4;
  created_at: string;
  updated_at: string;
}

// Shared wire contract: omicsops-dto/conversation_branches.rs.
export type ConversationBranchCheckpointKindV4 = "before_user" | "after_response";
export interface ConversationBranchCheckpointV4 {
  source_message_id: string;
  source_sequence: number;
  source_head_sequence: number;
  checkpoint_kind: ConversationBranchCheckpointKindV4;
  boundary_hash: string;
}
export interface CreateConversationBranchRequestV4 {
  request_id: string;
  project_id: string;
  source_conversation_id: string;
  source_message_id: string;
  checkpoint_kind: ConversationBranchCheckpointKindV4;
  expected_source_sequence: number;
  expected_head_sequence: number;
  expected_boundary_hash: string;
  title: string;
}
export interface ConversationBranchV4 extends ConversationBranchCheckpointV4 {
  request_id: string;
  branch_conversation_id: string;
  project_id: string;
  source_conversation_id: string;
  request_hash: string;
  state: "active" | "merged" | "archived";
  created_at: string;
  updated_at: string;
}

export type ContextLimitSourceV4 = { kind: "exact_catalog"; source_provider: string; source_sha256: string } | { kind: "configured_bound" } | { kind: "unknown" };
export interface ModelRequestStartedV4 { logical_request_id: string; attempt_id: string; model_profile_id: string; model_configuration_hash?: string | null; context_limit_tokens?: number | null; context_limit_source: ContextLimitSourceV4; serialized_request_bytes?: number | null; image_count?: number | null; image_bound_tokens?: number | null; breakdown?: Array<{ category: string; bytes?: number | null; tokens?: number | null; estimated: boolean }> | null }
export interface ModelUsageObservationV4 extends Omit<ModelRequestStartedV4, "image_count" | "breakdown"> {
  sample_index: number; state: "partial" | "final" | "interrupted"; aggregation: "cumulative" | "delta" | "unknown";
  input_tokens?: number | null; output_tokens?: number | null; reasoning_tokens?: number | null;
  cache_read_input_tokens?: number | null; cache_creation_input_tokens?: number | null; reported_total_tokens?: number | null;
  context_tokens?: number | null; serialized_request_bytes?: number | null; image_bound_tokens?: number | null;
}
export interface ObservedCounterV4 { known?: number | null; incomplete_attempts: number }
export interface UsageTotalsV4 {
  input_tokens: ObservedCounterV4; output_tokens: ObservedCounterV4; reasoning_tokens: ObservedCounterV4;
  cache_read_input_tokens: ObservedCounterV4; cache_creation_input_tokens: ObservedCounterV4; reported_total_tokens: ObservedCounterV4;
  observed_attempts: number; final_attempts: number; partial_attempts: number; interrupted_attempts: number; unknown_attempts: number;
}
export interface ContextUsageSnapshotV4 {
  project_id: string; conversation_id: string; run_id?: string | null; model_profile_id?: string | null; model_configuration_hash?: string | null;
  last_request?: ModelUsageObservationV4 | null; observed_total: UsageTotalsV4;
  current_context: { used_tokens?: number | null; max_tokens?: number | null; limit_source: ContextLimitSourceV4; estimated: boolean };
  conservative_budget: { serialized_request_bytes?: number | null; host_context_max_bytes: number; image_count?: number | null; image_bound_tokens?: number | null; fits_host_budget?: boolean | null };
  breakdown?: Array<{ category: string; bytes?: number | null; tokens?: number | null; estimated: boolean }> | null;
  latest_compaction?: ContextCompactionReceiptV4 | null;
}

export interface ReplaceComposerTurnRequestV4 {
  turn: EnqueueComposerTurnRequestV4;
  target_run_id: string;
  expected_event_sequence: number;
  expected_event_hash: string;
  stop_request_id: string;
}
export interface ComposerReplacementReceiptV4 {
  request_id: string;
  project_id: string;
  conversation_id: string;
  target_run_id: string;
  source_message_id: string | null;
  source_event_sequence: number;
  source_event_hash: string;
  stop: StopRunReceiptV4;
  accepted_at: string;
}

export interface CompactContextRequestV4 {
  request_id: string; project_id: string; conversation_id: string; run_id: string;
}
export interface ContextCompactionReceiptV4 extends CompactContextRequestV4 {
  status: "not_needed" | "completed" | "attention";
  source_through_sequence: number; source_head_hash: string; before_bytes: number;
  after_bytes?: number | null;
  archive?: { archive_id: string; through_sequence: number; size_bytes: number; sha256: string } | null;
  checkpoint_through_sequence?: number | null; checkpoint_sha256?: string | null;
  frozen_spec_hash?: string | null; message?: string | null;
  created_at: string; updated_at: string;
}

export interface CreateConversationBranchAndSendRequestV4 {
  branch: CreateConversationBranchRequestV4; message_markdown: string; mode: ComposerQueueModeV4; model_profile_id: string; compute_selection: ComputeSelectionV4;
  queue_request_id: string; queue_message_id: string; queue_run_id: string; references: ComposerReference[]; attachments: string[];
}
export interface ConversationBranchSendReceiptV4 { branch: ConversationBranchV4; queue: ComposerQueueItemV4 }

export type SideChatTurnStatusV4 = "queued" | "running" | "completed" | "no_evidence" | "failed" | "interrupted";
export type SideChatFailureCodeV4 = "configuration_changed" | "material_changed" | "source_changed" | "provider_failed" | "invalid_response" | "invalid_citation" | "lease_uncertain" | "interrupted";
export interface SideChatSendRequestV4 {
  request_id: string; parent_request_id?: string | null; project_id: string; conversation_id: string;
  model_profile_id: string; question_markdown: string; references: ComposerReference[]; attachments: string[];
}
export interface SideChatSourceV4 {
  source_id: string; message_id?: string | null; message_content_sha256?: string | null; run_id?: string | null;
  event_sequence?: number | null; event_hash?: string | null; sequence: number;
  role: string; label: string; excerpt: string;
}
export interface SideChatSourceWatermarkV4 {
  message_count: number; event_count: number; message_head_sequence?: number | null;
  event_heads: Array<{ run_id: string; sequence: number; event_hash: string }>;
}
export interface SideChatSendErrorV4 { kind: "rejected" | "unknown"; code?: SideChatFailureCodeV4 | null; message: string }
export interface SideChatTurnV4 {
  id: string; request_id: string; parent_request_id?: string | null; project_id: string; conversation_id: string;
  question_markdown: string; references: ComposerReference[]; attachments: string[];
  model_profile_id: string; model_label: string; source_snapshot_sha256: string;
  source_watermark: SideChatSourceWatermarkV4; sources: SideChatSourceV4[]; status: SideChatTurnStatusV4;
  answer_markdown?: string | null; cited_source_ids: string[]; failure_code?: SideChatFailureCodeV4 | null;
  usage?: UsageTotalsV4 | null; created_at: string; updated_at: string;
}

export type CredentialTarget =
  | { kind: "model"; id: string }
  | { kind: "ssh"; id: string }
  | { kind: "mcp_binding"; server_id: string; name: string }
  | { kind: "managed"; id: string };

export type CredentialPresence = "present" | "missing" | "unavailable";
export type CredentialValueKind = "api_key" | "password" | "ssh_private_key";

export interface CredentialConsumer {
  kind: "model" | "ssh" | "mcp" | string;
  id: string;
  label: string;
  binding_name: string | null;
}

export interface CredentialEntry {
  target: CredentialTarget;
  reference: string;
  label: string;
  presence: CredentialPresence;
  value_kind: CredentialValueKind;
  consumers: CredentialConsumer[];
  can_replace: boolean;
  can_delete: boolean;
}

export interface CreateManagedCredentialRequest { label: string; secret: string }
export type CreateCredentialResult = { kind: "saved"; entry: CredentialEntry } | { kind: "secret_save_not_confirmed"; entry: CredentialEntry };
export interface ReplaceCredentialRequest { target: CredentialTarget; expected_reference: string; expected_value_kind: CredentialValueKind; secret: string }
export type DeleteCredentialResult = { kind: "deleted" } | { kind: "in_use"; consumers: CredentialConsumer[] };
