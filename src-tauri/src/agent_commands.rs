use chrono::{DateTime, Utc};
use omicsops_core::workspace::{Conversation, Message, MessageRole};
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

#[derive(Debug, Clone, Serialize)]
pub struct ConversationUpdatedEvent {
    pub project_id: Uuid,
    pub conversation: Conversation,
}

pub fn conversation_title_from_first_message(markdown: &str) -> String {
    markdown.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn title_conversation_from_first_message(
    app: &AppHandle,
    repository: &omicsops_adapters::persistence::Repository,
    message: &Message,
) -> Result<(), String> {
    if repository
        .messages_for_conversation(message.conversation_id)
        .map_err(|error| error.to_string())?
        .iter()
        .any(|stored| stored.role == MessageRole::User)
    {
        return Ok(());
    }
    let mut conversation = repository
        .conversations_for_project(message.project_id)
        .map_err(|error| error.to_string())?
        .into_iter()
        .find(|conversation| conversation.id == message.conversation_id)
        .ok_or_else(|| "conversation not found".to_string())?;
    conversation.title = conversation_title_from_first_message(&message.markdown);
    conversation.updated_at = message.created_at;
    repository
        .save_conversation(&conversation)
        .map_err(|error| error.to_string())?;
    app.emit(
        "conversation-updated",
        ConversationUpdatedEvent {
            project_id: message.project_id,
            conversation,
        },
    )
    .map_err(|error| error.to_string())
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
    title_conversation_from_first_message(&app, &state.repository, &message)?;
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn empty_messages_are_rejected_and_titles_are_compacted() {
        assert!(
            user_message_from_request(
                SubmitMessageRequest {
                    project_id: Uuid::new_v4(),
                    conversation_id: Uuid::new_v4(),
                    markdown: "  ".into(),
                    sequence: 1
                },
                Uuid::new_v4(),
                Utc::now()
            )
            .is_err()
        );
        assert_eq!(
            conversation_title_from_first_message("  QC   the data "),
            "QC the data"
        );
    }
}
