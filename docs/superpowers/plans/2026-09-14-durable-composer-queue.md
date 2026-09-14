# Durable Composer Queue Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Persist composer turns submitted while a conversation is busy, so a queued turn survives a window or process restart, remains FIFO and project/conversation scoped, and can be edited, cancelled, reordered, or used as a text-only guidance cut-in without losing its references or attachments.

**Architecture:** Add a typed DTO contract and a dedicated SQLite queue table. Acceptance freezes the exact model/configuration/material snapshot but creates no ordinary message, run, or event. A short `BEGIN IMMEDIATE` claim transaction gives one dispatcher a durable lease. Preparation happens outside SQLite; one second transaction atomically creates the reserved user message, Agent V4 run and first event, and changes the queue row to `running`. Existing run/event reconciliation is extended to settle terminal queue rows and expired leases. Queue actions use owner checks and revision-based compare-and-swap (CAS).

**Tech Stack:** Rust `omicsops-dto`, `omicsops-store`, `src-tauri` Tauri commands, SQLite/sqlx, existing Agent Runtime V4 protocol and guidance/event stores, React/TypeScript API wrappers.

**Spec:** E01 durable composer queue; fixed Wisp reference behavior in `main.rs` lines 3960–4006 and 4645–4715 and `agent_turn.rs` lines 1338–1475; no generic DAG or background scheduler.

## Global Constraints

- This document is design-only. The implementation starts only after the root agent reviews and accepts the contracts and transaction boundaries below.
- A queue row is always owned by one `(project_id, conversation_id)` pair. Every read and mutation receives both IDs and validates ownership in the same SQLite transaction that changes the row.
- `pending` rows must not insert an ordinary `messages` row, advance the conversation sequence, change a title, create an `agent_runs_v4` row, append an event, or enter the active run's model context. The queued text is durable queue state until dispatch.
- The host, rather than the browser, freezes model settings and material. A queued dispatch must reject a changed profile, preference, service tier, delegated/reviewer binding, compute selection, reference snapshot, or attachment snapshot. It must never silently substitute current settings.
- SQLite contains IDs, typed JSON snapshots, bounded sanitized context, hashes, and metadata. It never contains attachment bytes, client absolute paths, credentials, private keys, or raw provider errors. Attachment IDs continue to resolve only through the existing immutable staged receipt/manifest path.
- `request_id`, `message_id`, and `run_id` are generated once before the first enqueue invoke and are reused for a retry, response loss, restart reconciliation, or deterministic retry. An unknown dispatch result must reconcile these IDs and must not create new ones.
- A conversation has at most one active Agent V4 run/plan. The queue driver may claim only the lowest eligible row for that conversation. Queue order is durable and is not a process-local vector.
- Existing plan locks, stop requests, review locks, approval hashes, event-chain validation, attachment bounds, reference resolver authorization, and frozen run specs remain authoritative. A queue command cannot bypass them.
- Every new schema change is additive and idempotent. Preserve legacy database rows and legacy `RunSpecV4` serialization. Do not store a second copy of an existing preference or protocol type in the DTO crate.
- Do not treat a released lease or a local stop as proof that a remote SSH/job computation was cancelled. Queue and run statuses describe the local durable lifecycle only.

## Evidence from the current code and reference

The fixed Wisp source demonstrates the user-visible behavior but is not a persistence design. `main.rs:3960-4006` clears the composer and optimistically adds a process-local `QueuedUser`, then invokes `enqueue_turn` with text, attachment paths, and references. `main.rs:4645-4715` edits/cancels/cuts in/reorders by a process-local numeric ID; edit restores text only, cut-in carries only text, and action failures are not durable. `agent_turn.rs:1338-1435` stores `QueuedItem` in `SessionRuntime`, drains a `Vec` FIFO, and loses the queue when the process exits. `agent_turn.rs:1437-1475` confirms that the current cut-in and edit paths can discard non-text payloads.

OmicsOps currently has separate side effects. `src/DesktopApp.tsx:1340-1410` and `:1380-1410` validate material, call `submitMessage`, then call `agentV4StartPlanning` or `agentV4StartDirect`; `src-tauri/src/agent_commands.rs:81-105` allocates a fresh message UUID inside `submit_message`. The current WorkspaceShell disables the composer while a run is active (`src/features/workspace/WorkspaceShell.tsx:355-370`), so the later UI integration must deliberately route a busy normal send to the queue command.

`src-tauri/src/agent_v4.rs:401-548` (planning) and `:781-934` (direct) resolve current preferences/profile, references, attachments, service tier, reviewer/delegation, and compute selection before creating a run. The direct path currently lets `compose` create the run ID (`:812-875`), while planning uses `Store::start_plan_run_v4` with a separate transaction (`crates/omicsops-store/src/lib.rs:1604-1780`). Both paths therefore need an internal queued-start seam that accepts the reserved ID and frozen values.

