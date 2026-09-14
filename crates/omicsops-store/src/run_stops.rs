//! Durable local stop intents for Agent V4 runs.
//!
//! A row in `agent_run_stop_requests_v4` records a request made to the local
//! host. It is deliberately separate from remote-job lifecycle state: the
//! host may observe a terminal local run without being able to claim that a
//! remote process was cancelled.

use chrono::Utc;
use omicsops_dto::{StopRunReceiptV4, StopRunRequestV4, StopRunStatusV4};
use omicsops_protocol::{AgentEventKindV4, AgentEventV4, ToolEffectV4};
use sqlx::{Row, Sqlite, SqliteConnection, Transaction};
use uuid::Uuid;

use super::{
    Store, StoreError, ensure_conversation_owner_executor, ensure_run_owner_executor,
    from_timestamp, insert_agent_event_in_tx, is_terminal_event, load_agent_events_in_tx,
    set_agent_run_status_in_tx, timestamp,
};

const REQUESTED: &str = "requested";
const OBSERVED: &str = "observed";

impl Store {
    /// Record a local stop intent, returning the canonical row for the run.
    ///
    /// The request ID is idempotent within its original scope. A run accepts
    /// only one durable intent, so a different request ID for the same run
    /// returns the original receipt rather than creating a second intent.
    pub async fn request_run_stop_v4(
        &self,
        request: &StopRunRequestV4,
    ) -> Result<StopRunReceiptV4, StoreError> {
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let receipt = request_run_stop_in_tx(&mut tx, request).await?;
        tx.commit().await?;
        Ok(receipt)
    }
    /// Read the scoped stop receipt and reconcile it with terminal local
    /// state. This method never performs a remote cancellation.
    pub async fn get_run_stop_v4(
        &self,
        project_id: Uuid,
        conversation_id: Uuid,
        run_id: Uuid,
    ) -> Result<Option<StopRunReceiptV4>, StoreError> {
        validate_scope_ids(project_id, conversation_id, run_id)?;
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        ensure_run_owner_executor(&mut *tx, project_id, conversation_id, run_id).await?;
        let Some(existing) = load_stop_by_run_in_tx(&mut tx, run_id).await? else {
            tx.commit().await?;
            return Ok(None);
        };
        ensure_receipt_scope(&existing, project_id, conversation_id, run_id)?;
        let (run_status, events) = load_run_state_in_tx(&mut tx, run_id).await?;
        let mut receipt = existing;
        if is_local_terminal(&run_status, &events) {
            mark_observed_in_tx(&mut tx, &mut receipt).await?;
        }
        tx.commit().await?;
        Ok(Some(receipt))
    }

    /// Host-internal existence check used by resume/driver guards. It does
    /// not expose the request scope or any run material.
    pub async fn has_run_stop_request_v4(&self, run_id: Uuid) -> Result<bool, StoreError> {
        Ok(sqlx::query_scalar::<_, i64>(
            "SELECT EXISTS(SELECT 1 FROM agent_run_stop_requests_v4 WHERE run_id=?1)",
        )
        .bind(run_id.to_string())
        .fetch_one(&self.pool)
        .await?
            != 0)
    }

