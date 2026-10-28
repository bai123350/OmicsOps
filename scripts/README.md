# Compiled model catalog

`src-tauri/src/model_catalog.json` is a normalized snapshot of the public
[models.dev API](https://models.dev/api.json), retrieved on 2026-09-21.
The artifact and every capability snapshot record the SHA-256 of the source.
The snapshot contains 616 model entries.
Only the nine explicitly mapped provider endpoints with supported protocols and
positive text-generation limits are included. No model family/prefix matching.
OpenCode Go rows also bind the exact `/zen/go/v1` base path and use an explicit
per-model Chat Completions or Anthropic Messages allowlist; Responses API rows
are excluded.
This single-source refresh also follows the current upstream catalog for the
previously compiled providers: retired rows disappear, capability rows may
change, and the MiniMax China endpoint now uses the upstream host. Saved
profiles retain their persisted snapshots until an explicit refresh.
Price data, automatic model routing, image token costs and runtime effective
reasoning reports are not implemented by this increment.

To intentionally update, download the public API JSON into a temporary file, then
run on Windows or macOS from the repository root:

```text
python scripts/import_model_catalog.py SOURCE_JSON src-tauri/src/model_catalog.json
python -B -m unittest discover -s scripts -p "test_*.py"
cargo test -p omicsops-desktop --lib model_
```

Review the generated diff and the source checksum before committing. The importer
itself, builds, tests and application startup do not access models.dev. Existing
profiles keep their snapshots; explicitly select the catalog refresh option when
editing in Settings to adopt new data. Recreating a profile also adopts it.
API host matching follows the application's provider contract; model IDs are
case sensitive and gateway prefixes are significant. A model listing is capability
metadata, not proof that an account can access that model.

# Offline Python regressions

From the repository root, run the bundled helper suite first, then all script
regressions through standard-library unittest discovery:

```text
python -B -m unittest discover -s scripts -p "test_bundled_skill_helpers.py" -v
python -B -m unittest discover -s scripts -p "test_*.py" -v
```

The unified command includes model catalog, remote job bridge, kernel inspection,
bundled helper and release validation tests. Use Python 3.12+ for this suite. CI on
Windows, Linux and macOS uses Python 3.12 after `npm ci` and before `npm run build`,
then `cargo test --locked --workspace` and `npm test` on Node 22.
The Web build generates `dist`, required when compiling the desktop crate; CI
also retains the V4 scenario gate and always-upload report before these gates.
The Python tests use the standard library; no pip install, real credentials,
network, SSH, GPU or R is required. `-B` avoids generating `.pyc` files.

`test_bundled_skill_helpers.py` loads only the fixed bundled `paper-narrative`,
`literature-review` and `figure-style` runtime sources into fresh `__main__`
namespaces. It checks claim/path pairing, literature success versus failure,
unknown DOI verification, safe errors, load side effects and crop coordinates
with isolated environment, HTTP and plotting fixtures. `panel_crops` returns
coordinates only; these tests never crop or write images. Discovery requires all
three nonempty behavior groups. A local deliberately failing case must yield an
unsuccessful `TestResult`; it is excluded from normal discovery.

Bundle/hash checks establish packaged source identity. Helper PASS establishes
behavior under controlled inputs. Scripted V4 PASS establishes only the runtime
contracts exercised by its own fixtures; helper output is not part of V4 model
call counts or suite hashes. The W08 V4 scenario manifest, runner and report are delivered; see
[deterministic scenario guidance](../docs/testing/v4-scenarios.md). Local PASS does not
mean the updated GitHub CI has run or that all 26 Skills, scientific dependencies
or scientific conclusions have been validated. Real model/SSH/R/PBMC acceptance
remains opt-in and unexecuted for this increment; follow
[`acceptance/README.md`](../acceptance/README.md) separately.

# Desktop release helpers

`build_desktop.cjs` is the `npm run build:desktop` entry point. It removes credential-like
environment inputs, remaps local Rust paths, builds the application without installers,
and only runs the native bundler after privacy checks pass. It requires Python 3.12+
(`OMICSOPS_BUILD_PYTHON` can select the executable). Its tests include an actual Vite
build with synthetic environment secrets, plus the audit-before-packaging boundary.

`check_release_privacy.py` checks the approved, tracked resource set and optionally
`--dist dist --binary target/release/omicsops-desktop.exe`. It rejects private file types,
known credential forms and current build-machine paths without printing matching values.
It never reads the OS credential vault or user database. These deterministic rules do not
identify every possible personal text; use clean CI builds and review new resources.

`release.py` validates repository versions, stages native installers, and assembles
the complete four-target release with SHA-256 checksums. It uses only Python 3.12's
standard library and does not contact GitHub. Run `python -B scripts/release.py --help`
for its three commands and `python -B -m unittest discover -s scripts -p test_release.py`
for offline fixture tests.

`release-assets.json` defines the shared Tauri-style filename suffixes used by staging,
publisher validation and download tables. `release_notes.cjs` reads
`.github/release-notes/vX.Y.Z.md`, resolves its optional `release-title` comment, and
renders download links for the specified repository. Use `--tag vX.Y.Z --repository owner/repo`
to validate notes, with optional `--output <new-file>` to preview the rendered body offline.
Tagged workflows also run `check_release_privacy.py --document <notes-file>` before native builds.

`github_release.cjs` is the GitHub Actions publisher. It validates the prepared files
and existing tag, and only creates/resumes a draft prerelease for the same commit.
`npm run test:release` runs its Node tests against a simulated GitHub client; these are
also part of `npm test` and excluded from the browser-oriented Vitest runner.

See [desktop releasing](../docs/releasing.md) for native prerequisites, manual builds,
tag/version rules, draft recovery, signing limitations and installed-app checks.
