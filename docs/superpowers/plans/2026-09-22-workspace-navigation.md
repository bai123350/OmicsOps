# Workspace navigation implementation plan

> **For agentic workers:** Use superpowers:subagent-driven-development or superpowers:executing-plans to implement the independent tasks with tests, then review the integrated change.

**Goal:** Deliver the approved sidebar, durable session groups, real research journey, versioned publication drafts and source-backed personal library.

**Architecture:** Store owns groups, revision history and immutable collection snapshots. Native commands resolve exact source identities, redact snapshots and project existing ScientificStateV4. React components consume shared DTOs through a typed API and preserve the conversation while pages are open.

**Tech Stack:** Rust/sqlx/SQLite, Tauri 2, React 19/TypeScript/Vitest. No new dependencies planned.

**Spec:** `docs/superpowers/specs/2026-09-22-workspace-navigation-design.md`

## Global constraints

- Windows first; all checks deterministic without SSH, network or model credentials.
- Cross-UI DTOs in omicsops-dto, re-exported by native dto.rs, serialization tested.
- All overlay/menu Escape handlers use the window stack and close only the top surface.
- Use existing redaction and host ownership checks. Text hashes describe the saved redacted bytes.
- No changes to Agent approvals, frozen plans, execution or automatic remote transfers.
- Stable operation IDs make creation/saves idempotent; version conflicts never overwrite.
- User's instruction “按照方案实现” authorizes execution in the existing dedicated branch. Continue through implementation and verification without another approval round.

## Review focus

1. Mixed source identities: V4/legacy artifact IDs and repeated call IDs cannot resolve the wrong source.
2. Late async results after navigation cannot overwrite another project's state.
3. Missing source and SSH rebinding cannot silently read a replacement path.
4. Lost save responses retry the same operation; draft navigation preserves unsaved text.
5. Generated code is never displayed as executed or scientifically verified.

## Shared interface contract

New Rust DTO module: `crates/omicsops-dto/src/workspace_navigation.rs` (re-export from lib.rs).
Frontend types: `src/workspace-navigation-types.ts`; API: `src/workspace-navigation-api.ts` (re-export via existing types.ts/tauri-api.ts where useful).

All timestamps are RFC3339 strings. UUIDs are strings in TypeScript. Nullable fields below serialize explicitly as null. Source byte ranges use UTF-8 offsets, verified at character boundaries.

```ts
type SourceKind = "message" | "tool" | "dataset" | "analysis" | "artifact" | "evidence" | "provenance" | "legacy_artifact" | "run";
interface WorkspaceSourceRef {
  project_id: string; kind: SourceKind; id: string;
  conversation_id: string | null; run_id: string | null;
  sequence: number | null; event_hash: string | null;
  content_sha256: string | null; start: number | null; end: number | null;
}
interface WorkspaceSourceSnapshot {
  source: WorkspaceSourceRef; title: string; text: string; sha256: string;
  status: string; metadata: Record<string,string>;
  availability: "available" | "changed" | "missing" | "unavailable";
}
interface ConversationGroup { id:string; project_id:string; name:string; created_at:string; updated_at:string }
interface ConversationGroupState { groups:ConversationGroup[]; memberships:{conversation_id:string;group_id:string}[] }
interface JourneyEntry { source:WorkspaceSourceRef; title:string; status:string; occurred_at:string; summary:string }
interface JourneyPage { entries:JourneyEntry[]; next_offset:number|null }
interface PublicationSummary { id:string; project_id:string; title:string; revision:number; updated_at:string }
interface PublicationRevision { id:string; publication_id:string; revision:number; title:string; markdown:string; references:WorkspaceSourceSnapshot[]; sha256:string; created_at:string; legacy:boolean }
interface PublicationDetail { publication:PublicationSummary; revisions:PublicationRevision[] }
interface LibrarySummary { id:string; kind:"code"|"excerpt"|"artifact"; title:string; source_project_id:string; source_project_name:string; source_conversation_id:string|null; source_conversation_title:string|null; text_preview:string; created_at:string }
interface LibraryDetail { item:LibrarySummary; snapshot:WorkspaceSourceSnapshot }
interface LibraryPage { items:LibrarySummary[]; next_offset:number|null; source_projects:{id:string;name:string}[] }
```

Tauri command names and arguments (all request objects use snake_case, individual arguments camelCase through invoke):

