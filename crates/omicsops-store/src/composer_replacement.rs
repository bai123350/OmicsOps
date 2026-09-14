//! Atomic replacement admission and exact-target settlement fences.
use crate::{Store, StoreError};
use chrono::Utc;
use omicsops_dto::{
    ComposerQueueFrozenConfigV4, ComposerQueueMaterialSnapshotV4, ComposerReplacementReceiptV4,
    ReplaceComposerTurnRequestV4, StopRunRequestV4,
};
use omicsops_protocol::{AgentEventKindV4, validate_event_chain_v4};
use sha2::{Digest, Sha256};
use sqlx::{Row, SqliteConnection};
use uuid::Uuid;

fn invalid(message: &str) -> StoreError {
    StoreError::InvalidInput(message.into())
}
fn request_hash(request: &ReplaceComposerTurnRequestV4) -> Result<String, StoreError> {
    Ok(hex::encode(Sha256::digest(serde_json::to_vec(request)?)))
}

impl Store {
    /// Retry lookup must happen before resolving mutable model/material state.
    pub async fn find_composer_replacement_retry(
        &self,
        request: &ReplaceComposerTurnRequestV4,
    ) -> Result<Option<ComposerReplacementReceiptV4>, StoreError> {
        let mut tx = self.pool.begin().await?;
        let result = retry_in_tx(&mut tx, request).await?;
        tx.commit().await?;
        Ok(result)
    }

    pub async fn replace_composer_turn(
        &self,
        request: &ReplaceComposerTurnRequestV4,
        frozen: &ComposerQueueFrozenConfigV4,
        material: &ComposerQueueMaterialSnapshotV4,
    ) -> Result<ComposerReplacementReceiptV4, StoreError> {
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        if let Some(receipt) = retry_in_tx(&mut tx, request).await? {
            tx.commit().await?;
            return Ok(receipt);
        }
        let turn = &request.turn;
        if request.stop_request_id.is_nil()
            || request.target_run_id.is_nil()
            || turn.run_id == request.target_run_id
        {
            return Err(invalid("invalid replacement identity"));
        }
        crate::ensure_run_owner_executor(
            &mut *tx,
            turn.project_id,
            turn.conversation_id,
            request.target_run_id,
        )
        .await?;
        let status: String = sqlx::query_scalar("SELECT status FROM agent_runs_v4 WHERE run_id=?1")
            .bind(request.target_run_id.to_string())
            .fetch_one(&mut *tx)
            .await?;
        if !matches!(
            status.as_str(),
            "running"
                | "planning"
                | "waiting_for_input"
                | "waiting_for_approval"
                | "awaiting_approval"
        ) {
            return Err(invalid("replacement target is no longer active"));
        }
        let events = crate::load_agent_events_in_tx(&mut tx, request.target_run_id).await?;
        validate_event_chain_v4(&events).map_err(|_| invalid("invalid target event chain"))?;
        // Progress appended while the host resolves material is legitimate.
        // Bind the user's observed ancestor, not a race with the latest event.
        let head = events
            .iter()
            .find(|event| {
                event.sequence == request.expected_event_sequence
                    && event.event_hash == request.expected_event_hash
            })
            .ok_or_else(|| invalid("replacement observed event boundary changed"))?;
        if events.iter().any(|event| {
            event.project_id != turn.project_id
                || event.conversation_id != turn.conversation_id
                || event.run_id != request.target_run_id
                || crate::is_terminal_event(event)
        }) {
            return Err(invalid(
                "replacement target changed; refresh before replacing",
            ));
        }
        let other_active: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM agent_runs_v4 WHERE project_id=?1 AND conversation_id=?2 AND run_id<>?3 AND status IN ('running','planning','waiting_for_input','waiting_for_approval','awaiting_approval','needs_attention')")
            .bind(turn.project_id.to_string()).bind(turn.conversation_id.to_string()).bind(request.target_run_id.to_string()).fetch_one(&mut *tx).await?;
        if other_active != 0 {
            return Err(invalid("another run owns the conversation"));
        }
        let already_replaced: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM composer_replacements_v4 WHERE target_run_id=?1",
        )
        .bind(request.target_run_id.to_string())
        .fetch_one(&mut *tx)
        .await?;
        if already_replaced != 0
            || crate::composer_queue::queue_row_by_request(&mut tx, turn.request_id)
                .await?
                .is_some()
        {
            return Err(invalid("replacement identity is already reserved"));
        }
        let source_message: Option<String> = sqlx::query_scalar("SELECT message_id FROM composer_queue_v4 WHERE run_id=?1 AND project_id=?2 AND conversation_id=?3")
            .bind(request.target_run_id.to_string()).bind(turn.project_id.to_string()).bind(turn.conversation_id.to_string()).fetch_optional(&mut *tx).await?;
        crate::composer_queue::enqueue_composer_turn_in_tx(&mut tx, turn, frozen, material).await?;
        let stop = crate::run_stops::request_run_stop_in_tx(
            &mut tx,
            &StopRunRequestV4 {
                request_id: request.stop_request_id,
                project_id: turn.project_id,
                conversation_id: turn.conversation_id,
                run_id: request.target_run_id,
            },
        )
        .await?;
        prioritize_in_tx(&mut tx, turn.conversation_id, turn.request_id).await?;
        let receipt = ComposerReplacementReceiptV4 {
            request_id: turn.request_id,
            project_id: turn.project_id,
            conversation_id: turn.conversation_id,
            target_run_id: request.target_run_id,
            source_message_id: source_message
                .map(|value| crate::parse_uuid(&value, "source message"))
                .transpose()?,
            source_event_sequence: head.sequence,
            source_event_hash: head.event_hash.clone(),
            stop,
            accepted_at: Utc::now(),
        };
        sqlx::query("INSERT INTO composer_replacements_v4(request_id,target_run_id,request_hash,receipt_json) VALUES (?1,?2,?3,?4)")
            .bind(turn.request_id.to_string()).bind(request.target_run_id.to_string()).bind(request_hash(request)?).bind(serde_json::to_string(&receipt)?).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(receipt)
    }
}

