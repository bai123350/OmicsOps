//! Bounded read projections used by the desktop's verified source resolver.
//! These methods never open a path or infer a source from a bare tool call ID.
use crate::{Store, StoreError};
use omicsops_core::workspace::{Conversation, Message};
use omicsops_dto::{JourneyEntry, JourneyPage, JourneyRequest, SourceKind, WorkspaceSourceRef};
use omicsops_protocol::AgentEventV4;
use serde_json::{Value, json};
use sqlx::{Row, Sqlite, Transaction};
use uuid::Uuid;

fn scientific_table(kind: SourceKind) -> Option<&'static str> {
    match kind {
        SourceKind::Dataset => Some("scientific_datasets_v4"),
        SourceKind::Analysis => Some("scientific_analyses_v4"),
        SourceKind::Artifact => Some("scientific_artifacts_v4"),
        SourceKind::Evidence => Some("scientific_evidence_v4"),
        SourceKind::Provenance => Some("scientific_provenance_v4"),
        _ => None,
    }
}

/// Transaction-aware scientific row reader: length is checked before fetching
/// or deserializing payload. The table is selected exclusively from this module's allowlist.
pub(crate) async fn bounded_scientific_value(
    tx: &mut Transaction<'_, Sqlite>,
    project: Uuid,
    kind: SourceKind,
    id: &str,
    bytes: &mut i64,
    truncated: &mut bool,
) -> Result<Option<Value>, StoreError> {
    let table = scientific_table(kind)
        .ok_or_else(|| StoreError::InvalidInput("not a scientific source kind".into()))?;
    let length_sql =
        format!("SELECT length(CAST(value_json AS BLOB)) FROM {table} WHERE project_id=? AND id=?");
    let length: Option<i64> = sqlx::query_scalar(&length_sql)
        .bind(project.to_string())
        .bind(id)
        .fetch_optional(&mut **tx)
        .await?;
    let Some(length) = length else {
        return Ok(None);
    };
    if length > 262_144 || *bytes + length > 4_194_304 {
        *truncated = true;
        return Ok(None);
    }
    let sql = format!("SELECT value_json FROM {table} WHERE project_id=? AND id=?");
    let json: String = sqlx::query_scalar(&sql)
        .bind(project.to_string())
        .bind(id)
        .fetch_one(&mut **tx)
        .await?;
    // Rows cannot change within this SQLite read transaction; retain a defense
    // against malformed lengths or a future query rewrite.
    if json.len() as i64 > 262_144 || *bytes + json.len() as i64 > 4_194_304 {
        *truncated = true;
        return Ok(None);
    }
    let value: Value = serde_json::from_str(&json)?;
    if value.get("project_id").and_then(Value::as_str) != Some(project.to_string().as_str())
        || value.get("id").and_then(Value::as_str) != Some(id)
    {
        return Err(StoreError::InvalidInput(
            "scientific record owner mismatch".into(),
        ));
    }
    *bytes += json.len() as i64;
    Ok(Some(value))
}

/// Discover manifests only through a known analysis and its recorded run.
/// The bounded ID query never deserializes arbitrary project records.
pub(crate) async fn provenance_ids_for_analysis(
    tx: &mut Transaction<'_, Sqlite>,
    project: Uuid,
    analysis: Uuid,
    run: Option<&str>,
    truncated: &mut bool,
) -> Result<Vec<Uuid>, StoreError> {
    let rows = sqlx::query("SELECT id FROM scientific_provenance_v4 WHERE project_id=? AND (analysis_id=? OR run_id=?) ORDER BY id LIMIT 65")
        .bind(project.to_string()).bind(analysis.to_string()).bind(run.unwrap_or(""))
        .fetch_all(&mut **tx).await?;
    if rows.len() > 64 {
        *truncated = true;
    }
    rows.into_iter()
        .take(64)
        .map(|row| {
            let id: String = row.try_get("id")?;
            Uuid::parse_str(&id)
                .map_err(|_| StoreError::InvalidInput("invalid provenance identifier".into()))
        })
        .collect()
}

