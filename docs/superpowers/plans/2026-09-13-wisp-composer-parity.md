# Wisp composer parity implementation plan

> **For agentic workers:** Use superpowers:subagent-driven-development or superpowers:executing-plans to implement the tasks. Read the accepted spec and this plan together. Do not mark later phases complete when only their menu entry exists.

**Goal:** Implement the accepted 64-item composer inventory with native OmicsOps execution, authorization and persistence semantics.

**Architecture:** Independent presentation/export changes are delivered before the dependent context and runtime changes. Shared actions back menus and slash commands; typed requests are validated by the host. Existing frozen V4 runs remain immutable.

**Tech Stack:** React 19, TypeScript, Vitest, Rust, Tauri 2, SQLite/sqlx.

**Spec:** `docs/superpowers/specs/2026-09-13-wisp-composer-parity-inventory.md` (approved by the user on 2026-09-13).

## Global constraints

- Windows first; deterministic tests do not require a real model, SSH, ACP or WSL.
- Credentials remain in keyring; export and reference material is sanitized.
- All overlays join the window Escape stack; immediate Escape closes only the top layer.
- Model capabilities use exact compiled catalog matching. Fast is not plan mode.
- Accepted scope includes all phases below; incomplete phases stay explicitly unchecked.

## Phase 1: independent composer actions

### Task 1: share preview and native export (B04–B07)

Files: create `src/features/workspace/ShareConversationDialog.tsx`, `conversationShare.ts`, matching tests and CSS; create `src-tauri/src/conversation_export.rs`; register in `src-tauri/src/lib.rs`. Put request DTO in `crates/omicsops-dto/src/`; API wrapper in a dedicated `src/conversation-export-api.ts` to avoid unrelated edits.

Interfaces: dialog accepts `{ messages: { id: string; role: string; markdown: string }[], locale: "zh-CN" | "en-US", onClose: () => void }`. Exports a default-named `ShareConversationDialog` function. Native command owns the save dialog and returns `string | null` (cancellation). Browser preview downloads a real Blob rather than returning a fake saved path.

- [x] Write tests: selection excludes unselected rows; redact literal keywords including regex characters; HTML escapes scripts and disallows active remote content; width rejects/clamps invalid values; native save cancellation leaves preview open; repeated export is locked; immediate window Escape closes dialog.
- [x] Run the targeted tests and capture expected missing-behavior failures.
- [x] Implement message selection, select all/none, live redaction, PNG/HTML format, width input and export lifecycle. PNG must render Markdown/table content readably with bounded dimensions. No false success on cancel/error; preserve draft.
- [x] Validate Rust payload limits, extensions and format signatures; use existing redaction helpers where relevant; test with temporary files and no real dialog.
- [x] Run targeted tests and inspect a generated export artifact.

### Task 2: model reasoning effort flyout (D03)

Files: `src/features/workspace/ApiModelPicker.tsx`, `api-model-picker.css`, corresponding tests, `src/DesktopApp.tsx` callback.

Interface: optional `onReasoningEffortChange(profile: ModelProfile, effort: ModelProfile["reasoning_effort"]): Promise<void>`. Resolve options only from `profile.catalog_limits.reasoning_efforts` (inspect actual type before writing); preserve a stored value for display. Save through existing `saveModelProfile` using `reasoning_effort: null` for default.

- [x] Add failing tests for supported levels, unknown capabilities, parent/child Escape, saving exclusion and failure retaining old value.
- [x] Implement flyout with its own Escape layer and accessible current value; prevent stale profile results from replacing active state.
- [x] Connect DesktopApp callback to existing save and state update with modelSelectionInFlight guard.
- [x] Run picker and DesktopApp regressions.

### Task 3: composer presentation, skill draft and keyboard behavior (A01–A03, B08)

Files: `WorkspaceShell.tsx`, `composer.css`, `ComposeActions.tsx`, composer/menu tests.

- [x] Add tests for save-skill click appending a generation request without dispatch, focusing input, Enter send, Shift+Enter newline and IME composition exclusion.
- [x] Implement scoped draft actions and reference dialog integration; keep existing public callbacks compatible.
- [x] Make send and chevron one teal segmented button, show Send text, keep stopping state legible; replace orbit icon with sliders.
- [x] Preserve existing mode controls until Fast is genuinely implemented; do not mislabel plan toggle as Fast. Do not advertise unavailable shortcuts in placeholder.
- [x] Run composer and full frontend regression tests.

## Remaining phase roadmap and dependency gates

Each phase gets its detailed task plan after inspecting the concrete host APIs it consumes; this roadmap is not permission to substitute placeholders for working behavior.

| Phase | Spec coverage | Required deliverable and tests |
| --- | --- | --- |
| 2 | A04–A11, B01–B02, B09, F10–F14 | Stable-ID reference DTO/resolver, scoped catalogs, attachments, commands/skills/workflows picker, search palette. Test missing/deleted IDs, cross-project scope, enabled skills, deduplication, transport retry and IME. |
| 3 | B03, C01–C03, C06–C10, D01–D04, D07, F05–F09 | Durable preferences and host-frozen configuration for optional reviewer/memory/delegation, reviewer/specialist selection, exact Fast/effort requests. Test migration twice, old defaults, host rejection and frozen-run invariance. |
| 4 | E01–E06, F02–F03 | Persistent send queue, reuse guidance, interrupt/replace state machine, side-chat and branch lineage. Test unique dispatch, uncertain cancellation, isolated histories, restart recovery and source evidence. |
| 5 | C04–C05, D08–D10, F01, F04, F15 | Background completion receipts/continuation, actual context accounting/compaction, undo preview and event-backed trajectory. Test duplicate completion, invalid summary, snapshot conflicts and irreversible remote effects. |
| 6 | C11–C13, D05–D06 | ACP binding and session configuration, WSL and multi-resource runtime adapters. Test fake stdio negotiation, unavailable environments and resource authorization; record real acceptance separately. |

