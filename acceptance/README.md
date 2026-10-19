# OmicsOps acceptance workflows

## Bundled helper regressions and evidence boundaries

Run these deterministic standard-library checks from the repository root:

```text
python -B -m unittest discover -s scripts -p "test_bundled_skill_helpers.py" -v
python -B -m unittest discover -s scripts -p "test_*.py" -v
```

The unified discovery includes bundled helpers, model catalog, remote job bridge
and kernel inspection. The helper suite loads only the three fixed bundled
runtime sources (`paper-narrative`, `literature-review`, `figure-style`), using
isolated namespaces, fake credentials and HTTP/plotting fixtures. It requires
all three nonempty behavior groups and checks that a controlled local failure
produces an unsuccessful unittest result. No pip install, network, real key,
SSH, R or GPU is needed; crop tests return coordinates without writing images.

Bundle/hash PASS proves packaged source identity and integrity. Helper PASS
proves behavior under controlled inputs. Scripted V4 PASS proves only the runtime
contracts covered by the executed model/tool fixtures. None establishes live
provider access, all 26 Skills' dependencies or scientific validity. Helper
results do not enter V4 model call counts or suite hashes. The W08 deterministic V4 scenario
manifest, runner and report are delivered; see the
[scenario commands and evidence boundaries](../docs/testing/v4-scenarios.md).
Deterministic delivery does not establish real-service or native acceptance.

Local checks and the updated GitHub CI must be reported separately. CI targets
Windows, Python 3.12 and Node 22; editing the workflow is not a CI pass. For this
helper increment, live model/SSH/R/GPU/PBMC and native UI acceptance are not
executed. Existing historical live results below retain their original scope.
Run opt-in live tests only with the explicitly configured disposable environment,
after the default workspace, frontend and build checks. Record actual PASS,
FAIL and not executed/ignored separately; ignored tests are not live acceptance.

## Subscription model acceptance (Windows, opt-in)

Run the deterministic workspace, frontend and build checks first. The new tests
are ignored by default and require both `OMICSOPS_LIVE_SUBSCRIPTION_DISPOSABLE=1`
and `OMICSOPS_LIVE_SUBSCRIPTION_PROFILE_ID`, the UUID of **one** explicitly chosen
saved subscription profile. Run each provider separately; do not automatically
enumerate or reuse every account. No additional credential environment variables
or auth-file imports are supported.

The selected row is read from `%APPDATA%/io.omicsops.desktop/omicsops.db` through
a read-only SQLite connection. Its existing keyring reference and, for Codex,
host-owned account binding are retained in an in-memory temporary Store. Codex
authentication may refresh/rotate the selected credential through the existing
keyring coordinator; it never writes tokens to SQLite. Claude authentication
remains owned by the official native CLI. No existing project is opened or changed.

The preflight requires the precise provider/endpoint pair: `open_ai_codex` at
`https://chatgpt.com/backend-api`, `claude_code` at `claude-code://local`, or
`open_ai_responses` at `https://opencode.ai/zen/go/v1`. Go additionally requires
one of the six reviewed complete Responses IDs. A missing binding/credential,
wrong provider, nonempty/foreign project root, or unverified Claude policy fails
before model generation. Profiles retain their saved budgets and options; unknown
capabilities remain conservative and can reject an oversized request before send.

Each nonce test creates its own empty temporary directory, then adds one random
nonce file. The prompt names that file without the nonce. Production Agent V4 must
execute `project.read` once, return the nonce with host evidence, and persist
`RunCompleted`. Delegation, auto-review, memory and model retries are disabled for
this bounded fixture. No SSH or scientific workflow is exercised.

```powershell
cargo test -p omicsops-desktop --lib agent_v4::subscription_live_acceptance_tests::live_codex_subscription_reads_nonce -- --ignored --exact --nocapture
cargo test -p omicsops-desktop --lib agent_v4::subscription_live_acceptance_tests::live_claude_subscription_reads_nonce -- --ignored --exact --nocapture
cargo test -p omicsops-desktop --lib agent_v4::subscription_live_acceptance_tests::live_go_responses_reads_nonce -- --ignored --exact --nocapture
cargo test -p omicsops-desktop --lib agent_v4::subscription_live_acceptance_tests::live_claude_subscription_windows_stop -- --ignored --exact --nocapture
```

**Claude production generation remains disabled.** Official read-only version and
subscription status checks cannot establish all effective managed hooks before
session startup. The two Claude live tests currently fail preflight if explicitly
run; compilation or an ignored result is not acceptance. The stop test requires
an eligible, active official generation before aborting its request; descendant
termination is separately covered by deterministic Windows Job Object fixtures.
Do not bypass the policy gate to obtain a live test pass.

