use omicsops_protocol::KernelLanguageV4;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SshInterpreterStatusV4 {
    Available,
    NotFound,
    Timeout,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SshInterpreterObservationV4 {
    pub language: KernelLanguageV4,
    pub status: SshInterpreterStatusV4,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub executable: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

impl SshInterpreterObservationV4 {
    pub fn new(language: KernelLanguageV4, status: SshInterpreterStatusV4) -> Self {
        Self {
            language,
            status,
            executable: None,
            version: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SshEnvironmentOptionV4 {
    pub environment: String,
    pub label: String,
    pub interpreters: Vec<SshInterpreterObservationV4>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SshEnvironmentReportV4 {
    pub project_id: Uuid,
    pub connection_id: Uuid,
    pub remote_root: String,
    pub requested_environment: String,
    pub observed_at: String,
    pub environments: Vec<SshEnvironmentOptionV4>,
    pub suggested_environment: Option<String>,
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProbeSshProjectEnvironmentsRequestV4 {
    pub project_id: Uuid,
    pub environment: String,
}