## Integration verification

- [x] Review independent diffs before integration, fix actionable issues.
- [x] `cargo test --workspace`.
- [x] `npm test`.
- [x] `npm run build`.
- [x] `npm run build:desktop` for host command/desktop composition changes.
- [x] Update spec status item-by-item with actual checks and outstanding work.

## Execution ledger

- User approved the complete design; no additional feature-scope confirmation required.
- Work starts from `codex/collapsible-workspace-sidebar`; working tree contains only the accepted spec. Work in current checkout to keep changes visible in the user's configured project; no merge, push or release.
- Phase 1 tasks 1–3: implemented and reviewed. Phase 2 typed references and local slash commands are in integration. Phases 3–6: not started.
- Task 1 review fixed SVG/iframe typography mismatch; a real 800×2697 PNG with long bilingual paragraphs and tables retained its final line. Math remains literal LaTeX, so B06 is partial rather than falsely complete.
- Task 2 review fixed submenu clipping and serialized Settings saves with composer saves; actual field is `catalog_capabilities.reasoning_efforts`.
- Task 3 review fixed accepted sends erasing text appended while pending. Regression reproduced failure then passed after comparison with the raw submitted draft. Auto-grow/manual-height and modifier-send tests added.
- Full Rust workspace tests passed (real model/SSH ignored cases remain unexecuted). Final frontend sweep passed 225 Vitest + 22 extension tests.
- Production Web build passed with a >500 kB bundle warning. Final desktop NSIS build including auto-height changes passed.
- Browser UI automation failed to connect (Page.navigate timeout, then nodeRepl.fetch failure). This does not establish screenshot parity; actual export PNG visual QA is separate and passed.

- Final frontend sweep: 225 Vitest tests + 22 extension tests passed. Final integration review: no additional actionable findings; actual native save/cancel and WebView2 visual smoke remain unexecuted.

## Phase 2 implementation slice: typed references

Approved scope continues. Implement the first complete reference transport before extending commands/search/attachments.

- [x] Add data-only tagged reference/catalog DTOs and host catalog of project artifacts, nonempty sessions and effective enabled skills.
- [x] Resolve stable IDs on the host; reject cross-project, deleted, disabled and self-session references. Bound count/content, deduplicate, treat imported text as untrusted context.
- [x] Add tested caret-aware @/#/skill trigger picker, keyboard selection, immediate window Escape and removable chips.
- [x] Connect selected IDs to Agent and Plan start requests, preserving failed-send drafts and references. Persist bounded context for planning recovery and freeze direct context with existing plan hash.
- [x] Add native contract and integration regression tests, run all required checks (before subsequent search/preflight review fixes; final sweep recorded below).

This slice does not complete project/workflow/runtime references, full command/search palette or attachments. Those remain tracked by the Phase 2 inventory, alongside Phases 3–6.
Subagents for bounded catalog and picker work use user-requested gpt-5.6-luna with max reasoning.

### Phase 2 local slash commands

- Extend the reference picker with separate Commands and Skills groups and a single keyboard selection order. Local commands remain available when remote skill loading fails.
- Wire `/plan`, `/permission`, `/files`, `/save-as-skill`, `/skills`, `/upload`, and `/share` to the same existing actions. Only expose actions whose callbacks are available; upload retains the existing project upload constraints.
- Treat exact commands submitted with Send as local actions. Preserve other draft text when selecting a command at the caret and never dispatch the command literal to the model.
- Test grouped results, keyboard and IME behavior, unavailable callbacks, and command dispatch. Remaining slash commands require their respective host lifecycle implementations.

Progress: host reference resolver/transport tests passed (16 matching tests). Agent and Plan frontend transport tests passed after waiting for actual hydration and compute readiness. Final suite/build verification is pending integration of slash commands and MathML export.

### Next Phase 2 slice: shared workspace search

Use the same search dialog from project library and workspace, opened by a visible search button or Ctrl/Cmd+K. Compose its index from existing projects plus the host-owned per-project reference catalog; do not invent a second reference resolver. Group project/session/artifact/skill results and locally supported actions. Separate Open from Attach: Open navigates to the source project/session/artifact; Attach keeps the current conversation and is offered only for reference types/scopes the current resolver actually supports. Preserve focus on close, ignore stale async results, and report partial catalog failures with retry. Project/workflow/runtime attachment remains its own subsequent implementation.

Tests: shortcut and immediate Escape from both entry points; grouping/filtering; default keyboard open versus explicit attach; failed/stale catalogs; attach preserves draft/current conversation; opening a cross-project session selects the requested saved ID rather than the first conversation.

### Integrated search and review fixes

