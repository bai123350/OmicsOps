# OmicsOps Autonomous Agent Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use
> superpowers:subagent-driven-development or superpowers:executing-plans.

**Goal:** Deliver a Windows Tauri desktop application that autonomously runs
approved bioinformatics stages on a remote Linux server.

**Architecture:** A React webview calls a trusted Rust control plane. The Rust
core persists state locally and manages an auditable, resumable remote project
over SSH without a daemon.

**Tech Stack:** Rust, Tauri 2, React, TypeScript, SQLite, SSH2, reqwest,
Micromamba, Scanpy, R, Seurat.

## Global Constraints

- Remote work uses a dedicated low-privilege SSH user.
- No raw analysis data is uploaded from the desktop.
- Three distinct autonomous repair attempts are allowed per failed step.
- SeuratDisk is forbidden in the h5ad-to-Seurat converter.
- Windows is the only packaged desktop target for v1.

## Tasks

1. Build and test the core domain model, state machine, policy, and remote
   project layout.
2. Build and test credentials, SQLite persistence, SSH/SFTP, document parsing,
   and OpenAI-compatible structured planning.
3. Build and test the autonomous runner, recovery, approvals, downloads, and
   artifact checks.
4. Add the Scanpy reference workflow and self-authored R conversion/validation
   scripts.
5. Expose typed Tauri commands/events and implement the five React workflows.
6. Run Rust, frontend, build, security, and end-to-end fixture verification.
