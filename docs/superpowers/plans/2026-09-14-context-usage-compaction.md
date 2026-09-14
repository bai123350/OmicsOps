# Context usage accounting and durable compaction implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Each implementation step starts with a failing test and ends with a focused verification command.

**Goal:** Deliver D08/D09 (the context meter and usage details) and the D10 manual-compaction seam with provider-reported usage, conservative request budgeting, exact model-limit provenance, and durable archive/checkpoint receipts.

**Architecture:** Keep provider observations transient at the adapter boundary, normalize them into optional typed counters, surface them through `ModelStreamEventV4`, and persist immutable per-attempt observations in the existing V4 event chain. A materialized scoped compaction receipt gives the host an idempotent command without replacing the event chain. Existing `ContextArchiveV4` and `ContextCheckpointV4` remain the source of recovery context; compaction only changes the model projection and never removes history.

**Tech Stack:** Rust workspace crates, serde/sqlx SQLite, Tauri 2 commands, React 19/TypeScript, Vitest, and the existing `useWindowEscapeLayer` overlay stack.

**Spec:** `docs/superpowers/specs/2026-09-13-wisp-composer-parity-inventory.md` (D08, D09 and D10), `docs/superpowers/specs/2026-09-11-adaptive-checkpoint-budget.md`, and the pinned Wisp source in `C:/Users/jindong/AppData/Local/Temp/omicsops-wisp-composer-reference`.

## Global constraints

- This plan adds no required field to `ModelTurnV4`. Hundreds of model fixtures construct it directly; usage travels through an additive `ModelStreamEventV4` variant and durable events.
- A missing provider field is `None`, never zero. Explicit provider-reported zero remains `Some(0)`. A missing usage object is shown as unavailable, not as a zero-token turn.
- Keep three quantities separate everywhere: actual provider usage for one request, observed cumulative consumption across unique provider attempts, and the conservative serialized-request budget used for admission/compaction. No display, total or percentage may silently substitute one for another.
- A model window is exact only when the frozen profile carries the exact compiled catalog capability and source hash. A manually entered profile limit is a local configured bound; a profile with no usable bound is unknown. Never infer a family, prefix, proxy or gateway capability.
- Cache-read, cache-creation and cached-input counters remain separate provider facets. Do not add cache counters to input/output, subtract them, or calculate cost without a provider contract.
- Usage and compaction DTOs contain IDs, bounded numeric counters, hashes and safe status text only. They never carry API keys, credentials, raw provider responses, prompt bodies or unbounded tool output.
- Existing event hashes, frozen `RunSpecV4`, approval/spec hashes, archive hashes and checkpoint validation remain authoritative. A compaction receipt is not evidence that a scientific result was verified.
- An interrupted or retried provider attempt is retained as partial/unknown evidence. It is never silently retried from the UI and never converted into a successful or zero-usage attempt.
- Manual compaction is unavailable while the selected run is actively mutating its context. The host, not a disabled-looking client button, makes the final scoped/busy decision.
- All overlays use the window-level Escape stack. Immediate Escape closes only the visual top layer and restores focus when the surface unmounts. No network, real model, SSH host, API key or clipboard is required by deterministic tests.

---

## Investigation baseline

### Pinned Wisp behavior

`context_usage.rs` owns a `ContextUsageState` with open, docked/floating, drag, resize, detail expansion and close state. `ContextUsagePanel` receives a `ContextUsageSnapshot` containing `used`, `max`, `breakdown` and `estimated`; it renders a percentage, a segmented bar, expandable category rows and a compact/new-session nudge. It treats `max == 0` as a total-used display and otherwise distinguishes exact from estimated totals.

The pinned `main.rs` keeps ACP snapshots by session and derives the regular-session snapshot from the latest transcript usage item. It rebases the idle panel's maximum to the currently bound model but keeps the running Agent's cached model window until the next request boundary. Usage rows are folded per reply, and compaction closes the panel, emits `CompactionStarted`, and later clears the in-flight set. Its `/compact` route is a UI/model-context action, while the original transcript and completed tool rows remain available for recovery. Those interaction and presentation choices are useful parity guidance; Wisp's integer defaults and cumulative addition are not a safe OmicsOps accounting contract.

### OmicsOps seams and current gaps

