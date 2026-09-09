//! Durable launch reservations. Never persist code, output, or credential values
//! here; the existing tool event/archive path owns results and provenance.
use super::*;
use omicsops_protocol::{ExecutionContextKeyV4, RuntimeJobStateV4, RuntimeJobV4, RuntimeResultV4};

impl Store {
    /// `true` grants the sole initial launch. Any existing reservation, including
    /// an unknown one after restart, must not be treated as permission to retry.
    pub async fn reserve_runtime_job_v4(
        &self,
        context: &ExecutionContextKeyV4,
        call_id: &str,
        request_sha256: &str,
    ) -> Result<(RuntimeJobV4, bool), StoreError> {
        if call_id.trim().is_empty() || call_id.len() > 256 || request_sha256.len() != 64
            || !request_sha256.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(StoreError::InvalidInput("invalid runtime job identity".into()));
        }
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let value: Option<String> = sqlx::query_scalar("SELECT value_json FROM agent_runs_v4 WHERE run_id=?1 AND project_id=?2")
            .bind(context.run_id.to_string()).bind(context.project_id.to_string()).fetch_optional(&mut *tx).await?;
        let value: Value = serde_json::from_str(&value.ok_or_else(|| StoreError::InvalidInput("runtime job run scope mismatch".into()))?)?;
        let spec: RunSpecV4 = serde_json::from_value(value.get("spec").cloned().ok_or_else(|| StoreError::InvalidInput("runtime job requires a frozen spec".into()))?)?;
        spec.validate_integrity().map_err(|error| StoreError::InvalidInput(error.to_string()))?;
        if spec.run_id != context.run_id || spec.project_id != context.project_id {
            return Err(StoreError::InvalidInput("runtime job spec scope mismatch".into()));
        }
        if let Some(selection) = &spec.compute_selection {
            if selection.backend_id != context.backend_id || selection.environment != context.environment {
                return Err(StoreError::InvalidInput("runtime job differs from frozen compute selection".into()));
            }
        }
        let previous: Option<String> = sqlx::query_scalar("SELECT value_json FROM runtime_jobs_v4 WHERE run_id=?1 AND call_id=?2")
            .bind(context.run_id.to_string()).bind(call_id).fetch_optional(&mut *tx).await?;
        if let Some(previous) = previous {
            let job: RuntimeJobV4 = serde_json::from_str(&previous)?;
            if job.context != *context || job.request_sha256 != request_sha256 {
                return Err(StoreError::InvalidInput("runtime job identity already binds another request".into()));
            }
            return Ok((job, false));
        }
        let events = load_agent_events_in_tx(&mut tx, context.run_id).await?;
        let request_matches = events.iter().rev().find_map(|event| match &event.event {
            AgentEventKindV4::ToolRequested { call } if call.call_id == call_id => Some(call),
            _ => None,
        }).is_some_and(|call| {
            call.tool_id == "runtime.execute" && call.canonical_hash().ok().as_deref() == Some(request_sha256)
                && call.arguments.get("language") == Some(&serde_json::to_value(context.language).unwrap_or(Value::Null))
                && call.arguments.get("environment").and_then(Value::as_str).unwrap_or("system") == context.environment
        });
        let resolved = events.iter().any(|event| match &event.event {
            AgentEventKindV4::ToolFinished { outcome } | AgentEventKindV4::ToolOutcomeReused { outcome, .. } => outcome.call_id == call_id,
            AgentEventKindV4::ToolDispatchResolved { call_id: id, .. } | AgentEventKindV4::ToolDispatchUncertain { call_id: id, .. } => id == call_id,
            _ => false,
        });
        if !request_matches || resolved || events.iter().any(is_terminal_event) || !events.iter().any(|event| matches!(&event.event,
            AgentEventKindV4::ToolDispatchStarted { call_id: id, tool_id, effect: ToolEffectV4::Runtime, .. }
            if id == call_id && tool_id == "runtime.execute")) {
            return Err(StoreError::InvalidInput("runtime job requires an active recorded dispatch".into()));
        }
        let job = RuntimeJobV4 {
            job_id: Uuid::new_v4(), context: context.clone(), call_id: call_id.into(),
            request_sha256: request_sha256.into(), state: RuntimeJobStateV4::Reserved,
            session_id: None, result_request_id: None, result_sha256: None,
        };
        sqlx::query("INSERT INTO runtime_jobs_v4(job_id,run_id,call_id,value_json) VALUES (?1,?2,?3,?4)")
            .bind(job.job_id.to_string()).bind(context.run_id.to_string()).bind(call_id)
            .bind(serde_json::to_string(&job)?).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok((job, true))
    }

    pub async fn runtime_job_v4(&self, context: &ExecutionContextKeyV4, call_id: &str) -> Result<Option<RuntimeJobV4>, StoreError> {
        let value: Option<String> = sqlx::query_scalar("SELECT value_json FROM runtime_jobs_v4 WHERE run_id=?1 AND call_id=?2")
            .bind(context.run_id.to_string()).bind(call_id).fetch_optional(&self.pool).await?;
        let job: Option<RuntimeJobV4> = value.map(|value| serde_json::from_str(&value)).transpose()?;
        if job.as_ref().is_some_and(|job| job.context != *context) {
            return Err(StoreError::InvalidInput("runtime job context mismatch".into()));
        }
        Ok(job)
    }

    /// Compare-and-swap prevents late workers from changing terminal metadata.
    pub async fn advance_runtime_job_v4(&self, previous: &RuntimeJobV4, state: RuntimeJobStateV4, session_id: Option<Uuid>, result: Option<&RuntimeResultV4>) -> Result<RuntimeJobV4, StoreError> {
        use RuntimeJobStateV4::*;
        let allowed = matches!((previous.state, state), (Reserved, Running | Unknown) | (Running, Unknown | Succeeded | Failed));
        if !allowed || (state == Running && session_id.is_none())
            || (state != Running && session_id.is_some())
            || (matches!(state, Succeeded | Failed) != result.is_some()) {
            return Err(StoreError::InvalidInput("invalid runtime job transition".into()));
        }
        if let Some(result) = result {
            if previous.session_id != Some(result.session_id) || result.succeeded != (state == Succeeded) {
                return Err(StoreError::InvalidInput("runtime job result mismatch".into()));
            }
        }
        let mut next = previous.clone();
        next.state = state;
        next.session_id = previous.session_id.or(session_id);
        if let Some(result) = result {
            next.result_request_id = Some(result.request_id);
            next.result_sha256 = Some(hex::encode(Sha256::digest(serde_json::to_vec(result)?)));
        }
        let changed = sqlx::query("UPDATE runtime_jobs_v4 SET value_json=?1 WHERE job_id=?2 AND value_json=?3")
            .bind(serde_json::to_string(&next)?).bind(previous.job_id.to_string())
            .bind(serde_json::to_string(previous)?).execute(&self.pool).await?.rows_affected();
        if changed != 1 { return Err(StoreError::InvalidInput("runtime job changed concurrently".into())); }
        Ok(next)
    }
}

pub(super) async fn observe_runtime_event(tx: &mut SqliteConnection, event: &AgentEventV4) -> Result<(), StoreError> {
    let call_id = match &event.event {
        AgentEventKindV4::ToolDispatchUncertain { call_id, .. } => Some(call_id.as_str()),
        AgentEventKindV4::RunCancelled | AgentEventKindV4::RunFailed { .. } => None,
        _ => return Ok(()),
    };
    sqlx::query("UPDATE runtime_jobs_v4 SET value_json=json_set(value_json,'$.state','unknown')
        WHERE run_id=?1 AND (?2 IS NULL OR call_id=?2)
        AND json_extract(value_json,'$.state') IN ('reserved','running')")
        .bind(event.run_id.to_string()).bind(call_id).execute(tx).await?;
    Ok(())
}
