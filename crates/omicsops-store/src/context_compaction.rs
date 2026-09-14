//! Durable, idempotent manual context compaction.
//!
//! The Store owns the compaction transaction because an archive, its
//! checkpoint events, and the bounded receipt must become visible together.
//! The complete event transcript is retained in `agent_context_archives_v4`;
//! the receipt only contains scope, status, sizes, and hashes.

use chrono::{DateTime, Utc};
use omicsops_protocol::{
    AgentEventKindV4, AgentEventV4, CompactContextRequestV4, ContextArchiveV4, ContextCheckpointV4,
    ContextCompactionReceiptV4, ContextCompactionStatusV4, RunSpecV4,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::{Row, Sqlite, Transaction};
use uuid::Uuid;

use super::{
    Store, StoreError, ensure_conversation_owner_executor, ensure_run_owner_executor,
    insert_agent_event_in_tx, load_agent_events_in_tx, timestamp,
};

const STARTED: &str = "started";
const NOT_NEEDED: &str = "not_needed";
const COMPLETED: &str = "completed";
const ATTENTION: &str = "attention";
const MAX_CHECKPOINT_STEP_BYTES: usize = 4 * 1024;
const MAX_GUIDANCE_ITEMS: usize = 16;
const MAX_RECENT_STEPS: usize = 16;

impl Store {
    /// Compact one genuinely paused `waiting_for_input` run using the same
    /// durable archive/checkpoint primitives as automatic context reduction.
    ///
    /// The candidate is deliberately deterministic and host-owned: it keeps
    /// the frozen plan, a bounded checkpoint, active guidance, and no recent
    /// event tail. A future model-aware projection can provide a richer
    /// candidate without changing the receipt or archive transaction.
    /// Terminal runs are deliberately represented as `NotNeeded`: their event
    /// chains are immutable and a later run does not inherit this checkpoint.
    pub async fn compact_context_v4(
        &self,
        request: &CompactContextRequestV4,
    ) -> Result<ContextCompactionReceiptV4, StoreError> {
        validate_request(request)?;
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;

        if let Some(mut existing) = load_receipt_row_in_tx(&mut tx, request.request_id).await? {
            ensure_receipt_scope(&existing, request)?;
            hydrate_archive_in_tx(&mut tx, &mut existing).await?;
            tx.commit().await?;
            if existing.status == STARTED {
                return self
                    .reconcile_context_compaction_v4(
                        request.project_id,
                        request.conversation_id,
                        request.run_id,
                    )
                    .await?
                    .ok_or_else(|| {
                        StoreError::InvalidInput(
                            "context compaction request has no durable outcome".into(),
                        )
                    });
            }
            return receipt_from_row(existing);
        }

        let result = compact_new_in_tx(&mut tx, request).await;
        match result {
            Ok(receipt) => {
                tx.commit().await?;
                Ok(receipt)
            }
            Err(error) => {
                let _ = tx.rollback().await;
                // Validation errors are intentionally returned without a
                // receipt: no compaction boundary was accepted. Once the
                // started row is committed, reconciliation records Attention
                // separately; this branch is only for pre-commit failures.
                Err(error)
            }
        }
    }

    /// Return the newest representable receipt for a project/conversation.
    /// A pre-commit `started` row is omitted until conservative reconciliation
    /// turns it into an explicit Attention outcome.
    pub async fn latest_context_compaction_v4(
        &self,
        project_id: Uuid,
        conversation_id: Uuid,
    ) -> Result<Option<ContextCompactionReceiptV4>, StoreError> {
        validate_scope(project_id, conversation_id)?;
        let mut tx = self.pool.begin().await?;
        ensure_conversation_owner_executor(&mut tx, project_id, conversation_id).await?;
        let row = sqlx::query(
            "SELECT request_id,project_id,conversation_id,run_id,status,
                    source_through_sequence,source_head_hash,before_bytes,after_bytes,
                    archive_id,checkpoint_through_sequence,checkpoint_sha256,
                    frozen_spec_hash,message,created_at,updated_at
             FROM agent_context_compactions_v4
             WHERE project_id=?1 AND conversation_id=?2
             ORDER BY updated_at DESC, request_id DESC LIMIT 1",
        )
        .bind(project_id.to_string())
        .bind(conversation_id.to_string())
        .fetch_optional(&mut *tx)
        .await?;
        let mut row = row.map(receipt_row_from_sql).transpose()?;
        if let Some(value) = &mut row {
            hydrate_archive_in_tx(&mut tx, value).await?;
        }
        tx.commit().await?;
        row.map(|row| {
            if row.status == STARTED {
                Ok(None)
            } else {
                receipt_from_row(row).map(Some)
            }
        })
        .transpose()
        .map(|value| value.flatten())
    }

    /// Reconcile a request that was durably marked `started` before a process
    /// interruption. No model call or second archive is launched here.
    pub async fn reconcile_context_compaction_v4(
        &self,
        project_id: Uuid,
        conversation_id: Uuid,
        run_id: Uuid,
    ) -> Result<Option<ContextCompactionReceiptV4>, StoreError> {
        validate_scope(project_id, conversation_id)?;
        if run_id.is_nil() {
            return Err(StoreError::InvalidInput(
                "context compaction run id cannot be nil".into(),
            ));
        }
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        ensure_conversation_owner_executor(&mut tx, project_id, conversation_id).await?;
        ensure_run_owner_executor(&mut tx, project_id, conversation_id, run_id).await?;
        let Some(mut row) =
            load_latest_receipt_for_run_in_tx(&mut tx, project_id, conversation_id, run_id).await?
        else {
            tx.commit().await?;
            return Ok(None);
        };

        if row.status == STARTED {
            let events = load_agent_events_in_tx(&mut tx, run_id).await?;
            let completed = events.iter().find_map(|event| match &event.event {
                AgentEventKindV4::ContextCompactionCompleted {
                    request_id,
                    archive,
                    checkpoint_through_sequence,
                    checkpoint_sha256,
                    before_bytes,
                    after_bytes,
                } if *request_id == row.request_id => Some((
                    archive.clone(),
                    *checkpoint_through_sequence,
                    checkpoint_sha256.clone(),
                    *before_bytes,
                    *after_bytes,
                )),
                _ => None,
            });
            if let Some((archive, checkpoint_sequence, checkpoint_hash, before, after)) = completed
            {
                let archive_exists: i64 = sqlx::query_scalar(
                    "SELECT EXISTS(
                        SELECT 1 FROM agent_context_archives_v4
                        WHERE archive_id=?1 AND run_id=?2
                    )",
                )
                .bind(archive.archive_id.to_string())
                .bind(run_id.to_string())
                .fetch_one(&mut *tx)
                .await?;
                if archive_exists != 0 {
                    sqlx::query(
                        "UPDATE agent_context_compactions_v4
                         SET status=?1,after_bytes=?2,archive_id=?3,
                             checkpoint_through_sequence=?4,checkpoint_sha256=?5,
                             updated_at=?6
                         WHERE request_id=?7 AND status=?8",
                    )
                    .bind(COMPLETED)
                    .bind(i64::try_from(after).map_err(|_| {
                        StoreError::InvalidInput("compaction size exceeds SQLite range".into())
                    })?)
                    .bind(archive.archive_id.to_string())
                    .bind(i64::try_from(checkpoint_sequence).map_err(|_| {
                        StoreError::InvalidInput("checkpoint sequence exceeds SQLite range".into())
                    })?)
                    .bind(&checkpoint_hash)
                    .bind(timestamp(Utc::now()))
                    .bind(row.request_id.to_string())
                    .bind(STARTED)
                    .execute(&mut *tx)
                    .await?;
                    row.status = COMPLETED.into();
                    row.after_bytes = Some(after);
                    row.archive_id = Some(archive.archive_id);
                    row.checkpoint_through_sequence = Some(checkpoint_sequence);
                    row.checkpoint_sha256 = Some(checkpoint_hash);
                    row.before_bytes = before;
                    row.updated_at = Utc::now();
                    row.archive = Some(archive);
                }
            }
            if row.status == STARTED {
                let message = "context compaction did not finish; original context retained";
                let now = timestamp(Utc::now());
                sqlx::query(
                    "UPDATE agent_context_compactions_v4
                     SET status=?1,message=?2,updated_at=?3
                     WHERE request_id=?4 AND status=?5",
                )
                .bind(ATTENTION)
                .bind(message)
                .bind(now)
                .bind(row.request_id.to_string())
                .bind(STARTED)
                .execute(&mut *tx)
                .await?;
                // A run may have become terminal before reconciliation. In
                // that case the existing event helper correctly refuses a
                // post-terminal compaction event; the receipt still records
                // the conservative outcome.
                let attention = events
                    .last()
                    .map(|previous| {
                        AgentEventV4::next(
                            previous,
                            Utc::now(),
                            AgentEventKindV4::ContextCompactionAttention {
                                request_id: row.request_id,
                                message: message.into(),
                            },
                        )
                    })
                    .unwrap_or_else(|| {
                        AgentEventV4::first(
                            run_id,
                            project_id,
                            conversation_id,
                            Utc::now(),
                            AgentEventKindV4::ContextCompactionAttention {
                                request_id: row.request_id,
                                message: message.into(),
                            },
                        )
                    });
                let _ = insert_agent_event_in_tx(&mut *tx, &attention).await;
                row.status = ATTENTION.into();
                row.message = Some(message.into());
                row.updated_at = DateTime::<Utc>::from_timestamp_millis(now).ok_or_else(|| {
                    StoreError::InvalidInput("invalid compaction timestamp".into())
                })?;
            }
        }
        if row.archive.is_none() {
            hydrate_archive_in_tx(&mut tx, &mut row).await?;
        }
        tx.commit().await?;
        receipt_from_row(row).map(Some)
    }
}

