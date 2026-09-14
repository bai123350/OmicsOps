use omicsops_dto::ComposerReference;
use tauri::State;
use uuid::Uuid;

use crate::commands::AppState;

/// Validate composer references against the current host-owned project state
/// without creating a run or persisting any message.
#[tauri::command]
pub async fn validate_composer_references(
    state: State<'_, AppState>,
    project_id: Uuid,
    conversation_id: Uuid,
    references: Vec<ComposerReference>,
) -> Result<(), String> {
    crate::composer_references::resolve_composer_references(
        &state.repository,
        project_id,
        conversation_id,
        &references,
    )
    .await?;
    crate::composer_files::validate_file_reference_sources(&state, project_id, &references).await
}