The reusable Store boundaries are `save_message_with_first_title` (`crates/omicsops-store/src/lib.rs:1032-1105`), `save_agent_run_v4_if_unlocked` (`:1566-1600`), `start_plan_run_v4` (`:1604-1780`), `append_agent_event_v4_with_conversation` (`:3605-3710`), and the private transaction helpers `insert_message` (`:5724`), `ensure_conversation_owner_executor` (`:6149`), `ensure_conversation_unlocked_executor` (`:6223`), and `insert_agent_event_in_tx` (`:6911`). These helpers must be extracted or parameterized for one queue dispatch transaction rather than called through nested public transactions.

The existing schema has owner FKs and sequence constraints for `messages`, `agent_runs_v4`, `agent_events_v4`, and `proposed_plans` (`crates/omicsops-store/migrations/init.sql:71-220`). `Store::initialize` runs idempotent `INIT_SQL` inside a transaction and currently records schema version 4 (`crates/omicsops-store/src/lib.rs:41,4469-4515`). A queue table should be added using the same migration path, with a version decision made during implementation instead of silently changing `user_version` in a one-off command.

`crates/omicsops-store/src/guidance.rs:8-183` accepts bounded text guidance under `BEGIN IMMEDIATE`, validates that the run is an owned running ordinary run, and `consume_guidance_v4` appends `GuidanceConsumed` events in the same transaction. A cut-in with files or references cannot use this path after dropping those payloads. `src-tauri/src/run_ownership.rs:6-44` supplies a process/window file lease for a run; it is useful after queue dispatch, but is not a durable queue lease and is not evidence of successful dispatch.

## Typed contracts

Add `crates/omicsops-dto/src/composer_queue.rs` and re-export it from `crates/omicsops-dto/src/lib.rs`. Use the existing protocol types by import/re-export: `ComputeSelectionV4`, `ConversationAgentPreferencesV4`, `DelegatedModelBindingV4`, `ReviewerModelBindingV4`, and `RunServiceTierV4`; use the existing `ComposerReference` and `ComposerAttachmentReceipt`. Mirror the public wire shapes in `src/types.ts`; do not create a UI-only copy of the Rust queue item.

