#!/usr/bin/env bash
set -Eeuo pipefail

url="https://cf.10xgenomics.com/samples/cell-exp/1.1.0/pbmc3k/pbmc3k_filtered_gene_bc_matrices.tar.gz"
archive="work/pbmc3k/pbmc3k_filtered_gene_bc_matrices.tar.gz"
destination="work/pbmc3k"

mkdir -p "$destination"
curl --fail --location --retry 5 --retry-all-errors \
  --continue-at - --output "$archive" "$url"
sha256sum "$archive" > "${archive}.sha256"
tar -xzf "$archive" -C "$destination"
test -s "$destination/filtered_gene_bc_matrices/hg19/matrix.mtx"