As of 2026-10-01, all four new live tests and native GUI smoke are **not executed**;
no disposable subscription configuration was supplied. Record actual commands,
exit codes and elapsed time here after any explicit live run, without credentials,
raw reasoning, account identity or sensitive host output.

Native Windows smoke steps (not yet executed): configure/sign in to Codex, cancel
before authorization, explicitly save after authorization, edit a saved Claude
native executable and inspect its status/gate, switch Go across all three
protocols, restart and verify the selected profile, complete one read-tool round,
deny deletion of a temporary file and confirm it remains, press Escape immediately
after opening each layer, then stop generation. A stopped Agent does not cancel an
already dispatched SSH job. macOS, SSH, PBMC and scientific provenance acceptance
are also not executed for this change.

Codex login regression (2026-10-01): a public device-code start was diagnosed on
Windows, without account authorization or credential reads/writes. The old
native client returned HTTP 403 / the fixed unsupported-region category while a
system-network request succeeded. Enabling reqwest system-proxy made the same
CodexDeviceAuth begin call succeed. This verifies device-code acquisition only;
GUI login, token exchange with a real account and model generation remain
unexecuted. Deterministic wire fixtures cover authorization exchange separately.

For the GUI smoke, restart the rebuilt app after changing a Windows system proxy.
With HTTP(S)_PROXY unset and a configured system proxy, open a new Codex profile
and start login while the model field is still blank: the official link and code
should appear. Authorize only the disposable account selected for acceptance,
fill its full model ID, then explicitly Save. Also verify failed/expired state
guidance and retry, immediate Escape cancellation, and that no raw provider body
or HTML appears. Do not record the real device code or credential material.

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

## OpenCode Go ordinary Agent acceptance (Windows, opt-in)

The separate subscription checks below cover Codex and Go Responses. The older
Go Chat acceptance in this section does not validate those transports.

This ignored test validates a real production `model -> project.read -> model`
exchange, rather than the smaller connection probe. Run the deterministic suite
first. In the same Windows user session as OmicsOps, set
`OMICSOPS_LIVE_MODEL_PROFILE_ID` to the UUID of an existing saved profile whose
exact protocol, endpoint, and model are `open_ai_compatible`,
`https://opencode.ai/zen/go/v1`, and one of `glm-5.3`, `glm-5.3-flash`, or
`deepseek-v4.1-flash`. These are exact allowlisted IDs; aliases, prefixes and longer
sibling IDs are rejected. Its API key must already exist in Windows Credential
Manager, and the profile must not impose a context bound below the compiled
1,000,000-token catalog limit.

The test opens only that profile row from
`%APPDATA%/io.omicsops.desktop/omicsops.db` through a read-only SQLite connection.
It explicitly rebuilds the profile in a temporary Store with the current trusted
catalog snapshot (1,000,000 context; 131,072 output for the two GLM IDs and
384,000 output for the exact DeepSeek ID), while retaining the saved
keyring reference. It never opens the user database through `Store`, writes to the
user database, prints the credential, uses SSH, or touches an existing project.
The temporary project contains one random nonce file; the prompt names the file
but does not contain the nonce. The production V4 composition and ordinary Agent
loop must read it once and return the nonce in the final completed answer.
The same fixture observes ephemeral reasoning previews from the real provider
stream. It prints only snapshot counts, byte lengths, attempt counts, and elapsed
milliseconds since the first model request; it never prints or persists reasoning
text. For `glm-5.3`, at least one nonempty preview must arrive before that model
attempt's result event.
The `glm-5.3-flash` profile reports the observation without requiring reasoning,
because a valid response may omit it.
The DeepSeek profile also reports observation counts without requiring reasoning
previews. It retains the saved reasoning effort; at `max`, the application output
reservation is 65,536 tokens, subject to the exact catalog and context bounds.
The profile and its configuration hash are frozen only in the temporary Store;
the original conversation and its historical runs are not resumed or modified.

```text
cargo test -p omicsops-desktop agent_v4::go_live_acceptance_tests::live_opencode_go_agent_reads_file_and_returns_nonce -- --ignored --exact --nocapture
```

