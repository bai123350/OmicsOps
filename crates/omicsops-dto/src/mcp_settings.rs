use serde::{Deserialize, Serialize};

/// One environment binding submitted while editing an MCP server profile.
///
/// `keep_existing` lets the web client retain a value without receiving it.
/// The desktop host resolves the saved binding and validates it again before
/// persisting the profile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SaveMcpEnvBindingRequest {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credential_reference: Option<String>,
    #[serde(default)]
    pub keep_existing: bool,
}