#[derive(Debug, Clone)]
struct ReceiptRow {
    request_id: Uuid,
    project_id: Uuid,
    conversation_id: Uuid,
    run_id: Uuid,
    status: String,
    source_through_sequence: u64,
    source_head_hash: String,
    before_bytes: u64,
    after_bytes: Option<u64>,
    archive_id: Option<Uuid>,
    archive: Option<ContextArchiveV4>,
    checkpoint_through_sequence: Option<u64>,
    checkpoint_sha256: Option<String>,
    frozen_spec_hash: Option<String>,
    message: Option<String>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

fn validate_request(request: &CompactContextRequestV4) -> Result<(), StoreError> {
    if request.request_id.is_nil() {
        return Err(StoreError::InvalidInput(
            "context compaction request id cannot be nil".into(),
        ));
    }
    validate_scope(request.project_id, request.conversation_id)?;
    if request.run_id.is_nil() {
        return Err(StoreError::InvalidInput(
            "context compaction run id cannot be nil".into(),
        ));
    }
    Ok(())
}

fn validate_scope(project_id: Uuid, conversation_id: Uuid) -> Result<(), StoreError> {
    if project_id.is_nil() || conversation_id.is_nil() {
        return Err(StoreError::InvalidInput(
            "context compaction scope identifiers cannot be nil".into(),
        ));
    }
    Ok(())
}

async fn completed_run_not_needed_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    request: &CompactContextRequestV4,
    value_json: &str,
) -> Result<ContextCompactionReceiptV4, StoreError> {
    let value: Value = serde_json::from_str(value_json)?;
    let spec_value = value.get("spec").ok_or_else(|| {
        StoreError::InvalidInput("context compaction requires a frozen run specification".into())
    })?;
    let spec: RunSpecV4 = serde_json::from_value(spec_value.clone()).map_err(|error| {
        StoreError::InvalidInput(format!("frozen run specification is invalid: {error}"))
    })?;
    validate_spec_scope(&spec, request)?;
    spec.validate_integrity().map_err(|error| {
        StoreError::InvalidInput(format!(
            "frozen run specification failed validation: {error}"
        ))
    })?;
    let events = load_agent_events_in_tx(&mut *tx, request.run_id).await?;
    if !events
        .iter()
        .any(|event| matches!(event.event, AgentEventKindV4::RunCompleted))
    {
        return Err(StoreError::InvalidInput(
            "completed run has no durable completion event".into(),
        ));
    }
    if events.iter().any(|event| {
        matches!(
            event.event,
            AgentEventKindV4::RunFailed { .. }
                | AgentEventKindV4::RunCancelled
                | AgentEventKindV4::RunNeedsAttention { .. }
        )
    }) {
        return Err(StoreError::InvalidInput(
            "completed run status does not match its terminal event".into(),
        ));
    }
    let transcript = serde_json::to_string(&events)?;
    let before_bytes = u64::try_from(transcript.len())
        .map_err(|_| StoreError::InvalidInput("context transcript exceeds size range".into()))?;
    let now = DateTime::<Utc>::from_timestamp_millis(timestamp(Utc::now()))
        .ok_or_else(|| StoreError::InvalidInput("invalid compaction timestamp".into()))?;
    let receipt = ContextCompactionReceiptV4 {
        request_id: request.request_id,
        project_id: request.project_id,
        conversation_id: request.conversation_id,
        run_id: request.run_id,
        status: ContextCompactionStatusV4::NotNeeded,
        source_through_sequence: events.last().map_or(0, |event| event.sequence),
        source_head_hash: events
            .last()
            .map_or_else(String::new, |event| event.event_hash.clone()),
        before_bytes,
        after_bytes: Some(before_bytes),
        archive: None,
        checkpoint_through_sequence: None,
        checkpoint_sha256: None,
        frozen_spec_hash: spec.spec_hash.clone(),
        message: Some(
            "completed runs retain their original context; no same-run projection was needed"
                .into(),
        ),
        created_at: now,
        updated_at: now,
    };
    insert_receipt_in_tx(
        tx,
        &receipt,
        NOT_NEEDED,
        None,
        None,
        None,
        receipt.message.as_deref(),
        None,
    )
    .await?;
    Ok(receipt)
}

