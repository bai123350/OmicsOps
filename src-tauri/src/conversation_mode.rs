//! Conversation-scoped Agent Runtime V4 mode commands.

use omicsops_dto::{
    GetConversationAgentModeResponseV4, SetConversationAgentModeRequestV4,
    SetConversationAgentModeResponseV4,
};
use omicsops_store::Store;
use tauri::State;
use uuid::Uuid;

use crate::commands::AppState;

pub(crate) async fn get_conversation_agent_mode_response(
    repository: &Store,
    project_id: Uuid,
    conversation_id: Uuid,
) -> Result<GetConversationAgentModeResponseV4, String> {
    let mode = repository
        .get_conversation_agent_mode(project_id, conversation_id)
        .await
        .map_err(|error| error.to_string())?;
    Ok(GetConversationAgentModeResponseV4 {
        project_id,
        conversation_id,
        mode,
    })
}

pub(crate) async fn set_conversation_agent_mode_response(
    repository: &Store,
    request: SetConversationAgentModeRequestV4,
) -> Result<SetConversationAgentModeResponseV4, String> {
    repository
        .set_conversation_agent_mode(request.project_id, request.conversation_id, request.mode)
        .await
        .map_err(|error| error.to_string())?;
    Ok(SetConversationAgentModeResponseV4 {
        project_id: request.project_id,
        conversation_id: request.conversation_id,
        mode: request.mode,
    })
}

#[tauri::command]
pub async fn get_conversation_agent_mode(
    state: State<'_, AppState>,
    project_id: Uuid,
    conversation_id: Uuid,
) -> Result<GetConversationAgentModeResponseV4, String> {
    get_conversation_agent_mode_response(&state.repository, project_id, conversation_id).await
}

#[tauri::command]
pub async fn set_conversation_agent_mode(
    state: State<'_, AppState>,
    request: SetConversationAgentModeRequestV4,
) -> Result<SetConversationAgentModeResponseV4, String> {
    set_conversation_agent_mode_response(&state.repository, request).await
}
