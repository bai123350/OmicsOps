# Agent output disclosure

The V4 conversation keeps public progress inside the execution timeline and the final answer as a separate assistant message. During an active run, the latest public progress is expanded as rendered Markdown; its disclosure remains manually collapsible. A running tool call opens its input and detail view automatically. A successful or reused result closes its detail view while keeping the summary visible. Failed calls remain expanded so the error is visible.

This uses the live-tool disclosure and Markdown progress pattern in [wisp-science's chat renderer](https://github.com/xuzhougeng/wisp-science/blob/main/ui/src/chat_render.rs) as a reference. OmicsOps keeps its existing evidence cards.

The 2026-09-25 follow-up adds a separate, expandable live plain-text "模型思考" view for reasoning that an external provider actually streams. It uses a bounded, host-redacted, run-and-attempt-scoped transient channel; it never becomes the public Markdown progress or final answer. A pending model request or tool call shows its real state and elapsed duration outside the collapsible execution timeline. The clock changes only the displayed duration; it does not count as provider activity or suppress the 90-second silence warning. When no reasoning bytes arrive, the view states that the request was sent and is waiting for model content. Approval and user-input pauses do not present the agent as actively working.

Earlier activity still folds after six items. Completed run timelines stay compact. Tool input and output continue through the existing redaction and evidence presentation paths. Reasoning preview is transient UI only and creates no audit events.

Verified on 2026-09-24: `cargo test --workspace` (1,292 passed, 12 ignored), `npm test` (887 Vitest and 22 browser-extension tests passed), `npm run build`, and `npm run build:desktop` all passed. The desktop build produced a local NSIS bundle; it was not distributed.

The isolated ignored OpenCode Go `glm-5.3` nonce test documented in [acceptance/README.md](../../../acceptance/README.md) was also run on 2026-09-24. Its first run in the restricted sandbox failed credential preflight in 0.01 seconds, before any model request. Running the same test in the normal Windows user session passed (1/1, 15.15 seconds, exit 0): the model read the temporary file through the Agent tool and returned the nonce in its final response. This validates that narrow model-to-tool-to-final path. It does not validate literature research, SSH, or live GUI activity and disclosure behavior.
