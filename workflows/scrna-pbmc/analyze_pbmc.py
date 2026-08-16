#!/usr/bin/env python3
"""Reproducible Scanpy acceptance analysis for the public 10x PBMC 3k data."""

from __future__ import annotations

import hashlib
import importlib.metadata
import json
import os
from pathlib import Path

import matplotlib

matplotlib.use("Agg")
import matplotlib.pyplot as plt
import numpy as np
import pandas as pd
import scanpy as sc
import scrublet as scr
from scipy import sparse


SCRIPT_ROOT = Path(__file__).resolve().parents[1]
PROJECT_ROOT = Path(os.environ.get("OMICSOPS_PROJECT_ROOT", SCRIPT_ROOT)).resolve()
INPUT = Path(
    os.environ.get(
        "OMICSOPS_PBMC_INPUT",
        PROJECT_ROOT / "work/pbmc3k/filtered_gene_bc_matrices/hg19",
    )
).resolve()
RESULTS = Path(
    os.environ.get("OMICSOPS_PBMC_RESULTS", PROJECT_ROOT / "results")
).resolve()
FIGURES = RESULTS / "figures"
RESULTS.mkdir(parents=True, exist_ok=True)
FIGURES.mkdir(parents=True, exist_ok=True)
sc.settings.figdir = FIGURES
sc.settings.verbosity = 2
sc.set_figure_params(dpi=110, facecolor="white")


def robust_upper(values: pd.Series, hard_cap: float | None = None) -> float:
    q1, q3 = np.quantile(values, [0.25, 0.75])
    threshold = float(q3 + 3.0 * (q3 - q1))
    return min(threshold, hard_cap) if hard_cap is not None else threshold


def robust_lower(values: pd.Series, hard_floor: float) -> float:
    q1, q3 = np.quantile(values, [0.25, 0.75])
    return max(float(q1 - 3.0 * (q3 - q1)), hard_floor)


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for block in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


required_inputs = [INPUT / "matrix.mtx", INPUT / "genes.tsv", INPUT / "barcodes.tsv"]
missing = [str(path) for path in required_inputs if not path.is_file() or path.stat().st_size == 0]
if missing:
    raise FileNotFoundError(f"missing 10x input files: {missing}")

# This fixture contains only Cell Ranger's filtered matrix. Empty droplets are not available,
# so SoupX cannot estimate the soup profile. DecontX is deliberately not applied to this public
# baseline without an unfiltered/background matrix or independent labels; the limitation and
# downstream contamination checks remain explicit in the result metadata.
raw_candidates = [
    INPUT.parents[1] / "raw_gene_bc_matrices" / INPUT.name,
    INPUT.parents[1] / "raw_feature_bc_matrix",
]
raw_available = any(path.is_dir() for path in raw_candidates)
ambient_rna = {
    "raw_matrix_available": raw_available,
    "method": "not_applied_filtered_matrix_only" if not raw_available else "requires_soupx",
    "correction_applied": False,
    "reason": (
        "Only the filtered 10x matrix is present; empty droplets required by SoupX are unavailable. "
        "DecontX was not applied without an independent background/reference signal. Marker "
        "specificity is assessed after clustering and this limitation is retained in the report."
        if not raw_available
        else "A raw matrix was unexpectedly found; this filtered-only workflow must stop for SoupX."
    ),
}
if raw_available:
    raise RuntimeError(ambient_rna["reason"])

adata = sc.read_10x_mtx(INPUT, var_names="gene_symbols", cache=False)
adata.var_names_make_unique()
if not sparse.issparse(adata.X):
    adata.X = sparse.csr_matrix(adata.X)
adata.layers["counts"] = adata.X.copy()
adata.var["mt"] = adata.var_names.str.upper().str.startswith("MT-")
adata.var["ribo"] = adata.var_names.str.upper().str.startswith(("RPS", "RPL"))
adata.var["hb"] = adata.var_names.str.upper().str.match(r"^HB[^P]")
sc.pp.calculate_qc_metrics(
    adata,
    qc_vars=["mt", "ribo", "hb"],
    percent_top=None,
    log1p=False,
    inplace=True,
)

