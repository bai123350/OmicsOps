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
        if self.effective_output_tokens() != 4096 {
            value["reserved_output_tokens"] = serde_json::json!(self.effective_output_tokens());
        }
        hex::encode(Sha256::digest(
            serde_json::to_vec(&value).expect("serializable profile"),
        ))
    }

    pub fn effective_context_window_tokens(&self) -> u32 {
        let requested = self.context_window_tokens.unwrap_or(32_768);
        self.catalog_capabilities.as_ref().map_or(requested, |caps| {
            // Conservatively reserve output even when a separate input limit exists.
            requested.min(caps.context_limit).min(caps.input_limit.unwrap_or(u32::MAX))
        })
    }

    pub fn effective_output_tokens(&self) -> u32 {
        self.catalog_capabilities.as_ref().map_or(4096, |caps| caps.output_limit.min(4096))
    }
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
