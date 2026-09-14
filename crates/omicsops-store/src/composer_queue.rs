use std::collections::HashSet;

use chrono::Utc;
use omicsops_dto::{
    ComposerQueueActionRequestV4, ComposerQueueActionV4, ComposerQueueFrozenConfigV4,
    ComposerQueueItemV4, ComposerQueueMaterialSnapshotV4, ComposerQueueModeV4,
    ComposerQueueStatusV4, ComposerReference, EnqueueComposerTurnRequestV4,
    SubmitGuidanceV4Request, UpdateComposerQueueRequestV4,
};
use omicsops_protocol::{RunExecutionKindV4, RunSpecV4};
use serde::Serialize;
use sha2::{Digest, Sha256};
use sqlx::{Row, Sqlite, Transaction, sqlite::SqliteRow};
use uuid::Uuid;

use super::{
    Store, StoreError, ensure_conversation_owner_executor, from_timestamp, parse_json_enum,
    parse_uuid, timestamp,
};

const MAX_QUEUE_NON_TERMINAL: i64 = 100;
const MAX_MESSAGE_BYTES: usize = 64 * 1024;
const MAX_REFERENCES: usize = 12;
const MAX_SESSION_REFERENCES: usize = 3;
const MAX_ATTACHMENTS: usize = 8;
const MAX_ATTACHMENT_BYTES: u64 = 20 * 1024 * 1024;
const MAX_TOTAL_ATTACHMENT_BYTES: u64 = 40 * 1024 * 1024;
const MAX_REFERENCE_CONTEXT_BYTES: usize = 64 * 1024;
const MAX_ATTACHMENT_NAME_BYTES: usize = 255;
const MAX_ATTACHMENT_RELATIVE_PATH_BYTES: usize = 1024;
const MAX_SNAPSHOT_JSON_BYTES: usize = 256 * 1024;

pub(super) const QUEUE_COLUMNS: &str = "request_id,message_id,run_id,project_id,conversation_id,position,revision,mode,message_markdown,status,request_hash,frozen_json,references_json,attachments_json,material_json,created_at,updated_at,failure_code,cutin_message_id,(SELECT target_run_id FROM composer_replacements_v4 WHERE composer_replacements_v4.request_id=composer_queue_v4.request_id),(SELECT receipt_json FROM composer_replacements_v4 WHERE composer_replacements_v4.request_id=composer_queue_v4.request_id)";

impl Store {
    /// Find an existing enqueue request using only the original canonical
    /// request hash. This is intentionally safe to call before the host loads
    /// the current model profile: an edited row or a deleted profile must not
    /// make a lost enqueue response create a second logical request.
    pub async fn find_composer_enqueue_retry(
        &self,
        request: &EnqueueComposerTurnRequestV4,
    ) -> Result<Option<ComposerQueueItemV4>, StoreError> {
        validate_request_ids(request)?;
        let request_hash = canonical_request_hash(request)?;
        let mut tx = self.pool.begin().await?;
        let row = sqlx::query(&format!(
            "SELECT {QUEUE_COLUMNS} FROM composer_queue_v4 WHERE request_id=?1"
        ))
        .bind(request.request_id.to_string())
        .fetch_optional(&mut *tx)
        .await?;
        let Some(row) = row else {
            tx.commit().await?;
            return Ok(None);
        };
        let stored_hash = row.try_get::<String, _>(10)?;
        let stored_project = row.try_get::<String, _>(3)?;
        let stored_conversation = row.try_get::<String, _>(4)?;
        if stored_hash != request_hash
            || stored_project != request.project_id.to_string()
            || stored_conversation != request.conversation_id.to_string()
        {
            return Err(StoreError::InvalidInput(
                "queue request id already belongs to a different logical request".into(),
            ));
        }
        ensure_conversation_owner_executor(&mut *tx, request.project_id, request.conversation_id)
            .await?;
        let item = queue_item_from_row(row)?;
        tx.commit().await?;
        Ok(Some(item))
    }

    /// Atomically append one pending composer request. The host passes the
    /// already frozen configuration and material snapshot; this Store method
    /// persists both as typed JSON while retaining the original request hash
    /// for idempotent retries.
    pub async fn enqueue_composer_turn(
        &self,
        request: &EnqueueComposerTurnRequestV4,
        frozen: &ComposerQueueFrozenConfigV4,
        material: &ComposerQueueMaterialSnapshotV4,
    ) -> Result<ComposerQueueItemV4, StoreError> {
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let item = enqueue_composer_turn_in_tx(&mut tx, request, frozen, material).await?;
        tx.commit().await?;
        Ok(item)
    }
    pub async fn list_composer_queue(
        &self,
        project_id: Uuid,
        conversation_id: Uuid,
    ) -> Result<Vec<ComposerQueueItemV4>, StoreError> {
        let mut tx = self.pool.begin().await?;
        ensure_conversation_owner_executor(&mut *tx, project_id, conversation_id).await?;
        let items = queue_items_in_tx(&mut tx, project_id, conversation_id).await?;
        tx.commit().await?;
        Ok(items)
    }