- `crates/omicsops-agent/src/provider.rs` currently exposes `ProviderStreamEvent::Usage { input_tokens: u64, output_tokens: u64, provider_json: Value }`.
- `crates/omicsops-adapters/src/llm.rs` routes streamed and non-streamed responses through `usage_from_value`. It currently uses `unwrap_or(0)`, emits Anthropic usage from both message-start and message-delta-shaped values, and keeps only an allowlisted numeric `provider_json`. The adapter therefore has the right sanitization boundary but cannot currently express missing fields or cumulative semantics.
- `src-tauri/src/agent_v4.rs` maps only text and provider-retry callbacks into `ModelStreamEventV4`; `ModelStreamEventV4` currently has `TextDelta` and `ProviderRetrying`. `ModelTurnV4` has only `public_text` and `tool_calls` and must remain unchanged.
- `crates/omicsops-agent-core/src/lib.rs` builds the complete serialized context in `context_for_internal`, validates it with `ModelPortV4::validate_request`, serializes all events, calls `EventStoreV4::archive_context`, then appends `ContextArchived` and `ContextCheckpointed`. The adaptive checkpoint spec already removes the oldest `recent_steps` against the actual serialized request while retaining frozen state and evidence.
- `crates/omicsops-store/src/lib.rs` stores the full transcript in `agent_context_archives_v4`, verifies archive size/hash during migration, and appends V4 events with the existing hash-chain transaction helper. Archive insertion and the two event appends are currently separate calls; manual compaction needs one idempotent host transaction around its receipt and terminal result.
- `ModelProfile.catalog_capabilities` carries `context_limit`, `input_limit`, `output_limit` and a catalog source hash. `ModelProfile.effective_context_window_tokens()` also has legacy/configured defaults, so a UI meter must not label that fallback as an exact provider window. `RequestBudget` already measures compact provider JSON and has a separate exact image-budget path; D08 must expose those as a conservative budget, never as actual provider tokens.
- `WorkspaceShell` currently shows a `context-meter` placeholder with an unknown label. `src/tauri-api.ts` has scoped Agent V4 state/event commands but no context-usage or manual-compaction command. `useWindowEscapeLayer` in `src/features/settings/BrowserSettings.tsx` is the existing application-wide overlay stack.

## Data contract

Put the cross-boundary value types in `crates/omicsops-protocol/src/lib.rs` or a protocol-owned `context_usage.rs` module re-exported from there, and mirror them in `crates/omicsops-dto`/`src/types.ts` rather than hand-copying Rust event shapes. All new serialized fields must be additive and use explicit `Option`/`#[serde(default)]` only where old data needs to deserialize.

### Usage provenance and counters

Use these protocol-owned concepts (names are proposed so the implementation can keep the existing V4 naming convention):

```rust
enum ContextLimitSourceV4 {
    ExactCatalog { source_provider: String, source_sha256: String },
    ConfiguredBound,
    Unknown,
}

enum UsageAggregationV4 {
    Cumulative,
    Delta,
    Unknown,
}

enum UsageObservationStateV4 {
    Partial,
    Final,
    Interrupted,
}

struct ModelUsageObservationV4 {
    logical_request_id: Uuid,
    attempt_id: Uuid,
    sample_index: u32,
    model_profile_id: Uuid,
    model_configuration_hash: Option<String>,
    state: UsageObservationStateV4,
    aggregation: UsageAggregationV4,
    input_tokens: Option<u64>,
    output_tokens: Option<u64>,
    reasoning_tokens: Option<u64>,
    cache_read_input_tokens: Option<u64>,
    cache_creation_input_tokens: Option<u64>,
    reported_total_tokens: Option<u64>,
    context_tokens: Option<u64>,
    context_limit_tokens: Option<u64>,
    context_limit_source: ContextLimitSourceV4,
    serialized_request_bytes: Option<u64>,
    image_bound_tokens: Option<u64>,
}
```

The adapter can use a transient `ProviderUsageSample` with the same optional counters and aggregation/state fields. The durable observation must not retain `provider_json`; map the existing safe allowlist into typed fields and discard unknown numeric names as well as non-numeric values.

For aggregate presentation, use an explicit shape such as `ObservedCounterV4 { known: Option<u64>, incomplete_attempts: u32 }`. `known` is `None` when no value was observed; when a known partial sum is shown, `incomplete_attempts` makes its limit visible. This lets the UI display “12,345 observed; one attempt incomplete” without claiming that 12,345 is the exact total. Include observed/final/partial attempt counts in the snapshot.

