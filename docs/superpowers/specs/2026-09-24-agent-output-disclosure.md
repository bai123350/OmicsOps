# Agent output disclosure

The V4 conversation keeps public progress inside the execution timeline and the final answer as a separate assistant message. During an active run, the latest public progress is expanded as rendered Markdown; its disclosure remains manually collapsible. A running tool call opens its input and detail view automatically. A successful or reused result closes its detail view while keeping the summary visible. Failed calls remain expanded so the error is visible.

This uses the live-tool disclosure and Markdown progress pattern in [wisp-science's chat renderer](https://github.com/xuzhougeng/wisp-science/blob/main/ui/src/chat_render.rs) as a reference. OmicsOps keeps its existing evidence cards and hides private provider reasoning.

Earlier activity still folds after six items. Completed run timelines stay compact. Tool input and output continue through the existing redaction and evidence presentation paths; this change does not expose private model reasoning or create new audit events.

Verified on 2026-09-24: `cargo test --workspace` (1,292 passed, 12 ignored), `npm test` (887 Vitest and 22 browser-extension tests passed), `npm run build`, and `npm run build:desktop` all passed. The desktop build produced a local NSIS bundle; it was not distributed. Isolated live model acceptance is recorded separately because a successful build does not demonstrate the full end-to-end UI behavior.
