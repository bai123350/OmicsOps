//! Conversation-scoped optional Agent preference commands.

use omicsops_dto::ConversationAgentPreferencesV4;
use omicsops_store::Store;
use tauri::State;
use uuid::Uuid;

use crate::commands::AppState;

pub(crate) async fn load_conversation_agent_preferences(
    repository: &Store,
    project_id: Uuid,
    conversation_id: Uuid,
) -> Result<ConversationAgentPreferencesV4, String> {
    repository
        .get_conversation_agent_preferences(project_id, conversation_id)
        .await
        .map_err(|error| error.to_string())
}

pub(crate) async fn get_conversation_agent_preferences_response(
    repository: &Store,
    project_id: Uuid,
    conversation_id: Uuid,
) -> Result<ConversationAgentPreferencesV4, String> {
    load_conversation_agent_preferences(repository, project_id, conversation_id).await
}

pub(crate) async fn save_conversation_agent_preferences_response(
    repository: &Store,
    project_id: Uuid,
    conversation_id: Uuid,
    preferences: ConversationAgentPreferencesV4,
) -> Result<ConversationAgentPreferencesV4, String> {
    repository
        .set_conversation_agent_preferences(project_id, conversation_id, preferences)
        .await
        .map_err(|error| error.to_string())?;
    Ok(preferences)
}

#[tauri::command]
pub async fn conversation_get_agent_preferences_v4(
    state: State<'_, AppState>,
    project_id: Uuid,
    conversation_id: Uuid,
) -> Result<ConversationAgentPreferencesV4, String> {
    get_conversation_agent_preferences_response(&state.repository, project_id, conversation_id)
        .await
}

#[tauri::command]
pub async fn conversation_save_agent_preferences_v4(
    state: State<'_, AppState>,
    project_id: Uuid,
    conversation_id: Uuid,
    preferences: ConversationAgentPreferencesV4,
) -> Result<ConversationAgentPreferencesV4, String> {
    save_conversation_agent_preferences_response(
        &state.repository,
        project_id,
        conversation_id,
        preferences,
    )
    .await
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use omicsops_core::workspace::{Conversation, Project, ProjectTemplate};

    use super::*;

    async fn fixture() -> (Store, Project, Conversation, Project) {
        let repository = Store::open_in_memory().await.unwrap();
        let project = Project::new(
            Uuid::new_v4(),
            "preferences project",
            r"C:\data\preferences-project",
            ProjectTemplate::Blank,
            Utc::now(),
        );
        let other_project = Project::new(
            Uuid::new_v4(),
            "other project",
            r"C:\data\other-project",
            ProjectTemplate::Blank,
            Utc::now(),
        );
        repository.save_project(&project).await.unwrap();
        repository.save_project(&other_project).await.unwrap();
        let conversation = Conversation::new(
            Uuid::new_v4(),
            project.id,
            "preferences conversation",
            Utc::now(),
        );
        repository.save_conversation(&conversation).await.unwrap();
        (repository, project, conversation, other_project)
    }

    #[tokio::test]
    async fn command_helpers_roundtrip_and_default() {
        let (repository, project, conversation, _) = fixture().await;
        assert_eq!(
            get_conversation_agent_preferences_response(&repository, project.id, conversation.id)
                .await
                .unwrap(),
            ConversationAgentPreferencesV4::default()
        );

        let preferences = ConversationAgentPreferencesV4 {
            delegation_enabled: false,
            auto_review: true,
            memory_enabled: false,
            fast_mode: None,
        };
        assert_eq!(
            save_conversation_agent_preferences_response(
                &repository,
                project.id,
                conversation.id,
                preferences,
            )
            .await
            .unwrap(),
            preferences
        );
        assert_eq!(
            get_conversation_agent_preferences_response(&repository, project.id, conversation.id)
                .await
                .unwrap(),
            preferences
        );
    }

    #[tokio::test]
    async fn command_helpers_reject_cross_project_requests() {
        let (repository, project, conversation, other_project) = fixture().await;
        let get_error = get_conversation_agent_preferences_response(
            &repository,
            other_project.id,
            conversation.id,
        )
        .await
        .unwrap_err();
        assert!(get_error.contains("does not belong"));

        let save_error = save_conversation_agent_preferences_response(
            &repository,
            other_project.id,
            conversation.id,
            ConversationAgentPreferencesV4::default(),
        )
        .await
        .unwrap_err();
        assert!(save_error.contains("does not belong"));
        assert_eq!(
            get_conversation_agent_preferences_response(&repository, project.id, conversation.id)
                .await
                .unwrap(),
            ConversationAgentPreferencesV4::default()
        );
    }
}