### Context snapshot and compaction receipt

Expose a scoped `ContextUsageSnapshotV4` from the host rather than making the frontend reconstruct provider semantics:

```rust
struct ContextUsageSnapshotV4 {
    project_id: Uuid,
    conversation_id: Uuid,
    run_id: Option<Uuid>,
    model_profile_id: Option<Uuid>,
    model_configuration_hash: Option<String>,
    last_request: Option<ModelUsageObservationV4>,
    observed_total: UsageTotalsV4,
    current_context: ContextWindowUsageV4,
    conservative_budget: ContextBudgetV4,
    breakdown: Option<Vec<ContextUsageRowV4>>,
    latest_compaction: Option<ContextCompactionReceiptV4>,
}

struct ContextWindowUsageV4 {
    used_tokens: Option<u64>,
    max_tokens: Option<u64>,
    limit_source: ContextLimitSourceV4,
    estimated: bool,
}

struct ContextBudgetV4 {
    serialized_request_bytes: Option<u64>,
    host_context_max_bytes: u64,
    image_count: u32,
    image_bound_tokens: Option<u64>,
    fits_host_budget: Option<bool>,
}

enum ContextCompactionStatusV4 { NotNeeded, Completed, Attention }

struct CompactContextRequestV4 {
    request_id: Uuid,
    project_id: Uuid,
    conversation_id: Uuid,
    run_id: Uuid,
}

struct ContextCompactionReceiptV4 {
    request_id: Uuid,
    project_id: Uuid,
    conversation_id: Uuid,
    run_id: Uuid,
    status: ContextCompactionStatusV4,
    before_bytes: u64,
    after_bytes: Option<u64>,
    archive: Option<ContextArchiveV4>,
    checkpoint_through_sequence: Option<u64>,
    checkpoint_sha256: Option<String>,
    frozen_spec_hash: Option<String>,
    message: Option<String>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}
```

`ContextUsageRowV4` should carry a category, optional byte count, optional token count and an `estimated` flag. Do not fabricate category tokens from bytes. If only the serialized candidate is available, show category byte counts as an estimated breakdown and leave category tokens absent.

## Provider normalization and core event flow

### Provider boundary

1. Replace the transient provider usage payload with optional typed counters (or add a `ProviderUsageSample` field) while keeping the event enum's external behavior bounded. `usage_from_value` should emit a sample when at least one allowlisted numeric counter is present, including an explicit zero; it should return no numeric value for a missing, negative, string, float or object field.
2. Mark all currently supported response shapes as cumulative within one provider attempt: OpenAI-compatible final/chunk usage, Anthropic `message_start`/`message_delta`/final usage, and Ollama final usage. A future documented delta must carry `Delta`; an unknown aggregation must never be added.
3. Merge Anthropic fields by presence and monotonic replacement/max within the same attempt. For example, input 10 at message start followed by output 1 and output 7 yields input 10 and output 7, not output 8. Preserve `cache_read_input_tokens` and `cache_creation_input_tokens` independently. Do the same for OpenAI cached prompt and reasoning details.
4. Keep the existing safe metadata allowlist and do not copy raw provider response JSON. The parser and decoder tests must exercise both streaming and non-streaming paths, including repeated partial samples and missing fields.
5. Have `UnifiedModelClient`/the decoder expose the logical request and provider-attempt identity to the V4 port. The final send boundary and fallback/retry path must use the same normalized sample and the same conservative request estimator; image base64 is replaced by a bounded placeholder for JSON sizing and is never counted as text bytes.

### AgentCore boundary

