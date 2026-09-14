//! Scoped pending queue commands. Payload snapshots are created by the host.
use crate::commands::AppState;
use omicsops_dto::{
    ComposerQueueActionRequestV4, ComposerQueueItemV4, ComposerQueueStatusV4,
    EnqueueComposerTurnRequestV4, UpdateComposerQueueRequestV4,
};
use tauri::{AppHandle, State};
use uuid::Uuid;

use omicsops_dto::{
    ComposerReplacementErrorV4 as ReplacementError, ComposerReplacementFailureKindV4,
};
fn replacement_error(error: omicsops_store::StoreError) -> ReplacementError {
    ReplacementError {
        kind: if matches!(error, omicsops_store::StoreError::InvalidInput(_)) {
            ComposerReplacementFailureKindV4::Rejected
        } else {
            ComposerReplacementFailureKindV4::Unknown
        },
        message:
            "Replacement could not be confirmed. Refresh the run or reconcile the original request."
                .into(),
    }
}

#[tauri::command]
pub async fn composer_queue_replace(
    app: AppHandle,
    state: State<'_, AppState>,
    request: omicsops_dto::ReplaceComposerTurnRequestV4,
) -> Result<omicsops_dto::ComposerReplacementReceiptV4, ReplacementError> {
    let receipt = if let Some(existing) = state
        .repository
        .find_composer_replacement_retry(&request)
        .await
        .map_err(replacement_error)?
    {
        existing
    } else {
        let (frozen, material) = crate::agent_v4::resolve_composer_queue_snapshot(&state, &request.turn).await.map_err(|_| ReplacementError {
            kind: ComposerReplacementFailureKindV4::Rejected, message: "The replacement model, environment or material could not be validated. The draft is retained.".into(),
        })?;
        state
            .repository
            .replace_composer_turn(&request, &frozen, &material)
            .await
            .map_err(replacement_error)?
    };
    // The durable transaction is authoritative even if signalling or emitting
    // fails. Retrying never retargets a newer run or starts a second request.
    let _ = crate::agent_v4::apply_durable_stop(
        &app,
        &state,
        &omicsops_dto::StopRunRequestV4 {
            request_id: receipt.stop.request_id,
            project_id: receipt.project_id,
            conversation_id: receipt.conversation_id,
            run_id: receipt.target_run_id,
        },
    )
    .await;
    crate::composer_queue_driver::kick_composer_queue(
        &app,
        receipt.project_id,
        receipt.conversation_id,
    );
    Ok(receipt)
}

#[tauri::command]
pub async fn composer_queue_list(
    state: State<'_, AppState>,
    project_id: Uuid,
    conversation_id: Uuid,
) -> Result<Vec<ComposerQueueItemV4>, String> {
    state
        .repository
        .list_composer_queue(project_id, conversation_id)
        .await
        .map_err(|_| "The send queue could not be loaded.".into())
}
#[tauri::command]
pub async fn composer_queue_enqueue(
    app: AppHandle,
    state: State<'_, AppState>,
    request: EnqueueComposerTurnRequestV4,
) -> Result<ComposerQueueItemV4, String> {
    if let Some(existing) = state
        .repository
        .find_composer_enqueue_retry(&request)
        .await
        .map_err(|_| "The queue request identity could not be confirmed.")?
    {
        crate::composer_queue_driver::kick_composer_queue(
            &app,
            request.project_id,
            request.conversation_id,
        );
        return Ok(existing);
    }
    let (frozen, material) = crate::agent_v4::resolve_composer_queue_snapshot(&state, &request)
        .await
        .map_err(
            |_| "The queued model, environment or attached material could not be validated.",
        )?;
    let item = state
        .repository
        .enqueue_composer_turn(&request, &frozen, &material)
        .await
        .map_err(|_| {
            "The queued message could not be confirmed. Retry the same request.".to_owned()
        })?;
    crate::composer_queue_driver::kick_composer_queue(&app, item.project_id, item.conversation_id);
    Ok(item)
}
#[tauri::command]
pub async fn composer_queue_update(
    state: State<'_, AppState>,
    request: UpdateComposerQueueRequestV4,
) -> Result<ComposerQueueItemV4, String> {
    let original = state
        .repository
        .list_composer_queue(request.project_id, request.conversation_id)
        .await
        .map_err(|_| "The queue could not be loaded.")?
        .into_iter()
        .find(|item| item.request_id == request.request_id)
        .ok_or("The queued message is unavailable.")?;
    if original.status != ComposerQueueStatusV4::Pending
        || original.revision != request.expected_revision
    {
        return Err("The queued message changed. Refresh the queue before editing.".into());
    }
    let material = crate::agent_v4::resolve_composer_queue_material(
        &state,
        request.project_id,
        request.conversation_id,
        &original.frozen,
        &request.references,
        &request.attachments,
    )
    .await
    .map_err(|_| "The queued material could not be validated; the original message is retained.")?;
    state
        .repository
        .update_composer_queue(&request, &material)
        .await
        .map_err(|_| {
            "The queue edit could not be confirmed. Refresh the queue before retrying.".into()
        })
}
#[tauri::command]
pub async fn composer_queue_action(
    state: State<'_, AppState>,
    request: ComposerQueueActionRequestV4,
) -> Result<Vec<ComposerQueueItemV4>, String> {
    state
        .repository
        .composer_queue_action(&request)
        .await
        .map_err(|_| {
            "The queue action could not be confirmed. Refresh the queue before retrying.".into()
        })
}

#[tauri::command]
pub async fn composer_queue_reconcile(
    app: AppHandle,
    state: State<'_, AppState>,
    project_id: Uuid,
    conversation_id: Uuid,
) -> Result<Vec<ComposerQueueItemV4>, String> {
    let items = state
        .repository
        .reconcile_composer_queue(project_id, conversation_id, chrono::Utc::now())
        .await
        .map_err(|_| "The send queue could not be reconciled.".to_owned())?;
    crate::composer_queue_driver::kick_composer_queue(&app, project_id, conversation_id);
    Ok(items)
}