    /// Reconcile a run row from its already-persisted terminal event.
    ///
    /// The caller must hold the run's host ownership lease. The event chain
    /// is the authority here because a driver can commit its terminal event
    /// and then exit before its in-memory run snapshot is saved. This method
    /// only changes the duplicated status column and the `status` member in
    /// `value_json`; it never appends an event or copies another snapshot.
    pub async fn repair_agent_run_terminal_status_v4(
        &self,
        project_id: Uuid,
        conversation_id: Uuid,
        run_id: Uuid,
    ) -> Result<Option<String>, StoreError> {
        if project_id.is_nil() || conversation_id.is_nil() || run_id.is_nil() {
            return Err(StoreError::InvalidInput(
                "run scope identifiers cannot be nil".into(),
            ));
        }
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        ensure_conversation_owner_executor(&mut tx, project_id, conversation_id).await?;
        ensure_run_owner_executor(&mut *tx, project_id, conversation_id, run_id).await?;
        let (current_status, events) = load_run_state_in_tx(&mut tx, run_id).await?;
        if events.iter().any(|event| {
            event.run_id != run_id
                || event.project_id != project_id
                || event.conversation_id != conversation_id
        }) {
            return Err(StoreError::InvalidInput(
                "run event scope does not match the requested run".into(),
            ));
        }
        let Some(terminal_status) = events
            .iter()
            .find_map(terminal_event_status)
            .map(str::to_owned)
        else {
            tx.commit().await?;
            return Ok(None);
        };

        let value_json: String = sqlx::query_scalar(
            "SELECT value_json FROM agent_runs_v4
             WHERE run_id=?1 AND project_id=?2 AND conversation_id=?3",
        )
        .bind(run_id.to_string())
        .bind(project_id.to_string())
        .bind(conversation_id.to_string())
        .fetch_one(&mut *tx)
        .await?;
        let value: serde_json::Value = serde_json::from_str(&value_json)?;
        let value_status = value.get("status").and_then(serde_json::Value::as_str);
        if current_status != terminal_status || value_status != Some(terminal_status.as_str()) {
            set_agent_run_status_in_tx(&mut *tx, run_id, &terminal_status).await?;
        }
        tx.commit().await?;
        Ok(Some(terminal_status))
    }

    /// Finalize a stop after the caller has proved that no local driver owns
    /// the run. The operation is local-only and atomically appends one
    /// terminal hash-chain event with the status update and receipt update.
    pub async fn finalize_inactive_run_stop_v4(
        &self,
        project_id: Uuid,
        conversation_id: Uuid,
        run_id: Uuid,
    ) -> Result<Option<AgentEventV4>, StoreError> {
        validate_scope_ids(project_id, conversation_id, run_id)?;
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        ensure_run_owner_executor(&mut *tx, project_id, conversation_id, run_id).await?;
        let Some(mut receipt) = load_stop_by_run_in_tx(&mut tx, run_id).await? else {
            tx.commit().await?;
            return Ok(None);
        };
        ensure_receipt_scope(&receipt, project_id, conversation_id, run_id)?;

        let (run_status, events) = load_run_state_in_tx(&mut tx, run_id).await?;
        if is_local_terminal(&run_status, &events) {
            // A driver can append its terminal event and crash before saving
            // the matching run row. Under the caller's lease, repair only a
            // nonterminal row from the already-persisted terminal evidence;
            // never replace an existing terminal status or append another
            // terminal event.
            if !is_terminal_run_status(&run_status) {
                if let Some(status) = events.iter().find_map(terminal_event_status) {
                    set_agent_run_status_in_tx(&mut *tx, run_id, status).await?;
                }
            }
            mark_observed_in_tx(&mut tx, &mut receipt).await?;
            tx.commit().await?;
            return Ok(None);
        }
        if receipt.status != StopRunStatusV4::Requested {
            return Err(StoreError::InvalidInput(
                "observed stop receipt belongs to a nonterminal run".into(),
            ));
        }

        // Planning revisions have additional durable mode/revision invariants
        // and are finalized through Store::cancel_plan_v4 by the host. Never
        // leave a plan row active while appending a generic Agent cancellation.
        let active_plan: i64 = sqlx::query_scalar(
            "SELECT EXISTS(
                SELECT 1 FROM proposed_plans
                WHERE project_id=?1 AND frame_id=?2 AND run_id=?3
                  AND status IN ('generating','revising','pending')
            )",
        )
        .bind(project_id.to_string())
        .bind(conversation_id.to_string())
        .bind(run_id.to_string())
        .fetch_one(&mut *tx)
        .await?;
        if active_plan != 0 {
            return Err(StoreError::InvalidInput(
                "active plan revision must be cancelled through cancel_plan_v4".into(),
            ));
        }

        let now = Utc::now();
        let terminal_event = if has_unresolved_side_effect_dispatch(&events) {
            set_agent_run_status_in_tx(&mut *tx, run_id, "needs_attention").await?;
            AgentEventKindV4::RunNeedsAttention {
                message: "run stopped while a side-effectful tool dispatch was unresolved".into(),
            }
        } else {
            set_agent_run_status_in_tx(&mut *tx, run_id, "cancelled").await?;
            AgentEventKindV4::RunCancelled
        };
        let event = if let Some(previous) = events.last() {
            AgentEventV4::next(previous, now, terminal_event)
        } else {
            AgentEventV4::first(run_id, project_id, conversation_id, now, terminal_event)
        };
        insert_agent_event_in_tx(&mut *tx, &event).await?;
        mark_observed_in_tx(&mut tx, &mut receipt).await?;
        tx.commit().await?;
        Ok(Some(event))
    }
}