fn validate_spec_scope(
    spec: &RunSpecV4,
    request: &CompactContextRequestV4,
) -> Result<(), StoreError> {
    if spec.run_id != request.run_id
        || spec.project_id != request.project_id
        || spec.conversation_id != request.conversation_id
    {
        return Err(StoreError::InvalidInput(
            "frozen run specification does not match compaction scope".into(),
        ));
    }
    Ok(())
}

async fn validate_paused_run_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    run_id: Uuid,
    events: &[AgentEventV4],
) -> Result<(), StoreError> {
    let stop_requested: i64 = sqlx::query_scalar(
        "SELECT EXISTS(
             SELECT 1 FROM agent_run_stop_requests_v4 WHERE run_id=?1
         )",
    )
    .bind(run_id.to_string())
    .fetch_one(&mut **tx)
    .await?;
    if stop_requested != 0 {
        return Err(StoreError::InvalidInput(
            "context compaction is unavailable while a stop request exists".into(),
        ));
    }

    let mut pending_inputs = std::collections::BTreeSet::new();
    let mut pending_approvals = std::collections::BTreeSet::new();
    for event in events {
        match &event.event {
            AgentEventKindV4::InputRequested { question_id, .. } => {
                pending_inputs.insert(question_id.as_str());
            }
            AgentEventKindV4::UserInputAnswered { question_id, .. } => {
                pending_inputs.remove(question_id.as_str());
            }
            AgentEventKindV4::ToolApprovalRequested { request } => {
                pending_approvals.insert(request.approval_id.as_str());
            }
            AgentEventKindV4::ToolApprovalDecided { approval_id, .. } => {
                pending_approvals.remove(approval_id.as_str());
            }
            _ => {}
        }
    }
    if pending_inputs.is_empty() || pending_inputs.len() > 1 {
        return Err(StoreError::InvalidInput(
            "context compaction requires exactly one pending user question".into(),
        ));
    }
    if !pending_approvals.is_empty() {
        return Err(StoreError::InvalidInput(
            "context compaction is unavailable while tool approval is pending".into(),
        ));
    }
    if super::has_unresolved_side_effect_dispatch(events) {
        return Err(StoreError::InvalidInput(
            "context compaction is unavailable while a side-effect dispatch is unresolved".into(),
        ));
    }
    Ok(())
}

