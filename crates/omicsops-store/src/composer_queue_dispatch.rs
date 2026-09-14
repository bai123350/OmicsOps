//! Fenced queue preparation and atomic message/run/event dispatch.
use crate::composer_queue::{QUEUE_COLUMNS, queue_item_from_row, queue_row_by_request};
use crate::{Store, StoreError};
use chrono::{DateTime, Utc};
use omicsops_core::workspace::{Message, MessageRole};
use omicsops_dto::{
    ComposerQueueFailureCodeV4, ComposerQueueItemV4, ComposerQueueMaterialSnapshotV4,
    ComposerQueueModeV4,
};
use omicsops_protocol::{AgentEventKindV4, AgentEventV4, RunExecutionKindV4, RunModeV4, RunSpecV4};
use serde_json::Value;
use sqlx::{Row, SqliteConnection};
use uuid::Uuid;

/// Host-only claim authority. It is never serialized to the browser.
#[derive(Debug, Clone)]
pub struct ComposerQueueDispatchLeaseV4 {
    pub item: ComposerQueueItemV4,
    pub material: ComposerQueueMaterialSnapshotV4,
    pub lease_token: Uuid,
    pub expires_at: DateTime<Utc>,
}

fn invalid(message: &str) -> StoreError {
    StoreError::InvalidInput(message.into())
}

impl Store {
    pub async fn claim_next_composer_queue(
        &self,
        project_id: Uuid,
        conversation_id: Uuid,
        now: DateTime<Utc>,
    ) -> Result<Option<ComposerQueueDispatchLeaseV4>, StoreError> {
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        crate::ensure_conversation_unlocked_executor(&mut tx, project_id, conversation_id).await?;
        let active: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM agent_runs_v4 WHERE project_id=?1 AND conversation_id=?2 AND status IN ('planning','running','waiting_for_input','waiting_for_approval','awaiting_approval','needs_attention')")
            .bind(project_id.to_string()).bind(conversation_id.to_string()).fetch_one(&mut *tx).await?;
        let claimed: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM composer_queue_v4 WHERE project_id=?1 AND conversation_id=?2 AND status IN ('dispatching','running','uncertain')")
            .bind(project_id.to_string()).bind(conversation_id.to_string()).fetch_one(&mut *tx).await?;
        if active > 0 || claimed > 0 {
            tx.commit().await?;
            return Ok(None);
        }
        let row = sqlx::query(&format!("SELECT {QUEUE_COLUMNS} FROM composer_queue_v4 WHERE project_id=?1 AND conversation_id=?2 AND status='pending' ORDER BY position LIMIT 1"))
            .bind(project_id.to_string()).bind(conversation_id.to_string()).fetch_optional(&mut *tx).await?;
        let Some(row) = row else {
            tx.commit().await?;
            return Ok(None);
        };
        let material: ComposerQueueMaterialSnapshotV4 =
            serde_json::from_str(&row.try_get::<String, _>(14)?)?;
        let item = queue_item_from_row(row)?;
        crate::composer_replacement::ensure_replacement_settled_in_tx(&mut tx, item.request_id)
            .await?;
        let revision = i64::try_from(item.revision)
            .ok()
            .and_then(|value| value.checked_add(1))
            .ok_or_else(|| invalid("queue revision exceeds SQLite range"))?;
        let lease_token = Uuid::new_v4();
        let expires_at = now
            .checked_add_signed(chrono::Duration::seconds(120))
            .ok_or_else(|| invalid("queue lease timestamp overflow"))?;
        let changed = sqlx::query("UPDATE composer_queue_v4 SET status='dispatching',revision=?1,lease_owner=?2,lease_expires_at=?3,dispatch_attempt=dispatch_attempt+1,updated_at=?4 WHERE request_id=?5 AND revision=?6 AND status='pending' AND dispatch_attempt < 2147483647")
            .bind(revision)
            .bind(lease_token.to_string())
            .bind(crate::timestamp(expires_at))
            .bind(crate::timestamp(now))
            .bind(item.request_id.to_string())
            .bind(i64::try_from(item.revision).map_err(|_| invalid("invalid queue revision"))?)
            .execute(&mut *tx)
            .await?
            .rows_affected();
        if changed != 1 {
            return Err(invalid(
                "queue claim changed or exhausted its attempt counter",
            ));
        }
        let item = queue_item_from_row(
            queue_row_by_request(&mut tx, item.request_id)
                .await?
                .ok_or_else(|| invalid("queue claim disappeared"))?,
        )?;
        tx.commit().await?;
        Ok(Some(ComposerQueueDispatchLeaseV4 {
            item,
            material,
            lease_token,
            expires_at,
        }))
    }

