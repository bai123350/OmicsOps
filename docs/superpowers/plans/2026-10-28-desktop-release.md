# Desktop Release Implementation Plan

> **For agentic workers:** Use superpowers:executing-plans or superpowers:subagent-driven-development to implement these tasks. The user has already approved execution of the proposal; no new design decision is pending.

**Goal:** Build Windows, Linux, and macOS installers and prepare complete draft GitHub Releases.

**Architecture:** Native build matrix with read-only jobs, deterministic version/asset
validation, and a single privileged draft publication job. Existing Tauri runtime and
resource boundaries remain unchanged except for selecting real OS keyring backends.

**Tech Stack:** Tauri 2, Rust, React/npm, Python standard library, GitHub Actions.

**Spec:** ../specs/2026-10-28-desktop-release-design.md

## Global constraints

- Four target IDs: windows-x64, linux-x64, macos-arm64, macos-x64.
- Node 22 / Python 3.12; npm ci; cargo --locked; dist before desktop Rust compilation.
- Numeric X.Y.Z versions; initial release is draft + prerelease; no tag creation or public release in this task.
- Keep existing settings UI changes untouched; work on this feature branch in the shared checkout.
- No real secrets, hosts, services, or networks in deterministic tests.

## Review focus

- Tag/version drift or a moved tag must fail before release mutation.
- A missing platform, duplicate installer, or changed bytes must fail aggregation.
- A public release or draft associated with another commit must never be overwritten.
- No platform may fall back to a mock credential store in production.
- Successful packaging must not be described as installed-app or scientific acceptance.

## Task 1: Version and installer validation

Files: scripts/release.py, scripts/test_release.py.

Interfaces (CLI, run from repository root):
- `check-version --tag TAG --github-output PATH`: TAG may be empty for artifact-only builds;
  emits version, tag, create_release (true/false) for Actions. Optional --root for fixtures.
- `stage --target ID --bundle-dir PATH --output-dir PATH --version VERSION --commit SHA`:
  stage normalized installers and manifest-ID.json.
- `assemble --input-dir PATH --output-dir PATH --version VERSION --commit SHA`:
  validate all four targets and emit only six installers, SHA256SUMS.txt,
  release-manifest.json. Both stage and assemble reject nonempty destinations.

- [x] Write/run failing fixture tests for version/tag mismatches, malformed versions,
  complete assembly, missing/duplicate/empty files, tampering, unsafe names, and wrong commit.
- [x] Implement the offline helper using json, tomllib, hashlib, pathlib, argparse.
- [x] Run `python -B -m unittest discover -s scripts -p test_release.py -v`.

## Task 2: OS credential backends and portable fixtures

Files: Cargo.toml, Cargo.lock, crates/omicsops-adapters/Cargo.toml,
crates/omicsops-adapters/src/credentials.rs tests, src-tauri/src/model_commands.rs fixture,
crates/omicsops-browser/src/lib.rs (Linux type inference and discovery tests).

- [x] Select platform-specific native keyring features, with sync-secret-service + crypto-rust on Linux.
- [x] Add an offline test that verifies production backend selection without accessing secrets.
- [x] Fix known Windows-path fixture assumptions without expanding Windows-only features.
- [x] Run targeted Rust tests and update the lockfile with minimal dependency churn.

## Task 3: Packaging and workflow

Files: package.json, src-tauri/tauri*.conf.json, .github/workflows/ci.yml,
.github/workflows/release.yml, scripts/github_release.cjs and its offline tests,
vitest.config.ts (exclude Node script tests from jsdom collection).

- [x] Configure platform targets and existing icons; generic build:desktop uses Tauri platform config.
- [x] Implement prepare/native build/assemble/draft jobs using Task 1's CLI.
- [x] Keep validation/build tokens read-only, pin resolved commit, preserve V4 reports,
  and validate the existing tag and release state before any upload.
- [x] Test release-state decisions using a simulated GitHub client; validate workflow YAML.

## Task 4: Documentation and final validation

Files: docs/releasing.md, README.md, scripts/README.md, this plan's verification record.

- [x] Document first setup, artifact-only runs, tag creation by the maintainer, draft reruns,
  checksums, manual publication, signing limitations, feature differences, installed-app smokes.
- [x] Run Python suite, npm ci, npm run build, cargo test --workspace, npm test,
  npm run build:desktop. Run appropriate formatting check for Rust edits.
- [x] Obtain an independent review and fix actionable findings.

## Verification record

Local validation on Windows, 2026-10-28:

