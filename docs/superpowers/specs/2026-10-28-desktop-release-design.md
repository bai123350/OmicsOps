# Desktop builds and draft GitHub Releases

Date: 2026-10-28. The user approved implementing the desktop release proposal in this chat.

## Intent and scope

Produce installable OmicsOps desktop packages on native GitHub runners, with a
reviewable draft Release after every required build and deterministic check succeeds.
Windows remains the primary platform; Linux and macOS are experimental until installed
application checks have been recorded. This change prepares the pipeline; it does not
create a Git tag, run a remote workflow, publish a release, or assert platform acceptance.

## Approaches

We considered independent platform release workflows (as in wisp-science), a single
matrix with final aggregation, and manual local packaging/upload. The approved proposal
uses a matrix and an aggregation job: a failed or incomplete platform cannot produce a
new release. Manual uploads remain possible but are not the normal path. Wisp's native
builds and fixed-tag provenance are retained; commercial signing, updater feeds, CDN
distribution, and extra clients are outside this increment.

## Build contract

| Target ID | Runner | Rust target | Required files |
| --- | --- | --- | --- |
| windows-x64 | windows-latest | x86_64-pc-windows-msvc | NSIS exe, MSI |
| linux-x64 | ubuntu-22.04 | x86_64-unknown-linux-gnu | deb, AppImage |
| macos-arm64 | macos-15 | aarch64-apple-darwin | dmg |
| macos-x64 | macos-15-intel | x86_64-apple-darwin | dmg |

Use Node 22, Python 3.12, npm ci, and locked Cargo dependency resolution. Generate dist
before compiling the desktop crate. Each native job runs Python regressions, V4 scenario
checks, Web build, workspace Rust tests, frontend tests, and desktop packaging. Retain
scenario reports independently from distributable assets. Existing resource mappings
remain intact. Configure bundled icons and platform-specific bundle targets.

Windows packages are initially unsigned; macOS uses ad-hoc signing, not notarization.
The release notes must state this and the experimental platform limitations. No automatic
updater is enabled. Developer ID/notarization and Windows signing require a later,
explicitly configured signing service; do not invent credentials or silently claim signing.

## Version, assets, and publication

The release helper validates the root npm version, lockfile root version, workspace
Cargo version, workspace lockfile packages, and Tauri version. A release tag must equal
v<version>. Only numeric X.Y.Z versions within MSI limits are supported initially;
GitHub prerelease status is independent and defaults to true.

Pushes of v* tags build a draft release. A manual run with no tag builds artifacts only;
a manual run naming an existing tag may create/recover its draft. Resolve the requested
ref once and pin all subsequent checkouts to the resulting commit. Validate the tag still
points at that commit before release creation. Never let a tool invent a missing tag.

Stage only the required nonempty installer files under unique, predictable names.
Per-target JSON manifests record version, target, commit, byte sizes, and SHA-256 hashes.
Aggregation must require exactly the four target manifests, six installers, matching
versions/commits, safe basenames, and matching sizes/hashes. Produce SHA256SUMS.txt and
a combined release-manifest.json; arbitrary files, source trees, databases, and logs are
not release assets. Reruns refuse published releases and drafts from another commit;
release mutation is serialized by requested ref and preserves manually edited draft notes.

Only the final release job has contents:write. Release creation stays draft and prerelease;
publication is a deliberate action in GitHub after installed application validation.

## Platform boundaries

Configure real OS credential stores: Windows Credential Manager, macOS Keychain,
Linux Secret Service with encrypted D-Bus transport. No plaintext/SQLite fallback. Tests
must not require an unlocked real keyring. Linux needs a working desktop Secret Service.
Fix deterministic test fixtures that accidentally assume Windows paths while preserving
the existing product restrictions. Do not widen Claude subscription process privileges or
port Windows filesystem safety code merely to make packaging pass.

Document Windows-only Claude Code subscription and offline example creation, Linux
browser discovery limitations, and installed GUI PATH requirements for python/Rscript.
Local and SSH execution remain separate; no scientific runtime or large data is bundled.

## Verification

Release files and notes follow the desktop conventions observed in wisp-science at
`d783ce154e70905a783f7ed461f4869744c5836a`: keep Tauri-style platform suffixes, store
reviewed notes in `.github/release-notes/vX.Y.Z.md`, and optionally resolve a custom title
from a `release-title` comment. Missing notes block a tagged release early. A shared
asset-name contract drives staging, upload validation and the download table. Notes link
to the current repository rather than copying upstream download URLs. Existing draft
notes remain untouched on reruns. OmicsOps retains the approved draft-only publication,
all-platform validation and privacy gates; upstream signing/updater/native-preview/mobile
capabilities are not assumed. No tag or remote release is created by implementing this.

The user's distribution constraint is a pristine application without maintainer API keys,
accounts, connection profiles, conversations, data, or personal build paths. Release
builds use clean hosted runners and an explicit tracked resource allowlist. Local release
builds remap compiler paths, disable debug information and development-directory fallbacks,
and do not expose dotenv/prefixed environment values to frontend assets. A privacy gate
scans resources, frontend assets and the uncompressed executable before invoking the
installer bundler; it reports rule IDs without matching values. Extra packager inputs must
be explicitly reviewed rather than bypassing that resource gate. Pattern checks supplement
source/resource review and do not certify every conceivable secret format. Fresh-state
acceptance uses a new OS account/VM; retained user data is never erased for this check.

Use offline tests for version drift, malformed tags, missing/duplicate/tampered installers,
wrong commit/target, and published-release refusal. Run the default repository checks
and Windows desktop packaging locally. GitHub Linux/macOS builds, installed application
smokes, real keyring persistence, real model and SSH/R/PBMC acceptance must be explicitly
reported as unexecuted until run; artifacts alone are not acceptance evidence.