On 2026-09-21, after aligning the production model timeout, the exact command
above also passed 1/1 in 31.57 seconds using a saved `glm-5.3-flash` profile and
the production default timeout. This isolated nonce/read-tool acceptance does
not rerun the literature workflow or validate scientific provenance end to end.
Deterministic checks passed: `cargo test --workspace` (1,254 passed, 12 ignored),
`npm test` (789 frontend and 22 bridge tests), `npm run build`, and
`npm run build:desktop`. The ignored tests in the workspace count are not live
acceptance passes; the explicit Go result above is recorded separately.

The run is bounded to four model turns, three tool calls, no model retries, and
the production 180-second meaningful-content idle timeout plus 15-minute hard
total per ordinary model attempt. It is ignored
during normal CI. A compiled, ignored, or unexecuted result is not a live
acceptance pass; record the actual command and result separately.

On 2026-09-21, the exact command above was run on Windows with
`OMICSOPS_LIVE_MODEL_PROFILE_ID` set to an existing saved OpenCode Go `glm-5.3`
profile and passed 1/1 in 110.50 seconds. The durable event chain contained two
real model requests around one successful `project.read`, the returned random
nonce appeared in the completion answer, and the run recorded `RunCompleted`.
This live fixture intentionally exposes only the read-only file tool plus Agent
coordination tools. The separate deterministic budget regression covers the full
built-in tool schema in an initial request larger than 150 KB. Neither result is
an SSH, miRNA, or general scientific-workflow acceptance.

On 2026-09-25, the exact ignored command above passed 1/1 in 110.05 seconds on
Windows with an existing saved `glm-5.3` profile. The real provider-to-Agent
stream produced 402 nonempty transient reasoning snapshots across three model
attempts; all 402 arrived before their result event. The largest snapshot was
16,917 bytes, the first and last arrived 2,199 and 104,807 milliseconds after
the first model request, and the preview was cleared three times. These are
metadata counts only; the test printed or persisted neither reasoning text nor
credentials. Deterministic checks also passed: `cargo test --workspace`
(1,299 passed, 12 ignored), `npm test` (890 frontend and 22 bridge tests),
`npm run build`, and `npm run build:desktop`. A separate browser check of a
synthetic WorkspaceShell fixture confirmed the Thinking panel appears outside
the collapsed trace and can be collapsed manually. That browser check is not
native desktop or live model UI acceptance; the live test does not cover SSH or
a miRNA workflow.

After the stream-deadline correction on 2026-09-25, the same isolated fixture was
explicitly executed with the existing saved `glm-5.3` profile and its `max`
reasoning effort. This command reused the workspace test build; the exact test
filter selected only the OpenCode fixture and no other ignored live tests:

```text
cargo test --workspace agent_v4::go_live_acceptance_tests::live_opencode_go_agent_reads_file_and_returns_nonce -- --ignored --exact --nocapture
```

It passed 1/1 in 139.96 seconds (exit 0), with 2,072 nonempty reasoning snapshots
across three attempts, all observed before the corresponding result. The first
and last snapshots arrived at 1,737 and 136,617 milliseconds; the largest preview
was 16,594 bytes and three clear updates were observed. The production Agent read
the temporary nonce file exactly once, returned that nonce, and completed.
Only metadata was printed; credentials and reasoning text were not recorded.
This run validates the real provider/tool/completion path, not a live request
lasting beyond 180 seconds or the original miRNA research workflow. Deterministic
tests cover active streams past the former deadline, silence, empty/usage/retry
events, the hard cap, cancellation, partial tool calls, and unchanged summary
deadlines. Final checks passed: `cargo test --workspace` (1,306 passed, 12 ignored),
`npm test` (893 frontend and 22 bridge tests), `npm run build`,
`npm run build:desktop`, and `cargo fmt --all -- --check`. No SSH acceptance was run.

On 2026-09-26, after matching output reservations to selected reasoning effort,
the exact ignored OpenCode command above was executed with the saved
`deepseek-v4.1-flash` profile and its `max` effort. The first isolated run failed
after 172.16 seconds because the provider response stream ended after partial
output (`error decoding response body`). A second fresh temporary run passed
1/1 in 67.62 seconds (exit 0), read the nonce file exactly once, returned the
nonce, and recorded `RunCompleted`. Both outcomes are retained; the pass is not
a claim that provider stream failures or arbitrary long-output truncation are
eliminated. No original research project or user database was modified.

Deterministic checks passed: `cargo test --workspace` (1,356 passed, 12 ignored),
`npm test` (944 frontend and 22 bridge tests), `npm run build`,
`npm run build:desktop`, and `cargo fmt --all -- --check`. This small live test
does not validate the original miRNA literature workflow, SSH, or native UI.

