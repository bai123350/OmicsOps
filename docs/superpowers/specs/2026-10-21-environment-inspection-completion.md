# Bounded environment inspection and completion recovery

## Observed failure

A read-only inspection of an existing SSH Python/R environment ran for over an
hour. A read-only examination of its persisted events found 35 runtime requests,
22 rejected completion proposals and no Reviewer invocation. Every rejected
proposal contained the same `provenance_incomplete` finding: a successful analysis
had not declared a random seed. Later reports and new analyses could not repair
the earlier immutable provenance manifests. The user eventually cancelled the run.

Environment inspection had unnecessarily supplied `runtime.execute.analysis`.
The tool schema allowed a missing/null seed, while the scientific verifier
required a seed for every successful declared analysis. The deterministic
completion failure branch had no independent correction budget. Recording new
scientific state also reset the general progress detector, so repeated reports
did not stop the loop.

The run accumulated 1,145 events containing about 26.6 MB of JSON, mostly
scientific checkpoint snapshots. The composer queue driver loaded and verified
the entire event chain on each poll, with a 250 ms delay between polls, even
while the run was still running. The frontend also launched four project-data
queries on every event without coalescing overlapping requests. These were
avoidable database/CPU load multipliers. No permanent database lock or
nested pool acquisition was established; this is not proof of the exact cause
of the screenshot's transient pool timeout.

## Behavior and boundaries

- Environment inspection uses bounded probes and existing tool outcomes. It
  omits `analysis`; it does not need new report files, evidence records or repeated
  audits. Finding Python/R on PATH does not establish dependency readiness.
- A supplied scientific `analysis` must declare an unsigned integer random seed
  actually used by the computation. Missing/null/invalid seeds are rejected in
  tool validation before approval, scientific-state mutation or dispatch. The
  host does not invent seeds or rewrite historical manifests. Existing stored
  analyses remain readable and subject to the same scientific verification.
  Host-only recovery of an already dispatched call retains the old declaration
  shape so it can retrieve that exact durable result. It retains capability,
  disabled-tool and other argument checks, and cannot authorize re-execution.
- Deterministic completion validation allows two correction opportunities by
  default. The third failed validation writes `RunNeedsAttention`. The count
  comes from durable events and survives recovery; new scientific state cannot
  reset it. Stop retains priority over this terminal state. Recovery after the
  budget is exhausted does not start pending tools again; already dispatched
  side effects still follow the exact-result/uncertainty reconciliation path.
  Reviewer correction limits remain independent. A valid corrected
  proposal can still complete before the limit. This bounds failed completion
  cycles, not the wall-clock duration of an individual model or runtime call.
- Queue settlement checks read the small run row while execution is active.
  Terminal status still requires the validated terminal event and the existing
  safe-attention checks before FIFO can advance. An in-flight remote job is not
  cancelled by this change or by a stopped Agent.
- A transient status-poll failure displays “运行状态暂时无法刷新” separately from
  actual submission/run failures, retains the last confirmed execution state,
  and clears after a successful refresh or conversation switch.
- Project-data refreshes respond to relevant persisted changes instead of every
  model/progress event. Overlapping refreshes are coalesced, with a trailing
  refresh for changes received while a request is pending. Responses from an
  obsolete project scope cannot replace the current project's data.

There is no database migration, credential storage change, new remote write,
approval expansion or hash-verification bypass. Earlier remote-file-list and
header-button changes in the working tree are separate work.

## Verification

Regression tests use in-memory stores, scripted models, mock commands and mock
Tauri APIs. They cover invalid declarations rejected before runtime execution,
ordinary inspection without an analysis, seeded scientific execution, repeated
completion failures across recovery, successful correction, terminal settlement
integrity, and transient status notices without loss of real errors.

Executed on Windows (2026-10-21):

- Tool validation/recovery: 24 tests passed after reproducing missing-seed
  acceptance and legacy-recovery rejection with failing regressions.
- Queue driver: 7 tests passed after the active-history-read regression failed
  against the original code.
- `npm run build`: passed.
- `npm test`: 1,488 frontend tests and 22 browser tests passed.

The final `cargo test --workspace` rerun passed: 1,970 passed, 0 failed and 19
ignored. The first sandboxed attempt reported failures in existing loopback HTTP and temporary
directory installation tests, and other mock-server tests remained waiting; that
test child was stopped. Both reported failures passed individually outside the
restricted sandbox, followed by the full workspace rerun. No ignored live tests
were enabled. `cargo fmt --all`, the format check and `git diff --check` passed;
formatting touched only files already changed by this work.

`npm run build:desktop` reached native compilation/linking but could not replace
`target/release/omicsops-desktop.exe` because the user's existing process was
running from that path (Windows access denied). This attempt is not a successful
desktop build. After all deterministic checks passed, the user explicitly
authorized closing the old application and completing the build. The old process
was closed. The final `npm run build:desktop` rerun passed (exit 0), including the
final cancellation fix, optimized native compilation and x64 NSIS packaging.
Outputs are `target/release/omicsops-desktop.exe` and
`target/release/bundle/nsis/OmicsOps_0.1.0_x64-setup.exe`; these local build
artifacts were not installed, published or committed.

Live model/SSH acceptance must follow `acceptance/README.md` with an explicitly
configured disposable environment; the read-only examination of the cancelled run is diagnosis, not
end-to-end acceptance.

Manual smoke: start the rebuilt Windows executable, create a new conversation in
the intended SSH project and request only Python/R interpreter and requested
dependency checks. Confirm that it reports actual probe results directly and
does not create an analysis/report solely for inspection. Simulate a status
refresh failure and verify the notice does not mark the execution failed; allow
refresh to recover and check the notice clears. Do not resume the old cancelled
run to erase or circumvent its incomplete provenance.
