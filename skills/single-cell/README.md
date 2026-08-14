# Bundled single-cell Agent Skills

The nine package directories in this folder are unmodified upstream packages
from [`K-Dense-AI/scientific-agent-skills`](https://github.com/K-Dense-AI/scientific-agent-skills)
at commit `13385c7c4db02fdcc84a020752c07cce91ef780e`. The repository had 33,460
GitHub stars when the snapshot was selected on 2026-08-14 and is distributed
under the MIT license. `SOURCE.json` records the pinned provenance.

`BUNDLE.json` is OmicsOps-owned installation metadata, not an Agent Skill. It
selects the core packages enabled on first install and retires packages from the
previous bundled source. Advanced packages remain available in Settings and can
be enabled when the task calls for RNA velocity, scVI models, GRN inference, or
additional statistical analysis.

The desktop application content-addresses every directory containing a
`SKILL.md` into its application-data Skill store. The Agent receives the full
text and code references of enabled packages and generates task-specific
analysis code after inspecting the actual project data and environment.
