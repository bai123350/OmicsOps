# OmicsOps

OmicsOps is a Windows-first Tauri desktop agent that plans and runs
bioinformatics workflows on a remote Linux server over SSH.

The Rust control plane owns credentials, approvals, policy enforcement, task
state, SSH, and audit records. Analysis data stays on the remote server.

## Development

Prerequisites:

- Rust stable
- Node.js 22 or newer
- Windows WebView2

```powershell
npm install
cargo test --workspace
npm test
npm run tauri dev
```

See `docs/superpowers/specs/2026-07-29-omicsops-agent-design.md` for the
approved design.

The V2 control plane uses versioned tool contracts, canonical plan approvals,
append-only hash-chained events, verified checkpoints, process-group recovery,
and auditable run bundles. See `acceptance/README.md` for the pinned PBMC and
bulk RNA-seq acceptance procedure.