    /// Replace the text and material payload of one pending row using a
    /// scoped revision CAS. The original request hash and frozen config are
    /// deliberately untouched.
    pub async fn update_composer_queue(
        &self,
        request: &UpdateComposerQueueRequestV4,
        material: &ComposerQueueMaterialSnapshotV4,
    ) -> Result<ComposerQueueItemV4, StoreError> {
        validate_payload(
            request.project_id,
            &request.message_markdown,
            &request.references,
            &request.attachments,
        )?;
        if request.expected_revision == 0 {
            return Err(StoreError::InvalidInput(
                "queue revision must be positive".into(),
            ));
        }
        validate_material_snapshot(
            request.project_id,
            request.conversation_id,
            &request.attachments,
            material,
        )?;

        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        crate::composer_replacement::ensure_ordinary_queue_item_in_tx(&mut tx, request.request_id)
            .await?;
        ensure_conversation_owner_executor(&mut *tx, request.project_id, request.conversation_id)
            .await?;
        let existing = pending_target(
            &mut tx,
            request.project_id,
            request.conversation_id,
            request.request_id,
            request.expected_revision,
        )
        .await?;
        let frozen_json = existing.try_get::<String, _>(11)?;
        let frozen: ComposerQueueFrozenConfigV4 =
            parse_snapshot(&frozen_json, "frozen queue config")?;
        validate_frozen_snapshot(&frozen)?;
        let references_json = serde_json::to_string(&request.references)?;
        let attachments_json = serde_json::to_string(&request.attachments)?;
        let material_json = serde_json::to_string(material)?;
        validate_snapshot_json_size(material_json.as_bytes(), "queued material snapshot")?;
        let next_revision = next_revision(existing.try_get::<i64, _>(6)?)?;
        let now = timestamp(Utc::now());
        let updated = sqlx::query(
            "UPDATE composer_queue_v4
             SET message_markdown=?1,references_json=?2,attachments_json=?3,
                 material_json=?4,revision=?5,updated_at=?6
             WHERE request_id=?7 AND project_id=?8 AND conversation_id=?9
               AND revision=?10 AND status='pending'",
        )
        .bind(&request.message_markdown)
        .bind(references_json)
        .bind(attachments_json)
        .bind(material_json)
        .bind(next_revision)
        .bind(now)
        .bind(request.request_id.to_string())
        .bind(request.project_id.to_string())
        .bind(request.conversation_id.to_string())
        .bind(request.expected_revision as i64)
        .execute(&mut *tx)
        .await?;
        if updated.rows_affected() != 1 {
            return Err(StoreError::InvalidInput("queue revision conflict".into()));
        }
        let row = queue_row_by_request(&mut tx, request.request_id)
            .await?
            .ok_or_else(|| {
                StoreError::InvalidInput("queued item disappeared after update".into())
            })?;
        let item = queue_item_from_row(row)?;
        tx.commit().await?;
        Ok(item)
    }

    pub async fn cancel_composer_queue(
        &self,
        request: &ComposerQueueActionRequestV4,
    ) -> Result<ComposerQueueItemV4, StoreError> {
        if request.action != ComposerQueueActionV4::Cancel {
            return Err(StoreError::InvalidInput(
                "queue action is not cancel".into(),
            ));
        }
        if request.expected_revision == 0 {
            return Err(StoreError::InvalidInput(
                "queue revision must be positive".into(),
            ));
        }
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        ensure_conversation_owner_executor(&mut *tx, request.project_id, request.conversation_id)
            .await?;
        let existing = pending_target(
            &mut tx,
            request.project_id,
            request.conversation_id,
            request.request_id,
            request.expected_revision,
        )
        .await?;
        let next_revision = next_revision(existing.try_get::<i64, _>(6)?)?;
        let updated = sqlx::query(
            "UPDATE composer_queue_v4
             SET status='cancelled',revision=?1,updated_at=?2
             WHERE request_id=?3 AND project_id=?4 AND conversation_id=?5
               AND revision=?6 AND status='pending'",
        )
        .bind(next_revision)
        .bind(timestamp(Utc::now()))
        .bind(request.request_id.to_string())
        .bind(request.project_id.to_string())
        .bind(request.conversation_id.to_string())
        .bind(request.expected_revision as i64)
        .execute(&mut *tx)
        .await?;
        if updated.rows_affected() != 1 {
            return Err(StoreError::InvalidInput("queue revision conflict".into()));
        }
        let row = queue_row_by_request(&mut tx, request.request_id)
            .await?
            .ok_or_else(|| {
                StoreError::InvalidInput("queued item disappeared after cancel".into())
            })?;
        let item = queue_item_from_row(row)?;
        tx.commit().await?;
        Ok(item)
    }

    /// Move one pending row across its nearest pending neighbour. The
    /// temporary position is above the current maximum, so the unique
    /// conversation-position constraint is never violated during the swap.
    pub async fn move_composer_queue(
        &self,
        request: &ComposerQueueActionRequestV4,
    ) -> Result<Vec<ComposerQueueItemV4>, StoreError> {
        if !matches!(
            request.action,
            ComposerQueueActionV4::MoveUp | ComposerQueueActionV4::MoveDown
        ) {
            return Err(StoreError::InvalidInput(
                "queue action is not a movement".into(),
            ));
        }
        if request.expected_revision == 0 {
            return Err(StoreError::InvalidInput(
                "queue revision must be positive".into(),
            ));
        }
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        crate::composer_replacement::ensure_ordinary_queue_item_in_tx(&mut tx, request.request_id)
            .await?;
        ensure_conversation_owner_executor(&mut *tx, request.project_id, request.conversation_id)
            .await?;
        let target = pending_target(
            &mut tx,
            request.project_id,
            request.conversation_id,
            request.request_id,
            request.expected_revision,
        )
        .await?;
        let target_position = target.try_get::<i64, _>(5)?;
        let neighbour = match request.action {
            ComposerQueueActionV4::MoveUp => {
                sqlx::query(
                    "SELECT request_id,position,revision FROM composer_queue_v4
                     WHERE project_id=?1 AND conversation_id=?2 AND status='pending'
                       AND position < ?3 ORDER BY position DESC LIMIT 1",
                )
                .bind(request.project_id.to_string())
                .bind(request.conversation_id.to_string())
                .bind(target_position)
                .fetch_optional(&mut *tx)
                .await?
            }
            ComposerQueueActionV4::MoveDown => {
                sqlx::query(
                    "SELECT request_id,position,revision FROM composer_queue_v4
                     WHERE project_id=?1 AND conversation_id=?2 AND status='pending'
                       AND position > ?3 ORDER BY position LIMIT 1",
                )
                .bind(request.project_id.to_string())
                .bind(request.conversation_id.to_string())
                .bind(target_position)
                .fetch_optional(&mut *tx)
                .await?
            }
            ComposerQueueActionV4::Cancel | ComposerQueueActionV4::CutIn => unreachable!(),
        };

        if let Some(neighbour) = neighbour {
            let neighbour_id = neighbour.try_get::<String, _>(0)?;
            crate::composer_replacement::ensure_ordinary_queue_item_in_tx(
                &mut tx,
                parse_uuid(&neighbour_id, "queue neighbour")?,
            )
            .await?;
            let neighbour_position = neighbour.try_get::<i64, _>(1)?;
            let target_revision = next_revision(target.try_get::<i64, _>(6)?)?;
            let neighbour_revision = next_revision(neighbour.try_get::<i64, _>(2)?)?;
            let max_position: i64 = sqlx::query_scalar(
                "SELECT COALESCE(MAX(position),0) FROM composer_queue_v4
                 WHERE conversation_id=?1",
            )
            .bind(request.conversation_id.to_string())
            .fetch_one(&mut *tx)
            .await?;
            let temporary_position = max_position
                .checked_add(1)
                .filter(|value| *value > 0)
                .ok_or_else(|| {
                    StoreError::InvalidInput("composer queue position exceeds SQLite range".into())
                })?;
            let now = timestamp(Utc::now());
            let moved_to_temporary = sqlx::query(
                "UPDATE composer_queue_v4 SET position=?1,revision=?2,updated_at=?3
                 WHERE request_id=?4 AND project_id=?5 AND conversation_id=?6
                   AND position=?7 AND revision=?8 AND status='pending'",
            )
            .bind(temporary_position)
            .bind(target_revision)
            .bind(now)
            .bind(request.request_id.to_string())
            .bind(request.project_id.to_string())
            .bind(request.conversation_id.to_string())
            .bind(target_position)
            .bind(request.expected_revision as i64)
            .execute(&mut *tx)
            .await?;
            if moved_to_temporary.rows_affected() != 1 {
                return Err(StoreError::InvalidInput("queue revision conflict".into()));
            }
            let moved_neighbour = sqlx::query(
                "UPDATE composer_queue_v4 SET position=?1,revision=?2,updated_at=?3
                 WHERE request_id=?4 AND project_id=?5 AND conversation_id=?6
                   AND position=?7 AND revision=?8 AND status='pending'",
            )
            .bind(target_position)
            .bind(neighbour_revision)
            .bind(now)
            .bind(&neighbour_id)
            .bind(request.project_id.to_string())
            .bind(request.conversation_id.to_string())
            .bind(neighbour_position)
            .bind(neighbour.try_get::<i64, _>(2)?)
            .execute(&mut *tx)
            .await?;
            if moved_neighbour.rows_affected() != 1 {
                return Err(StoreError::InvalidInput("queue neighbour changed".into()));
            }
            let moved_target = sqlx::query(
                "UPDATE composer_queue_v4 SET position=?1
                 WHERE request_id=?2 AND project_id=?3 AND conversation_id=?4
                   AND position=?5 AND revision=?6 AND status='pending'",
            )
            .bind(neighbour_position)
            .bind(request.request_id.to_string())
            .bind(request.project_id.to_string())
            .bind(request.conversation_id.to_string())
            .bind(temporary_position)
            .bind(target_revision)
            .execute(&mut *tx)
            .await?;
            if moved_target.rows_affected() != 1 {
                return Err(StoreError::InvalidInput(
                    "queue movement could not be completed".into(),
                ));
            }
        }
        let items = queue_items_in_tx(&mut tx, request.project_id, request.conversation_id).await?;
        tx.commit().await?;
        Ok(items)
    }

