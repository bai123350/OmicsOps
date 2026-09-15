use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GeneralNativePreferences {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_directory_start: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GeneralUpdateStatus {
    Unconfigured,
    Configured,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GeneralSystemStatus {
    pub app_version: String,
    pub app_data_directory: String,
    pub update_status: GeneralUpdateStatus,
    pub update_source_configured: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SystemInterpreterStatus {
    Found,
    Missing,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SystemInterpreterDiagnostic {
    pub program: String,
    pub status: SystemInterpreterStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SystemInterpreterDiagnostics {
    pub python: SystemInterpreterDiagnostic,
    pub r: SystemInterpreterDiagnostic,
    pub checked_at: String,
}
