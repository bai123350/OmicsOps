use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum CredentialTarget {
    Model { id: Uuid },
    Ssh { id: Uuid },
    McpBinding { server_id: Uuid, name: String },
    Managed { id: Uuid },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CredentialPresence {
    Present,
    Missing,
    Unavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CredentialValueKind {
    ApiKey,
    Password,
    SshPrivateKey,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CredentialConsumer {
    pub kind: String,
    pub id: Uuid,
    pub label: String,
    pub binding_name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CredentialEntry {
    pub target: CredentialTarget,
    pub reference: String,
    pub label: String,
    pub presence: CredentialPresence,
    pub value_kind: CredentialValueKind,
    pub consumers: Vec<CredentialConsumer>,
    pub can_replace: bool,
    pub can_delete: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManagedCredentialMetadata {
    pub id: Uuid,
    pub label: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateManagedCredentialRequest {
    pub label: String,
    pub secret: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CreateCredentialResult {
    Saved { entry: CredentialEntry },
    SecretSaveNotConfirmed { entry: CredentialEntry },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReplaceCredentialRequest {
    pub target: CredentialTarget,
    pub expected_reference: String,
    pub expected_value_kind: CredentialValueKind,
    pub secret: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DeleteCredentialResult {
    Deleted,
    InUse { consumers: Vec<CredentialConsumer> },
}
