"""Normalize an explicitly downloaded models.dev api.json; never accesses the network.
Usage: python scripts/import_model_catalog.py SOURCE_JSON OUTPUT_JSON
"""
import hashlib
import json
import sys
from pathlib import Path
from urllib.parse import urlsplit

# Protocols actually implemented by OmicsOps. Default endpoints are explicit;
# never infer an API protocol from a model name or a provider family.
PROVIDERS = {
    "openai": ("open_ai_compatible", "https://api.openai.com/v1"),
    "anthropic": ("anthropic", "https://api.anthropic.com/v1"),
    "deepseek": ("open_ai_compatible", None),
    "alibaba": ("open_ai_compatible", None),
    "alibaba-cn": ("open_ai_compatible", None),
    "minimax": ("anthropic", None),
    "minimax-cn": ("anthropic", None),
    "openrouter": ("open_ai_compatible", None),
}


def normalize(raw):
    source = json.loads(raw)
    rows = []
    for provider_id, (protocol, default_url) in sorted(PROVIDERS.items()):
        provider = source[provider_id]
        endpoint = default_url or provider["api"]
        url = urlsplit(endpoint)
        if url.scheme != "https" or not url.hostname or url.username or url.query:
            raise ValueError(f"invalid public API endpoint: {provider_id}")
        for model_id, model in sorted(provider["models"].items()):
            limit = model.get("limit", {})
            modalities = model.get("modalities", {})
            # Only text-producing models with usable, explicitly reported limits.
            if "text" not in modalities.get("output", []):
                continue
            if any(type(limit.get(k)) is not int or not 0 < limit[k] <= 2**32-1
                   for k in ("context", "output")):
                continue
            efforts = None
            for option in model.get("reasoning_options", []):
                if option.get("type") == "effort":
                    efforts = option["values"]
            rows.append({
                "provider": protocol, "host": url.hostname,
                "port": url.port or 443, "model": model_id,
                "supports_tools": model.get("tool_call") is True,
                "supports_vision": "image" in modalities.get("input", []),
                "capabilities": {
                    "source_provider": provider_id,
                    "source_sha256": hashlib.sha256(raw).hexdigest(),
                    "context_limit": limit["context"],
                    "input_limit": limit.get("input"),
                    "output_limit": limit["output"],
                    "reasoning": model.get("reasoning") is True,
                    "reasoning_efforts": efforts,
                },
            })
    return {"source": "https://models.dev/api.json",
            "source_sha256": hashlib.sha256(raw).hexdigest(), "models": rows}


if __name__ == "__main__":
    result = normalize(Path(sys.argv[1]).read_bytes())
    Path(sys.argv[2]).write_text(json.dumps(result, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
