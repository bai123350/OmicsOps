#!/usr/bin/env python3
from __future__ import annotations

import html
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
results = ROOT / "results"
qc = json.loads((results / "qc-summary.json").read_text(encoding="utf-8"))
validation = json.loads((results / "seurat-validation.json").read_text(encoding="utf-8"))
rows = "\n".join(
    f"<tr><th>{html.escape(str(key))}</th><td>{html.escape(str(value))}</td></tr>"
    for key, value in qc.items()
)
document = f"""<!doctype html>
<html lang="en"><head><meta charset="utf-8"><title>OmicsOps PBMC report</title>
<style>body{{font:16px system-ui;max-width:960px;margin:40px auto;color:#17201e}}
table{{border-collapse:collapse}}th,td{{padding:8px 14px;border:1px solid #ccd8d4;text-align:left}}
img{{max-width:100%;border:1px solid #ccd8d4;border-radius:10px}}</style></head>
<body><h1>OmicsOps PBMC scRNA-seq report</h1>
<p>Seurat round-trip valid: <strong>{validation["valid"]}</strong></p>
<table>{rows}</table>
<h2>Quality control</h2><img src="figures/violin-qc.png" alt="QC violin plots">
<h2>Clusters and annotation</h2><img src="figures/umap-clusters.png" alt="UMAP clusters">
</body></html>"""
(results / "report.html").write_text(document, encoding="utf-8")