1. Add `ModelStreamEventV4::Usage(ModelUsageObservationV4)` (or an equivalent usage-sample variant) and map it in `DesktopModelPortV4`. Do not add usage, context or budget fields to `ModelTurnV4`.
2. At each model boundary, create a stable `logical_request_id` and a fresh `attempt_id` for every actual provider attempt. Persist a bounded `ModelRequestStarted` event containing IDs, profile/configuration identity and budget provenance, but no prompt body. A `ModelUsageObserved` event contains the merged final/partial/interrupted observation.
3. `AttemptUsageAccumulator` merges cumulative samples by field presence and aggregation. It deduplicates `(logical_request_id, attempt_id, sample_index)` and sums each distinct attempt record at most once; known counters from a partial/interrupted attempt contribute to a visibly qualified known subtotal, while `incomplete_attempts` prevents that subtotal from being presented as an exact total. A retried request's partial consumption is retained as its own attempt; a missing field remains unknown. `Delta` values may be added only after a stable sample identity is verified; `Unknown` is replaced/marked ambiguous and never summed.
4. Refactor the `model_turn_with_policy` exits so the accumulator is flushed before every return from success, provider error, malformed/truncated response, timeout and cancellation. The current cancellation branch returns from inside `tokio::select!` before callback events are persisted; that branch must instead finalize a partial/interrupted usage event before returning `Cancelled`.
5. If the process dies after `ModelRequestStarted` and before an observation, hydration/reconciliation reports one interrupted unknown attempt. It does not write zero counters or blindly rerun the provider. The next user-approved retry gets a new attempt ID and keeps the old evidence. Existing `ModelRetrying`, `ModelText` and tool evidence remain unchanged.
6. Keep usage events out of the bounded narrative tail or render only a short safe summary in `build_checkpoint`; they must not push out frozen plan, completion criteria, active guidance, unresolved errors or scientific state. The full event chain and archive remain available for audit.

## Durable compaction transaction

### Store schema and idempotency

Add an idempotent `agent_context_compactions_v4` table in `crates/omicsops-store/migrations/init.sql` with `request_id` as the primary key, scoped `project_id`, `conversation_id` and `run_id`, status (`started`, `not_needed`, `completed`, `attention`), source sequence/hash, before/after byte fields, optional archive/checkpoint IDs and hashes, safe reason, and created/updated timestamps. Foreign keys and project/conversation indexes must match the existing V4 cleanup rules. The transcript remains only in `agent_context_archives_v4`; do not duplicate it in the receipt table.

Add Store helpers in a dedicated `context_usage.rs` module or the existing Store module:

```rust
pub async fn context_usage_v4(
    &self, project_id: Uuid, conversation_id: Uuid,
) -> Result<ContextUsageSnapshotV4, StoreError>;

pub async fn compact_context_v4(
    &self, request: &CompactContextRequestV4,
) -> Result<ContextCompactionReceiptV4, StoreError>;

pub async fn reconcile_context_compaction_v4(
    &self, project_id: Uuid, conversation_id: Uuid, run_id: Uuid,
) -> Result<Option<ContextCompactionReceiptV4>, StoreError>;
```

The production implementation owns the SQLite transaction; `EventStoreV4` gains a default-compatible `commit_context_compaction` hook for AgentCore memory fixtures, with the Store implementation overriding it atomically. Existing `archive_context` remains available for old callers during migration. The first manual slice uses the Store transaction for a paused same-run projection; it does not yet route the automatic AgentCore path through this hook.

### Compaction state machine

