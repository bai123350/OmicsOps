# Wisp scientific Skills and MCP integration

User requested direct import of Wisp Science builtins and explicitly accepted
the associated AGPL license adjustment. Pin upstream to
`3628a4209e494ba6fbef1095bb964782f7d2c430` from
https://github.com/xuzhougeng/wisp-science.

## Scope and boundaries

Bundle all 26 upstream Skill packages, their scripts/reference files and notices,
alongside the existing single-cell bundle. Retain upstream bytes and add separate
OmicsOps compatibility guidance. Do not claim Wisp-specific tools, environment
managers, theme installers or sidecars are present or executed. Skill scripts are
source material to inspect and execute through existing approved tools, in the
selected local/SSH execution context. Preserve user enable/disable choices across
restart. V4 must expose referenced resources on demand through `use_skill` rather
than inject every script into every conversation.

Vendor the actual native scientific implementations into `omicsops-bio`. This
small boundary isolates scientific HTTP protocols from desktop state and MCP
transport. It is a new crate because its domain clients and tests should not
depend on the desktop, agent, store or MCP lifecycle. Retain all 23 upstream
domains, schemas, implementations, parser/mock tests and source/license notices.
Adapt only the upstream host-specific schema/tool layer.

Expose each domain via `--omicsops-bio-mcp <domain>` using the packaged desktop
executable and the existing project-isolated stdio MCP runtime. A backend-derived
catalog provides descriptions and real tool counts to Settings. Startup registers
all missing presets idempotently as enabled, discoverable, unapproved profiles.
Tool schemas and catalog hashes come from the same compiled catalog as stdio;
registration does not launch a process or contact a service. Existing user choices
are preserved. Empty legacy builtin catalogs are backfilled only when their
command matches the current executable and domain arguments. Inspection and
execution approvals remain required. Existing native PubMed
profiles remain available. Domain filtering must apply to discovery AND calls.
Malformed input/unknown tools must not contact external services. Preserve
structured scientific results; failures must remain MCP error results.

No new credential persistence: only existing keyring references and in-memory
allowlisted environment resolution. External providers receive requested query
terms, identifiers and, for submission tools, supplied sequences; bundle does
not imply offline retrieval or that Python/R dependencies are installed.

## Other upstream resources (inventory only)

Python/R kernel workers, browser extension and seed/demo datasets/manifests are
also bundled upstream. Community skills exist outside its bundled resources.
These are outside this implementation; do not import demo data or unrelated
runtime/UI features.

## Verification

Use temporary stores/directories, mock Tauri commands and localhost HTTP
fixtures. Cover catalog parity, domain isolation, idempotent profile addition,
authorization defaults/preservation, Skill installation/restart and lazy
resources, DTO serialization and UI error/retry. Run `cargo test --workspace`,
`npm test`, `npm run build`, and `npm run build:desktop`. No real SSH, API keys or
public services are automatic test prerequisites. Record ignored/live acceptance
separately from deterministic results.

Startup-import regression verification: the discoverability assertion failed
before the fix and passed afterwards. Full workspace tests: 740 passed, 11
ignored; frontend: 179 passed; extension: 22 passed. Production Web and Windows
desktop builds passed. No live database/model/SSH calls or installed-app UI
acceptance were performed for this fix.
