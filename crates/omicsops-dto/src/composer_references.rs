use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ComposerReference {
    Artifact {
        project_id: Uuid,
        id: Uuid,
    },
    Session {
        project_id: Uuid,
        id: Uuid,
    },
    Project {
        project_id: Uuid,
        id: Uuid,
    },
    ExecutionContext {
        project_id: Uuid,
        backend_id: String,
    },
    Runtime {
        project_id: Uuid,
        backend_id: String,
        language: String,
    },
    WorkspaceFile {
        project_id: Uuid,
        backend_id: String,
        relative_path: String,
    },
    Workflow {
        project_id: Uuid,
        id: Uuid,
    },
    Quote {
        project_id: Uuid,
        id: Uuid,
    },
    Skill {
        id: Uuid,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ComposerCatalogItem {
    pub reference: ComposerReference,
    pub label: String,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ComposerTextPreview {
    pub project_id: Uuid,
    pub backend_id: String,
    pub relative_path: String,
    pub sha256: String,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateComposerQuoteRequest {
    pub project_id: Uuid,
    pub conversation_id: Uuid,
    pub backend_id: String,
    pub relative_path: String,
    pub sha256: String,
    pub text: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use uuid::Uuid;

    #[test]
    fn references_use_stable_tagged_snake_case_wire_shape() {
        let project_id = Uuid::new_v4();
        let artifact_id = Uuid::new_v4();
        let session_id = Uuid::new_v4();
        let skill_id = Uuid::new_v4();

        assert_eq!(
            serde_json::to_value(ComposerReference::Artifact {
                project_id,
                id: artifact_id,
            })
            .unwrap(),
            json!({"kind":"artifact","project_id":project_id,"id":artifact_id})
        );
        assert_eq!(
            serde_json::to_value(ComposerReference::Session {
                project_id,
                id: session_id,
            })
            .unwrap(),
            json!({"kind":"session","project_id":project_id,"id":session_id})
        );
        assert_eq!(
            serde_json::to_value(ComposerReference::Project {
                project_id,
                id: project_id,
            })
            .unwrap(),
            json!({"kind":"project","project_id":project_id,"id":project_id})
        );
        assert_eq!(
            serde_json::to_value(ComposerReference::ExecutionContext {
                project_id,
                backend_id: "local".into(),
            })
            .unwrap(),
            json!({"kind":"execution_context","project_id":project_id,"backend_id":"local"})
        );
        assert_eq!(
            serde_json::to_value(ComposerReference::Runtime {
                project_id,
                backend_id: "ssh:host".into(),
                language: "python".into(),
            })
            .unwrap(),
            json!({"kind":"runtime","project_id":project_id,"backend_id":"ssh:host","language":"python"})
        );
        assert_eq!(
            serde_json::to_value(ComposerReference::Skill { id: skill_id }).unwrap(),
            json!({"kind":"skill","id":skill_id})
        );
    }

    #[test]
    fn catalog_item_round_trips_without_extra_client_path_fields() {
        let item = ComposerCatalogItem {
            reference: ComposerReference::Skill { id: Uuid::new_v4() },
            label: "workflow".into(),
            description: "Untrusted method guidance".into(),
        };
        let wire = serde_json::to_value(&item).unwrap();
        assert_eq!(wire["label"], "workflow");
        assert_eq!(wire["description"], "Untrusted method guidance");
        assert!(wire.get("source_path").is_none());
        assert_eq!(
            serde_json::from_value::<ComposerCatalogItem>(wire).unwrap(),
            item
        );
    }
}
