use omicsops_dto::{
    ComposerWorkflowTemplate, QuickAction, SaveQuickActionRequest, SaveSpecialistTemplateRequest,
    SpecialistTemplate,
};
use omicsops_store::Store;
use tauri::State;
use uuid::Uuid;

use crate::commands::AppState;

pub const QUICK_ACTION_STORE_KIND: &str = "quick_action_v1";
pub const SPECIALIST_TEMPLATE_STORE_KIND: &str = "specialist_template_v1";
pub const MAX_PROJECT_TEMPLATES: usize = 100;
pub const MAX_TEMPLATE_NAME_CHARS: usize = 100;
pub const MAX_TEMPLATE_DESCRIPTION_CHARS: usize = 500;
pub const MAX_SPECIALIST_INSTRUCTIONS_BYTES: usize = 16 * 1024;

const TEMPLATE_STORAGE_ERROR: &str = "project template storage is unavailable";
const INVALID_TEMPLATE_STORAGE_ERROR: &str = "project template storage contains invalid data";
const SENSITIVE_TEMPLATE_ERROR: &str = "specialist template contains prohibited sensitive content";
const SENSITIVE_ACTION_ERROR: &str = "quick action contains prohibited sensitive content";
static PROJECT_TEMPLATE_MUTATIONS: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[tauri::command]
pub async fn list_quick_actions(
    state: State<'_, AppState>,
    project_id: Uuid,
) -> Result<Vec<QuickAction>, String> {
    list_quick_actions_for_repository(&state.repository, project_id).await
}

#[tauri::command]
pub async fn save_quick_action(
    state: State<'_, AppState>,
    request: SaveQuickActionRequest,
) -> Result<QuickAction, String> {
    save_quick_action_for_repository(&state.repository, request).await
}

#[tauri::command]
pub async fn delete_quick_action(
    state: State<'_, AppState>,
    project_id: Uuid,
    id: Uuid,
) -> Result<(), String> {
    delete_quick_action_for_repository(&state.repository, project_id, id).await
}

#[tauri::command]
pub async fn list_specialist_templates(
    state: State<'_, AppState>,
    project_id: Uuid,
) -> Result<Vec<SpecialistTemplate>, String> {
    list_specialist_templates_for_repository(&state.repository, project_id).await
}

#[tauri::command]
pub async fn save_specialist_template(
    state: State<'_, AppState>,
    request: SaveSpecialistTemplateRequest,
) -> Result<SpecialistTemplate, String> {
    save_specialist_template_for_repository(&state.repository, request).await
}

#[tauri::command]
pub async fn delete_specialist_template(
    state: State<'_, AppState>,
    project_id: Uuid,
    id: Uuid,
) -> Result<(), String> {
    delete_specialist_template_for_repository(&state.repository, project_id, id).await
}

pub async fn list_quick_actions_for_repository(
    repository: &Store,
    project_id: Uuid,
) -> Result<Vec<QuickAction>, String> {
    ensure_project(repository, project_id).await?;
    let records = repository
        .list_json::<QuickAction>(QUICK_ACTION_STORE_KIND)
        .await
        .map_err(|_| TEMPLATE_STORAGE_ERROR.to_owned())?;
    let mut actions = Vec::new();
    for action in records {
        if action.project_id != project_id {
            continue;
        }
        validate_quick_action(&action).map_err(|_| INVALID_TEMPLATE_STORAGE_ERROR.to_owned())?;
        actions.push(action);
    }
    actions.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(actions)
}

