# OmicsOps Autonomous Bioinformatics Agent Design

## Goal

Build a Windows-first Tauri 2 desktop application that accepts an uploaded
analysis plan, connects to a dedicated low-privilege Linux SSH account, and
autonomously completes the analysis inside an empty remote project directory.

## Architecture

React renders five workflows: connection, project creation, plan approval,
run monitoring, and artifact browsing. Rust is the trusted control plane for
credentials, SQLite state, OpenAI-compatible planning, SSH/SFTP, policy,
approvals, audit events, recovery, and downloads.

The remote host has no daemon. Each project contains `.omicsops`, `scripts`,
`logs`, `state`, `work`, and `results`. Background steps persist PID, exit
status, logs, and checksums so the desktop can reconnect without rerunning
completed work.

## Autonomy and Safety

The user approves stage-level goals. Within the project directory and its
dedicated Micromamba environment, the agent may install dependencies, change
scripts, adjust parameters, switch tools, and try at most three distinct repair
strategies. Sudo, system writes, project-external destructive operations,
secret access, and undeclared exfiltration are rejected. Overwriting successful
artifacts, materially raising resources, or using new private credentials
requires approval.

The dedicated low-privilege SSH account is the hard sandbox. Command policy is
defense in depth and is not represented as an operating-system sandbox.

## scRNA-seq Acceptance Workflow

A public 10x PBMC dataset is downloaded remotely and processed with Scanpy
through QC, filtering, normalization, HVG, PCA, neighbors, UMAP, Leiden,
markers, and basic annotation. The workflow emits a validated `.h5ad`, an
environment lock, figures, report, and audit log.

An in-repository R converter reads h5ad HDF5/sparse structures without
SeuratDisk, constructs a Seurat object, saves `.rds`, and verifies counts,
metadata, reductions, clusters, and annotations against the h5ad source.

## Initial Boundaries

The first release excludes OCR, Slurm, bastion/MFA, multi-user operation,
private data providers, automatic large-result sync, and h5Seurat.