| Check | Actual result |
| --- | --- |
| `npm ci --cache target/npm-cache --no-audit --no-fund --fetch-retries=1` | PASS, 304 packages installed from lockfile; sandbox network restriction required retry outside sandbox |
| Python 3.12 `-B -m unittest discover -s scripts -p test_*.py` | PASS, 74 tests, 2 real-symlink tests skipped for Windows link privileges; workspace temporary directory used for sandbox filesystem operations |
| `python -B scripts/release.py check-version --tag v0.1.0` | PASS, checks only; no tag created |
| `npm run build` | PASS; Vite warns about existing large chunks |
| `cargo test --locked --workspace -j 1` | PASS, 2055 passed / 0 failed / 21 ignored across 91 test/doc-test summaries; normal system temporary directory and NO_PROXY for local mocked HTTP services, outside sandbox |
| `npm test` | PASS, 1555 Vitest + 22 extension + 23 publisher tests |
| Node 22.14.0 `--test scripts/github_release.test.cjs` | PASS, 23 tests; full local npm suite used Node 24.16.0, GitHub jobs specify Node 22 |
| `cargo fmt --all -- --check` | PASS; formatting affected only newly added test code |
| actionlint 1.7.12 on both workflows | PASS; downloaded official binary with matching published SHA-256 |
| scoped `git diff --check` | PASS; Git's CRLF conversion notices only |
| `npm run build:desktop -- -- --locked` | Release application compilation and NSIS packaging passed; command failed during the official WiX download timeout. Packaging recovered separately below; this full command is not recorded as passing |
| `npm run tauri -- bundle --bundles nsis,msi` | Completed both Windows installers after caching the official WiX archive with Tauri's expected SHA-256; EXE 18,231,322 bytes, MSI 27,488,856 bytes. Existing compiled application/resources reused without source changes; no installation smoke performed |

Review rulings and resolved failures:

- Independent review found that Tauri leaves AppDir staging directories with internal
  symlinks. Stage now selects only top-level installer candidates, keeping strict checks
  on the installer itself. New tests failed before the fix and passed afterward;
  independent re-review found no remaining actionable findings.
- Linux's browser candidate list needed an explicit Vec<PathBuf> because no platform
  defaults are populated there. A Linux-equivalent compiler reproduction failed before
  the annotation and compiled after it; Linux discovery tests are gated to Linux CI.
