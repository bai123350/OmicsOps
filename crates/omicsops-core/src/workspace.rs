use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ProjectTemplate {
    Blank,
    SingleCellRnaSeq,
    BulkRnaSeq,
    LiteratureReview,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ProjectStatus {
    Ready,
    Running,
    WaitingForInput,
    NeedsAttention,
    Archived,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Project {
    pub id: Uuid,
    pub name: String,
    pub description: String,
    pub local_root: String,
    pub remote_root: Option<String>,
    pub connection_id: Option<Uuid>,
    pub template: ProjectTemplate,
    pub status: ProjectStatus,
    pub ollama_only: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Project {
    pub fn new(
        id: Uuid,
        name: impl Into<String>,
        local_root: impl Into<String>,
        template: ProjectTemplate,
        now: DateTime<Utc>,
    ) -> Self {
        Self {
            id,
            name: name.into(),
            description: String::new(),
            local_root: local_root.into(),
            remote_root: None,
            connection_id: None,
            template,
            status: ProjectStatus::Ready,
            ollama_only: false,
            created_at: now,
            updated_at: now,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ConversationStatus {
    Idle,
    Running,
    WaitingForInput,
    NeedsAttention,
    Completed,
    Archived,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Conversation {
    pub id: Uuid,
    pub project_id: Uuid,
    pub title: String,
    pub status: ConversationStatus,
    pub model_profile_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Conversation {
    pub fn new(id: Uuid, project_id: Uuid, title: impl Into<String>, now: DateTime<Utc>) -> Self {
        Self {
            id,
            project_id,
            title: title.into(),
            status: ConversationStatus::Idle,
            model_profile_id: None,
            created_at: now,
            updated_at: now,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MessageRole {
    User,
    Assistant,
    Tool,
    System,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Message {
    pub id: Uuid,
    pub project_id: Uuid,
    pub conversation_id: Uuid,
    pub sequence: u64,
    pub role: MessageRole,
    pub markdown: String,
    pub created_at: DateTime<Utc>,
}

impl Message {
    pub fn markdown(
        id: Uuid,
        project_id: Uuid,
        conversation_id: Uuid,
        sequence: u64,
        role: MessageRole,
        markdown: impl Into<String>,
        created_at: DateTime<Utc>,
    ) -> Self {
        Self {
            id,
            project_id,
            conversation_id,
            sequence,
            role,
            markdown: markdown.into(),
            created_at,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Artifact {
    pub id: Uuid,
    pub project_id: Uuid,
    pub run_id: Option<Uuid>,
    pub relative_path: String,
    pub remote_path: Option<String>,
    pub media_type: String,
    pub size_bytes: u64,
    pub sha256: String,
    pub verified: bool,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum NotebookEntryKind {
    Goal,
    Hypothesis,
    Method,
    Observation,
    Decision,
    Evidence,
    Code,
    Environment,
    Command,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct NotebookEntry {
    pub id: Uuid,
    pub project_id: Uuid,
    pub conversation_id: Option<Uuid>,
    pub turn_id: Option<Uuid>,
    pub kind: NotebookEntryKind,
    pub title: String,
    pub markdown: String,
    pub confidence: Option<f32>,
    pub evidence_ids: Vec<String>,
    pub artifact_ids: Vec<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SkillPackage {
    pub id: Uuid,
    pub name: String,
    pub version: String,
    pub source_path: String,
    pub sha256: String,
    pub enabled: bool,
    pub capabilities: Vec<String>,
    #[serde(default)]
    pub category: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct EvidenceReference {
    pub source_kind: String,
    pub source_id: String,
    pub excerpt: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct MemoryFact {
    pub id: Uuid,
    pub project_id: Uuid,
    pub conversation_id: Option<Uuid>,
    pub run_id: Option<Uuid>,
    pub dimension: String,
    pub key: String,
    pub value: String,
    pub statement: String,
    pub evidence: Vec<EvidenceReference>,
    pub conflicted_with: Vec<Uuid>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SkillCitation {
    pub skill_id: Uuid,
    pub name: String,
    pub version: String,
    pub package_sha256: String,
    pub section: String,
    pub excerpt_sha256: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ModelProviderKind {
    Anthropic,
    OpenAiCompatible,
    Ollama,
}

/// Catalog data captured when a profile is created; never refreshed implicitly.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ModelCatalogCapabilities {
    pub source_provider: String,
    pub source_sha256: String,
    pub context_limit: u32,
    pub input_limit: Option<u32>,
    pub output_limit: u32,
    pub reasoning: bool,
    pub reasoning_efforts: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ModelProfile {
    pub id: Uuid,
    pub label: String,
    pub provider: ModelProviderKind,
    pub base_url: String,
    pub model: String,
    pub credential_reference: Option<String>,
    pub supports_tools: bool,
    pub supports_vision: bool,
    #[serde(default)]
    pub context_window_tokens: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub catalog_capabilities: Option<ModelCatalogCapabilities>,
    /// Explicit OpenAI-compatible wire request; not a capability declaration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<String>,
    /// Optional Fast mode request: `None` inherits the provider/project
    /// default, `Some(false)` requests standard processing, and `Some(true)`
    /// requests Fast mode for the exact reviewed OpenAI models below.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fast_mode: Option<bool>,
    /// Optional profile for read-only delegation in newly created ordinary runs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delegated_model_profile_id: Option<Uuid>,
}

impl ModelProfile {
    pub fn execution_configuration_hash(&self) -> String {
        use sha2::{Digest, Sha256};
        // Credentials and labels do not belong in the execution identity.
        let mut value = serde_json::json!({
            "profile_id": self.id, "provider": self.provider, "base_url": self.base_url,
            "model": self.model, "supports_tools": self.supports_tools,
            "supports_vision": self.supports_vision,
            "context_window_tokens": self.effective_context_window_tokens(),
        });
        // Preserve hashes of legacy profiles that never requested an effort.
        if let Some(effort) = &self.reasoning_effort {
            value["reasoning_effort"] = serde_json::json!(effort);
        }
        // Preserve hashes of legacy profiles that never selected a Fast mode.
        if let Some(fast_mode) = self.fast_mode {
            value["fast_mode"] = serde_json::json!(fast_mode);
        }
        if self.effective_output_tokens() != 4096 {
            value["reserved_output_tokens"] = serde_json::json!(self.effective_output_tokens());
        }
        hex::encode(Sha256::digest(
            serde_json::to_vec(&value).expect("serializable profile"),
        ))
    }

    pub fn effective_context_window_tokens(&self) -> u32 {
        let requested = self.context_window_tokens.unwrap_or(32_768);
        self.catalog_capabilities
            .as_ref()
            .map_or(requested, |caps| {
                // Conservatively reserve output even when a separate input limit exists.
                requested
                    .min(caps.context_limit)
                    .min(caps.input_limit.unwrap_or(u32::MAX))
            })
    }

    pub fn effective_output_tokens(&self) -> u32 {
        self.catalog_capabilities.as_ref().map_or(4096, |caps| {
            // Reasoning and tool arguments share the provider output allowance.
            // Only exact catalog capabilities permit a larger reservation.
            let requested = if caps.reasoning { 16_384 } else { 4096 };
            caps.output_limit
                .min(requested)
                .min(self.effective_context_window_tokens() / 2)
                .max(1)
        })
    }

    /// Whether this profile is allowed to request Fast mode under the exact
    /// reviewed OpenAI endpoint and model contract.
    pub fn supports_fast_mode(&self) -> bool {
        supports_fast_mode(self.provider, &self.base_url, &self.model)
    }
}

const FAST_MODELS: [&str; 4] = [
    "gpt-6-astra",
    "gpt-5.6-sol",
    "gpt-5.6-terra",
    "gpt-5.6-luna",
];

/// Return whether an exact provider endpoint and model have the reviewed
/// Fast mode capability. Model families, aliases and custom gateways are not
/// inferred as supported.
pub fn supports_fast_mode(provider: ModelProviderKind, base_url: &str, model: &str) -> bool {
    if provider != ModelProviderKind::OpenAiCompatible {
        return false;
    }
    let Ok(url) = url::Url::parse(base_url) else {
        return false;
    };
    url.scheme() == "https"
        && url
            .host_str()
            .is_some_and(|host| host.eq_ignore_ascii_case("api.openai.com"))
        && url.port_or_known_default() == Some(443)
        && matches!(url.path(), "" | "/" | "/v1" | "/v1/")
        && url.username().is_empty()
        && url.password().is_none()
        && url.query().is_none()
        && url.fragment().is_none()
        && FAST_MODELS.contains(&model)
}

pub fn validate_reasoning_effort(
    provider: ModelProviderKind,
    effort: Option<&str>,
) -> Result<(), &'static str> {
    let Some(effort) = effort else {
        return Ok(());
    };
    if provider != ModelProviderKind::OpenAiCompatible {
        return Err("explicit reasoning effort currently requires an OpenAI-compatible provider");
    }
    if !matches!(
        effort,
        "none" | "minimal" | "low" | "medium" | "high" | "xhigh" | "max" | "ultra"
    ) {
        return Err("unsupported reasoning effort value");
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SyncDirection {
    LocalToRemote,
    RemoteToLocal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SyncState {
    Pending,
    Transferring,
    Paused,
    Canceled,
    Synced,
    Conflict,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SyncEntry {
    pub id: Uuid,
    pub project_id: Uuid,
    pub relative_path: String,
    #[serde(default)]
    pub local_relative_path: Option<String>,
    pub remote_path: Option<String>,
    pub direction: SyncDirection,
    pub size_bytes: u64,
    pub sha256: String,
    pub state: SyncState,
    #[serde(default)]
    pub transferred_bytes: u64,
    #[serde(default)]
    pub retry_count: u32,
    #[serde(default)]
    pub error: Option<String>,
    pub updated_at: DateTime<Utc>,
}

pub fn conflict_sibling_path(relative_path: &str, version: u32) -> String {
    let (stem, extension) = relative_path
        .rsplit_once('.')
        .unwrap_or((relative_path, ""));
    if extension.contains('/') || extension.contains('\\') || extension.is_empty() {
        format!("{relative_path}.conflict-{version}")
    } else {
        format!("{stem}.conflict-{version}.{extension}")
    }
}

#[cfg(test)]
mod fast_mode_tests {
    use super::*;

    fn profile(base_url: &str, model: &str, provider: ModelProviderKind) -> ModelProfile {
        serde_json::from_value(serde_json::json!({
            "id": Uuid::from_u128(1),
            "label": "test",
            "provider": provider,
            "base_url": base_url,
            "model": model,
            "credential_reference": null,
            "supports_tools": true,
            "supports_vision": false
        }))
        .expect("profile fixture")
    }

    #[test]
    fn fast_mode_requires_exact_official_endpoint_path_and_model() {
        for base_url in [
            "https://api.openai.com",
            "https://api.openai.com/",
            "https://api.openai.com/v1",
            "https://api.openai.com/v1/",
            "https://API.OPENAI.COM:443/v1",
        ] {
            for model in [
                "gpt-6-astra",
                "gpt-5.6-sol",
                "gpt-5.6-terra",
                "gpt-5.6-luna",
            ] {
                assert!(
                    profile(base_url, model, ModelProviderKind::OpenAiCompatible)
                        .supports_fast_mode(),
                    "{base_url} {model}"
                );
            }
        }

        for (base_url, model, provider) in [
            (
                "http://api.openai.com/v1",
                "gpt-5.6-luna",
                ModelProviderKind::OpenAiCompatible,
            ),
            (
                "https://api.openai.com:8443/v1",
                "gpt-5.6-luna",
                ModelProviderKind::OpenAiCompatible,
            ),
            (
                "https://api.openai.com/v2",
                "gpt-5.6-luna",
                ModelProviderKind::OpenAiCompatible,
            ),
            (
                "https://api.openai.com/v1/chat/completions",
                "gpt-5.6-luna",
                ModelProviderKind::OpenAiCompatible,
            ),
            (
                "https://proxy.example/v1",
                "gpt-5.6-luna",
                ModelProviderKind::OpenAiCompatible,
            ),
            (
                "https://api.openai.com/v1",
                "gpt-5.6-luna-preview",
                ModelProviderKind::OpenAiCompatible,
            ),
            (
                "https://api.openai.com/v1",
                "GPT-5.6-luna",
                ModelProviderKind::OpenAiCompatible,
            ),
            (
                "https://api.openai.com/v1",
                "gpt-5.6-luna",
                ModelProviderKind::Anthropic,
            ),
            (
                "https://api.openai.com/v1",
                "gpt-5.6-luna",
                ModelProviderKind::Ollama,
            ),
        ] {
            assert!(
                !profile(base_url, model, provider).supports_fast_mode(),
                "unexpected fast-mode support for {base_url} {model} {provider:?}"
            );
        }
    }

    #[test]
    fn fast_mode_hash_is_backward_compatible_for_legacy_and_none() {
        let mut profile = profile(
            "https://api.openai.com/v1",
            "gpt-5.6-luna",
            ModelProviderKind::OpenAiCompatible,
        );
        let legacy_hash = profile.execution_configuration_hash();
        let encoded = serde_json::to_value(&profile).expect("serialize profile");
        assert!(encoded.get("fast_mode").is_none());

        profile.fast_mode = None;
        assert_eq!(profile.execution_configuration_hash(), legacy_hash);
        profile.fast_mode = Some(false);
        let default_hash = profile.execution_configuration_hash();
        assert_ne!(default_hash, legacy_hash);
        profile.fast_mode = Some(true);
        assert_ne!(profile.execution_configuration_hash(), default_hash);
    }
}

#[cfg(test)]
mod output_budget_tests {
    use super::*;
    #[test]
    fn reasoning_output_reservation_uses_only_catalog_limits() {
        let mut profile: ModelProfile = serde_json::from_value(serde_json::json!({
            "id":Uuid::new_v4(), "label":"test", "provider":"open_ai_compatible",
            "base_url":"https://gateway.example/v1", "model":"unknown", "credential_reference":null,
            "supports_tools":true,"supports_vision":false
        }))
        .unwrap();
        assert_eq!(profile.effective_output_tokens(), 4096);
        let legacy_hash = profile.execution_configuration_hash();
        profile.catalog_capabilities = Some(ModelCatalogCapabilities {
            source_provider: "fixture".into(),
            source_sha256: "fixture".into(),
            context_limit: 131072,
            input_limit: None,
            output_limit: 32768,
            reasoning: true,
            reasoning_efforts: None,
        });
        assert_eq!(profile.effective_output_tokens(), 16384);
        assert_ne!(profile.execution_configuration_hash(), legacy_hash);
        profile.catalog_capabilities.as_mut().unwrap().output_limit = 2048;
        assert_eq!(profile.effective_output_tokens(), 2048);
        profile.catalog_capabilities.as_mut().unwrap().output_limit = 32768;
        profile.catalog_capabilities.as_mut().unwrap().reasoning = false;
        assert_eq!(profile.effective_output_tokens(), 4096);
    }
}