    /// No provider/tool call is allowed before this transaction commits.
    /// A failed insert rolls back the message, title, run, plan seed and events.
    pub async fn commit_composer_queue_dispatch(
        &self,
        lease: &ComposerQueueDispatchLeaseV4,
        value: &Value,
        first_events: &[AgentEventV4],
        now: DateTime<Utc>,
    ) -> Result<ComposerQueueItemV4, StoreError> {
        let item = &lease.item;
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        crate::ensure_conversation_unlocked_executor(
            &mut tx,
            item.project_id,
            item.conversation_id,
        )
        .await?;
        ensure_lease(&mut tx, lease, now).await?;
        crate::composer_replacement::ensure_replacement_settled_in_tx(&mut tx, item.request_id)
            .await?;
        let row = queue_row_by_request(&mut tx, item.request_id)
            .await?
            .ok_or_else(|| invalid("queue claim disappeared"))?;
        let material: ComposerQueueMaterialSnapshotV4 =
            serde_json::from_str(&row.try_get::<String, _>(14)?)?;
        let item = queue_item_from_row(row)?;
        validate_prepared_run(&item, &material, value, first_events)?;
        crate::composer_queue::validate_frozen_snapshot_current(
            &mut tx,
            item.project_id,
            item.conversation_id,
            &item.frozen,
        )
        .await?;
        let active: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM agent_runs_v4 WHERE project_id=?1 AND conversation_id=?2 AND status IN ('planning','running','waiting_for_input','waiting_for_approval','awaiting_approval','needs_attention')")
            .bind(item.project_id.to_string()).bind(item.conversation_id.to_string()).fetch_one(&mut *tx).await?;
        if active > 0 {
            return Err(invalid("conversation became busy before queue dispatch"));
        }
        ensure_no_dispatch_side_effects(&mut tx, &item).await?;
        let next: i64 =
            sqlx::query_scalar("SELECT COALESCE(MAX(seq),0) FROM messages WHERE frame_id=?1")
                .bind(item.conversation_id.to_string())
                .fetch_one(&mut *tx)
                .await?;
        let sequence = next
            .checked_add(1)
            .and_then(|value| u64::try_from(value).ok())
            .ok_or_else(|| invalid("message sequence exceeds SQLite range"))?;
        let message = Message::markdown(
            item.message_id,
            item.project_id,
            item.conversation_id,
            sequence,
            MessageRole::User,
            item.message_markdown.trim(),
            now,
        );
        let title = item
            .message_markdown
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        crate::save_message_with_first_title_in_tx(&mut tx, &message, &title).await?;
        match item.mode {
            ComposerQueueModeV4::Agent => {
                sqlx::query("INSERT INTO agent_runs_v4(run_id,project_id,conversation_id,status,value_json) VALUES (?1,?2,?3,'running',?4)")
                    .bind(item.run_id.to_string()).bind(item.project_id.to_string()).bind(item.conversation_id.to_string()).bind(serde_json::to_string(value)?).execute(&mut *tx).await?;
            }
            ComposerQueueModeV4::Plan => {
                crate::start_plan_run_in_tx_v4(
                    &mut tx,
                    item.run_id,
                    item.project_id,
                    item.conversation_id,
                    "planning",
                    value,
                    &item.message_markdown,
                    now,
                )
                .await?;
            }
        }
        for event in first_events {
            crate::insert_agent_event_in_tx(&mut tx, event).await?;
        }
        let revision = i64::try_from(item.revision)
            .ok()
            .and_then(|value| value.checked_add(1))
            .ok_or_else(|| invalid("queue revision exceeds SQLite range"))?;
        let changed = sqlx::query("UPDATE composer_queue_v4 SET status='running',revision=?1,lease_owner=NULL,lease_expires_at=NULL,updated_at=?2 WHERE request_id=?3 AND revision=?4 AND status='dispatching' AND lease_owner=?5")
            .bind(revision)
            .bind(crate::timestamp(now))
            .bind(item.request_id.to_string())
            .bind(i64::try_from(item.revision).map_err(|_| invalid("invalid queue revision"))?)
            .bind(lease.lease_token.to_string())
            .execute(&mut *tx)
            .await?
            .rows_affected();
        if changed != 1 {
            return Err(invalid("queue dispatch lost its claim"));
        }
        let updated = queue_item_from_row(
            queue_row_by_request(&mut tx, item.request_id)
                .await?
                .ok_or_else(|| invalid("queue dispatch disappeared"))?,
        )?;
        tx.commit().await?;
        Ok(updated)
    }

