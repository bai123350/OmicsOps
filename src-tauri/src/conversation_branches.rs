//! Branch commands expose only scoped transcript copies, never run authority.
use omicsops_dto::{
    BranchCreateFailureKindV4, BranchCreateFailureV4, ConversationBranchCheckpointKindV4,
    ConversationBranchCheckpointV4, ConversationBranchSendReceiptV4, ConversationBranchV4,
    CreateConversationBranchAndSendRequestV4, CreateConversationBranchRequestV4,
    EnqueueComposerTurnRequestV4,
};
use tauri::{AppHandle, State};
use uuid::Uuid;

use crate::commands::AppState;
use crate::conversation_branch_material::{
    branch_queue_attachments, branch_queue_references, copy_branch_material,
};

#[tauri::command]
pub async fn conversation_branch_checkpoint_v4(
    state: State<'_, AppState>,
    project_id: Uuid,
    source_conversation_id: Uuid,
    source_message_id: Uuid,
    checkpoint_kind: ConversationBranchCheckpointKindV4,
) -> Result<ConversationBranchCheckpointV4, String> {
    state
        .repository
        .get_conversation_branch_checkpoint_v4(
            project_id,
            source_conversation_id,
            source_message_id,
            checkpoint_kind,
        )
        .await
        .map_err(|_| {
            "The branch checkpoint is unavailable. Refresh the conversation and retry.".into()
        })
}
#[tauri::command]
pub async fn conversation_branch_create_v4(
    state: State<'_, AppState>,
    request: CreateConversationBranchRequestV4,
) -> Result<ConversationBranchV4, BranchCreateFailureV4> {
    state
        .repository
        .create_conversation_branch_v4(&request)
        .await
        .map_err(|error| BranchCreateFailureV4 {
            kind: if matches!(error, omicsops_store::StoreError::InvalidInput(_)) {
                BranchCreateFailureKindV4::Rejected
            } else {
                BranchCreateFailureKindV4::Uncertain
            },
            message:
                "The branch could not be confirmed. Retry the same request or refresh its source."
                    .into(),
        })
}

/// Create a branch, preserve its selected composer material, and enqueue the
/// complete first turn in the new conversation. The queue driver owns actual
/// execution; this command never calls the ordinary submit/start path.
#[tauri::command]
pub async fn conversation_branch_create_and_send_v4(
    app: AppHandle,
    state: State<'_, AppState>,
    request: CreateConversationBranchAndSendRequestV4,
) -> Result<ConversationBranchSendReceiptV4, BranchCreateFailureV4> {
    let branch = state
        .repository
        .prepare_conversation_branch_send_v4(&request)
        .await
        .map_err(|error| map_branch_store_error(error, false))?;

    let queue_request = EnqueueComposerTurnRequestV4 {
        request_id: request.queue_request_id,
        message_id: request.queue_message_id,
        run_id: request.queue_run_id,
        project_id: branch.project_id,
        conversation_id: branch.branch_conversation_id,
        mode: request.mode,
        message_markdown: request.message_markdown.clone(),
        model_profile_id: request.model_profile_id,
        compute_selection: request.compute_selection.clone(),
        references: branch_queue_references(branch.branch_conversation_id, &request.references),
        attachments: branch_queue_attachments(branch.branch_conversation_id, &request.attachments),
    };

    // Once the queue row exists, its frozen material and request hash are the
    // durable acceptance record. A retry must return that receipt before
    // inspecting mutable source attachments/quotes or resolving a live model
    // profile; those sources may have changed after the original enqueue.
    let existing_queue = state
        .repository
        .find_composer_enqueue_retry(&queue_request)
        .await
        .map_err(|_| branch_send_retry_failure())?;
    if let Some(queue) = existing_queue {
        crate::composer_queue_driver::kick_composer_queue(
            &app,
            queue.project_id,
            queue.conversation_id,
        );
        return Ok(ConversationBranchSendReceiptV4 { branch, queue });
    }

    let copied = copy_branch_material(
        &state,
        request.branch.project_id,
        request.branch.source_conversation_id,
        branch.branch_conversation_id,
        &request.attachments,
        &request.references,
    )
    .await
    .map_err(|_| branch_send_retry_failure())?;
    let copied_attachment_ids = copied
        .attachments
        .iter()
        .map(|receipt| receipt.id)
        .collect::<Vec<_>>();
    if copied.references != queue_request.references
        || copied_attachment_ids != queue_request.attachments
    {
        return Err(branch_send_retry_failure());
    }
    let queue = match state
        .repository
        .find_composer_enqueue_retry(&queue_request)
        .await
    {
        Ok(Some(existing)) => existing,
        Ok(None) => {
            let (frozen, material) =
                crate::agent_v4::resolve_composer_queue_snapshot(&state, &queue_request)
                    .await
                    .map_err(|_| branch_send_retry_failure())?;
            state
                .repository
                .enqueue_composer_turn(&queue_request, &frozen, &material)
                .await
                .map_err(|_| branch_send_retry_failure())?
        }
        Err(_) => return Err(branch_send_retry_failure()),
    };
    crate::composer_queue_driver::kick_composer_queue(
        &app,
        queue.project_id,
        queue.conversation_id,
    );
    Ok(ConversationBranchSendReceiptV4 { branch, queue })
}
#[tauri::command]
pub async fn conversation_branch_get_v4(
    state: State<'_, AppState>,
    project_id: Uuid,
    branch_conversation_id: Uuid,
) -> Result<Option<ConversationBranchV4>, String> {
    state
        .repository
        .get_conversation_branch_v4(project_id, branch_conversation_id)
        .await
        .map_err(|_| "The branch source could not be loaded.".into())
}
#[tauri::command]
pub async fn conversation_branch_list_v4(
    state: State<'_, AppState>,
    project_id: Uuid,
    source_conversation_id: Uuid,
) -> Result<Vec<ConversationBranchV4>, String> {
    state
        .repository
        .conversation_branches_for_source_v4(project_id, source_conversation_id)
        .await
        .map_err(|_| "Conversation branches could not be loaded.".into())
}

fn map_branch_store_error(
    error: omicsops_store::StoreError,
    _after_branch_creation: bool,
) -> BranchCreateFailureV4 {
    BranchCreateFailureV4 {
        kind: if matches!(error, omicsops_store::StoreError::InvalidInput(_)) {
            BranchCreateFailureKindV4::Rejected
        } else {
            BranchCreateFailureKindV4::Uncertain
        },
        message:
            "The branch send could not be confirmed. Retry the same request or refresh its source."
                .into(),
    }
}

fn branch_send_retry_failure() -> BranchCreateFailureV4 {
    BranchCreateFailureV4 {
        kind: BranchCreateFailureKindV4::Uncertain,
        message: "The branch was preserved but its queued turn was not confirmed. Retry the same request.".into(),
    }
}