# Score doublets on unfiltered raw counts. Filtering happens only in the unified mask below.
scrub = scr.Scrublet(adata.layers["counts"], expected_doublet_rate=0.05, random_state=0)
doublet_scores, predicted_doublets = scrub.scrub_doublets(
    min_counts=2,
    min_cells=3,
    min_gene_variability_pctl=85,
    n_prin_comps=30,
)
adata.obs["doublet_score"] = doublet_scores
adata.obs["predicted_doublet"] = predicted_doublets
doublet_threshold = float(scrub.threshold_)

fig, axes = plt.subplots(2, 2, figsize=(12, 8))
axes[0, 0].hist(adata.obs["n_genes_by_counts"], bins=60, color="#4c78a8")
axes[0, 0].set(title="Genes per cell", xlabel="genes", ylabel="cells")
axes[0, 1].hist(adata.obs["total_counts"], bins=60, color="#59a14f")
axes[0, 1].set(title="UMI counts per cell", xlabel="counts", ylabel="cells")
axes[1, 0].hist(adata.obs["pct_counts_mt"], bins=60, color="#f28e2b")
axes[1, 0].set(title="Mitochondrial fraction", xlabel="percent", ylabel="cells")
axes[1, 1].hist(adata.obs["doublet_score"], bins=60, color="#b07aa1")
axes[1, 1].axvline(
    doublet_threshold,
    color="#b22222",
    linestyle="--",
    label=f"threshold {doublet_threshold:.3f}",
)
axes[1, 1].set(title="Scrublet doublet score", xlabel="score", ylabel="cells")
axes[1, 1].legend()
fig.tight_layout()
fig.savefig(FIGURES / "qc-prefilter.png", dpi=150, bbox_inches="tight")
plt.close(fig)

fig, axes = plt.subplots(1, 2, figsize=(12, 5))
first = axes[0].scatter(
    adata.obs["total_counts"],
    adata.obs["n_genes_by_counts"],
    c=adata.obs["pct_counts_mt"],
    s=7,
    cmap="viridis",
)
axes[0].set(xlabel="total counts", ylabel="genes", title="Counts, genes and MT%")
fig.colorbar(first, ax=axes[0], label="MT%")
axes[1].scatter(
    adata.obs["total_counts"],
    adata.obs["doublet_score"],
    c=predicted_doublets,
    s=7,
    cmap="coolwarm",
)
axes[1].axhline(doublet_threshold, color="#b22222", linestyle="--")
axes[1].set(
    xlabel="total counts",
    ylabel="doublet score",
    title="Doublet evidence before filtering",
)
fig.tight_layout()
fig.savefig(FIGURES / "qc-scatter-prefilter.png", dpi=150, bbox_inches="tight")
plt.close(fig)

thresholds = {
    "min_genes": int(robust_lower(adata.obs["n_genes_by_counts"], 200)),
    "max_genes": int(robust_upper(adata.obs["n_genes_by_counts"], 5000)),
    "min_counts": int(robust_lower(adata.obs["total_counts"], 500)),
    "max_counts": int(robust_upper(adata.obs["total_counts"])),
    "max_pct_mt": float(robust_upper(adata.obs["pct_counts_mt"], 20.0)),
    "max_doublet_score": doublet_threshold,
}
before_cells, before_genes = adata.n_obs, adata.n_vars
qc_mask = (
    (adata.obs["n_genes_by_counts"] >= thresholds["min_genes"])
    & (adata.obs["n_genes_by_counts"] <= thresholds["max_genes"])
    & (adata.obs["total_counts"] >= thresholds["min_counts"])
    & (adata.obs["total_counts"] <= thresholds["max_counts"])
    & (adata.obs["pct_counts_mt"] <= thresholds["max_pct_mt"])
)
doublet_mask = ~adata.obs["predicted_doublet"].astype(bool)
adata.obs["qc_metrics_pass"] = qc_mask
adata.obs["qc_doublet_pass"] = doublet_mask
adata.obs["qc_pass"] = qc_mask & doublet_mask
all_cell_qc = adata.obs.copy()
all_cell_qc.to_csv(RESULTS / "cell_qc_all.tsv", sep="\t", index_label="barcode")
removed_by_metrics = int((~qc_mask).sum())
predicted_doublet_count = int((~doublet_mask).sum())
adata = adata[adata.obs["qc_pass"]].copy()
sc.pp.filter_genes(adata, min_cells=3)

