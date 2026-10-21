# Bounded inspection of the selected Python/R environment

## User problem and evidence

The user requested only an inspection of prepared Python/R environments in the
remote project. The selected backend is now correctly SSH. Read-only inspection
of the persisted events and generated code found two successful execution
results and existing environment reports, followed by a third, broader script.
That script searched multiple shared Conda locations, launched every discovered
R/Python interpreter, loaded twelve R packages and eleven Python packages, and
allowed each child process 300 seconds. The enclosing call had no total budget.
The user stopped it after about 307 seconds; the persisted terminal state is
needs-attention with an unresolved dispatch. This does not establish a deadlock
or prove that the remote subprocess was cancelled.

The general-purpose persistent kernel buffers output until the code completes.
Its unlimited wait supports scientific computation and must not be shortened
globally to address this inspection request. Previous prompt-only advice to
perform a bounded inspection did not provide an actual bounded operation.

## Chosen change

Add a small host-owned, read-only tool, `runtime.environment.inspect`, to the
existing tool registry and desktop environment port. It accepts exactly `{}`.
The host supplies the frozen backend and environment. It checks Python and R
executable discovery and versions only, with a fixed overall probe budget of at
most 20 seconds after existing resource preparation, and at most 5 seconds per
language probe. Outcomes distinguish available, not found, timeout and error.
Missing interpreters are valid negative observations; transport failure or a
timeout must never be reported as proof of absence.

The operation does not install software, load scientific packages, create a
persistent kernel, write remote files, activate another environment, or enumerate
all Conda installations. Local child processes are terminated on timeout/drop;
remote version commands must carry their own bounded timeout with a kill grace
period in addition to the host wait deadline. If that remote timeout facility is
unavailable, return an explicit error instead of running an unbounded command.
The host timeout alone cannot establish remote process termination. Output is
bounded and raw process/connection errors are not returned to the model.

The observation explicitly identifies its selected-environment scope and that
dependencies and unselected environments were not checked. When the question
concerns project-local preparation, use the existing project-root listing for
nearby environment markers without recursively searching the server. A marker
file or executable path alone does not prove dependency or workflow readiness.
Container inspection may report unsupported; it must not start a container under
a read-only capability merely to obtain a version.

Tool guidance directs ordinary availability checks to this tool and then to
completion using its observation. Existing reports may supply context, but do
not justify repeated full-server audits. Package import compatibility testing or
inspection of other environments requires that additional user-requested scope.
No keyword-based prompt routing is introduced, and arbitrary scientific runtime
execution retains its existing lifecycle. The model still selects tools; the
new tool's host limits are enforceable independently of model-generated code.

Plan/read-only authorization, frozen Execute capabilities and disabled tools
retain their existing checks. The new tool cannot rewrite a frozen selection or
run old capabilities with new authority. No migration, credential storage,
historical event rewrite or automatic uncertain-dispatch retry is introduced.
The old stopped run must remain auditable; a new request uses the new tool.

## Validation

Deterministic tests cover strict zero arguments and capability boundaries,
normal and negative observations, error/timeout parsing, per-language and
overall deadlines, bounded output, sanitized failure, frozen selection, no
kernel dispatch or project writes, and a scripted inspection-to-completion
sequence. The initial five probe regressions were observed failing against the
stub before implementation. The final focused desktop/probe group passed 11/11.
The total-deadline test allows the timer wheel's one-millisecond rounding.

Validation on 2026-10-21:

- `npm run build`: passed.
- `npm test`: 1,494 frontend tests and 22 browser bridge tests passed.
- `cargo test --workspace`: 1,991 passed, zero failed, 20 ignored. Ignored tests
  are not live acceptance results.
- `cargo fmt --all -- --check` and `git diff --check`: passed.
- `npm run build:desktop`: passed (exit 0). The Windows release executable and
  local NSIS bundle were generated; they were not installed, published or run as
  native UI acceptance.

The opt-in Go routing fixture uses the saved selected model through production
composition, a temporary empty Local/system project, and the ordinary tool
directory. Its natural Chinese request does not name the inspection tool. A
fixture-only guard rejects non-read-only dispatch before the executor; a wrong
tool choice fails the fixture instead of running arbitrary model code. It allows
four model requests, four tool calls, zero retries and 180 seconds total. It must
record exactly one successful inspection and RunCompleted without execute/ensure
dispatch. This validates real-model routing, not the SSH transport or installed
scientific dependencies. Live SSH acceptance still requires the disposable
configuration in `acceptance/README.md`; reading the historical run is diagnosis,
not acceptance of the replacement flow.

The explicit live command, after the deterministic checks, was:

```powershell
cargo test --workspace agent_v4::go_live_acceptance_tests::live_opencode_go_agent_inspects_selected_environment -- --ignored --exact --nocapture
```

The single selected saved OpenCode Go `deepseek-v4.1-flash` profile passed (exit
0). The fixture completed in 10,323 ms; the Agent portion took 10,283 ms with two
model requests, three tool requests, one successful inspection, zero
execute/ensure dispatches, zero blocked side-effect dispatches, and RunCompleted.
The workspace command reused the full-check build and selected only this exact
ignored case. No real SSH host or existing project was accessed.

## Native SSH smoke (not executed)

Start the newly built executable and create a new conversation with the project's
SSH backend and intended environment selected. Ask only whether Python and R are
prepared in the current project folder. Expect the bounded inspection, a project
root listing when needed, and a scoped conclusion. An ordinary availability check
should not dispatch arbitrary runtime code, load scientific packages, or write
an audit report. Missing R should be reported as an observation about the selected
environment, not as an uncertain environment installation. A failed or timed-out
probe must remain unverified. Connection preparation and model latency are
outside the 20-second probe budget. Existing stopped runs remain unchanged and
must not be automatically replayed.