```ts
workspace_list_groups({projectId}) => ConversationGroupState
workspace_save_group({request:{request_id,project_id,group_id:null|string,name}}) => ConversationGroup
workspace_delete_group({projectId,groupId}) => void
workspace_move_conversations({request:{project_id,group_id:null|string,conversation_ids:string[]}}) => void
workspace_journey({request:{project_id,query:string,kind:SourceKind|null,status?:string|null,offset:number,limit:number}}) => JourneyPage
workspace_source_detail({source:WorkspaceSourceRef}) => WorkspaceSourceSnapshot
workspace_list_publications({projectId}) => PublicationSummary[]
workspace_get_publication({projectId,publicationId}) => PublicationDetail
workspace_save_publication({request:{request_id,project_id,publication_id:null|string,expected_revision:number,title,markdown,sources:WorkspaceSourceRef[]}}) => PublicationDetail
workspace_restore_publication({request:{request_id,project_id,publication_id,expected_revision:number,revision:number}}) => PublicationDetail
workspace_export_publication({projectId,publicationId,revision:number}) => string|null // host save dialog; null = cancelled
workspace_list_library({request:{query:string,kind:null|"code"|"excerpt"|"artifact",project_id:null|string,offset:number,limit:number}}) => LibraryPage
workspace_get_library_item({itemId}) => LibraryDetail
workspace_save_library_item({request:{request_id,title,kind:"code"|"excerpt"|"artifact",source:WorkspaceSourceRef}}) => LibraryDetail
workspace_delete_library_item({itemId}) => void
```

## Task 1: Shared contracts and durable persistence

Owner: persistence worker. Own DTO module/lib re-export; Store new workspace_navigation.rs, init.sql and minimal lib.rs module registration; Store tests.

- [x] Add failing Store tests for groups, cross-project all-or-nothing moves, restart, duplicate request rejection, deletion preserving conversations.
- [x] Implement group APIs and idempotent schema additions, using a request ledger for stable operation IDs.
- [x] Add failing publication tests for append-only revisions, expected-head conflicts, stable retry, legacy content, restore with preserved historical title.
- [x] Implement publication APIs accepting host-resolved snapshots; normalize persisted title/text and validate bounded sizes.
- [x] Add failing library tests for immutable snapshots, source deletion survival, pagination/search, remove-only-collection and request replay.
- [x] Implement library storage with no cascading foreign key to source projects/messages.
- [x] Run `cargo test -p omicsops-store --test workspace_navigation` and record output.

Rust method names mirror the command suffix with a `workspace_` prefix. Publication Store write accepts a `SavePublicationRecord` including resolved references; native save request contains only sources. Library Store write accepts `SaveLibraryRecord` including source names and validated snapshot. Coordinate exact method signatures with native worker before implementation.

## Task 2: Native source resolver, journey and desktop commands

Owner: native worker. Own src-tauri/src/workspace_navigation.rs, lib.rs registration, dto_contract_tests.rs additions; optional Store workspace_journey.rs with minimal module registration coordinated with Task 1.

- [x] Write failing tests for verified source resolution: UTF-8 message ranges and hashes, run+sequence+event hash+call identity, same call ID in two runs, cross-project rejection.
- [x] Implement a redacted source snapshot resolver from persisted messages/events/scientific state and legacy artifacts. Exclude raw process identity and credentials.
- [x] Project a bounded journey list with real timestamps, stable source identity, no guessed run/conversation association, per-source detail and explicit source status.
- [x] Add command wrappers for groups/publications/library; host resolves snapshots and names rather than trusting client-provided status.
- [x] On detail reads compare saved snapshots with the current source. Preserve original snapshot text/hash; decorate availability as changed/missing/unavailable.
- [x] Implement saved-revision Markdown export using a native save dialog, preserved revision title, redaction and cancellation return.
- [x] Add DTO serialization tests and run targeted desktop library tests.

## Task 3: Research journey, publication and library pages

Owner: pages worker. Own new `WorkspaceResearchPages.tsx`, `workspace-research-pages.css`, component tests. Root supplies TS contract/API. Export `WorkspaceResearchPages` with props `{ page:"journey"|"publication"|"library", projectId, locale, onOpenConversation:(projectId,conversationId)=>void, onInsert:(text)=>void, registerBeforeLeave?:(guard:(next:()=>void,onCancel?:()=>void)=>void)=>void }`.

- [x] Write failing tests for loading/retry/empty/detail, late-result discard after project change, immediate Escape closing only source detail.
- [x] Implement journey filtering, pagination, source details, source navigation and artifact/provenance collection.
- [x] Write failing tests for draft save/head conflict/retry, versions/read-only history/restore/export cancellation and unsaved leave guard.
- [x] Implement manuscript list/editor, evidence picker using journey, saved versions and host export. Unsaved text stays when save fails; retry reuses the exact request ID and payload.
- [x] Write failing tests for collection search/filter/detail/remove, source missing/changed and insert appending without sending.
- [x] Implement library using bounded list summaries and on-demand detail. No arbitrary path preview. Show source metadata when preview unsupported or identity cannot be verified.
- [x] Run targeted Vitest tests and record output.

## Task 4: Sidebar, APIs and integration

