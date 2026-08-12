from __future__ import annotations

import csv
import html
import json
import sys
from pathlib import Path


cfg = json.loads(sys.argv[1])
output = Path(cfg["output_path"])
output.parent.mkdir(parents=True, exist_ok=True)
parts = [
    "<!doctype html><html><head><meta charset='utf-8'>",
    f"<title>{html.escape(str(cfg.get('title', 'OmicsOps analysis report')))}</title>",
    "<style>body{font:14px system-ui;max-width:1100px;margin:40px auto;color:#172033}table{border-collapse:collapse}th,td{border:1px solid #ddd;padding:5px}code{background:#f2f4f7;padding:2px 4px}</style>",
    "</head><body>",
    f"<h1>{html.escape(str(cfg.get('title', 'OmicsOps analysis report')))}</h1>",
]
if cfg.get("sections"):
    parts.append("<h2>Planned sections</h2><ul>")
    parts.extend(f"<li>{html.escape(str(section))}</li>" for section in cfg["sections"])
    parts.append("</ul>")
parts.append("<h2>Verified analysis outputs</h2>")
for raw in cfg.get("inputs", []):
    path = Path(raw)
    parts.append(f"<h3><code>{html.escape(str(path))}</code></h3>")
    if not path.is_file():
        parts.append("<p>Output was not present when the report was rendered.</p>")
        continue
    parts.append(f"<p>{path.stat().st_size:,} bytes</p>")
    if path.suffix.lower() in {".tsv", ".csv"}:
        delimiter = "\t" if path.suffix.lower() == ".tsv" else ","
        with path.open(encoding="utf-8", errors="replace", newline="") as handle:
            rows = list(csv.reader(handle, delimiter=delimiter))[:11]
        if rows:
            parts.append("<table>")
            for index, row in enumerate(rows):
                tag = "th" if index == 0 else "td"
                parts.append("<tr>" + "".join(f"<{tag}>{html.escape(cell)}</{tag}>" for cell in row) + "</tr>")
            parts.append("</table>")
    elif path.suffix.lower() in {".png", ".jpg", ".jpeg", ".svg"}:
        relative = Path("..") / path.relative_to(output.parent.parent) if output.parent.parent in path.parents else path
        parts.append(f"<img src='{html.escape(str(relative).replace(chr(92), '/'))}' style='max-width:100%'>")
parts.append("</body></html>")
output.write_text("".join(parts), encoding="utf-8")
print(json.dumps({"report": str(output), "inputs": len(cfg.get("inputs", []))}))
