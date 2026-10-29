# Bundled Agent Skills

Every bundled package is a direct child of this folder:
`skills/<skill-name>/SKILL.md`. Each package retains its own scripts, references
and other resources. There are no source or category directories between
`skills/` and an individual package.

`BUNDLE.json` records first-install defaults, retired package names and internal
host compatibility categories. These categories do not create folders or groups
in Settings. Existing enable/disable choices are preserved by the application.

The 35 packages comprise nine scientific-agent-skills packages and 26 Wisp
Science packages. Their pinned provenance and original license notices live in
[`docs/third-party/skills/`](../docs/third-party/skills/), and are packaged under
`licenses/skills/` in desktop installers. Upstream package bytes are retained.

The application imports packages into its content-addressed application-data
store. Loading a Skill supplies guidance and resources; it does not install
scientific dependencies, configure credentials, execute code or grant permissions.