pub async fn save_quick_action_for_repository(
    repository: &Store,
    request: SaveQuickActionRequest,
) -> Result<QuickAction, String> {
    ensure_project(repository, request.project_id).await?;
    let _mutation = PROJECT_TEMPLATE_MUTATIONS.lock().await;
    let id = request.id.unwrap_or_else(Uuid::new_v4);
    if id.is_nil() {
        return Err("quick action id is invalid".into());
    }
    let existing = repository
        .get_json::<QuickAction>(QUICK_ACTION_STORE_KIND, &id.to_string())
        .await
        .map_err(|_| TEMPLATE_STORAGE_ERROR.to_owned())?;
    validate_existing_quick_action(existing.as_ref(), request.id, request.project_id, id)?;
    validate_workflow_binding(repository, request.project_id, request.workflow_id).await?;

    let action = QuickAction {
        id,
        project_id: request.project_id,
        name: request.name.trim().to_owned(),
        description: request.description.trim().to_owned(),
        workflow_id: request.workflow_id,
        enabled: request.enabled,
    };
    validate_quick_action(&action)?;
    if existing.is_none() {
        ensure_project_count::<QuickAction>(
            repository,
            QUICK_ACTION_STORE_KIND,
            action.project_id,
            |item| item.project_id,
            "quick action",
        )
        .await?;
    }
    repository
        .put_json(QUICK_ACTION_STORE_KIND, &action.id.to_string(), &action)
        .await
        .map_err(|_| TEMPLATE_STORAGE_ERROR.to_owned())?;
    Ok(action)
}

pub async fn delete_quick_action_for_repository(
    repository: &Store,
    project_id: Uuid,
    id: Uuid,
) -> Result<(), String> {
    ensure_project(repository, project_id).await?;
    let _mutation = PROJECT_TEMPLATE_MUTATIONS.lock().await;
    let existing = repository
        .get_json::<QuickAction>(QUICK_ACTION_STORE_KIND, &id.to_string())
        .await
        .map_err(|_| TEMPLATE_STORAGE_ERROR.to_owned())?
        .ok_or_else(|| "quick action was not found".to_owned())?;
    if existing.id != id {
        return Err("quick action id does not match its storage key".into());
    }
    if existing.project_id != project_id {
        return Err("quick action belongs to a different project".into());
    }
    repository
        .delete_json(QUICK_ACTION_STORE_KIND, &id.to_string())
        .await
        .map_err(|_| TEMPLATE_STORAGE_ERROR.to_owned())?
        .then_some(())
        .ok_or_else(|| "quick action was not found".to_owned())
}

pub async fn list_specialist_templates_for_repository(
    repository: &Store,
    project_id: Uuid,
) -> Result<Vec<SpecialistTemplate>, String> {
    ensure_project(repository, project_id).await?;
    let records = repository
        .list_json::<SpecialistTemplate>(SPECIALIST_TEMPLATE_STORE_KIND)
        .await
        .map_err(|_| TEMPLATE_STORAGE_ERROR.to_owned())?;
    let mut templates = Vec::new();
    for template in records {
        if template.project_id != project_id {
            continue;
        }
        validate_specialist_template(&template)
            .map_err(|_| INVALID_TEMPLATE_STORAGE_ERROR.to_owned())?;
        templates.push(template);
    }
    templates.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(templates)
}

pub async fn save_specialist_template_for_repository(
    repository: &Store,
    request: SaveSpecialistTemplateRequest,
) -> Result<SpecialistTemplate, String> {
    ensure_project(repository, request.project_id).await?;
    let _mutation = PROJECT_TEMPLATE_MUTATIONS.lock().await;
    let id = request.id.unwrap_or_else(Uuid::new_v4);
    if id.is_nil() {
        return Err("specialist template id is invalid".into());
    }
    let existing = repository
        .get_json::<SpecialistTemplate>(SPECIALIST_TEMPLATE_STORE_KIND, &id.to_string())
        .await
        .map_err(|_| TEMPLATE_STORAGE_ERROR.to_owned())?;
    validate_existing_specialist(existing.as_ref(), request.id, request.project_id, id)?;

    let template = SpecialistTemplate {
        id,
        project_id: request.project_id,
        name: request.name.trim().to_owned(),
        description: request.description.trim().to_owned(),
        instructions: request.instructions.trim().to_owned(),
        enabled: request.enabled,
    };
    validate_specialist_template(&template)?;
    if existing.is_none() {
        ensure_project_count::<SpecialistTemplate>(
            repository,
            SPECIALIST_TEMPLATE_STORE_KIND,
            template.project_id,
            |item| item.project_id,
            "specialist template",
        )
        .await?;
    }
    repository
        .put_json(
            SPECIALIST_TEMPLATE_STORE_KIND,
            &template.id.to_string(),
            &template,
        )
        .await
        .map_err(|_| TEMPLATE_STORAGE_ERROR.to_owned())?;
    Ok(template)
}

