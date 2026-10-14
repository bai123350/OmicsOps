//! Reversible project-owned presentation preferences and immutable request receipts.
use crate::{Store, StoreError, conversation_from_row, from_timestamp, parse_uuid};
use chrono::Utc;
use omicsops_core::{redaction::redact_secrets, workspace::Conversation};
use omicsops_dto::*;
use sha2::{Digest, Sha256};
use sqlx::{Row, SqliteConnection, sqlite::SqliteRow};
use uuid::Uuid;

const OPERATION: &str = "set_conversation_visibility";
const VISIBILITY_JOIN: &str = "LEFT JOIN conversation_visibility_v4 v ON v.conversation_id=c.frame_id AND v.project_id=c.project_id";

impl Store {
    /// Visible discovery; internal ALL queries and exact source resolution stay unchanged.
    pub async fn workspace_conversations_for_presentation(
        &self,
        project_id: Uuid,
    ) -> Result<Vec<Conversation>, StoreError> {
        let rows=sqlx::query(&format!("SELECT c.frame_id,c.project_id,c.title,c.status,c.model_profile_id,c.created_at,c.updated_at FROM conversation_records c {VISIBILITY_JOIN} WHERE c.project_id=? AND COALESCE(v.visibility,'visible')='visible' ORDER BY c.updated_at DESC,c.frame_id ASC"))
            .bind(project_id.to_string()).fetch_all(&self.pool).await?;
        rows.into_iter().map(conversation_from_row).collect()
    }

