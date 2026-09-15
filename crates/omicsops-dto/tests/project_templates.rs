use omicsops_dto::{
    QuickAction, SaveQuickActionRequest, SaveSpecialistTemplateRequest, SpecialistTemplate,
};
use serde_json::{Value, json};
use uuid::Uuid;

#[test]
fn project_template_wire_shapes_are_stable_and_reject_unknown_fields() {
    let action = QuickAction {
        id: Uuid::from_u128(1),
        project_id: Uuid::from_u128(2),
        name: "QC review".into(),
        description: "Insert the validated QC workflow.".into(),
        workflow_id: Uuid::from_u128(3),
        enabled: true,
    };
    assert_eq!(
        serde_json::to_value(&action).unwrap(),
        json!({
            "id": Uuid::from_u128(1),
            "project_id": Uuid::from_u128(2),
            "name": "QC review",
            "description": "Insert the validated QC workflow.",
            "workflow_id": Uuid::from_u128(3),
            "enabled": true,
        })
    );

    let specialist = SpecialistTemplate {
        id: Uuid::from_u128(4),
        project_id: Uuid::from_u128(2),
        name: "Methods reviewer".into(),
        description: "Review the visible draft.".into(),
        instructions: "Act as a methods reviewer and list unsupported claims.".into(),
        enabled: false,
    };
    assert_eq!(
        serde_json::to_value(&specialist).unwrap(),
        json!({
            "id": Uuid::from_u128(4),
            "project_id": Uuid::from_u128(2),
            "name": "Methods reviewer",
            "description": "Review the visible draft.",
            "instructions": "Act as a methods reviewer and list unsupported claims.",
            "enabled": false,
        })
    );

    let mut action_with_unknown = serde_json::to_value(action).unwrap();
    action_with_unknown["secret"] = Value::String("must not be accepted".into());
    assert!(serde_json::from_value::<QuickAction>(action_with_unknown).is_err());
    let mut specialist_with_unknown = serde_json::to_value(specialist).unwrap();
    specialist_with_unknown["model"] = Value::String("not part of this contract".into());
    assert!(serde_json::from_value::<SpecialistTemplate>(specialist_with_unknown).is_err());
}

#[test]
fn save_requests_support_create_and_explicit_update_ids() {
    let quick_create: SaveQuickActionRequest = serde_json::from_value(json!({
        "project_id": Uuid::from_u128(2),
        "name": "QC review",
        "description": "",
        "workflow_id": Uuid::from_u128(3),
        "enabled": true,
    }))
    .unwrap();
    assert_eq!(quick_create.id, None);

    let specialist_update: SaveSpecialistTemplateRequest = serde_json::from_value(json!({
        "id": Uuid::from_u128(4),
        "project_id": Uuid::from_u128(2),
        "name": "Methods reviewer",
        "description": "",
        "instructions": "Review the draft.",
        "enabled": true,
    }))
    .unwrap();
    assert_eq!(specialist_update.id, Some(Uuid::from_u128(4)));
}
