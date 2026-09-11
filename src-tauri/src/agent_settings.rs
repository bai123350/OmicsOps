use omicsops_dto::AgentIterationSettingsV4;
use omicsops_store::Store;
use tauri::State;

use crate::commands::AppState;

pub(crate) async fn load_iteration_settings(
    repository: &Store,
) -> Result<AgentIterationSettingsV4, String> {
    repository
        .agent_iteration_settings()
        .await
        .map_err(|error| error.to_string())?
        .map(serde_json::from_value)
        .transpose()
        .map(|settings| settings.unwrap_or_default())
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn agent_get_iteration_settings(
    state: State<'_, AppState>,
) -> Result<AgentIterationSettingsV4, String> {
    load_iteration_settings(&state.repository).await
}

#[tauri::command]
pub async fn agent_save_iteration_settings(
    state: State<'_, AppState>,
    settings: AgentIterationSettingsV4,
) -> Result<AgentIterationSettingsV4, String> {
    state
        .repository
        .save_agent_iteration_settings(
            &serde_json::to_value(&settings).map_err(|error| error.to_string())?,
        )
        .await
        .map_err(|error| error.to_string())?;
    Ok(settings)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn loads_default_and_persisted_unlimited_settings() {
        let repository = Store::open_in_memory().await.unwrap();
        assert_eq!(
            load_iteration_settings(&repository)
                .await
                .unwrap()
                .max_iterations,
            100
        );
        repository
            .save_agent_iteration_settings(&serde_json::json!({"max_iterations": 0}))
            .await
            .unwrap();
        assert_eq!(
            load_iteration_settings(&repository)
                .await
                .unwrap()
                .max_iterations,
            0
        );
    }
}

#[cfg(test)]
mod session_tests {
    use super::*;

    #[tokio::test]
    async fn legacy_and_full_session_settings_load_with_defaults() {
        let repository = Store::open_in_memory().await.unwrap();
        assert_eq!(
            load_iteration_settings(&repository).await.unwrap(),
            AgentIterationSettingsV4::default()
        );
        repository
            .save_agent_iteration_settings(&serde_json::json!({"max_iterations":0}))
            .await
            .unwrap();
        assert_eq!(
            load_iteration_settings(&repository).await.unwrap(),
            AgentIterationSettingsV4 {
                max_iterations: 0,
                ..Default::default()
            }
        );
        let custom = AgentIterationSettingsV4 {
            max_iterations: 25,
            auto_continue: true,
            auto_continue_limit: 0,
            auto_compact: false,
            follow_up_questions: false,
        };
        repository
            .save_agent_iteration_settings(&serde_json::to_value(&custom).unwrap())
            .await
            .unwrap();
        assert_eq!(load_iteration_settings(&repository).await.unwrap(), custom);
    }
}