fn ensure_receipt_scope(
    row: &ReceiptRow,
    request: &CompactContextRequestV4,
) -> Result<(), StoreError> {
    if row.project_id != request.project_id
        || row.conversation_id != request.conversation_id
        || row.run_id != request.run_id
    {
        return Err(StoreError::InvalidInput(
            "context compaction request id is bound to another scope".into(),
        ));
    }
    Ok(())
}

async fn compact_new_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    request: &CompactContextRequestV4,
) -> Result<ContextCompactionReceiptV4, StoreError> {
    ensure_conversation_owner_executor(&mut *tx, request.project_id, request.conversation_id)
        .await?;
    ensure_run_owner_executor(
        &mut *tx,
        request.project_id,
        request.conversation_id,
        request.run_id,
    )
    .await?;

    let (status, value_json) =
        sqlx::query("SELECT status,value_json FROM agent_runs_v4 WHERE run_id=?1")
            .bind(request.run_id.to_string())
            .fetch_optional(&mut **tx)
            .await?
            .ok_or_else(|| StoreError::InvalidInput("context compaction run was not found".into()))
            .and_then(|row| Ok((row.try_get::<String, _>(0)?, row.try_get::<String, _>(1)?)))?;
    if status == "completed" {
        return completed_run_not_needed_in_tx(tx, request, &value_json).await;
    }
    if status != "waiting_for_input" {
        return Err(StoreError::InvalidInput(
            "context compaction requires a paused waiting_for_input run".into(),
        ));
    }
    let value: Value = serde_json::from_str(&value_json)?;
    let spec_value = value.get("spec").ok_or_else(|| {
        StoreError::InvalidInput("context compaction requires a frozen run specification".into())
    })?;
    let spec: RunSpecV4 = serde_json::from_value(spec_value.clone()).map_err(|error| {
        StoreError::InvalidInput(format!("frozen run specification is invalid: {error}"))
    })?;
    validate_spec_scope(&spec, request)?;
    spec.validate_integrity().map_err(|error| {
        StoreError::InvalidInput(format!(
            "frozen run specification failed validation: {error}"
        ))
    })?;
    let frozen_spec_hash = spec.spec_hash.clone();

    let events = load_agent_events_in_tx(&mut *tx, request.run_id).await?;
    validate_paused_run_in_tx(tx, request.run_id, &events).await?;
    if events.iter().any(super::is_terminal_event) {
        return Err(StoreError::InvalidInput(
            "context compaction cannot append to a terminal event chain".into(),
        ));
    }
    let source_through_sequence = events.last().map_or(0, |event| event.sequence);
    let source_head_hash = events
        .last()
        .map_or_else(String::new, |event| event.event_hash.clone());
    let transcript = serde_json::to_string(&events)?;
    let checkpoint = build_checkpoint(&spec, &events);
    if checkpoint.through_sequence > source_through_sequence {
        return Err(StoreError::InvalidInput(
            "context checkpoint is newer than the captured event head".into(),
        ));
    }
    let candidate = build_candidate(&spec, &checkpoint, &events)?;
    let before_bytes = u64::try_from(transcript.len())
        .map_err(|_| StoreError::InvalidInput("context transcript exceeds size range".into()))?;
    let after_bytes = u64::try_from(candidate.len())
        .map_err(|_| StoreError::InvalidInput("context candidate exceeds size range".into()))?;
    let created_at = DateTime::<Utc>::from_timestamp_millis(timestamp(Utc::now()))
        .ok_or_else(|| StoreError::InvalidInput("invalid compaction timestamp".into()))?;

    let mut receipt = ContextCompactionReceiptV4 {
        request_id: request.request_id,
        project_id: request.project_id,
        conversation_id: request.conversation_id,
        run_id: request.run_id,
        status: if after_bytes >= before_bytes {
            ContextCompactionStatusV4::NotNeeded
        } else {
            ContextCompactionStatusV4::Completed
        },
        source_through_sequence,
        source_head_hash: source_head_hash.clone(),
        before_bytes,
        after_bytes: Some(after_bytes),
        archive: None,
        checkpoint_through_sequence: None,
        checkpoint_sha256: None,
        frozen_spec_hash: frozen_spec_hash.clone(),
        message: None,
        created_at,
        updated_at: created_at,
    };

    insert_receipt_in_tx(tx, &receipt, STARTED, None, None, None, None, None).await?;
    append_compaction_event(
        tx,
        request.run_id,
        request.project_id,
        request.conversation_id,
        AgentEventKindV4::ContextCompactionStarted {
            request_id: request.request_id,
            source_through_sequence,
            source_head_hash,
            frozen_spec_hash,
        },
    )
    .await?;

    if after_bytes >= before_bytes {
        append_compaction_event(
            tx,
            request.run_id,
            request.project_id,
            request.conversation_id,
            AgentEventKindV4::ContextCompactionNotNeeded {
                request_id: request.request_id,
                before_bytes,
            },
        )
        .await?;
        receipt.message = Some("context already fits the bounded projection".into());
        update_receipt_in_tx(tx, &receipt, NOT_NEEDED).await?;
        return Ok(receipt);
    }

    let archive = ContextArchiveV4 {
        archive_id: Uuid::new_v4(),
        through_sequence: checkpoint.through_sequence,
        size_bytes: before_bytes,
        sha256: hex::encode(Sha256::digest(transcript.as_bytes())),
    };
    let checkpoint_json = serde_json::to_string(&checkpoint)?;
    sqlx::query(
        "INSERT INTO agent_context_archives_v4
         (archive_id,run_id,through_sequence,size_bytes,sha256,transcript_json,checkpoint_json,created_at)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
    )
    .bind(archive.archive_id.to_string())
    .bind(request.run_id.to_string())
    .bind(i64::try_from(archive.through_sequence).map_err(|_| {
        StoreError::InvalidInput("checkpoint sequence exceeds SQLite range".into())
    })?)
    .bind(i64::try_from(archive.size_bytes).map_err(|_| {
        StoreError::InvalidInput("archive size exceeds SQLite range".into())
    })?)
    .bind(&archive.sha256)
    .bind(&transcript)
    .bind(&checkpoint_json)
    .bind(timestamp(created_at))
    .execute(&mut **tx)
    .await?;
    let checkpoint_sha256 = hex::encode(Sha256::digest(checkpoint_json.as_bytes()));
    append_compaction_event(
        tx,
        request.run_id,
        request.project_id,
        request.conversation_id,
        AgentEventKindV4::ContextArchived {
            archive: archive.clone(),
        },
    )
    .await?;
    append_compaction_event(
        tx,
        request.run_id,
        request.project_id,
        request.conversation_id,
        AgentEventKindV4::ContextCheckpointed {
            checkpoint: checkpoint.clone(),
        },
    )
    .await?;
    append_compaction_event(
        tx,
        request.run_id,
        request.project_id,
        request.conversation_id,
        AgentEventKindV4::ContextCompactionCompleted {
            request_id: request.request_id,
            archive: archive.clone(),
            checkpoint_through_sequence: checkpoint.through_sequence,
            checkpoint_sha256: checkpoint_sha256.clone(),
            before_bytes,
            after_bytes,
        },
    )
    .await?;
    receipt.archive = Some(archive);
    receipt.checkpoint_through_sequence = Some(checkpoint.through_sequence);
    receipt.checkpoint_sha256 = Some(checkpoint_sha256);
    update_receipt_in_tx(tx, &receipt, COMPLETED).await?;
    Ok(receipt)
}