After the incomplete-stream retry classification and scientific code projection
changes on 2026-09-26, the same exact ignored OpenCode command passed 1/1 in
171.95 seconds (exit 0) using `deepseek-v4.1-flash` with `max` effort. The test
read its temporary nonce file and completed; no original project or saved
configuration was changed. EOF recovery was tested separately with local SSE
and bounded-retry fixtures, because this live run did not inject a disconnect.
This small task is not a timing comparison against the original literature run.

Deterministic checks passed: `cargo test --workspace` (1,364 passed, 12 ignored),
`npm test` (944 frontend and 22 bridge tests), `npm run build`, and formatting.
`npm run build:desktop` was blocked when replacing the executable: Windows
reported access denied while two running OmicsOps instances held that file.
The existing executable therefore remains the old build until those instances
exit and the build is rerun. No application was forcibly terminated.

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

The desktop presents an uncertain dispatch as a failed tool/run, without a
mandatory side-effect verification form. The original uncertain dispatch and
unknown job state remain in the audit trail: a failed wait is not proof that
remote execution stopped, and the original side effect is never replayed
automatically. The host reconciliation API remains available for independently
verified recovery; changing presentation does not manufacture a resolution.

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
npm run build
cargo test --workspace
npm test -- --run
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
backend and inspect the original run: verify it displays a failure without a
verification form and does not automatically execute again. An unknown record is
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


## Detached remote job reconnection (opt-in Linux SSH)

Windows deterministic UI observation regressions (mocked native commands, fake
timers; no SSH, model, environment probe or Agent start):

```text
npx vitest run src/features/workspace/RuntimeDialog.test.tsx src/features/workspace/WorkspaceShell.test.tsx src/features/workspace/ResearchOverlays.test.tsx src/features/workspace/useRemoteJobs.test.tsx src/features/workspace/RemoteJobsPanel.test.tsx
```

Optional Windows desktop smoke with an existing disposable Linux SSH job: open
Python/R environment -> Remote jobs. Confirm current conversation metadata and
All project jobs use only the local list; unsupported backends safely show an
empty list when no records exist. A frozen run still exposes this read-only entry
without changing its compute selection. Immediately press Escape: only Remote
jobs closes; a second Escape closes Runtimes. Select one original job and manually
refresh. Explicitly enable automatic queries to check serial 2/4/8/16/30-second
backoff and at most 10 attempts; close, switch project/conversation/binding, or
reach a terminal state and confirm no subsequent query. Reopening or restarting
must not restore the window. Stop local queries never cancels remote compute,
resubmits code or resumes the Agent. Unknown/timeout/unavailable is not proof of
remote failure. Retained receipt verification is local and corrupt receipts must
reject without SSH fallback. Excerpts (4 KiB per stream) and safe references
(at most 128) are bounded observations, not complete scientific evidence.
These manual steps and the optional live test below are not established by a
build or fake tests; record them as not executed unless actually performed.

The deterministic tests reopen a temporary SQLite store, use a fresh mocked
transport and Python subprocess stubs, and verify no resubmission. Run them with:

```text
cargo test -p omicsops-desktop --lib remote_job
python -B -m unittest discover -s scripts -p "test_remote_job_bridge.py"
```

Real acceptance requires the existing `OMICSOPS_LIVE_SSH_HOST`, `_PORT` (optional),
`_USER`, `_PASSWORD`, `_FINGERPRINT` variables plus
`OMICSOPS_LIVE_REMOTE_JOB_ROOT`, an existing empty disposable absolute directory.
It needs a low-privilege Linux host with Python 3 and detached process support;
no model/API key is used. The test rejects a nonempty directory, submits a job
that exclusively creates a counter then sleeps, explicitly disconnects SSH,
reopens the local store, reconnects and verifies a single execution and completion.
It leaves remote output in that disposable root for inspection.

```text
cargo test -p omicsops-desktop --lib remote_jobs_v4::tests::live_remote_job_survives_disconnect_and_store_reopen -- --ignored --exact
```

This test is ignored by default. Do not describe it as passed unless explicitly
executed with the disposable environment. Manual desktop smoke: submit a background
job, close the app before completion, reopen the same project/SSH binding, ask the
Agent to list remote jobs and reconnect to its ID. Verify finished output and that
no new execution occurred. Changing project, backend, root or trusted host key must
refuse the old identity. Do not resubmit to recover an unknown job. Agent Stop and
run cancellation do not cancel detached computation. R/Micromamba require separate
live environment acceptance; Windows/macOS client tests do not imply those passed.

