use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A project-owned ordered recipe that can be attached to a composer request.
///
/// The host validates and resolves every recipe.  The steps are guidance for
/// the existing Agent/Plan path, not an independent scheduler contract.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ComposerWorkflowTemplate {
    pub id: Uuid,
    pub project_id: Uuid,
    pub name: String,
    pub description: String,
    pub steps: Vec<String>,
    pub enabled: bool,
}

/// Host request used for both creating and updating a project workflow.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SaveComposerWorkflowRequest {
    #[serde(default)]
    pub id: Option<Uuid>,
    pub project_id: Uuid,
    pub name: String,
    pub description: String,
    pub steps: Vec<String>,
    pub enabled: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};
    use uuid::Uuid;

    #[test]
    fn workflow_template_wire_shape_is_stable() {
        let template = ComposerWorkflowTemplate {
            id: Uuid::from_u128(1),
            project_id: Uuid::from_u128(2),
            name: "QC then report".into(),
            description: "Run the ordered QC recipe.".into(),
            steps: vec!["Check counts".into(), "Write report".into()],
            enabled: true,
        };

        assert_eq!(
            serde_json::to_value(template).unwrap(),
            json!({
                "id": Uuid::from_u128(1),
                "project_id": Uuid::from_u128(2),
                "name": "QC then report",
                "description": "Run the ordered QC recipe.",
                "steps": ["Check counts", "Write report"],
                "enabled": true,
            })
        );
    }

    #[test]
    fn save_request_accepts_new_and_existing_ids() {
        let new_request = SaveComposerWorkflowRequest {
            id: None,
            project_id: Uuid::from_u128(2),
            name: "Recipe".into(),
            description: String::new(),
            steps: vec!["Do the work".into()],
            enabled: true,
        };
        let existing_request = SaveComposerWorkflowRequest {
            id: Some(Uuid::from_u128(3)),
            ..new_request.clone()
        };

        assert_eq!(
            serde_json::to_value(new_request).unwrap()["id"],
            Value::Null
        );
        assert_eq!(
            serde_json::from_value::<SaveComposerWorkflowRequest>(
                serde_json::to_value(existing_request.clone()).unwrap()
            )
            .unwrap(),
            existing_request
        );
        let omitted_id = serde_json::from_value::<SaveComposerWorkflowRequest>(json!({
            "project_id": Uuid::from_u128(2),
            "name": "Recipe",
            "description": "",
            "steps": ["Do the work"],
            "enabled": true,
        }))
        .unwrap();
        assert_eq!(omitted_id.id, None);
    }

    #[test]
    fn workflow_dtos_reject_unknown_fields() {
        let value = json!({
            "id": Uuid::from_u128(1),
            "project_id": Uuid::from_u128(2),
            "name": "Recipe",
            "description": "",
            "steps": ["Do the work"],
            "enabled": true,
            "source_path": "C:/secret.txt",
        });

        assert!(serde_json::from_value::<ComposerWorkflowTemplate>(value.clone()).is_err());
        assert!(serde_json::from_value::<SaveComposerWorkflowRequest>(value).is_err());
    }
}
