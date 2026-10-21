# Project compute defaults and system environment probes

## Observed failure

A request to inspect an existing remote Python/R environment failed after four
steps. The persisted run selected `local/system`, matching the composer's Local
label. Python execution succeeded, but `runtime.environment.ensure` for `R` and
`system` failed with `program not found`. The host recorded
`ToolDispatchUncertain` and terminated the run. These events establish a host
environment-selection and tool-error problem, not an OpenCode authentication or
model-protocol failure.

The composer initialized its selection to Local and preserved that value whenever
the local backend was available. That condition ran before the default for a
project's SSH binding, so a newly opened SSH project still selected Local.
Selections could also be submitted while the backend list belonged to the
previous project or container image.

The desktop interpreter preflight covered execution and rebuild, but omitted
`runtime.environment.ensure(system)`. Its missing-program error escaped to the
core's conservative side-effect uncertainty handler. That handler is necessary
for managed environment creation, but a system interpreter version probe does
not create or install an environment.

## Behavior and boundaries

- Initialize the composer backend within the current project/binding scope.
  Prefer the project's available bound SSH backend on first load. Preserve an
  explicit Local choice while that scope remains current. Do not infer execution
  authority from the natural-language prompt or alter a frozen run.
- A new request may freeze a compute selection only after its backend catalog
  has loaded for the current project, binding and container image. The composer
  and submission paths must agree on this readiness condition.
- Preflight system environment checks for the requested interpreter and frozen
  environment. An unavailable interpreter produces a structured recoverable
  tool result before dispatch instead of an uncertain side-effect failure.
- If the system version probe fails after preflight, return a structured failed
  check and accurately record that the probe was attempted. Do not describe it
  as an unstarted operation or expose raw process/connection diagnostics.
- A failed check is not proof that the interpreter is absent and does not become
  successful completion evidence. Guide the model to a bounded read-only probe
  using an already available interpreter in the same frozen environment, or to
  report that verification is blocked. Do not install software, switch backends
  or repeatedly call ensure merely because readiness could not be verified.
- Claim system readiness only when the requested interpreter returned a
  nonempty version. Interpreter availability alone does not establish installed
  project dependencies or scientific workflow readiness.
- Keep managed environment creation's existing approval, mutation and uncertain
  dispatch handling. A failed or interrupted Micromamba creation must not be
  silently retried or described as having no side effects.

No model-specific routing, credential access, database migration or historical
event rewrite is needed. A failed historical run retains its original frozen
selection and events.

## Verification

Regression tests use fake Tauri APIs and mock environment ports, without real
Python/R, SSH or model credentials. They cover SSH project defaults, explicit
Local selection, scope changes, pending/stale catalogs, missing R, failure after
preflight, successful version evidence and managed-environment error retention.

Executed on Windows (2026-10-21):

- Five frontend regressions failed against the original behavior and passed
  after the fix. The two affected frontend files passed all 326 tests. An older
  Docker permission test now supplies its required image before asserting a
  ready configuration.
- Review caught the loading flag disabling the container image input before
  its debounce elapsed. A sixth regression reproduced that failure and passed
  after restoring the delayed busy flag while immediately invalidating the
  catalog response. Continuous input remains editable; only the final verified
  image can be frozen. Eight targeted frontend tests passed after this repair.
- Three new native regressions reproduced the original failures; the managed
  environment uncertainty regression already passed and remains unchanged.
  `cargo test -p omicsops-desktop --lib environment_ensure -- --nocapture`
  passed all five matching tests. The existing language-specific interpreter
  preflight regression also passed.
- `cargo test -p omicsops-agent-core missing_r_preflight_can_complete_from_a_successful_bounded_python_probe -- --nocapture`
  passed. This scripted sequence checks a failed R preflight, one successful
  Python observation and verified completion with no repeated ensure,
  uncertain dispatch or completion-correction loop.
- `npm run build`: passed.
- Final `npm test`: 1,494 frontend tests and 22 browser-extension tests passed.
- `cargo test --workspace`: 1,975 passed, 0 failed and 19 ignored. It ran in the
  normal Windows process environment so deterministic loopback fixtures could
  bind their local test servers. No ignored live tests were enabled.
- `cargo fmt --all` corrected formatting in the newly added test; the subsequent
  format check and `git diff --check` passed.

`npm run build:desktop` passed (exit 0): optimized Windows native compilation
finished in 4m 34s, followed by x64 NSIS packaging. Outputs are
`target/release/omicsops-desktop.exe` and
`target/release/bundle/nsis/OmicsOps_0.1.0_x64-setup.exe`. These local artifacts
were not installed, published or committed. Real OpenCode/SSH acceptance is
separate and requires the explicit disposable configuration in
`acceptance/README.md`; it has not been run for this change.

Manual smoke: start the rebuilt Windows application and open the SSH-bound
project. Before sending a new inspection request, confirm the composer shows
its SSH backend. Check that an explicit Local choice persists within the same
project, that switching projects does not retain another project's selection,
and that sending is disabled while the new backend catalog is loading. In a
disposable environment with no R interpreter, request a check and confirm the
tool reports the unavailable interpreter without an uncertain-dispatch failure.