    /// Release a preparation lease before the atomic start transaction has
    /// crossed its side-effect boundary.  This is deliberately fenced by the
    /// durable lease token and refuses to requeue a claim once its reserved
    /// message, run, plan, or event exists.
    pub async fn release_preparation(
        &self,
        lease: &ComposerQueueDispatchLeaseV4,
        now: DateTime<Utc>,
        failure: Option<ComposerQueueFailureCodeV4>,
    ) -> Result<ComposerQueueItemV4, StoreError> {
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        ensure_lease(&mut tx, lease, now).await?;
        let row = queue_row_by_request(&mut tx, lease.item.request_id)
            .await?
            .ok_or_else(|| invalid("queue claim no longer exists"))?;
        let item = queue_item_from_row(row)?;
        ensure_no_dispatch_side_effects(&mut tx, &item).await?;

        let revision = next_dispatch_revision(item.revision)?;
        let status = if failure.is_some() {
            "failed"
        } else {
            "pending"
        };
        let failure_code = failure.as_ref().map(crate::enum_string).transpose()?;
        let changed = sqlx::query(
            "UPDATE composer_queue_v4
             SET status=?1,revision=?2,lease_owner=NULL,lease_expires_at=NULL,
                 failure_code=?3, failure_message=NULL, updated_at=?4
             WHERE request_id=?5 AND project_id=?6 AND conversation_id=?7
               AND revision=?8 AND status='dispatching' AND lease_owner=?9",
        )
        .bind(status)
        .bind(revision)
        .bind(failure_code)
        .bind(crate::timestamp(now))
        .bind(item.request_id.to_string())
        .bind(item.project_id.to_string())
        .bind(item.conversation_id.to_string())
        .bind(i64::try_from(item.revision).map_err(|_| invalid("invalid queue revision"))?)
        .bind(lease.lease_token.to_string())
        .execute(&mut *tx)
        .await?
        .rows_affected();
        if changed != 1 {
            return Err(invalid("queue preparation lease changed or was revoked"));
        }
        let updated = queue_item_from_row(
            queue_row_by_request(&mut tx, item.request_id)
                .await?
                .ok_or_else(|| invalid("queue preparation disappeared after release"))?,
        )?;
        tx.commit().await?;
        Ok(updated)
    }

    /// Descriptive alias used by host dispatchers; both names are kept on the
    /// Store so the lease release operation remains explicit at call sites.
    pub async fn release_composer_queue_preparation(
        &self,
        lease: &ComposerQueueDispatchLeaseV4,
        now: DateTime<Utc>,
        failure: Option<ComposerQueueFailureCodeV4>,
    ) -> Result<ComposerQueueItemV4, StoreError> {
        self.release_preparation(lease, now, failure).await
    }

