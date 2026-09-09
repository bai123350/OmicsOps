//! Durable launch reservations. Never persist code, output, or credential values
//! here; the existing tool event/archive path owns results and provenance.
use super::*;
use omicsops_protocol::{ExecutionContextKeyV4, RuntimeJobStateV4, RuntimeJobV4, RuntimeResultV4};

impl Store {
    /// End a paused saved-result recovery, under the desktop's driver slot.
    /// Receipts and job identities remain available for audit; nothing executes.
    pub async fn cancel_runtime_recovery_v4(&self, run_id: Uuid) -> Result<AgentEventV4, StoreError> {
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let row = sqlx::query("SELECT status,value_json FROM agent_runs_v4 WHERE run_id=?1")
            .bind(run_id.to_string()).fetch_optional(&mut *tx).await?
            .ok_or_else(|| StoreError::InvalidInput("run not found".into()))?;
        let status: String = row.try_get(0)?;
        let events = load_agent_events_in_tx(&mut tx, run_id).await?;
        let previous = events.last().ok_or_else(|| StoreError::InvalidInput("run events missing".into()))?;
        if status == "cancelled" && matches!(previous.event, AgentEventKindV4::RunCancelled)
            && events.iter().rev().nth(1).is_some_and(|event| matches!(event.event, AgentEventKindV4::RuntimeRecoveryAvailable { .. })) {
            return Ok(previous.clone());
        }
        if status != "waiting_for_input" || events.iter().any(is_terminal_event)
            || !matches!(previous.event, AgentEventKindV4::RuntimeRecoveryAvailable { .. }) {
            return Err(StoreError::InvalidInput("run is not waiting for saved-result recovery".into()));
        }
        let event = AgentEventV4::next(previous, Utc::now(), AgentEventKindV4::RunCancelled);
        insert_agent_event_in_tx(&mut tx, &event).await?;
        let mut value: Value = serde_json::from_str(&row.try_get::<String, _>(1)?)?;
        value["status"] = Value::String("cancelled".into());
        sqlx::query("UPDATE agent_runs_v4 SET status='cancelled',value_json=?1 WHERE run_id=?2")
            .bind(serde_json::to_string(&value)?).bind(run_id.to_string()).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(event)
    }