pub async fn delete_specialist_template_for_repository(
    repository: &Store,
    project_id: Uuid,
    id: Uuid,
) -> Result<(), String> {
    ensure_project(repository, project_id).await?;
    let _mutation = PROJECT_TEMPLATE_MUTATIONS.lock().await;
    let existing = repository
        .get_json::<SpecialistTemplate>(SPECIALIST_TEMPLATE_STORE_KIND, &id.to_string())
        .await
        .map_err(|_| TEMPLATE_STORAGE_ERROR.to_owned())?
        .ok_or_else(|| "specialist template was not found".to_owned())?;
    if existing.id != id {
        return Err("specialist template id does not match its storage key".into());
    }
    if existing.project_id != project_id {
        return Err("specialist template belongs to a different project".into());
    }
    repository
        .delete_json(SPECIALIST_TEMPLATE_STORE_KIND, &id.to_string())
        .await
        .map_err(|_| TEMPLATE_STORAGE_ERROR.to_owned())?
        .then_some(())
        .ok_or_else(|| "specialist template was not found".to_owned())
}

async fn ensure_project(repository: &Store, project_id: Uuid) -> Result<(), String> {
    repository
        .get_project(project_id)
        .await
        .map_err(|_| TEMPLATE_STORAGE_ERROR.to_owned())?
        .map(|_| ())
        .ok_or_else(|| "project was not found".to_owned())
}

async fn validate_workflow_binding(
    repository: &Store,
    project_id: Uuid,
    workflow_id: Uuid,
) -> Result<(), String> {
    let workflow = repository
        .get_json::<ComposerWorkflowTemplate>(
            crate::composer_workflows::WORKFLOW_STORE_KIND,
            &workflow_id.to_string(),
        )
        .await
        .map_err(|_| TEMPLATE_STORAGE_ERROR.to_owned())?
        .ok_or_else(|| "workflow was not found".to_owned())?;
    if workflow.id != workflow_id {
        return Err("workflow id does not match its storage key".into());
    }
    if workflow.project_id != project_id {
        return Err("workflow belongs to a different project".into());
    }
    if !workflow.enabled {
        return Err("workflow is disabled".into());
    }
    crate::composer_workflows::resolve_workflow_reference(repository, project_id, workflow_id)
        .await
        .map(|_| ())
        .map_err(|_| "workflow is invalid".to_owned())
}

fn validate_existing_quick_action(
    existing: Option<&QuickAction>,
    requested_id: Option<Uuid>,
    project_id: Uuid,
    id: Uuid,
) -> Result<(), String> {
    if existing.is_none() && requested_id.is_some() {
        return Err("quick action was not found".into());
    }
    if let Some(existing) = existing {
        if existing.id != id {
            return Err("quick action id does not match its storage key".into());
        }
        if existing.project_id != project_id {
            return Err("quick action belongs to a different project".into());
        }
    }
    Ok(())
}

fn validate_existing_specialist(
    existing: Option<&SpecialistTemplate>,
    requested_id: Option<Uuid>,
    project_id: Uuid,
    id: Uuid,
) -> Result<(), String> {
    if existing.is_none() && requested_id.is_some() {
        return Err("specialist template was not found".into());
    }
    if let Some(existing) = existing {
        if existing.id != id {
            return Err("specialist template id does not match its storage key".into());
        }
        if existing.project_id != project_id {
            return Err("specialist template belongs to a different project".into());
        }
    }
    Ok(())
}