sc.pp.normalize_total(adata, target_sum=1e4)
sc.pp.log1p(adata)
adata.layers["lognorm"] = adata.X.copy()
adata.raw = adata
sc.pp.highly_variable_genes(adata, n_top_genes=2000, flavor="seurat", subset=False)
sc.tl.pca(adata, use_highly_variable=True, svd_solver="arpack", random_state=0)
sc.pp.neighbors(
    adata,
    n_neighbors=15,
    n_pcs=min(40, adata.obsm["X_pca"].shape[1]),
    random_state=0,
)
sc.tl.umap(adata, random_state=0)
sc.tl.leiden(adata, resolution=0.8, key_added="leiden", random_state=0)
sc.tl.rank_genes_groups(
    adata,
    groupby="leiden",
    method="wilcoxon",
    pts=True,
    use_raw=True,
)

markers = sc.get.rank_genes_groups_df(adata, group=None)
markers.to_csv(RESULTS / "cluster_markers.tsv", sep="\t", index=False)
marker_sets = {
    "T cells": ["CD3D", "CD3E", "TRBC1"],
    "B cells": ["MS4A1", "CD79A", "CD37"],
    "NK cells": ["NKG7", "GNLY", "KLRD1"],
    "Classical monocytes": ["LYZ", "S100A8", "S100A9", "CTSS"],
    "FCGR3A monocytes": ["FCGR3A", "LST1", "IFITM3", "LGALS3BP"],
    "Dendritic cells": ["FCER1A", "CD1C", "CLEC10A"],
    "Platelets": ["PPBP", "PF4", "GNG11"],
}
score_columns: list[tuple[str, str]] = []
for label, genes in marker_sets.items():
    present = [gene for gene in genes if gene in adata.var_names]
    column = f"marker_score_{label.replace(' ', '_').lower()}"
    if present:
        sc.tl.score_genes(adata, present, score_name=column, random_state=0)
        score_columns.append((label, column))

cluster_scores = adata.obs.groupby("leiden", observed=True)[
    [column for _, column in score_columns]
].mean()
cluster_sizes = adata.obs.groupby("leiden", observed=True).size()
top_marker_map = (
    markers.sort_values(["group", "scores"], ascending=[True, False])
    .groupby("group", observed=True)["names"]
    .apply(lambda values: ", ".join(values.head(8).astype(str)))
    .to_dict()
)
annotation_rows = []
cluster_to_celltype: dict[str, str] = {}
for cluster, row in cluster_scores.iterrows():
    ordered = row.sort_values(ascending=False)
    best_column = str(ordered.index[0])
    best_label = next(label for label, column in score_columns if column == best_column)
    margin = float(ordered.iloc[0] - ordered.iloc[1]) if len(ordered) > 1 else float("inf")
    confidence = "high" if margin >= 0.25 else "medium" if margin >= 0.10 else "low"
    if confidence == "low":
        best_label = f"Uncertain ({best_label})"
    cluster_to_celltype[str(cluster)] = best_label
    annotation_rows.append(
        {
            "cluster": str(cluster),
            "cell_type": best_label,
            "confidence": confidence,
            "n_cells": int(cluster_sizes.loc[cluster]),
            "marker_score": float(ordered.iloc[0]),
            "score_margin": margin,
            "top_markers": top_marker_map.get(str(cluster), ""),
        }
    )
