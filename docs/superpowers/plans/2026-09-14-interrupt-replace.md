# Interrupt and replace (E03) implementation boundary

User approval covers this action. Implement after the queue/Stop/branch checkpoint is verified; simple independent workers use GPT-5.6 Luna Max.

Pinned reference `main.rs` 4132 onward awaits `stop_agent` before sending with `replace=true`; `agent_turn.rs` 798 onward removes the interrupted turn from subsequent model context while keeping the visual transcript. OmicsOps must additionally preserve durable stop uncertainty and its frozen authority boundaries.

## Required behavior

- A complete replacement payload includes text, references, attachments, mode, exact model profile and compute selection. Never clear or silently reduce material before durable acceptance.
- Bind an explicit target run ID and its scoped event head; do not implement as a frontend Stop followed by ordinary Send, which can target a newer run or lose intent between calls.
- Persist one replacement intent, stable replacement queue IDs and a target-run Stop intent transactionally. Same request and payload reconcile idempotently; conflicting payload, foreign scope and stale target reject without new Stop or queue side effects.
- Replacement waits for the exact old run's durable terminal state. `needs_attention`, unresolved dispatch, unknown cancellation, a pending approval or a foreign active driver blocks it. Stopping local waiting never proves remote cancellation.
- When safe settlement is confirmed, dispatch the replacement before previously pending ordinary turns, retaining their payloads and relative order. Existing pending turns are not silently cancelled.
- Store an immutable model-projection exclusion receipt for the interrupted turn, with source run/message IDs and verified sequence/hash boundaries. Do not delete messages, events, tool evidence, approvals or archives. Apply the exclusion only to later model input construction; transcript and evidence inspection remain complete.
- Approval/spec authority from the old run is never copied into the replacement. Replacement uses a new frozen RunSpec or new Plan revision under ordinary host checks.
- Recover accepted intent after window close, conversation switch and process restart. No automatic provider replay under a completed or uncertain request identity.

## Verification before exposing the action

Store tests cover atomic rollback, duplicate/concurrent requests, stale target, cross-scope rejection, pending FIFO ordering and terminal/uncertain settlement. Native tests verify exact-run Stop sequencing, projection exclusion without transcript deletion, no dispatch before safe settlement and preservation of material/profile authority. Frontend tests cover retained unknown request IDs, complete payload, active-run change, immediate Escape/top overlay, scope switching and clear-only-exact-accepted-draft. Run all repository default checks and desktop build; do not substitute mocks for real SSH/model acceptance.

## Implementation checkpoint

The Store now accepts the replacement queue row, exact-target Stop, priority rotation and immutable source boundary in one immediate transaction. Exact retries resolve before mutable model/material reads. A source run accepts one replacement identity. A cancelled replacement preserves its Stop receipt and can be restored as a draft; payload editing, cut-in and crossing its priority slot are refused. Ordinary pending messages retain their relative order and full material.

`composer_queue_replace` resolves the complete ordinary queue snapshot and signals only the committed target. UI response uncertainty retains all four reserved IDs and the original event head; the explicit reconciliation action cannot retarget a newer run. Source events and transcript remain present. Both claim and dispatch require a valid target chain, matching recorded source boundary, durable terminal event and no unresolved side effects. The native queue driver also holds the predecessor run's OS lock through dispatch.

### Context adaptation to OmicsOps

Wisp truncates the interrupted turn from a shared session message projection. OmicsOps V4 builds each turn under a new run ID: `RepositoryEventStoreV4::load` and the execution driver read `agent_events_v4(spec.run_id)`, and queue dispatch freezes a fresh RunSpec. Thus the replacement excludes the entire old run from its implicit model input by using the existing run boundary; it does not need to delete or rewrite a shared message list. The durable receipt records the source event sequence/hash and a source user-message ID when the original run came through the queue. Legacy runs without a trustworthy message association store `null`, never guess an adjacent transcript message. Explicitly selected reference material still follows ordinary host validation and untrusted-reference framing. No old approval or frozen spec is imported.