    /// The native action command returns one fresh scoped list for both
    /// cancellation and movement, avoiding stale optimistic rows in another
    /// window. Lease/dispatch actions are intentionally not accepted here.
    pub async fn composer_queue_action(
        &self,
        request: &ComposerQueueActionRequestV4,
    ) -> Result<Vec<ComposerQueueItemV4>, StoreError> {
        match request.action {
            ComposerQueueActionV4::Cancel => {
                self.cancel_composer_queue(request).await?;
                self.list_composer_queue(request.project_id, request.conversation_id)
                    .await
            }
            ComposerQueueActionV4::MoveUp | ComposerQueueActionV4::MoveDown => {
                self.move_composer_queue(request).await
            }
            ComposerQueueActionV4::CutIn => self.cut_in_composer_queue(request).await,
        }
    }

    /// Atomically fold one text-only pending queue item into the currently
    /// running ordinary Agent as guidance. The deterministic message id is
    /// stored on the queue row and the guidance row in the same transaction;
    /// a retry therefore observes the completed cut-in instead of dispatching
    /// the item through the FIFO driver.
    pub async fn cut_in_composer_queue(
        &self,
        request: &ComposerQueueActionRequestV4,
    ) -> Result<Vec<ComposerQueueItemV4>, StoreError> {
        if request.action != ComposerQueueActionV4::CutIn {
            return Err(StoreError::InvalidInput(
                "queue action is not cut-in".into(),
            ));
        }
        if request.expected_revision == 0 {
            return Err(StoreError::InvalidInput(
                "queue revision must be positive".into(),
            ));
        }
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        crate::composer_replacement::ensure_ordinary_queue_item_in_tx(&mut tx, request.request_id)
            .await?;
        ensure_conversation_owner_executor(&mut *tx, request.project_id, request.conversation_id)
            .await?;
        let row = queue_row_by_request(&mut tx, request.request_id)
            .await?
            .ok_or_else(|| StoreError::InvalidInput("queue item was not found".into()))?;
        if row.try_get::<String, _>(3)? != request.project_id.to_string()
            || row.try_get::<String, _>(4)? != request.conversation_id.to_string()
        {
            return Err(StoreError::InvalidInput(
                "queue item does not belong to the requested scope".into(),
            ));
        }
        let expected_cutin_id = stable_cutin_message_id(request.request_id);
        let stored_cutin_id = row
            .try_get::<Option<String>, _>(18)?
            .map(|value| parse_uuid(&value, "queue cut-in message id"))
            .transpose()?;
        if stored_cutin_id.is_some_and(|value| value != expected_cutin_id) {
            return Err(StoreError::InvalidInput(
                "queued cut-in receipt is malformed".into(),
            ));
        }

        // A lost response may be retried after the target row has been
        // cancelled. Verify the exact guidance row and return the fresh scoped
        // queue list without requiring the run to remain active.
        if let Some(cutin_id) = stored_cutin_id {
            let guidance = sqlx::query(
                "SELECT g.run_id,g.markdown,r.project_id,r.conversation_id
                 FROM agent_guidance_v4 g
                 JOIN agent_runs_v4 r ON r.run_id=g.run_id
                 WHERE g.message_id=?1",
            )
            .bind(cutin_id.to_string())
            .fetch_optional(&mut *tx)
            .await?;
            let target_text = row.try_get::<String, _>(8)?.trim().to_owned();
            let project_text = request.project_id.to_string();
            let conversation_text = request.conversation_id.to_string();
            let valid_retry = row.try_get::<String, _>(9)? == "cancelled"
                && guidance.as_ref().is_some_and(|guidance| {
                    guidance.try_get::<String, _>(1).ok().as_deref() == Some(target_text.as_str())
                        && guidance.try_get::<String, _>(2).ok().as_deref()
                            == Some(project_text.as_str())
                        && guidance.try_get::<String, _>(3).ok().as_deref()
                            == Some(conversation_text.as_str())
                });
            if valid_retry {
                let items =
                    queue_items_in_tx(&mut tx, request.project_id, request.conversation_id).await?;
                tx.commit().await?;
                return Ok(items);
            }
            return Err(StoreError::InvalidInput(
                "queued cut-in receipt has no matching guidance".into(),
            ));
        }

        if row.try_get::<String, _>(9)? != "pending" {
            return Err(StoreError::InvalidInput(
                "only pending queue items can be cut in".into(),
            ));
        }
        if row.try_get::<i64, _>(6)? != i64::try_from(request.expected_revision).unwrap_or(i64::MAX)
        {
            return Err(StoreError::InvalidInput("queue revision conflict".into()));
        }
        let mode =
            parse_json_enum::<ComposerQueueModeV4>(&row.try_get::<String, _>(7)?, "queue mode")?;
        if mode != ComposerQueueModeV4::Agent {
            return Err(StoreError::InvalidInput(
                "only ordinary Agent queue items can be cut in".into(),
            ));
        }
        let message = row.try_get::<String, _>(8)?.trim().to_owned();
        if message.is_empty() || message.as_bytes().len() > 2048 {
            return Err(StoreError::InvalidInput(
                "cut-in guidance must contain 1..2048 UTF-8 bytes".into(),
            ));
        }
        let references: Vec<ComposerReference> =
            parse_snapshot(&row.try_get::<String, _>(12)?, "queued references")?;
        let attachments: Vec<Uuid> =
            parse_snapshot(&row.try_get::<String, _>(13)?, "queued attachments")?;
        if !references.is_empty() || !attachments.is_empty() {
            return Err(StoreError::InvalidInput(
                "cut-in requires a text-only queue item".into(),
            ));
        }

        let active_rows = sqlx::query(
            "SELECT run_id,value_json FROM agent_runs_v4
             WHERE project_id=?1 AND conversation_id=?2 AND status='running'",
        )
        .bind(request.project_id.to_string())
        .bind(request.conversation_id.to_string())
        .fetch_all(&mut *tx)
        .await?;
        let mut active_spec: Option<(Uuid, RunSpecV4)> = None;
        for active in active_rows {
            let run_id = parse_uuid(&active.try_get::<String, _>(0)?, "active run id")?;
            let value: serde_json::Value = serde_json::from_str(&active.try_get::<String, _>(1)?)?;
            let spec: RunSpecV4 =
                serde_json::from_value(value.get("spec").cloned().ok_or_else(|| {
                    StoreError::InvalidInput("active run has no frozen specification".into())
                })?)?;
            spec.validate_integrity()
                .map_err(|error| StoreError::InvalidInput(error.to_string()))?;
            if spec.run_id != run_id
                || spec.project_id != request.project_id
                || spec.conversation_id != request.conversation_id
            {
                return Err(StoreError::InvalidInput(
                    "active run has a mismatched frozen scope".into(),
                ));
            }
            if spec.execution_kind != RunExecutionKindV4::OrdinaryAgent {
                continue;
            }
            if active_spec.replace((run_id, spec)).is_some() {
                return Err(StoreError::InvalidInput(
                    "conversation has multiple active ordinary runs".into(),
                ));
            }
        }
        let (active_run_id, _active_spec) = active_spec.ok_or_else(|| {
            StoreError::InvalidInput("cut-in requires one active ordinary Agent run".into())
        })?;
        let guidance_request = SubmitGuidanceV4Request {
            message_id: expected_cutin_id,
            project_id: request.project_id,
            conversation_id: request.conversation_id,
            run_id: active_run_id,
            markdown: message,
        };
        super::guidance::accept_guidance_in_tx_v4(&mut *tx, &guidance_request).await?;
        let accepted_at = timestamp(Utc::now());
        let next = next_revision(row.try_get::<i64, _>(6)?)?;
        let updated = sqlx::query(
            "UPDATE composer_queue_v4
             SET status='cancelled',revision=?1,cutin_message_id=?2,updated_at=?3
             WHERE request_id=?4 AND project_id=?5 AND conversation_id=?6
               AND revision=?7 AND status='pending'",
        )
        .bind(next)
        .bind(expected_cutin_id.to_string())
        .bind(accepted_at)
        .bind(request.request_id.to_string())
        .bind(request.project_id.to_string())
        .bind(request.conversation_id.to_string())
        .bind(request.expected_revision as i64)
        .execute(&mut *tx)
        .await?;
        if updated.rows_affected() != 1 {
            return Err(StoreError::InvalidInput("queue revision conflict".into()));
        }
        let items = queue_items_in_tx(&mut tx, request.project_id, request.conversation_id).await?;
        tx.commit().await?;
        Ok(items)
    }
}