async fn ensure_project_count<T>(
    repository: &Store,
    kind: &str,
    project_id: Uuid,
    owner: impl Fn(&T) -> Uuid,
    label: &str,
) -> Result<(), String>
where
    T: serde::de::DeserializeOwned,
{
    let count = repository
        .list_json::<T>(kind)
        .await
        .map_err(|_| TEMPLATE_STORAGE_ERROR.to_owned())?
        .iter()
        .filter(|item| owner(item) == project_id)
        .count();
    if count >= MAX_PROJECT_TEMPLATES {
        return Err(format!("project {label} limit of 100 reached"));
    }
    Ok(())
}

fn validate_quick_action(action: &QuickAction) -> Result<(), String> {
    validate_common_fields("quick action", &action.name, &action.description)?;
    reject_sensitive_text(&action.name, SENSITIVE_ACTION_ERROR)?;
    reject_sensitive_text(&action.description, SENSITIVE_ACTION_ERROR)
}

fn validate_specialist_template(template: &SpecialistTemplate) -> Result<(), String> {
    validate_common_fields("specialist", &template.name, &template.description)?;
    if template.instructions.is_empty() {
        return Err("specialist instructions are required".into());
    }
    if template.instructions.len() > MAX_SPECIALIST_INSTRUCTIONS_BYTES {
        return Err("specialist instructions exceed 16 KiB".into());
    }
    reject_sensitive_text(&template.name, SENSITIVE_TEMPLATE_ERROR)?;
    reject_sensitive_text(&template.description, SENSITIVE_TEMPLATE_ERROR)?;
    reject_sensitive_text(&template.instructions, SENSITIVE_TEMPLATE_ERROR)
}

fn validate_common_fields(label: &str, name: &str, description: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err(format!("{label} name is required"));
    }
    if name.chars().count() > MAX_TEMPLATE_NAME_CHARS {
        return Err(format!("{label} name exceeds 100 characters"));
    }
    if description.chars().count() > MAX_TEMPLATE_DESCRIPTION_CHARS {
        return Err(format!("{label} description exceeds 500 characters"));
    }
    Ok(())
}