/// Raw current source projection for a saved reference. This validates project,
/// conversation, run and event sequence in the same transaction as its caller.
pub(crate) async fn publication_source_value(
    tx: &mut Transaction<'_, Sqlite>,
    project: Uuid,
    source: &WorkspaceSourceRef,
    bytes: &mut i64,
    truncated: &mut bool,
) -> Result<Option<Value>, StoreError> {
    if source.project_id != project || source.id.is_empty() || source.id.len() > 512 {
        return Err(StoreError::InvalidInput(
            "saved source scope is invalid".into(),
        ));
    }
    if let Some(conversation) = source.conversation_id {
        let exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM conversation_records WHERE project_id=? AND frame_id=?)",
        )
        .bind(project.to_string())
        .bind(conversation.to_string())
        .fetch_one(&mut **tx)
        .await?;
        if !exists {
            return Ok(None);
        }
    }
    if let Some(run) = source
        .run_id
        .filter(|_| source.kind != SourceKind::LegacyArtifact)
    {
        let conversation: Option<String> = sqlx::query_scalar(
            "SELECT conversation_id FROM agent_runs_v4 WHERE project_id=? AND run_id=?",
        )
        .bind(project.to_string())
        .bind(run.to_string())
        .fetch_optional(&mut **tx)
        .await?;
        let Some(conversation) = conversation else {
            return Ok(None);
        };
        if source
            .conversation_id
            .is_some_and(|id| id.to_string() != conversation)
        {
            return Ok(None);
        }
    }
    match source.kind {
        SourceKind::Message => {
            if source.run_id.is_some() || source.sequence.is_some() || source.event_hash.is_some() {
                return Ok(None);
            }
            let row = sqlx::query("SELECT id,project_id,conversation_id,seq,role,content,ts,project_id FROM messages WHERE project_id=? AND id=?")
                .bind(project.to_string()).bind(&source.id).fetch_optional(&mut **tx).await?;
            let Some(row) = row else {
                return Ok(None);
            };
            if source.conversation_id.is_some_and(|id| {
                row.try_get::<String, _>("conversation_id").ok().as_deref()
                    != Some(id.to_string().as_str())
            }) {
                return Ok(None);
            }
            Ok(Some(serde_json::to_value(crate::message_from_row(row)?)?))
        }
        SourceKind::Tool => {
            let (Some(run), Some(sequence)) = (source.run_id, source.sequence) else {
                return Ok(None);
            };
            let sequence = i64::try_from(sequence)
                .map_err(|_| StoreError::InvalidInput("event sequence is too large".into()))?;
            let row = sqlx::query("SELECT e.value_json,e.event_hash,e.conversation_id FROM agent_events_v4 e JOIN agent_runs_v4 r ON r.run_id=e.run_id AND r.project_id=e.project_id AND r.conversation_id=e.conversation_id WHERE e.project_id=? AND e.run_id=? AND e.sequence=?")
                .bind(project.to_string()).bind(run.to_string()).bind(sequence).fetch_optional(&mut **tx).await?;
            let Some(row) = row else {
                return Ok(None);
            };
            if source.event_hash.as_deref().is_some_and(|hash| {
                row.try_get::<String, _>("event_hash").ok().as_deref() != Some(hash)
            }) {
                return Ok(None);
            }
            if source.conversation_id.is_some_and(|id| {
                row.try_get::<String, _>("conversation_id").ok().as_deref()
                    != Some(id.to_string().as_str())
            }) {
                return Ok(None);
            }
            let json: String = row.try_get("value_json")?;
            if json.len() > 262_144 {
                *truncated = true;
                return Ok(None);
            }
            Ok(Some(serde_json::from_str(&json)?))
        }
        SourceKind::Run => {
            if source.run_id.is_some_and(|id| id.to_string() != source.id) {
                return Ok(None);
            }
            let row = sqlx::query("SELECT run_id,project_id,conversation_id,status FROM agent_runs_v4 WHERE project_id=? AND run_id=?")
                .bind(project.to_string()).bind(&source.id).fetch_optional(&mut **tx).await?;
            let Some(row) = row else {
                return Ok(None);
            };
            if source.conversation_id.is_some_and(|id| {
                row.try_get::<String, _>("conversation_id").ok().as_deref()
                    != Some(id.to_string().as_str())
            }) {
                return Ok(None);
            }
            Ok(Some(
                json!({"id":row.try_get::<String,_>("run_id")?,"project_id":project,"conversation_id":row.try_get::<String,_>("conversation_id")?,"status":row.try_get::<String,_>("status")?}),
            ))
        }
        SourceKind::LegacyArtifact => {
            let row = sqlx::query("SELECT id,project_id,source_run_id,filename,remote_path,content_type,size_bytes,sha256,verified,created_at FROM artifacts WHERE project_id=? AND id=?")
                .bind(project.to_string()).bind(&source.id).fetch_optional(&mut **tx).await?;
            row.map(|row| Ok(serde_json::to_value(crate::artifact_from_row(row)?)?))
                .transpose()
        }
        kind if scientific_table(kind).is_some() => {
            if source.sequence.is_some()
                || source.event_hash.is_some()
                || source.start.is_some()
                || source.end.is_some()
                || source.content_sha256.is_some()
            {
                return Ok(None);
            }
            let value =
                bounded_scientific_value(tx, project, kind, &source.id, bytes, truncated).await?;
            let Some(value) = value else {
                return Ok(None);
            };
            let derived_run = match kind {
                SourceKind::Analysis | SourceKind::Provenance => value
                    .get("run_id")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                SourceKind::Artifact
                    if source.run_id.is_some() || source.conversation_id.is_some() =>
                {
                    let producer = value
                        .get("producer_analysis_id")
                        .and_then(Value::as_str)
                        .unwrap_or("");
                    let producer_value = bounded_scientific_value(
                        tx,
                        project,
                        SourceKind::Analysis,
                        producer,
                        bytes,
                        truncated,
                    )
                    .await?;
                    producer_value
                        .and_then(|v| v.get("run_id").and_then(Value::as_str).map(str::to_owned))
                }
                _ => None,
            };
            if source
                .run_id
                .is_some_and(|id| derived_run.as_deref() != Some(id.to_string().as_str()))
            {
                return Ok(None);
            }
            if let Some(conversation) = source.conversation_id {
                let Some(run) = derived_run else {
                    return Ok(None);
                };
                let owner: Option<String> = sqlx::query_scalar(
                    "SELECT conversation_id FROM agent_runs_v4 WHERE project_id=? AND run_id=?",
                )
                .bind(project.to_string())
                .bind(run)
                .fetch_optional(&mut **tx)
                .await?;
                if owner.as_deref() != Some(conversation.to_string().as_str()) {
                    return Ok(None);
                }
            }
            Ok(Some(value))
        }
        _ => Ok(None),
    }
}

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
