# Agent V4 deterministic scenarios

The private `omicsops-agent-core` test suite runs five scenarios sequentially:
ReadEvidence, DenyApproval, RecoverUncertain, CorrectMcpArguments, and
CompactPreserveSources. It checks evidence integrity and scope, denied approval
without dispatch, uncertain dispatch without replay, corrected MCP arguments,
and context compaction with retrievable sources and frozen authority.

Run the suite from the workspace root in PowerShell:

```powershell
cargo test -p omicsops-agent-core scenario_evaluation_suite -- --nocapture
```

Default workspace tests include this suite. No report file is written unless
`OMICSOPS_EVAL_REPORT_PATH` is explicitly set. To save an ignored artifact:

```powershell
$env:OMICSOPS_EVAL_REPORT_PATH = 'target/evaluation/v4-scenarios.json'
try {
    cargo test -p omicsops-agent-core scenario_evaluation_suite -- --nocapture
} finally {
    Remove-Item Env:OMICSOPS_EVAL_REPORT_PATH
}
```

Relative destinations resolve from the workspace root, including when Cargo
starts a test in its crate directory. Absolute paths are accepted only inside
the workspace's ignored `target/` directory. Parent traversal, outside paths,
file parents, symlinks, Windows reparse points, and ancestors resolving outside
`target/` are rejected. An explicit valid request may create a missing target
root and report parents after validating the existing workspace and destination
ancestry. The suite reads only this named environment variable. It uses
Rust file APIs and a same-directory `tempfile::NamedTempFile` with `persist`;
repeated writes atomically replace the file on Windows. A failed write or
replacement preserves the previous report. This is atomic replacement, not a
claim of power-loss durability or protection against concurrent directory
mutation by another process.

Every emitted stdout record starts with `OMICSOPS_V4_SCENARIOS=`. JSON is sorted
by stable scenario IDs. The report contains schema/runner versions, a suite hash,
pass/fail totals, assertion totals, actual model-attempt/tool-proposal/dispatch/
recovery counts, and closed failure codes. It excludes raw prompts, model output,
reasoning, tool arguments/results, transcripts, paths, host information,
credentials, timestamps, durations, and random attempt IDs. The curated suite's
public identities and hash format are validated before serialization. All
fixtures are inline; the suite hash covers the normalized manifest with an empty
external fixture inventory. Future external fixtures must join that inventory.

All five outcomes are collected and emitted before asserting that no scenario
failed. A scenario failure exits Cargo nonzero after saving the complete report.
File emission failures also fail the test, using only `InvalidPath` or
`WriteFailed`, without printing raw I/O errors or the requested path. An invalid
manifest fails strict parsing before execution and does not claim a complete
scenario report.

Windows CI preserves Python helper discovery, `npm ci`, workspace Rust tests,
frontend tests, and the Web build. Its directed V4 step sets only the report path
above; an `always()` upload saves artifact `v4-scenarios`, including when scenario
execution fails. If an earlier CI step prevents the directed test from running,
there may be no artifact to upload. Generated reports stay under ignored target
and must never be committed.

These paused-time core substitutes require no network, real model, SSH, GPU, R,
or scientific library. They verify control behavior, not elapsed time or model
answer quality. They do not prove SQLite transactions, real host MCP validation,
remote jobs, scientific computation, or UI behavior. Follow
[`acceptance/README.md`](../../acceptance/README.md) for separate real acceptance;
a passing suite, build, or ignored acceptance test does not establish production
end-to-end acceptance.