fn reject_sensitive_text(value: &str, message: &str) -> Result<(), String> {
    if crate::composer_references::public_text(value) != value {
        return Err(message.into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use omicsops_core::workspace::{Project, ProjectTemplate};
    use omicsops_dto::{
        ComposerWorkflowTemplate, SaveQuickActionRequest, SaveSpecialistTemplateRequest,
    };
    use omicsops_store::Store;
    use tempfile::tempdir;
    use uuid::Uuid;

    use super::*;

    async fn store_with_projects(project_ids: &[Uuid]) -> Store {
        let store = Store::open_in_memory().await.unwrap();
        for (index, project_id) in project_ids.iter().enumerate() {
            store
                .save_project(&Project::new(
                    *project_id,
                    format!("Project {index}"),
                    format!("C:/omics/project-{index}"),
                    ProjectTemplate::Blank,
                    Utc::now(),
                ))
                .await
                .unwrap();
        }
        store
    }

    async fn workflow(store: &Store, project_id: Uuid, id: Uuid, enabled: bool) {
        store
            .put_json(
                crate::composer_workflows::WORKFLOW_STORE_KIND,
                &id.to_string(),
                &ComposerWorkflowTemplate {
                    id,
                    project_id,
                    name: "QC workflow".into(),
                    description: String::new(),
                    steps: vec!["Inspect counts".into()],
                    enabled,
                },
            )
            .await
            .unwrap();
    }

    fn action_request(project_id: Uuid, workflow_id: Uuid) -> SaveQuickActionRequest {
        SaveQuickActionRequest {
            id: None,
            project_id,
            name: "Review QC".into(),
            description: "Insert the QC workflow reference".into(),
            workflow_id,
            enabled: true,
        }
    }

    fn specialist_request(project_id: Uuid) -> SaveSpecialistTemplateRequest {
        SaveSpecialistTemplateRequest {
            id: None,
            project_id,
            name: "Methods reviewer".into(),
            description: "Review the draft methods".into(),
            instructions: "Act as a methods reviewer and identify unsupported claims.".into(),
            enabled: true,
        }
    }

    #[tokio::test]
    async fn quick_actions_are_project_scoped_and_require_an_enabled_owned_workflow() {
        let first = Uuid::from_u128(1);
        let second = Uuid::from_u128(2);
        let enabled = Uuid::from_u128(10);
        let disabled = Uuid::from_u128(11);
        let store = store_with_projects(&[first, second]).await;
        workflow(&store, first, enabled, true).await;
        workflow(&store, first, disabled, false).await;

        let saved = save_quick_action_for_repository(&store, action_request(first, enabled))
            .await
            .unwrap();
        assert_eq!(
            list_quick_actions_for_repository(&store, first)
                .await
                .unwrap(),
            vec![saved]
        );
        assert!(
            list_quick_actions_for_repository(&store, second)
                .await
                .unwrap()
                .is_empty()
        );

        let cross_project =
            save_quick_action_for_repository(&store, action_request(second, enabled))
                .await
                .unwrap_err();
        assert_eq!(cross_project, "workflow belongs to a different project");
        let unavailable = save_quick_action_for_repository(&store, action_request(first, disabled))
            .await
            .unwrap_err();
        assert_eq!(unavailable, "workflow is disabled");
        let missing =
            save_quick_action_for_repository(&store, action_request(first, Uuid::from_u128(99)))
                .await
                .unwrap_err();
        assert_eq!(missing, "workflow was not found");
    }

    #[tokio::test]
    async fn quick_action_updates_and_deletes_cannot_change_owners_or_delete_workflows() {
        let first = Uuid::from_u128(1);
        let second = Uuid::from_u128(2);
        let workflow_id = Uuid::from_u128(10);
        let store = store_with_projects(&[first, second]).await;
        workflow(&store, first, workflow_id, true).await;
        let saved = save_quick_action_for_repository(&store, action_request(first, workflow_id))
            .await
            .unwrap();

        let mut cross_project = action_request(second, workflow_id);
        cross_project.id = Some(saved.id);
        assert_eq!(
            save_quick_action_for_repository(&store, cross_project)
                .await
                .unwrap_err(),
            "quick action belongs to a different project"
        );
        assert_eq!(
            delete_quick_action_for_repository(&store, second, saved.id)
                .await
                .unwrap_err(),
            "quick action belongs to a different project"
        );
        assert_eq!(
            delete_quick_action_for_repository(&store, first, Uuid::from_u128(99))
                .await
                .unwrap_err(),
            "quick action was not found"
        );

        delete_quick_action_for_repository(&store, first, saved.id)
            .await
            .unwrap();
        assert!(
            list_quick_actions_for_repository(&store, first)
                .await
                .unwrap()
                .is_empty()
        );
        assert!(
            store
                .get_json::<ComposerWorkflowTemplate>(
                    crate::composer_workflows::WORKFLOW_STORE_KIND,
                    &workflow_id.to_string(),
                )
                .await
                .unwrap()
                .is_some()
        );
    }

    #[tokio::test]
    async fn disabled_quick_action_state_survives_a_store_reopen() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("project-templates.sqlite");
        let project_id = Uuid::from_u128(1);
        let workflow_id = Uuid::from_u128(10);
        let store = Store::open(&path).await.unwrap();
        store
            .save_project(&Project::new(
                project_id,
                "Project",
                "C:/omics/project",
                ProjectTemplate::Blank,
                Utc::now(),
            ))
            .await
            .unwrap();
        workflow(&store, project_id, workflow_id, true).await;
        let mut request = action_request(project_id, workflow_id);
        request.enabled = false;
        let saved = save_quick_action_for_repository(&store, request)
            .await
            .unwrap();
        drop(store);

        let reopened = Store::open(&path).await.unwrap();
        assert_eq!(
            list_quick_actions_for_repository(&reopened, project_id)
                .await
                .unwrap(),
            vec![saved]
        );
    }

    #[tokio::test]
    async fn specialists_are_project_scoped_and_enforce_update_and_delete_ownership() {
        let first = Uuid::from_u128(1);
        let second = Uuid::from_u128(2);
        let store = store_with_projects(&[first, second]).await;
        let saved = save_specialist_template_for_repository(&store, specialist_request(first))
            .await
            .unwrap();
        assert_eq!(
            list_specialist_templates_for_repository(&store, first)
                .await
                .unwrap(),
            vec![saved.clone()]
        );
        assert!(
            list_specialist_templates_for_repository(&store, second)
                .await
                .unwrap()
                .is_empty()
        );

        let mut cross_project = specialist_request(second);
        cross_project.id = Some(saved.id);
        assert_eq!(
            save_specialist_template_for_repository(&store, cross_project)
                .await
                .unwrap_err(),
            "specialist template belongs to a different project"
        );
        assert_eq!(
            delete_specialist_template_for_repository(&store, second, saved.id)
                .await
                .unwrap_err(),
            "specialist template belongs to a different project"
        );
        delete_specialist_template_for_repository(&store, first, saved.id)
            .await
            .unwrap();
        assert!(
            list_specialist_templates_for_repository(&store, first)
                .await
                .unwrap()
                .is_empty()
        );
    }

    #[tokio::test]
    async fn explicit_updates_of_unknown_template_ids_are_rejected() {
        let project_id = Uuid::from_u128(1);
        let workflow_id = Uuid::from_u128(10);
        let store = store_with_projects(&[project_id]).await;
        workflow(&store, project_id, workflow_id, true).await;
        let mut action = action_request(project_id, workflow_id);
        action.id = Some(Uuid::from_u128(80));
        assert_eq!(
            save_quick_action_for_repository(&store, action)
                .await
                .unwrap_err(),
            "quick action was not found"
        );
        let mut specialist = specialist_request(project_id);
        specialist.id = Some(Uuid::from_u128(81));
        assert_eq!(
            save_specialist_template_for_repository(&store, specialist)
                .await
                .unwrap_err(),
            "specialist template was not found"
        );
    }

    #[tokio::test]
    async fn limits_and_sensitive_content_are_rejected_without_echoing_secrets() {
        let project_id = Uuid::from_u128(1);
        let workflow_id = Uuid::from_u128(10);
        let store = store_with_projects(&[project_id]).await;
        workflow(&store, project_id, workflow_id, true).await;

        let mut action = action_request(project_id, workflow_id);
        action.name = "n".repeat(101);
        assert_eq!(
            save_quick_action_for_repository(&store, action)
                .await
                .unwrap_err(),
            "quick action name exceeds 100 characters"
        );
        let mut specialist = specialist_request(project_id);
        specialist.description = "d".repeat(501);
        assert_eq!(
            save_specialist_template_for_repository(&store, specialist)
                .await
                .unwrap_err(),
            "specialist description exceeds 500 characters"
        );
        let mut specialist = specialist_request(project_id);
        specialist.instructions = "i".repeat(16 * 1024 + 1);
        assert_eq!(
            save_specialist_template_for_repository(&store, specialist)
                .await
                .unwrap_err(),
            "specialist instructions exceed 16 KiB"
        );
        let secret = "password=do-not-store-this";
        let mut specialist = specialist_request(project_id);
        specialist.instructions = secret.into();
        let error = save_specialist_template_for_repository(&store, specialist)
            .await
            .unwrap_err();
        assert_eq!(
            error,
            "specialist template contains prohibited sensitive content"
        );
        assert!(!error.contains(secret));
        assert!(
            list_specialist_templates_for_repository(&store, project_id)
                .await
                .unwrap()
                .is_empty()
        );
    }

    #[tokio::test]
    async fn each_project_catalog_stops_at_one_hundred_records_but_allows_updates() {
        let project_id = Uuid::from_u128(1);
        let workflow_id = Uuid::from_u128(10);
        let store = store_with_projects(&[project_id]).await;
        workflow(&store, project_id, workflow_id, true).await;
        for index in 0..MAX_PROJECT_TEMPLATES {
            let action = omicsops_dto::QuickAction {
                id: Uuid::from_u128(1_000 + index as u128),
                project_id,
                name: format!("Action {index}"),
                description: String::new(),
                workflow_id,
                enabled: true,
            };
            store
                .put_json(QUICK_ACTION_STORE_KIND, &action.id.to_string(), &action)
                .await
                .unwrap();
            let specialist = omicsops_dto::SpecialistTemplate {
                id: Uuid::from_u128(2_000 + index as u128),
                project_id,
                name: format!("Specialist {index}"),
                description: String::new(),
                instructions: "Review the visible draft.".into(),
                enabled: true,
            };
            store
                .put_json(
                    SPECIALIST_TEMPLATE_STORE_KIND,
                    &specialist.id.to_string(),
                    &specialist,
                )
                .await
                .unwrap();
        }

        assert_eq!(
            save_quick_action_for_repository(&store, action_request(project_id, workflow_id))
                .await
                .unwrap_err(),
            "project quick action limit of 100 reached"
        );
        assert_eq!(
            save_specialist_template_for_repository(&store, specialist_request(project_id))
                .await
                .unwrap_err(),
            "project specialist template limit of 100 reached"
        );

        let mut update = action_request(project_id, workflow_id);
        update.id = Some(Uuid::from_u128(1_000));
        update.enabled = false;
        assert!(
            !save_quick_action_for_repository(&store, update)
                .await
                .unwrap()
                .enabled
        );
    }

    #[tokio::test]
    async fn concurrent_creates_cannot_cross_the_project_limit() {
        let project_id = Uuid::from_u128(1);
        let workflow_id = Uuid::from_u128(10);
        let store = store_with_projects(&[project_id]).await;
        workflow(&store, project_id, workflow_id, true).await;
        for index in 0..99_u128 {
            let action = omicsops_dto::QuickAction {
                id: Uuid::from_u128(1_000 + index),
                project_id,
                name: format!("Action {index}"),
                description: String::new(),
                workflow_id,
                enabled: true,
            };
            store
                .put_json(QUICK_ACTION_STORE_KIND, &action.id.to_string(), &action)
                .await
                .unwrap();
        }

        let request = |index| {
            let mut request = action_request(project_id, workflow_id);
            request.name = format!("Concurrent {index}");
            request
        };
        let results = tokio::join!(
            save_quick_action_for_repository(&store, request(0)),
            save_quick_action_for_repository(&store, request(1)),
            save_quick_action_for_repository(&store, request(2)),
            save_quick_action_for_repository(&store, request(3)),
            save_quick_action_for_repository(&store, request(4)),
            save_quick_action_for_repository(&store, request(5)),
            save_quick_action_for_repository(&store, request(6)),
            save_quick_action_for_repository(&store, request(7)),
        );
        let results = [
            results.0, results.1, results.2, results.3, results.4, results.5, results.6, results.7,
        ];

        assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
        assert_eq!(
            list_quick_actions_for_repository(&store, project_id)
                .await
                .unwrap()
                .len(),
            100
        );
    }

    #[tokio::test]
    async fn concurrent_update_and_delete_cannot_resurrect_a_quick_action() {
        let project_id = Uuid::from_u128(1);
        let workflow_id = Uuid::from_u128(10);
        let store = store_with_projects(&[project_id]).await;
        workflow(&store, project_id, workflow_id, true).await;
        let saved =
            save_quick_action_for_repository(&store, action_request(project_id, workflow_id))
                .await
                .unwrap();
        let mut update = action_request(project_id, workflow_id);
        update.id = Some(saved.id);
        update.name = "Late update".into();

        let (delete_result, update_result) = tokio::join!(
            delete_quick_action_for_repository(&store, project_id, saved.id),
            save_quick_action_for_repository(&store, update),
        );

        assert!(delete_result.is_ok());
        if let Err(error) = update_result {
            assert_eq!(error, "quick action was not found");
        }
        assert!(
            list_quick_actions_for_repository(&store, project_id)
                .await
                .unwrap()
                .is_empty()
        );
    }
}