Owner: root. Own TS types/API/tests; new WorkspaceNavigation component/hooks/CSS/tests; WorkspaceShell, DesktopApp integration; source collection affordances in SidebarPanels/MessageSelectionActions as needed.

- [x] Write API tests asserting exact invoke names and payloads; implement wrappers and common types matching Task 1.
- [x] Write sidebar tests for entry order/actions, group persistence, filter/status/sort/date buckets, select/move, collapse, group editor Escape.
- [x] Implement navigation with reusable existing capabilities footer, local preference storage only for view state, durable groups through host API.
- [x] Wire project pages while keeping conversation mounted and preserving draft. Register unsaved-publication guard for sidebar/cross-project navigation.
- [x] Add local/remote Files view without automatic transfers. Preserve existing local list limits and file preview restrictions.
- [x] Connect exact code/message collection identities using persisted message digest/ranges or tool event identity; append library insertions to draft without sending.
- [x] Reuse current app shortcut dispatch; add Ctrl+N only where absent and avoid modal/busy duplicate dispatch.
- [x] Run closest component/API/integration tests.

## Task 5: Integrated review and verification

- [x] Review changed contracts, ownership and UI layering independently; fix actionable findings and add regression coverage.
- [x] Run `cargo fmt --all -- --check`; if formatting only fails, run fmt and isolate formatting-only changes per AGENTS.md.
- [x] Run `cargo test --workspace`, `npm test`, `npm run build`, `npm run build:desktop`.
- [x] Run `npm ci` if a lockfile changed. Inspect UI with mocked/synthetic local data; do not claim native or remote smoke from browser-only checks.
- [x] Update spec/plan with executed results and honest limitations; keep generated output and real data out of Git.

## Execution record

- 2026-09-22: user approved implementation. Existing branch is dedicated and clean; no second checkout needed. Independent ownership above enables parallel implementation without overlapping feature files.

### Final verification — 2026-09-22

| Check | Actual result |
| --- | --- |
| `cargo test --workspace` | Passed: 1286 tests; 0 failed; 12 live-environment tests ignored. |
| `npm test` | Passed: 105 Vitest files / 884 tests, plus 22 browser-extension Node tests. |
| `npm run build` | Passed, including the desktop pre-build invocation on the final application sources. |
| `npm run build:desktop` | Passed: Windows x64 release application and local NSIS bundle built. No installation, distribution, tag or release performed. |
| `cargo fmt --all -- --check` | Passed. Initial check moved a newly added export to the standard location; `cargo fmt --all` produced no unrelated formatting-only changes. |
| `git diff --check` | Passed. |
| `npm ci` | Not needed: package manifests and dependency lockfiles unchanged. |

The final frontend run began at 16:00:59 local time. Earlier full frontend runs exposed three tests that treated an initial placeholder heading as completed hydration; those now wait for the expected host call / message load. Tests were rerun until the full suite passed. The final desktop build used the final application sources; the subsequent edits only strengthened those test waits and updated these records.

### Reviewed behavior

- Group creation/rename/replay, project ownership and atomic bulk moves; deletion only ungroups sessions.
- Host-resolved message UTF-8 ranges and tool event identities; legacy run IDs do not alias V4 run IDs, even on UUID collisions.
- Publication CAS and append-only history; retries reuse requests; legacy restore redacts before persisting a new version; saved references remain usable when their source disappears. Explicit pre-write rejection preserves an editable draft; ambiguous failures preserve request identity.
- Global immutable collection snapshots survive deleted source projects/conversations. All source-project filter options are returned independently of the current page; journey status is filtered before pagination.
- Manuscript leave protection covers sidebar, search and Usage navigation. Search waits for async completion; cancelled decisions can be retried. Background plan updates keep the manuscript mounted.
- Window Escape ordering covers group editor, research detail, nested evidence chooser, global search and the unsaved manuscript decision.

### UI inspection and remaining acceptance

Browser preview used a synthetic, in-memory project at native default 1100×680 and minimum 900×560 sizes. Checked navigation entries, local/SSH file source controls and list-limit notice, full-width research content after Files, manuscript editing/leave/Escape preservation, and the small-window navigation scroll to Settings. At 900px, collapsed navigation plus Files uses two grid columns (`64px 836px`) with the existing floating file panel, without an empty reserved column. Temporary browser tabs and preview servers were closed.

Browser preview deliberately has no native persistence; its desktop-required errors were not counted as host integration success. Store tests used temporary databases and reopened connections; native tests used persisted synthetic sources and temporary export paths. No real credentials, samples or remote output were added.

Not executed: manual Windows native save-dialog/restart smoke, live SSH/model/browser-extension acceptance, R/Micromamba/PBMC workflows. The 12 ignored cases remain ignored. Artifact collections are metadata references with file availability unverified, not copied file bytes or an evidence capsule. Existing non-blocking large-bundle and Windows linker informational warnings remain.
