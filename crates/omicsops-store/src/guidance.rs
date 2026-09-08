//! The inbox never appends to the event chain from a sender. The existing
//! driver consumes rows and appends events together at a model boundary.
use super::*;
use omicsops_dto::{GuidanceRecordV4, SubmitGuidanceV4Request};
use omicsops_protocol::RunExecutionKindV4;

impl Store {
    pub async fn accept_guidance_v4(&self, request: &SubmitGuidanceV4Request) -> Result<GuidanceRecordV4, StoreError> {
        let markdown = request.markdown.trim();
        if markdown.is_empty() || markdown.len() > 2048 {
            return Err(StoreError::InvalidInput("guidance must contain 1..2048 UTF-8 bytes".into()));
        }
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let (status, spec) = guidance_spec(&mut tx, request.project_id, request.conversation_id, request.run_id).await?;
        if let Some(row) = sqlx::query("SELECT run_id,ordinal,markdown,accepted_at,consumed_at FROM agent_guidance_v4 WHERE message_id=?1")
            .bind(request.message_id.to_string()).fetch_optional(&mut *tx).await? {
            if row.try_get::<String,_>(0)? != request.run_id.to_string() || row.try_get::<String,_>(2)? != markdown {
                return Err(StoreError::InvalidInput("guidance message id already belongs to a different request".into()));
            }
            return guidance_record(request.message_id, &spec, row);
        }
        let events = load_agent_events_in_tx(&mut tx, request.run_id).await?;
        if status != "running" || events.iter().any(is_terminal_event) {
            return Err(StoreError::InvalidInput("guidance requires an active ordinary Agent run".into()));
        }
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM agent_guidance_v4 WHERE run_id=?1")
            .bind(request.run_id.to_string()).fetch_one(&mut *tx).await?;
        if count >= 16 { return Err(StoreError::InvalidInput("run guidance limit reached (16 messages)".into())); }
        let accepted_at = from_timestamp(timestamp(Utc::now()), "guidance accepted timestamp")?;
        sqlx::query("INSERT INTO agent_guidance_v4(message_id,run_id,ordinal,markdown,accepted_at) VALUES (?1,?2,?3,?4,?5)")
            .bind(request.message_id.to_string()).bind(request.run_id.to_string()).bind(count + 1).bind(markdown)
            .bind(timestamp(accepted_at)).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(GuidanceRecordV4 { message_id: request.message_id, run_id: request.run_id, project_id: request.project_id,
            conversation_id: request.conversation_id, ordinal: (count + 1) as u64, markdown: markdown.into(), accepted_at, consumed_at: None })
    }

    pub async fn list_guidance_v4(&self, project_id: Uuid, conversation_id: Uuid, run_id: Uuid) -> Result<Vec<GuidanceRecordV4>, StoreError> {
        let mut tx = self.pool.begin().await?;
        let (_, spec) = guidance_spec(&mut tx, project_id, conversation_id, run_id).await?;
        let rows = sqlx::query("SELECT run_id,ordinal,markdown,accepted_at,consumed_at,message_id FROM agent_guidance_v4 WHERE run_id=?1 ORDER BY ordinal")
            .bind(run_id.to_string()).fetch_all(&mut *tx).await?;
        rows.into_iter().map(|row| {
            let id = parse_uuid(&row.try_get::<String,_>(5)?, "guidance id")?;
            guidance_record(id, &spec, row)
        }).collect()
    }

    pub async fn has_pending_guidance_v4(&self, run_id: Uuid) -> Result<bool, StoreError> {
        Ok(sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM agent_guidance_v4 WHERE run_id=?1 AND consumed_at IS NULL")
            .bind(run_id.to_string()).fetch_one(&self.pool).await? > 0)
    }

    pub async fn consume_guidance_v4(&self, expected: &RunSpecV4) -> Result<Vec<AgentEventV4>, StoreError> {
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let (_, spec) = guidance_spec(&mut tx, expected.project_id, expected.conversation_id, expected.run_id).await?;
        if spec != *expected { return Err(StoreError::InvalidInput("guidance consumer has a different frozen specification".into())); }
        let rows = sqlx::query("SELECT message_id,markdown FROM agent_guidance_v4 WHERE run_id=?1 AND consumed_at IS NULL ORDER BY ordinal")
            .bind(spec.run_id.to_string()).fetch_all(&mut *tx).await?;
        if rows.is_empty() { return Ok(vec![]); }
        let history = load_agent_events_in_tx(&mut tx, spec.run_id).await?;
        if history.iter().any(is_terminal_event) { return Err(StoreError::InvalidInput("cannot consume guidance after the run stopped".into())); }
        let mut previous = history.last().cloned().ok_or_else(|| StoreError::InvalidInput("run has not started".into()))?;
        let mut consumed = Vec::new();
        for row in rows {
            let message_id = parse_uuid(&row.try_get::<String,_>(0)?, "guidance message id")?;
            let markdown = row.try_get::<String,_>(1)?;
            let now = from_timestamp(timestamp(Utc::now()), "guidance consumption timestamp")?;
            let event = AgentEventV4::next(&previous, now, AgentEventKindV4::GuidanceConsumed { message_id, markdown });
            insert_agent_event_in_tx(&mut tx, &event).await?;
            sqlx::query("UPDATE agent_guidance_v4 SET consumed_at=?1 WHERE message_id=?2 AND consumed_at IS NULL")
                .bind(timestamp(now)).bind(message_id.to_string()).execute(&mut *tx).await?;
            previous = event.clone();
            consumed.push(event);
        }
        tx.commit().await?;
        Ok(consumed)
    }
}

async fn guidance_spec(tx: &mut SqliteConnection, project_id: Uuid, conversation_id: Uuid, run_id: Uuid) -> Result<(String, RunSpecV4), StoreError> {
    let row = sqlx::query("SELECT status,value_json FROM agent_runs_v4 WHERE run_id=?1 AND project_id=?2 AND conversation_id=?3")
        .bind(run_id.to_string()).bind(project_id.to_string()).bind(conversation_id.to_string()).fetch_optional(&mut *tx).await?
        .ok_or_else(|| StoreError::InvalidInput("guidance run does not belong to this project and conversation".into()))?;
    let value: Value = serde_json::from_str(&row.try_get::<String,_>(1)?)?;
    let spec: RunSpecV4 = serde_json::from_value(value.get("spec").cloned().ok_or_else(|| StoreError::InvalidInput("run has no frozen specification".into()))?)?;
    spec.validate_integrity().map_err(|error| StoreError::InvalidInput(error.to_string()))?;
    if spec.run_id != run_id || spec.project_id != project_id || spec.conversation_id != conversation_id || spec.execution_kind != RunExecutionKindV4::OrdinaryAgent {
        return Err(StoreError::InvalidInput("guidance cannot modify an approved plan or another run".into()));
    }
    Ok((row.try_get(0)?, spec))
}

fn guidance_record(message_id: Uuid, spec: &RunSpecV4, row: sqlx::sqlite::SqliteRow) -> Result<GuidanceRecordV4, StoreError> {
    Ok(GuidanceRecordV4 { message_id, project_id: spec.project_id, conversation_id: spec.conversation_id, run_id: spec.run_id,
        ordinal: row.try_get::<i64,_>(1)? as u64, markdown: row.try_get(2)?,
        accepted_at: from_timestamp(row.try_get(3)?, "guidance accepted timestamp")?,
        consumed_at: row.try_get::<Option<i64>,_>(4)?.map(|value| from_timestamp(value, "guidance consumed timestamp")).transpose()?,
    })
}
