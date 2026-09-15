# MCP directory, automatic approval and dispatch boundaries

This supersedes the persistently-tool-approved restriction in the earlier MCP recovery increment for ordinary RiskBased runs.

- Discover the complete enabled MCP directory once per run, then select tools from its result. Remove search_mcp_tools from subsequent model requests; repeat requests reuse the recorded directory. Actual literature search, pagination and fetching records remain separate operations. Directory discovery is not a paper search.
- RiskBased plus an enabled, configured, launch-approved server authorizes calls to advertised read-only tools without individual tool approval prompts. Exact catalog/schema checks remain mandatory before dispatch and readOnlyHint is rechecked by the MCP runtime. This third-party assertion is trusted under the selected policy, not an independently verified guarantee. Unknown or mutating tools still require approval. RequestApproval retains per-call approval. Existing frozen policies are not rewritten.
- Validate MCP metadata before dispatch. A wrong model-supplied hash returns a failed, undispatched outcome with current metadata, allowing correction without another directory search. Runtime schema/catalog/annotation rejection also returns a failed, undispatched result and audit record. Actual transport loss after dispatch retains uncertain audit state and prohibits automatic replay; the desktop displays failure without a side-effect verification form, as specified in the 2026-09-15 failure-handling update.
- Bundled PubMed, including the unified `--omicsops-bio-mcp pubmed` implementation, retries an NCBI E-utilities HTTP 400 request containing `api_key` once without the optional key. The endpoint and all other form parameters are preserved, and NCBI pacing applies to both attempts. A keyless 400, any other status, and failure of the anonymous attempt are not retried. Request errors omit URLs and upstream error bodies to avoid exposing key values. Empty or recognized template key values are omitted after trimming; other opaque values are accepted without a client-defined format restriction. Other MCP server implementations are unaffected.

Validation: deterministic tests cover policy grants, hash/launch/hint restrictions, discovery descriptor removal, undispatched-versus-uncertain error classification, and bounded anonymous PubMed retry. Run workspace/frontend tests and Web/desktop builds. Live model and MCP acceptance is separate and remains unverified without an explicit disposable fixture.

Executed: cargo test --workspace (416 passed, 11 ignored), npm test (149 frontend and 22 extension passed), npm run build, cargo fmt --all -- --check, and npm run build:desktop using target/timeline-desktop (all passed). Live model/MCP retrieval was not executed.

2026-09-15 regression follow-up: the migrated unified server was observed returning HTTP 400
for valid PubMed tool arguments while a credential reference was configured. The secret was not
inspected, so rejection of that optional key remains a likely explanation rather than a proven
cause. Deterministic Axum tests cover keyed-then-anonymous success, bounded double failure,
keyless 400, non-400 failure, parameter preservation, and shared JSON/XML behavior. A synthetic
anonymous connectivity probe succeeded; no real literature query or production MCP acceptance
was executed.