async fn append_compaction_event(
    tx: &mut Transaction<'_, Sqlite>,
    run_id: Uuid,
    project_id: Uuid,
    conversation_id: Uuid,
    kind: AgentEventKindV4,
) -> Result<(), StoreError> {
    let events = load_agent_events_in_tx(&mut *tx, run_id).await?;
    let event = events
        .last()
        .map(|previous| AgentEventV4::next(previous, Utc::now(), kind.clone()))
        .unwrap_or_else(|| {
            AgentEventV4::first(run_id, project_id, conversation_id, Utc::now(), kind)
        });
    insert_agent_event_in_tx(&mut *tx, &event).await
}

async fn insert_receipt_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    receipt: &ContextCompactionReceiptV4,
    status: &str,
    archive_id: Option<Uuid>,
    checkpoint_sequence: Option<u64>,
    checkpoint_sha256: Option<&str>,
    message: Option<&str>,
    updated_at: Option<DateTime<Utc>>,
) -> Result<(), StoreError> {
    sqlx::query(
        "INSERT INTO agent_context_compactions_v4
         (request_id,project_id,conversation_id,run_id,status,
          source_through_sequence,source_head_hash,before_bytes,after_bytes,
          archive_id,checkpoint_through_sequence,checkpoint_sha256,
          frozen_spec_hash,message,created_at,updated_at)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16)",
    )
    .bind(receipt.request_id.to_string())
    .bind(receipt.project_id.to_string())
    .bind(receipt.conversation_id.to_string())
    .bind(receipt.run_id.to_string())
    .bind(status)
    .bind(
        i64::try_from(receipt.source_through_sequence)
            .map_err(|_| StoreError::InvalidInput("source sequence exceeds SQLite range".into()))?,
    )
    .bind(&receipt.source_head_hash)
    .bind(
        i64::try_from(receipt.before_bytes)
            .map_err(|_| StoreError::InvalidInput("context size exceeds SQLite range".into()))?,
    )
    .bind(optional_i64(receipt.after_bytes, "compaction after size")?)
    .bind(archive_id.map(|value| value.to_string()))
    .bind(optional_i64(checkpoint_sequence, "checkpoint sequence")?)
    .bind(checkpoint_sha256)
    .bind(&receipt.frozen_spec_hash)
    .bind(message.or(receipt.message.as_deref()))
    .bind(timestamp(receipt.created_at))
    .bind(timestamp(updated_at.unwrap_or(receipt.updated_at)))
    .execute(&mut **tx)
    .await?;
    Ok(())
}

