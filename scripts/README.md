# Compiled model catalog

`src-tauri/src/model_catalog.json` is a normalized snapshot of the public
[models.dev API](https://models.dev/api.json), retrieved on 2026-09-09.
The artifact and every capability snapshot record the SHA-256 of the source.
The snapshot contains 574 model entries.
Only the eight explicitly mapped provider endpoints with supported protocols and
positive text-generation limits are included. No model family/prefix matching.
Price data, automatic model routing, image token costs and runtime effective
reasoning reports are not implemented by this increment.

To intentionally update, download the public API JSON into a temporary file, then
run on Windows or macOS from the repository root:

```text
python scripts/import_model_catalog.py SOURCE_JSON src-tauri/src/model_catalog.json
python -B -m unittest discover -s scripts -p "test_*.py"
cargo test -p omicsops-desktop --lib model_
```

Review the generated diff and the source checksum before committing. The importer
itself, builds, tests and application startup do not access models.dev. Existing
profiles keep their snapshots; recreate a profile to intentionally adopt new data.
API host matching follows the application's provider contract; model IDs are
case sensitive and gateway prefixes are significant. A model listing is capability
metadata, not proof that an account can access that model.
