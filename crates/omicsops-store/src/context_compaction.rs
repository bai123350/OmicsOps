//! Durable, idempotent manual context compaction.
//!
//! The Store owns the compaction transaction because an archive, its
//! checkpoint events, and the bounded receipt must become visible together.
//! The complete event transcript is retained in `agent_context_archives_v4`;
//! the receipt only contains scope, status, sizes, and hashes.

use chrono::{DateTime, Utc};
use omicsops_core::workspace::ModelProfile;
use omicsops_protocol::{
    AgentEventKindV4, AgentEventV4, CompactContextRequestV4, ComputeBackendKindV4,
    ContextArchiveV4, ContextCheckpointV4, ContextCompactionAdmissionV4,
    ContextCompactionReceiptV4, ContextCompactionSnapshotV4, ContextCompactionStatusV4,
    PreparedContextCompactionV4, RunExecutionKindV4, RunSpecV4,
};
use omicsops_science::ScientificStateV4;
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

#[derive(Debug, Clone)]
pub struct CompactionSnapshotV4 {
    pub context: ContextCompactionSnapshotV4,
    pub run_value: Value,
}

impl Store {
    /// Capture one read-consistent source without reconciliation or writes.
    /// The desktop host must hold its run lease and exclude active drivers.
    pub async fn context_compaction_snapshot_v4(
        &self,
        request: &CompactContextRequestV4,
    ) -> Result<CompactionSnapshotV4, StoreError> {
        validate_request(request)?;
        let mut tx = self.pool.begin().await?;
        let snapshot = snapshot_in_tx(&mut tx, request).await?;
        tx.commit().await?;
        Ok(snapshot)
    }

    /// Recheck the captured source and atomically persist the exact admitted
    /// archive/event chain. No intermediate STARTED state escapes this commit.
    pub async fn commit_context_compaction_v4(
        &self,
        request: &CompactContextRequestV4,
        prepared: &PreparedContextCompactionV4,
    ) -> Result<ContextCompactionReceiptV4, StoreError> {
        validate_request(request)?;
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        if let Some(mut row) = load_receipt_row_in_tx(&mut tx, request.request_id).await? {
            ensure_receipt_scope(&row, request)?;
            hydrate_archive_in_tx(&mut tx, &mut row).await?;
            tx.commit().await?;
            if row.status == STARTED {
                return self
                    .context_compaction_receipt_v4(request)
                    .await?
                    .ok_or_else(|| invalid("context compaction request has no durable outcome"));
            }
            return receipt_from_row(row);
        }
        let result = commit_prepared_in_tx(&mut tx, request, prepared).await;
        match result {
            Ok(receipt) => {
                tx.commit().await?;
                Ok(receipt)
            }
            Err(error) => {
                let _ = tx.rollback().await;
                Err(error)
            }
        }
    }

    /// Request-specific idempotency lookup. Only historical STARTED rows use
    /// the conservative recovery writer; ordinary lookups are read-only.
    pub async fn context_compaction_receipt_v4(
        &self,
        request: &CompactContextRequestV4,
    ) -> Result<Option<ContextCompactionReceiptV4>, StoreError> {
        validate_request(request)?;
        let mut tx = self.pool.begin().await?;
        ensure_conversation_owner_executor(&mut tx, request.project_id, request.conversation_id)
            .await?;
        ensure_run_owner_executor(
            &mut tx,
            request.project_id,
            request.conversation_id,
            request.run_id,
        )
        .await?;
        let Some(mut row) = load_receipt_row_in_tx(&mut tx, request.request_id).await? else {
            tx.commit().await?;
            return Ok(None);
        };
        ensure_receipt_scope(&row, request)?;
        hydrate_archive_in_tx(&mut tx, &mut row).await?;
        tx.commit().await?;
        if row.status == STARTED {
            return self
                .reconcile_context_compaction_request_v4(
                    request.project_id,
                    request.conversation_id,
                    request.run_id,
                    Some(request.request_id),
                )
                .await;
        }
        receipt_from_row(row).map(Some)
    }
}

fn invalid(message: &str) -> StoreError {
    StoreError::InvalidInput(message.into())
}

async fn snapshot_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    request: &CompactContextRequestV4,
) -> Result<CompactionSnapshotV4, StoreError> {
    ensure_conversation_owner_executor(&mut *tx, request.project_id, request.conversation_id)
        .await?;
    ensure_run_owner_executor(
        &mut *tx,
        request.project_id,
        request.conversation_id,
        request.run_id,
    )
    .await?;
    let row = sqlx::query("SELECT status,value_json FROM agent_runs_v4 WHERE run_id=?1")
        .bind(request.run_id.to_string())
        .fetch_one(&mut **tx)
        .await?;
    let run_json: String = row.try_get(1)?;
    let run_value: Value = serde_json::from_str(&run_json)?;
    if row.try_get::<String, _>(0)? != "waiting_for_input"
        || run_value.get("status").and_then(Value::as_str) != Some("waiting_for_input")
    {
        return Err(invalid(
            "context compaction requires a paused waiting_for_input run",
        ));
    }
    let spec: RunSpecV4 = serde_json::from_value(
        run_value
            .get("spec")
            .cloned()
            .ok_or_else(|| invalid("context compaction requires a frozen run specification"))?,
    )?;
    validate_spec_scope(&spec, request)?;
    spec.validate_integrity()
        .map_err(|_| invalid("invalid frozen run specification"))?;
    if spec.execution_kind != RunExecutionKindV4::OrdinaryAgent {
        return Err(invalid("context compaction requires an ordinary agent run"));
    }
    let frozen_hash = spec
        .spec_hash
        .as_deref()
        .ok_or_else(|| invalid("context compaction requires an anchored frozen spec"))?;
    let events = load_agent_events_in_tx(&mut *tx, request.run_id).await?;
    if events.iter().any(|e| {
        e.project_id != request.project_id
            || e.conversation_id != request.conversation_id
            || e.run_id != request.run_id
            || super::is_terminal_event(e)
    }) {
        return Err(invalid(
            "invalid context compaction event scope or terminal chain",
        ));
    }
    let anchor = events.iter().rev().find_map(|event| match &event.event {
        AgentEventKindV4::RunSpecFrozen { spec_hash, .. } => Some(spec_hash.as_str()),
        _ => None,
    });
    if anchor != Some(frozen_hash) {
        return Err(invalid(
            "frozen specification differs from its event anchor",
        ));
    }
    validate_paused_run_in_tx(tx, request.run_id, &events).await?;
    let pending: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM agent_guidance_v4 WHERE run_id=?1 AND consumed_at IS NULL",
    )
    .bind(request.run_id.to_string())
    .fetch_one(&mut **tx)
    .await?;
    if pending != 0 {
        return Err(invalid(
            "context compaction is unavailable while guidance is pending",
        ));
    }
    validate_bindings_in_tx(tx, &spec).await?;
    let science = science_in_tx(tx, request.project_id).await?;
    let head = events
        .last()
        .ok_or_else(|| invalid("context compaction requires a durable event head"))?;
    let context = ContextCompactionSnapshotV4 {
        source_bindings_sha256: binding_fingerprint_in_tx(tx, &spec).await?,
        source_run_sha256: hex::encode(Sha256::digest(run_json.as_bytes())),
        request: request.clone(),
        spec,
        scientific_revision: science.revision,
        scientific_sha256: science.digest(),
        scientific_state: serde_json::to_value(science)?,
        source_through_sequence: head.sequence,
        source_head_hash: head.event_hash.clone(),
        events,
    };
    Ok(CompactionSnapshotV4 { context, run_value })
}