The proposed DTOs are:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ComposerQueueModeV4 {
    Agent,
    Plan,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ComposerQueueStatusV4 {
    Pending,
    Dispatching,
    Running,
    Completed,
    Failed,
    Cancelled,
    Uncertain,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ComposerQueueFailureCodeV4 {
    InvalidInput,
    ConfigurationChanged,
    MaterialChanged,
    MissingAttachment,
    ConversationBusy,
    DispatchFailed,
    RunFailed,
    CancelledByUser,
    LeaseUncertain,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ComposerQueueFrozenConfigV4 {
    pub model_profile_id: Uuid,
    pub model_configuration_hash: String,
    pub conversation_preferences: ConversationAgentPreferencesV4,
    pub service_tier: RunServiceTierV4,
    pub delegated_model: Option<DelegatedModelBindingV4>,
    pub reviewer_model: Option<ReviewerModelBindingV4>,
    pub compute_selection: ComputeSelectionV4,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ComposerQueueMaterialSnapshotV4 {
    pub reference_context: String,
    pub reference_context_sha256: String,
    pub attachment_receipts: Vec<ComposerAttachmentReceipt>,
    pub attachment_snapshot_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ComposerQueueFailureV4 {
    pub code: ComposerQueueFailureCodeV4,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ComposerQueueItemV4 {
    pub request_id: Uuid,
    pub message_id: Uuid,
    pub run_id: Uuid,
    pub project_id: Uuid,
    pub conversation_id: Uuid,
    pub position: u64,
    pub revision: u64,
    pub mode: ComposerQueueModeV4,
    pub message_markdown: String,
    pub frozen: ComposerQueueFrozenConfigV4,
    pub references: Vec<ComposerReference>,
    pub attachments: Vec<Uuid>,
    pub material: ComposerQueueMaterialSnapshotV4,
    pub status: ComposerQueueStatusV4,
    pub dispatch_attempt: u32,
    pub lease_owner: Option<String>,
    pub lease_expires_at: Option<DateTime<Utc>>,
    pub cutin_message_id: Option<Uuid>,
    pub failure: Option<ComposerQueueFailureV4>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
```

`reference_context` is the already sanitized, bounded result of the existing reference resolver plus attachment material. Cap it at the existing 64 KiB aggregate limit. `attachment_receipts` is a bounded metadata snapshot for restart/restore; it contains no bytes. The raw `references` and attachment UUIDs remain in the item so edit/restore does not depend on an in-memory chip list. The material hashes are hashes of canonical serialized snapshots and must be recomputed before dispatch.

Use these request/result types:

```rust
pub struct EnqueueComposerTurnRequestV4 {
    pub request_id: Uuid,
    pub message_id: Uuid,
    pub run_id: Uuid,
    pub project_id: Uuid,
    pub conversation_id: Uuid,
    pub mode: ComposerQueueModeV4,
    pub message_markdown: String,
    pub model_profile_id: Uuid,
    pub compute_selection: ComputeSelectionV4,
    pub references: Vec<ComposerReference>,
    pub attachments: Vec<Uuid>,
}

pub struct UpdateComposerQueueRequestV4 {
    pub project_id: Uuid,
    pub conversation_id: Uuid,
    pub request_id: Uuid,
    pub expected_revision: u64,
    pub message_markdown: String,
    pub references: Vec<ComposerReference>,
    pub attachments: Vec<Uuid>,
}

pub struct ComposerQueueActionRequestV4 {
    pub project_id: Uuid,
    pub conversation_id: Uuid,
    pub request_id: Uuid,
    pub expected_revision: u64,
    pub action: ComposerQueueActionV4,
}

pub enum ComposerQueueActionV4 {
    Cancel,
    CutIn,
    MoveUp,
    MoveDown,
    Retry,
    Reconcile,
}

pub struct ComposerQueueDispatchLeaseV4 {
    pub item: ComposerQueueItemV4,
    pub lease_token: String,
}
```

The implementation may use a single action request for move/cancel/cut-in, but it must keep a distinct update request for full payload replacement. All DTOs that cross the Tauri boundary should use `serde(deny_unknown_fields)` where current compatibility allows it, and all strings must be bounded before persistence. A request reusing an existing `request_id` with a different scope or payload is an error; an exact duplicate returns the original row.

## SQLite shape and invariants

Add a dedicated `composer_queue_v4` table to `crates/omicsops-store/migrations/init.sql` (and the established migration/version path if the schema version is incremented):

```sql
CREATE TABLE IF NOT EXISTS composer_queue_v4 (
    request_id TEXT PRIMARY KEY CHECK (length(trim(request_id)) > 0),
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    conversation_id TEXT NOT NULL REFERENCES conversation_records(frame_id) ON DELETE CASCADE,
    message_id TEXT NOT NULL UNIQUE CHECK (length(trim(message_id)) > 0),
    run_id TEXT NOT NULL UNIQUE CHECK (length(trim(run_id)) > 0),
    position INTEGER NOT NULL CHECK (position > 0),
    revision INTEGER NOT NULL CHECK (revision > 0),
    mode TEXT NOT NULL CHECK (mode IN ('agent', 'plan')),
    status TEXT NOT NULL CHECK (status IN ('pending','dispatching','running','completed','failed','cancelled','uncertain')),
    message_markdown TEXT NOT NULL,
    frozen_json TEXT NOT NULL,
    references_json TEXT NOT NULL,
    attachments_json TEXT NOT NULL,
    material_json TEXT NOT NULL,
    dispatch_attempt INTEGER NOT NULL DEFAULT 0 CHECK (dispatch_attempt >= 0),
    lease_owner TEXT,
    lease_expires_at INTEGER,
    cutin_message_id TEXT,
    failure_code TEXT,
    failure_message TEXT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    UNIQUE (conversation_id, position)
);

CREATE INDEX IF NOT EXISTS idx_composer_queue_fifo
    ON composer_queue_v4(project_id, conversation_id, status, position);
CREATE INDEX IF NOT EXISTS idx_composer_queue_lease
    ON composer_queue_v4(status, lease_expires_at);
```

The pending queue cannot have foreign keys to `messages` or `agent_runs_v4`, because those rows intentionally do not exist until dispatch. Store code must enforce the one-to-one `message_id`/`run_id` invariants in the dispatch transaction and reject an existing ID from another scope. `position` is unique for the conversation, is assigned as `MAX(position)+1` under `BEGIN IMMEDIATE`, and is swapped for adjacent pending rows. Check overflow and renumber active rows transactionally if needed; never use a wall-clock timestamp as FIFO order.

Keep terminal rows until an explicit list/ack retention policy exists, so a restart can explain a completed, failed, cancelled, or uncertain request. Terminal rows still retain references, attachment IDs/receipts, frozen config, and failure metadata. Conversation/project deletion cascades pending terminal rows through the owner FKs, but deletion must be serialized with queue transactions and must reject an active `dispatching`/`running`/`uncertain` row until its linked run/lease is reconciled. Do not delete staged attachment files as a side effect of queue-row deletion; existing frozen runs may still reference them.

## Store API and command seams

Add methods to `Store` (a `composer_queue` module is preferred if it avoids growing unrelated `lib.rs` responsibilities):

- `list_composer_queue(project_id, conversation_id) -> Result<Vec<ComposerQueueItemV4>, StoreError>`: owner-check, return rows ordered by `position`, and include terminal rows until the UI intentionally filters them.
- `enqueue_composer_turn(request, frozen, material) -> Result<ComposerQueueItemV4, StoreError>`: insert idempotently under `BEGIN IMMEDIATE`; exact duplicate returns the original row, conflicting request ID or reserved IDs fail.
- `update_composer_queue(request, material) -> Result<ComposerQueueItemV4, StoreError>`: CAS only `pending` rows using owner, request ID, and expected revision; replace all three payload fields and material snapshot while preserving the frozen config; increment revision.
- `cancel_composer_queue(request) -> Result<ComposerQueueItemV4, StoreError>`: CAS `pending` to `cancelled`; a `dispatching` row can be cancelled only by its lease owner before the atomic start boundary. A `running` row is stopped through the existing scoped stop API using its stored `run_id`; it is not silently deleted.
- `move_composer_queue(request) -> Result<Vec<ComposerQueueItemV4>, StoreError>`: in one `BEGIN IMMEDIATE`, CAS the target pending row, swap positions with the adjacent pending row, increment both revisions, and return fresh FIFO rows. Moving a terminal, dispatching, or running row is rejected.
- `claim_next_composer_queue(project_id, conversation_id, dispatcher_id) -> Result<Option<ComposerQueueDispatchLeaseV4>, StoreError>`: owner-check, verify no active run/plan/stop/review blocks the conversation, select the lowest pending row, and atomically set `dispatching`, increment `dispatch_attempt`, set a bounded lease owner and expiry, and increment revision.
- `commit_composer_queue_dispatch(lease, prepared_run, first_events) -> Result<ComposerQueueItemV4, StoreError>`: the atomic start transaction described below; CAS the exact lease and row revision, insert the reserved message/run/event, and set `running`.
- `finish_composer_queue_for_terminal_event(event/run_id)`: update `running` to `completed`, `failed`, or `cancelled` in the same transaction that persists the terminal Agent event/message. A `needs_attention` or waiting state leaves the queue `running` and the linked run summary explains the pause.
- `reconcile_composer_queue(project_id, conversation_id, dispatcher_id) -> Result<Vec<ComposerQueueItemV4>, StoreError>`: inspect expired dispatch leases and linked rows/events; settle only states proven by durable evidence.

Tauri commands should expose the same scope explicitly, with names such as `composer_queue_list`, `composer_queue_enqueue`, `composer_queue_update`, `composer_queue_action`, and `composer_queue_reconcile`. The browser API wrapper should expose `listComposerQueue`, `enqueueComposerTurn`, `updateComposerQueue`, `actComposerQueue`, and `reconcileComposerQueue`. Preview/browser mode must return an explicit unsupported error for enqueue/action/reconcile; it must not report a fake durable success. List can return an empty queue only if the current preview contract clearly states that persistence is unavailable.

The enqueue command owns the freeze step. It accepts only `model_profile_id` and `compute_selection` from the request, resolves the current exact profile and conversation preferences, derives service tier and reviewer/delegated bindings with the existing helpers, validates compute, resolves references/files, validates attachment receipts/model image budget, and writes the returned `ComposerQueueFrozenConfigV4` and material snapshot. The client cannot submit an arbitrary configuration hash or binding as authority.

## Enqueue acceptance sequence

Implement acceptance in this order:

1. Validate the non-empty bounded message, mode, IDs, reference ownership, attachment count/size, and queue count. Reuse existing limits: eight attachments/40 MiB aggregate, 64 KiB combined material, bounded composer message, and existing reference resolver limits.
2. Validate project/conversation ownership. Resolve the exact model profile and compute binding, load the conversation preference snapshot, derive service tier and reviewer/delegated bindings, and resolve/sanitize reference and attachment material. Build canonical hashes and a bounded `ComposerQueueMaterialSnapshotV4`.
3. Open `BEGIN IMMEDIATE`, revalidate owner, profile configuration hash, preference value, and material fingerprints against the preflight values. If any changed during resolution, abort with a deterministic stale/configuration error and ask the caller to retry the same logical enqueue after a fresh freeze; do not persist a mixed snapshot.
4. Check idempotency. If `request_id` exists with the exact owner and canonical request payload, return that row. If its owner, IDs, mode, text, references, attachments, or frozen snapshot differ, return an ID collision error. Also reject `message_id` or `run_id` already used in another queue/run/message scope.
5. Assign `position = MAX(position)+1` for the conversation, insert `revision=1`, `status='pending'`, frozen config, raw IDs, receipt/context snapshot, and timestamps, then commit. No `messages`, `agent_runs_v4`, `agent_events_v4`, title, sequence, or active-run context write occurs here.
6. Return the committed row. The UI may show an optimistic row only keyed by the same `request_id`; on a lost response it lists/reconciles before retrying. If the active run finished between preflight and commit, leave the item pending and let the same durable dispatcher claim it; do not bypass the queue by starting a second direct call.

Editing a pending row re-runs only the content/material validation, recomputes the material snapshot, and retains the original frozen config. If a user wants a different model/configuration, the UI must cancel the old pending row and enqueue a new logical request with new IDs; an edit must never silently unfreeze the accepted run settings.

## Dispatch and atomic start sequence

The dispatcher has two distinct transactions and one non-transactional preparation phase:

1. **Claim:** `claim_next_composer_queue` uses `BEGIN IMMEDIATE`. It selects the lowest `pending` row, checks the conversation has no active run/plan/stop/review and that no older pending row is eligible, then updates it to `dispatching`, increments `dispatch_attempt` and `revision`, and records `lease_owner`, `lease_expires_at`, and a random lease token. It commits before any model/provider work.
2. **Prepare outside SQLite:** load the exact stored profile by `model_profile_id` and require the stored `model_configuration_hash`; load and compare the stored conversation preferences, service tier, reviewer/delegated bindings, and compute selection; revalidate staged attachment manifests/hashes and reference source fingerprints/context. Reconstruct the `RunRecordV4`/`RunSpecV4` using the stored `run_id`, stored config, stored context, and stored material. Refactor `compose` and the planning seed path to accept a supplied run ID and frozen values. A mismatch is a deterministic `failed` row with `configuration_changed` or `material_changed`; it is never repaired by current settings.
3. **Atomic start:** `commit_composer_queue_dispatch` opens `BEGIN IMMEDIATE` and CASes `(request_id, project_id, conversation_id, revision, status='dispatching', lease token)`. It rechecks owner/lock and the absence of an existing conflicting message/run. It then, in this one transaction:
   - chooses the next conversation message sequence and inserts the reserved `message_id` as a user message;
   - updates the first conversation title in the same transaction when this is the first user message, using the same title derivation as `save_message_with_first_title`;
   - inserts the run JSON with the reserved `run_id` and exact frozen config/spec;
   - for Plan mode, creates the generating plan seed and Plan mode setting through an extracted transaction helper equivalent to `start_plan_run_v4`;
   - inserts the first `RunCreated` event (and `RunSpecFrozen` when the prepared direct spec is available) through `insert_agent_event_in_tx`;
   - changes the queue row to `running`, clears its lease, and records the new revision.
   Any failure rolls back every listed side effect. Do not call `submit_message`, `save_message`, `save_agent_run_v4_if_unlocked`, `start_plan_run_v4`, or the public event method from inside this transaction because they open their own transaction or allocate IDs.
4. **Start/continue:** after commit, acquire the existing process run lease with the stored `run_id` and spawn the direct executor or planning continuation. A spawn/prepare error after the atomic commit persists a deterministic run failure event and queue `failed`; a process crash is handled by reconciliation. The durable run/event is the source of truth even if the Tauri emit or command response is lost.
5. **Terminal update:** extend the existing event persistence path so a terminal Agent event and its terminal assistant message, if any, update the linked queue row in the same SQLite transaction. `RunCompleted` maps to `completed`, `RunFailed`/deterministic terminal attention maps to `failed`, and `RunCancelled` maps to `cancelled`. Waiting for input, approval, or plan approval remains `running` because the requested queue status set has no paused state.

For Plan mode, `running` covers generating, awaiting approval, revision, and the eventual approved execution attached to the same `run_id`. The next queue item must not dispatch while the plan lock or run remains active. Plan approval cannot replace the stored model/configuration snapshot; the existing plan approval/spec hash checks remain mandatory.

## Status semantics and state transitions

Use only the seven public statuses in the DTO and document the following boundaries:

| Status | Durable meaning | Allowed next states |
| --- | --- | --- |
| `pending` | Payload accepted; no message/run/event side effect; eligible FIFO item. | `dispatching`, `cancelled`, `failed` for deterministic validation. |
| `dispatching` | A dispatcher lease owns preparation or text-only cut-in handling; the atomic start boundary has not been proven. | `running`, `pending` only when the lease is explicitly released before side effects, `failed` for proven pre-start failure, `uncertain` after lease expiry/ambiguous side effects. |
| `running` | Reserved message/run/first event committed; linked run may be executing, waiting for input/approval, or planning. | `completed`, `failed`, `cancelled`, `uncertain` only after reconciliation evidence. |
| `completed` | Linked run emitted `RunCompleted` and the terminal event/message is durable. | No automatic transition. |
| `failed` | Deterministic pre-start failure or linked run terminal failure; bounded safe failure code/message retained. | Explicit retry only when no run/message side effect exists, reusing the same IDs only after the host proves that fact. |
| `cancelled` | User cancelled before dispatch, or linked run emitted `RunCancelled`. | No automatic transition. |
| `uncertain` | The host cannot prove whether the dispatch commit or side effect crossed the boundary, usually after a lost response or expired lease. | `running`/terminal after evidence; no blind retry. |

`needs_attention` is a run/event state, not a new queue status. Keep the queue `running` until the run reaches a terminal state or an explicit host reconciliation records `failed`. A local stop request sets the run's existing stopping behavior; it does not immediately mark the queue `cancelled` until an observed terminal event is durable.

## Cross-window, restart, and uncertain reconciliation

The durable lease is separate from `run_ownership`'s file lease. Use a bounded dispatcher identity such as a process nonce plus Tauri window label and a short lease expiry. Do not rely on an in-memory `SessionRuntime`, `active_runs`, or a window callback for ownership.

On startup or conversation hydration, run event reconciliation before using a stale run snapshot, then list/reconcile the queue. For every expired `dispatching` row:

- If the stored `run_id` exists with the same project/conversation, validate its event chain and run status. Set the queue to `running` or the proven terminal status and resume the existing run with the same ID when the existing resume path permits it.
- If the run is absent but the reserved `message_id` exists in the same scope, mark `uncertain`; the message crossed a side-effect boundary without a provable run. Never delete the message or start another run automatically.
- If both rows are absent, a lease expiry may be returned to `pending` only when the dispatcher recorded that preparation had not entered the atomic start transaction and the lease was explicitly released. If the process died during or after the commit attempt, retain `uncertain` until a host check proves no side effect.
- If rows/events are contradictory, preserve the queue payload and IDs as `uncertain`, expose a bounded recovery error, and require an explicit reconciliation/resolution action. A timed-out `invoke` is not evidence that dispatch failed.

Only one window can win each CAS. A stale window receives a revision/lease conflict, refetches the scoped queue, and does not apply its optimistic reorder/edit/cancel result. Event publication failures do not roll back a committed queue transition and must not cause a second dispatch.

## Edit, cancel, reorder, and cut-in behavior

- **Edit/restore:** An edit is a full payload replacement for a `pending` row. The store keeps `references` and `attachments` when the caller supplies them and returns the fresh row/revision. WorkspaceShell restores the returned text, all reference chips, and attachment receipts from that row. It must not restore only text as the Wisp implementation does.
- **Cancel:** A pending row transitions to `cancelled` and remains auditable. A dispatching row is cancellable only by its lease owner before commit. A running row uses the existing scoped `request_stop` with the stored run ID; the queue stays `running` until an observed terminal event. An uncertain row is reconciled, never blindly restarted or deleted.
- **Reorder:** Move only pending rows. Under one CAS transaction swap adjacent positions and increment both revisions. Return a fresh scoped FIFO list so another window cannot retain a false order.
- **Text-only cut-in:** Require `mode=Agent`, an active ordinary running run, no stop request, and empty references and attachments. Persist or reuse `cutin_message_id` in the queue row, call `accept_guidance_v4` with the bounded `message_markdown`, and let the existing guidance consumer append `GuidanceConsumed` under its frozen spec. Mark the queue row `completed` only when that consumption event is durable; until then use `dispatching` with the lease/cutin ID so a second driver cannot handle it.
- **Cut-in with material:** Never send only the text and clear files/references. The recommended behavior is a deterministic rejected action that leaves the complete row `pending`, with a UI message that cut-in is available for plain text only. The normal FIFO dispatch then retains and sends the full payload. If a later product decision wants “guidance first, then full turn,” it must add an explicit durable internal cut-in phase and tests; it must not overload the seven public statuses or drop material.

## Failure handling and bounds

Failure text is generated by the host, trimmed to a small fixed UTF-8 bound, and classified by `ComposerQueueFailureCodeV4`. Do not persist provider responses, URLs containing credentials, stack traces, or raw attachment/reference content in the error field. Keep sanitized reference context only in the material snapshot, subject to the existing 64 KiB cap.

Deterministic pre-start failures (`invalid_input`, `configuration_changed`, `material_changed`, `missing_attachment`, `conversation_busy`) do not create messages/runs/events and may be retried explicitly with the same IDs only after the user edits/revalidates the row. `dispatch_failed` after the atomic start must be represented by the linked run/event and queue `failed`; if the side effect status is unknown, use `uncertain` instead. Do not expose a generic “cancelled” label for an observed run that actually completed or failed.

## Implementation tasks after design review

- [ ] Add the DTO module, re-exports, TypeScript wire types, JSON contract tests, enum/status transition tests, and exact UUID/revision fields. Verify old DTO serialization remains unchanged.
- [ ] Add the additive queue table/indexes and migration/version handling. Add Store parsers/serializers with bounded JSON and owner/scope checks. Test fresh, existing-v4, repeated-open, and conversation/project deletion behavior.
- [ ] Implement enqueue/list/update/cancel/move/CAS methods. Test duplicate exact request, conflicting request/ID collision, two concurrent enqueue calls, stale edit/reorder revision, FIFO order, queue caps, and cross-project/cross-conversation rejection.
- [ ] Extract transaction-local message/title, run/plan seed, and event helpers. Implement the atomic dispatch commit with reserved `message_id`/`run_id`; test rollback at each insert and prove no pending message/run/event exists before dispatch.
- [ ] Add host freeze/preparation and exact config/material hash checks. Test profile/preference/service-tier/delegated/reviewer/compute changes between enqueue and dispatch, deleted/tampered attachments, changed references, and rejection without fallback.
- [ ] Integrate the durable dispatcher with direct and planning start paths. Test direct, planning/approval/revision, run lease acquisition, model preparation failure, spawn failure, terminal queue transitions, and no second active run.
- [ ] Add durable lease heartbeat/release/reconcile. Test two windows racing to claim, lease expiry with no side effect, message-only evidence, run/event evidence, lost invoke response, restart hydration, and explicit uncertain recovery without new IDs.
- [ ] Integrate edit/restore/cancel/reorder and text-only guidance cut-in. Test that attachments/references survive every action, material cut-in is rejected without mutation, guidance is consumed once, and a cut-in race with run termination requeues/settles safely.
- [ ] Add Tauri/frontend wrappers and WorkspaceShell queue UI after the store/host contract is stable. Test busy-send routing, scoped hydration/generation guards, optimistic rollback, window-stack Escape behavior, retry reachability, and browser-mode unsupported behavior.
- [ ] Run the closest deterministic checks, then the repository defaults required by AGENTS.md: `cargo test --workspace`, `npm test`, `npm run build`, and `npm run build:desktop` because Tauri commands/registration change. Record any ignored real-model/SSH acceptance separately.

## Real implementation difficulties to resolve explicitly

1. The current direct `compose` allocates a run ID and both start functions read live settings. Stable queued IDs and frozen settings require an internal start/compose seam; wrapping the current public commands would duplicate messages or silently change configuration.
2. The current Store public methods each open a transaction. Atomic queue dispatch needs transaction-local helpers and a clearly defined boundary before model calls; nested calls cannot satisfy the message+run+event+queue atomicity requirement.
3. SQLite cannot enforce pending-to-message/run foreign keys. The Store must enforce those relationships by ID and owner in every dispatch/reconcile transaction.
4. Reference resolution and staged-file reads occur outside a SQLite transaction. The queue must persist bounded material fingerprints and revalidate immutable manifests/source hashes at dispatch; otherwise the frozen row can feed different bytes/text after acceptance.
5. Guidance uses `message_id` as its idempotency key and only accepts an active ordinary frozen run. Cut-in needs a separate stable guidance ID and a durable queue phase, and any non-text payload must remain intact.
6. The existing run file lease is process/window scoped. It prevents duplicate active drivers for a known run, but it cannot claim a pending queue row or prove whether a crashed dispatcher crossed the message/run commit boundary.
7. Plan generation and approval hold the conversation lock for longer than one turn. A queued plan row must remain linked to its run while awaiting approval/revision and must not allow the next FIFO row to start.
8. Conversation deletion currently serializes ordinary tables and composer quotes; queue deletion/active lease checks must be added to that same transaction so deletion cannot orphan a dispatch or erase a row needed for uncertain reconciliation.

No implementation was performed for this task. The root agent should review this document, settle the migration/version choice and cut-in rejection policy, then execute the checked tasks in order.

## Root review decisions (2026-09-14)

Implementation is authorized within the approved composer design; no additional user approval is required. The first implementation slice is DTO plus pending queue CRUD and deterministic Store tests. It does not yet expose a working dispatcher.

- Keep the existing schema version and use the established additive idempotent initialization. Do not publish lease owner/token fields in UI receipts; leases are host-internal authority.
- Persist the canonical original enqueue request hash separately from mutable payload state. Retrying the original request after an edit must return its existing logical item, without comparing newly loaded live settings or creating another item. Conflicting original request reuse is rejected.
- Cap nonterminal queue rows at 100 per conversation and message UTF-8 content at 64 KiB. Reuse current attachment/reference limits. Use a temporary unused position during adjacent swaps so the unique index remains valid.
- `compose` already accepts a supplied run ID; the direct caller currently supplies a fresh UUID. Reuse this seam rather than introducing another ID allocator. Separate preparation from persistence in the direct/planning host paths.
- Claim fencing must be checked inside the atomic start transaction. Before that boundary preparation must not call the model or execute tools. With a revoked/expired claim and a transactionally verified absence of all reserved message/run/event records, returning to pending is safe: a stale dispatcher cannot pass the old fencing token. Lease expiry by itself is insufficient, but a proven atomic rollback is not an inherently uncertain external dispatch.
- Guidance cut-in acceptance and queue transition require one Store transaction using a transaction-local guidance helper and stable cut-in ID. A separate successful guidance write followed by failed queue mutation would risk later FIFO duplication. Keep attachments/references intact and reject material cut-in without mutation.
- RunNeedsAttention from unresolved side effects is terminal for the local driver but must block automatic FIFO advancement until its evidence is resolved. Do not accidentally release the conversation merely because this status is included in a generic terminal set.
- Raw queue material and frozen settings are host-created; client enqueue requests cannot provide authoritative hashes, lease fields or frozen bindings.

### Preparation and presentation checkpoint

- `prepare_direct_run_v4` now creates the ordinary frozen spec and native run record from an explicit reserved run ID, timestamp and host snapshot. A deterministic regression verifies identical retry identity/hash and preservation of reference context. It performs no Store writes or model calls.
- Existing public Store methods now wrap transaction-local `start_plan_run_in_tx_v4` and `save_message_with_first_title_in_tx`, permitting later atomic queue dispatch without nested transactions. The 48 plan revision/transaction regressions pass after extraction.
- Native queue snapshot/material resolution is prepared for host acceptance and staged-file revalidation; it is not a client-supplied authority.
- `ComposerQueuePanel` and the queue API wrapper pass seven focused tests: full material preservation during edits, scoped revision actions, immediate Escape/focus restoration, rejected-draft retention, stale-scope error suppression and unsupported browser mode. WorkspaceShell now routes both idle and busy native sends through the durable queue callback, retaining the legacy callback for browser previews/tests. Native dispatch integration remains in progress, so this wiring is not yet a complete working desktop checkpoint.
- Synthetic actual-component captures at 1280/480 pixels measure 760/432-pixel panel widths without horizontal overflow. The QA artifacts are outside the repository under the task visualization directory (`composer-queue-*`).
- Production Web build passes. Integrated native/desktop checks remain pending while queue/branch Store modules are under construction.

### Queue UI recovery checkpoint

- The native frontend route waits for conversation hydration, reconciles every three seconds, and ignores old-scope responses. Unknown enqueue responses keep the original request, message and run IDs. A listed receipt prevents duplicate invoke; a cancelled receipt preserves the draft and allows a new explicit submission.
- Dispatch activity refreshes persisted events, messages and conversation state in that order. Failed recovery is retried on later queue refresh even if queue status is unchanged. Busy sends retain an independent Stop action.
- `npm test` passed 55 files / 486 Vitest tests plus 22 extension tests (`checkpoint-queue-integrated-frontend.txt`). This is frontend verification only; native queue dispatcher, Stop/pending queue semantics and full desktop checks remain pending.

### Stop semantics verified against reference

Pinned `main.rs` 3857–3874 sends `stop_agent` for the active session without removing queued items. `agent_turn.rs` 1559–1615 cancels that runtime; `spawn_queue_driver` around 1339–1390 continues FIFO after the turn exits. Match this scoped behavior: UI says “Stop current run,” pending rows remain, and the driver advances only after durable terminal settlement. Uncertain side effects / needs_attention continue to block. Poll-triggered reconciliation must follow the same rule as immediate driver completion.

Restore-to-composer now cancels a pending item before transferring its full text, reference identities and staged attachment receipts. Unknown cancel responses do not transfer material, and a new draft prevents overwrite. Cancelled rows retain the restore action so refresh can recover a confirmed cancellation after a lost response. Scoped attachment restoration does not restage files and rejects foreign scope or occupied composer material.