Deterministic checks cover duplicate/concurrent admission, complete reference/attachment/config preservation, stale and foreign targets, rollback after a Stop identity collision, priority/edit guards, restart recovery after model changes, terminal-row-only refusal, unresolved effects, unknown response retry, scope changes and immediate window Escape. Live model/SSH acceptance remains unexecuted; local Stop does not assert remote-job cancellation.

### Verified checkpoint before independent review

- `cargo test --workspace`: passed, including 273 desktop tests (5 explicit ignored acceptance tests), all Store tests and doc tests. Log: `checkpoint-replacement-rust-final.txt`.
- `npm test`: passed, 65 Vitest files / 548 tests plus 22 extension tests. Log: `checkpoint-replacement-frontend.txt`.
- `npm run build`: passed after the final menu layout change. Log: `checkpoint-replacement-web-final.txt`.
- `npm run build:desktop`: passed after the final native ownership fence and menu layout change. Log: `checkpoint-replacement-desktop-final.txt`. The NSIS artifact is local validation output only; it was not distributed.
- `cargo fmt --all -- --check` and `git diff --check`: passed. The workspace contains pre-existing uncommitted work; no commit or release was created.
- Actual React/CSS fixture screenshots at 1280×800 and 480×800 show 300px / 217px send menus without overflow or ancestor clipping. These use synthetic data, not real model output.

Logs and screenshots are in `C:/Users/jindong/.codex/visualizations/2026/09/13/01a099ab-6dff-75b2-9b54-0fcd3d7536b9/`. Real model/SSH/runtime acceptance remains unexecuted. This checkpoint does not complete the full 64-item composer parity inventory.

### Independent review corrections

The review identified a wakeup gap when the predecessor OS lock was busy. The driver now observes the committed Stop in a retained background wait and retries a busy predecessor lock after releasing the unstarted queue claim. It does not require a visible window or another user request to continue; it still exits without dispatch at uncertainty/approval fences. A deterministic native test holds the Stop in requested state, verifies that the wait remains active, then changes it to observed and verifies wakeup without a UI kick.

An observed event head is now validated as an ancestor of the same still-active run, rather than requiring it to remain the final event while material resolves. Normal appended progress is accepted; changed hashes, foreign scope, a different active target or any terminal event are rejected. The immutable receipt retains the exact user-observed sequence/hash. A regression test appends a read-only tool dispatch after the observed head and confirms one accepted replacement with the original boundary.

Accepted replacement recovery was already durable in the Store. The queue now also returns the complete replacement receipt, exposing source head, source message (when known) and canonical Stop ID after restart. The queue panel offers inline receipt details. Unconfirmed requests that never reached durable acceptance are retained only in the current UI process; this is distinct from recovering an accepted queue/Stop transaction and does not authorize automatic replay after restart.

### Final verification after independent review

The GPT-5.6 Luna Max reviewer confirmed that the retained wakeup and observed-ancestor corrections address the reported defects. Accepted intent recovery is durable; the remaining in-memory limitation applies only to an unconfirmed client request that never reached durable acceptance.

- `cargo test --workspace`: passed, including 274 desktop tests (5 explicit ignored acceptance tests), all Store tests and doc tests. Log: `checkpoint-replacement-review-rust.txt`.
- `npm test`: passed, 65 Vitest files / 549 tests plus 22 extension tests. Log: `checkpoint-replacement-review-frontend.txt`.
- `npm run build`: passed. Log: `checkpoint-replacement-review-web.txt`.
- `npm run build:desktop`: passed, including the Windows NSIS bundle. Log: `checkpoint-replacement-review-desktop.txt`. The bundle is local verification output, not a release or distributed installer.
- `cargo fmt --all -- --check` and `git diff --check`: passed after the implementation corrections.

These results supersede the pre-review checkpoint above. Real model, SSH and scientific runtime acceptance remain unexecuted. No commit, push, tag or release was created. Guarded branch summary merge and the other outstanding entries in the 64-item parity inventory remain unfinished.
