use omicsops_core::workspace::ModelProviderKind;
use url::Url;

/// Minimal compiled visual-capability slice used by profile creation. Every
/// row binds the exact provider, API host/port, and model ID. Unknown gateway
/// aliases and longer sibling IDs fail closed; callers must never add prefix
/// or family matching here.
const VISUAL_MODELS: &[(ModelProviderKind, &str, u16, &str)] = &[
    (
        ModelProviderKind::OpenAiCompatible,
        "api.openai.com",
        443,
        "gpt-4o",
    ),
    (
        ModelProviderKind::OpenAiCompatible,
        "api.openai.com",
        443,
        "gpt-4o-mini",
    ),
    (
        ModelProviderKind::OpenAiCompatible,
        "api.openai.com",
        443,
        "gpt-4.1",
    ),
    (
        ModelProviderKind::OpenAiCompatible,
        "api.openai.com",
        443,
        "gpt-4.1-mini",
    ),
    (
        ModelProviderKind::OpenAiCompatible,
        "api.openai.com",
        443,
        "gpt-4.1-nano",
    ),
    (
        ModelProviderKind::OpenAiCompatible,
        "api.openai.com",
        443,
        "gpt-5",
    ),
    (
        ModelProviderKind::OpenAiCompatible,
        "api.openai.com",
        443,
        "gpt-5-mini",
    ),
    (
        ModelProviderKind::OpenAiCompatible,
        "api.openai.com",
        443,
        "gpt-5-nano",
    ),
    (
        ModelProviderKind::Anthropic,
        "api.anthropic.com",
        443,
        "claude-3-5-sonnet-20241022",
    ),
    (
        ModelProviderKind::Anthropic,
        "api.anthropic.com",
        443,
        "claude-3-7-sonnet-20250219",
    ),
    (
        ModelProviderKind::Anthropic,
        "api.anthropic.com",
        443,
        "claude-sonnet-4-20250514",
    ),
    (
        ModelProviderKind::Anthropic,
        "api.anthropic.com",
        443,
        "claude-opus-4-20250514",
    ),
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
    let host = base_url.host_str().unwrap_or_default().to_ascii_lowercase();
    let Some(port) = base_url.port_or_known_default() else {
        return false;
    };
    VISUAL_MODELS.iter().any(|entry| {
        entry.0 == provider && entry.1 == host && entry.2 == port && entry.3 == model_id
    })
}