async fn update_receipt_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    receipt: &ContextCompactionReceiptV4,
    status: &str,
) -> Result<(), StoreError> {
    sqlx::query(
        "UPDATE agent_context_compactions_v4
         SET status=?1,after_bytes=?2,archive_id=?3,
             checkpoint_through_sequence=?4,checkpoint_sha256=?5,
             message=?6,updated_at=?7
         WHERE request_id=?8 AND status=?9",
    )
    .bind(status)
    .bind(optional_i64(receipt.after_bytes, "compaction after size")?)
    .bind(
        receipt
            .archive
            .as_ref()
            .map(|archive| archive.archive_id.to_string()),
    )
    .bind(optional_i64(
        receipt.checkpoint_through_sequence,
        "checkpoint sequence",
    )?)
    .bind(&receipt.checkpoint_sha256)
    .bind(&receipt.message)
    .bind(timestamp(receipt.updated_at))
    .bind(receipt.request_id.to_string())
    .bind(STARTED)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

async fn load_receipt_row_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    request_id: Uuid,
) -> Result<Option<ReceiptRow>, StoreError> {
    sqlx::query(
        "SELECT request_id,project_id,conversation_id,run_id,status,
                source_through_sequence,source_head_hash,before_bytes,after_bytes,
                archive_id,checkpoint_through_sequence,checkpoint_sha256,
                frozen_spec_hash,message,created_at,updated_at
         FROM agent_context_compactions_v4 WHERE request_id=?1",
    )
    .bind(request_id.to_string())
    .fetch_optional(&mut **tx)
    .await?
    .map(receipt_row_from_sql)
    .transpose()
}

async fn load_latest_receipt_for_run_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    project_id: Uuid,
    conversation_id: Uuid,
    run_id: Uuid,
) -> Result<Option<ReceiptRow>, StoreError> {
    sqlx::query(
        "SELECT request_id,project_id,conversation_id,run_id,status,
                source_through_sequence,source_head_hash,before_bytes,after_bytes,
                archive_id,checkpoint_through_sequence,checkpoint_sha256,
                frozen_spec_hash,message,created_at,updated_at
         FROM agent_context_compactions_v4
         WHERE project_id=?1 AND conversation_id=?2 AND run_id=?3
         ORDER BY updated_at DESC, request_id DESC LIMIT 1",
    )
    .bind(project_id.to_string())
    .bind(conversation_id.to_string())
    .bind(run_id.to_string())
    .fetch_optional(&mut **tx)
    .await?
    .map(receipt_row_from_sql)
    .transpose()
}

fn receipt_row_from_sql(row: sqlx::sqlite::SqliteRow) -> Result<ReceiptRow, StoreError> {
    Ok(ReceiptRow {
        request_id: parse_uuid(row.try_get::<String, _>(0)?, "compaction request id")?,
        project_id: parse_uuid(row.try_get::<String, _>(1)?, "compaction project id")?,
        conversation_id: parse_uuid(row.try_get::<String, _>(2)?, "compaction conversation id")?,
        run_id: parse_uuid(row.try_get::<String, _>(3)?, "compaction run id")?,
        status: row.try_get(4)?,
        source_through_sequence: u64::try_from(row.try_get::<i64, _>(5)?).map_err(|_| {
            StoreError::InvalidInput("compaction source sequence is negative".into())
        })?,
        source_head_hash: row.try_get(6)?,
        before_bytes: u64::try_from(row.try_get::<i64, _>(7)?)
            .map_err(|_| StoreError::InvalidInput("compaction before size is negative".into()))?,
        after_bytes: row
            .try_get::<Option<i64>, _>(8)?
            .map(|value| {
                u64::try_from(value).map_err(|_| {
                    StoreError::InvalidInput("compaction after size is negative".into())
                })
            })
            .transpose()?,
        archive_id: row
            .try_get::<Option<String>, _>(9)?
            .map(|value| parse_uuid(value, "compaction archive id"))
            .transpose()?,
        archive: None,
        checkpoint_through_sequence: row
            .try_get::<Option<i64>, _>(10)?
            .map(|value| {
                u64::try_from(value).map_err(|_| {
                    StoreError::InvalidInput("compaction checkpoint sequence is negative".into())
                })
            })
            .transpose()?,
        checkpoint_sha256: row.try_get(11)?,
        frozen_spec_hash: row.try_get(12)?,
        message: row.try_get(13)?,
        created_at: DateTime::<Utc>::from_timestamp_millis(row.try_get(14)?).ok_or_else(|| {
            StoreError::InvalidInput("compaction created timestamp is invalid".into())
        })?,
        updated_at: DateTime::<Utc>::from_timestamp_millis(row.try_get(15)?).ok_or_else(|| {
            StoreError::InvalidInput("compaction updated timestamp is invalid".into())
        })?,
    })
}