fn validate_request_ids(request: &EnqueueComposerTurnRequestV4) -> Result<(), StoreError> {
    for (label, id) in [
        ("queue request id", request.request_id),
        ("queued message id", request.message_id),
        ("queued run id", request.run_id),
        ("queue project id", request.project_id),
        ("queue conversation id", request.conversation_id),
        ("queue model profile id", request.model_profile_id),
    ] {
        if id.is_nil() {
            return Err(StoreError::InvalidInput(format!("{label} cannot be nil")));
        }
    }
    validate_payload(
        request.project_id,
        &request.message_markdown,
        &request.references,
        &request.attachments,
    )?;
    Ok(())
}

fn validate_payload(
    project_id: Uuid,
    message_markdown: &str,
    references: &[ComposerReference],
    attachments: &[Uuid],
) -> Result<(), StoreError> {
    if message_markdown.as_bytes().len() > MAX_MESSAGE_BYTES {
        return Err(StoreError::InvalidInput(
            "queued composer text exceeds the 64 KiB limit".into(),
        ));
    }
    if message_markdown.trim().is_empty() && references.is_empty() && attachments.is_empty() {
        return Err(StoreError::InvalidInput(
            "queued composer request cannot be empty".into(),
        ));
    }
    if references.len() > MAX_REFERENCES {
        return Err(StoreError::InvalidInput(format!(
            "queued composer references exceed the maximum of {MAX_REFERENCES}"
        )));
    }
    let session_count = references
        .iter()
        .filter(|reference| matches!(reference, ComposerReference::Session { .. }))
        .count();
    if session_count > MAX_SESSION_REFERENCES {
        return Err(StoreError::InvalidInput(format!(
            "queued composer session references exceed the maximum of {MAX_SESSION_REFERENCES}"
        )));
    }
    for reference in references {
        if !reference_belongs_to_project(project_id, reference) {
            return Err(StoreError::InvalidInput(
                "composer reference does not belong to the requested project".into(),
            ));
        }
    }
    if attachments.len() > MAX_ATTACHMENTS {
        return Err(StoreError::InvalidInput(format!(
            "queued composer attachments exceed the maximum of {MAX_ATTACHMENTS}"
        )));
    }
    let mut seen = HashSet::with_capacity(attachments.len());
    if attachments
        .iter()
        .any(|id| id.is_nil() || !seen.insert(*id))
    {
        return Err(StoreError::InvalidInput(
            "queued composer attachments must be unique non-nil IDs".into(),
        ));
    }
    Ok(())
}