## 2026-09-27 approval and MCP directory regression

After narrowing the runtime path-metadata approval check and reducing MCP directory
model views, deterministic checks passed: `cargo test --workspace` (1,372 passed,
12 ignored, 0 failed), `npm test` (944 frontend and 22 bridge tests),
`npm run build`, `npm run build:desktop`, and `cargo fmt --all -- --check`.
The desktop executable was rebuilt successfully; the previous locked-file build
failure is no longer the latest build result.

The exact ignored OpenCode nonce/read-tool test documented above was explicitly
executed with the saved `deepseek-v4.1-flash` profile and unchanged `max` effort.
It passed 1/1 in 129.61 seconds, using a disposable project and a read-only profile
lookup. No user project, user database, or credential was changed. This verifies
the small production model/tool loop, not the original miRNA research workflow,
real MCP performance, SSH, or a native UI smoke test. Deterministic directory
tests compare the same 247-tool fixture: 34,219 bytes/7 pages for the former
description preview and 6,346 bytes/2 pages for the new name index.

For manual smoke, start the newly built app and a new RiskBased run. A script
using explicit `os.path.join/getsize` with proven project-relative paths should
not request approval solely for `import os`; opaque paths, aliases, and dangerous
operations still require approval. Existing pending exact-call approvals retain
their recorded semantics. Search the MCP directory, refine or clear its query,
retrieve one exact schema, and confirm compaction preserves the same signed
directory reference. These native UI and original-workflow checks remain unrun.

## 2026-09-28 approval and provider tool identity validation

Deterministic checks passed: `cargo test --workspace` (1,381 passed, 12 ignored),
`npm test` (945 frontend and 22 bridge tests), `npm run build`, and
`npm run build:desktop`. Formatting was applied in a separate commit;
`cargo fmt --all -- --check` then passed. The new local Windows executable
contains the project-relative `os.path.exists` approval correction and stable
provider tool names with one bounded invalid-call repair attempt.

The first live-test invocation used the workspace filter and failed before any
model request: the Windows linker returned exit 1 while linking an adapters
unit-test target. No live acceptance pass is claimed for that invocation.
The documented desktop-only command was then run explicitly:

```text
cargo test -p omicsops-desktop agent_v4::go_live_acceptance_tests::live_opencode_go_agent_reads_file_and_returns_nonce -- --ignored --exact --nocapture
```

It passed 1/1 (exit 0) in 70.11 seconds with the saved `deepseek-v4.1-flash`
profile, using read-only profile selection, its existing keyring credential,
and a disposable project. Two attempts produced content; 1,104 nonempty transient
reasoning observations arrived before their result. Only observation metadata was
printed, never reasoning text or credentials. The production Agent read the
random nonce file and returned its contents successfully.

This validates the real provider/tool/completion path with the changed tool
identity mapping. Unknown-name recovery, whole-response non-dispatch, shared
repair budget and risky path rejection were tested deterministically, not
injected into this live run. The original miRNA task, SSH, and native UI manual
acceptance were not run. The 70.11 seconds is not a before/after literature-task
speed comparison; the original run's event analysis is recorded in
`docs/superpowers/specs/2026-09-28-approval-and-tool-identity.md`.

## 2026-09-28 follow-up approval diagnostics and execution status

The follow-up screenshot's stored and frozen compute selections both deserialize
to RiskBased. Static analysis of the exact runtime arguments found two remaining
rejection points: directory creation and the date-derived output path. Expanding
automatic approval for these patterns remains pending explicit risk-aware user
authorization after automatic review rejected the production changes. No such
expansion was applied, and no historical approval was changed.

The delivered changes distinguish model waiting, requested tools, running tools,
and delegated execution progress, and add bounded, non-sensitive host approval
reasons. The existing approval classifier and authorization results are unchanged.
Final deterministic checks passed: `cargo test --workspace` (1,389 passed,
12 ignored), `npm test` (953 frontend and 22 bridge tests), `npm run build`,
`npm run build:desktop`, and `cargo fmt --all -- --check`. Formatting has its own
commit. Native compilation and local NSIS bundling succeeded; nothing was
published or distributed.

After these checks, the exact desktop-only ignored OpenCode command shown above
passed 1/1 (exit 0) in 12.86 seconds of test execution with the existing
`deepseek-v4.1-flash` profile and keyring reference. It used a disposable nonce
project and read-only profile lookup. Two attempts produced content; all 121
nonempty reasoning snapshots preceded the corresponding result. Only metadata
was printed. This is a small model/read-tool/completion acceptance, not a timing
comparison or acceptance of runtime auto-approval, SSH, native UI, or the original
miRNA research workflow. The original script was not rerun.

