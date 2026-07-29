#!/usr/bin/env python3
"""Reproducible Scanpy acceptance analysis for the public 10x PBMC 3k data."""

from __future__ import annotations

import json
from pathlib import Path

import matplotlib

matplotlib.use("Agg")
import numpy as np
import pandas as pd
import scanpy as sc
from scipy import sparse


ROOT = Path(__file__).resolve().parents[1]
INPUT = ROOT / "work/pbmc3k/filtered_gene_bc_matrices/hg19"
RESULTS = ROOT / "results"
FIGURES = RESULTS / "figures"
RESULTS.mkdir(parents=True, exist_ok=True)
FIGURES.mkdir(parents=True, exist_ok=True)
sc.settings.figdir = FIGURES
sc.settings.verbosity = 2
sc.set_figure_params(dpi=100, facecolor="white")


def robust_upper(values: pd.Series, hard_cap: float | None = None) -> float:
    q1, q3 = np.quantile(values, [0.25, 0.75])
    threshold = float(q3 + 3.0 * (q3 - q1))
    return min(threshold, hard_cap) if hard_cap is not None else threshold


def robust_lower(values: pd.Series, hard_floor: float) -> float:
    q1, q3 = np.quantile(values, [0.25, 0.75])
    return max(float(q1 - 3.0 * (q3 - q1)), hard_floor)


adata = sc.read_10x_mtx(INPUT, var_names="gene_symbols", cache=False)
adata.var_names_make_unique()
adata.var["mt"] = adata.var_names.str.upper().str.startswith("MT-")
sc.pp.calculate_qc_metrics(
    adata,
    qc_vars=["mt"],
    percent_top=None,
    log1p=False,
    inplace=True,
)

thresholds = {
    "min_genes": int(robust_lower(adata.obs["n_genes_by_counts"], 200)),
    "max_genes": int(robust_upper(adata.obs["n_genes_by_counts"])),
    "max_counts": int(robust_upper(adata.obs["total_counts"])),
    "max_pct_mt": float(robust_upper(adata.obs["pct_counts_mt"], 20.0)),
}
before_cells, before_genes = adata.n_obs, adata.n_vars
keep = (
    (adata.obs["n_genes_by_counts"] >= thresholds["min_genes"])
    & (adata.obs["n_genes_by_counts"] <= thresholds["max_genes"])
    & (adata.obs["total_counts"] <= thresholds["max_counts"])
    & (adata.obs["pct_counts_mt"] <= thresholds["max_pct_mt"])
)
adata = adata[keep].copy()
sc.pp.filter_genes(adata, min_cells=3)

if not sparse.issparse(adata.X):
    adata.X = sparse.csr_matrix(adata.X)
adata.layers["counts"] = adata.X.copy()
sc.pp.normalize_total(adata, target_sum=1e4)
sc.pp.log1p(adata)
adata.layers["lognorm"] = adata.X.copy()
adata.raw = adata
sc.pp.highly_variable_genes(adata, n_top_genes=2000, flavor="seurat", subset=False)
sc.tl.pca(adata, use_highly_variable=True, svd_solver="arpack")
sc.pp.neighbors(adata, n_neighbors=15, n_pcs=min(40, adata.obsm["X_pca"].shape[1]))
sc.tl.umap(adata, random_state=0)
sc.tl.leiden(adata, resolution=0.8, key_added="leiden", random_state=0)
sc.tl.rank_genes_groups(adata, groupby="leiden", method="wilcoxon")

marker_sets = {
    "T cells": ["CD3D", "CD3E", "TRBC1"],
    "B cells": ["MS4A1", "CD79A", "CD37"],
    "NK cells": ["NKG7", "GNLY", "KLRD1"],
    "Monocytes": ["LYZ", "S100A8", "CTSS"],
    "Dendritic cells": ["FCER1A", "CST3"],
}
score_columns = []
for label, genes in marker_sets.items():
    present = [gene for gene in genes if gene in adata.var_names]
    column = f"marker_score_{label.replace(' ', '_').lower()}"
    if present:
        sc.tl.score_genes(adata, present, score_name=column, random_state=0)
        score_columns.append((label, column))

if score_columns:
    scores = adata.obs[[column for _, column in score_columns]].to_numpy()
    labels = np.array([label for label, _ in score_columns], dtype=object)
    best = scores.argmax(axis=1)
    adata.obs["cell_type"] = pd.Categorical(labels[best])
else:
    adata.obs["cell_type"] = pd.Categorical(["Unassigned"] * adata.n_obs)

adata.uns["omicsops"] = {
    "workflow": "scrna-pbmc-reference-v1",
    "qc_thresholds": thresholds,
    "cells_before": int(before_cells),
    "genes_before": int(before_genes),
    "cells_after": int(adata.n_obs),
    "genes_after": int(adata.n_vars),
}

sc.pl.violin(
    adata,
    ["n_genes_by_counts", "total_counts", "pct_counts_mt"],
    multi_panel=True,
    show=False,
    save="-qc.png",
)
sc.pl.umap(adata, color=["leiden", "cell_type"], show=False, save="-clusters.png")

output = RESULTS / "pbmc3k.h5ad"
adata.write_h5ad(output, compression="gzip")
with (RESULTS / "qc-summary.json").open("w", encoding="utf-8") as handle:
    json.dump(adata.uns["omicsops"], handle, indent=2)
print(json.dumps({"h5ad": str(output), **adata.uns["omicsops"]}, indent=2))
