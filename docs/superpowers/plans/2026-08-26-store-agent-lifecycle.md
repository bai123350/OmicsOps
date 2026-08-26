# Store and Agent Session Lifecycle Implementation Plan

> **Required workflow:** execute with Superpowers `subagent-driven-development`,
> `test-driven-development`, and `verification-before-completion`.

**Goal:** Move SQLite ownership into an asynchronous `omicsops-store`, migrate
schema v3 data atomically to the v4 normalized schema, and add durable
conversation Agent/Plan modes with revision-safe plan approval and effect-based
tool gating.

**Reference boundary:** Architecture mechanisms were inspected at
[`wisp-science@8d57fb3`](https://github.com/xuzhougeng/wisp-science/tree/8d57fb34808f0073218e0b07108d146745169a3b).
That repository is AGPL-3.0. OmicsOps uses an original Apache-2.0 implementation;
reference SQL and source code must not be copied.

## Global constraints

- Work on `codex/store-agent-lifecycle` from `dafce8d` and preserve the existing
  working tree.
- Complete and verify Store compatibility before Agent/Plan integration.
- Use `sqlx::SqlitePool`; adapters must no longer own SQLite persistence.
- Migrations are backward compatible, idempotent, backed up before v4, and
  transactional. Credentials never enter SQLite, events, logs, exports, or Git.
- Preserve existing command names and serialized fields. Additive fields remain
  backward compatible.
- Every production behavior begins with a focused failing test.
- Do not push, open a PR, tag, publish, or build an installer.

## Task 1: Async Store, original v4 schema, and compatibility migration

1. Create the `omicsops-store` workspace crate using `sqlx::SqlitePool`.
2. Independently rewrite `migrations/init.sql` to cover the 58 referenced table
   concepts, add `env_snapshots`, and retain all OmicsOps model, Skill, MCP,
   Agent V4, scientific, and retired-runtime data.
3. Add a v3 fixture and failing tests for backup naming, atomic upgrade,
   rollback injection, data mapping, integrity, foreign keys, and idempotence.
4. Map projects into normalized project rows plus OmicsOps extensions;
   conversations into root frames; and JSON messages into ordered messages.
5. Validate identifiers, counts, order, foreign keys, and event hashes before
   commit. Refuse writes on failure and never double-write legacy tables.
6. Move repository persistence APIs from `omicsops-adapters` into Store while
   preserving consumer compatibility during the cutover.

## Task 2: Shared DTO and durable conversation modes

1. Create the native/wasm-safe `omicsops-dto` crate and re-export boundary
   types through `src/dto.rs`.
2. Add `SessionAgentModeV4 = agent | plan`, get/set requests and responses, and
   optional `RunSummaryV4.plan_revision` and `session_mode` fields.
3. Persist `conversation_agent_mode:{conversation_id}` in settings; legacy
   conversations default to Agent.
4. Add Tauri commands `get_conversation_agent_mode` and
   `set_conversation_agent_mode`, contract tests, and ownership validation.

## Task 3: Plan revisions, locks, and atomic approval

1. Store immutable proposed-plan revisions with structured plan, Markdown, run
   ID, revision, hash, status, and feedback.
2. Lock only the conversation with an active generating/revising/pending plan;
   allow approve, request changes, or cancel while locked.
3. Add `agent_v4_request_plan_revision({ run_id, plan_hash, feedback })` and a
   backward-compatible revision-request event.
4. Approve only the latest pending revision after checking ownership, status,
   revision, and hash. In one Store transaction freeze the specification, write
   approval/mode events, and switch the conversation to Agent. Start execution
   only after commit. Cancellation keeps Plan mode.

## Task 4: Effect-based Plan tool gate and read-only MCP

1. Replace the built-in name allowlist with `ToolEffectV4::ReadOnly`, plus
   `agent.request_input` and `agent.propose_plan`.
2. Deny runtime, mutation, network, and delegation by default in Plan.
3. Permit an MCP target only when its current `annotations.readOnlyHint` is
   exactly true, server state and startup approval are valid, and catalog/schema
   hashes match.
4. Reuse the existing schema-bound one-time tool approval to pause and resume
   planning. Re-check all facts immediately before execution.
5. State in approval UI and docs that third-party read-only annotations are
   unverified hints trusted by the user, not a host guarantee.

## Task 5: Conversation UI and documentation

1. Make the Plan toggle conversation-scoped and reload its persisted state.
2. Lock the active conversation while a plan is pending and add an inline plan
   revision form. Approving immediately renders Agent mode; cancelling remains
   in Plan.
3. Add focused frontend tests for conversation switching, restart hydration,
   lock behavior, revisions, stale approval, and mode transitions. Any new
   closable overlay must join the window Escape stack.
4. Update architecture, Agent-mode user guidance, and v4 database backup and
   recovery documentation. Do not update release notes.

## Task 6: Verification and delivery

1. Run focused Store, Agent Core, Tauri, and workspace UI tests.
2. Run `cargo test --workspace`, `npm test`, `npm run build`, and
   `npm run build:desktop`.
3. Follow `acceptance/README.md` for explicitly configured real model, SSH, and
   MCP acceptance. Record absent environments as not run/ignored.
4. Perform task-scoped reviews and a final whole-branch review. Do not push or
   create a PR.
