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