1. Validate request UUIDs and project/conversation/run ownership. Load the run's frozen `RunSpecV4`, verify its stored `spec_hash` and approval/plan hashes using the existing validation helpers, and accept only a genuinely paused `waiting_for_input` run with exactly one unanswered question, no pending tool approval, no durable stop request, no unresolved side-effect dispatch, and no active driver lease. Actively mutating and unknown states are rejected. A run with a durable `completed` terminal event returns an explicit `NotNeeded` receipt without appending to its immutable chain. Capture the current event-head sequence/hash and the current profile/configuration identity.
2. Under `BEGIN IMMEDIATE`, return the existing receipt for a same-`request_id` retry; reject a cross-scope reuse. For a new request, re-read the event head and insert `started` plus `ContextCompactionStarted` (source head and frozen spec hash). If the head changed, restart calculation from the new snapshot rather than compacting stale context.
3. Build a deterministic paused-run candidate from the full pre-compaction transcript, frozen plan/compute selection, bounded checkpoint and active guidance. The Store slice preserves the complete transcript in the archive and keeps the pending question in the checkpoint. It does not have a `ModelPortV4` or provider-limit admission hook, so `Completed` means that the durable same-run projection was written and reduced the serialized candidate; it does not claim that a provider request was validated. Do not trim frozen state, criteria, guidance, errors or source evidence from durable storage.
4. If the current request already fits and no stale oversized checkpoint requires rebuilding, commit `started` plus `ContextCompactionNotNeeded` and the `not_needed` receipt. This is an explicit durable outcome; no archive is created and no event/history is deleted.
5. If compaction is needed, call the existing `EventStoreV4::archive_context` semantics with the complete pre-compaction transcript. Recompute and verify the archive byte length and SHA-256, serialize/hash the checkpoint for the receipt, and ensure `checkpoint.through_sequence` is no newer than the captured source head.
6. In one Store transaction, insert the archive, append `ContextArchived`, append `ContextCheckpointed`, append `ContextCompactionCompleted`, and update the receipt to `completed`. The original event rows and archive remain intact; only subsequent context projection reads the checkpoint. Existing hash-chain verification must pass before commit.
7. On validation, archive, checkpoint or persistence failure, write a bounded safe `ContextCompactionAttention` event and `attention` receipt in a separate CAS transaction when possible. Do not append a false completion, delete the transcript, change the frozen spec, or turn a manual-compaction failure into an unrelated `RunCancelled`. If the database dies before this attention write, a later reconciliation of `started` marks it `attention` after checking for an existing archive/final event.
8. Reconciliation is conservative: a durable completed archive/checkpoint/final event returns the completed receipt; a `started` request with no complete pair becomes attention with “compaction did not finish; original context retained.” It never launches a model or performs an automatic second archive. Retrying after explicit user action reuses the same request identity only when the Store proves no completed result exists and the operation is still safe; otherwise the UI asks for a new request ID.

The automatic `context_for_internal` path remains on its existing archive/checkpoint seam in this slice and is not silently treated as using the manual receipt. A later model-aware transaction hook must share the candidate and hash rules before automatic compaction is advertised. If immutable state alone still exceeds the host/provider budget, return `NeedsAttention` with the original evidence retained.

## Tauri commands and frontend seam

Register two scoped commands in `src-tauri/src/agent_v4.rs` and the invoke handler:

```rust
#[tauri::command]
pub async fn agent_v4_context_usage(
    state: State<'_, AppState>,
    project_id: Uuid,
    conversation_id: Uuid,
) -> Result<ContextUsageSnapshotV4, String>;

#[tauri::command]
pub async fn agent_v4_compact_context(
    state: State<'_, AppState>,
    request: CompactContextRequestV4,
) -> Result<ContextCompactionReceiptV4, String>;
```

The first command reads the scoped event chain/run snapshot in one host operation. The second is the only manual compaction dispatch; `/compact`, the context panel button and a future command picker call it rather than sending the literal `/compact` text to a model. Tauri errors are generic and actionable; detailed provider/SQLite internals stay in host logs where the existing redaction boundary permits them.

Add typed wrappers to `src/tauri-api.ts`, shared DTO mirrors to `src/types.ts`, and a `useContextUsage(projectId, conversationId, runId?)` hook. The hook must cancel/ignore stale scope responses, retain the last valid snapshot while a refresh is pending, expose `loading`, `error`, `retry`, `compacting` and the durable receipt, and never synthesize an empty snapshot on browser fallback. A browser-only panel may render an explicit unavailable state; it must not claim that a compaction succeeded without the desktop command.

Replace the `WorkspaceShell` context-meter placeholder with a Wisp-aligned `ContextUsagePanel`/dialog and `context-usage.css`:

- The header shows current context only when `used_tokens` and a limit are known. It labels `Exact catalog`, `Configured bound` or `Unknown`; unknown displays “Usage unavailable”/localized equivalent and never `0 / 0`.
- Details keep separate sections for “Last request” (actual provider counters), “Observed run/session total” (deduplicated attempts with partial/unknown count), “Conservative request budget” (serialized bytes, host byte cap and image bound), and “Context window” (limit provenance). Cache-read/create and reasoning rows appear only when present.
- A breakdown is optional and bounded. Known byte rows can be expanded; absent token rows stay absent. Show model profile ID/configuration hash and archive/checkpoint IDs/hashes as provenance, never the archived transcript or raw provider JSON by default.
- The compact action is disabled for an active run, pending command, terminal/attention reconciliation or missing scope. It reports durable `NotNeeded`, `Completed` and `Attention` outcomes, leaves the panel open on failure, and offers a safe retry. A context reduction may flash/refresh the panel, but it never removes historical messages or tool evidence.
- Follow the Wisp interaction cues for docked/floating placement, drag/resize, segmented usage bar and close behavior. Keep the panel above Settings at the app's overlay z-index. Register it with `useWindowEscapeLayer` before any local focus handler, trap Tab within the active surface, restore the meter trigger on unmount, close on project/conversation switch, and ensure one immediate Escape affects only the top overlay.
- While a run is active, preserve that run's frozen model limit in the panel. When idle, a newly selected model may change the displayed future limit, but it must not mutate historical usage or a frozen run's configuration hash. ACP/other future adapters can populate the same DTO without inventing HTTP counters.