/// Reject a new guidance row after any durable stop intent. The caller must
/// invoke this after checking the guidance message-id retry, so an identical
/// retry remains idempotent.
pub(super) async fn ensure_no_stop_request_in_tx(
    tx: &mut SqliteConnection,
    run_id: Uuid,
) -> Result<(), StoreError> {
    let exists: i64 = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM agent_run_stop_requests_v4 WHERE run_id=?1)",
    )
    .bind(run_id.to_string())
    .fetch_one(&mut *tx)
    .await?;
    if exists != 0 {
        return Err(StoreError::InvalidInput(
            "run has a durable stop request; new guidance is rejected".into(),
        ));
    }
    Ok(())
}

/// Guard ordinary sends/starts. A terminal save for the same run may use the
/// optional exception so that a stop intent cannot prevent final persistence;
/// all new runs and nonterminal writes remain blocked.
pub(super) async fn ensure_no_active_stop_request_executor(
    tx: &mut SqliteConnection,
    project_id: Uuid,
    conversation_id: Uuid,
    terminal_run: Option<Uuid>,
) -> Result<(), StoreError> {
    let terminal_run = terminal_run.map(|run_id| run_id.to_string());
    let exists: i64 = sqlx::query_scalar(
        "SELECT EXISTS(
            SELECT 1
            FROM agent_run_stop_requests_v4
            WHERE project_id=?1 AND conversation_id=?2 AND status='requested'
              AND (?3 IS NULL OR run_id != ?3)
        )",
    )
    .bind(project_id.to_string())
    .bind(conversation_id.to_string())
    .bind(terminal_run)
    .fetch_one(&mut *tx)
    .await?;
    if exists != 0 {
        return Err(StoreError::InvalidInput(
            "conversation has a pending run stop request".into(),
        ));
    }
    Ok(())
}

/// Protect terminal event evidence from a delayed nonterminal run snapshot.
pub(super) async fn ensure_run_save_status_in_tx(
    tx: &mut SqliteConnection,
    run_id: Uuid,
    incoming_status: &str,
) -> Result<(), StoreError> {
    let has_stop: i64 = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM agent_run_stop_requests_v4 WHERE run_id=?1)",
    )
    .bind(run_id.to_string())
    .fetch_one(&mut *tx)
    .await?;
    if has_stop == 0 {
        return Ok(());
    }
    let current_status: String =
        sqlx::query_scalar("SELECT status FROM agent_runs_v4 WHERE run_id=?1")
            .bind(run_id.to_string())
            .fetch_one(&mut *tx)
            .await?;
    if is_terminal_run_status(&current_status) && current_status != incoming_status {
        return Err(StoreError::InvalidInput(
            "delayed run snapshot cannot overwrite terminal local status".into(),
        ));
    }
    let events = load_agent_events_in_tx(&mut *tx, run_id).await?;
    if let Some(terminal_status) = events.iter().find_map(terminal_event_status) {
        if terminal_status != incoming_status {
            return Err(StoreError::InvalidInput(
                "delayed run snapshot cannot overwrite terminal event evidence".into(),
            ));
        }
    }
    Ok(())
}