async fn retry_in_tx(
    tx: &mut SqliteConnection,
    request: &ReplaceComposerTurnRequestV4,
) -> Result<Option<ComposerReplacementReceiptV4>, StoreError> {
    let row = sqlx::query(
        "SELECT request_hash,receipt_json FROM composer_replacements_v4 WHERE request_id=?1",
    )
    .bind(request.turn.request_id.to_string())
    .fetch_optional(&mut *tx)
    .await?;
    let Some(row) = row else { return Ok(None) };
    if row.try_get::<String, _>(0)? != request_hash(request)? {
        return Err(invalid("replacement request identity conflict"));
    }
    crate::ensure_conversation_owner_executor(
        &mut *tx,
        request.turn.project_id,
        request.turn.conversation_id,
    )
    .await?;
    Ok(Some(serde_json::from_str(&row.try_get::<String, _>(1)?)?))
}

/// Rotate pending slots only; history and the relative order of ordinary rows
/// remain intact. Revision changes invalidate stale edits in other windows.
async fn prioritize_in_tx(
    tx: &mut SqliteConnection,
    conversation: Uuid,
    request: Uuid,
) -> Result<(), StoreError> {
    let rows = sqlx::query("SELECT request_id,position FROM composer_queue_v4 WHERE conversation_id=?1 AND status='pending' ORDER BY position")
        .bind(conversation.to_string()).fetch_all(&mut *tx).await?;
    if rows.len() <= 1 {
        return Ok(());
    }
    let last = rows.last().unwrap();
    if last.try_get::<String, _>(0)? != request.to_string() {
        return Err(invalid("replacement position changed"));
    }
    let temp = last
        .try_get::<i64, _>(1)?
        .checked_add(1)
        .ok_or_else(|| invalid("queue position exhausted"))?;
    sqlx::query("UPDATE composer_queue_v4 SET position=?1 WHERE request_id=?2")
        .bind(temp)
        .bind(request.to_string())
        .execute(&mut *tx)
        .await?;
    for index in (0..rows.len() - 1).rev() {
        let changed = sqlx::query("UPDATE composer_queue_v4 SET position=?1,revision=revision+1,updated_at=?2 WHERE request_id=?3 AND revision<9223372036854775807")
            .bind(rows[index+1].try_get::<i64,_>(1)?).bind(crate::timestamp(Utc::now())).bind(rows[index].try_get::<String,_>(0)?).execute(&mut *tx).await?;
        if changed.rows_affected() != 1 {
            return Err(invalid("queue revision exhausted"));
        }
    }
    sqlx::query("UPDATE composer_queue_v4 SET position=?1 WHERE request_id=?2")
        .bind(rows[0].try_get::<i64, _>(1)?)
        .bind(request.to_string())
        .execute(&mut *tx)
        .await?;
    Ok(())
}

/// Both claim and dispatch recheck the exact old run, even if a stale row
/// reports 'completed' or its Stop was already marked observed.
pub(super) async fn ensure_replacement_settled_in_tx(
    tx: &mut SqliteConnection,
    request: Uuid,
) -> Result<(), StoreError> {
    let receipt: Option<String> =
        sqlx::query_scalar("SELECT receipt_json FROM composer_replacements_v4 WHERE request_id=?1")
            .bind(request.to_string())
            .fetch_optional(&mut *tx)
            .await?;
    let Some(receipt) = receipt else {
        return Ok(());
    };
    let receipt: ComposerReplacementReceiptV4 = serde_json::from_str(&receipt)?;
    crate::ensure_run_owner_executor(
        &mut *tx,
        receipt.project_id,
        receipt.conversation_id,
        receipt.target_run_id,
    )
    .await?;
    let events = crate::load_agent_events_in_tx(&mut *tx, receipt.target_run_id).await?;
    validate_event_chain_v4(&events)
        .map_err(|_| invalid("replacement target evidence is invalid"))?;
    if events.iter().any(|event| {
        event.project_id != receipt.project_id
            || event.conversation_id != receipt.conversation_id
            || event.run_id != receipt.target_run_id
    }) || !events.iter().any(|event| {
        event.sequence == receipt.source_event_sequence
            && event.event_hash == receipt.source_event_hash
    }) || crate::has_unresolved_side_effect_dispatch(&events)
        || !events.last().is_some_and(|event| {
            matches!(
                event.event,
                AgentEventKindV4::RunCompleted
                    | AgentEventKindV4::RunCancelled
                    | AgentEventKindV4::RunFailed { .. }
            )
        })
    {
        return Err(invalid("replacement is waiting for safe target settlement"));
    }
    Ok(())
}

pub(super) async fn ensure_ordinary_queue_item_in_tx(
    tx: &mut SqliteConnection,
    request: Uuid,
) -> Result<(), StoreError> {
    let exists: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM composer_replacements_v4 WHERE request_id=?1")
            .bind(request.to_string())
            .fetch_one(&mut *tx)
            .await?;
    if exists != 0 {
        return Err(invalid(
            "replacement payload and priority are immutable; cancel it to restore a draft",
        ));
    }
    Ok(())
}
