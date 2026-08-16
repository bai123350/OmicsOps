# OmicsOps Agent Harness v3 Implementation Plan

> **Required workflow:** execute with Superpowers `executing-plans`,
> `test-driven-development`, and `verification-before-completion`.

**Goal:** Replace the default one-command remote loop for new work with an
event-sourced, multi-tool scientific harness while preserving the Rust/SSH
trust boundary and v2 recovery.

**Architecture:** Pure v3 contracts and state transitions live in
`omicsops-agent`; SQLite/provider implementations live in adapters; a dedicated
Tauri harness module composes model, policy, tools, SSH, Kernel, MCP, and UI
events. Existing commands route by the frozen plan harness identifier.

**Delivery rule:** Every production behavior starts with a focused failing
test. Commit each independently reviewable subsystem. Never claim ignored live
acceptance as passed.

## Task 1: Core contracts, hash chain, reducer, and gates

**Files:**

- Create `crates/omicsops-agent/src/harness_v3/{mod,model,events,tools,context,review,engine}.rs`
- Create `crates/omicsops-agent/tests/harness_v3_contracts.rs`
- Modify `crates/omicsops-agent/src/lib.rs`

**Steps:**

1. Write compile-failing tests for all public v3 types, defaults, canonical
   event hashing, replay validation, completion evidence, uncertain effects,
   context compaction, and reviewer correction limits.
2. Run the focused test and confirm failure is caused by missing v3 behavior.
3. Implement the smallest serializable contracts and pure reducers that pass.
4. Add corruption, ordering, cancellation, and untrusted-source edge cases.
5. Run crate tests and `cargo fmt --check`; commit as core contracts.

## Task 2: SQLite event store and recovery

**Files:**

- Modify `crates/omicsops-adapters/src/persistence.rs`
- Create `crates/omicsops-adapters/tests/harness_v3_persistence.rs`

**Steps:**

1. Write failing migration/store tests for append-only events, indexes,
   snapshots, project/conversation cleanup, and legacy database migration.
2. Add `agent_run_events_v3` and `agent_run_snapshots_v3` transactionally.
3. Enforce monotonic append and previous-hash consistency inside a transaction.
4. Implement chain-verified replay and snapshot-as-cache semantics.
5. Test stale snapshots, tampered hashes, interrupted calls, idempotent success,
   and uncertain side effects; commit persistence.

## Task 3: Multi-tool provider protocol

**Files:**

- Modify `crates/omicsops-adapters/src/llm.rs`
- Modify model profile persistence/types where defined
- Create provider protocol tests alongside existing LLM tests

**Steps:**

1. Write failing fixtures for multiple tools, interleaved argument fragments,
   multiple call IDs, usage, cancellation, malformed arguments, and providers
   without native tool support.
2. Implement `ModelRequestV2` serialization for Anthropic,
   OpenAI-compatible, and Ollama while retaining the old request API.
3. Assemble each tool call independently and propagate provider usage.
4. Permit exactly one strict-JSON repair, then return a structured model error.
5. Add optional `context_window_tokens` with effective default 32,768 and test
   old profiles; commit provider support.

## Task 4: ToolRouter, built-ins, and MCP

**Files:**

- Add router/runtime modules under `src-tauri/src/harness_v3/`
- Reuse policy/SSH/Kernel/MCP entry points from existing modules
- Add focused Rust integration tests

**Steps:**

1. Write failing tests for schema rejection, traversal, allowlists, approval,
   revocation, timeout, cancellation, truncation, provenance, and concurrency.
2. Implement the shared schema-policy-approval-execute-validate-persist pipeline.
3. Add the eight built-ins; make writes project-relative and atomic, commands
   low-privilege, and previews 32 KiB per stream with full-log hashes.
4. Adapt configured MCP tools to `mcp::<server_id>::<tool>` and re-check enabled,
   declared, and approved state at invocation time.
5. Verify four-way read concurrency and serialized side effects; commit tools.

## Task 5: Context, coordinator, and reviewer

**Files:**

- Complete `crates/omicsops-agent/src/harness_v3/context.rs`
- Complete `crates/omicsops-agent/src/harness_v3/engine.rs`
- Add Tauri coordinator/reviewer modules and black-box scripted model tests

**Steps:**

1. Write failing small-budget tests showing that objective, criteria,
   unresolved errors, provenance, and artifacts survive compaction.
2. Implement 75% compaction, eight recent tool steps, source-labelled untrusted
   data, and deterministic fallback.
3. Implement the cancellable loop with 64 model-step, 48 tool-call, and
   idempotency limits.
4. Implement `agent.complete` evidence validation and isolated read-only
   reviewer output capped at eight findings.
5. Test two correction cycles and `needs_attention`; commit engine/reviewer.

## Task 6: Version routing and desktop APIs

**Files:**

- Modify `src-tauri/src/agent_commands.rs`
- Modify `src-tauri/src/commands.rs`
- Modify `src-tauri/src/lib.rs`
- Add `src-tauri/src/harness_v3/` modules and command tests

**Steps:**

1. Write failing routing tests for new `agent.harness_v3@3.0.0` plans and legacy
   `agent.remote_task@1.0.0` resume.
2. Make new plan generation freeze `AgentRunSpecV3`; preserve historical v2
   state and rendering.
3. Add v3 start/resume/cancel, paginated event query, user-answer, and stream
   contracts.
4. Verify restart recovery, SSH disconnect, uncertain results, and hash-chain
   failure are explicit states; commit desktop wiring.

## Task 7: Structured trajectory UI

**Files:**

- Modify `src/types.ts`, `src/DesktopApp.tsx`, and workspace components
- Add dedicated v3 trajectory components and Vitest tests

**Steps:**

1. Write failing UI tests for replay/live equivalence and sequence deduplication.
2. Render status, tool, approval, user-input, compaction, recovery, artifact,
   and reviewer cards without replacing the current workbench layout.
3. Preserve v2 event display and accessibility semantics.
4. Run targeted and full frontend tests plus production build; commit UI.

## Task 8: Acceptance and final verification

**Files:**

- Add scripted black-box harness acceptance tests
- Add ignored live SSH PBMC3k acceptance and environment documentation
- Update `acceptance/README.md` without embedding secrets or data

**Steps:**

1. Test complete plans, parallel reads, failure/repair, completion gate, reviewer
   correction, cancellation, restart, and malicious untrusted content with a
   scripted model and temporary runtime.
2. Create an ignored live test requiring explicit SSH/model environment. Start
   with an empty project and fail if repository PBMC workflow assets are called.
3. In a configured environment, verify dynamic input inspection, isolated
   environment, generated code, counts/QC/seed/version, H5AD, tables, plots,
   HTML, and artifact SHA-256 values. Otherwise report it as ignored.
4. Run fresh `cargo test --workspace`, `npm test -- --run`, `npm run build`, and
   `git diff --check`.
5. Inspect the branch diff and commit acceptance separately. Do not push or
   create a PR under this plan.

## Baseline recorded on 2026-08-16

- `cargo test --workspace`: passed; explicitly configured live tests ignored.
- `npm test -- --run`: 8 files, 46 tests passed.
- `npm run build`: TypeScript and Vite production build passed.

## Completion criteria

- New plans default to v3; existing v2 runs still resume and display.
- All model-visible state is reconstructable from a verified event chain.
- Native and approved MCP tools share one policy/approval/outcome contract.
- Context pressure cannot discard completion criteria or unresolved failures.
- Completion requires deterministic artifact/evidence checks and independent
  read-only scientific review.
- Local automated verification is green, and unconfigured live acceptance is
  explicitly ignored rather than represented as successful.
