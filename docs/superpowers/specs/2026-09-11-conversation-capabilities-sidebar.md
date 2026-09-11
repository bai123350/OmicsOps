# Conversation capability sidebar

Add the Wisp-inspired footer requested by the user: counts for Skills, MCP and
Memory above vertical Capabilities and Settings actions, pinned to the bottom of
the existing left conversation navigation. Preserve the language selector.

Counts come from a read-only snapshot validated against the active project and
conversation. Skills count the Agent's selected packages including dependency
closure; the overview also lists installed inactive packages. MCP counts enabled
configured servers with discoverable tools, never the number of tools or all
unconfigured bundled presets. Memory counts the actual project-wide facts
searchable by V4 (the Agent currently shares this scope across conversations).
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

Review follow-ups: deleting another conversation refreshes the shared project
memory count; the conversation list can shrink and scroll to preserve footer
space in short windows.

Verification: workspace tests passed (738 passed, 11 ignored); frontend tests
passed (179), browser extension tests passed (22), and production Web build
passed. The final Windows desktop build also passed. Browser demo verified the overview, Settings navigation and immediate
Escape. The final 900×560 layout recheck was not completed because the preview
connection failed. Native UI with real data and live model/SSH acceptance were
not executed.
