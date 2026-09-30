use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::SaveModelProfileRequest;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BeginCodexLoginResponse {
    pub login_id: Uuid,
    pub verification_uri: String,
    pub user_code: String,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CodexLoginState {
    Pending,
    Authorized,
    Saved,
    Cancelled,
    Expired,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CodexLoginStateResponse {
    pub login_id: Uuid,
    pub state: CodexLoginState,
    pub expires_at: DateTime<Utc>,
    pub error_code: Option<String>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FinishCodexLoginRequest {
    pub login_id: Uuid,
    pub profile: SaveModelProfileRequest,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SubscriptionModelStatus {
    pub provider: String,
    pub authenticated: bool,
    pub masked_account_label: Option<String>,
    pub cli_version: Option<String>,
    pub error_code: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelDiscoverySource {
    Provider,
    ConfiguredOnly,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelDiscoveryResult {
    pub models: Vec<String>,
    pub source: ModelDiscoverySource,
    pub can_refresh: bool,
}