- Shared search is wired to both app entry points, real project catalogs, supported settings/files actions, same-project attachment, and explicit cross-project session selection. Artifact opening shows source metadata and its verification status; it does not read file contents.
- Reference preflight runs before message persistence in both Agent and Plan modes. A rejected preflight leaves the draft and chips intact without adding a user message. Native start still resolves references independently; preflight does not freeze or authorize a later run.
- Empty reference results no longer swallow Enter or arrow keys. Search handles IME, async cancellation, explicit Open/Attach, retry and focus restoration. The project creation modal now also uses the window Escape stack.
- Latest frontend sweep (including the consumed-search-request remount regression): `npm test` passed 284 Vitest + 22 extension tests; `npm run build` passed with the existing large-bundle warning (1,097 kB main chunk).
- Native review added the raw 12-reference bound before deduplication, UTF-8-safe title-inclusive session bounds, and sanitization of known password/token/authorization assignments, quoted JSON keys, and PEM private-key blocks. Focused resolver tests passed 14 cases; `cargo test --workspace` passed after these fixes. This is pattern-based sanitization, not a guarantee of recognizing arbitrary unlabelled secrets.
- `cargo fmt --all -- --check` and `git diff --check` passed. Final `npm run build:desktop` passed (Windows x64 NSIS local build only; no publication). Search browser QA passed at 1280×800 (760×720 dialog) and 480×800 (456×776 dialog), with no internal horizontal overflow; autofocus, ArrowDown and Escape worked. The existing page-wide `body min-width:1080px` causes outer scrollbars at 480px, so this does not establish whole-app narrow-window parity. Browser focus restoration was not independently established by the harness; the component regression test covers it. Real model/SSH/ACP/WSL acceptance remains unexecuted or ignored, separate from deterministic checks.
- Math export browser QA passed: actual 800×1083 PNG, four MathML nodes, bilingual text/fractions/matrix/final marker visible; no external resources in generated HTML. Native WebView2/save-dialog manual smoke remains unexecuted.

## Next Phase 2 slice: explicit local attachments

The existing Upload action is an SSH project-file transfer, so it cannot implement the composer's local file/image attachment contract. Add explicit, bounded local attachment staging without implicitly syncing scientific data to a remote host.

1. Add data-only attachment receipts and staging requests to `omicsops-dto`. The host chooses files through its native picker or stages explicit browser drop/paste bytes. Store immutable bytes plus a hash/size manifest under `.omicsops/attachments/<id>/`; use stable IDs at submission, never trust a client-supplied absolute read path. Bound individual files and total message attachments. Large data continues to use references rather than automatic copies.
2. Resolve attachment IDs against the current project and conversation, canonicalize inside the project root, reject symlinks/changed hashes/missing bytes, and produce bounded untrusted metadata/text context. Images use the existing verified model-image request path and exact profile vision capability. Persist attachment/image snapshots with the run so recovery and frozen plans use the same content. Clipboard/workspace paths remain references, distinct from image uploads.
3. Add shared attachment state for native selection, file drop and image paste: uploading/ready/error cards, retry/remove, generation guards on session switch, and preservation on failed sends. The Add files menu and `/upload` use this composer staging flow; existing remote transfer remains in the file panel.
4. Test temporary-directory staging, limits/hash/path/symlink checks, old DTO defaults, model image transport/budget behavior, no-vision handling, no duplicate uploads, paste/drop/IME, async removal/session changes, failed-send preservation and top-layer Escape. Run the required full checks after integration; real native picker/clipboard/WebView smoke is separate.

This extends the approved design without changing frozen host approval or remote job semantics. Files staged locally are not automatically available in an SSH execution directory.