async fn science_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    project_id: Uuid,
) -> Result<ScientificStateV4, StoreError> {
    let row = sqlx::query("SELECT project_id,revision,state_sha256,value_json FROM scientific_states_v4 WHERE project_id=?1").bind(project_id.to_string()).fetch_optional(&mut **tx).await?;
    let Some(row) = row else {
        return Ok(ScientificStateV4::new(project_id));
    };
    let state: ScientificStateV4 = serde_json::from_str(&row.try_get::<String, _>(3)?)?;
    if state.project_id != project_id
        || state.project_id.to_string() != row.try_get::<String, _>(0)?
        || i64::try_from(state.revision).ok() != Some(row.try_get::<i64, _>(1)?)
        || state.digest() != row.try_get::<String, _>(2)?
    {
        return Err(invalid("scientific state has inconsistent durable columns"));
    }
    Ok(state)
}

async fn profile_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    id: Uuid,
    hash: &str,
) -> Result<(), StoreError> {
    let raw: Option<String> =
        sqlx::query_scalar("SELECT value_json FROM model_profiles WHERE id=?1")
            .bind(id.to_string())
            .fetch_optional(&mut **tx)
            .await?;
    let profile: ModelProfile =
        serde_json::from_str(&raw.ok_or_else(|| invalid("frozen model profile is unavailable"))?)?;
    if profile.id != id || profile.execution_configuration_hash() != hash {
        return Err(invalid("frozen model configuration changed"));
    }
    Ok(())
}

async fn validate_bindings_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    spec: &RunSpecV4,
) -> Result<(), StoreError> {
    profile_in_tx(
        tx,
        spec.model_profile_id,
        spec.model_configuration_hash
            .as_deref()
            .ok_or_else(|| invalid("manual compaction requires a frozen model configuration"))?,
    )
    .await?;
    if let Some(binding) = &spec.delegated_model {
        profile_in_tx(tx, binding.profile_id, &binding.configuration_hash).await?;
    }
    if let Some(binding) = &spec.reviewer_model {
        profile_in_tx(tx, binding.profile_id, &binding.configuration_hash).await?;
    }
    let selection = spec
        .compute_selection
        .as_ref()
        .ok_or_else(|| invalid("manual compaction requires a frozen compute selection"))?;
    selection
        .validate()
        .map_err(|_| invalid("invalid frozen compute selection"))?;
    let project = sqlx::query("SELECT COALESCE(o.local_root,p.workspace_dir),o.remote_root,o.connection_id FROM projects p LEFT JOIN project_omicsops o ON o.project_id=p.id WHERE p.id=?1").bind(spec.project_id.to_string()).fetch_one(&mut **tx).await?;
    if project.try_get::<String, _>(0)?.trim().is_empty() {
        return Err(invalid("project local binding is unavailable"));
    }
    if selection.backend_kind == ComputeBackendKindV4::Ssh {
        let connection = project
            .try_get::<Option<String>, _>(2)?
            .ok_or_else(|| invalid("project remote connection is unavailable"))?;
        if selection.backend_id != format!("ssh:{connection}")
            || project.try_get::<Option<String>, _>(1)?.is_none()
        {
            return Err(invalid("project compute binding changed"));
        }
    }
    Ok(())
}

/// Only execution identity and trust metadata enter this digest. Credentials,
/// labels, and unrelated project metadata are deliberately not fingerprinted.
async fn binding_fingerprint_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    spec: &RunSpecV4,
) -> Result<String, StoreError> {
    let project = sqlx::query("SELECT COALESCE(o.local_root,p.workspace_dir),o.remote_root,o.connection_id FROM projects p LEFT JOIN project_omicsops o ON o.project_id=p.id WHERE p.id=?1").bind(spec.project_id.to_string()).fetch_one(&mut **tx).await?;
    let connection_id: Option<String> = project.try_get(2)?;
    let mut connection = Value::Null;
    if let Some(id) = &connection_id {
        let raw: Option<String> =
            sqlx::query_scalar("SELECT profile_json FROM connections WHERE id=?1")
                .bind(id)
                .fetch_optional(&mut **tx)
                .await?;
        let profile: omicsops_core::domain::ConnectionProfile = serde_json::from_str(
            &raw.ok_or_else(|| invalid("project connection is unavailable"))?,
        )?;
        if profile.id.to_string() != *id {
            return Err(invalid("project connection identity mismatch"));
        }
        if spec
            .compute_selection
            .as_ref()
            .is_some_and(|selection| selection.backend_kind == ComputeBackendKindV4::Ssh)
            && profile
                .host_key_fingerprint
                .as_deref()
                .is_none_or(|value| value.trim().is_empty())
        {
            return Err(invalid("project SSH connection is not trusted"));
        }
        connection = json!({"id": profile.id,"host":profile.host,"port":profile.port,"username":profile.username,"authentication":profile.authentication,"host_key_fingerprint":profile.host_key_fingerprint});
    }
    let identity = json!({"local_root":project.try_get::<String,_>(0)?,"remote_root":project.try_get::<Option<String>,_>(1)?,"connection_id":connection_id,"connection":connection});
    Ok(hex::encode(Sha256::digest(serde_json::to_vec(&identity)?)))
}

