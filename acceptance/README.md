# OmicsOps acceptance workflows

`bulk-rnaseq.json` freezes the six-sample yeast comparison from
`nf-core/test-datasets` at commit `72a702d346833d5523bc40d032323ea548603b00`.
The paired FASTQ files live under `testdata/GSE110004/{accession}_{1,2}.fastq.gz`.

The automated suite validates the fixture, plan/tool contracts, migration,
approval hashing, audit chaining, shell quoting, process-group cancellation,
resource guards, validators, and recovery decisions. A real Linux SSH host is
still required to run the PBMC and bulk workflows end to end. Such runs must
verify h5ad/Seurat RDS or bulk tables/figures/report, capture the Micromamba
explicit lock, interrupt once during execution, and export the final run bundle.

## Harness v3 dynamic PBMC3k acceptance

`harness_v3::tests::live_harness_v3_dynamically_delivers_pbmc3k_without_repository_workflow`
is ignored by default. It starts from a caller-provided, existing, empty and
disposable remote directory. The model must inspect/obtain the input, create a
project-local environment, generate task-specific code, repair failures, and
artifact-verify the counts/QC/seed/version records, h5ad, table, figure, HTML,
and hash manifest. The test rejects references to the repository's bundled PBMC
workflow; those assets are not an acceptance shortcut.

Run it explicitly only with a low-privilege SSH account and a pinned host key:

```text
OMICSOPS_LIVE_SSH_HOST
OMICSOPS_LIVE_SSH_PORT                 # optional, defaults to 22
OMICSOPS_LIVE_SSH_USER
OMICSOPS_LIVE_SSH_PASSWORD
OMICSOPS_LIVE_SSH_FINGERPRINT
OMICSOPS_LIVE_PBMC_ROOT                # must already exist and be empty
OMICSOPS_LIVE_MODEL_PROTOCOL           # anthropic | openai | ollama
OMICSOPS_LIVE_MODEL_BASE_URL
OMICSOPS_LIVE_MODEL_NAME
OMICSOPS_LIVE_MODEL_CREDENTIAL         # omitted only for Ollama
```

Then run:

```text
cargo test -p omicsops-desktop live_harness_v3_dynamically_delivers_pbmc3k_without_repository_workflow -- --ignored --exact --nocapture
```

An ignored result means the live SSH/model/MCP acceptance was not executed and
must not be reported as passed.

## Agent Runtime V4 stage-1 acceptance

`agent_v4::tests::live_v4_model_plan_and_persistent_ssh_python_kernel` uses the
same explicit live model/SSH variables listed above. It asks the real model for
an `ExecutionPlanV4`, validates its canonical SHA-256 contract, then launches a
run-scoped SSH JSONL Python kernel. Cell 1 defines `v4_probe`; cell 2 prints that
variable without redefining it. The test requires the same kernel session and
process identity for both cells.

Run only against an empty disposable remote root:

```text
cargo test -p omicsops-desktop live_v4_model_plan_and_persistent_ssh_python_kernel -- --ignored --exact --nocapture
```

The V4 route must not become the default new-task route until this live test
actually passes. An ignored result is not acceptance.