Windows drag/drop uses HTML5 `File` events with `dragDropEnabled: false`, as required by [Tauri's window configuration reference](https://v2.tauri.app/reference/config/#windowconfig). This avoids a UI API accepting arbitrary absolute filesystem read paths. Native picker selection remains host-owned.

### Local attachment integration verification

Implemented host staging/receipts, project and conversation ownership, content/hash/type/path validation, picker/drop/paste UI, stable submission IDs, bounded text material and frozen image references. Fixed StrictMode lifecycle, concurrent drops, retry limit bypass, picker readiness and initial missing-conversation input race. The targeted regressions failed before their fixes and passed afterward.

Image budgets retain the model-independent unknown-image rejection. A credential-free exact official endpoint/model capability check runs before message persistence; the adapter applies explicit high detail and a conservative documented maximum per image, measuring the remaining provider JSON separately. Rules derive from [OpenAI Images and vision](https://developers.openai.com/api/docs/guides/images-vision); no family/unknown gateway inference or change to compiled catalog context/output limits. No keyring access or external model call is needed for capability preflight.

Project/execution-context/runtime references now resolve real project bindings without interpreter/SSH probes or permission changes. Catalog/picker/search tests cover typed keys and supported attach behavior. Workflow and file/quote references remain subsequent work.

Verification: `cargo test --workspace` passed; `npm test` passed 321 Vitest + 22 extension tests after the initialization race fix; `npm run build` passed (1,108 kB main chunk warning). `cargo fmt --all` normalized formatting; `git diff --check` passed. No commit was made, including no standalone formatting commit. Final desktop build and independent review are pending at this entry.

## Next Phase 2 slice: durable workflow references

The reference implementation stores real workflow templates and attaches their stable IDs; OmicsOps has no equivalent source. Add project-owned, persisted sequential task templates with names, descriptions, enabled state and ordered instruction steps, stored through existing `Store::put_json/get_json/list_json`. Do not add invented front-end workflow rows or claim an independent DAG scheduler. The current Agent/Plan consumes the resolved task recipe under its existing frozen approvals; selection alone does not execute anything.

- DTO: workflow template and save request with bounded ordered steps, UUID/project ownership, and a workflow reference variant. Reject empty/oversized fields and credential-bearing recipe text before persistence; resolved content remains untrusted task context.
- Host: catalog, save and disable management; records scoped to a project, disabled/missing references reject. Existing resolver budget and preflight/frozen snapshot apply. No new migration or compute permissions.
- UI: project workflow library with create/edit/enable/disable, error/retry and draft preservation; immediate window Escape closes the editor before its library. `/` groups Commands, Workflows, Skills; selecting a workflow produces the existing removable stable chip. A management action opens the real library.
- Tests: project isolation, invalid definitions, restart persistence, disabling stale selected IDs, ordered bounded rendering, distinct picker grouping, save failure/editor state, immediate nested Escape and Agent/Plan reference transport.

This implements the composer workflow-reference contract using OmicsOps execution semantics. It does not implement Wisp's separate dynamic DAG authoring/scheduler as an unreviewed backend replacement.

Reference refinement: fetched `quick_actions.rs` at the approved `ce9a6768...` commit (the separate older local clone is `3628a420...`). Its `render_workflow_reference` preserves a DAG and requests one delegation dispatch. The sequential recipe foundation above therefore does **not** complete workflow execution parity: dependency-preserving structured workflow dispatch and built-in mappings remain required alongside the delegation lifecycle phases. Do not mark A08 fully complete solely because this catalog/editor works.

## Next Phase 2 slice: workspace file paths

Add a separate typed `workspace_file` reference whose composite identity is project + exact backend binding + project-relative path. Local and SSH files remain source references; attachment staging and image transport never receive these references. The host checks safe relative paths and ownership, then validates canonical regular files at submission without reading content; remote checks reuse trusted SSH canonical metadata operations, not download or image preview APIs.

Provide a bounded local metadata listing beside the existing remote file tree, with source switching, refresh and explicit Attach / internal drag actions for file leaves. The composer accepts a custom typed drag payload. Clipboard text consisting solely of filesystem paths is converted by a local host validator to project-relative references; URLs, commands and ordinary text keep normal paste behavior. Reject outside-project, missing, internal and symlink paths rather than attaching invented entries. Remote paths cannot be inferred from clipboard text.

Tests cover safe paths and deleted sources, exact SSH binding checks, no-download attachment behavior, local/remote source retention, internal drag priority over OS file uploads, mixed/ordinary paste, asynchronous project changes and failed preflight draft retention. Source-aware quote text remains the next distinct step; file attachment does not imply a file read or permission to edit.

### File/workflow checkpoint and source-aware quotes

The full frontend sweep passed 350 Vitest and 22 extension tests. Rust workspace tests passed before the subsequent quote changes; final checks must be rerun after integration. Approved plans now freeze the main and delegated model configurations while preserving legacy snapshot validation. Workspace references distinguish local and SSH sources in visible chips. Workflow focus handling and bounded native directory scans are under final review.

For explicit text quoting, add a separate read-only file preview action. The host reads at most 64 KiB of UTF-8 text from a validated local or trusted SSH project file, sanitizes displayed text, and returns its source/hash. Quoting selected text rechecks the source hash and verifies the exact selection against the sanitized preview, then stores a bounded, conversation-owned quote snapshot. Composer submission carries only its stable quote ID; the resolver adds source/path/hash and untrusted text. Merely attaching a file path never invokes this read. Binary and oversized files retain the metadata-reference path. No auto-edit instruction is inserted by quoting.

Text quote integration: focused native composer suite passed 63 tests; the subsequent full frontend suite passed 366 Vitest + 22 extension tests, and production Web build passed (1,138 kB main chunk warning). Preview source/hash/selection checks, stable retry IDs, stale UI results and draft-preserving dispatch are covered. These precede the next conversation-preference changes and do not replace final workspace/desktop checks.

Actual headless Chrome fixture screenshots at 1280×800 and emulated 480×800 were inspected for workflow editor and quote preview. Both narrow dialogs measure 460 px inside the viewport, with no internal horizontal overflow; editor content scrolls and its save controls remain available. Fixtures use synthetic data and actual React/CSS components. This is visual component QA, not real SSH, Tauri WebView2 or model acceptance. The CUA connector was unavailable; the local headless harness succeeded. Saved images and measured dimensions are under the task visualization directory.

## Phase 3 slice: frozen optional conversation preferences

Reuse the existing durable Plan/Agent mode; keep the global iteration settings separate. Add conversation-owned delegation, optional auto-review, and memory-retrieval preferences with legacy all-enabled defaults. Store them in the existing settings table, delete them with the conversation, and validate project ownership and runtime locks on updates. The UI loads/saves real preferences, preserves values on failure and blocks conflicting sends while saving.

Snapshot preferences before initial planning/direct composition. Planning recovery and approval use that saved snapshot; executable RunSpec hashes cover the optional preference field, while its absence preserves legacy serialized hashes. Execution and resume use the frozen spec exclusively. Disable both advertisement and dispatch of new delegation/memory-retrieval tools when selected; note/evidence saving remains independent. Disabling optional auto-review skips the model reviewer only after mandatory deterministic verification and evidence requirements succeed. It does not claim that failed science is verified.

Fast service tier, reviewer-profile selection, background completion, failure analysis, specialist and additional runtime features remain subsequent complete slices. No decorative persistence for unimplemented behavior is added here.


### Preference integration and review follow-up

The frontend preference slice passed 109 focused tests and TypeScript project build. Protocol preferences and service-tier hash tests passed 18 cases; AgentCore optional capability/review behavior previously passed all 102 tests. Native plan revision tests passed 13 cases. Aggregate composer material now shares a strict 64 KiB budget across references and attachment excerpts, reserving room for attachment metadata and marking truncation; its UTF-8 regression passed. Full checks remain pending the concurrent review fixes.

Review fixes in progress: transactional quote cap/dedup/ownership insertion and deletion cleanup; ordinary-run preference locks; preserving an already-persisted message on same-draft retry after native start failure; treating post-start event read failures separately from rejected starts. Windows trailing-dot/space path aliases are rejected. Real model/SSH acceptance remains unexecuted.

## D01 Fast service tier implementation

Ruling: represent the currently exposed Fast toggle as `Option<bool>` rather than introduce a generic service-tier enum across core/protocol boundaries. None inherits, false requests standard, true requests priority; a run-owned wrapper distinguishes a frozen provider-default omission from a legacy run with no snapshot. This covers the approved toggle without claiming support for other service tiers; additional tiers would require an explicit extension.

Model profiles keep an optional default and conversation preferences keep an optional override. Resolve the main model's effective tier before planning/direct start and freeze it in the run record and spec/hash. Planning records also bind their main profile configuration so recovery/approval cannot silently pick up edited defaults. Child models retain their own configured defaults. Off on an unsupported endpoint omits the field; explicit Fast on is rejected. The UI must allow clearing an old enabled override after a model switch.

Use exact official OpenAI HTTPS endpoint and verified full model IDs only: gpt-6-astra, gpt-5.6-sol, gpt-5.6-terra, gpt-5.6-luna. No unknown gateway or model-family inference. `priority` is the accepted wire spelling, applied consistently to normal, probe and fallback requests; service acceptance/speed remain provider-dependent. References: https://developers.openai.com/api/docs/guides/fast-mode and https://developers.openai.com/api/docs/pricing?latest-pricing=fast . No paid calls are used in deterministic tests.


### Next reviewer slice: confirmed reference behavior

Read the native `lib.rs` at the same pinned ce9a6768 commit (downloaded as native-lib.rs in the reference cache). `review_session` checks the target session only, rejects an empty/busy session, prevents duplicate reviews, generates a separate report, persists it and emits review start/failure/result events. `generate_review_with_backend` fixes the reviewer rubric on the host, sends a read-only transcript and no tools, and selects HTTP or configured ACP. The UI reviewer selector edits the internal reviewer configuration and offers default HTTP, follow session, explicit HTTP and ACP. Therefore the current draft-prefill Review action remains incomplete; it must be replaced with an independent report operation, not reused as an ordinary execution request. Existing OmicsOps deterministic science verification remains separate from such a retrospective report.


D01 native checkpoint: exact capability/profile tests, tri-state DTO test, adapter normal/probe/fallback tier tests and 11 model-command tests passed. Full core, DTO and adapter crate suites also passed; live SSH/WSL tests remained ignored. Independent native/protocol review found no actionable defects in profile-hash or service-tier freezing. Root service-tier resolution regression passed, and the 18 protocol tests passed with legacy omission, all three effective tier hashes and unbound-snapshot rejection.

Store review fixes complete: quote ownership/dedup/cap/insert are atomic with deletion; all Store tests passed, including five quote atomic/cleanup tests and nine preference tests. Native quote tests passed six cases. New-run transactions compare the provided optional preference snapshot with the current setting before locking/inserting; resumes keep frozen values and legacy absent snapshots remain compatible. Root inspected the transaction and quote call sites; the scope/key checks, returned duplicate validation and optional initial-run comparison are consistent. Independent review confirmed the aggregate 64 KiB composer bound.

Full Rust workspace checkpoint after native Fast, preference transaction, quote atomicity and aggregate context fixes: `cargo test --workspace` passed; log saved as checkpoint-fast-rust.txt in the task visualization directory. `cargo fmt --all -- --check` initially found formatting only; `cargo fmt --all` was applied and the subsequent check passed. No commit was created. `git diff --check` passed. Frontend Fast UI and final Web/desktop build checks remain in progress.


## Reviewer implementation boundaries

Use the existing session_reviews table and a separate retrospective report contract, not a fabricated deterministic verification result. Reviewer selection is a host-owned setting with FollowSession (legacy-compatible default), DefaultHttp (requires an explicitly configured default reviewer profile), and HttpProfile (an exact configured ID). Ruling: FollowSession resolves the explicitly selected session model profile supplied by the same trusted start contract used for ordinary sends, then freezes its configuration; it must not choose an arbitrary first profile. Independence means an isolated read-only review request with a fixed rubric, not necessarily a different model provider. ACP selection remains part of the later real ACP adapter slice.

Persist request identity, source IDs and sanitized bounded source snapshot, reviewer configuration hash, status and report. Atomic per-conversation review acquisition prevents duplicate dispatch and conflicts with active runs; matching request IDs return the existing operation. Persisted interrupted reviews are abandoned on host startup, never silently reissued. Manual review does not emit AgentCore ReviewerFinished/RunCompleted or imply scientific validation. Automatic review uses a separate frozen reviewer binding in the run spec and keeps mandatory verification unchanged.

Fast frontend independent checkpoint: `npm test` passed 413 Vitest tests across 41 files plus 22 browser-extension tests; `npm run build` passed (1,146.64 kB main chunk warning). Failure-retry identity now uses stable reference keys, not object property order. Explicit Fast off writes false; model-default reset writes null. Reviewer native/store/UI work begins after this checkpoint, so later final checks must include that slice.


Reviewer native checkpoint: five retrospective transport/source/parser tests passed; a frozen automatic-reviewer selection test passed; all desktop library tests then passed (239 passed, 5 ignored). The approval regression now verifies that the reviewer binding from planning survives a different current global selection and is preserved in the persisted RunSpec. Cross-UI contract coverage rejects client-supplied review sources and forged verification fields. New review bindings omit execution resources and attachment bytes from the independent model port; the automatic reviewer assesses its textual frozen ReviewerRequest, not original images or an independently rerun experiment. The manual report likewise describes bounded message excerpts only.

Frontend review follow-up: the native host alone owns Running/Abandoned transitions. A webview remount or a newly opened dialog cannot infer abandonment from missing local ownership. Loaded running records must continue polling and block conflicting submissions. Startup abandonment is durable and happens once when the native repository opens. Full workspace, frontend and desktop build checks still need to cover the final integrated reviewer slice.


## Send lifecycle follow-on: reference inspection

The pinned reference main.rs around 3960 parks a normal send to a busy session via enqueue_turn, preserving message, attachments and reference arguments. Around 4645 its ComposerQueue exposes cancel, restore-to-editor, cut-in, move-up and move-down. These are part of E01's clickable behavior, not merely a queued badge. ComposerSendAction distinguishes normal, branch, guide-append and interrupt-replace. A queue implementation needs durable scoped IDs and atomic transitions into one initial run; an optimistic React array or a second call to ordinary start is insufficient. Existing OmicsOps agent_v4_submit_guidance / Store::accept_guidance_v4 / consume_guidance_v4 provide E02's authority-preserving delivery path and should be reused. The queue/replace implementation remains pending the completed reviewer integration and its final checks.


Retrospective review visual QA: actual SessionReviewDialog with synthetic Chinese findings and stored source/hash metadata rendered at 1280×800 (dialog 1042 px) and 480×800 (dialog 462 px), without internal horizontal overflow. Root inspected both screenshots; long report content scrolls while the close/request controls remain visible. Files: review-dialog-desktop.png, review-dialog-narrow.png, review-dialog-measurements.json in the task visualization directory. This is component layout QA, not native WebView2 or live-model acceptance. The first fixture captured a transient missing helper during concurrent implementation; that intermediate blank render was diagnosed, rebuilt after the helper fix, and replaced by the successful captures. Fast QA separately found narrow permission/settings overflow; those CSS fixes are still being verified.


Final native reviewer sweep (2026-09-14): cargo test --workspace passed after the Unicode/control-whitespace source qualification fix and formatting; log checkpoint-review-final-rust.txt. Independent native/protocol review found no additional confirmed defects in authority, source snapshots, idempotency, CAS, startup abandonment, frozen reviewer propagation or legacy hash compatibility. The focused Store reviewer suite passes nine tests. Review API and lifecycle-hook tests passed eight cases at the current frontend checkpoint; a settings rollback regression correctly exposed a draft-versus-persisted-baseline bug and remains under repair before the final frontend suite. C08 still excludes ACP pending its genuine adapter phase.


Read pinned src-tauri/src/agent_turn.rs for the queue implementation. Its enqueue_turn uses a process-local Vec plus an atomic draining slot; cut-in removes a queued item and applies its text as guidance, restoring it to the front if the active turn ends before consumption. Reference cut-in drops attachments. OmicsOps must preserve the approved durable-queue requirement and must not silently discard attached scientific context: cut-in should either resolve bounded supported references into the authorized guidance path or explicitly retain the original queued item when attachments cannot be delivered. Remote job cancellation remains independent of interrupting a local Agent wait.


Multi-instance recovery correction: this app has no verified global single-instance exclusion. Therefore startup cannot abandon every running review. Each new review acquires a per-request OS file lock in the host data directory before creating its durable record and holds it through result persistence. Startup and scoped list/get reconciliation attempt the same lock; only a record with no live lock holder can transition to Abandoned using a scoped running-only CAS. Lock pathnames are retained (no unlink/recreate race); they contain no transcript or credentials. This also allows one live app window to recover a review whose other owning process exited. The implementation uses the standard library [File::try_lock](https://doc.rust-lang.org/std/fs/struct.File.html#method.try_lock), supported by the current Rust toolchain, with a deterministic temporary-directory ownership regression. These changes follow the passing desktop-build checkpoint and require a fresh native/desktop verification.


Reviewer frontend checkpoint complete: 434 Vitest tests across 46 files and 22 extension tests passed; Web build passed with the existing main-chunk size warning. Independent review fixed load-failure save blocking; rejected saves restore the last persisted settings, not an edited draft. Same-ID start recovery retains its identity even after a null lookup; loaded native Running records remain Running across UI remounts. Existing review prompt-prefill tests now exercise actual Share behavior because Review opens its own durable report workflow.

Fast narrow layout follow-up complete: permission menu is centered and clamped to the viewport; the open composer permits its menu to escape clipping; settings content/model rows shrink without horizontal overflow. Actual headless computed-bounds and visibility assertions passed at 1280 and 480 px; immediate Escape layering still works. Focused ComposerIntegration/SettingsPanel tests passed 54 cases. Temporary root QA script was removed; artifacts remain outside the repository.

Native per-review lease tests passed six cases, including preservation of another live owner and abandonment only after lease release. Store scoped recovery tests passed ten cases. The previous Windows NSIS build passed before the lease correction; a final repeat remains required. No installer was published or distributed.


## E04 durable stop request Store/DTO foundation

Implement the durable local stop intent contract in the shared DTO and SQLite Store. The host will own the OS run lease and call finalization; this slice never contacts or cancels a remote job, deletes run events/evidence, changes a frozen RunSpec, or changes pending-plan cancellation rules. `observed` means that the local run already has a terminal status or terminal event, so it does not claim remote cancellation.

### Contract and persistence

Add `StopRunStatusV4::{Requested,Observed}` with snake-case serde values and deny-unknown-fields DTOs:

```rust
pub struct StopRunRequestV4 {
    pub request_id: Uuid,
    pub project_id: Uuid,
    pub conversation_id: Uuid,
    pub run_id: Uuid,
}

pub struct StopRunReceiptV4 {
    pub request_id: Uuid,
    pub project_id: Uuid,
    pub conversation_id: Uuid,
    pub run_id: Uuid,
    pub status: StopRunStatusV4,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
```

Create the idempotent `agent_run_stop_requests_v4` table in `crates/omicsops-store/migrations/init.sql`. `request_id` is the primary key, `run_id` is unique and references `agent_runs_v4(run_id) ON DELETE CASCADE`, and the persisted project/conversation columns are foreign-keyed for cleanup and scoped lookups. The status check only permits `requested` and `observed`; timestamps use the Store's existing Unix-second helpers. Reopening an initialized database must leave the table and rows intact.

### Store API and transaction behavior

Add `crates/omicsops-store/src/run_stops.rs`, register it from the Store crate, and expose:

```rust
impl Store {
    pub async fn request_run_stop_v4(
        &self,
        request: &StopRunRequestV4,
    ) -> Result<StopRunReceiptV4, StoreError>;
    pub async fn get_run_stop_v4(
        &self,
        project_id: Uuid,
        conversation_id: Uuid,
        run_id: Uuid,
    ) -> Result<Option<StopRunReceiptV4>, StoreError>;
    pub async fn has_run_stop_request_v4(&self, run_id: Uuid) -> Result<bool, StoreError>;
    pub async fn finalize_inactive_run_stop_v4(
        &self,
        project_id: Uuid,
        conversation_id: Uuid,
        run_id: Uuid,
    ) -> Result<Option<AgentEventV4>, StoreError>;
}
```

Every mutating/read-reconcile operation starts `BEGIN IMMEDIATE`, validates project/conversation/run ownership before returning scoped data, and validates the raw request before cloning or inserting it. A retry with the same request ID returns the original scoped receipt; a cross-scope reuse errors. A new request for a run that already has a stop row returns that row and does not insert a second intent. A newly inserted receipt is `observed` when the local run is already terminal, otherwise `requested`. `get_run_stop_v4` reconciles a requested receipt to `observed` only after checking the current local status/event chain. `has_run_stop_request_v4(run_id)` is a host-internal existence check and never exposes transcript data.

`finalize_inactive_run_stop_v4` requires the caller's run lease, checks the scoped intent and current local terminal status/event chain, and returns `None` without rewriting a terminal run. For a nonterminal run with a requested intent, one transaction updates only the run's local status/value JSON to `cancelled`, appends exactly one existing hash-chain `RunCancelled` event through `insert_agent_event_in_tx`, and marks the receipt `observed`. Repeated finalization is idempotent. No plan row is changed; `cancel_plan_v4` remains the only pending-plan cancellation path.

Reject new `accept_guidance_v4` rows in the same transaction whenever the run has any stop intent, preserving an existing identical guidance retry before the new-stop check. Add the pending-stop check to `ensure_conversation_unlocked_executor` so ordinary sends/starts cannot race a durable stop; both operations already serialize through `BEGIN IMMEDIATE`.

### TDD implementation steps

1. Add `crates/omicsops-dto/tests/run_stops.rs` with serde round-trip and deny-unknown-fields tests. Run `cargo test -p omicsops-dto --test run_stops`; it must first fail because the shared types and module exports do not exist.
2. Add the DTO module and exports, then rerun the DTO test until the wire contract is green.
3. Add `crates/omicsops-store/tests/run_stops.rs` covering migration/reopen, same-request and same-run idempotency, cross-scope rejection, terminal `observed` reconciliation, guidance/new-send rejection after a stop, finalization event/hash/CAS behavior, no-intent no-op, foreign source scope, deletion cascades, and raw request/status bounds. Run `cargo test -p omicsops-store --test run_stops`; it must first fail to compile because the table and Store methods are absent.
4. Add the migration and Store implementation with bounded UUID/status parsing and the existing event/hash helpers; rerun the focused Store test until it is green. Include a multi-operation test that runs concurrent same-run requests and asserts one canonical row/receipt.
5. Run `cargo fmt --all -- --check`, `cargo test -p omicsops-dto --test run_stops`, `cargo test -p omicsops-store --test run_stops`, and `git diff --check`. Do not run remote jobs or use credentials; report full workspace checks to the parent for the integrated native slice.

Only `crates/omicsops-dto/src/run_stops.rs`, its DTO export, `crates/omicsops-store/src/run_stops.rs`, the Store module/export/lock hooks, the idempotent schema addition, and their deterministic tests are in scope. Do not edit `src-tauri/src/agent_v4.rs`, AgentCore, or frontend code.

E02 integration checkpoint: GuidanceDialog, GuidancePanel and WorkspaceShell passed 88 focused tests. Pending guidance drafts retain their message ID across close/reopen, and a draft clears only after the exact accepted markdown is returned. Synthetic component QA at 1280 and 480 px produced `guidance-dialog-*` captures in the task artifact directory; dialogs measured 700 and 460 px without horizontal overflow.

E04 Store/DTO checkpoint: the shared stop DTO wire tests pass 2 cases; the Store stop suite passes 14 cases covering scoped idempotency, terminal observation and repair, hash-chain cancellation, uncertain side-effect preservation, guidance/send locks, reopen and project/conversation cascades, concurrency, stale snapshots, and both schema exports. `cargo test -p omicsops-desktop --lib` passed 252 tests before the final Store-only regression additions; the focused DTO/Store commands were rerun after those additions.


### 2026-09-14 Stop 集成审查与收尾

- E02 的宿主已接收请求身份由 WorkspaceShell 持有，关闭/重开指导对话框后仍核对同一 message_id；仅在已接收 markdown 与原主草稿一致时清空主草稿。相关 88 项测试通过，真实组件合成数据的 1280/480 像素视觉检查无横向溢出。
- Stop 前端使用按 project/conversation/run 作用域的 request/get API、稳定请求 ID、Requested/Observed 回执及切换会话后的异步结果保护。恢复会话时先完成事件协调，再读取状态，避免旧 running 快照覆盖已停止的运行。完整前端验证：460 Vitest + 22 扩展测试，生产 Web 构建通过。
- 原生 driver 持有 OS 文件锁；等待锁的调用取得所有权后重新读取运行状态、冻结 spec 和事件头。任何终态不再派发，等待期间已经推进的非终态需刷新后再请求。原生 248 项通过、5 项忽略。
- Stop 请求一旦提交，后续观察/广播失败保留并返回已提交回执；不将已受理的请求误报为未受理。离线/进程重启后仍可按运行找到唯一意图。
- AgentCore batch 取消修复已完成：保留已完成结果，对未完成副作用先持久化 ToolDispatchUncertain，再结束为 RunNeedsAttention；只读调用记录取消结果。原生最终状态以持久事件为准，无法确认终态时保留 needs_attention。普通运行的 E04 停止已验证；队列取消 E01/E04、E03 替换、E05/E06 等仍属于后续范围。
- 最终集成验证：cargo test --workspace 通过（原生 249 项通过、5 项忽略；AgentCore 106 项通过），npm test 通过（49 个文件、461 Vitest + 22 扩展测试），npm run build:desktop 通过（包含生产 Web 构建），cargo fmt --all -- --check 通过。对应日志 checkpoint-stop-final-rust.txt、checkpoint-stop-final-frontend.txt、checkpoint-stop-final-desktop.txt 保存在任务可视化目录。Guidance 未知结果重试现在按项目/会话/运行分别保留 UUID，跨会话切换后仍能核对同一请求。无真实模型或 SSH 验收，无发布/提交/安装包分发。

### E06 branch foundation checkpoint (incomplete parity)

The scoped checkpoint DTO, atomic branch transcript copy, lineage, idempotent creation and source navigation are implemented. Branch creation copies no run, approval or execution evidence into new authority. Focused Store and frontend tests cover stale boundaries, duplicate request recovery and navigation recovery.

This does not yet complete E06. Pinned `main.rs` around 4028 creates a composer branch and then continues the send lifecycle; the current UI only creates/opens its snapshot and uses the draft as its title. Branch-and-send must preserve all draft material, clone conversation-owned attachment/quote receipts safely, and use stable queue IDs after creation. Message-level branching also restores the selected user text into the composer in the reference. Guarded merge behavior is still pending. Do not label these remaining actions implemented on the basis of snapshot creation tests.

### 2026-09-14 queue / branch / context UI integration

- Native desktop idle and busy sends route through durable queue acceptance. The existing direct callback remains for browser preview/legacy test coverage. Native-route integration verifies no separate submit/start call and recovery of the committed user message when delivery events are absent.
- Queue UI supports scoped CAS edit/cancel/reorder, complete payload restoration after confirmed cancellation, failed-payload restoration, and typed text-only guidance cut-in UI. Guidance-delivered receipts are distinguished from cancelled rows and cannot be restored as if unsent. Native cut-in transaction integration is still under construction at this checkpoint.
- Branch-and-send UI now uses the shared complete payload request and reserved branch/queue/message/run IDs. Unknown retry retains the same request; committed navigation failure does not enqueue again; long markdown retains its full body while only the title is bounded. Message-level branching restores the selected user text into the draft, including after navigation retry. Native material-copy and full integration verification remain in progress; guarded merge remains pending.
- Context UI is connected to `agent_v4_context_usage`, with scope guards, unavailable/stale data presentation, typed counters, expandable facets, dock/float/drag/resize, Escape and source-change closure. Provider propagation/projection are implemented, but the actual current-context occupancy and conservative request budget still need their complete measured metadata path; no complete D08/D09/D10 parity claim yet.
- Full frontend checkpoint: 59 files / 511 Vitest tests plus 22 extension tests passed (`checkpoint-branch-send-frontend.txt`). Later focused branch/panel/recovery tests pass. Earlier integrated Rust workspace checkpoint passed, including native 253 passed / 5 ignored and AgentCore 110 passed (`checkpoint-queue-runtime-rust.txt`); new native work requires fresh final checks.
- `cargo fmt --all -- --check` reported formatting-only differences, then `cargo fmt --all` completed. No Git commit is made in this read-only Git workspace; formatting-only changes must be separated if/when commits are prepared.

### 2026-09-14 recovery and frontend verification follow-up

- A stale driver with unresolved non-read-only tool dispatch now repairs to `needs_attention`, preserving the FIFO fence; resolved dispatch and read-only interruption retain normal failure recovery. The existing terminal repair regression covers runtime, network, mutating, delegation, read-only and unclassified uncertainty. Native verification is pending after concurrent worker builds finish (one attempt hit Windows LNK1104 while the test executable was occupied).
- Full frontend checkpoint passed 513 Vitest tests plus 22 extension tests (`checkpoint-cutin-context-frontend.txt`). A subsequent floating-panel viewport resize regression failed before the fix and passed afterwards (7 focused panel tests). Production Web build passed (`checkpoint-context-web-build.txt`).
- The new request-start metadata fields are mirrored in TypeScript. D08 actual measurement, E01 cut-in and E06 branch material work still require the next integrated Rust and desktop checks. This checkpoint does not mark the 64-item parity scope complete.

Integrated checkpoint `checkpoint-branch-cutin-usage-rust-2.txt`: `cargo test --workspace` passed, including AgentCore 113 tests and desktop 264 passed / 5 ignored. `checkpoint-context-final-frontend.txt` passed 59 files / 514 Vitest tests plus 22 extension tests; `checkpoint-context-final-web.txt` production build passed. Subsequent independent recovery review identified failed-status uncertainty and event/status crash windows; those fixes require fresh validation. The provider retry persistence gap remains explicit, and D10 / E05 / guarded branch merge are not complete.

2026-09-14 integrated side-chat checkpoint: native side-chat send/list/get, durable evidence/material snapshots, one-shot tool-free model dispatch and per-request ownership recovery are now wired. Full workspace tests passed (desktop 272 passed / 5 ignored); latest frontend passed 540 Vitest + 22 extension tests and Web build. Desktop NSIS build passed before the final frontend timestamp-order correction. D10 host exposes only a completed-run NotNeeded receipt until same-run model projection/admission is proven. F02/F03/F15 commands are connected; E03 and guarded merge remain planned. See the side-chat plan for audit limitations and log filenames. No commit, release, installer distribution or real-model/SSH acceptance.