    /// Reclaim an expired preparation only after proving that none of its
    /// reserved durable side effects exists. The old fencing token is revoked.
    pub async fn reconcile_composer_queue(
        &self,
        project_id: Uuid,
        conversation_id: Uuid,
        now: DateTime<Utc>,
    ) -> Result<Vec<ComposerQueueItemV4>, StoreError> {
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        crate::ensure_conversation_owner_executor(&mut tx, project_id, conversation_id).await?;
        let rows = sqlx::query("SELECT request_id,message_id,run_id,status,lease_expires_at,revision,lease_owner FROM composer_queue_v4 WHERE project_id=?1 AND conversation_id=?2 AND status IN ('dispatching','running','uncertain')")
            .bind(project_id.to_string()).bind(conversation_id.to_string()).fetch_all(&mut *tx).await?;
        for row in rows {
            let request_id: String = row.try_get(0)?;
            let message_id: String = row.try_get(1)?;
            let run_id = crate::parse_uuid(row.try_get::<String, _>(2)?, "queued run id")?;
            let status: String = row.try_get(3)?;
            let expiry: Option<i64> = row.try_get(4)?;
            let lease_owner: Option<String> = row.try_get(6)?;
            if status == "dispatching"
                && expiry.is_some_and(|expiry| expiry > crate::timestamp(now))
            {
                continue;
            }
            let run =
                sqlx::query("SELECT project_id,conversation_id FROM agent_runs_v4 WHERE run_id=?1")
                    .bind(run_id.to_string())
                    .fetch_optional(&mut *tx)
                    .await?;
            let message: Option<(String, String)> =
                sqlx::query_as("SELECT project_id,frame_id FROM messages WHERE id=?1")
                    .bind(&message_id)
                    .fetch_optional(&mut *tx)
                    .await?;
            let event_count: i64 =
                sqlx::query_scalar("SELECT COUNT(*) FROM agent_events_v4 WHERE run_id=?1")
                    .bind(run_id.to_string())
                    .fetch_one(&mut *tx)
                    .await?;
            let (events, events_valid) = match crate::load_agent_events_in_tx(&mut tx, run_id).await
            {
                Ok(events) => (events, true),
                Err(_) => (Vec::new(), false),
            };
            let events_scoped = events.iter().all(|event| {
                event.run_id == run_id
                    && event.project_id == project_id
                    && event.conversation_id == conversation_id
            });
            let next = if let (Some(run), Some((message_project, message_conversation))) =
                (&run, &message)
            {
                if run.try_get::<String, _>(0)? != project_id.to_string()
                    || run.try_get::<String, _>(1)? != conversation_id.to_string()
                    || message_project != &project_id.to_string()
                    || message_conversation != &conversation_id.to_string()
                    || !events_valid
                    || !events_scoped
                    || events.is_empty()
                {
                    "uncertain"
                } else {
                    terminal_queue_status(&events).unwrap_or("running")
                }
            } else if run.is_none()
                && message.is_none()
                && event_count == 0
                && events_valid
                && status == "dispatching"
            {
                "pending"
            } else {
                "uncertain"
            };
            let clear_stale_lease = lease_owner.is_some() || expiry.is_some();
            if next != status || clear_stale_lease {
                let revision: i64 = row.try_get(5)?;
                let revision = revision
                    .checked_add(1)
                    .ok_or_else(|| invalid("queue revision exceeds SQLite range"))?;
                let failure_code = queue_failure_code_for_status(next)
                    .map(|code| crate::enum_string(&code))
                    .transpose()?;
                sqlx::query("UPDATE composer_queue_v4 SET status=?1,revision=?2,lease_owner=NULL,lease_expires_at=NULL,failure_code=?3,updated_at=?4 WHERE request_id=?5")
                    .bind(next)
                    .bind(revision)
                    .bind(failure_code)
                    .bind(crate::timestamp(now))
                    .bind(&request_id)
                    .execute(&mut *tx)
                    .await?;
            }
        }
        let items =
            crate::composer_queue::queue_items_in_tx(&mut tx, project_id, conversation_id).await?;
        tx.commit().await?;
        Ok(items)
    }
}

fn next_dispatch_revision(current: u64) -> Result<i64, StoreError> {
    i64::try_from(current)
        .ok()
        .and_then(|value| value.checked_add(1))
        .ok_or_else(|| invalid("queue revision exceeds SQLite range"))
}