    /// Default entry ranks genuine user turns, including attachment-only turns.
    /// Assistant/tool/system activity does not outrank a more recent user turn.
    pub async fn workspace_latest_visible_conversation(
        &self,
        project_id: Uuid,
    ) -> Result<Option<Conversation>, StoreError> {
        let row=sqlx::query(&format!("SELECT c.frame_id,c.project_id,c.title,c.status,c.model_profile_id,c.created_at,c.updated_at FROM conversation_records c {VISIBILITY_JOIN}
            WHERE c.project_id=? AND COALESCE(v.visibility,'visible')='visible'
            AND EXISTS(SELECT 1 FROM messages m WHERE m.frame_id=c.frame_id AND m.conversation_id=c.frame_id AND m.project_id=c.project_id AND m.role='user')
            ORDER BY (SELECT MAX(m.ts) FROM messages m WHERE m.frame_id=c.frame_id AND m.conversation_id=c.frame_id AND m.project_id=c.project_id AND m.role='user') DESC,c.frame_id ASC LIMIT 1"))
            .bind(project_id.to_string()).fetch_optional(&self.pool).await?;
        row.map(conversation_from_row).transpose()
    }

    /// Exact owner/id lookup. Missing preference means Visible only for an existing owner.
    pub async fn workspace_get_conversation_visibility(
        &self,
        project_id: Uuid,
        conversation_id: Uuid,
    ) -> Result<ConversationVisibilityRecordV1, StoreError> {
        let mut tx = self.pool.begin().await?;
        let state = current_state(&mut tx, project_id, conversation_id).await?;
        tx.commit().await?;
        Ok(state)
    }

    /// Filter public titles in SQL before pagination. The safe title projection is
    /// O(total eligible title bytes), rather than page-bounded; bodies are never read.
    pub async fn workspace_list_conversation_visibility(
        &self,
        request: &ConversationVisibilityListRequestV1,
    ) -> Result<ConversationVisibilityPageV1, StoreError> {
        if request.query.chars().count() > 200
            || request.query.len() > 1024
            || !(1..=100).contains(&request.limit)
            || request.offset > 10_000
        {
            return Err(invalid("invalid_input"));
        }
        let filter = match request.visibility {
            ConversationVisibilityFilterV1::Visible => "visible",
            ConversationVisibilityFilterV1::Shelved => "shelved",
            ConversationVisibilityFilterV1::All => "all",
        };
        let mut tx = self.pool.begin().await?;
        ensure_project(&mut tx, request.project_id).await?;
        let titles=sqlx::query(&format!("SELECT c.frame_id,c.title FROM conversation_records c {VISIBILITY_JOIN} WHERE c.project_id=? AND (?='all' OR COALESCE(v.visibility,'visible')=?)"))
            .bind(request.project_id.to_string()).bind(filter).bind(filter).fetch_all(&mut *tx).await?;
        let public_titles: Vec<(String, String)> = titles
            .into_iter()
            .map(|row| {
                Ok((
                    row.try_get("frame_id")?,
                    redact_secrets(row.try_get::<&str, _>("title")?, &[] as &[&str]),
                ))
            })
            .collect::<Result<_, StoreError>>()?;
        // SQL uses only redacted titles. Searching a secret cannot disclose which
        // conversation stored it, even indirectly through pagination/counts.
        let titles_json = serde_json::to_string(&public_titles)?;
        let rows=sqlx::query(&format!("WITH public_titles AS (SELECT json_extract(value,'$[0]') AS id,json_extract(value,'$[1]') AS title FROM json_each(?1))
            SELECT c.frame_id,c.project_id,v.visibility,v.revision,v.changed_at,p.title AS public_title,c.status,c.created_at,c.updated_at
            FROM conversation_records c {VISIBILITY_JOIN} JOIN public_titles p ON p.id=c.frame_id
            WHERE c.project_id=?2 AND (?3='all' OR COALESCE(v.visibility,'visible')=?3) AND instr(lower(p.title),lower(?4))>0
            ORDER BY c.updated_at DESC,c.frame_id ASC LIMIT ?5 OFFSET ?6"))
            .bind(titles_json).bind(request.project_id.to_string()).bind(filter).bind(&request.query).bind(i64::from(request.limit)+1).bind(i64::from(request.offset)).fetch_all(&mut *tx).await?;
        let more = rows.len() > request.limit as usize;
        let items = rows
            .into_iter()
            .take(request.limit as usize)
            .map(|row| {
                // Validate the existing core enum, rather than defining another status.
                let status: omicsops_core::workspace::ConversationStatus = crate::parse_json_enum(
                    row.try_get::<&str, _>("status")?,
                    "conversation status",
                )?;
                Ok(ConversationVisibilityListItemV1 {
                    state: state_from_row(&row)?,
                    title: row.try_get("public_title")?,
                    conversation_status: crate::enum_string(&status)?,
                    conversation_created_at: utc_string(row.try_get("created_at")?)?,
                    conversation_updated_at: utc_string(row.try_get("updated_at")?)?,
                })
            })
            .collect::<Result<Vec<_>, StoreError>>()?;
        tx.commit().await?;
        Ok(ConversationVisibilityPageV1 {
            items,
            next_offset: more.then_some(request.offset + request.limit),
        })
    }

    pub async fn workspace_set_conversation_visibility(
        &self,
        request: &SetConversationVisibilityRequestV1,
    ) -> Result<ConversationVisibilityReceiptV1, StoreError> {
        let digest = hex::encode(Sha256::digest(serde_json::to_vec(request)?));
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let prior=sqlx::query("SELECT operation,request_sha256,result_json FROM workspace_request_ledger WHERE request_id=?")
            .bind(request.request_id.to_string()).fetch_optional(&mut *tx).await?;
        if let Some(prior) = prior {
            if prior.try_get::<&str, _>("operation")? != OPERATION
                || prior.try_get::<&str, _>("request_sha256")? != digest
            {
                return Err(invalid("request_conflict"));
            }
            let receipt = serde_json::from_str(prior.try_get("result_json")?)?;
            tx.commit().await?;
            return Ok(receipt);
        }
        if request.expected_revision > i64::MAX as u64 {
            return Err(invalid("invalid_input"));
        }
        let mut state = current_state(&mut tx, request.project_id, request.conversation_id).await?;
        if state.revision != request.expected_revision {
            return Err(invalid("revision_conflict"));
        }
        let changed = state.visibility != request.visibility;
        if changed {
            let revision = state
                .revision
                .checked_add(1)
                .filter(|n| *n <= i64::MAX as u64)
                .ok_or_else(|| invalid("invalid_input"))?;
            let now = Utc::now().timestamp_millis();
            sqlx::query("INSERT INTO conversation_visibility_v4(conversation_id,project_id,visibility,revision,changed_at) VALUES(?,?,?,?,?) ON CONFLICT(conversation_id) DO UPDATE SET visibility=excluded.visibility,revision=excluded.revision,changed_at=excluded.changed_at")
                .bind(request.conversation_id.to_string()).bind(request.project_id.to_string()).bind(visibility_name(request.visibility)).bind(revision as i64).bind(now).execute(&mut *tx).await?;
            state.visibility = request.visibility;
            state.revision = revision;
            state.changed_at = Some(utc_string(now)?);
        }
        let receipt = ConversationVisibilityReceiptV1 {
            request_id: request.request_id,
            state,
            changed,
        };
        // Only identities, safe state and a request hash; never raw request/title/body.
        sqlx::query("INSERT INTO workspace_request_ledger(request_id,operation,request_sha256,result_json,created_at) VALUES(?,?,?,?,?)")
            .bind(request.request_id.to_string()).bind(OPERATION).bind(digest).bind(serde_json::to_string(&receipt)?).bind(Utc::now().timestamp_millis()).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(receipt)
    }
}

async fn ensure_project(
    connection: &mut SqliteConnection,
    project_id: Uuid,
) -> Result<(), StoreError> {
    let exists: i64 = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM projects WHERE id=?)")
        .bind(project_id.to_string())
        .fetch_one(connection)
        .await?;
    if exists == 0 {
        return Err(invalid("not_found"));
    }
    Ok(())
}
async fn current_state(
    connection: &mut SqliteConnection,
    project_id: Uuid,
    conversation_id: Uuid,
) -> Result<ConversationVisibilityRecordV1, StoreError> {
    ensure_project(connection, project_id).await?;
    let row=sqlx::query(&format!("SELECT c.frame_id,c.project_id,v.visibility,v.revision,v.changed_at FROM conversation_records c {VISIBILITY_JOIN} WHERE c.frame_id=?"))
        .bind(conversation_id.to_string()).fetch_optional(connection).await?.ok_or_else(||invalid("not_found"))?;
    if row.try_get::<&str, _>("project_id")? != project_id.to_string() {
        return Err(invalid("ownership_mismatch"));
    }
    state_from_row(&row)
}
fn state_from_row(row: &SqliteRow) -> Result<ConversationVisibilityRecordV1, StoreError> {
    let visibility = match row.try_get::<Option<&str>, _>("visibility")? {
        None | Some("visible") => ConversationVisibilityV1::Visible,
        Some("shelved") => ConversationVisibilityV1::Shelved,
        _ => return Err(invalid("invalid_input")),
    };
    let revision = row
        .try_get::<Option<i64>, _>("revision")?
        .unwrap_or(0)
        .try_into()
        .map_err(|_| invalid("invalid_input"))?;
    Ok(ConversationVisibilityRecordV1 {
        project_id: parse_uuid(row.try_get::<&str, _>("project_id")?, "visibility project")?,
        conversation_id: parse_uuid(
            row.try_get::<&str, _>("frame_id")?,
            "visibility conversation",
        )?,
        visibility,
        revision,
        changed_at: row
            .try_get::<Option<i64>, _>("changed_at")?
            .map(utc_string)
            .transpose()?,
    })
}
fn visibility_name(visibility: ConversationVisibilityV1) -> &'static str {
    match visibility {
        ConversationVisibilityV1::Visible => "visible",
        ConversationVisibilityV1::Shelved => "shelved",
    }
}
fn utc_string(ts: i64) -> Result<String, StoreError> {
    Ok(from_timestamp(ts, "conversation visibility timestamp")?
        .to_rfc3339_opts(chrono::SecondsFormat::AutoSi, true))
}
fn invalid(tag: &str) -> StoreError {
    StoreError::InvalidInput(format!("conversation_visibility_{tag}"))
}