/// Validate stop rows while opening the database. Foreign keys establish
/// existence, while this check establishes that the denormalized persisted
/// scope matches the owning run and that no malformed status/id can become a
/// host authority record after restart.
pub(super) async fn validate_persisted_stop_requests(
    tx: &mut Transaction<'_, Sqlite>,
) -> Result<(), StoreError> {
    let rows = sqlx::query(
        "SELECT s.request_id,s.run_id,s.project_id,s.conversation_id,s.status,
                s.created_at,s.updated_at,r.project_id,r.conversation_id
         FROM agent_run_stop_requests_v4 s
         LEFT JOIN agent_runs_v4 r ON r.run_id=s.run_id
         ORDER BY s.request_id",
    )
    .fetch_all(&mut **tx)
    .await?;
    for row in rows {
        let request_id = row.try_get::<String, _>(0)?;
        let run_id = row.try_get::<String, _>(1)?;
        let project_id = row.try_get::<String, _>(2)?;
        let conversation_id = row.try_get::<String, _>(3)?;
        let status = row.try_get::<String, _>(4)?;
        let owner_project = row.try_get::<Option<String>, _>(7)?;
        let owner_conversation = row.try_get::<Option<String>, _>(8)?;
        let parsed_request = super::parse_uuid(&request_id, "stop request id")
            .map_err(|error| StoreError::Migration(error.to_string()))?;
        let parsed_run = super::parse_uuid(&run_id, "stop run id")
            .map_err(|error| StoreError::Migration(error.to_string()))?;
        let parsed_project = super::parse_uuid(&project_id, "stop project id")
            .map_err(|error| StoreError::Migration(error.to_string()))?;
        let parsed_conversation = super::parse_uuid(&conversation_id, "stop conversation id")
            .map_err(|error| StoreError::Migration(error.to_string()))?;
        if parsed_request.is_nil()
            || parsed_run.is_nil()
            || parsed_project.is_nil()
            || parsed_conversation.is_nil()
            || parse_status(&status).is_err()
            || owner_project.as_deref() != Some(project_id.as_str())
            || owner_conversation.as_deref() != Some(conversation_id.as_str())
            || from_timestamp(row.try_get(5)?, "stop request created_at").is_err()
            || from_timestamp(row.try_get(6)?, "stop request updated_at").is_err()
        {
            return Err(StoreError::Migration(format!(
                "stop request {request_id} has invalid status, identity, scope, or timestamp"
            )));
        }
    }
    Ok(())
}

pub(super) fn is_terminal_run_status(status: &str) -> bool {
    matches!(
        status,
        "completed" | "cancelled" | "failed" | "needs_attention"
    )
}

fn validate_stop_request(request: &StopRunRequestV4) -> Result<(), StoreError> {
    validate_scope_ids(request.project_id, request.conversation_id, request.run_id)?;
    if request.request_id.is_nil() {
        return Err(StoreError::InvalidInput(
            "stop request id cannot be nil".into(),
        ));
    }
    Ok(())
}

fn validate_scope_ids(
    project_id: Uuid,
    conversation_id: Uuid,
    run_id: Uuid,
) -> Result<(), StoreError> {
    if project_id.is_nil() || conversation_id.is_nil() || run_id.is_nil() {
        return Err(StoreError::InvalidInput(
            "stop scope identifiers cannot be nil".into(),
        ));
    }
    Ok(())
}

async fn load_stop_by_request_in_tx(
    tx: &mut SqliteConnection,
    request_id: Uuid,
) -> Result<Option<StopRunReceiptV4>, StoreError> {
    sqlx::query(
        "SELECT request_id,project_id,conversation_id,run_id,status,created_at,updated_at
         FROM agent_run_stop_requests_v4 WHERE request_id=?1",
    )
    .bind(request_id.to_string())
    .fetch_optional(&mut *tx)
    .await?
    .map(receipt_from_row)
    .transpose()
}

async fn load_stop_by_run_in_tx(
    tx: &mut SqliteConnection,
    run_id: Uuid,
) -> Result<Option<StopRunReceiptV4>, StoreError> {
    sqlx::query(
        "SELECT request_id,project_id,conversation_id,run_id,status,created_at,updated_at
         FROM agent_run_stop_requests_v4 WHERE run_id=?1",
    )
    .bind(run_id.to_string())
    .fetch_optional(&mut *tx)
    .await?
    .map(receipt_from_row)
    .transpose()
}

fn receipt_from_row(row: sqlx::sqlite::SqliteRow) -> Result<StopRunReceiptV4, StoreError> {
    Ok(StopRunReceiptV4 {
        request_id: super::parse_uuid(row.try_get::<String, _>(0)?, "stop request id")?,
        project_id: super::parse_uuid(row.try_get::<String, _>(1)?, "stop project id")?,
        conversation_id: super::parse_uuid(row.try_get::<String, _>(2)?, "stop conversation id")?,
        run_id: super::parse_uuid(row.try_get::<String, _>(3)?, "stop run id")?,
        status: parse_status(row.try_get::<String, _>(4)?.as_str())?,
        created_at: from_timestamp(row.try_get(5)?, "stop request created_at")?,
        updated_at: from_timestamp(row.try_get(6)?, "stop request updated_at")?,
    })
}

