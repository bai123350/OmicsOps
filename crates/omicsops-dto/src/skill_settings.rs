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