fn receipt_from_row(row: ReceiptRow) -> Result<ContextCompactionReceiptV4, StoreError> {
    let status = match row.status.as_str() {
        NOT_NEEDED => ContextCompactionStatusV4::NotNeeded,
        COMPLETED => ContextCompactionStatusV4::Completed,
        ATTENTION => ContextCompactionStatusV4::Attention,
        STARTED => {
            return Err(StoreError::InvalidInput(
                "context compaction is still in progress".into(),
            ));
        }
        _ => {
            return Err(StoreError::InvalidInput(
                "context compaction receipt has an invalid status".into(),
            ));
        }
    };
    Ok(ContextCompactionReceiptV4 {
        request_id: row.request_id,
        project_id: row.project_id,
        conversation_id: row.conversation_id,
        run_id: row.run_id,
        status,
        source_through_sequence: row.source_through_sequence,
        source_head_hash: row.source_head_hash,
        before_bytes: row.before_bytes,
        after_bytes: row.after_bytes,
        archive: row.archive,
        checkpoint_through_sequence: row.checkpoint_through_sequence,
        checkpoint_sha256: row.checkpoint_sha256,
        frozen_spec_hash: row.frozen_spec_hash,
        message: row.message,
        created_at: row.created_at,
        updated_at: row.updated_at,
    })
}

async fn hydrate_archive_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    row: &mut ReceiptRow,
) -> Result<(), StoreError> {
    let Some(archive_id) = row.archive_id else {
        return Ok(());
    };
    let archive = sqlx::query(
        "SELECT run_id,through_sequence,size_bytes,sha256
         FROM agent_context_archives_v4 WHERE archive_id=?1",
    )
    .bind(archive_id.to_string())
    .fetch_optional(&mut **tx)
    .await?;
    let Some(archive) = archive else {
        return Err(StoreError::InvalidInput(
            "context compaction receipt references a missing archive".into(),
        ));
    };
    let archive_run_id = parse_uuid(archive.try_get(0)?, "archive run id")?;
    if archive_run_id != row.run_id {
        return Err(StoreError::InvalidInput(
            "context compaction archive ownership does not match its receipt".into(),
        ));
    }
    row.archive = Some(ContextArchiveV4 {
        archive_id,
        through_sequence: u64::try_from(archive.try_get::<i64, _>(1)?)
            .map_err(|_| StoreError::InvalidInput("archive sequence is negative".into()))?,
        size_bytes: u64::try_from(archive.try_get::<i64, _>(2)?)
            .map_err(|_| StoreError::InvalidInput("archive size is negative".into()))?,
        sha256: archive.try_get(3)?,
    });
    Ok(())
}

fn optional_i64(value: Option<u64>, label: &str) -> Result<Option<i64>, StoreError> {
    value
        .map(|value| {
            i64::try_from(value)
                .map_err(|_| StoreError::InvalidInput(format!("{label} exceeds SQLite range")))
        })
        .transpose()
}

fn parse_uuid(value: String, label: &str) -> Result<Uuid, StoreError> {
    Uuid::parse_str(&value)
        .map_err(|error| StoreError::InvalidInput(format!("invalid {label}: {error}")))
}