async fn ensure_no_dispatch_side_effects(
    tx: &mut SqliteConnection,
    item: &ComposerQueueItemV4,
) -> Result<(), StoreError> {
    let message_exists: i64 =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM messages WHERE id=?1)")
            .bind(item.message_id.to_string())
            .fetch_one(&mut *tx)
            .await?;
    let run_exists: i64 =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM agent_runs_v4 WHERE run_id=?1)")
            .bind(item.run_id.to_string())
            .fetch_one(&mut *tx)
            .await?;
    let event_exists: i64 =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM agent_events_v4 WHERE run_id=?1)")
            .bind(item.run_id.to_string())
            .fetch_one(&mut *tx)
            .await?;
    let plan_exists: i64 =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM proposed_plans WHERE run_id=?1)")
            .bind(item.run_id.to_string())
            .fetch_one(&mut *tx)
            .await?;
    if message_exists != 0 || run_exists != 0 || event_exists != 0 || plan_exists != 0 {
        return Err(invalid(
            "queue preparation crossed its durable side-effect boundary",
        ));
    }
    Ok(())
}

async fn ensure_lease(
    tx: &mut SqliteConnection,
    lease: &ComposerQueueDispatchLeaseV4,
    now: DateTime<Utc>,
) -> Result<(), StoreError> {
    let row = sqlx::query("SELECT project_id,conversation_id,status,revision,lease_owner,lease_expires_at FROM composer_queue_v4 WHERE request_id=?1")
        .bind(lease.item.request_id.to_string()).fetch_optional(&mut *tx).await?.ok_or_else(|| invalid("queue claim no longer exists"))?;
    if row.try_get::<String, _>(0)? != lease.item.project_id.to_string()
        || row.try_get::<String, _>(1)? != lease.item.conversation_id.to_string()
        || row.try_get::<String, _>(2)? != "dispatching"
        || row.try_get::<i64, _>(3)?
            != i64::try_from(lease.item.revision).map_err(|_| invalid("invalid queue revision"))?
        || row.try_get::<Option<String>, _>(4)?.as_deref()
            != Some(lease.lease_token.to_string().as_str())
        || row
            .try_get::<Option<i64>, _>(5)?
            .is_none_or(|expiry| expiry <= crate::timestamp(now))
    {
        return Err(invalid("queue preparation lease is stale"));
    }
    Ok(())
}

fn validate_prepared_run(
    item: &ComposerQueueItemV4,
    material: &ComposerQueueMaterialSnapshotV4,
    value: &Value,
    events: &[AgentEventV4],
) -> Result<(), StoreError> {
    for (field, expected) in [
        ("run_id", item.run_id),
        ("project_id", item.project_id),
        ("conversation_id", item.conversation_id),
        ("model_profile_id", item.frozen.model_profile_id),
    ] {
        if !crate::scoped_json_uuid_matches(value, field, expected) {
            return Err(invalid("prepared queue run has a mismatched identity"));
        }
    }
    if value.get("objective").and_then(Value::as_str) != Some(item.message_markdown.as_str())
        || value
            .get("reference_context")
            .and_then(Value::as_str)
            .unwrap_or("")
            != material.reference_context
    {
        return Err(invalid(
            "prepared queue content differs from its accepted snapshot",
        ));
    }
    for (field, expected) in [
        (
            "model_configuration_hash",
            serde_json::to_value(&item.frozen.model_configuration_hash)?,
        ),
        (
            "conversation_preferences",
            serde_json::to_value(item.frozen.conversation_preferences)?,
        ),
        (
            "service_tier",
            serde_json::to_value(item.frozen.service_tier)?,
        ),
        (
            "compute_selection",
            serde_json::to_value(&item.frozen.compute_selection)?,
        ),
        (
            "reviewer_model",
            serde_json::to_value(&item.frozen.reviewer_model)?,
        ),
    ] {
        if value.get(field).unwrap_or(&Value::Null) != &expected {
            return Err(invalid(
                "prepared queue settings differ from their accepted snapshot",
            ));
        }
    }
    let expected_mode = if item.mode == ComposerQueueModeV4::Agent {
        RunModeV4::Execute
    } else {
        RunModeV4::Plan
    };
    if events.first().is_none_or(|event| event.run_id != item.run_id || event.project_id != item.project_id || event.conversation_id != item.conversation_id || !matches!(event.event, AgentEventKindV4::RunCreated { mode } if mode == expected_mode)) { return Err(invalid("queued run must start with its owned RunCreated event")); }
    omicsops_protocol::validate_event_chain_v4(events)
        .map_err(|_| invalid("invalid initial queue event chain"))?;
    if item.mode == ComposerQueueModeV4::Agent {
        if value.get("status").and_then(Value::as_str) != Some("running") {
            return Err(invalid("invalid queued direct status"));
        }
        let spec: RunSpecV4 = serde_json::from_value(
            value
                .get("spec")
                .cloned()
                .ok_or_else(|| invalid("queued direct run has no frozen spec"))?,
        )?;
        spec.validate_integrity()
            .map_err(|_| invalid("invalid queued run spec"))?;
        if spec.run_id != item.run_id
            || spec.project_id != item.project_id
            || spec.conversation_id != item.conversation_id
            || spec.model_profile_id != item.frozen.model_profile_id
            || spec.execution_kind != RunExecutionKindV4::OrdinaryAgent
            || spec.model_configuration_hash.as_deref()
                != Some(&item.frozen.model_configuration_hash)
            || spec.conversation_preferences != Some(item.frozen.conversation_preferences)
            || spec.service_tier != Some(item.frozen.service_tier)
            || spec.reviewer_model != item.frozen.reviewer_model
            || spec.delegated_model != item.frozen.delegated_model
            || spec.compute_selection.as_ref() != Some(&item.frozen.compute_selection)
        {
            return Err(invalid(
                "queued spec is not the accepted frozen configuration",
            ));
        }
        if events.len() != 2
            || !matches!(&events[1].event, AgentEventKindV4::RunSpecFrozen { approval_hash, spec_hash } if Some(approval_hash) == spec.approval_hash.as_ref() && Some(spec_hash) == spec.spec_hash.as_ref())
        {
            return Err(invalid(
                "queued direct run needs its exact frozen spec event",
            ));
        }
    } else if events.len() != 1 {
        return Err(invalid("queued plan starts only with RunCreated"));
    }
    Ok(())
}

