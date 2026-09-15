use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillOrigin {
    Bundled,
    ManagedImport,
    PluginOwned,
    LegacyUnknown,
    External,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SkillInstallationReceipt {
    pub skill_id: Uuid,
    pub package_sha256: String,
    pub installed_root: String,
    pub origin: SkillOrigin,
    pub owns_files: bool,
    pub plugin_installation_id: Option<Uuid>,
    pub phase: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SkillSettingsPackage {
    pub id: Uuid,
    pub name: String,
    pub version: String,
    pub source_path: String,
    pub sha256: String,
    pub enabled: bool,
    pub capabilities: Vec<String>,
    pub category: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SkillSettingsFile {
    pub relative_path: String,
    pub size_bytes: u64,
    pub previewable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SkillSettingsDetail {
    pub skill: SkillSettingsPackage,
    pub origin: SkillOrigin,
    pub integrity: String,
    pub files: Vec<SkillSettingsFile>,
    pub inventory_complete: bool,
    pub dependent_skills: Vec<Uuid>,
    pub can_remove_from_library: bool,
    pub can_delete_files: bool,
    pub blocking_reasons: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SkillFilePreview {
    pub relative_path: String,
    pub content: String,
    pub redacted: bool,
    pub package_sha256: String,
}