fn parse_status(value: &str) -> Result<StopRunStatusV4, StoreError> {
    match value {
        REQUESTED => Ok(StopRunStatusV4::Requested),
        OBSERVED => Ok(StopRunStatusV4::Observed),
        _ => Err(StoreError::InvalidInput(
            "invalid persisted stop request status".into(),
        )),
    }
}

fn status_string(status: StopRunStatusV4) -> &'static str {
    match status {
        StopRunStatusV4::Requested => REQUESTED,
        StopRunStatusV4::Observed => OBSERVED,
    }
}

fn ensure_receipt_matches_request(
    receipt: &StopRunReceiptV4,
    request: &StopRunRequestV4,
) -> Result<(), StoreError> {
    if receipt.request_id != request.request_id {
        return Err(StoreError::InvalidInput(
            "stop request id does not match persisted receipt".into(),
        ));
    }
    ensure_receipt_scope(
        receipt,
        request.project_id,
        request.conversation_id,
        request.run_id,
    )
}

fn ensure_receipt_scope(
    receipt: &StopRunReceiptV4,
    project_id: Uuid,
    conversation_id: Uuid,
    run_id: Uuid,
) -> Result<(), StoreError> {
    if receipt.project_id != project_id
        || receipt.conversation_id != conversation_id
        || receipt.run_id != run_id
    {
        return Err(StoreError::InvalidInput(
            "stop request belongs to a different project, conversation, or run".into(),
        ));
    }
    Ok(())
}

async fn mark_observed_in_tx(
    tx: &mut SqliteConnection,
    receipt: &mut StopRunReceiptV4,
) -> Result<(), StoreError> {
    if receipt.status == StopRunStatusV4::Observed {
        return Ok(());
    }
    let now = from_timestamp(timestamp(Utc::now()), "stop observation timestamp")?;
    let updated = sqlx::query(
        "UPDATE agent_run_stop_requests_v4
         SET status='observed',updated_at=?1
         WHERE request_id=?2 AND status='requested'",
    )
    .bind(timestamp(now))
    .bind(receipt.request_id.to_string())
    .execute(&mut *tx)
    .await?;
    if updated.rows_affected() == 1 {
        receipt.status = StopRunStatusV4::Observed;
        receipt.updated_at = now;
    } else {
        let current = load_stop_by_request_in_tx(&mut *tx, receipt.request_id)
            .await?
            .ok_or_else(|| {
                StoreError::InvalidInput("stop receipt disappeared during reconciliation".into())
            })?;
        *receipt = current;
    }
    Ok(())
}

async fn load_run_state_in_tx(
    tx: &mut SqliteConnection,
    run_id: Uuid,
) -> Result<(String, Vec<AgentEventV4>), StoreError> {
    let status: String = sqlx::query_scalar("SELECT status FROM agent_runs_v4 WHERE run_id=?1")
        .bind(run_id.to_string())
        .fetch_one(&mut *tx)
        .await?;
    let events = load_agent_events_in_tx(&mut *tx, run_id).await?;
    Ok((status, events))
}

fn is_local_terminal(status: &str, events: &[AgentEventV4]) -> bool {
    is_terminal_run_status(status) || events.iter().any(is_terminal_event)
}

fn terminal_event_status(event: &AgentEventV4) -> Option<&'static str> {
    match event.event {
        AgentEventKindV4::RunCompleted => Some("completed"),
        AgentEventKindV4::RunFailed { .. } => Some("failed"),
        AgentEventKindV4::RunNeedsAttention { .. } => Some("needs_attention"),
        AgentEventKindV4::RunCancelled => Some("cancelled"),
        _ => None,
    }
}