- Vitest initially collected the new node:test file and reported no test suite. Excluding
  scripts/** from Vitest while explicitly running the Node tests fixed the full suite.
- Initial sandbox adapter attempts failed
  `local_lm_studio_client_does_not_follow_redirects` (local proxy/socket denied) and
  `installation_reports_only_the_rename_that_created_the_managed_directory` (rename denied).
  The extra partial run was interrupted, not passed. The later full workspace run passed.
- The first full Rust attempt hit an MSVC linker failure; isolated linking and the later
  single-job workspace run passed. A trial TEMP directory under target triggered
  `clipboard_paths_accept_project_absolute_or_relative_files_and_dedupe_stably`'s path
  safety rule. Restoring the normal system temporary directory fixed this environment
  issue without weakening the path policy.
- The first Python suite attempt hit sandbox TEMP rename denial in
  `test_worker_result_can_be_read_by_new_controller_without_running_code`; the final
  run with workspace TEMP passed. These were local harness constraints, not bypassed tests.
- The initial desktop build could not download NSIS inside the sandbox. Retrying outside
  the sandbox produced the application and EXE but timed out downloading WiX. The official
  WiX 3.14 archive was verified against Tauri CLI's expected SHA-256 and placed in Tauri's
  normal local cache; packaging-only recovery then generated both installers. Rebundling
  the already patched executable warned that `__TAURI_BUNDLE_TYPE` was absent and updater
  support might be affected. OmicsOps does not configure an updater; no updater behavior
  is claimed or tested. Fresh native CI packaging remains to be verified.

Not executed: GitHub Actions runs, Linux/macOS native builds, installation/upgrade smokes,
real OS credential persistence, real model/SSH/R/Micromamba/PBMC acceptance. The 21 ignored
Rust tests and two skipped Python link tests are not acceptance passes. No tags, commits,
GitHub Releases, signing credentials or public distribution were created by this task.

## Follow-up: pristine distribution requirement

The user explicitly requires packages without personal information or API credentials.
Read-only input inspection found that the previous Windows executable embedded local
home/checkout paths in compiler strings and development resource fallbacks. No known
credential pattern matched the inspected installer inputs, and the installer resource
list contained no user database, logs, environment files or private keys. Previous local
packages must nevertheless be replaced before distribution because of those paths.

- [x] Disable frontend dotenv/prefixed environment injection; verify with an actual Vite
  build containing synthetic secrets, which failed before the configuration fix.
- [x] Compile development resource fallbacks only with debug assertions; test the same
  helper in both debug and release configurations, including a failing release test first.
- [x] Wrap desktop builds with path remapping, debug-info stripping, removal of credential
  environment inputs, and resource/frontend/native-executable checks before packaging.
- [x] Add offline regressions for package-input boundaries, secret formats, safe diagnostic
  output, and blocking packaging when an audit fails; review the gate independently.
- [x] Document clean CI builds and fresh-user acceptance without erasing existing user data.
- [x] Complete final regression checks and replace Windows installers after privacy checks.

Validation of the rebuilt artifacts is recorded below. No real API key,
OS keyring entry or user database was read for the audit. Third-party public attribution
and licenses are retained. Pattern scans are bounded checks, not proof about every unknown
secret format; Linux/macOS native builds and clean-account installation remain unexecuted.

Follow-up validation on Windows:

- `npm run build`: PASS; existing Vite chunk-size warning remains.
- `npm test`: PASS, 1555 Vitest + 22 browser-extension + 28 release/build-helper tests.
- Python 3.12 full script suite: PASS, 95 tests, 2 existing real-symlink privilege skips;
  all 21 privacy tests passed. Resource/frontend audit passed; the prior executable was
  correctly rejected for local-build-path, with no credential value printed.
- `cargo test --locked --workspace -j 1`: PASS, 2057 passed, 0 failed, 21 ignored;
  cache hard-link fallback and linker-information warnings only. Used normal system TEMP
  and local mock HTTP NO_PROXY outside the sandbox, as in the earlier full run.
- `cargo fmt --all -- --check`, scoped diff whitespace check, and actionlint: PASS.
- Independent privacy review identified and resolved additional packager input paths,
  implicit macOS Info.plist, configuration metadata scanning, and scanner false positives.
- Node 22.14.0 build-helper tests: PASS, all 5 tests, including the actual frontend build
  with synthetic secrets; the full local npm suite used Node 24.16.0.
- `npm run build:desktop -- -- --locked`: PASS with the privacy wrapper and Python 3.12;
  release compilation completed in 5m 27s and both pre-packaging privacy checks passed.
  Generated replacement NSIS EXE (18,245,687 bytes) and MSI (27,497,048 bytes).
- Independent final artifact-input inspection: native executable (85,933,056 bytes),
  234 resource files, MSI's 9 summary fields and 19 property fields contained zero matches
  for the current username, home directory or workspace directory (UTF-8/UTF-16 and path
  variants). WiX lists 235 files including the executable; NSIS's 234 resources match.
  No missing, external or forbidden resource file was found. This inspects package inputs
  and installer metadata, not an installed application or an exhaustive secret detector.

## Follow-up: wisp-science release conventions

Reference inspected at `d783ce154e70905a783f7ed461f4869744c5836a` and published v1.22.0.
The user's request covers release files/rules; it does not request publishing or adding
upstream's signing, updater, SwiftUI/WinUI, Android, or Linux ARM64 features.

- [x] Use a shared native asset-name contract in staging, publishing and download tables.
- [x] Read versioned release notes/title; reject missing/mismatched notes before native jobs.
- [x] Preserve existing draft edits and all privacy/platform completion gates.
- [x] Update release templates and documented maintenance/download steps with exact references.
- [x] Verify offline naming/notes/publisher tests, default checks and workflow syntax.

Files added: `scripts/release-assets.json`, `scripts/release_notes.cjs`, its Node tests,
and `.github/release-notes/v0.1.0.md`. Existing staging/publisher tests first failed against
the new native filenames/custom title before the implementation changes. Notes tests cover
missing/empty notes, version mismatches, unsafe coordinates, CRLF input, generated links,
and no title/body echoing to the CLI log. A new privacy CLI test rejects synthetic
credentials in a release document without printing them.

Independent review found a title echo before the privacy gate; a reproducing CLI test
failed, then passed after the log was limited to the validated tag. The workflow now scans
the notes before parsing. Re-review found no remaining actionable findings.

Validation for this follow-up:

- Python 3.12 full suite: PASS, 96 tests, 2 existing symlink privilege skips.
- Node release/build tests: PASS, 37 tests. Node 22.14.0 notes/publisher run: PASS, 32 tests.
- `npm run build`: PASS, existing Vite chunk-size warning only.
- `cargo test --locked --workspace -j 1`: PASS, 2057 passed, 0 failed, 21 ignored.
- Initial full `npm test` hit the existing 5-second timeout in
  `ProjectMigrationPreviewDialog.test.tsx` / `caps explicit file selection at 32, rejects
  unsafe metadata and never sends directory entries`. Its isolated file rerun passed all
  18 tests without changing code or timeout. A second default-concurrency run failed
  `PluginsSettings.test.tsx` / `closes only the top package preview on immediate Escape`;
  that file's isolated rerun passed all 6 tests. No UI code, assertion or timeout was
  changed. Full-suite verification with bounded worker concurrency is recorded below;
  the default-concurrency instability remains a separate existing test concern.
- actionlint: PASS. No desktop binary/resource/runtime changes in this follow-up; the
  prior privacy-checked Windows installers remain the native build evidence. Linux/macOS
  builds, installation acceptance and remote publication remain unexecuted.
- Final full `npm test` with `VITEST_MAX_THREADS=4`, `VITEST_MIN_THREADS=1`,
  `VITEST_MAX_FORKS=4`, `VITEST_MIN_FORKS=1`: PASS, 1555 Vitest + 22 extension + 37
  release/build tests. Worker limits were process-local; no repository test concurrency,
  assertion, timeout or UI behavior was changed. Both default-concurrency failures above
  remain recorded rather than being treated as successful runs.
- Rendered v0.1.0 notes offline with an example repository: six native installer links
  plus both metadata links are present, with no unresolved placeholders. Source and
  rendered notes passed privacy checks. Scoped `git diff --check`: PASS.

## Integration verification — 2026-10-28

The user explicitly requested functional commits, integration into `main`, and pushing
`main` to the existing origin. The existing feature history is retained; this does not
authorize a version tag or a GitHub Release. The pending changes are grouped into settings
and model configuration, native platform support, build privacy, and release automation/docs.

- `npm ci`: PASS after the restricted npm process failed with an exit-handler error.
  The unchanged npm dependency lock reports 13 advisories (4 low, 3 moderate, 4 high,
  2 critical); no automatic dependency upgrades were applied during integration.
- Python 3.12 full script suite: PASS, 96 tests, 2 existing symlink privilege skips.
- `npm run build`: PASS, existing Vite chunk-size warning only.
- `cargo test --locked --workspace -j 1`: PASS, 2057 passed, 0 failed, 21 ignored.
- The first bounded-worker `npm test` run, concurrent with native compilation, timed out
  the existing `DesktopApp` test `routes search settings actions when Settings is already
  on another section` at 5 seconds. It passed 1554 other tests; the extension and release
  test stages were not reached. This failed run is retained in the local verification log.
- `cargo fmt --all -- --check`, workflow actionlint, and diff whitespace checks: PASS.
- Independent settings review found no new blocker. The privacy review found no real
  credential, local user configuration, database, log, or installer in candidate Git
  changes; synthetic redaction fixtures are retained. Release input checks passed.
- The timed-out frontend case passed in isolation after native compilation (one test
  passed, 176 unrelated tests skipped). The subsequent full `npm test` passed all
  1555 Vitest tests in 130 files, 22 extension tests, and 37 release/build tests with
  the same process-local four-worker limits. No assertion, timeout, UI code, or checked-in
  test concurrency was changed to obtain this result.
- `npm run build:desktop -- -- --locked`: PASS with Python 3.12, including resource and
  raw-executable privacy gates, followed by both Windows NSIS EXE and MSI bundles.
  The release compilation completed in 4m 54s. Native Linux/macOS builds, installation,
  and real model/SSH acceptance remain unexecuted locally.

## First cloud execution — 2026-10-28

The user requested execution of the cloud build and Release workflow after `main`
was pushed. The first CI run (`40660363376`, commit `4693d07`) exposed two native
runner assumptions before a release tag was created:

- Windows/macOS Python privacy tests passed an aliased temporary root while the
  scanner canonicalized only its root. Build inputs now map the caller's root alias
  to the canonical root without resolving away descendant symlinks/junctions.
  External inputs and parent traversal remain rejected. Fixtures use canonical roots;
  three new regressions cover alias scanning, linked descendants, and traversal.
- The HTTP feature contract forced offline Windows dependency resolution on Linux,
  requiring an uncached Windows-only crate. The test now queries its Cargo toolchain's
  native host and resolves that platform, retaining the offline/locked and system-proxy
  feature assertions for both desktop and adapter clients.

Local focused validation: Python 3.12 full suite PASS (99 tests, 2 existing symlink
privilege skips); native HTTP feature contract PASS (1 test); resource/release-notes
privacy and diff checks PASS. The next cloud run must verify the fixes on real runners.

The next ordinary CI (`40664597955`, `da459b8`) passed on Linux. macOS progressed past
Python and exposed the same runner temporary-directory alias in 85 desktop Rust tests:
the application's intentional ancestor-link checks rejected those fixture paths before
their read/budget assertions. CI and release setup now export the canonical existing
macOS temporary directory as `TMPDIR` before the unchanged suites run. No runtime path
policy, symlink rejection test, or test timeout is relaxed. Native reruns remain the
confirmation for this environment-only adjustment.
