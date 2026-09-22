//! Bounded read projections used by the desktop's verified source resolver.
//! These methods never open a path or infer a source from a bare tool call ID.
use crate::{Store, StoreError};
use omicsops_core::workspace::{Conversation, Message};
use omicsops_dto::{JourneyEntry, JourneyPage, JourneyRequest, SourceKind, WorkspaceSourceRef};
use omicsops_protocol::AgentEventV4;
use serde_json::{Value, json};
use sqlx::Row;
use uuid::Uuid;

impl Store {
    pub async fn workspace_source_conversation(
        &self,
        project: Uuid,
        id: Uuid,
    ) -> Result<Option<Conversation>, StoreError> {
        sqlx::query("SELECT frame_id,project_id,title,status,model_profile_id,created_at,updated_at FROM conversation_records WHERE project_id=?1 AND frame_id=?2")
            .bind(project.to_string()).bind(id.to_string()).fetch_optional(&self.pool).await?
            .map(crate::conversation_from_row).transpose()
    }

    pub async fn workspace_source_message(
        &self,
        project: Uuid,
        id: Uuid,
    ) -> Result<Option<Message>, StoreError> {
        sqlx::query("SELECT id,project_id,conversation_id,seq,role,content,ts,project_id FROM messages WHERE project_id=?1 AND id=?2")
            .bind(project.to_string()).bind(id.to_string()).fetch_optional(&self.pool).await?
            .map(crate::message_from_row).transpose()
    }

    pub async fn workspace_source_event(
        &self,
        project: Uuid,
        run: Uuid,
        sequence: u64,
    ) -> Result<Option<AgentEventV4>, StoreError> {
        let sequence = i64::try_from(sequence)
            .map_err(|_| StoreError::InvalidInput("event sequence is too large".into()))?;
        let row = sqlx::query("SELECT e.value_json FROM agent_events_v4 e JOIN agent_runs_v4 r ON r.run_id=e.run_id AND r.project_id=e.project_id AND r.conversation_id=e.conversation_id WHERE e.project_id=?1 AND e.run_id=?2 AND e.sequence=?3")
            .bind(project.to_string()).bind(run.to_string()).bind(sequence).fetch_optional(&self.pool).await?;
        row.map(|row| Ok(serde_json::from_str(&row.try_get::<String, _>(0)?)?))
            .transpose()
    }

    pub async fn workspace_run_conversation(
        &self,
        project: Uuid,
        run: Uuid,
    ) -> Result<Option<Uuid>, StoreError> {
        let value: Option<String> = sqlx::query_scalar(
            "SELECT conversation_id FROM agent_runs_v4 WHERE project_id=?1 AND run_id=?2",
        )
        .bind(project.to_string())
        .bind(run.to_string())
        .fetch_optional(&self.pool)
        .await?;
        value
            .map(|value| crate::parse_uuid(value, "run conversation"))
            .transpose()
    }