fn build_checkpoint(spec: &RunSpecV4, events: &[AgentEventV4]) -> ContextCheckpointV4 {
    let resolved_uncertain = events
        .iter()
        .filter_map(|event| match &event.event {
            AgentEventKindV4::ToolDispatchResolved { call_id, .. } => Some(call_id),
            _ => None,
        })
        .collect::<std::collections::BTreeSet<_>>();
    let unresolved_errors = events
        .iter()
        .filter_map(|event| match &event.event {
            AgentEventKindV4::ToolFinished { outcome } if !outcome.succeeded => {
                Some(format!("{}: {}", outcome.tool_id, outcome.model_content))
            }
            AgentEventKindV4::ToolDispatchUncertain { call_id, tool_id }
                if !resolved_uncertain.contains(call_id) =>
            {
                Some(format!("uncertain dispatch {tool_id} ({call_id})"))
            }
            AgentEventKindV4::RunFailed { message } => Some(message.clone()),
            _ => None,
        })
        .rev()
        .take(12)
        .map(|value| truncate_utf8(&value, MAX_CHECKPOINT_STEP_BYTES))
        .collect();
    let recent_steps = events
        .iter()
        .rev()
        .filter_map(|event| match &event.event {
            AgentEventKindV4::ModelText { text } => Some(format!("model: {text}")),
            AgentEventKindV4::ToolFinished { outcome } => Some(format!(
                "tool {}: {}",
                outcome.tool_id, outcome.model_content
            )),
            AgentEventKindV4::UserInputAnswered {
                question_id,
                answer,
            } => Some(format!("input {question_id}: {answer}")),
            AgentEventKindV4::InputRequested {
                question_id,
                question,
                reason,
            } => Some(format!(
                "pending input {question_id} ({reason:?}): {question}"
            )),
            _ => None,
        })
        .take(MAX_RECENT_STEPS)
        .map(|value| truncate_utf8(&value, MAX_CHECKPOINT_STEP_BYTES))
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    ContextCheckpointV4 {
        schema_version: 4,
        through_sequence: events.last().map_or(0, |event| event.sequence),
        completion_criteria: spec.plan.completion_criteria.clone(),
        unresolved_errors,
        recent_steps,
        scientific_state: Value::Null,
        task_shape: None,
        phase: None,
        task_revision: None,
        tasks: Vec::new(),
        cycle_id: None,
    }
}

fn build_candidate(
    spec: &RunSpecV4,
    checkpoint: &ContextCheckpointV4,
    events: &[AgentEventV4],
) -> Result<String, StoreError> {
    let active_guidance = events
        .iter()
        .filter_map(|event| match &event.event {
            AgentEventKindV4::GuidanceConsumed {
                message_id,
                markdown,
            } => Some(json!({
                "message_id": message_id,
                "markdown": truncate_utf8(markdown, MAX_CHECKPOINT_STEP_BYTES),
            })),
            _ => None,
        })
        .take(MAX_GUIDANCE_ITEMS)
        .collect::<Vec<_>>();
    serde_json::to_string(&json!({
        "frozen_plan": spec.plan,
        "compute_selection": spec.compute_selection,
        "checkpoint": checkpoint,
        "recent_events": [],
        "scientific_state": Value::Null,
        "active_guidance": active_guidance,
    }))
    .map_err(StoreError::from)
}

fn truncate_utf8(value: &str, max_bytes: usize) -> String {
    if value.len() <= max_bytes {
        return value.to_owned();
    }
    let mut end = max_bytes;
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].to_owned()
}

/// Validate receipts during Store initialization. This is intentionally
/// bounded to identity/hash columns; the archive validator remains the source
/// of truth for full transcript bytes.
pub(super) async fn validate_context_compactions(
    tx: &mut Transaction<'_, Sqlite>,
) -> Result<(), StoreError> {
    let rows = sqlx::query(
        "SELECT c.request_id,c.project_id,c.conversation_id,c.run_id,c.status,
                c.source_through_sequence,c.source_head_hash,c.before_bytes,c.after_bytes,
                c.archive_id,c.checkpoint_through_sequence,c.checkpoint_sha256,
                c.frozen_spec_hash,c.message,c.created_at,c.updated_at,
                p.id,con.frame_id,r.run_id
         FROM agent_context_compactions_v4 c
         LEFT JOIN projects p ON p.id=c.project_id
         LEFT JOIN conversation_records con ON con.frame_id=c.conversation_id
         LEFT JOIN agent_runs_v4 r ON r.run_id=c.run_id
         ORDER BY c.request_id",
    )
    .fetch_all(&mut **tx)
    .await?;
    for row in rows {
        let request_id = row.try_get::<String, _>(0)?;
        let owner_project = row.try_get::<Option<String>, _>(16)?;
        let owner_conversation = row.try_get::<Option<String>, _>(17)?;
        let owner_run = row.try_get::<Option<String>, _>(18)?;
        let parsed = receipt_row_from_sql(row)?;
        let valid_status = matches!(
            parsed.status.as_str(),
            STARTED | NOT_NEEDED | COMPLETED | ATTENTION
        );
        let expected_project = parsed.project_id.to_string();
        let expected_conversation = parsed.conversation_id.to_string();
        let expected_run = parsed.run_id.to_string();
        let scope_matches = owner_project.as_deref() == Some(expected_project.as_str())
            && owner_conversation.as_deref() == Some(expected_conversation.as_str())
            && owner_run.as_deref() == Some(expected_run.as_str());
        if !valid_status || !scope_matches || request_id.trim().is_empty() {
            return Err(StoreError::Migration(format!(
                "context compaction receipt {request_id} has invalid status, identity, or ownership"
            )));
        }
        if parsed.status == COMPLETED
            && (parsed.archive_id.is_none()
                || parsed.checkpoint_through_sequence.is_none()
                || parsed
                    .checkpoint_sha256
                    .as_deref()
                    .map_or(true, str::is_empty))
        {
            return Err(StoreError::Migration(format!(
                "context compaction receipt {request_id} has an incomplete completed result"
            )));
        }
    }
    Ok(())
}
