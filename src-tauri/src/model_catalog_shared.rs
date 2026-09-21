use omicsops_core::workspace::{ModelCatalogCapabilities, ModelProviderKind};
use serde::Deserialize;
use std::sync::LazyLock;
use url::Url;

#[derive(Deserialize)]
pub(crate) struct CatalogModel {
    provider: ModelProviderKind,
    host: String,
    port: u16,
    #[serde(default)]
    path: Option<String>,
    model: String,
    pub supports_tools: bool,
    pub supports_vision: bool,
    pub capabilities: ModelCatalogCapabilities,
}

#[derive(Deserialize)]
struct Catalog {
    models: Vec<CatalogModel>,
}

static CATALOG: LazyLock<Catalog> = LazyLock::new(|| {
    serde_json::from_str(include_str!("model_catalog.json"))
        .expect("validated compiled model catalog")
});

/// Exact protocol + API host/port + full model ID. Gateway IDs retain their
/// provider prefix; no family, prefix or arbitrary gateway fallback is allowed.
pub(crate) fn exact_model_capabilities(
    provider: ModelProviderKind,
    base_url: &Url,
    model_id: &str,
) -> Option<&'static CatalogModel> {
    if base_url.scheme() != "https"
        || !base_url.username().is_empty()
        || base_url.password().is_some()
        || base_url.query().is_some()
        || base_url.fragment().is_some()
    {
        return None;
    }
    let request_path = base_url.path().trim_end_matches('/');
    CATALOG.models.iter().find(|row| {
        row.provider == provider
            && Some(row.host.as_str()) == base_url.host_str()
            && Some(row.port) == base_url.port_or_known_default()
            && row.path.as_deref().is_none_or(|path| path == request_path)
            && row.model == model_id
    })
}

// Preserve the existing local Ollama visual allowlist; models.dev cloud rows
// must never be applied to a local model with a similar name.
const VISUAL_MODELS: &[(ModelProviderKind, &str, u16, &str)] = &[
    (
        ModelProviderKind::Ollama,
        "localhost",
        11434,
        "llava:latest",
    ),
    (
        ModelProviderKind::Ollama,
        "127.0.0.1",
        11434,
        "llava:latest",
    ),
    (ModelProviderKind::Ollama, "::1", 11434, "llava:latest"),
    (
        ModelProviderKind::Ollama,
        "localhost",
        11434,
        "llama3.2-vision:latest",
    ),
    (
        ModelProviderKind::Ollama,
        "127.0.0.1",
        11434,
        "llama3.2-vision:latest",
    ),
    (
        ModelProviderKind::Ollama,
        "::1",
        11434,
        "llama3.2-vision:latest",
    ),
    (
        ModelProviderKind::Ollama,
        "localhost",
        11434,
        "qwen2.5vl:latest",
    ),
    (
        ModelProviderKind::Ollama,
        "127.0.0.1",
        11434,
        "qwen2.5vl:latest",
    ),
    (ModelProviderKind::Ollama, "::1", 11434, "qwen2.5vl:latest"),
];

pub(crate) fn exact_model_supports_vision(
    provider: ModelProviderKind,
    base_url: &Url,
    model_id: &str,
) -> bool {
    if let Some(row) = exact_model_capabilities(provider, base_url, model_id) {
        return row.supports_vision;
    }
    let host = base_url.host_str().unwrap_or_default().to_ascii_lowercase();
    let Some(port) = base_url.port_or_known_default() else {
        return false;
    };
    VISUAL_MODELS.iter().any(|entry| {
        entry.0 == provider && entry.1 == host && entry.2 == port && entry.3 == model_id
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn compiled_catalog_has_valid_unique_exact_keys() {
        let mut keys = std::collections::HashSet::new();
        assert!(CATALOG.models.len() > 500);
        for row in &CATALOG.models {
            assert!(keys.insert(format!(
                "{:?}|{}|{}|{:?}|{}",
                row.provider, row.host, row.port, row.path, row.model
            )));
            assert!(row.capabilities.context_limit > 0 && row.capabilities.output_limit > 0);
            assert!(row.capabilities.input_limit.is_none_or(|value| value > 0));
            assert_eq!(row.capabilities.source_sha256.len(), 64);
        }
    }
    #[test]
    fn gateway_and_official_models_require_exact_host_port_protocol_and_full_id() {
        let lookup = |url: &str, id: &str| {
            exact_model_capabilities(
                ModelProviderKind::OpenAiCompatible,
                &Url::parse(url).unwrap(),
                id,
            )
        };
        assert!(lookup("https://API.OPENAI.COM:443/v1", "gpt-4o").is_some());
        assert!(lookup("https://openrouter.ai/api/v1", "openai/gpt-4o").is_some());
        for (url, id) in [
            ("https://openrouter.ai/api/v1", "gpt-4o"),
            ("https://api.openai.com/v1", "openai/gpt-4o"),
            ("https://gateway.example/v1", "gpt-4o"),
            ("https://api.openai.com:8443/v1", "gpt-4o"),
            ("http://api.openai.com/v1", "gpt-4o"),
            ("https://api.openai.com/v1", "gpt-4o-custom"),
        ] {
            assert!(lookup(url, id).is_none());
        }
        assert!(
            exact_model_capabilities(
                ModelProviderKind::Anthropic,
                &Url::parse("https://api.openai.com/v1").unwrap(),
                "gpt-4o"
            )
            .is_none()
        );
    }

    #[test]
    fn opencode_go_glm_5_3_has_the_exact_compiled_budget() {
        let row = exact_model_capabilities(
            ModelProviderKind::OpenAiCompatible,
            &Url::parse("https://opencode.ai/zen/go/v1").unwrap(),
            "glm-5.3",
        )
        .expect("reviewed OpenCode Go model must have a compiled snapshot");
        assert_eq!(row.capabilities.source_provider, "opencode-go");
        assert_eq!(row.capabilities.context_limit, 1_000_000);
        assert_eq!(row.capabilities.output_limit, 131_072);
        assert!(row.supports_tools);
        assert!(!row.supports_vision);
        assert!(
            exact_model_capabilities(
                ModelProviderKind::OpenAiCompatible,
                &Url::parse("https://opencode.ai/zen/go/v1/").unwrap(),
                "glm-5.3",
            )
            .is_some()
        );
        for (url, model) in [
            ("https://opencode.ai/zen/v1", "glm-5.3"),
            ("https://opencode.ai/zen/go/v1/other", "glm-5.3"),
            ("https://opencode.ai:8443/zen/go/v1", "glm-5.3"),
            ("http://opencode.ai/zen/go/v1", "glm-5.3"),
            ("https://user@opencode.ai/zen/go/v1", "glm-5.3"),
            ("https://opencode.ai/zen/go/v1?route=other", "glm-5.3"),
            ("https://opencode.ai/zen/go/v1#other", "glm-5.3"),
            ("https://opencode.ai/zen/go/v1", "glm-5.3-sibling"),
        ] {
            assert!(
                exact_model_capabilities(
                    ModelProviderKind::OpenAiCompatible,
                    &Url::parse(url).unwrap(),
                    model,
                )
                .is_none(),
                "unexpected match for {url} {model}"
            );
        }
        assert!(
            exact_model_capabilities(
                ModelProviderKind::Anthropic,
                &Url::parse("https://opencode.ai/zen/go/v1").unwrap(),
                "glm-5.3",
            )
            .is_none()
        );
    }
}