## TDD implementation sequence

### Task 1: protocol DTOs and additive events

Files: `crates/omicsops-protocol/src/lib.rs` or a new protocol context-usage module, `crates/omicsops-dto/src/lib.rs`, `src/types.ts`, and protocol/DTO contract tests.

- [ ] Add failing serde tests for optional counters, explicit zero versus missing, all limit-source variants, compaction statuses and deny-unknown request payloads. Add a legacy event-chain fixture proving old events still verify.
- [ ] Add `ModelRequestStarted`, `ModelUsageObserved`, `ContextCompactionStarted`, `ContextCompactionNotNeeded`, `ContextCompactionCompleted` and `ContextCompactionAttention` as non-terminal additive `AgentEventKindV4` variants, plus the DTOs above. Keep terminal-position validation unchanged.
- [ ] Run `cargo test -p omicsops-protocol` and the DTO contract test; then `cargo fmt --all -- --check`.

### Task 2: provider usage parser and per-attempt accumulator

Files: `crates/omicsops-agent/src/provider.rs`, `crates/omicsops-adapters/src/llm.rs`, their existing tests, and a focused adapter accumulator test module.

- [ ] First add red tests for Anthropic message-start/output partial/output cumulative samples, cache-read/create preservation, missing fields, explicit zero, OpenAI/Ollama final usage, repeated samples and safe metadata stripping.
- [ ] Implement optional typed samples, cumulative field-wise merge, provider-attempt/sample identity and no-zero semantics without counting image base64 as text. Keep model-independent `RequestBudget` image rejection and the trusted exact image path unchanged.
- [ ] Run `cargo test -p omicsops-agent --lib` and `cargo test -p omicsops-adapters --lib`; include a multiple-retry fixture proving distinct attempts are retained and not double-counted within an attempt.

### Task 3: AgentCore usage events and interruption recovery

Files: `crates/omicsops-agent-core/src/lib.rs`, `src-tauri/src/agent_v4.rs`, protocol event fixtures and AgentCore tests.

- [ ] Add red tests for `ModelStreamEventV4::Usage`, no `ModelTurnV4` field change, success/error/timeout/cancel flushes, partial retry retention, process-restart started-without-observation recovery, and bounded checkpoint rendering.
- [ ] Map provider samples in `DesktopModelPortV4`, add the attempt recorder and flush it on every `tokio::select!` exit before returning. Persist safe start/observation events and keep preview text ephemeral.
- [ ] Run `cargo test -p omicsops-agent-core --lib` and the focused desktop Agent V4 tests. Verify all existing `ModelTurnV4` fixtures compile unchanged.

### Task 4: atomic Store usage projection and compaction receipt

Files: `crates/omicsops-store/migrations/init.sql`, a new Store context-usage module or `src/lib.rs`, Store event helpers and persistence tests.

- [ ] Add failing temporary/in-memory tests for scoped usage totals, exact/configured/unknown limit provenance, same-request idempotency, cross-scope rejection, not-needed no-op, completed archive/checkpoint/hash, attention after failure/interruption, migration/reopen and no history deletion.
- [ ] Add the receipt table, scoped queries and `BEGIN IMMEDIATE` CAS/transaction implementation. Reuse archive size/hash and `insert_agent_event_in_tx`; reconcile `started` conservatively after restart.
- [ ] Route automatic core archive/checkpoint commits through the same production transaction hook while retaining the memory-store default for existing tests.
- [ ] Run `cargo test -p omicsops-store --lib`, the focused Store integration tests and `cargo fmt --all -- --check`.

### Task 5: Tauri command and API boundary

Files: `src-tauri/src/agent_v4.rs`, command registration, `src/tauri-api.ts`, `src/types.ts`, DTO contract tests.

