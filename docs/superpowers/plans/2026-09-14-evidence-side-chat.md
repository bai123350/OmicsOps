# Evidence-backed side chat (E05 / F03)

The user has approved the full composer design and asked for every clickable behavior. This implementation needs no further design permission. Continue after the current queue/branch/usage integration reaches a verified checkpoint. Use the requested GPT-5.6 Luna with Max effort for independent workers.

## Reference behavior

Pinned Wisp `main.rs` around 4234 opens the SideChat tab, submits a separate question, maintains separate busy/history state, and routes a late result back to the originating session. `native-lib.rs` around 6317 snapshots primary-session events, retrieves evidence, answers without tools, and returns source excerpts plus a snapshot watermark. `side_chat.rs` explicitly uses immutable visual history rather than compacted model context; it selects bounded source excerpts, supports session/comparison/lookup intent, and returns no-evidence instead of inventing an answer. The additional source file is cached under the same pinned SHA as the parity inventory.

## OmicsOps implementation boundary

- Side questions may run while the primary Agent is busy. They never create or mutate a primary RunSpec, approval, plan, guidance record, execution context, or main message.
- Use project/conversation-scoped durable side-chat turns with a stable request ID and canonical request hash. Keep history, answer status, source snapshot hash/watermarks, frozen model identity and safe usage metadata independent of primary runs. Credentials stay in the existing keyring path.
- Empty main draft opens the side panel. A nonempty draft submits its complete supported payload and clears only the exact accepted original text/material. If a material type is unsupported, keep it intact and make that action unavailable; never send text alone and drop files or quotes.
- A model selector belongs to the side panel and must not change the primary model. Freeze its exact profile/configuration and request bounds before dispatch. No tool definitions, shell, delegated execution, MCP or mutable project capabilities are available to this call.
- Read a transactionally coherent source snapshot of persisted primary messages and completed V4 tool evidence. Do not use streaming partial text, a generated plan, or a summary as executed evidence. Source IDs must point to immutable records and remain usable after main context compaction.
- Reuse existing source sanitization and bounded reference/attachment resolution where their contracts apply. A side-chat snapshot should not require renaming or weakening reviewer-specific authority checks.
- Select bounded evidence deterministically (or use an explicitly bounded read-only classifier). Preserve source IDs, roles, excerpts and snapshot provenance. No matching evidence returns a typed no-evidence result without a fabricated scientific answer.
- Validate model citations against the selected source IDs. Return a safe bounded answer and allow the user to inspect source excerpts / navigate to the source message. The source prompt treats quoted history as untrusted data.

## Lifecycle and recovery

1. Shared DTOs plus TypeScript mirror: send/list/get requests; queued/running/completed/no_evidence/failed/interrupted result; source references and safe failure codes. No client-provided frozen authority.
2. Store transaction binds request hash, frozen profile and evidence snapshot to one logical side question. At most one running side question per conversation; duplicate exact requests return its original receipt and never start a second provider call. Conflicting reuse and foreign scope reject.
3. Native driver holds a per-request OS lease before starting provider work. Its acceptance receipt remains accepted after emit/UI delivery failure. A fresh process can identify a lost driver and mark interruption without pretending usage is zero or automatically replaying the request.
4. Explicit retry reconciles a request whose outcome is unknown. A terminal failed/interrupted question requires an explicit fresh attempt with traceable parent request identity; it never silently charges another provider call under the old completed request.
5. Frontend retains a pending request per source scope through closing, switching and reopening. List/get recovery restores history and busy state. Stale callbacks do not mutate or navigate the selected conversation.

## Verification

- Store tests: fresh/reopen, duplicate/concurrent sends, payload conflicts, scope ownership, one running turn, terminal transitions, interrupted recovery, bounded source and output data, source watermark/hash integrity, deletion rules.
- Native pure tests: deterministic evidence ranking, no-evidence behavior, redaction, invalid citations, frozen profile change, absence of tools, token/request bounds, request lease and simulated provider failure. No live model or SSH dependency.
- Frontend tests: empty open vs populated submit, independent busy/model selection, full payload preservation, unknown retry identity, late responses, source expansion/navigation, immediate window Escape/top layer and focus, narrow layout.
- Final checks: `cargo test --workspace`, `npm test`, `npm run build`, `npm run build:desktop`. Ignored real-model/SSH tests remain explicitly unexecuted.

This file is an implementation plan. No side-chat host or UI completion is claimed by its presence.

## Frontend checkpoint (2026-09-14)

The native side-chat contract is being implemented. Frontend API/hook/panel and Workspace/Send wiring are present: separate model/draft/history per project and conversation, complete main-composer payload, stable unknown-result retry, source excerpts and message navigation, older-history cursor, and window Escape through the existing sidebar layer. The frontend also reconciles cached active records outside the newest page instead of leaving a stale busy fence forever.

Focused side-chat tests passed 15 cases. Full frontend checkpoint passed 63 files / 529 Vitest tests plus 22 extension tests (`checkpoint-side-chat-frontend.txt`); subsequent DTO mirror and typing corrections passed the Web build. Native side-chat persistence/provider dispatch is not yet complete and must be checked before claiming E05/F03 done.

Synthetic actual-component layout QA at 1280 and 480 pixels produced `side-chat-desktop.png` and `side-chat-narrow.png` in the task artifact directory. The panel measured 379px with scrollWidth equal to clientWidth at both sizes. All example answer/source text is synthetic layout data, not a scientific result or real model acceptance.

Review follow-up: acceptance clears references only if the original selection instance is unchanged. Removing and re-adding the same reference while the request is pending is now covered by a regression test. Side-chat and branch integration passed 11 focused cases, including late receipts after conversation changes. Layout captures were refreshed after the model_label DTO change; both viewport measurements still show no horizontal overflow. Native command completion and pagination race fixes are still being integrated.

## Integrated native checkpoint

Native send/list/get commands and startup recovery are registered. A scoped request OS lease spans acceptance, single provider dispatch and result persistence; exact retries read the original record, and an absent owner is marked interrupted without automatic replay. No-source requests complete as no_evidence without calling the model. Provider requests contain no tools; selected references and validated attachments become bounded citable material snapshots, and supported images use the existing exact-model request budget. Message source content hashes detect edits between snapshot construction and Store acceptance. Answers reject invented/duplicate citations and persist only bounded redacted text and typed errors.

The selected client configuration is held for the accepted in-process call; restart never reconstructs or replays that call. Public history currently retains profile ID and a frozen display label, but not the full model configuration hash. Side-chat provider usage remains unavailable in the public optional usage field; no zero-cost or exact-usage claim is made. These audit improvements remain explicit follow-up work.

Verification: cargo test --workspace passed (desktop 272 passed / 5 ignored), native side-chat tests passed 7, and npm test passed 540 Vitest plus 22 extension tests after the RFC3339 millisecond-order regression fix. Web build passed. Desktop NSIS build passed before that final frontend-only timestamp comparison correction; no installer was distributed. Logs are checkpoint-side-chat-final-rust.txt, checkpoint-side-chat-time-frontend.txt, checkpoint-side-chat-time-build.txt and checkpoint-side-chat-final-desktop-2.txt in the task artifact directory. The first desktop attempt exposed a TypeScript redundant condition; it was corrected before the successful build. No real model, SSH or WSL acceptance was executed.

The /btw command preserves its full original draft until acceptance, /fork requires actual branch-and-send support, and /trajectory uses only current project/conversation events with read-only controls and immediate window Escape. The complete 64-item scope remains unfinished.
