# Agent failure handling and context cost

## Observed problem

A local literature-research run stopped with `kernel event request identity
mismatch` after four unified PubMed `search_articles` calls returned
`NCBI returned HTTP 400`. The desktop required the researcher to enter
side-effect verification evidence before continuing.

Read-only inspection confirmed valid search arguments and dispatch through
`--omicsops-bio-mcp pubmed`, with a migrated NCBI key reference. The stored
status-only error cannot prove why NCBI rejected the request. No credential
value was inspected. The unified client omitted the older client's anonymous
fallback for HTTP 400 on a request carrying an optional API key.

A deterministic Python-driver reproduction under ASCII stdio produced the
identity-mismatch failure path: Unicode output raised an encoding error, then
the outer error handler emitted a different request ID. This establishes a
code defect, but the historical event does not preserve the underlying Python
exception well enough to prove it was the only cause of that particular run.

## Bounded changes

- Make the Python JSONL wire independent of Windows locale and preserve the
  active request identity when reporting protocol failures. A failed or
  desynchronized process must not be reused for a later cell; the failed cell
  itself must not be automatically replayed.
- Show uncertain dispatch as failure in the desktop, including historical
  events, without a side-effect evidence form. Retain durable uncertainty,
  unknown job state, approval boundaries and the host reconciliation API.
  Failure of observation does not establish remote cancellation.
- Restore bounded anonymous PubMed fallback and keep diagnostics free of
  keys, URLs and raw upstream bodies. Keep NCBI pacing and parameter validation.
- Reduce model-only event projections by removing repeated envelope fields
  and exact duplicate successful tool content. Preserve bounded large outputs,
  failure diagnostics and run-scoped retrieval of the unchanged original event.

No schema migration or model-family capability inference is introduced. The
requested GPT-6/GPT-5.6 model split applies to development work for this change;
existing application model profiles and their frozen bindings remain user
configured.

In a deterministic duplicate-result fixture, the model projection shrank from
12,168 to 5,888 serialized bytes (51.6%). This is a fixture measurement, not a
claim about the full research run's token bill; provider tokenization, reasoning,
cache use and subsequent tool choices also affect cost.

## Validation and limits

Regression coverage targets forced-ASCII Unicode execution, failed-process
reuse, the desktop failure presentation, bounded PubMed fallback with a mock
HTTP server, and projection size/retrievability. Delivery checks are Rust
workspace tests, frontend tests, Web build and desktop build.

Executed on Windows after the final source edits:

- `cargo test --workspace`: passed, 1,097 tests; 11 ignored tests were not run.
- `npm test`: passed, 563 Vitest tests and 22 browser-extension tests.
- `npm run build`: passed.
- `npm run build:desktop`: passed after the SSH driver update, including the
  local NSIS packaging check. No installation or distribution was performed.
- `cargo fmt --all -- --check`: passed.

Build output retains the Vite large-chunk advisory and the Windows linker's
informational library-creation warning; neither failed the build.

Independent review covered both drivers, local/SSH health handling, the actual
desktop background-job path, historical failure UI, PubMed fallback and context
retrieval. Its actionable test/documentation findings were fixed and rechecked.

Only a synthetic anonymous NCBI connectivity request was sent externally; it
returned a successful JSON response. The real research query was not sent.
This probe is not a live Agent, MCP, SSH or model acceptance run. Real model/SSH
acceptance requires the disposable environment in `acceptance/README.md`.

Manual Windows smoke: run a disposable Python cell printing Chinese text and
an emoji, then a second cell; exercise a failed kernel connection and confirm
failure appears without a verification form or automatic replay; query PubMed
with a separately configured test profile; inspect both a new and historical
failed conversation. Repeat local/SSH cases separately. macOS behavior is not
asserted from a Windows build.

Reference: [Wisp Science Agent implementation](https://github.com/xuzhougeng/wisp-science/blob/main/crates/wisp-core/src/agent.rs)
uses bounded tool results with retrievable originals and loop safeguards;
[its MCP bridge](https://github.com/xuzhougeng/wisp-science/blob/main/src-tauri/src/mcp_bridge.rs)
returns tool errors as error results. The OmicsOps implementation retains its
existing durable execution and approval contracts.