The actual research input uses serial queries, a 90-second network timeout and
up to four attempts; Python output is buffered until execution finishes. The UI
fix therefore reports unknown tool progress without claiming the computation
is stuck or faster. Detailed evidence, remaining authorization, and unexecuted
manual smoke steps are recorded in
`docs/superpowers/specs/2026-09-28-runtime-approval-paths.md` and
`docs/superpowers/plans/2026-09-28-runtime-approval-paths.md`.

## 2026-10-04 Go DeepSeek low and context efficiency

Deterministic checks passed (exit 0): `cargo test --workspace` (1,509 passed,
0 failed, 16 ignored), `npm test` (992 frontend and 22 bridge tests),
`npm run build`, `npm run build:desktop`, `cargo fmt --all -- --check`, and
`git diff --check`. The Windows release and local NSIS bundle were built;
no application was installed or distributed. Regression fixtures cover MCP
input-shape preflight, recoverable local argument errors versus server failures,
and bounded Runtime projections with retained artifact facts and exact signed
result recovery.

After those checks, the exact desktop-only command was explicitly run:

```text
cargo test -p omicsops-desktop agent_v4::go_live_acceptance_tests::live_opencode_go_agent_reads_file_and_returns_nonce -- --ignored --exact --nocapture
```

The selected existing `deepseek-v4.1-flash` / Go Chat profile retained the
user-authorized saved `low` effort, confirmed again through a read-only lookup.
The test used its existing Windows keyring reference, a temporary project, and
an in-memory Store. It passed 1/1 (exit 0) in 6.79 seconds of test execution;
test-target recompilation took an additional 1m 46s. Two model attempts surrounded
exactly one successful `project.read`; the final answer returned the random nonce
and the production loop recorded `RunCompleted`. All 12 nonempty transient
reasoning observations preceded their result, with a largest preview of 251 bytes
and first/last observations at 2,405/6,016 milliseconds after the first request.
Only metadata was printed, never reasoning text or credential contents.

This bounded nonce acceptance confirms the real low provider/tool/completion
path. It is not a paired timing comparison of the original literature task and
does not exercise the new MCP or Runtime behavior with scientific data. SSH,
scientific provenance and native UI research acceptance remain unexecuted.
The read-only historical diagnosis and the remaining manual smoke steps are in
`docs/superpowers/specs/2026-10-04-go-agent-latency-diagnosis.md`.

## W04：一次性本地 R 内核对象检查（显式 opt-in）

### UI 确定性回归与手工 smoke

```powershell
npx vitest run src/features/workspace/RuntimeDialog.test.tsx src/features/workspace/useKernelInspection.test.tsx src/features/workspace/WorkspaceShell.test.tsx
python -B -m unittest discover -s scripts -p "test_*.py"
npm run build
cargo test --workspace
npm test
npm run build:desktop
cargo fmt --all -- --check
git diff --check
```

UI 使用模拟命令验证实际 WorkspaceShell → RuntimeDialog 链路：从当前或历史运行的
“运行环境”进入“查看内核元数据”，打开本身不查询，点击“刷新元数据”才调用只读
`agent_v4_inspect_kernel({ request })`。请求只有 project/conversation/run/language/
expected_session_id；不探测环境、调用模型、准备计算或更改冻结配置。W03 远端作业
保留独立来源与查询生命周期，不把作业状态当内核存活证明。

Windows 原生 UI smoke 尚未执行：在一次性项目的存活 V4 Python/R 运行中打开上述
入口并手动刷新，检查名称、受控类型、维度、元素数、摘要、来源及截断标记；未知
尺寸保持未知，不显示值或 repr。执行占锁时应显示 Busy，历史/未初始化内核显示
未启动/当前不可观察，不启动解释器或新 SSH。切换语言、运行、项目、会话或关闭
后，延迟的旧对象不能出现；重建 session 后旧响应应拒绝，下一次刷新仍需手动。
打开后立即从 window/body 按 Escape 只关闭顶层元数据面板并恢复父级入口焦点，
第二次关闭 Runtime。Tab/Shift+Tab 保持在顶层，刷新禁用按钮时焦点仍有归属。
同时验证远端作业入口可独立打开与关闭，冻结运行不出现准备环境操作。

