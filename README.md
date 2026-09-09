# OmicsOps

OmicsOps is a Windows-first Tauri desktop agent that plans and runs
bioinformatics workflows on a remote Linux server over SSH.

The Rust control plane owns credentials, approvals, policy enforcement, task
state, SSH, and audit records. Analysis data stays on the remote server.

Desktop V4 model requests are checked against the selected profile's context
window before sending, including system instructions, tool schemas and provider
formatting. Unset windows retain the existing 32,768-token fallback. The initial
budget uses conservative UTF-8 byte estimation, reserves 4,096 output tokens and
1,024 safety tokens, and explicitly caps the provider's output request. These are
request allowances, not inferred model capabilities. The current catalog does
not supply maximum output limits or model-specific image costs.

Execution archives and checkpoints oversized context once, then checks again.
If it still does not fit, the run needs attention and retains its evidence; it
does not silently discard the plan or send the oversized request. Requests with
actual image parts currently stop with an unknown-image-cost error. Non-vision
profiles continue to receive the existing textual image notice. A provider's
explicit context-overflow error permits one archive-first retry with a smaller
context; a second overflow stops the run. Estimation is not an exact tokenizer.

Runs with the `agent.read_tool_result` capability receive bounded model views of
large tool results. The original stays in the event store and can be paged by
run-scoped sequence/hash references. This tool is unavailable in Plan mode;
existing approved capabilities remain enforced. Repeated completed calls are
compared with their results, so changing job state counts as progress. Unchanged
repeated observations stop the run for attention. Truncated responses and streams
without a terminal provider event never dispatch partial tool calls.

Read-only subagents receive their objective, explicit dependency results and
required output schema. Each child request is capped at 64 KiB (or the smaller
parent context limit), including tool schemas; submissions are capped at 8 KiB.
Oversized evidence fails the node with its original tool outcomes retained.
Model attempts and read-only tool waits have deadlines and observe cancellation.
The parent receives conclusions and call counts, with run-scoped references for
the complete delegation trace. Recovery reuses successful nodes only for an
identical graph and successful dependencies. Delegation reserves at most 32 model
turns and 64 tool calls per run; restarting an incomplete graph reserves its full
declared budget again, so recovery cannot reset that allowance.

In a model profile's settings, **Read-only subagent model** selects another saved
profile for newly created ordinary Agent runs. The child profile ID and exact
execution-configuration hash are frozen into the run. A missing or changed child
configuration prevents execution/resume; restore it or start a new run. Existing
runs and approved plans retain their previous model behavior. Profile labels and
credential rotation do not change the execution hash; credentials remain in the
keyring. Reasoning-effort overrides and automatic main-model routing are not yet
supported; selecting a profile does not imply a `max` effort setting.

## Development

Prerequisites:

- Rust stable
- Node.js 22 or newer
- Windows WebView2

```powershell
npm install
cargo test --workspace
npm test
npm run tauri:dev
```

## Production build

Build the Windows application and NSIS installer with:

```powershell
npm run build:desktop
```

The standalone production executable is written to
`target/release/omicsops-desktop.exe`, and the installer is written under
`target/release/bundle/nsis/`. Do not distribute `target/debug/omicsops-desktop.exe`:
the default Tauri configuration embeds `dist` and never points at localhost.
The Vite development URL is isolated in `src-tauri/tauri.dev.conf.json` and is
used only by `npm run tauri:dev`.

See `docs/superpowers/specs/2026-07-29-omicsops-agent-design.md` for the
approved design.

The V2 control plane uses versioned tool contracts, canonical plan approvals,
append-only hash-chained events, verified checkpoints, process-group recovery,
and auditable run bundles. See `acceptance/README.md` for the pinned PBMC and
bulk RNA-seq acceptance procedure.

Running ordinary Agent tasks accept short follow-up guidance through the inline
**Run guidance** panel. Each run accepts up to 16 messages of 2048 UTF-8 bytes each.
“Received” means durably stored; “Applied” means added to the model context, not
that the requested work has finished. Guidance interrupts a pending model request
and is applied at the next model boundary. Pending read-only tools yield;
completed results are retained. Dispatched side-effecting tools finish their
batch first. Retries reuse the message ID, and pending input survives restart. Guidance
cannot modify an approved plan or expand execution permissions.

Runtime invocations now have a durable job identity separate from the interpreter
session. Repeating the same recorded invocation cannot launch a second computation,
including after an uncertain connection failure. Cancellation records unfinished
jobs as unknown, not as confirmed remote cancellation. Completed output remains
in the existing tool events and archives; the job ledger stores only metadata and
a result digest. In-process workers now support event-driven waiting and rereading
results without rerunning computation. Dropping a waiter does not cancel its worker.
Automatic reconnection across application restarts is not yet implemented.

A bounded, temporary runtime-result receipt now bridges the gap between recording
job completion and committing its tool event. Recovery verifies the original
request, session and result digest and never executes the cell again. The receipt
is removed atomically with the tool event. Explicitly uncertain dispatches retain
the existing reconciliation requirement; terminal runs are not reopened.

When an inactive run exceeds the existing stale-run threshold, the desktop now
checks for verified receipts for every unfinished dispatched operation. If all
are available, it preserves the run and offers **Resume saved results** instead
of marking it failed. Resuming follows the original run and frozen configuration.
Missing receipts, explicitly uncertain dispatches and terminal runs retain their
existing handling; this does not reconnect to a still-running remote process.

OpenAI-compatible model settings now expose **Requested reasoning effort**. Save
an explicit wire value (including `max`) or choose **Provider default** to omit
it. Each saved profile, including a bound read-only child, uses its own setting;
changing a frozen child's effort requires a new run. Older callers that omit
this setting preserve it, while explicit null clears it. Test sends the same
effort with a 4096-token output allowance unless a request budget overrides it.
Acceptance of a request does not prove the service used that effort. There is no
automatic downgrade, model-family capability inference or automatic Luna route;
the full capability catalog and effective-effort reporting remain pending.

New ordinary runs also freeze the main model's execution configuration. Resuming
with a changed model ID, endpoint, provider, reasoning effort, capability flags
or context window fails before opening execution resources. Restore the original
settings or start a new run; renaming a profile or rotating its keyring credential
does not invalidate it. The client and request budget use the same loaded profile
snapshot. Legacy runs without the fingerprint and approved-plan runs retain their
existing behavior; no historical configuration is invented or backfilled.

A run paused at **Resume saved results** can now be ended with **Cancel this run**.
Cancellation records its terminal event and status atomically, retains the saved
receipts/job identities for audit, and does not launch computation or validate the
results. Resume and cancel are mutually exclusive while an action is submitting;
a delayed resume cannot overwrite a committed cancellation. This action applies
only to saved-result recovery, not other pauses or live remote jobs.
