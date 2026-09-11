# Agent MCP approval and recovery

## Observable behavior

- RiskBased (帮我批准) skips the generic network approval card only for a currently enabled, launch-approved, persistently tool-approved MCP target whose exact server, tool, catalog hash and schema hash match and whose configured readOnlyHint is true. The hint remains a third-party assertion the user chose to trust. Unknown or mutating MCP calls retain approval. RequestApproval retains its per-call prompt. Invocation rechecks current authorization; this does not change frozen run permissions.
- Agent browser descriptors are temporarily hidden and host dispatch is disabled. Pending, undispatched browser calls are rejected before approval. Browser bridge code is retained for later; no fallback is proposed in the desktop model request.
- MCP isError responses produce failed tool outcomes and failed MCP audit records, preserving original result evidence. A completed protocol exchange is not a successful literature search.
- Bundled PubMed omits obvious API-key template placeholders from request URLs. Other invalid credentials still produce real errors. Credentials stay in the existing vault; no values are logged or migrated.
- A truncated model turn is discarded and gets one retry requesting one concise complete call. No partial tools or partial text are committed. A second truncation fails with existing evidence retained. Provider budget validation and cancellation still apply.
- Exact catalog reasoning capabilities reserve up to 16384 output tokens, bounded by the output limit and half the effective context. Unknown gateways retain 4096. Changed model budget is part of the frozen configuration hash: start a new run to use it, rather than silently rewriting an old run.

## Validation

Deterministic coverage includes approval policy separation, changed/revoked MCP bindings, hidden/blocked browser tools, MCP error status, PubMed placeholder URLs, truncated-turn retry bounds and discarded partial output, and catalog output budgets. Run workspace tests, frontend tests, web and desktop builds.

Manual smoke: launch the newly built application, create a new ordinary run with 帮我批准, use an inspected and persistently approved read-only literature MCP, and confirm repeated queries have no generic approval cards. Verify an unapproved tool still pauses; an isError response is red; no browser calls are offered. Real model/MCP and SSH acceptance require the disposable environment in acceptance/README.md and are distinct from these tests.

Executed 2026-09-11: `cargo test --workspace` (412 passed, 11 ignored); `npm test` (147 frontend + 22 extension passed); `npm run build`; `cargo fmt --all -- --check`; `npm run build:desktop` with `CARGO_TARGET_DIR=E:\Project\OmicsOps\target\timeline-desktop` (passed). Real model/MCP and SSH acceptance was not executed. The desktop build emits the existing large Web chunk and Windows linker-message warnings.
