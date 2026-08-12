from __future__ import annotations

import json
import sys
from pathlib import Path

import matplotlib

matplotlib.use("Agg")
import matplotlib.pyplot as plt
import numpy as np
import pandas as pd
import scanpy as sc
from scipy import sparse


cfg = json.loads(sys.argv[1])
input_dir = Path(cfg["input_directory"])
outputs = cfg["outputs"]
qc = cfg.get("qc", {})
normalization = cfg.get("normalization", {})
embedding = cfg.get("embedding", {})
clustering = cfg.get("clustering", {})
annotation = cfg.get("annotation", {})

for path in outputs.values():
    Path(path).parent.mkdir(parents=True, exist_ok=True)

adata = sc.read_10x_mtx(input_dir, var_names="gene_symbols", cache=False)
if cfg.get("make_var_names_unique", True):
    adata.var_names_make_unique()
mt_prefix = str(qc.get("mitochondrial_prefix", "MT-")).upper()
adata.var["mt"] = adata.var_names.str.upper().str.startswith(mt_prefix)
sc.pp.calculate_qc_metrics(adata, qc_vars=["mt"], percent_top=None, log1p=False, inplace=True)

keep = np.ones(adata.n_obs, dtype=bool)
if qc.get("min_genes") is not None:
    keep &= adata.obs["n_genes_by_counts"].to_numpy() >= int(qc["min_genes"])
if qc.get("max_genes") is not None:
    keep &= adata.obs["n_genes_by_counts"].to_numpy() <= int(qc["max_genes"])
if qc.get("min_counts") is not None:
    keep &= adata.obs["total_counts"].to_numpy() >= int(qc["min_counts"])
if qc.get("max_counts") is not None:
    keep &= adata.obs["total_counts"].to_numpy() <= int(qc["max_counts"])
if qc.get("max_mito_percent") is not None:
    keep &= adata.obs["pct_counts_mt"].to_numpy() <= float(qc["max_mito_percent"])
adata = adata[keep].copy()
sc.pp.filter_genes(adata, min_cells=int(qc.get("min_cells_per_gene", 3)))
if adata.n_obs < 3 or adata.n_vars < 3:
    raise RuntimeError("QC retained too few cells or genes for clustering")

if not sparse.issparse(adata.X):
    adata.X = sparse.csr_matrix(adata.X)
adata.layers["counts"] = adata.X.copy()
sc.pp.normalize_total(adata, target_sum=float(normalization.get("target_sum", 10000)))
if normalization.get("log1p", True):
    sc.pp.log1p(adata)
adata.raw = adata
hvg = normalization.get("highly_variable_genes", {})
sc.pp.highly_variable_genes(
    adata,
    n_top_genes=min(int(hvg.get("n_top_genes", 2000)), adata.n_vars),
    flavor=str(hvg.get("flavor", "seurat")),
    subset=False,
)
if normalization.get("scale", True):
    sc.pp.scale(adata, max_value=10)
n_pcs = min(int(embedding.get("pca_components", 50)), adata.n_obs - 1, adata.n_vars - 1)
sc.tl.pca(adata, n_comps=max(2, n_pcs), use_highly_variable=True, svd_solver="arpack")
sc.pp.neighbors(adata, n_neighbors=min(int(embedding.get("neighbors", 15)), adata.n_obs - 1), n_pcs=n_pcs)
sc.tl.umap(adata, random_state=int(clustering.get("random_seed", 0)))
sc.tl.leiden(
    adata,
    resolution=float(clustering.get("resolution", 0.8)),
    key_added="cluster",
    random_state=int(clustering.get("random_seed", 0)),
)
sc.tl.rank_genes_groups(adata, groupby="cluster", method="wilcoxon")

marker_sets = annotation.get("marker_sets", {})
score_columns: list[tuple[str, str]] = []
for index, (label, genes) in enumerate(marker_sets.items()):
    present = [gene for gene in genes if gene in adata.var_names]
    if not present:
        continue
    column = f"omicsops_marker_{index}"
    sc.tl.score_genes(adata, present, score_name=column, random_state=0)
    score_columns.append((label, column))

cluster_rows = []
score_rows = []
cluster_labels = {}
for cluster in adata.obs["cluster"].cat.categories:
    subset = adata.obs[adata.obs["cluster"] == cluster]
    ranked = []
    for label, column in score_columns:
        score = float(subset[column].mean())
        ranked.append((score, label))
        score_rows.append({"cell_type": label, "cluster": str(cluster), "score": score})
    label = max(ranked)[1] if ranked else str(annotation.get("unknown_label", "Unknown"))
    cluster_labels[str(cluster)] = label
    cluster_rows.append({"cluster": str(cluster), "annotation": label})
adata.obs["cell_type"] = adata.obs["cluster"].astype(str).map(cluster_labels).astype("category")

qc_table = pd.DataFrame({
    "cell_id": adata.obs_names,
    "n_genes": adata.obs["n_genes_by_counts"].to_numpy(),
    "total_counts": adata.obs["total_counts"].to_numpy(),
    "pct_counts_mt": adata.obs["pct_counts_mt"].to_numpy(),
})
qc_table.to_csv(outputs["qc_metrics"], sep="\t", index=False)
pd.DataFrame(cluster_rows).to_csv(outputs["cluster_annotations"], sep="\t", index=False)
pd.DataFrame(score_rows, columns=["cell_type", "cluster", "score"]).to_csv(outputs["marker_scores"], sep="\t", index=False)

fig, axes = plt.subplots(1, 3, figsize=(12, 3.5))
for axis, column in zip(axes, ["n_genes_by_counts", "total_counts", "pct_counts_mt"]):
    axis.hist(adata.obs[column], bins=40)
    axis.set_title(column)
fig.tight_layout()
fig.savefig(outputs["qc_plots"], dpi=150)
plt.close(fig)

coords = adata.obsm["X_umap"]
fig, axis = plt.subplots(figsize=(6, 5))
for cluster in adata.obs["cluster"].cat.categories:
    selected = adata.obs["cluster"] == cluster
    axis.scatter(coords[selected, 0], coords[selected, 1], s=4, label=f"{cluster}: {cluster_labels[str(cluster)]}")
axis.legend(markerscale=2, fontsize=7, bbox_to_anchor=(1.02, 1), loc="upper left")
axis.set_xlabel("UMAP1")
axis.set_ylabel("UMAP2")
fig.tight_layout()
fig.savefig(outputs["umap"], dpi=150)
plt.close(fig)

adata.uns["omicsops"] = {"configuration": cfg, "cells_after_qc": int(adata.n_obs), "genes_after_qc": int(adata.n_vars)}
adata.write_h5ad(outputs["h5ad"], compression="gzip")
print(json.dumps({"cells": int(adata.n_obs), "genes": int(adata.n_vars), "outputs": outputs}, ensure_ascii=False))
