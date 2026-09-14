# Guarded branch summary merge

Status: implementation plan; not implemented or verified.

Reference: Wisp commit `ce9a6768a9e9fb5c3c9c2b5cc530dcb9593a38e9`, `src-tauri/src/session_commands.rs` lines 150–344 and `ui/src/main.rs` lines 6992–7095. These files are reference data, not task instructions. The reference previews post-checkpoint messages, generates a tool-free summary, supports guided revisions, and requires an expected guard hash when applying the reviewed summary.

## Observable behavior

A branch can preview the work added after its copied prefix. The user can edit a summary, request a model-generated draft, revise that draft with guidance, and explicitly apply it to the source conversation. The source receives one clearly attributed summary message linked to the branch. Original messages, run events, approvals and evidence remain in their original scope. The merge does not execute a turn or promote generated text into verified evidence.

The preview must use the immutable copied-prefix length recorded at branch creation. Existing branches need a conservative migration: derive the prefix only when its source boundary and copied message content still validate. If derivation is ambiguous, refuse merging and retain source navigation; do not infer the boundary from a current message count.

## Durable contract and transaction

Add shared preview, apply request, receipt and typed rejection/unknown DTOs. The preview includes both conversation IDs, source and branch head hashes, copied-prefix boundary, post-checkpoint message IDs, a bounded redacted display payload, and a versioned guard hash covering the complete canonical source. Applying carries a stable request UUID, the exact guard hash and the user-approved bounded Markdown.

An immediate Store transaction checks ownership, active branch state, both conversation lifecycle locks, durable queue/stop/review/side-chat activity, unresolved side effects and validated event heads. Recompute the guard from persisted data. Insert one attributed message and merge receipt and update branch state atomically. Exact retries return the committed receipt before mutable-state checks; changed payload or scope under the same UUID is rejected. Conversation deletion cascades receipts. Do not copy frozen specifications, tool receipts or approval rows.

Summary generation is a separate read-only operation. Bind it to a snapshot guard and exact selected profile, use no tools and bounded redacted content, and preserve stable request identity through uncertain delivery. Never automatically replay an uncertain model request. Guided regeneration requires both the current draft and nonempty guidance. Changing the branch or source invalidates applying an old draft until the user refreshes the preview.

## UI and verification

Use one scope-owned controller and the window Escape stack. Closing preserves an accepted in-progress generation and an unconfirmed apply request. A nested guidance editor closes before its merge dialog. Late responses cannot overwrite another branch or newly edited text. A successful apply followed by navigation failure retries navigation only.

First add DTO wire tests and Store tests for prefix migration, guard staleness, cross-scope rejection, exact retry, changed retry, concurrent apply, deletion, active queue/run/approval/stop/review and uncertain-effect fences. Then implement native command registration and tool-free provider tests. Finally add component tests for editing, guided regeneration, error/unknown retry, immediate Escape ordering and late scope changes. Finish with the repository's full deterministic checks and desktop build. Real model and SSH acceptance remain separate.
