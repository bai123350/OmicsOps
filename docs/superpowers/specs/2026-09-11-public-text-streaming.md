# Public Agent text streaming

The provider already yields public TextDelta chunks. Agent core now forwards a transient cumulative text snapshot through EventStoreV4, immediately on the first chunk and at most once per 40 ms thereafter. The desktop broadcasts the shared AgentTextPreviewV4 DTO on agent-v4-text-preview. This channel is presentation-only: it never enters the event hash chain, model context, database or scientific evidence.

The current conversation accepts previews only for one of its known run IDs. The active run renders a single PROGRESS row with an inline cursor, updating the same DOM node and using existing scroll-follow behavior. The completed ModelText event replaces the preview. An attempt-scoped drop guard clears it on success, error, timeout, cancellation or guidance interruption, including when the model future is dropped. Only public text is forwarded; provider private reasoning and incomplete tool arguments are not displayed as text.

Completed replies retain the existing atomic persistence behavior. No schema migration or approval changes are introduced. Providers that send only tool calls, or have not yet produced public text, cannot display public prose until it arrives.

Verification covers shared DTO serialization, preview cleanup after truncated responses, the single-row update/commit behavior, and app subscription isolation. Run cargo test --workspace, npm test, npm run build, and npm run build:desktop. Real provider streaming requires a separate live smoke test; deterministic tests do not establish live provider availability.
