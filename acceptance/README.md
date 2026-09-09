# OmicsOps acceptance workflows

`bulk-rnaseq.json` freezes the six-sample yeast comparison from
`nf-core/test-datasets` at commit `72a702d346833d5523bc40d032323ea548603b00`.
The paired FASTQ files live under `testdata/GSE110004/{accession}_{1,2}.fastq.gz`.

The automated suite validates the fixture, plan/tool contracts, migration,
approval hashing, audit chaining, shell quoting, process-group cancellation,
resource guards, validators, and recovery decisions. A real Linux SSH host is
still required to run the PBMC and bulk workflows end to end. Such runs must
verify h5ad/Seurat RDS or bulk tables/figures/report, capture the Micromamba
explicit lock, interrupt once during execution, and export the final run bundle.

The live V4 acceptance runs must use a low-privilege SSH account, a pinned host
key, an existing empty disposable remote directory, and explicitly configured
model credentials:

```text
OMICSOPS_LIVE_SSH_HOST
OMICSOPS_LIVE_SSH_PORT                 # optional, defaults to 22
OMICSOPS_LIVE_SSH_USER
OMICSOPS_LIVE_SSH_PASSWORD
OMICSOPS_LIVE_SSH_FINGERPRINT
OMICSOPS_LIVE_PBMC_ROOT                # must already exist and be empty
OMICSOPS_LIVE_MODEL_PROTOCOL           # anthropic | openai | ollama
OMICSOPS_LIVE_MODEL_BASE_URL
OMICSOPS_LIVE_MODEL_NAME
OMICSOPS_LIVE_MODEL_CREDENTIAL         # omitted only for Ollama
```

## Agent Runtime V4 stage-1 acceptance

`agent_v4::tests::live_v4_model_plan_and_persistent_ssh_python_kernel` uses the
same explicit live model/SSH variables listed above. It asks the real model for
an `ExecutionPlanV4`, validates its canonical SHA-256 contract, then launches a
run-scoped SSH JSONL Python kernel. Cell 1 defines `v4_probe`; cell 2 prints that
variable without redefining it. The test requires the same kernel session and
process identity for both cells.

Run only against an empty disposable remote root:

```text
cargo test -p omicsops-desktop live_v4_model_plan_and_persistent_ssh_python_kernel -- --ignored --exact --nocapture
```

An ignored result means the live SSH/model acceptance was not executed and must
not be reported as passed.

## Agent Runtime V4 stage-2 acceptance

Stage 2 adds typed provider retry handling, model/tool cancellation, bounded
tool-call and iteration budgets, repeated-call/result termination, four-way
read-only concurrency, per-project side-effect serialization, archive-first
context compaction, and crash recovery that never automatically repeats a
dispatched side effect whose outcome is missing.

An uncertain dispatch moves the run to `needs_attention`. After independently
checking remote state, record the conclusion with
`agent_v4_resolve_uncertain` (call id, resolution and non-empty evidence), then
call `agent_v4_resume`. The resolution is hash-chained and the original side
effect is never replayed automatically.

The Wisp-inspired loop increment additionally checks the complete provider
request before dispatch and after compaction, permits one smaller-request retry
on explicit provider context overflow, and rejects truncated or unterminated
responses. Runs with `agent.read_tool_result` can page original results by
run-scoped sequence/hash while the model sees bounded projections. The progress
guard compares completed outcomes (closed batches when present), preserving
changing job state as progress. These paths have deterministic fake-provider and
temporary-store tests; they still require separate live model acceptance.

For a manual smoke test, use a disposable project with a synthetic large text
result. Confirm its shortened model view can be paged back exactly; a reference
from another run must fail. Lower the test profile's context window and confirm
the original run enters needs-attention without tool dispatch after unsuccessful
compaction. Restore the real window, resume the same run, and confirm prior
outcomes remain available. Exercise normal text, a long answer that reaches the
output cap, and a provider disconnect. On both Windows and macOS, confirm no
unexpected local shell window or unrelated process cleanup occurs. Record actual
commands, provider model IDs, run IDs and results separately from unit tests.

For read-only delegation, create two disposable model profiles and select the
child in the main profile's **Read-only subagent model** setting. Start a new
ordinary Agent run and check its frozen child binding. Change the main profile's
selection, then resume: the existing run must retain its binding. Changing the
bound child's exact model or endpoint must block resume until restored. An
approved plan must retain its original behavior. With synthetic evidence, check
large-result failure retains the full trace, repeated recovery reuses successful
nodes and consumes the run's remaining delegation allowance, and cancellation
ends a pending child model/read wait. In settings, press Escape immediately after
opening the model form: only the form closes; the next Escape closes settings.
Run these checks on Windows and macOS and record results explicitly. They have
not been executed against real models as part of the deterministic tests.

Every Python/R cell writes complete stdout and stderr under
`.omicsops/runs/<run-id>/outputs/`. Events and model context contain only a
bounded head/tail excerpt plus byte count, SHA-256 and project-relative archive
path. Project environments live under `.omicsops/environments/<name>` and are
created only by the explicitly approved `runtime.environment.ensure` tool.

Run the deterministic suite first:

```text
cargo test --workspace
npm test -- --run
npm run build
```

Then run the real low-privilege SSH test using the same pinned-host variables
from stage 1. The disposable remote root must have Micromamba available; the
test creates `stage2-r`, executes a persistent R namespace, emits more than
100KB from Python, interrupts a long cell, and explicitly rebuilds a kernel:

```text
cargo test -p omicsops-desktop agent_v4::tests::live_v4_stage2_r_output_cancel_rebuild_and_project_environment -- --ignored --exact --nocapture
```

The live test is intentionally ignored during normal CI. An ignored result is
not a pass and stage 2 must not be accepted until it has completed against the
real acceptance host.

## Ordinary Agent guidance smoke (Windows and macOS)

After the deterministic tests, start an ordinary Agent run and submit guidance
while its model request is waiting. Confirm one received record becomes applied,
the same run continues, and Stop remains available. Repeat during a tool batch:
pending reads yield with a recoverable interruption while completed results remain visible; side-effecting operations finish before guidance is applied. Restart after acceptance and
resume the original run; confirm the guidance appears once. Retry an uncertain
submission without editing its text, switch conversations during submission, and
verify no duplicate or cross-run guidance appears. Approved plans must not expose
the input panel or accept the backend command. Applied means context consumption,
not task completion. Real model, SSH and macOS smoke have not been executed for
this change; automated tests use fake models and temporary SQLite databases.

## Runtime job identity smoke (Windows/macOS/SSH)

Use only the existing disposable acceptance environment. Execute a small synthetic
Python or R cell, verify its result retains the kernel-session reference and adds
a runtime-job reference. During another cell, stop the Agent or disconnect the
backend and resume the original run: verify the original dispatch requires
reconciliation and does not automatically execute again. An unknown record is
not proof that a remote process stopped. Check that normal subsequent cells with
new call IDs still share the intended interpreter session. These manual scenarios
have not been executed for this increment; deterministic tests use temporary SQLite
and a fake interpreter, without API keys or a real SSH host.

The in-process worker increment additionally requires checking that an ordinary
runtime cell still returns its original output/artifact references and Stop wakes
its wait promptly. Deterministic tests simulate a waiter timeout, reattachment to
the same manager, repeated reads, worker failure and capacity exhaustion. A waiter
reattachment is not an application-restart or SSH-reconnection acceptance test.

For saved-result recovery, the desktop should offer “Resume saved results” when
all unfinished dispatches have verified receipts and the inactive run reaches the
existing stale threshold. Confirm the original run continues, the action cannot
be submitted twice while busy, and the prompt disappears after tool results are
recorded. A missing receipt or explicitly uncertain dispatch must not expose this
shortcut. The exact interruption window is covered by deterministic receipt/store
fixtures; real desktop restart and remote-host smoke have not been executed.

For reasoning-effort smoke on Windows/macOS, edit an OpenAI-compatible profile,
select a documented effort for its exact service/model, save and reopen it; clear
it with Provider default. Edit a bound child independently and check the parent
selection remains intact. Test now sends the selected effort and allows 4096
output tokens (or the configured request budget). A passing connection probe
shows request acceptance only, not confirmed effective effort. Verify a frozen
child rejects changed effort on resume. Real model/gateway smoke has not been
executed; do not interpret deterministic request-shape tests as Luna/max service
acceptance. Anthropic/Ollama effort mapping and catalog-driven choices are pending.

For main-model freeze smoke on Windows/macOS, pause a new ordinary run, edit the
main profile's effort/model/endpoint, and attempt resume: it must report a frozen
configuration mismatch without continuing the model or opening a kernel. Restore
the original settings and resume the same run. Profile labels and keyring secret
rotation may change without invalidating the fingerprint. Old runs lacking the
field retain legacy behavior. This manual desktop/model smoke remains unexecuted;
deterministic protocol and temporary-database tests verify the binding logic.

Saved-result cancellation smoke (Windows/macOS): when a run offers Resume saved
results, click Cancel this run. Both actions should be disabled during submission;
completion should show Cancelled and remove both actions. A failed submission
remains retryable. Verify the original job and receipt remain auditable, and no
kernel/model is started by cancellation. A delayed resume must not overwrite the
cancelled state. Deterministic store/UI/desktop tests cover these transitions;
manual desktop, real-model and SSH cancellation smoke have not been executed.

For child guidance smoke on Windows/macOS, send guidance while an ordinary
read-only delegation is waiting for a child response or read. Completed evidence
should remain visible; pending child work should fail with guidance interruption,
its dependents should not run, and the parent should apply the guidance once and
replan. This is not a compute-job interrupt. Deterministic tests use pending
futures and notifications; real-model/SSH/manual desktop smoke remains unexecuted.


### Compiled catalog snapshot smoke (Windows/macOS; manual, not yet executed)

Create a known official model profile and confirm Settings shows its saved
models.dev context/output limits. Edit only its label and confirm the frozen
capabilities remain unchanged. An unknown gateway or longer sibling ID must not
inherit the official entry. Explicit unsupported reasoning effort or a context
above the catalog limit must be rejected before credentials are written. A legacy
profile must retain its original context/vision/tools on edit. Select the catalog refresh option when editing to adopt the bundled catalog.
Confirm the option is unchecked after reopening the editor, failed refreshes keep
the editor available for retry, and Escape immediately closes only the editor.
Check a changed capability/budget refuses frozen-run resume; an unchanged
effective configuration must retain its fingerprint. No real API key or network is needed for saving
these configurations; live model inference is a separate opt-in acceptance test.