    pub async fn workspace_source_record(
        &self,
        project: Uuid,
        kind: SourceKind,
        id: &str,
    ) -> Result<Option<Value>, StoreError> {
        if kind == SourceKind::LegacyArtifact {
            let row = sqlx::query("SELECT id,project_id,source_run_id,filename,remote_path,content_type,size_bytes,sha256,verified,created_at FROM artifacts WHERE project_id=?1 AND id=?2")
                .bind(project.to_string()).bind(id).fetch_optional(&self.pool).await?;
            return row
                .map(|row| Ok(serde_json::to_value(crate::artifact_from_row(row)?)?))
                .transpose();
        }
        if kind == SourceKind::Run {
            // Run timestamps are sourced from persisted events, never the legacy zero defaults.
            let row = sqlx::query("SELECT r.run_id,r.project_id,r.conversation_id,r.status,
                (SELECT json_extract(e.value_json,'$.occurred_at') FROM agent_events_v4 e WHERE e.run_id=r.run_id ORDER BY e.sequence LIMIT 1),
                (SELECT json_extract(e.value_json,'$.occurred_at') FROM agent_events_v4 e WHERE e.run_id=r.run_id ORDER BY e.sequence DESC LIMIT 1)
                FROM agent_runs_v4 r WHERE r.project_id=?1 AND r.run_id=?2")
                .bind(project.to_string()).bind(id).fetch_optional(&self.pool).await?;
            return row.map(|row| Ok(json!({"id":row.try_get::<String,_>(0)?,"project_id":row.try_get::<String,_>(1)?,
                "conversation_id":row.try_get::<String,_>(2)?,"status":row.try_get::<String,_>(3)?,
                "started_at":row.try_get::<Option<String>,_>(4)?,"last_event_at":row.try_get::<Option<String>,_>(5)?}))).transpose();
        }
        let table = match kind {
            SourceKind::Dataset => "scientific_datasets_v4",
            SourceKind::Analysis => "scientific_analyses_v4",
            SourceKind::Artifact => "scientific_artifacts_v4",
            SourceKind::Evidence => "scientific_evidence_v4",
            SourceKind::Provenance => "scientific_provenance_v4",
            _ => {
                return Err(StoreError::InvalidInput(
                    "source kind has no record projection".into(),
                ));
            }
        };
        // The table is selected only from the allowlist above; all user values are bound.
        let row = sqlx::query(&format!(
            "SELECT value_json FROM {table} WHERE project_id=?1 AND id=?2"
        ))
        .bind(project.to_string())
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        row.map(|row| Ok(serde_json::from_str(&row.try_get::<String, _>(0)?)?))
            .transpose()
    }

    pub async fn workspace_journey(
        &self,
        request: &JourneyRequest,
    ) -> Result<JourneyPage, StoreError> {
        if request.query.len() > 512
            || request
                .status
                .as_ref()
                .is_some_and(|status| status.len() > 512)
            || request.offset > 1_000_000
        {
            return Err(StoreError::InvalidInput(
                "journey query exceeds limits".into(),
            ));
        }
        let kind = request.kind.as_ref().map(crate::enum_string).transpose()?;
        let query = request
            .query
            .trim()
            .replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_");
        let limit = request.limit.clamp(1, 100);
        let status = request
            .status
            .as_deref()
            .map(str::trim)
            .filter(|status| !status.is_empty());
        let rows = sqlx::query(r#"
            WITH sources(kind,id,run_id,title,status,occurred_at,summary) AS (
                SELECT 'dataset',id,NULL,json_extract(value_json,'$.relative_path'),
                    CASE active WHEN 1 THEN 'active' ELSE 'superseded' END,
                    json_extract(value_json,'$.registered_at'),json_extract(value_json,'$.modality') FROM scientific_datasets_v4 WHERE project_id=?1
                UNION ALL SELECT 'analysis',id,json_extract(value_json,'$.run_id'),json_extract(value_json,'$.analysis_type'),status,
                    json_extract(value_json,'$.started_at'),json_extract(value_json,'$.method') FROM scientific_analyses_v4 WHERE project_id=?1
                UNION ALL SELECT 'artifact',a.id,json_extract(p.value_json,'$.run_id'),json_extract(a.value_json,'$.relative_path'),
                    CASE a.valid WHEN 1 THEN 'valid' ELSE 'invalid' END,json_extract(a.value_json,'$.created_at'),json_extract(a.value_json,'$.artifact_type')
                    FROM scientific_artifacts_v4 a LEFT JOIN scientific_analyses_v4 p ON p.id=a.producer_analysis_id AND p.project_id=a.project_id WHERE a.project_id=?1
                UNION ALL SELECT 'evidence',id,NULL,json_extract(value_json,'$.claim'),CASE valid WHEN 1 THEN 'valid' ELSE 'invalid' END,
                    json_extract(value_json,'$.recorded_at'),COALESCE(json_extract(value_json,'$.invalid_reason'),json_extract(value_json,'$.strength')) FROM scientific_evidence_v4 WHERE project_id=?1
                UNION ALL SELECT 'provenance',id,run_id,'Provenance',CASE complete WHEN 1 THEN 'complete' ELSE 'incomplete' END,
                    json_extract(value_json,'$.recorded_at'),json_extract(value_json,'$.environment') FROM scientific_provenance_v4 WHERE project_id=?1
                UNION ALL SELECT 'legacy_artifact',id,NULL,filename,CASE verified WHEN 1 THEN 'verified' ELSE 'unverified' END,
                    strftime('%Y-%m-%dT%H:%M:%fZ',created_at/1000.0,'unixepoch'),content_type FROM artifacts WHERE project_id=?1
                UNION ALL SELECT 'run',r.run_id,r.run_id,'Agent run',r.status,
                    (SELECT json_extract(e.value_json,'$.occurred_at') FROM agent_events_v4 e WHERE e.run_id=r.run_id ORDER BY e.sequence DESC LIMIT 1),
                    'Agent run status; scientific verification is recorded separately' FROM agent_runs_v4 r WHERE r.project_id=?1
            ) SELECT s.kind,s.id,r.run_id,r.conversation_id,substr(COALESCE(s.title,''),1,180),s.status,s.occurred_at,substr(COALESCE(s.summary,''),1,600)
                FROM sources s LEFT JOIN agent_runs_v4 r ON r.run_id=s.run_id AND r.project_id=?1
                WHERE s.occurred_at IS NOT NULL AND julianday(s.occurred_at) IS NOT NULL
                AND (?2 IS NULL OR s.kind=?2) AND (?3='%%' OR (COALESCE(s.title,'') || ' ' || COALESCE(s.summary,'') || ' ' || s.status) LIKE ?3 ESCAPE '\')
                AND (?6 IS NULL OR s.status=?6 COLLATE NOCASE)
                ORDER BY julianday(s.occurred_at) DESC,s.kind,s.id LIMIT ?4 OFFSET ?5
        "#).bind(request.project_id.to_string()).bind(kind).bind(format!("%{query}%"))
            .bind(i64::from(limit) + 1).bind(i64::from(request.offset)).bind(status).fetch_all(&self.pool).await?;
        let next_offset = (rows.len() > limit as usize).then_some(request.offset + limit);
        let entries = rows
            .into_iter()
            .take(limit as usize)
            .map(|row| {
                let kind = crate::parse_json_enum(&row.try_get::<String, _>(0)?, "source kind")?;
                Ok(JourneyEntry {
                    source: WorkspaceSourceRef {
                        project_id: request.project_id,
                        kind,
                        id: row.try_get(1)?,
                        run_id: row
                            .try_get::<Option<String>, _>(2)?
                            .map(|v| crate::parse_uuid(v, "source run"))
                            .transpose()?,
                        conversation_id: row
                            .try_get::<Option<String>, _>(3)?
                            .map(|v| crate::parse_uuid(v, "source conversation"))
                            .transpose()?,
                        sequence: None,
                        event_hash: None,
                        content_sha256: None,
                        start: None,
                        end: None,
                    },
                    title: row.try_get(4)?,
                    status: row.try_get(5)?,
                    occurred_at: row.try_get::<Option<String>, _>(6)?.unwrap_or_default(),
                    summary: row.try_get(7)?,
                })
            })
            .collect::<Result<_, StoreError>>()?;
        Ok(JourneyPage {
            entries,
            next_offset,
        })
    }
}