fn validate_enqueue_snapshot(
    request: &EnqueueComposerTurnRequestV4,
    frozen: &ComposerQueueFrozenConfigV4,
    material: &ComposerQueueMaterialSnapshotV4,
) -> Result<(), StoreError> {
    frozen.compute_selection.validate().map_err(|error| {
        StoreError::InvalidInput(format!("invalid frozen compute selection: {error}"))
    })?;
    if request.model_profile_id != frozen.model_profile_id
        || request.compute_selection != frozen.compute_selection
    {
        return Err(StoreError::InvalidInput(
            "queued request and frozen configuration do not match".into(),
        ));
    }
    validate_frozen_snapshot(frozen)?;
    validate_material_snapshot(
        request.project_id,
        request.conversation_id,
        &request.attachments,
        material,
    )?;
    let material_json = serde_json::to_vec(material)?;
    validate_snapshot_json_size(&material_json, "queued material snapshot")?;
    Ok(())
}

fn validate_frozen_snapshot(frozen: &ComposerQueueFrozenConfigV4) -> Result<(), StoreError> {
    if frozen.model_profile_id.is_nil() {
        return Err(StoreError::InvalidInput(
            "frozen model profile id cannot be nil".into(),
        ));
    }
    validate_sha256(&frozen.model_configuration_hash, "model configuration hash")?;
    if let Some(binding) = &frozen.delegated_model {
        if binding.profile_id.is_nil() {
            return Err(StoreError::InvalidInput(
                "frozen delegated profile id cannot be nil".into(),
            ));
        }
        validate_sha256(&binding.configuration_hash, "delegated configuration hash")?;
    }
    if let Some(binding) = &frozen.reviewer_model {
        if binding.profile_id.is_nil() {
            return Err(StoreError::InvalidInput(
                "frozen reviewer profile id cannot be nil".into(),
            ));
        }
        validate_sha256(&binding.configuration_hash, "reviewer configuration hash")?;
    }
    Ok(())
}

/// Re-read every host-owned input that contributes to a queue item's frozen
/// configuration while the enqueue transaction holds SQLite's immediate
/// writer lock. The caller may have resolved the same values before opening
/// the transaction; this second check closes the profile/settings race before
/// the row becomes durable.
pub(super) async fn validate_frozen_snapshot_current(
    tx: &mut Transaction<'_, Sqlite>,
    project_id: Uuid,
    conversation_id: Uuid,
    frozen: &ComposerQueueFrozenConfigV4,
) -> Result<(), StoreError> {
    ensure_conversation_owner_executor(&mut **tx, project_id, conversation_id).await?;
    validate_frozen_snapshot(frozen)?;

    let main = model_profile_in_tx(&mut **tx, frozen.model_profile_id).await?;
    if !main
        .execution_configuration_hash()
        .eq_ignore_ascii_case(&frozen.model_configuration_hash)
    {
        return Err(StoreError::InvalidInput(
            "main model profile changed while enqueueing".into(),
        ));
    }

    let preferences_json: Option<String> =
        sqlx::query_scalar("SELECT value_json FROM settings WHERE scope=?1 AND key=?2")
            .bind(super::SETTINGS_GLOBAL_SCOPE)
            .bind(super::conversation_agent_preferences_setting_key(
                conversation_id,
            ))
            .fetch_optional(&mut **tx)
            .await?;
    let current_preferences = preferences_json
        .map(|value| {
            serde_json::from_str::<omicsops_dto::ConversationAgentPreferencesV4>(&value).map_err(
                |error| {
                    StoreError::InvalidInput(format!(
                        "invalid conversation agent preferences setting: {error}"
                    ))
                },
            )
        })
        .transpose()?
        .unwrap_or_default();
    if current_preferences != frozen.conversation_preferences {
        return Err(StoreError::InvalidInput(
            "conversation preferences changed while enqueueing".into(),
        ));
    }

    if let Some(binding) = &frozen.delegated_model {
        let profile = model_profile_in_tx(&mut **tx, binding.profile_id).await?;
        if !profile
            .execution_configuration_hash()
            .eq_ignore_ascii_case(&binding.configuration_hash)
        {
            return Err(StoreError::InvalidInput(
                "delegated model profile changed while enqueueing".into(),
            ));
        }
    }
    if let Some(binding) = &frozen.reviewer_model {
        let profile = model_profile_in_tx(&mut **tx, binding.profile_id).await?;
        if !profile
            .execution_configuration_hash()
            .eq_ignore_ascii_case(&binding.configuration_hash)
        {
            return Err(StoreError::InvalidInput(
                "reviewer model profile changed while enqueueing".into(),
            ));
        }
    }
    Ok(())
}

async fn model_profile_in_tx(
    connection: &mut sqlx::SqliteConnection,
    profile_id: Uuid,
) -> Result<omicsops_core::workspace::ModelProfile, StoreError> {
    let value: Option<String> =
        sqlx::query_scalar("SELECT value_json FROM model_profiles WHERE id=?1")
            .bind(profile_id.to_string())
            .fetch_optional(&mut *connection)
            .await?;
    let Some(value) = value else {
        return Err(StoreError::InvalidInput(
            "frozen model profile is no longer available".into(),
        ));
    };
    let profile: omicsops_core::workspace::ModelProfile =
        serde_json::from_str(&value).map_err(|error| {
            StoreError::InvalidInput(format!("invalid frozen model profile: {error}"))
        })?;
    if profile.id != profile_id {
        return Err(StoreError::InvalidInput(
            "frozen model profile identity does not match its row".into(),
        ));
    }
    Ok(profile)
}

