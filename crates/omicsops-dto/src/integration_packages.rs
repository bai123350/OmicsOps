use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginPhase {
    Staging,
    Installed,
    Updating,
    Removing,
    NeedsAttention,
    Removed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginFilePreview {
    pub relative_path: String,
    pub size_bytes: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginPresetBinding {
    pub preset_id: String,
    pub server_id: Uuid,
    pub ownership: String,
    pub configured: bool,
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginInspection {
    pub manifest_digest: String,
    pub source_path: String,
    pub package_id: String,
    pub version: String,
    pub name: String,
    pub files: Vec<PluginFilePreview>,
    pub bindings: Vec<PluginPresetBinding>,
    pub existing_installation_id: Option<Uuid>,
    pub changes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InstallPluginRequest {
    pub source_path: String,
    pub expected_digest: String,
    #[serde(default)]
    pub expected_old_digest: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginOwnedSkill {
    pub skill_id: Uuid,
    pub name: String,
    pub relative_path: String,
    pub package_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InstalledPlugin {
    pub installation_id: Uuid,
    pub package_id: String,
    pub version: String,
    pub name: String,
    pub digest: String,
    pub source_path: String,
    pub trust: String,
    pub enabled: bool,
    pub phase: PluginPhase,
    #[serde(default)]
    pub cleanup_pending: bool,
    #[serde(default)]
    pub files: Vec<PluginFilePreview>,
    pub skills: Vec<PluginOwnedSkill>,
    pub mcp_bindings: Vec<PluginPresetBinding>,
    pub last_error: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SetPluginEnabledRequest {
    pub installation_id: Uuid,
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RemovePluginRequest {
    pub installation_id: Uuid,
    pub expected_digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginRemovalResult {
    pub installation_id: Uuid,
    pub status: String,
    pub removed_skills: usize,
    pub preserved_files: Vec<String>,
    pub mcp_references_removed: usize,
    pub message: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn install_request_rejects_unknown_wire_fields() {
        let value = json!({
            "source_path": "C:/plugin",
            "expected_digest": "a".repeat(64),
            "expected_old_digest": null,
            "command": "run.exe"
        });
        assert!(serde_json::from_value::<InstallPluginRequest>(value).is_err());
    }

    #[test]
    fn lifecycle_values_are_stable_snake_case() {
        assert_eq!(
            serde_json::to_value(PluginPhase::NeedsAttention).unwrap(),
            json!("needs_attention")
        );
        assert_eq!(
            serde_json::to_value(PluginPhase::Installed).unwrap(),
            json!("installed")
        );
    }
}