/// A read-only dispatch cannot create an external side effect. Any
/// unresolved runtime/network/mutating/delegation dispatch stays
/// `needs_attention` so cancellation or driver recovery never hides uncertain
/// evidence or advances the conversation queue past an unresolved effect.
pub fn has_unresolved_side_effect_dispatch(events: &[AgentEventV4]) -> bool {
    let mut started_effects = std::collections::HashMap::<&str, ToolEffectV4>::new();
    let mut resolved = std::collections::HashSet::<&str>::new();
    for event in events {
        match &event.event {
            AgentEventKindV4::ToolDispatchStarted {
                call_id, effect, ..
            } => {
                started_effects.insert(call_id.as_str(), *effect);
            }
            AgentEventKindV4::ToolFinished { outcome }
            | AgentEventKindV4::ToolOutcomeReused { outcome, .. } => {
                resolved.insert(outcome.call_id.as_str());
            }
            AgentEventKindV4::ToolDispatchResolved { call_id, .. } => {
                resolved.insert(call_id.as_str());
            }
            _ => {}
        }
    }
    for event in events {
        match &event.event {
            AgentEventKindV4::ToolDispatchStarted {
                call_id, effect, ..
            } if *effect != ToolEffectV4::ReadOnly && !resolved.contains(call_id.as_str()) => {
                return true;
            }
            AgentEventKindV4::ToolDispatchUncertain { call_id, .. }
                if !resolved.contains(call_id.as_str()) =>
            {
                // A read-only dispatch has no external side effect. A
                // standalone marker has no trustworthy classification, so it
                // remains fail-closed and retains attention.
                if started_effects
                    .get(call_id.as_str())
                    .is_none_or(|effect| *effect != ToolEffectV4::ReadOnly)
                {
                    return true;
                }
            }
            _ => {}
        }
    }
    false
}

/// Shared transaction boundary for an ordinary Stop or an atomic replacement.
pub(super) async fn request_run_stop_in_tx(
    tx: &mut SqliteConnection,
    request: &StopRunRequestV4,
) -> Result<StopRunReceiptV4, StoreError> {
    validate_stop_request(request)?;

    if let Some(existing) = load_stop_by_request_in_tx(&mut *tx, request.request_id).await? {
        ensure_receipt_matches_request(&existing, request)?;
        ensure_run_owner_executor(
            &mut *tx,
            request.project_id,
            request.conversation_id,
            request.run_id,
        )
        .await?;
        let (run_status, events) = load_run_state_in_tx(&mut *tx, request.run_id).await?;
        let mut receipt = existing;
        if is_local_terminal(&run_status, &events) {
            mark_observed_in_tx(&mut *tx, &mut receipt).await?;
        }
        return Ok(receipt);
    }

    ensure_run_owner_executor(
        &mut *tx,
        request.project_id,
        request.conversation_id,
        request.run_id,
    )
    .await?;

    if let Some(existing) = load_stop_by_run_in_tx(&mut *tx, request.run_id).await? {
        ensure_receipt_scope(
            &existing,
            request.project_id,
            request.conversation_id,
            request.run_id,
        )?;
        let (run_status, events) = load_run_state_in_tx(&mut *tx, request.run_id).await?;
        let mut receipt = existing;
        if is_local_terminal(&run_status, &events) {
            mark_observed_in_tx(&mut *tx, &mut receipt).await?;
        }
        return Ok(receipt);
    }

    let (run_status, events) = load_run_state_in_tx(&mut *tx, request.run_id).await?;
    let status = if is_local_terminal(&run_status, &events) {
        StopRunStatusV4::Observed
    } else {
        StopRunStatusV4::Requested
    };
    let now = from_timestamp(timestamp(Utc::now()), "stop request timestamp")?;
    let status_value = status_string(status);
    sqlx::query(
        "INSERT INTO agent_run_stop_requests_v4
             (request_id,run_id,project_id,conversation_id,status,created_at,updated_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7)",
    )
    .bind(request.request_id.to_string())
    .bind(request.run_id.to_string())
    .bind(request.project_id.to_string())
    .bind(request.conversation_id.to_string())
    .bind(status_value)
    .bind(timestamp(now))
    .bind(timestamp(now))
    .execute(&mut *tx)
    .await?;
    Ok(StopRunReceiptV4 {
        request_id: request.request_id,
        project_id: request.project_id,
        conversation_id: request.conversation_id,
        run_id: request.run_id,
        status,
        created_at: now,
        updated_at: now,
    })
}
