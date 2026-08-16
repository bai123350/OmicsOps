#!/usr/bin/env python3
from __future__ import annotations

import html
import json
import os
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
results = Path(os.environ.get("OMICSOPS_PBMC_RESULTS", ROOT / "results")).resolve()
qc = json.loads((results / "qc-summary.json").read_text(encoding="utf-8"))
validation = json.loads((results / "seurat-validation.json").read_text(encoding="utf-8"))
rows = "\n".join(
    f"<tr><th>{html.escape(str(key))}</th><td><pre>{html.escape(json.dumps(value, ensure_ascii=False, indent=2) if isinstance(value, (dict, list)) else str(value))}</pre></td></tr>"
    for key, value in qc.items()
)
document = f"""<!doctype html>
<html lang="en"><head><meta charset="utf-8"><title>OmicsOps PBMC report</title>
<style>body{{font:16px system-ui;max-width:960px;margin:40px auto;color:#17201e}}
table{{border-collapse:collapse;width:100%}}th,td{{padding:8px 14px;border:1px solid #ccd8d4;text-align:left;vertical-align:top}}pre{{white-space:pre-wrap;margin:0}}
img{{max-width:100%;border:1px solid #ccd8d4;border-radius:10px}}</style></head>
<body><h1>OmicsOps PBMC scRNA-seq report</h1>
<p>Seurat round-trip valid: <strong>{validation["valid"]}</strong></p>
<table>{rows}</table>
<h2>Quality control before filtering</h2><img src="figures/qc-prefilter.png" alt="QC distributions including Scrublet doublet scores">
<img src="figures/qc-scatter-prefilter.png" alt="QC and doublet scatter plots">
<h2>Quality control after unified filtering</h2><img src="figures/violin-qc-postfilter.png" alt="Post-filter QC violin plots">
<h2>Clusters and annotation</h2><img src="figures/umap-clusters.png" alt="UMAP clusters">
<img src="figures/marker-dotplot.png" alt="Canonical marker dotplot">
</body></html>"""
(results / "report.html").write_text(document, encoding="utf-8")
