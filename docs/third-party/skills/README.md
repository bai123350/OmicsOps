# Bundled Skill provenance and licenses

Skill package files are stored directly under the repository's `skills/`
directory. This directory retains the source snapshots' provenance, license
texts, third-party notices and original bundle metadata:

- [`single-cell/`](single-cell/): nine packages from scientific-agent-skills,
  with their original MIT license.
- [`wisp-science/`](wisp-science/): 26 packages with the recorded AGPL-3.0 and
  per-file Apache-2.0 declarations and notices.

The active installation metadata is `skills/BUNDLE.json`. Original `BUNDLE.json`
files retained here document each source and are not loaded by the application.
Wisp `SOURCE.json` file paths resolve relative to the repository's `skills/`
directory; its hashes continue to identify the unchanged package bytes.

Desktop installers package this provenance directory as `licenses/skills/`.
