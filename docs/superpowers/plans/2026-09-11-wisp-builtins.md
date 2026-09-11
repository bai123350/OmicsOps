# Wisp Builtins Implementation Plan

**Goal:** Make the pinned Wisp Skills and native scientific tools available in
OmicsOps through its existing Skill and MCP operations.

**Architecture:** Preserve vendored scientific code and Skill resources; adapt
host boundaries in OmicsOps. Keep approval and credential decisions in the host.

**Tech stack:** Rust, rmcp, SQLite, Tauri 2, React/TypeScript.

**Spec:** `docs/superpowers/specs/2026-09-11-wisp-builtins.md`

## Tasks

- [x] Import `skills/wisp-science` with license/provenance and installation
  metadata; test default choices apply only to first installation.
- [x] Add bounded resource discovery/loading helpers in `skill_commands.rs`;
  wire V4 `use_skill` to freeze requested resource sections with compatibility
  instructions. Test real bundle loading and missing resources.
- [x] Adapt `crates/omicsops-bio` from upstream, replacing Wisp-only host schema
  and tool adapters; preserve domain tests. Add filtered dynamic stdio server in
  `src-tauri/src/bio_mcp.rs` and dispatch in `main.rs`.
- [x] Add shared preset DTO and catalog/registration commands. Use stable IDs
  under the existing server lock. Test unknown IDs, disabled/unapproved defaults,
  repeated registration and preservation of configured state.
- [x] Add backend-loaded preset selector in Settings and API wrappers; refresh
  existing profiles after addition. Test calls, errors, retry and no implicit
  inspection or enablement.
- [x] Bundle new Skill resources in Tauri and initialize both bundles. Preserve
  existing single-cell packages and native PubMed entry point.
- [x] Record licensing, upstream inventory, compatibility and manual smoke
  steps. Run all required deterministic tests and desktop build; record live
  acceptance as unexecuted unless separately run.

## Verification results (2026-09-11)

- Passed: cargo test --workspace — 732 passed, 11 explicitly ignored live tests.
- Passed: npm ci — clean dependency installation.
- Passed: npm test — 170 frontend tests and 22 browser-extension tests.
- Passed: npm run build — production Web assets generated; Vite reports its existing large-chunk advisory.
- Passed: cargo fmt --all -- --check and git diff --check.
- Passed: actual desktop executable MCP discovery for all 23 domains, without public network requests; legacy PubMed discovery also passed.
- Passed: review regressions for lazy resource selection and installed package integrity (failure observed before fix).
- Passed: npm run build:desktop — Windows release executable and NSIS bundle built successfully; no release published or installer distributed.
- Not executed: real model/SSH/R/Micromamba acceptance, public database requests, optional third-party Skill dependencies, manual installed-UI smoke, macOS packaging. These are not implied by deterministic tests.
