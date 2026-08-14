# Bundled single-cell Agent Skills

These packages are vendored from `GPTomics/bioSkills` at commit
`2b459ce66acafa62673abd95277dc4b41b9ec130` under the upstream MIT license.
`SOURCE.json` is the machine-readable provenance record.

The desktop application discovers child directories containing `SKILL.md`,
content-addresses them into its application-data Skill store, and displays
them in Settings. Workflow packages and dependencies declared by their
frontmatter are enabled automatically. The Agent receives every text/code
file in the enabled packages and generates project-specific analysis code at
runtime; files under this directory are instructions and examples, not a
fixed PBMC execution script.
