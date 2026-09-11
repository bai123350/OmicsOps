# Conversation capability sidebar

Add the Wisp-inspired footer requested by the user: counts for Skills, MCP and
Memory above vertical Capabilities and Settings actions, pinned to the bottom of
the existing left conversation navigation. Preserve the language selector.

Counts come from a read-only snapshot validated against the active project and
conversation. Skills count the Agent's selected packages including dependency
closure; the overview also lists installed inactive packages. MCP counts enabled
configured servers with discoverable tools, never the number of tools or all
unconfigured bundled presets. Memory counts regular `.md` files directly under the local project root
`.omicsops/memory/`, using Wisp’s file-based counting unit with the OmicsOps directory.
Do not recurse, count messages/artifacts/notebook entries, create directories,
or contact SSH hosts. Missing directories count as zero; read failures surface as errors.
Agent memory retrieval reads these same files. `save_memory` creates a new
Markdown file under mutation approval, without overwriting existing notes.
Reading does not create directories. SSH conversations use the local project
memory directory without remote connections. Database messages, artifacts and
notebook entries remain intact but are not synthesized into memory documents.
No automatic migration or model summarization is performed.
Explain these scopes and that enabled MCP still requires call authorization.
This does not add conversation-specific installation or change Agent access.

Capabilities opens an overview with counts/status lists and a manage action to
the existing Skills/MCP settings. Settings opens existing model settings. Use
the window Escape stack; an immediate Escape closes only the topmost layer.
No network request, process start or authorization mutation occurs on inspection.

Refresh after conversation switches, configuration changes, new messages and
run completion. Discard stale or mismatched responses. Loading/error/unselected
states must not show a previous conversation's counts or a false successful zero.
Offer retry for read failures. Memory search filters elsewhere must not alter
this authoritative count.

Tests: backend scope and effective selection, DTO roundtrip, frontend counts and
navigation, window Escape layering, asynchronous switch/race/retry. Run default
workspace/frontend/build checks and desktop build for the new native command.

Review follow-ups: conversation-list changes refresh the snapshot; the conversation list can shrink and scroll to preserve footer
space in short windows.

Verification: workspace tests passed (738 passed, 11 ignored); frontend tests
passed (179), browser extension tests passed (22), and production Web build
passed. The final Windows desktop build also passed. Browser demo verified the overview, Settings navigation and immediate
Escape. The final 900×560 layout recheck was not completed because the preview
connection failed. Native UI with real data and live model/SSH acceptance were
not executed.

File-count alignment verification: regression failed with the old message count
(2 instead of 3 Markdown files), then passed after the change. Workspace tests:
741 passed, 11 ignored; frontend: 179 passed; extension: 22 passed. Web and
Windows desktop builds passed. No installed-app UI or real SSH acceptance was
performed for the counter change.

Unified memory follow-up: `save_memory` is a Mutating tool excluded from Plan
mode. A shared storage module atomically creates new notes without overwrite;
directory links/escapes and invalid filenames are rejected. Search returns
`memory_file` sources from the same directory counted by the sidebar. Notes
must be UTF-8 and at most 256 KiB; failures identify the offending file rather
than silently returning partial search results. Deterministic checks cover
save/search/count, disk edits/deletes, path validation, no-overwrite behavior,
Plan exclusion and SSH conversation storage without a remote connection.
Final checks: 744 Rust tests passed, 11 ignored; 179 frontend and 22 extension
tests passed; production Web and Windows desktop builds passed. Real model,
SSH and installed desktop UI acceptance were not executed.