Ready 是临时内存元数据快照，不是科研证据；结果不写事件、日志、SQLite、模型
上下文、Artifact 或导出，也不增加暂停保活。观察不会取消/重跑计算。Windows
编译、确定性 native pipe 测试及 NSIS 构建不代表安装、真实 GUI 或 Runtime
端到端验收；本轮不运行或分发安装包。Linux/macOS native readiness 分支未编译、
未执行，不作平台保证。readiness 只检查当下可观察 queued 数据，不能保证拦截
检查后任意新帧。真实 R fixture（以下命令）、SSH、模型、容器和科学数据验收
均未执行，默认 ignored 不算通过。W04 确定性实现与独立审阅已完成；W08 确定性场景也已交付，
见[场景说明](../docs/testing/v4-scenarios.md)。这些结果不替代上述真实验收。

### 一次性 R worker

先完成默认确定性检查，准备独立的一次性本地目录与已安装 `Rscript`、`jsonlite` 的环境。此 ignored fixture 使用临时目录启动一个本地 SSH typed worker 脚本，检查 active binding、delayedAssign、基础矩阵及 S3/S4 对象；不连接 SSH，不读取真实项目，不安装依赖。

```powershell
$env:OMICSOPS_LIVE_R_INSPECTION_DISPOSABLE = '1'
cargo test -p omicsops-adapters --lib kernel::kernel_inspection_tests::live_r_inspection_bindings_have_no_side_effects -- --ignored --exact
Remove-Item Env:OMICSOPS_LIVE_R_INSPECTION_DISPOSABLE
```

必须记录实际命令和结果。默认测试中的 ignored 不代表通过；未显式执行时记录“未执行”。mock transport 和 Python AST helper 测试不证明真实 R 或 SSH 端到端验收。检查只返回基础元数据，不强制 promise 或调用用户方法；未求值表达式与 S3/S4 保守保持 opaque。SSH readiness 只检查当前 buffer/russh 可观测队列，不保证 clean 检查后任意网络帧不会到达。已派发检查/执行的等待被取消时会话不再复用，不表示远端计算已取消。

## Paused ordinary Run manual context compaction (W05)

Deterministic regression commands (no provider credentials or SSH):

```powershell
npx vitest run src/features/workspace/useContextCompaction.test.tsx src/features/workspace/ContextUsagePanel.test.tsx src/features/workspace/WorkspaceShell.test.tsx
cargo test -p omicsops-desktop --lib manual_compaction_reopen_answer_resume
```

The Rust regression commits through the actual host flow into a disposable file
SQLite database, closes and reopens it, validates and persists an explicit answer,
and runs the ordinary core with the frozen desktop composition and a scripted
provider transport. It checks the same durable checkpoint, unchanged source
chain and paginated original evidence. This is deterministic recovery evidence,
not a live model/SSH or native GUI acceptance result.

Windows manual smoke (record separately as passed, failed or not executed):

1. Use a disposable project and ordinary Agent run paused on exactly one question.
   Type an answer draft, open `/context`, then compact. Double-click and close /
   reopen the panel while pending: at most one operation should be in flight.
   Confirm a success or no-change receipt, unchanged question, draft and visible
   history. No answer, resume, compute initialization or SSH connection should
   occur. Usage refresh failure must not turn a confirmed receipt into a failed
   compaction. Bytes are admission/context sizes, not provider token usage.
2. Retry an unconfirmed failure: keep the same request identity. After a known
   success, another explicit click begins a new operation. Switch conversation,
   project or run while pending: the late receipt/error must not appear in the
   new scope; returning while the original request is pending must not duplicate
   it. Closing the UI does not cancel the host transaction.
3. Open the usage panel over an existing closeable layer and immediately press
   Escape on the window: only the top panel closes. Press again to close its
   parent. Test both docked and floating panel modes without moving focus inside.
4. Confirm planning / approved-plan / running / stopping / multiple-question /
   tool-approval states refuse compaction. The host also rejects an active driver,
   pending guidance, unresolved side effects, changed frozen bindings and
   insufficient full-request budget; retain the question and original evidence.
   Do not interpret `unknown` dispatch as cancelled or solve it by compacting.
5. After successful compaction restart the disposable app, reopen the same run,
   explicitly answer and resume, and inspect original evidence through its
   recorded references. New answers, rule files and scientific changes require
   fresh resume admission; the compaction receipt certifies only its snapshot.
   Compare the archive/checkpoint and frozen specification identity before and
   after recovery. A deterministic checkpoint does not replace original evidence.