    /// Called only after the desktop has acquired the inactive driver's slot.
    /// Preserve an interrupted run when every outstanding dispatched operation
    /// has a verified terminal receipt. Unknown side effects are never promoted.
    pub async fn prepare_runtime_recovery_v4(
        &self,
        run_id: Uuid,
    ) -> Result<Option<AgentEventV4>, StoreError> {
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let row = sqlx::query("SELECT status,value_json FROM agent_runs_v4 WHERE run_id=?1")
            .bind(run_id.to_string())
            .fetch_optional(&mut *tx)
            .await?;
        let Some(row) = row else {
            return Ok(None);
        };
        if row.try_get::<String, _>(0)? != "running" {
            return Ok(None);
        }
        let mut value: Value = serde_json::from_str(&row.try_get::<String, _>(1)?)?;
        let Some(spec_value) = value.get("spec").filter(|spec| !spec.is_null()) else {
            return Ok(None);
        };
        let spec: RunSpecV4 = serde_json::from_value(spec_value.clone())?;
        spec.validate_integrity()
            .map_err(|error| StoreError::InvalidInput(error.to_string()))?;
        let events = load_agent_events_in_tx(&mut tx, run_id).await?;
        if events.iter().any(is_terminal_event) || spec.run_id != run_id {
            return Ok(None);
        }
        let mut requested = BTreeMap::new();
        let mut pending = BTreeMap::new();
        for event in &events {
            match &event.event {
                AgentEventKindV4::ToolRequested { call } => {
                    requested.insert(call.call_id.clone(), call.clone());
                }
                AgentEventKindV4::ToolDispatchStarted {
                    call_id, tool_id, ..
                } => {
                    pending.insert(call_id.clone(), tool_id.clone());
                }
                AgentEventKindV4::ToolFinished { outcome }
                | AgentEventKindV4::ToolOutcomeReused { outcome, .. } => {
                    pending.remove(&outcome.call_id);
                }
                AgentEventKindV4::ToolDispatchResolved { call_id, .. } => {
                    pending.remove(call_id);
                }
                AgentEventKindV4::ToolDispatchUncertain { .. } => return Ok(None),
                _ => {}
            }
        }
        if pending.is_empty() {
            return Ok(None);
        }
        for (call_id, tool_id) in &pending {
            if tool_id != "runtime.execute" {
                return Ok(None);
            }
            let Some(call) = requested.get(call_id) else {
                return Ok(None);
            };
            let row = sqlx::query("SELECT j.value_json,r.result_json FROM runtime_jobs_v4 j JOIN runtime_job_results_v4 r ON j.job_id=r.job_id WHERE j.run_id=?1 AND j.call_id=?2")
                .bind(run_id.to_string()).bind(call_id).fetch_optional(&mut *tx).await?;
            let Some(row) = row else {
                return Ok(None);
            };
            let job: RuntimeJobV4 = serde_json::from_str(&row.try_get::<String, _>(0)?)?;
            let serialized: String = row.try_get(1)?;
            if job.context.project_id != spec.project_id
                || job.context.run_id != run_id
                || job.call_id != *call_id
                || !matches!(
                    job.state,
                    RuntimeJobStateV4::Succeeded | RuntimeJobStateV4::Failed
                )
                || call.canonical_hash().ok().as_deref() != Some(job.request_sha256.as_str())
                || serialized.len() > 1024 * 1024
                || Some(hex::encode(Sha256::digest(serialized.as_bytes()))) != job.result_sha256
            {
                return Ok(None);
            }
            let result: RuntimeResultV4 = serde_json::from_str(&serialized)?;
            if Some(result.session_id) != job.session_id
                || Some(result.request_id) != job.result_request_id
                || result.succeeded != (job.state == RuntimeJobStateV4::Succeeded)
            {
                return Ok(None);
            }
            if spec.compute_selection.as_ref().is_some_and(|selection| {
                selection.backend_id != job.context.backend_id
                    || selection.environment != job.context.environment
            }) {
                return Ok(None);
            }
        }
        let Some(previous) = events.last() else {
            return Ok(None);
        };
        let event = AgentEventV4::next(
            previous,
            Utc::now(),
            AgentEventKindV4::RuntimeRecoveryAvailable {
                call_ids: pending.into_keys().collect(),
            },
        );
        insert_agent_event_in_tx(&mut tx, &event).await?;
        value["status"] = Value::String("waiting_for_input".into());
        sqlx::query(
            "UPDATE agent_runs_v4 SET status='waiting_for_input',value_json=?1 WHERE run_id=?2",
        )
        .bind(serde_json::to_string(&value)?)
        .bind(run_id.to_string())
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(Some(event))
    }
    /// `true` grants the sole initial launch. Any existing reservation, including
    /// an unknown one after restart, must not be treated as permission to retry.
    pub async fn reserve_runtime_job_v4(
        &self,
        context: &ExecutionContextKeyV4,
        call_id: &str,
        request_sha256: &str,
    ) -> Result<(RuntimeJobV4, bool), StoreError> {
        if call_id.trim().is_empty()
            || call_id.len() > 256
            || request_sha256.len() != 64
            || !request_sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            return Err(StoreError::InvalidInput(
                "invalid runtime job identity".into(),
            ));
        }
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let value: Option<String> = sqlx::query_scalar(
            "SELECT value_json FROM agent_runs_v4 WHERE run_id=?1 AND project_id=?2",
        )
        .bind(context.run_id.to_string())
        .bind(context.project_id.to_string())
        .fetch_optional(&mut *tx)
        .await?;
        let value: Value =
            serde_json::from_str(&value.ok_or_else(|| {
                StoreError::InvalidInput("runtime job run scope mismatch".into())
            })?)?;
        let spec: RunSpecV4 =
            serde_json::from_value(value.get("spec").cloned().ok_or_else(|| {
                StoreError::InvalidInput("runtime job requires a frozen spec".into())
            })?)?;
        spec.validate_integrity()
            .map_err(|error| StoreError::InvalidInput(error.to_string()))?;
        if spec.run_id != context.run_id || spec.project_id != context.project_id {
            return Err(StoreError::InvalidInput(
                "runtime job spec scope mismatch".into(),
            ));
        }
        if let Some(selection) = &spec.compute_selection {
            if selection.backend_id != context.backend_id
                || selection.environment != context.environment
            {
                return Err(StoreError::InvalidInput(
                    "runtime job differs from frozen compute selection".into(),
                ));
            }
        }
        let previous: Option<String> = sqlx::query_scalar(
            "SELECT value_json FROM runtime_jobs_v4 WHERE run_id=?1 AND call_id=?2",
        )
        .bind(context.run_id.to_string())
        .bind(call_id)
        .fetch_optional(&mut *tx)
        .await?;
        if let Some(previous) = previous {
            let job: RuntimeJobV4 = serde_json::from_str(&previous)?;
            if job.context != *context || job.request_sha256 != request_sha256 {
                return Err(StoreError::InvalidInput(
                    "runtime job identity already binds another request".into(),
                ));
            }
            return Ok((job, false));
        }
        let events = load_agent_events_in_tx(&mut tx, context.run_id).await?;
        let request_matches = events
            .iter()
            .rev()
            .find_map(|event| match &event.event {
                AgentEventKindV4::ToolRequested { call } if call.call_id == call_id => Some(call),
                _ => None,
            })
            .is_some_and(|call| {
                call.tool_id == "runtime.execute"
                    && call.canonical_hash().ok().as_deref() == Some(request_sha256)
                    && call.arguments.get("language")
                        == Some(&serde_json::to_value(context.language).unwrap_or(Value::Null))
                    && call
                        .arguments
                        .get("environment")
                        .and_then(Value::as_str)
                        .unwrap_or("system")
                        == context.environment
            });
        let resolved = events.iter().any(|event| match &event.event {
            AgentEventKindV4::ToolFinished { outcome }
            | AgentEventKindV4::ToolOutcomeReused { outcome, .. } => outcome.call_id == call_id,
            AgentEventKindV4::ToolDispatchResolved { call_id: id, .. }
            | AgentEventKindV4::ToolDispatchUncertain { call_id: id, .. } => id == call_id,
            _ => false,
        });
        if !request_matches || resolved || events.iter().any(is_terminal_event) || !events.iter().any(|event| matches!(&event.event,
            AgentEventKindV4::ToolDispatchStarted { call_id: id, tool_id, effect: ToolEffectV4::Runtime, .. }
            if id == call_id && tool_id == "runtime.execute")) {
            return Err(StoreError::InvalidInput("runtime job requires an active recorded dispatch".into()));
        }
        let job = RuntimeJobV4 {
            job_id: Uuid::new_v4(),
            context: context.clone(),
            call_id: call_id.into(),
            request_sha256: request_sha256.into(),
            state: RuntimeJobStateV4::Reserved,
            session_id: None,
            result_request_id: None,
            result_sha256: None,
        };
        sqlx::query(
            "INSERT INTO runtime_jobs_v4(job_id,run_id,call_id,value_json) VALUES (?1,?2,?3,?4)",
        )
        .bind(job.job_id.to_string())
        .bind(context.run_id.to_string())
        .bind(call_id)
        .bind(serde_json::to_string(&job)?)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok((job, true))
    }

    pub async fn runtime_job_v4(
        &self,
        context: &ExecutionContextKeyV4,
        call_id: &str,
    ) -> Result<Option<RuntimeJobV4>, StoreError> {
        let value: Option<String> = sqlx::query_scalar(
            "SELECT value_json FROM runtime_jobs_v4 WHERE run_id=?1 AND call_id=?2",
        )
        .bind(context.run_id.to_string())
        .bind(call_id)
        .fetch_optional(&self.pool)
        .await?;
        let job: Option<RuntimeJobV4> = value
            .map(|value| serde_json::from_str(&value))
            .transpose()?;
        if job.as_ref().is_some_and(|job| job.context != *context) {
            return Err(StoreError::InvalidInput(
                "runtime job context mismatch".into(),
            ));
        }
        Ok(job)
    }

    /// Compare-and-swap prevents late workers from changing terminal metadata.
    pub async fn advance_runtime_job_v4(
        &self,
        previous: &RuntimeJobV4,
        state: RuntimeJobStateV4,
        session_id: Option<Uuid>,
        result: Option<&RuntimeResultV4>,
    ) -> Result<RuntimeJobV4, StoreError> {
        use RuntimeJobStateV4::*;
        let allowed = matches!(
            (previous.state, state),
            (Reserved, Running | Unknown) | (Running, Unknown | Succeeded | Failed)
        );
        if !allowed
            || (state == Running && session_id.is_none())
            || (state != Running && session_id.is_some())
            || (matches!(state, Succeeded | Failed) != result.is_some())
        {
            return Err(StoreError::InvalidInput(
                "invalid runtime job transition".into(),
            ));
        }
        if let Some(result) = result {
            if previous.session_id != Some(result.session_id)
                || result.succeeded != (state == Succeeded)
            {
                return Err(StoreError::InvalidInput(
                    "runtime job result mismatch".into(),
                ));
            }
        }
        let mut next = previous.clone();
        next.state = state;
        next.session_id = previous.session_id.or(session_id);
        if let Some(result) = result {
            next.result_request_id = Some(result.request_id);
            next.result_sha256 = Some(hex::encode(Sha256::digest(serde_json::to_vec(result)?)));
        }
        let receipt = result.map(serde_json::to_string).transpose()?;
        if receipt
            .as_ref()
            .is_some_and(|value| value.len() > 1024 * 1024)
        {
            return Err(StoreError::InvalidInput(
                "runtime result receipt exceeds 1 MiB".into(),
            ));
        }
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let changed = sqlx::query(
            "UPDATE runtime_jobs_v4 SET value_json=?1 WHERE job_id=?2 AND value_json=?3",
        )
        .bind(serde_json::to_string(&next)?)
        .bind(previous.job_id.to_string())
        .bind(serde_json::to_string(previous)?)
        .execute(&mut *tx)
        .await?
        .rows_affected();
        if changed != 1 {
            return Err(StoreError::InvalidInput(
                "runtime job changed concurrently".into(),
            ));
        }
        if let Some(receipt) = receipt {
            sqlx::query("INSERT INTO runtime_job_results_v4(job_id,result_json) VALUES (?1,?2)")
                .bind(next.job_id.to_string())
                .bind(receipt)
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;
        Ok(next)
    }

    pub async fn recover_runtime_result_v4(
        &self,
        context: &ExecutionContextKeyV4,
        call: &omicsops_protocol::ToolCallV4,
    ) -> Result<Option<(RuntimeJobV4, RuntimeResultV4)>, StoreError> {
        let Some(job) = self.runtime_job_v4(context, &call.call_id).await? else {
            return Ok(None);
        };
        if call.tool_id != "runtime.execute"
            || call
                .canonical_hash()
                .map_err(|error| StoreError::InvalidInput(error.to_string()))?
                != job.request_sha256
        {
            return Err(StoreError::InvalidInput(
                "runtime receipt request mismatch".into(),
            ));
        }
        if !matches!(
            job.state,
            RuntimeJobStateV4::Succeeded | RuntimeJobStateV4::Failed
        ) {
            return Ok(None);
        }
        let serialized: Option<String> =
            sqlx::query_scalar("SELECT result_json FROM runtime_job_results_v4 WHERE job_id=?1")
                .bind(job.job_id.to_string())
                .fetch_optional(&self.pool)
                .await?;
        let Some(serialized) = serialized else {
            return Ok(None);
        };
        if serialized.len() > 1024 * 1024
            || Some(hex::encode(Sha256::digest(serialized.as_bytes()))) != job.result_sha256
        {
            return Err(StoreError::InvalidInput(
                "runtime receipt digest mismatch".into(),
            ));
        }
        let result: RuntimeResultV4 = serde_json::from_str(&serialized)?;
        if Some(result.session_id) != job.session_id
            || Some(result.request_id) != job.result_request_id
            || result.succeeded != (job.state == RuntimeJobStateV4::Succeeded)
        {
            return Err(StoreError::InvalidInput(
                "runtime receipt identity mismatch".into(),
            ));
        }
        Ok(Some((job, result)))
    }
}

pub(super) async fn observe_runtime_event(
    tx: &mut SqliteConnection,
    event: &AgentEventV4,
) -> Result<(), StoreError> {
    if let AgentEventKindV4::ToolFinished { outcome } = &event.event {
        sqlx::query("DELETE FROM runtime_job_results_v4 WHERE job_id IN (SELECT job_id FROM runtime_jobs_v4 WHERE run_id=?1 AND call_id=?2)")
            .bind(event.run_id.to_string()).bind(&outcome.call_id).execute(&mut *tx).await?;
    }
    let call_id = match &event.event {
        AgentEventKindV4::ToolDispatchUncertain { call_id, .. } => Some(call_id.as_str()),
        AgentEventKindV4::RunCancelled | AgentEventKindV4::RunFailed { .. } => None,
        _ => return Ok(()),
    };
    sqlx::query(
        "UPDATE runtime_jobs_v4 SET value_json=json_set(value_json,'$.state','unknown')
        WHERE run_id=?1 AND (?2 IS NULL OR call_id=?2)
        AND json_extract(value_json,'$.state') IN ('reserved','running')",
    )
    .bind(event.run_id.to_string())
    .bind(call_id)
    .execute(tx)
    .await?;
    Ok(())
}
