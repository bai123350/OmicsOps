//! Conversation-scoped optional Agent preference commands.

use chrono::Utc;
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

#[cfg(test)]
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

pub(crate) async fn save_conversation_agent_preferences_with_deadline_response(
    repository: &Store,
    project_id: Uuid,
    conversation_id: Uuid,
    preferences: ConversationAgentPreferencesV4,
    expires_at_ms: Option<i64>,
) -> Result<ConversationAgentPreferencesV4, String> {
    let expires_at_ms = expires_at_ms.unwrap_or_else(|| Utc::now().timestamp_millis() + 10_000);
    repository
        .set_conversation_agent_preferences_with_deadline(
            project_id,
            conversation_id,
            preferences,
            expires_at_ms,
        )
        .await
        .map_err(|error| error.to_string())?;
    Ok(preferences)
}

pub(crate) async fn reconcile_conversation_agent_preferences_response(
    repository: &Store,
    project_id: Uuid,
    conversation_id: Uuid,
    expires_at_ms: i64,
) -> Result<ConversationAgentPreferencesV4, String> {
    repository
        .reconcile_conversation_agent_preferences_after_deadline(
            project_id,
            conversation_id,
            expires_at_ms,
        )
        .await
        .map_err(|error| error.to_string())
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
    expires_at_ms: Option<i64>,
) -> Result<ConversationAgentPreferencesV4, String> {
    save_conversation_agent_preferences_with_deadline_response(
        &state.repository,
        project_id,
        conversation_id,
        preferences,
        expires_at_ms,
    )
    .await
}

#[tauri::command]
pub async fn conversation_reconcile_agent_preferences_v4(
    state: State<'_, AppState>,
    project_id: Uuid,
    conversation_id: Uuid,
    expires_at_ms: i64,
) -> Result<ConversationAgentPreferencesV4, String> {
    reconcile_conversation_agent_preferences_response(
        &state.repository,
        project_id,
        conversation_id,
        expires_at_ms,
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

    #[tokio::test]
    async fn deadline_save_and_fenced_read_keep_project_scope() {
        let (repository, project, conversation, other_project) = fixture().await;
        let candidate = ConversationAgentPreferencesV4 {
            delegation_enabled: false,
            ..Default::default()
        };
        let future = Utc::now().timestamp_millis() + 10_000;
        assert!(
            save_conversation_agent_preferences_with_deadline_response(
                &repository,
                project.id,
                conversation.id,
                candidate,
                Some(Utc::now().timestamp_millis() - 1),
            )
            .await
            .is_err()
        );
        assert!(
            save_conversation_agent_preferences_with_deadline_response(
                &repository,
                project.id,
                conversation.id,
                candidate,
                Some(Utc::now().timestamp_millis() + 31_000),
            )
            .await
            .is_err()
        );
        assert!(
            reconcile_conversation_agent_preferences_response(
                &repository,
                project.id,
                conversation.id,
                future,
            )
            .await
            .is_err()
        );
        assert_eq!(
            save_conversation_agent_preferences_with_deadline_response(
                &repository,
                project.id,
                conversation.id,
                candidate,
                Some(future),
            )
            .await
            .unwrap(),
            candidate
        );
        assert!(
            reconcile_conversation_agent_preferences_response(
                &repository,
                other_project.id,
                conversation.id,
                Utc::now().timestamp_millis() - 1,
            )
            .await
            .is_err()
        );
        assert_eq!(
            reconcile_conversation_agent_preferences_response(
                &repository,
                project.id,
                conversation.id,
                Utc::now().timestamp_millis() - 1,
            )
            .await
            .unwrap(),
            candidate
        );
    }
}