- [ ] Add red command tests for project/conversation/run scope, busy/terminal rejection, stable request IDs, generic error mapping and browser unavailable behavior.
- [ ] Register the two commands and wrappers. Have `/compact` call the command and use one scoped host response for hydration; do not persist a model user message for the local action.
- [ ] Run focused desktop command tests and TypeScript type/build checks.

### Task 6: context meter and usage details UI

Files: new `src/features/workspace/ContextUsagePanel.tsx`, `context-usage.css`, `useContextUsage.ts` and focused tests; minimal `WorkspaceShell`/composer wiring.

- [ ] Add red React tests for exact/estimated/configured/unknown rendering, no fabricated zeros, separate actual/observed/budget sections, partial/cache rows, stale scope protection, loading/error/retry and durable compaction statuses.
- [ ] Implement the responsive teal/white Wisp-aligned panel, floating/docked details, category expansion, pending locks, immediate Escape/focus trap/restoration and `/context`/`/compact` callbacks.
- [ ] Run the focused UI tests at desktop and narrow viewport fixtures. Check computed bounds for no internal horizontal overflow and verify Settings remains below the panel's z-index.

### Task 7: integrated verification

- [ ] Run `cargo test --workspace`, `npm test`, `npm run build`, `npm run build:desktop`, `cargo fmt --all -- --check` and `git diff --check` after all integration work is complete.
- [ ] Add one deterministic end-to-end fixture that emits an Anthropic partial, a retry, a completed usage observation and a compaction receipt, then rehydrates from SQLite and renders the same scoped snapshot.
- [ ] Record real model/SSH/ACP acceptance as unexecuted unless an approved one-time environment is available. A passing mock does not establish provider tokenizer accuracy or remote-job behavior.

## Risks and explicit non-goals

- Provider token counters are evidence from the provider, not a tokenizer implementation. The conservative UTF-8/JSON estimate remains a safety budget and may be larger than actual tokens.
- If a process is terminated before its in-memory partial sample flushes, the durable `ModelRequestStarted` event proves an interrupted unknown attempt; the UI must say unknown rather than reconstructing a value. A later bounded usage inbox can improve crash-window durability without changing this contract.
- The first compaction slice is a deterministic archive/checkpoint projection for the same paused run only. A completed run is an explicit `NotNeeded` outcome because its terminal chain is immutable and a later new run does not inherit the checkpoint. The UI must keep the action unavailable until it can select a real paused run and report this boundary. This slice does not add a separate DAG scheduler, model-generated summary, scientific verification, remote-job cancellation, or automatic history deletion.
- Existing legacy `ContextArchived` rows without a matching new receipt are read and verified; they are not rewritten as successful compaction receipts without a valid archive/checkpoint/hash pair.
- UI visual parity with the pinned Wisp source is limited to the inspected context panel behavior. It does not imply Wisp's provider, ACP or pricing semantics are present in OmicsOps.

## Presentation checkpoint (2026-09-14)

`ContextUsagePanel` and the composer meter / `/context` entry now provide a nonmodal usage detail surface. It separates current context, latest provider counters, observed consumption, optional cache/reasoning facets and serialized byte/image admission estimates. Missing values remain unavailable and do not produce a fabricated percentage. Incomplete totals are labelled. The panel supports dock/float, pointer drag, native resize, expandable details and the window Escape stack with focus restoration; changing conversation closes it.

Five focused component/integration tests pass. Actual-component synthetic captures at 1280 and 480 pixels measure 422px width without horizontal overflow (`context-usage-*` in the task artifact directory). Production Web build passes. This presentation checkpoint does not yet connect the host snapshot: current absent data is explicitly unavailable. The durable host command now exists for a real paused `waiting_for_input` run, but the UI action remains disabled until it can select that run and explain the same-run-only boundary; no new-run context import or model-generated summary is claimed.

Provider accounting clarification: Claude prompt caching documents total input as `input_tokens + cache_creation_input_tokens + cache_read_input_tokens` (https://platform.claude.com/docs/en/build-with-claude/prompt-caching). This protocol-specific context quantity must be normalized at the adapter boundary with checked arithmetic and unknown-field handling. It must not be inferred generically in AgentCore, and OpenAI cached-token facets must not be added a second time. The panel explicitly labels occupancy as the latest request's input context, not the next request after generated output.
