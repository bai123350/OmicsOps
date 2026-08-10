use chrono::{DateTime, Utc};
use omicsops_core::workspace::{Message, MessageRole};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, State};
use uuid::Uuid;

use crate::commands::AppState;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubmitMessageRequest {
    pub project_id: Uuid,
    pub conversation_id: Uuid,
    pub markdown: String,
    pub sequence: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ConversationEvent {
    pub project_id: Uuid,
    pub conversation_id: Uuid,
    pub message: Message,
}

pub fn user_message_from_request(
    request: SubmitMessageRequest,
    id: Uuid,
    now: DateTime<Utc>,
) -> Result<Message, String> {
    let markdown = request.markdown.trim();
    if markdown.is_empty() {
        return Err("message cannot be empty".into());
    }
    Ok(Message::markdown(
        id,
        request.project_id,
        request.conversation_id,
        request.sequence,
        MessageRole::User,
        markdown,
        now,
    ))
}

#[tauri::command]
pub fn list_messages(
    state: State<'_, AppState>,
    conversation_id: Uuid,
) -> Result<Vec<Message>, String> {
    state
        .repository
        .messages_for_conversation(conversation_id)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn submit_message(
    app: AppHandle,
    state: State<'_, AppState>,
    request: SubmitMessageRequest,
) -> Result<Message, String> {
    let message = user_message_from_request(request, Uuid::new_v4(), Utc::now())?;
    state
        .repository
        .save_message(&message)
        .map_err(|error| error.to_string())?;
    app.emit(
        "conversation-event",
        ConversationEvent {
            project_id: message.project_id,
            conversation_id: message.conversation_id,
            message: message.clone(),
        },
    )
    .map_err(|error| error.to_string())?;
    Ok(message)
}