async fn commit_prepared_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    request: &CompactContextRequestV4,
    prepared: &PreparedContextCompactionV4,
) -> Result<ContextCompactionReceiptV4, StoreError> {
    prepared
        .validate_structure()
        .map_err(|_| invalid("invalid prepared context compaction"))?;
    if &prepared.request != request {
        return Err(invalid("prepared context compaction scope mismatch"));
    }
    let snapshot = snapshot_in_tx(tx, request).await?.context;
    if prepared.source_run_sha256 != snapshot.source_run_sha256
        || prepared.source_bindings_sha256 != snapshot.source_bindings_sha256
        || prepared.source_through_sequence != snapshot.source_through_sequence
        || prepared.source_head_hash != snapshot.source_head_hash
        || snapshot.spec.spec_hash.as_deref() != Some(prepared.frozen_spec_hash.as_str())
        || prepared.scientific_revision != snapshot.scientific_revision
        || prepared.scientific_sha256 != snapshot.scientific_sha256
        || prepared.transcript != serde_json::to_string(&snapshot.events)?
        || snapshot.spec.model_configuration_hash.as_deref()
            != Some(prepared.admission.configuration_hash.as_str())
    {
        return Err(invalid(
            "context compaction source or frozen binding changed",
        ));
    }
    if let Some(checkpoint) = &prepared.checkpoint {
        if checkpoint.scientific_state != snapshot.scientific_state
            || checkpoint.completion_criteria != snapshot.spec.plan.completion_criteria
            || prepared
                .archive
                .as_ref()
                .is_none_or(|archive| archive.archive_id.is_nil())
        {
            return Err(invalid(
                "prepared checkpoint does not preserve captured science and plan",
            ));
        }
    }
    let now = DateTime::<Utc>::from_timestamp_millis(timestamp(Utc::now()))
        .ok_or_else(|| invalid("invalid compaction timestamp"))?;
    let checkpoint_json = prepared
        .checkpoint
        .as_ref()
        .map(serde_json::to_string)
        .transpose()?;
    let checkpoint_sha256 = checkpoint_json
        .as_ref()
        .map(|json| hex::encode(Sha256::digest(json.as_bytes())));
    if let Some(archive) = &prepared.archive {
        sqlx::query(
            "INSERT INTO agent_context_archives_v4
             (archive_id,run_id,through_sequence,size_bytes,sha256,transcript_json,checkpoint_json,created_at)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
        )
        .bind(archive.archive_id.to_string())
        .bind(request.run_id.to_string())
        .bind(optional_i64(Some(archive.through_sequence), "archive sequence")?)
        .bind(optional_i64(Some(archive.size_bytes), "archive size")?)
        .bind(&archive.sha256)
        .bind(&prepared.transcript)
        .bind(&checkpoint_json)
        .bind(timestamp(now))
        .execute(&mut **tx)
        .await?;
    }
    for event in &prepared.planned_events {
        insert_agent_event_in_tx(&mut *tx, event).await?;
    }
    let completed = prepared.status == ContextCompactionStatusV4::Completed;
    let receipt = ContextCompactionReceiptV4 {
        request_id: request.request_id,
        project_id: request.project_id,
        conversation_id: request.conversation_id,
        run_id: request.run_id,
        status: prepared.status,
        source_through_sequence: prepared.source_through_sequence,
        source_head_hash: prepared.source_head_hash.clone(),
        before_bytes: prepared.before_bytes,
        after_bytes: Some(prepared.after_bytes),
        provider_request_bytes: completed
            .then_some(prepared.admission.provider_request_bytes)
            .flatten(),
        admission_configuration_hash: completed
            .then(|| prepared.admission.configuration_hash.clone()),
        archive: prepared.archive.clone(),
        checkpoint_through_sequence: prepared.checkpoint.as_ref().map(|c| c.through_sequence),
        checkpoint_sha256,
        frozen_spec_hash: Some(prepared.frozen_spec_hash.clone()),
        message: None,
        created_at: now,
        updated_at: now,
    };
    insert_receipt_in_tx(
        tx,
        &receipt,
        if completed { COMPLETED } else { NOT_NEEDED },
        receipt.archive.as_ref().map(|a| a.archive_id),
        receipt.checkpoint_through_sequence,
        receipt.checkpoint_sha256.as_deref(),
        None,
        None,
    )
    .await?;
    Ok(receipt)
}

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
        self.reconcile_context_compaction_request_v4(project_id, conversation_id, run_id, None)
            .await
    }

    async fn reconcile_context_compaction_request_v4(
        &self,
        project_id: Uuid,
        conversation_id: Uuid,
        run_id: Uuid,
        request_id: Option<Uuid>,
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
        let selected = match request_id {
            Some(id) => load_receipt_row_in_tx(&mut tx, id).await?,
            None => {
                load_latest_receipt_for_run_in_tx(&mut tx, project_id, conversation_id, run_id)
                    .await?
            }
        };
        let Some(mut row) = selected else {
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
                    ..
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
                    let recovered_at =
                        DateTime::<Utc>::from_timestamp_millis(timestamp(Utc::now()))
                            .ok_or_else(|| invalid("invalid compaction recovery timestamp"))?;
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
                    .bind(timestamp(recovered_at))
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
                    row.updated_at = recovered_at;
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
        hydrate_archive_in_tx(&mut tx, &mut row).await?;
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
    admission: Option<ContextCompactionAdmissionV4>,
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
        provider_request_bytes: None,
        admission_configuration_hash: None,
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
        provider_request_bytes: None,
        admission_configuration_hash: None,
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
            admission: None,
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
        admission: None,
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
        provider_request_bytes: row
            .admission
            .as_ref()
            .and_then(|a| a.provider_request_bytes),
        admission_configuration_hash: row.admission.map(|a| a.configuration_hash),
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
        if row.status == COMPLETED {
            return Err(invalid("completed context compaction has no archive"));
        }
        return Ok(());
    };
    let archive = sqlx::query(
        "SELECT run_id,through_sequence,size_bytes,sha256,transcript_json,checkpoint_json
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
    if row.status == COMPLETED {
        let events = load_agent_events_in_tx(&mut *tx, row.run_id).await?;
        let matches = events
            .iter()
            .filter(|event| matches!(&event.event, AgentEventKindV4::ContextCompactionCompleted { request_id, .. } if *request_id == row.request_id))
            .collect::<Vec<_>>();
        let [completed] = matches.as_slice() else {
            return Err(invalid(
                "completed compaction receipt requires one verified completion event",
            ));
        };
        let AgentEventKindV4::ContextCompactionCompleted {
            archive: event_archive,
            checkpoint_through_sequence,
            checkpoint_sha256,
            before_bytes,
            after_bytes,
            admission,
            ..
        } = &completed.event
        else {
            unreachable!()
        };
        let transcript: String = archive.try_get(4)?;
        let checkpoint_json: String = archive.try_get(5)?;
        let checkpoint: ContextCheckpointV4 = serde_json::from_str(&checkpoint_json)?;
        if completed.project_id != row.project_id
            || completed.conversation_id != row.conversation_id
            || row.archive.as_ref() != Some(event_archive)
            || Some(*checkpoint_through_sequence) != row.checkpoint_through_sequence
            || Some(checkpoint_sha256) != row.checkpoint_sha256.as_ref()
            || *before_bytes != row.before_bytes
            || Some(*after_bytes) != row.after_bytes
            || transcript.len() as u64 != event_archive.size_bytes
            || hex::encode(Sha256::digest(transcript.as_bytes())) != event_archive.sha256
            || hex::encode(Sha256::digest(checkpoint_json.as_bytes())) != *checkpoint_sha256
            || checkpoint.through_sequence != *checkpoint_through_sequence
        {
            return Err(invalid(
                "compaction receipt or archive differs from its verified completion event",
            ));
        }
        if let Some(admission) = admission {
            let bounded_hash = |s: &str| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit());
            if !bounded_hash(&admission.configuration_hash)
                || !bounded_hash(&admission.prompt_sha256)
            {
                return Err(invalid("invalid completed compaction admission hash"));
            }
            row.admission = Some(admission.clone());
        }
    }
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

#[cfg(test)]
mod manual_tests {
    use super::*;
    use omicsops_core::workspace::{Conversation, ModelProfile, Project, ProjectTemplate};
    use omicsops_protocol::*;
    use omicsops_science::ScientificStateV4;

    async fn fixture(store: Store) -> (Store, ContextCompactionSnapshotV4) {
        let project = Project::new(
            Uuid::new_v4(),
            "manual fixture",
            "C:\\fixture",
            ProjectTemplate::Blank,
            Utc::now(),
        );
        store.save_project(&project).await.unwrap();
        let conversation = Conversation::new(Uuid::new_v4(), project.id, "manual", Utc::now());
        store.save_conversation(&conversation).await.unwrap();
        let profile: ModelProfile = serde_json::from_value(json!({"id":Uuid::new_v4(),"label":"fixture","provider":"open_ai_compatible","base_url":"https://example.invalid/v1","model":"fixture","credential_reference":null,"supports_tools":true,"supports_vision":false,"context_window_tokens":32768})).unwrap();
        store.save_model_profile(&profile).await.unwrap();
        let run_id = Uuid::new_v4();
        let plan = ExecutionPlanV4 {
            schema_version: 4,
            objective: "retain evidence".into(),
            steps: vec!["inspect".into()],
            completion_criteria: vec!["verifiable".into()],
            requested_capabilities: Default::default(),
        };
        let selection = ComputeSelectionV4 {
            schema_version: 4,
            backend_id: "local".into(),
            backend_kind: ComputeBackendKindV4::Local,
            autonomy_mode: AutonomyModeV4::Supervised,
            approval_policy: ApprovalPolicyV4::RiskBased,
            environment: "system".into(),
            network_policy: NetworkPolicyV4::HostInherited,
            container_image: None,
        };
        let approval = RunSpecV4::approval_hash_for(
            run_id,
            project.id,
            conversation.id,
            profile.id,
            &plan,
            &selection,
        )
        .unwrap();
        let mut spec = RunSpecV4::freeze_ordinary_agent_with_compute(
            run_id,
            project.id,
            conversation.id,
            profile.id,
            plan,
            selection,
            &approval,
            Utc::now(),
        )
        .unwrap();
        spec.model_configuration_hash = Some(profile.execution_configuration_hash());
        let mut child = profile.clone();
        child.id = Uuid::new_v4();
        store.save_model_profile(&child).await.unwrap();
        spec.delegated_model = Some(DelegatedModelBindingV4 {
            profile_id: child.id,
            configuration_hash: child.execution_configuration_hash(),
        });
        let mut reviewer = profile.clone();
        reviewer.id = Uuid::new_v4();
        store.save_model_profile(&reviewer).await.unwrap();
        spec.reviewer_model = Some(ReviewerModelBindingV4 {
            profile_id: reviewer.id,
            configuration_hash: reviewer.execution_configuration_hash(),
            service_tier: RunServiceTierV4 { fast_mode: None },
        });
        spec.spec_hash = Some(spec.calculate_spec_hash().unwrap());
        store
            .save_agent_run_v4(
                run_id,
                project.id,
                conversation.id,
                "waiting_for_input",
                &json!({"status":"waiting_for_input","spec":spec}),
            )
            .await
            .unwrap();
        let first = AgentEventV4::first(
            run_id,
            project.id,
            conversation.id,
            Utc::now(),
            AgentEventKindV4::RunCreated {
                mode: RunModeV4::Execute,
            },
        );
        let mut events = vec![first];
        for kind in [
            AgentEventKindV4::RunSpecFrozen {
                approval_hash: approval,
                spec_hash: spec.spec_hash.clone().unwrap(),
            },
            AgentEventKindV4::ModelText {
                text: "evidence ".repeat(3000),
            },
            AgentEventKindV4::InputRequested {
                question_id: "q".into(),
                question: "Next?".into(),
                reason: AgentInputReasonV4::Decision,
            },
        ] {
            events.push(AgentEventV4::next(events.last().unwrap(), Utc::now(), kind));
        }
        for event in &events {
            store.append_agent_event_v4(event).await.unwrap();
        }
        let science = ScientificStateV4::new(project.id);
        let snapshot = ContextCompactionSnapshotV4 {
            source_run_sha256: {
                let raw: String =
                    sqlx::query_scalar("SELECT value_json FROM agent_runs_v4 WHERE run_id=?1")
                        .bind(run_id.to_string())
                        .fetch_one(&store.pool)
                        .await
                        .unwrap();
                hex::encode(Sha256::digest(raw.as_bytes()))
            },
            source_bindings_sha256: {
                let mut tx = store.pool.begin().await.unwrap();
                binding_fingerprint_in_tx(&mut tx, &spec).await.unwrap()
            },
            request: CompactContextRequestV4 {
                request_id: Uuid::new_v4(),
                project_id: project.id,
                conversation_id: conversation.id,
                run_id,
            },
            spec,
            source_through_sequence: events.last().unwrap().sequence,
            source_head_hash: events.last().unwrap().event_hash.clone(),
            events,
            scientific_state: serde_json::to_value(&science).unwrap(),
            scientific_revision: science.revision,
            scientific_sha256: science.digest(),
        };
        (store, snapshot)
    }

    // Store tests construct an admitted protocol payload; provider admission is
    // tested in agent-core. No model, filesystem execution or network is used.
    fn prepared(source: &ContextCompactionSnapshotV4) -> PreparedContextCompactionV4 {
        let transcript = serde_json::to_string(&source.events).unwrap();
        let checkpoint = ContextCheckpointV4 {
            schema_version: 4,
            through_sequence: source.source_through_sequence,
            completion_criteria: source.spec.plan.completion_criteria.clone(),
            unresolved_errors: vec![],
            recent_steps: vec!["pending input q: Next?".into()],
            scientific_state: source.scientific_state.clone(),
            task_shape: None,
            phase: None,
            task_revision: None,
            tasks: vec![],
            cycle_id: None,
        };
        let candidate_context = serde_json::to_string(&checkpoint).unwrap();
        let archive = ContextArchiveV4 {
            archive_id: Uuid::new_v4(),
            through_sequence: source.source_through_sequence,
            size_bytes: transcript.len() as u64,
            sha256: hex::encode(Sha256::digest(transcript.as_bytes())),
        };
        let admission = ContextCompactionAdmissionV4 {
            configuration_hash: source.spec.model_configuration_hash.clone().unwrap(),
            prompt_sha256: "b".repeat(64),
            provider_request_bytes: Some(12345),
        };
        let kinds = [
            AgentEventKindV4::ContextCompactionStarted {
                request_id: source.request.request_id,
                source_through_sequence: source.source_through_sequence,
                source_head_hash: source.source_head_hash.clone(),
                frozen_spec_hash: source.spec.spec_hash.clone(),
            },
            AgentEventKindV4::ContextArchived {
                archive: archive.clone(),
            },
            AgentEventKindV4::ContextCheckpointed {
                checkpoint: checkpoint.clone(),
            },
            AgentEventKindV4::ContextCompactionCompleted {
                request_id: source.request.request_id,
                archive: archive.clone(),
                checkpoint_through_sequence: checkpoint.through_sequence,
                checkpoint_sha256: hex::encode(Sha256::digest(
                    serde_json::to_vec(&checkpoint).unwrap(),
                )),
                before_bytes: transcript.len() as u64,
                after_bytes: candidate_context.len() as u64,
                admission: Some(admission.clone()),
            },
        ];
        let mut previous = source.events.last().unwrap().clone();
        let planned_events = kinds
            .into_iter()
            .map(|kind| {
                let event = AgentEventV4::next(&previous, Utc::now(), kind);
                previous = event.clone();
                event
            })
            .collect();
        let result = PreparedContextCompactionV4 {
            source_bindings_sha256: source.source_bindings_sha256.clone(),
            source_run_sha256: source.source_run_sha256.clone(),
            request: source.request.clone(),
            source_through_sequence: source.source_through_sequence,
            source_head_hash: source.source_head_hash.clone(),
            frozen_spec_hash: source.spec.spec_hash.clone().unwrap(),
            scientific_revision: source.scientific_revision,
            scientific_sha256: source.scientific_sha256.clone(),
            before_bytes: transcript.len() as u64,
            after_bytes: candidate_context.len() as u64,
            transcript,
            candidate_context,
            checkpoint: Some(checkpoint),
            archive: Some(archive),
            planned_events,
            status: ContextCompactionStatusV4::Completed,
            admission,
        };
        result.validate_structure().unwrap();
        result
    }

    async fn counts(store: &Store) -> (i64, i64, i64) {
        sqlx::query_as("SELECT (SELECT count(*) FROM agent_context_compactions_v4),(SELECT count(*) FROM agent_context_archives_v4),(SELECT count(*) FROM agent_events_v4)").fetch_one(&store.pool).await.unwrap()
    }

    #[tokio::test]
    async fn manual_compaction_commit_is_atomic_and_idempotent() {
        let (store, source) = fixture(Store::open_in_memory().await.unwrap()).await;
        let prepared = prepared(&source);
        let before = counts(&store).await;
        for (table, condition) in [
            ("agent_context_archives_v4", "1".to_owned()),
            ("agent_context_compactions_v4", "1".to_owned()),
        ]
        .into_iter()
        .chain((1..=4).map(|offset| {
            (
                "agent_events_v4",
                format!("NEW.sequence={}", source.source_through_sequence + offset),
            )
        })) {
            sqlx::query(&format!("CREATE TEMP TRIGGER compaction_failure BEFORE INSERT ON {table} WHEN {condition} BEGIN SELECT RAISE(ABORT,'injected compaction failure'); END")).execute(&store.pool).await.unwrap();
            let error = store
                .commit_context_compaction_v4(&source.request, &prepared)
                .await
                .unwrap_err();
            assert!(
                error.to_string().contains("injected compaction failure"),
                "unexpected failure: {error}"
            );
            assert_eq!(counts(&store).await, before);
            sqlx::query("DROP TRIGGER compaction_failure")
                .execute(&store.pool)
                .await
                .unwrap();
        }
        let result = store
            .commit_context_compaction_v4(&source.request, &prepared)
            .await;
        assert!(
            result.is_ok(),
            "valid prepared transaction must commit: {result:?}"
        );
        let first = result.unwrap();
        let (left, right) = tokio::join!(
            store.commit_context_compaction_v4(&source.request, &prepared),
            store.commit_context_compaction_v4(&source.request, &prepared)
        );
        assert_eq!(left.unwrap(), first);
        assert_eq!(right.unwrap(), first);
        assert_eq!(
            first,
            store
                .commit_context_compaction_v4(&source.request, &prepared)
                .await
                .unwrap()
        );
        assert_eq!(
            Some(first.clone()),
            store
                .context_compaction_receipt_v4(&source.request)
                .await
                .unwrap()
        );
        let mut other = source.request.clone();
        other.conversation_id = Uuid::new_v4();
        assert!(
            store
                .commit_context_compaction_v4(&other, &prepared)
                .await
                .is_err()
        );
        assert!(store.context_compaction_receipt_v4(&other).await.is_err());
        assert_eq!(counts(&store).await, (1, 1, before.2 + 4));
        let events = store.agent_events_v4(source.request.run_id).await.unwrap();
        assert_eq!(
            &events[source.events.len()..],
            prepared.planned_events.as_slice()
        );
    }

    #[tokio::test]
    async fn manual_compaction_commit_rechecks_all_pause_inputs() {
        for change in [
            "local_root",
            "remote_root",
            "head",
            "science",
            "science_header",
            "profile",
            "delegated_profile",
            "reviewer_profile",
            "spec",
            "guidance",
            "answer",
            "stop",
            "approval",
            "dispatch",
            "unknown",
            "status",
            "execution_kind",
        ] {
            let (store, source) = fixture(Store::open_in_memory().await.unwrap()).await;
            let result = store.context_compaction_snapshot_v4(&source.request).await;
            assert!(
                result.is_ok(),
                "read-only paused snapshot must exist: {result:?}"
            );
            assert_eq!(result.unwrap().context, source);
            let prepared = prepared(&source);
            let mut event = None;
            match change {
                "local_root" => {
                    sqlx::query("UPDATE project_omicsops SET local_root='C:\\changed'")
                        .execute(&store.pool)
                        .await
                        .unwrap();
                }
                "remote_root" => {
                    sqlx::query("UPDATE project_omicsops SET remote_root='/changed'")
                        .execute(&store.pool)
                        .await
                        .unwrap();
                }
                "head" => {
                    event = Some(AgentEventKindV4::ModelText {
                        text: "changed".into(),
                    })
                }
                "science" | "science_header" => {
                    let mut state = ScientificStateV4::new(source.request.project_id);
                    state.revision = 1;
                    store.save_scientific_state_v4(&state).await.unwrap();
                    if change == "science_header" {
                        sqlx::query("UPDATE scientific_states_v4 SET state_sha256='bad'")
                            .execute(&store.pool)
                            .await
                            .unwrap();
                    }
                }
                "profile" => {
                    sqlx::query("UPDATE model_profiles SET value_json=json_set(value_json,'$.model','changed')").execute(&store.pool).await.unwrap();
                }
                "delegated_profile" | "reviewer_profile" => {
                    let id = if change == "delegated_profile" {
                        source.spec.delegated_model.as_ref().unwrap().profile_id
                    } else {
                        source.spec.reviewer_model.as_ref().unwrap().profile_id
                    };
                    sqlx::query("UPDATE model_profiles SET value_json=json_set(value_json,'$.model','changed') WHERE id=?1")
                        .bind(id.to_string()).execute(&store.pool).await.unwrap();
                }
                "spec" | "execution_kind" => {
                    let mut spec = source.spec.clone();
                    if change == "spec" {
                        spec.plan.objective = "changed".into();
                    } else {
                        spec.execution_kind = RunExecutionKindV4::ApprovedPlan;
                    }
                    spec.spec_hash = Some(spec.calculate_spec_hash().unwrap());
                    sqlx::query("UPDATE agent_runs_v4 SET value_json=?1")
                        .bind(json!({"status":"waiting_for_input","spec":spec}).to_string())
                        .execute(&store.pool)
                        .await
                        .unwrap();
                }
                "guidance" => {
                    sqlx::query("INSERT INTO agent_guidance_v4(message_id,run_id,ordinal,markdown,accepted_at) VALUES(?1,?2,1,'pending',0)").bind(Uuid::new_v4().to_string()).bind(source.request.run_id.to_string()).execute(&store.pool).await.unwrap();
                }
                "answer" => {
                    event = Some(AgentEventKindV4::UserInputAnswered {
                        question_id: "q".into(),
                        answer: "yes".into(),
                    })
                }
                "stop" => {
                    store
                        .request_run_stop_v4(&omicsops_dto::StopRunRequestV4 {
                            request_id: Uuid::new_v4(),
                            project_id: source.request.project_id,
                            conversation_id: source.request.conversation_id,
                            run_id: source.request.run_id,
                        })
                        .await
                        .unwrap();
                }
                "approval" => {
                    event = Some(AgentEventKindV4::ToolApprovalRequested {
                        request: ToolApprovalRequestV4 {
                            approval_id: "a".into(),
                            call: ToolCallV4 {
                                call_id: "c".into(),
                                tool_id: "runtime.execute".into(),
                                arguments: json!({}),
                            },
                            effect: ToolEffectV4::Runtime,
                            reason: "approval".into(),
                            call_hash: "hash".into(),
                            scope_hash: None,
                            mode: RunModeV4::Execute,
                        },
                    })
                }
                "dispatch" => {
                    event = Some(AgentEventKindV4::ToolDispatchStarted {
                        call_id: "c".into(),
                        tool_id: "runtime.execute".into(),
                        effect: ToolEffectV4::Runtime,
                        idempotency_key: "c".into(),
                    })
                }
                "unknown" => {
                    event = Some(AgentEventKindV4::ToolDispatchUncertain {
                        call_id: "c".into(),
                        tool_id: "runtime.execute".into(),
                    })
                }
                "status" => {
                    sqlx::query("UPDATE agent_runs_v4 SET status='running'")
                        .execute(&store.pool)
                        .await
                        .unwrap();
                }
                _ => unreachable!(),
            }
            if let Some(kind) = event {
                store
                    .append_agent_event_v4(&AgentEventV4::next(
                        source.events.last().unwrap(),
                        Utc::now(),
                        kind,
                    ))
                    .await
                    .unwrap();
            }
            let before = counts(&store).await;
            assert!(
                store
                    .commit_context_compaction_v4(&source.request, &prepared)
                    .await
                    .is_err(),
                "accepted changed {change}"
            );
            assert_eq!(counts(&store).await, before, "wrote on changed {change}");
            if matches!(
                change,
                "guidance"
                    | "answer"
                    | "stop"
                    | "approval"
                    | "dispatch"
                    | "unknown"
                    | "status"
                    | "execution_kind"
            ) {
                assert!(
                    store
                        .context_compaction_snapshot_v4(&source.request)
                        .await
                        .is_err(),
                    "snapshot accepted {change}"
                );
            }
        }
    }

    #[tokio::test]
    async fn manual_compaction_restart_preserves_refs_and_admission() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("manual.sqlite");
        let (store, source) = fixture(Store::open(&path).await.unwrap()).await;
        let prepared = prepared(&source);
        let result = store
            .commit_context_compaction_v4(&source.request, &prepared)
            .await;
        assert!(result.is_ok(), "commit before reopen: {result:?}");
        let first = result.unwrap();
        store.pool.close().await;
        drop(store);
        let store = Store::open(&path).await.unwrap();
        let restored = store
            .context_compaction_receipt_v4(&source.request)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(restored, first);
        assert_eq!(restored.provider_request_bytes, Some(12345));
        assert_eq!(
            restored.admission_configuration_hash,
            Some(prepared.admission.configuration_hash.clone())
        );
        assert_eq!(
            store
                .latest_context_compaction_v4(
                    source.request.project_id,
                    source.request.conversation_id
                )
                .await
                .unwrap(),
            Some(first)
        );
        let (transcript, checkpoint) = store
            .agent_context_archive_v4(restored.archive.unwrap().archive_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(transcript, prepared.transcript);
        assert_eq!(Some(checkpoint.clone()), prepared.checkpoint);
        assert!(!checkpoint.scientific_state.is_null());
        assert_eq!(
            &store.agent_events_v4(source.request.run_id).await.unwrap()[source.events.len()..],
            prepared.planned_events.as_slice()
        );
        store.pool.close().await;
    }

    #[tokio::test]
    async fn manual_compaction_has_no_schema_change() {
        let (store, source) = fixture(Store::open_in_memory().await.unwrap()).await;
        let schema = sqlx::query_scalar::<_, String>(
            "SELECT sql FROM sqlite_master WHERE sql IS NOT NULL ORDER BY name",
        )
        .fetch_all(&store.pool)
        .await
        .unwrap();
        let before = counts(&store).await;
        let result = store.context_compaction_snapshot_v4(&source.request).await;
        assert!(result.is_ok(), "snapshot must be available: {result:?}");
        assert_eq!(counts(&store).await, before);
        let science_count: i64 = sqlx::query_scalar("SELECT count(*) FROM scientific_states_v4")
            .fetch_one(&store.pool)
            .await
            .unwrap();
        assert_eq!(science_count, 0);
        store
            .commit_context_compaction_v4(&source.request, &prepared(&source))
            .await
            .unwrap();
        assert_eq!(
            schema,
            sqlx::query_scalar::<_, String>(
                "SELECT sql FROM sqlite_master WHERE sql IS NOT NULL ORDER BY name"
            )
            .fetch_all(&store.pool)
            .await
            .unwrap()
        );
    }
    #[tokio::test]
    async fn manual_compaction_rejects_corrupt_prepared_and_keeps_originals() {
        let (store, source) = fixture(Store::open_in_memory().await.unwrap()).await;
        let original = prepared(&source);
        let before = counts(&store).await;
        for change in 0..9 {
            let mut bad = original.clone();
            match change {
                0 => bad.transcript.push(' '),
                1 => bad.source_head_hash = "c".repeat(64),
                2 => bad.planned_events[1].sequence += 1,
                3 => bad.planned_events[2].event_hash = "c".repeat(64),
                4 => bad.archive.as_mut().unwrap().sha256 = "c".repeat(64),
                5 => bad.request.project_id = Uuid::new_v4(),
                6 => bad.admission.prompt_sha256 = "unbounded".into(),
                7 => bad.source_bindings_sha256 = "c".repeat(64),
                _ => {
                    bad.checkpoint.as_mut().unwrap().scientific_state = Value::Null;
                    let cp = bad.checkpoint.clone().unwrap();
                    bad.planned_events[2].event = AgentEventKindV4::ContextCheckpointed {
                        checkpoint: cp.clone(),
                    };
                    if let AgentEventKindV4::ContextCompactionCompleted {
                        checkpoint_sha256, ..
                    } = &mut bad.planned_events[3].event
                    {
                        *checkpoint_sha256 =
                            hex::encode(Sha256::digest(serde_json::to_vec(&cp).unwrap()));
                    }
                    let mut previous = source.events.last().unwrap().clone();
                    for event in &mut bad.planned_events {
                        *event =
                            AgentEventV4::next(&previous, event.occurred_at, event.event.clone());
                        previous = event.clone();
                    }
                    assert!(
                        bad.validate_structure().is_ok(),
                        "rehashed candidate exercises Store science check"
                    );
                }
            }
            assert!(
                store
                    .commit_context_compaction_v4(&source.request, &bad)
                    .await
                    .is_err(),
                "accepted corruption {change}"
            );
            assert_eq!(counts(&store).await, before);
        }
    }

    #[tokio::test]
    async fn manual_compaction_hydrates_only_verified_bounded_admission() {
        for change in ["hash", "config", "prompt", "legacy"] {
            let (store, source) = fixture(Store::open_in_memory().await.unwrap()).await;
            let prepared = prepared(&source);
            store
                .commit_context_compaction_v4(&source.request, &prepared)
                .await
                .unwrap();
            let mut event = prepared.planned_events.last().unwrap().clone();
            if let AgentEventKindV4::ContextCompactionCompleted { admission, .. } = &mut event.event
            {
                match change {
                    "config" => admission.as_mut().unwrap().configuration_hash = "g".repeat(64),
                    "prompt" => admission.as_mut().unwrap().prompt_sha256 = "b".repeat(65),
                    "legacy" => *admission = None,
                    _ => {}
                }
            }
            event = AgentEventV4::next(&prepared.planned_events[2], event.occurred_at, event.event);
            if change == "hash" {
                event.event_hash = "a".repeat(64);
            }
            sqlx::query("UPDATE agent_events_v4 SET value_json=?1,event_hash=?2 WHERE run_id=?3 AND sequence=?4").bind(serde_json::to_string(&event).unwrap()).bind(&event.event_hash).bind(source.request.run_id.to_string()).bind(event.sequence as i64).execute(&store.pool).await.unwrap();
            let result = store.context_compaction_receipt_v4(&source.request).await;
            if change == "legacy" {
                let receipt = result.unwrap().unwrap();
                assert_eq!(receipt.provider_request_bytes, None);
                assert_eq!(receipt.admission_configuration_hash, None);
            } else {
                assert!(result.is_err(), "accepted {change}");
            }
        }
    }

    #[tokio::test]
    async fn manual_compaction_not_needed_and_legacy_started_are_request_specific() {
        let (store, source) = fixture(Store::open_in_memory().await.unwrap()).await;
        let mut prepared = prepared(&source);
        prepared.status = ContextCompactionStatusV4::NotNeeded;
        prepared.archive = None;
        prepared.checkpoint = None;
        prepared.planned_events = vec![AgentEventV4::next(
            source.events.last().unwrap(),
            Utc::now(),
            AgentEventKindV4::ContextCompactionNotNeeded {
                request_id: source.request.request_id,
                before_bytes: prepared.before_bytes,
            },
        )];
        let receipt = store
            .commit_context_compaction_v4(&source.request, &prepared)
            .await
            .unwrap();
        assert_eq!(receipt.provider_request_bytes, None);
        assert_eq!(receipt.admission_configuration_hash, None);
        assert_eq!(
            store
                .context_compaction_receipt_v4(&source.request)
                .await
                .unwrap(),
            Some(receipt.clone())
        );
        let old_id = Uuid::new_v4();
        let mut old = receipt.clone();
        old.request_id = old_id;
        old.after_bytes = None;
        let mut tx = store.pool.begin().await.unwrap();
        insert_receipt_in_tx(&mut tx, &old, STARTED, None, None, None, None, None)
            .await
            .unwrap();
        tx.commit().await.unwrap();
        // This deliberately newer receipt must not be returned for old_id.
        sqlx::query(
            "UPDATE agent_context_compactions_v4 SET updated_at=updated_at+100 WHERE request_id=?1",
        )
        .bind(source.request.request_id.to_string())
        .execute(&store.pool)
        .await
        .unwrap();
        let mut old_request = source.request.clone();
        old_request.request_id = old_id;
        let recovered = store
            .context_compaction_receipt_v4(&old_request)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(recovered.request_id, old_id);
        assert_eq!(recovered.status, ContextCompactionStatusV4::Attention);
        assert_eq!(recovered.provider_request_bytes, None);
        assert_eq!(counts(&store).await.1, 0);
        assert_eq!(
            store
                .context_compaction_receipt_v4(&old_request)
                .await
                .unwrap(),
            Some(recovered)
        );
    }

    #[tokio::test]
    async fn manual_compaction_connection_fingerprint_and_project_fallback() {
        use omicsops_core::domain::{AuthenticationMethod, ConnectionProfile};
        for change in ["host", "trust", "port", "user", "connection", "fallback"] {
            let (store, source) = fixture(Store::open_in_memory().await.unwrap()).await;
            let mut profile = ConnectionProfile {
                id: Uuid::new_v4(),
                label: "fixture".into(),
                host: "fixture.invalid".into(),
                port: 22,
                username: "fixture".into(),
                authentication: AuthenticationMethod::Password,
                authentication_reference: "fixture-reference".into(),
                host_key_fingerprint: Some("SHA256:fixture".into()),
            };
            store.save_connection(&profile).await.unwrap();
            sqlx::query("UPDATE project_omicsops SET connection_id=?1")
                .bind(profile.id.to_string())
                .execute(&store.pool)
                .await
                .unwrap();
            if change == "fallback" {
                sqlx::query("DELETE FROM project_omicsops")
                    .execute(&store.pool)
                    .await
                    .unwrap();
            }
            let snapshot = store
                .context_compaction_snapshot_v4(&source.request)
                .await
                .unwrap();
            let prepared = prepared(&snapshot.context);
            match change {
                "host" => profile.host = "changed.invalid".into(),
                "trust" => profile.host_key_fingerprint = Some("SHA256:changed".into()),
                "port" => profile.port = 2222,
                "user" => profile.username = "changed".into(),
                "connection" => {
                    sqlx::query("UPDATE project_omicsops SET connection_id=NULL")
                        .execute(&store.pool)
                        .await
                        .unwrap();
                }
                "fallback" => {
                    sqlx::query("UPDATE projects SET workspace_dir='C:\\changed'")
                        .execute(&store.pool)
                        .await
                        .unwrap();
                }
                _ => unreachable!(),
            }
            store.save_connection(&profile).await.unwrap();
            let before = counts(&store).await;
            assert!(
                store
                    .commit_context_compaction_v4(&source.request, &prepared)
                    .await
                    .is_err(),
                "accepted {change}"
            );
            assert_eq!(counts(&store).await, before);
        }
    }
    #[tokio::test]
    async fn manual_compaction_rechecks_raw_run_record() {
        for field in [
            "objective",
            "reference_context",
            "input_images",
            "future_field",
        ] {
            let (store, source) = fixture(Store::open_in_memory().await.unwrap()).await;
            let captured = store
                .context_compaction_snapshot_v4(&source.request)
                .await
                .unwrap();
            let prepared = prepared(&captured.context);
            let mut changed = captured.run_value;
            changed[field] = if field == "input_images" {
                json!([{ "file_id": "changed" }])
            } else {
                json!("changed")
            };
            store
                .save_agent_run_v4(
                    source.request.run_id,
                    source.request.project_id,
                    source.request.conversation_id,
                    "waiting_for_input",
                    &changed,
                )
                .await
                .unwrap();
            let before = counts(&store).await;
            assert!(
                store
                    .commit_context_compaction_v4(&source.request, &prepared)
                    .await
                    .is_err(),
                "accepted changed record field {field}"
            );
            assert_eq!(counts(&store).await, before);
        }
    }
    #[tokio::test]
    async fn manual_compaction_reads_persisted_science_headers_without_reconciliation() {
        let (store, source) = fixture(Store::open_in_memory().await.unwrap()).await;
        let mut science = ScientificStateV4::new(source.request.project_id);
        science.revision = 7;
        store.save_scientific_state_v4(&science).await.unwrap();
        let before = counts(&store).await;
        let snapshot = store
            .context_compaction_snapshot_v4(&source.request)
            .await
            .unwrap();
        assert_eq!(snapshot.context.scientific_revision, 7);
        assert_eq!(snapshot.context.scientific_sha256, science.digest());
        assert_eq!(
            snapshot.context.scientific_state,
            serde_json::to_value(&science).unwrap()
        );
        assert_eq!(counts(&store).await, before);
        sqlx::query("UPDATE scientific_states_v4 SET revision=8")
            .execute(&store.pool)
            .await
            .unwrap();
        assert!(
            store
                .context_compaction_snapshot_v4(&source.request)
                .await
                .is_err()
        );
        assert_eq!(counts(&store).await, before);
        sqlx::query("UPDATE scientific_states_v4 SET revision=7")
            .execute(&store.pool)
            .await
            .unwrap();
        let prepared = prepared(&snapshot.context);
        let receipt = store
            .commit_context_compaction_v4(&source.request, &prepared)
            .await
            .unwrap();
        let (_, checkpoint) = store
            .agent_context_archive_v4(receipt.archive.unwrap().archive_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            checkpoint.scientific_state,
            snapshot.context.scientific_state
        );
        assert_eq!(
            store
                .scientific_state_v4(source.request.project_id)
                .await
                .unwrap(),
            Some(science)
        );
    }
    #[tokio::test]
    async fn manual_compaction_legacy_completed_recovery_replays_identical_receipt() {
        let (store, source) = fixture(Store::open_in_memory().await.unwrap()).await;
        store
            .commit_context_compaction_v4(&source.request, &prepared(&source))
            .await
            .unwrap();
        sqlx::query("UPDATE agent_context_compactions_v4 SET status='started'")
            .execute(&store.pool)
            .await
            .unwrap();
        let restored = store
            .context_compaction_receipt_v4(&source.request)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(restored.status, ContextCompactionStatusV4::Completed);
        assert_eq!(restored.provider_request_bytes, Some(12345));
        assert_eq!(
            Some(restored),
            store
                .context_compaction_receipt_v4(&source.request)
                .await
                .unwrap()
        );
    }
}