fn validate_material_snapshot(
    project_id: Uuid,
    conversation_id: Uuid,
    attachment_ids: &[Uuid],
    material: &ComposerQueueMaterialSnapshotV4,
) -> Result<(), StoreError> {
    if material.reference_context.as_bytes().len() > MAX_REFERENCE_CONTEXT_BYTES {
        return Err(StoreError::InvalidInput(
            "queued reference context exceeds the 64 KiB limit".into(),
        ));
    }
    validate_sha256(&material.reference_context_sha256, "reference context hash")?;
    if !hash_bytes(material.reference_context.as_bytes())
        .eq_ignore_ascii_case(&material.reference_context_sha256)
    {
        return Err(StoreError::InvalidInput(
            "reference context hash does not match its snapshot".into(),
        ));
    }
    if material.attachment_receipts.len() != attachment_ids.len() {
        return Err(StoreError::InvalidInput(
            "attachment receipts do not match queued attachment IDs".into(),
        ));
    }
    let mut seen = HashSet::with_capacity(material.attachment_receipts.len());
    let mut total_bytes = 0_u64;
    for (attachment_id, receipt) in attachment_ids.iter().zip(&material.attachment_receipts) {
        if *attachment_id != receipt.id
            || receipt.id.is_nil()
            || receipt.project_id != project_id
            || receipt.conversation_id != conversation_id
            || !seen.insert(receipt.id)
            || receipt.name.trim().is_empty()
            || receipt.name.as_bytes().len() > MAX_ATTACHMENT_NAME_BYTES
            || receipt.relative_path.as_bytes().len() > MAX_ATTACHMENT_RELATIVE_PATH_BYTES
            || receipt.relative_path.contains('\0')
        {
            return Err(StoreError::InvalidInput(
                "attachment receipt does not match the queued scope".into(),
            ));
        }
        if !is_generated_attachment_path(&receipt.relative_path, receipt.id) {
            return Err(StoreError::InvalidInput(
                "attachment receipt path is not a generated staging path".into(),
            ));
        }
        if receipt.size_bytes > MAX_ATTACHMENT_BYTES {
            return Err(StoreError::InvalidInput(format!(
                "attachment receipt exceeds the per-file {MAX_ATTACHMENT_BYTES} byte limit"
            )));
        }
        total_bytes = total_bytes.checked_add(receipt.size_bytes).ok_or_else(|| {
            StoreError::InvalidInput("attachment receipt size exceeds its bound".into())
        })?;
        if total_bytes > MAX_TOTAL_ATTACHMENT_BYTES {
            return Err(StoreError::InvalidInput(format!(
                "attachment receipts exceed the total {MAX_TOTAL_ATTACHMENT_BYTES} byte limit"
            )));
        }
        validate_sha256(&receipt.sha256, "attachment hash")?;
    }
    validate_sha256(
        &material.attachment_snapshot_sha256,
        "attachment snapshot hash",
    )?;
    if !hash_json(&material.attachment_receipts)
        .eq_ignore_ascii_case(&material.attachment_snapshot_sha256)
    {
        return Err(StoreError::InvalidInput(
            "attachment snapshot hash does not match its receipts".into(),
        ));
    }
    Ok(())
}

fn is_generated_attachment_path(path: &str, id: Uuid) -> bool {
    if path.contains('\\') {
        return false;
    }
    let normalized = path;
    let prefix = format!(".omicsops/attachments/{id}/");
    let payload_name = normalized.strip_prefix(&prefix).unwrap_or_default();
    normalized.starts_with(&prefix)
        && !payload_name.is_empty()
        && payload_name.len() <= 64
        && !payload_name.contains('/')
        && !payload_name.contains("..")
        && payload_name != "."
}

fn validate_snapshot_json_size(value: &[u8], label: &str) -> Result<(), StoreError> {
    if value.len() > MAX_SNAPSHOT_JSON_BYTES {
        return Err(StoreError::InvalidInput(format!(
            "{label} exceeds its bound"
        )));
    }
    Ok(())
}

fn reference_belongs_to_project(project_id: Uuid, reference: &ComposerReference) -> bool {
    match reference {
        ComposerReference::Artifact {
            project_id: owner, ..
        }
        | ComposerReference::Session {
            project_id: owner, ..
        }
        | ComposerReference::Project {
            project_id: owner, ..
        }
        | ComposerReference::ExecutionContext {
            project_id: owner, ..
        }
        | ComposerReference::Runtime {
            project_id: owner, ..
        }
        | ComposerReference::WorkspaceFile {
            project_id: owner, ..
        }
        | ComposerReference::Workflow {
            project_id: owner, ..
        }
        | ComposerReference::Quote {
            project_id: owner, ..
        } => *owner == project_id,
        ComposerReference::Skill { .. } => true,
    }
}

fn canonical_request_hash(request: &EnqueueComposerTurnRequestV4) -> Result<String, StoreError> {
    Ok(hash_json(request))
}

fn hash_json<T: Serialize>(value: &T) -> String {
    hash_bytes(&serde_json::to_vec(value).expect("queue DTO serialization cannot fail"))
}

fn hash_bytes(value: &[u8]) -> String {
    hex::encode(Sha256::digest(value))
}

fn stable_cutin_message_id(request_id: Uuid) -> Uuid {
    let mut input = b"omicsops.composer.queue.cutin.v4\0".to_vec();
    input.extend_from_slice(request_id.as_bytes());
    let digest = Sha256::digest(input);
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    // Mark this deterministic identifier as a version-5/name UUID while
    // retaining the fixed hash-derived value across retries and restarts.
    bytes[6] = (bytes[6] & 0x0f) | 0x50;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Uuid::from_bytes(bytes)
}

fn validate_sha256(value: &str, label: &str) -> Result<(), StoreError> {
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(StoreError::InvalidInput(format!(
            "{label} must be a 64-character hexadecimal SHA-256"
        )));
    }
    Ok(())
}