annotations = pd.DataFrame(annotation_rows).sort_values("cluster")
annotations.to_csv(RESULTS / "cluster_annotations.tsv", sep="\t", index=False)
adata.obs["cell_type"] = pd.Categorical(
    adata.obs["leiden"].astype(str).map(cluster_to_celltype)
)
adata.obs.to_csv(RESULTS / "cell_metadata.tsv", sep="\t", index_label="barcode")

specificity_rows = []
for cluster in adata.obs["leiden"].cat.categories:
    cluster_mask = adata.obs["leiden"] == cluster
    for lineage, genes in marker_sets.items():
        for gene in genes:
            if gene not in adata.raw.var_names:
                continue
            values = adata.raw[cluster_mask, gene].X
            if sparse.issparse(values):
                mean = float(values.mean())
                pct = float(values.getnnz() / values.shape[0] * 100.0)
            else:
                mean = float(np.mean(values))
                pct = float(np.count_nonzero(values) / values.shape[0] * 100.0)
            specificity_rows.append(
                {
                    "cluster": str(cluster),
                    "lineage": lineage,
                    "gene": gene,
                    "mean_lognorm": mean,
                    "pct_expressed": pct,
                }
            )
pd.DataFrame(specificity_rows).to_csv(
    RESULTS / "marker_specificity.tsv", sep="\t", index=False
)

sc.pl.violin(
    adata,
    ["n_genes_by_counts", "total_counts", "pct_counts_mt", "doublet_score"],
    multi_panel=True,
    show=False,
    save="-qc-postfilter.png",
)
sc.pl.umap(adata, color=["leiden", "cell_type"], show=False, save="-clusters.png")
present_markers = [
    gene for genes in marker_sets.values() for gene in genes if gene in adata.raw.var_names
]
dotplot = sc.pl.dotplot(
    adata,
    var_names=present_markers,
    groupby="cell_type",
    use_raw=True,
    standard_scale="var",
    show=False,
    return_fig=True,
)
dotplot.savefig(FIGURES / "marker-dotplot.png", dpi=150, bbox_inches="tight")

summary = {
    "workflow": "scrna-pbmc-reference-v2",
    "input": str(INPUT),
    "input_sha256": {path.name: sha256(path) for path in required_inputs},
    "ambient_rna": ambient_rna,
    "doublet_detection": {
        "method": "Scrublet",
        "scored_before_filtering": True,
        "threshold": doublet_threshold,
        "predicted": predicted_doublet_count,
        "rate": predicted_doublet_count / before_cells,
        "removed_in_unified_filter": int((~doublet_mask & qc_mask).sum()),
    },
    "qc_thresholds": thresholds,
    "cells_before": int(before_cells),
    "genes_before": int(before_genes),
    "cells_after": int(adata.n_obs),
    "genes_after": int(adata.n_vars),
    "removed_by_qc_metrics": removed_by_metrics,
    "clusters": int(adata.obs["leiden"].nunique()),
    "annotation": {
        "method": "cluster-level canonical marker scoring plus Wilcoxon marker review",
        "low_confidence_clusters": int((annotations["confidence"] == "low").sum()),
        "marker_specificity_table": "marker_specificity.tsv",
    },
    "software": {
        "scanpy": sc.__version__,
        "scrublet": importlib.metadata.version("scrublet"),
        "anndata": importlib.metadata.version("anndata"),
        "numpy": np.__version__,
        "pandas": pd.__version__,
    },
    "random_seed": 0,
}
adata.uns["omicsops"] = summary
output = RESULTS / "pbmc3k.h5ad"
adata.write_h5ad(output, compression="gzip")
with (RESULTS / "qc-summary.json").open("w", encoding="utf-8") as handle:
    json.dump(summary, handle, indent=2)
print(json.dumps({"h5ad": str(output), **summary}, indent=2))
