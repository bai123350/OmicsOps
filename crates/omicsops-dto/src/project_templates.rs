use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A project-owned shortcut to an existing composer workflow template.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QuickAction {
    pub id: Uuid,
    pub project_id: Uuid,
    pub name: String,
    pub description: String,
    pub workflow_id: Uuid,
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SaveQuickActionRequest {
    #[serde(default)]
    pub id: Option<Uuid>,
    pub project_id: Uuid,
    pub name: String,
    pub description: String,
    pub workflow_id: Uuid,
    pub enabled: bool,
}

/// User-visible project draft text. It is not a hidden system prompt or an
/// independently executing agent configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpecialistTemplate {
    pub id: Uuid,
    pub project_id: Uuid,
    pub name: String,
    pub description: String,
    pub instructions: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SaveSpecialistTemplateRequest {
    #[serde(default)]
    pub id: Option<Uuid>,
    pub project_id: Uuid,
    pub name: String,
    pub description: String,
    pub instructions: String,
    pub enabled: bool,
}
