# Wisp Science skill snapshot

All 26 skill directories are copied byte-for-byte from
https://github.com/xuzhougeng/wisp-science at commit
`3628a4209e494ba6fbef1095bb964782f7d2c430` (retrieved 2026-09-11).
See `SOURCE.json` for file hashes and license declarations. Upstream's root
AGPL-3.0 license, Apache-2.0 text and third-party notices are retained here.
Individual explicit upstream license declarations remain in each SKILL.md.
OmicsOps-owned files in this directory are BUNDLE.json, SOURCE.json,
.gitattributes and this README. Snapshot bytes are preserved across checkouts.

Installing a skill copies guidance and resources; it never installs Python/R,
third-party CLIs, Word, Zotero or environments, configures credentials, executes
code, enables an MCP server or grants permissions.

The 13 research-oriented defaults are listed in BUNDLE.json. The remaining 13
are available in Settings but disabled initially: they depend on Wisp-specific
host features, optional external tools or unusually large workflows. Enabling
them does not provide those capabilities. Existing user enable/disable choices
are retained on startup. Wisp itself defaults to enabling all discovered skills.

Runtime guidance is added by OmicsOps outside the original snapshot. It explains
the actual tool and execution boundaries. `use_skill` supplies a resource
manifest; request an exact `Resource: <relative-path>` section to load a script
or reference. This works without exposing app-data paths to local/SSH file
tools. Resource loading only returns text. Sidecars must be explicitly loaded
through the approved persistent runtime; standalone scripts can be materialized
under the active project through the normal approved file-write tools.

In particular, Wisp's interpreter selection, project `.wisp` discovery,
specialists, theme import, automatic Run polling/cancellation and external
credential-file instructions are not OmicsOps capabilities. The Word/Zotero
skill requires the separately installed `zotero_mcp.word_citations` package;
the InfiniSynapse skill requires its CLI and service. Never write credentials
into the files those upstream examples describe: use existing host credential
references and the configured keyring.