fn next_revision(value: i64) -> Result<i64, StoreError> {
    value
        .checked_add(1)
        .filter(|next| *next > 0)
        .ok_or_else(|| StoreError::InvalidInput("queue revision exceeds SQLite range".into()))
}

async fn ensure_reserved_ids_available(
    tx: &mut Transaction<'_, Sqlite>,
    request: &EnqueueComposerTurnRequestV4,
) -> Result<(), StoreError> {
    let message_id = request.message_id.to_string();
    let run_id = request.run_id.to_string();
    let queue_conflict: i64 = sqlx::query_scalar(
        "SELECT EXISTS(
             SELECT 1 FROM composer_queue_v4
             WHERE message_id=?1 OR run_id=?2
         )",
    )
    .bind(&message_id)
    .bind(&run_id)
    .fetch_one(&mut **tx)
    .await?;
    if queue_conflict != 0 {
        return Err(StoreError::InvalidInput(
            "queued message or run ID is already reserved".into(),
        ));
    }
    let message_conflict: i64 =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM messages WHERE id=?1)")
            .bind(&message_id)
            .fetch_one(&mut **tx)
            .await?;
    let run_conflict: i64 =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM agent_runs_v4 WHERE run_id=?1)")
            .bind(&run_id)
            .fetch_one(&mut **tx)
            .await?;
    if message_conflict != 0 || run_conflict != 0 {
        return Err(StoreError::InvalidInput(
            "queued message or run ID is already in use".into(),
        ));
    }
    Ok(())
}

pub(super) async fn queue_row_by_request(
    tx: &mut Transaction<'_, Sqlite>,
    request_id: Uuid,
) -> Result<Option<SqliteRow>, StoreError> {
    Ok(sqlx::query(&format!(
        "SELECT {QUEUE_COLUMNS} FROM composer_queue_v4 WHERE request_id=?1"
    ))
    .bind(request_id.to_string())
    .fetch_optional(&mut **tx)
    .await?)
}

async fn pending_target(
    tx: &mut Transaction<'_, Sqlite>,
    project_id: Uuid,
    conversation_id: Uuid,
    request_id: Uuid,
    expected_revision: u64,
) -> Result<SqliteRow, StoreError> {
    let row = sqlx::query(&format!(
        "SELECT {QUEUE_COLUMNS} FROM composer_queue_v4
         WHERE request_id=?1 AND project_id=?2 AND conversation_id=?3"
    ))
    .bind(request_id.to_string())
    .bind(project_id.to_string())
    .bind(conversation_id.to_string())
    .fetch_optional(&mut **tx)
    .await?;
    let Some(row) = row else {
        let exists_elsewhere: i64 = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM composer_queue_v4 WHERE request_id=?1)",
        )
        .bind(request_id.to_string())
        .fetch_one(&mut **tx)
        .await?;
        return Err(StoreError::InvalidInput(if exists_elsewhere != 0 {
            "queue item does not belong to the requested scope".into()
        } else {
            "queue item was not found".into()
        }));
    };
    let status = row.try_get::<String, _>(9)?;
    if status != "pending" {
        return Err(StoreError::InvalidInput(
            "only pending queue items can be changed".into(),
        ));
    }
    let stored_revision = row.try_get::<i64, _>(6)?;
    if stored_revision != i64::try_from(expected_revision).unwrap_or(i64::MAX) {
        return Err(StoreError::InvalidInput("queue revision conflict".into()));
    }
    Ok(row)
}

pub(super) async fn queue_items_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    project_id: Uuid,
    conversation_id: Uuid,
) -> Result<Vec<ComposerQueueItemV4>, StoreError> {
    let rows = sqlx::query(&format!(
        "SELECT {QUEUE_COLUMNS} FROM composer_queue_v4
         WHERE project_id=?1 AND conversation_id=?2
         ORDER BY position,request_id"
    ))
    .bind(project_id.to_string())
    .bind(conversation_id.to_string())
    .fetch_all(&mut **tx)
    .await?;
    rows.into_iter().map(queue_item_from_row).collect()
}

pub(super) fn queue_item_from_row(row: SqliteRow) -> Result<ComposerQueueItemV4, StoreError> {
    let request_hash = row.try_get::<String, _>(10)?;
    validate_sha256(&request_hash, "queue request hash")?;
    let request_id = parse_uuid(row.try_get::<String, _>(0)?, "queue request id")?;
    let message_id = parse_uuid(row.try_get::<String, _>(1)?, "queued message id")?;
    let run_id = parse_uuid(row.try_get::<String, _>(2)?, "queued run id")?;
    let project_id = parse_uuid(row.try_get::<String, _>(3)?, "queue project id")?;
    let conversation_id = parse_uuid(row.try_get::<String, _>(4)?, "queue conversation id")?;
    let position = positive_u64(row.try_get::<i64, _>(5)?, "queue position")?;
    let revision = positive_u64(row.try_get::<i64, _>(6)?, "queue revision")?;
    let mode = parse_json_enum::<ComposerQueueModeV4>(&row.try_get::<String, _>(7)?, "queue mode")?;
    let message_markdown = row.try_get::<String, _>(8)?;
    let status =
        parse_json_enum::<ComposerQueueStatusV4>(&row.try_get::<String, _>(9)?, "queue status")?;
    let frozen: ComposerQueueFrozenConfigV4 =
        parse_snapshot(&row.try_get::<String, _>(11)?, "frozen queue config")?;
    let references: Vec<ComposerReference> =
        parse_snapshot(&row.try_get::<String, _>(12)?, "queued references")?;
    let attachments: Vec<Uuid> =
        parse_snapshot(&row.try_get::<String, _>(13)?, "queued attachments")?;
    let material: ComposerQueueMaterialSnapshotV4 =
        parse_snapshot(&row.try_get::<String, _>(14)?, "queued material snapshot")?;
    validate_payload(project_id, &message_markdown, &references, &attachments)?;
    validate_frozen_snapshot(&frozen)?;
    if frozen.model_profile_id.is_nil() || frozen.compute_selection.validate().is_err() {
        return Err(StoreError::InvalidInput(
            "persisted queue frozen configuration is invalid".into(),
        ));
    }
    validate_material_snapshot(project_id, conversation_id, &attachments, &material)?;
    let created_at = from_timestamp(row.try_get(15)?, "queue created_at")?;
    let updated_at = from_timestamp(row.try_get(16)?, "queue updated_at")?;
    let cut_in_message_id = row
        .try_get::<Option<String>, _>(18)?
        .map(|value| parse_uuid(&value, "queue cut-in message id"))
        .transpose()?;
    if cut_in_message_id.is_some_and(|value| value != stable_cutin_message_id(request_id)) {
        return Err(StoreError::InvalidInput(
            "persisted queue cut-in receipt is malformed".into(),
        ));
    }
    let replacement_receipt: Option<omicsops_dto::ComposerReplacementReceiptV4> = row
        .try_get::<Option<String>, _>(20)?
        .map(|value| serde_json::from_str(&value))
        .transpose()?;
    if replacement_receipt.as_ref().is_some_and(|receipt| {
        receipt.request_id != request_id
            || receipt.project_id != project_id
            || receipt.conversation_id != conversation_id
            || receipt.stop.project_id != project_id
            || receipt.stop.conversation_id != conversation_id
            || receipt.stop.run_id != receipt.target_run_id
    }) {
        return Err(StoreError::InvalidInput(
            "replacement receipt scope is invalid".into(),
        ));
    }
    Ok(ComposerQueueItemV4 {
        replacement_receipt,
        replacement_target_run_id: row
            .try_get::<Option<String>, _>(19)?
            .map(|value| parse_uuid(&value, "replacement target"))
            .transpose()?,
        request_id,
        message_id,
        run_id,
        project_id,
        conversation_id,
        position,
        revision,
        mode,
        message_markdown,
        frozen,
        references,
        attachments,
        attachment_receipts: material.attachment_receipts,
        cut_in_message_id,
        failure_code: row
            .try_get::<Option<String>, _>(17)?
            .map(|code| {
                parse_json_enum::<omicsops_dto::ComposerQueueFailureCodeV4>(
                    &code,
                    "queue failure code",
                )
            })
            .transpose()?,
        status,
        created_at,
        updated_at,
    })
}