fn queue_failure_code_for_status(status: &str) -> Option<ComposerQueueFailureCodeV4> {
    match status {
        "failed" => Some(ComposerQueueFailureCodeV4::RunFailed),
        "cancelled" => Some(ComposerQueueFailureCodeV4::CancelledByUser),
        "uncertain" => Some(ComposerQueueFailureCodeV4::LeaseUncertain),
        _ => None,
    }
}

fn terminal_queue_status(events: &[AgentEventV4]) -> Option<&'static str> {
    events.iter().find_map(|event| match event.event {
        AgentEventKindV4::RunCompleted { .. } => Some("completed"),
        AgentEventKindV4::RunFailed { .. } => Some("failed"),
        AgentEventKindV4::RunCancelled { .. } => Some("cancelled"),
        _ => None,
    })
}

pub(super) async fn observe_terminal_event_in_tx(
    tx: &mut SqliteConnection,
    event: &AgentEventV4,
) -> Result<(), StoreError> {
    let Some(status) = terminal_queue_status(std::slice::from_ref(event)) else {
        return Ok(());
    };
    let failure_code = match status {
        "failed" => Some(crate::enum_string(&ComposerQueueFailureCodeV4::RunFailed)?),
        "cancelled" => Some(crate::enum_string(
            &ComposerQueueFailureCodeV4::CancelledByUser,
        )?),
        _ => None,
    };
    sqlx::query(
        "UPDATE composer_queue_v4
         SET status=?1,
             failure_code=?2,
             revision=CASE WHEN revision < 9223372036854775807 THEN revision+1 ELSE revision END,
             lease_owner=NULL,
             lease_expires_at=NULL,
             updated_at=?3
         WHERE run_id=?4 AND project_id=?5 AND conversation_id=?6
           AND status IN ('running','uncertain')",
    )
    .bind(status)
    .bind(failure_code)
    .bind(crate::timestamp(event.occurred_at))
    .bind(event.run_id.to_string())
    .bind(event.project_id.to_string())
    .bind(event.conversation_id.to_string())
    .execute(&mut *tx)
    .await?;
    Ok(())
}