Run `npm run build` first to generate `dist`, then `cargo test --workspace`, `npm test`,
`npm run build:desktop`, formatting and whitespace checks before delivery. A built
installer is not an installation or GUI smoke pass. Real provider/SSH/R/Micromamba
or PBMC workflows require the disposable-environment procedures above and an
explicit recorded run; ignored tests, this scripted transport and successful
builds must never be reported as that acceptance.


## W07 offline synthetic example / 离线合成示例

Deterministic UI/host tests and build results are recorded separately from native
smoke and real scientific acceptance. The fixture is original OmicsOps content,
`synthetic-evidence-report-v1`, `source_kind=synthetic`, AGPL-3.0-only. It creates
only a local Blank/Ready Project, fixed files and a source marker. Viewing does not
execute Python/R, contact a model/MCP/SSH service, or create production conversations,
Runs, approvals, Evidence or Agent events. Global settings can still read local
model/Skills/MCP/connection metadata. A previously identified ordinary workspace
may remain visible during a cancellable source/usage lookup; its earlier authorized
work may continue. Only the candidate is guaranteed to have zero initialization.

Windows native manual smoke (use an isolated disposable app-data directory and
local parent folder, no live credentials; record actual passed/failed/not executed):

1. With credentials absent and networking unavailable, open Project Library's
   **Offline example / 离线示例**. Immediately press window Escape: only this top
   creation layer closes. Reopen, cancel the parent-folder picker: zero creation.
   Choose an existing local parent and create; no ordinary remote-default flow.
2. Confirm bilingual synthetic/preset/not-executed labels, four sections and seven
   files. Open observations, summary and both reports. Check A = 2, 4; B = 5, 7;
   preset means 3 and 6, difference 3. No conversation, model/compute/SSH startup or
   Agent dispatch is permitted for the example. Hashes only compare file equality.
3. Restart that isolated app and open the example via startup, Project Library,
   exact search/source and usage navigation where available. Inapplicable production
   targets must show the synthetic view with a target-unavailable notice. During a
   deferred lookup, cancel navigation: keep the identified old workspace/draft and
   discard late responses. No candidate initialization before marker None.
4. Edit a fixed text file to include literal HTML/Markdown remote image/link/script/
   iframe strings. Reopen and see **Changed / 已更改**, current user text and a changed
   hash, with zero remote resource requests. Delete another fixed file and reopen:
   **Missing / 缺失**, no preview or automatic repair. Over-limit/unreadable files show
   **Unavailable / 不可用** rather than a truncated complete-file claim.
5. Delete `.omicsops/offline-example.json`, restart/reopen: retain the synthetic
   identity, display receipt-missing unavailable, no file reads/previews and zero
   production initialization. Corrupt/rebind root identity similarly fails closed.
6. Inject a registration failure in a disposable host: saved directory remains;
   retry within the same open creation panel uses the same UUID. Close/reopen starts
   a new explicit request. A conflicting or user-edited target must not be overwritten.

中文：示例/捆绑哈希/三个 helper/脚本 V4 的成功各自只证明覆盖的契约，不代表真实
科学结果或全部依赖可用。changed 文件是当前用户文本，不能继续称作原始预置结果。
普通模型、MCP 或外部服务仍可能接收提示词、结果或元数据；此示例的离线能力不是
全产品的数据不出本机保证。

2026-10-11 status: W07 native GUI creation/restart/changed/missing/receipt smoke
**not executed** in this task. Computer-use was stopped by the user; no application,
installer or real database was launched through a substitute route. The separate
older-HEAD W06 harness is not W07 evidence. Real provider, SSH, R, Micromamba, GPU,
SLURM and PBMC scientific workflows **not executed**. Default ignored tests, mocks,
web/native builds and installer generation are not a native or live acceptance pass.


2026-10-11 deterministic gate results: `cargo test --locked --workspace` passed
(1777 passed, 0 failed, 18 ignored); `npm test` passed (1316 Vitest + 22 extension
Node tests); `npm run build` and normal `npm run build:desktop` passed. Final
Task4 closest three-file suite passed 157/157; `npx tsc -b --force`,
`cargo fmt --all -- --check` and `git diff --check` passed. Rust cache/dead-code/
linker and Vite chunk-size warnings are retained in the task report. No locks
changed, so `npm ci` was not required. Only the later fail-fast test helper was
retested; product code matches full gates, with a separately byte-proven removal
of one extra EOF newline from the type-only DTO file. Native app/bundle generation
is recorded as build evidence; no W07 GUI smoke or live scientific workflow pass
is claimed.