fn parse_snapshot<T: serde::de::DeserializeOwned>(
    value: &str,
    label: &str,
) -> Result<T, StoreError> {
    serde_json::from_str(value)
        .map_err(|error| StoreError::InvalidInput(format!("invalid {label}: {error}")))
}

fn positive_u64(value: i64, label: &str) -> Result<u64, StoreError> {
    u64::try_from(value).map_err(|_| StoreError::InvalidInput(format!("{label} must be positive")))
}

pub(super) async fn enqueue_composer_turn_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    request: &EnqueueComposerTurnRequestV4,
    frozen: &ComposerQueueFrozenConfigV4,
    material: &ComposerQueueMaterialSnapshotV4,
) -> Result<ComposerQueueItemV4, StoreError> {
    validate_request_ids(request)?;
    let request_hash = canonical_request_hash(request)?;

    // Idempotency must precede live snapshot validation. An enqueue retry
    // can arrive after a profile was deleted or after the row was edited;
    // the original request hash remains the authority for this logical ID.
    if let Some(row) = queue_row_by_request(&mut *tx, request.request_id).await? {
        let stored_hash = row.try_get::<String, _>(10)?;
        let stored_project = row.try_get::<String, _>(3)?;
        let stored_conversation = row.try_get::<String, _>(4)?;
        if stored_hash != request_hash
            || stored_project != request.project_id.to_string()
            || stored_conversation != request.conversation_id.to_string()
        {
            return Err(StoreError::InvalidInput(
                "queue request id already belongs to a different logical request".into(),
            ));
        }
        let item = queue_item_from_row(row)?;

        return Ok(item);
    }

    ensure_conversation_owner_executor(&mut *tx, request.project_id, request.conversation_id)
        .await?;
    validate_enqueue_snapshot(request, frozen, material)?;
    validate_frozen_snapshot_current(
        &mut *tx,
        request.project_id,
        request.conversation_id,
        frozen,
    )
    .await?;
    ensure_reserved_ids_available(&mut *tx, request).await?;

    let active_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM composer_queue_v4
             WHERE project_id=?1 AND conversation_id=?2
               AND status NOT IN ('completed','failed','cancelled')",
    )
    .bind(request.project_id.to_string())
    .bind(request.conversation_id.to_string())
    .fetch_one(&mut **tx)
    .await?;
    if active_count >= MAX_QUEUE_NON_TERMINAL {
        return Err(StoreError::InvalidInput(format!(
            "composer queue already contains the maximum of {MAX_QUEUE_NON_TERMINAL} non-terminal items"
        )));
    }

    let max_position: i64 = sqlx::query_scalar(
        "SELECT COALESCE(MAX(position),0) FROM composer_queue_v4
             WHERE conversation_id=?1",
    )
    .bind(request.conversation_id.to_string())
    .fetch_one(&mut **tx)
    .await?;
    let position = max_position
        .checked_add(1)
        .filter(|value| *value > 0)
        .ok_or_else(|| {
            StoreError::InvalidInput("composer queue position exceeds SQLite range".into())
        })?;
    let now = timestamp(Utc::now());
    let frozen_json = serde_json::to_string(frozen)?;
    let references_json = serde_json::to_string(&request.references)?;
    let attachments_json = serde_json::to_string(&request.attachments)?;
    let material_json = serde_json::to_string(material)?;
    sqlx::query(
        "INSERT INTO composer_queue_v4
             (request_id,message_id,run_id,project_id,conversation_id,position,revision,
              mode,message_markdown,status,request_hash,frozen_json,references_json,
              attachments_json,material_json,created_at,updated_at)
             VALUES (?1,?2,?3,?4,?5,?6,1,?7,?8,'pending',?9,?10,?11,?12,?13,?14,?14)",
    )
    .bind(request.request_id.to_string())
    .bind(request.message_id.to_string())
    .bind(request.run_id.to_string())
    .bind(request.project_id.to_string())
    .bind(request.conversation_id.to_string())
    .bind(position)
    .bind(super::enum_string(&request.mode)?)
    .bind(&request.message_markdown)
    .bind(request_hash)
    .bind(frozen_json)
    .bind(references_json)
    .bind(attachments_json)
    .bind(material_json)
    .bind(now)
    .execute(&mut **tx)
    .await?;

    let row = queue_row_by_request(&mut *tx, request.request_id)
        .await?
        .ok_or_else(|| StoreError::InvalidInput("queued item disappeared after insert".into()))?;
    let item = queue_item_from_row(row)?;

    Ok(item)
}
