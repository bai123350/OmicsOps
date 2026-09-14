//! Durable, read-only side-chat turns.
//!
//! Side-chat state is intentionally separate from the primary Agent
//! lifecycle.  A side turn may coexist with a running primary run, plan, or
//! retrospective review; it only owns its own request row and lease in the
//! native layer.  The Store validates the frozen source snapshot and performs
//! all lifecycle transitions with scoped compare-and-set updates.

use std::collections::{BTreeMap, HashMap, HashSet};

use chrono::{DateTime, Utc};
use omicsops_dto::{
    ComposerReference, SIDE_CHAT_MAX_ANSWER_BYTES, SIDE_CHAT_MAX_ATTACHMENTS,
    SIDE_CHAT_MAX_CITATIONS, SIDE_CHAT_MAX_EVENT_HEADS, SIDE_CHAT_MAX_LABEL_BYTES,
    SIDE_CHAT_MAX_MODEL_LABEL_BYTES, SIDE_CHAT_MAX_PER_PROJECT, SIDE_CHAT_MAX_QUESTION_BYTES,
    SIDE_CHAT_MAX_REFERENCES, SIDE_CHAT_MAX_ROLE_BYTES, SIDE_CHAT_MAX_SOURCE_ID_BYTES,
    SIDE_CHAT_MAX_SOURCE_SNAPSHOT_BYTES, SIDE_CHAT_MAX_SOURCE_TEXT_BYTES, SIDE_CHAT_MAX_SOURCES,
    SideChatBeginResultV4, SideChatEventHeadV4, SideChatFailureCodeV4, SideChatTurnStatusV4,
    SideChatTurnV4, side_chat_request_hash, side_chat_source_snapshot_hash,
    side_chat_source_snapshot_value,
};
use omicsops_protocol::{AgentEventKindV4, AgentEventV4, deserialize_event_chain_v4};
use sha2::{Digest, Sha256};
use sqlx::{Row, SqliteConnection};
use uuid::Uuid;

use super::{
    Store, StoreError, ensure_conversation_owner_executor, from_timestamp, parse_uuid, timestamp,
};

const SIDE_CHAT_COLUMNS: &str =
    "request_id,project_id,conversation_id,request_hash,status,value_json,created_at,updated_at";
const DEFAULT_SIDE_CHAT_LIST_LIMIT: u32 = 50;
const MAX_SIDE_CHAT_LIST_LIMIT: u32 = 100;

impl Store {
    /// Atomically accept one side turn.  An existing request ID is checked
    /// before source/profile material is re-read, allowing a lost response to
    /// reconcile without re-dispatching or observing mutable source state.
    pub async fn begin_side_chat_turn(
        &self,
        turn: SideChatTurnV4,
    ) -> Result<SideChatBeginResultV4, StoreError> {
        validate_request_identity(&turn)?;
        let request_hash = side_chat_request_hash(&turn)?;
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let result: Result<SideChatBeginResultV4, StoreError> = async {
            if let Some((stored_hash, existing)) = load_side_chat_in_tx(&mut *tx, turn.id).await? {
                if existing.project_id != turn.project_id
                    || existing.conversation_id != turn.conversation_id
                {
                    return Err(StoreError::InvalidInput(
                        "side chat request ID belongs to another scope".into(),
                    ));
                }
                if !stored_hash.eq_ignore_ascii_case(&request_hash) {
                    return Err(StoreError::InvalidInput(
                        "side chat request ID was reused with a different payload".into(),
                    ));
                }
                return Ok(SideChatBeginResultV4 {
                    turn: existing,
                    acquired: false,
                });
            }

            ensure_conversation_owner_executor(&mut *tx, turn.project_id, turn.conversation_id)
                .await?;
            validate_initial_turn(&turn)?;
            validate_parent_request(&mut *tx, &turn).await?;

            let active: i64 = sqlx::query_scalar(
                "SELECT EXISTS(
                     SELECT 1 FROM side_chat_turns_v4
                     WHERE project_id=?1 AND conversation_id=?2
                       AND status IN ('queued','running')
                 )",
            )
            .bind(turn.project_id.to_string())
            .bind(turn.conversation_id.to_string())
            .fetch_one(&mut *tx)
            .await?;
            if active != 0 {
                return Err(StoreError::InvalidInput(
                    "conversation already has an active side chat turn".into(),
                ));
            }

            let project_count: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM side_chat_turns_v4 WHERE project_id=?1",
            )
            .bind(turn.project_id.to_string())
            .fetch_one(&mut *tx)
            .await?;
            if project_count >= SIDE_CHAT_MAX_PER_PROJECT as i64 {
                return Err(StoreError::InvalidInput(format!(
                    "side chat project cap has been reached (maximum {SIDE_CHAT_MAX_PER_PROJECT})"
                )));
            }

            validate_source_snapshot_in_tx(&mut *tx, &turn).await?;
            let value_json = serde_json::to_string(&turn)?;
            sqlx::query(
                "INSERT INTO side_chat_turns_v4
                 (request_id,project_id,conversation_id,request_hash,status,value_json,created_at,updated_at)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
            )
            .bind(turn.id.to_string())
            .bind(turn.project_id.to_string())
            .bind(turn.conversation_id.to_string())
            .bind(&request_hash)
            .bind(status_string(turn.status))
            .bind(value_json)
            .bind(timestamp(turn.created_at))
            .bind(timestamp(turn.updated_at))
            .execute(&mut *tx)
            .await?;
            Ok(SideChatBeginResultV4 {
                turn,
                acquired: true,
            })
        }
        .await;
        match result {
            Ok(value) => {
                tx.commit().await?;
                Ok(value)
            }
            Err(error) => {
                let _ = tx.rollback().await;
                Err(error)
            }
        }
    }

    /// Mark an accepted turn as actively dispatching. This is a scoped CAS;
    /// terminal or already-running rows are never reopened.
    pub async fn mark_side_chat_turn_running(
        &self,
        project_id: Uuid,
        conversation_id: Uuid,
        request_id: Uuid,
    ) -> Result<SideChatTurnV4, StoreError> {
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let result = async {
            ensure_conversation_owner_executor(&mut *tx, project_id, conversation_id).await?;
            let (current_hash, current) = load_side_chat_in_tx(&mut *tx, request_id)
                .await?
                .ok_or_else(|| StoreError::InvalidInput("side chat turn was not found".into()))?;
            let _ = current_hash;
            ensure_turn_scope(&current, project_id, conversation_id)?;
            if current.status == SideChatTurnStatusV4::Running {
                return Ok(current);
            }
            if current.status != SideChatTurnStatusV4::Queued {
                return Err(StoreError::InvalidInput(
                    "side chat turn is no longer queued".into(),
                ));
            }
            let mut running = current;
            running.status = SideChatTurnStatusV4::Running;
            running.updated_at = Utc::now();
            let updated = sqlx::query(
                "UPDATE side_chat_turns_v4
                 SET status='running',value_json=?1,updated_at=?2
                 WHERE request_id=?3 AND project_id=?4 AND conversation_id=?5 AND status='queued'",
            )
            .bind(serde_json::to_string(&running)?)
            .bind(timestamp(running.updated_at))
            .bind(request_id.to_string())
            .bind(project_id.to_string())
            .bind(conversation_id.to_string())
            .execute(&mut *tx)
            .await?;
            if updated.rows_affected() != 1 {
                return Err(StoreError::InvalidInput(
                    "side chat turn changed before dispatch could be recorded".into(),
                ));
            }
            Ok(running)
        }
        .await;
        match result {
            Ok(value) => {
                tx.commit().await?;
                Ok(value)
            }
            Err(error) => {
                let _ = tx.rollback().await;
                Err(error)
            }
        }
    }

    pub async fn get_side_chat_turn(
        &self,
        project_id: Uuid,
        conversation_id: Uuid,
        request_id: Uuid,
    ) -> Result<Option<SideChatTurnV4>, StoreError> {
        let mut tx = self.pool.begin().await?;
        ensure_conversation_owner_executor(&mut *tx, project_id, conversation_id).await?;
        let value = match load_side_chat_in_tx(&mut *tx, request_id).await? {
            Some((_, turn)) => {
                ensure_turn_scope(&turn, project_id, conversation_id)?;
                Some(turn)
            }
            None => None,
        };
        tx.commit().await?;
        Ok(value)
    }

    /// List newest turns using a stable `(created_at, request_id)` cursor.
    /// The host clamps the requested limit to the bounded UI page size.
    pub async fn list_side_chat_turns(
        &self,
        project_id: Uuid,
        conversation_id: Uuid,
        limit: Option<u32>,
        before_created_at: Option<&str>,
        before_request_id: Option<Uuid>,
    ) -> Result<Vec<SideChatTurnV4>, StoreError> {
        let limit = limit
            .unwrap_or(DEFAULT_SIDE_CHAT_LIST_LIMIT)
            .clamp(1, MAX_SIDE_CHAT_LIST_LIMIT);
        let before_created_at = before_created_at.map(parse_cursor_timestamp).transpose()?;
        if before_request_id.is_some() && before_created_at.is_none() {
            return Err(StoreError::InvalidInput(
                "side chat cursor request ID requires created_at".into(),
            ));
        }

        let mut tx = self.pool.begin().await?;
        ensure_conversation_owner_executor(&mut *tx, project_id, conversation_id).await?;
        let rows = match (before_created_at, before_request_id) {
            (Some(created_at), Some(request_id)) => {
                sqlx::query(&format!(
                    "SELECT {SIDE_CHAT_COLUMNS} FROM side_chat_turns_v4
                 WHERE project_id=?1 AND conversation_id=?2
                   AND (created_at < ?3 OR (created_at = ?3 AND request_id < ?4))
                 ORDER BY created_at DESC,request_id DESC LIMIT ?5"
                ))
                .bind(project_id.to_string())
                .bind(conversation_id.to_string())
                .bind(created_at)
                .bind(request_id.to_string())
                .bind(i64::from(limit))
                .fetch_all(&mut *tx)
                .await?
            }
            (Some(created_at), None) => {
                sqlx::query(&format!(
                    "SELECT {SIDE_CHAT_COLUMNS} FROM side_chat_turns_v4
                 WHERE project_id=?1 AND conversation_id=?2 AND created_at < ?3
                 ORDER BY created_at DESC,request_id DESC LIMIT ?4"
                ))
                .bind(project_id.to_string())
                .bind(conversation_id.to_string())
                .bind(created_at)
                .bind(i64::from(limit))
                .fetch_all(&mut *tx)
                .await?
            }
            (None, None) => {
                sqlx::query(&format!(
                    "SELECT {SIDE_CHAT_COLUMNS} FROM side_chat_turns_v4
                 WHERE project_id=?1 AND conversation_id=?2
                 ORDER BY created_at DESC,request_id DESC LIMIT ?3"
                ))
                .bind(project_id.to_string())
                .bind(conversation_id.to_string())
                .bind(i64::from(limit))
                .fetch_all(&mut *tx)
                .await?
            }
            (None, Some(_)) => unreachable!("validated side chat cursor"),
        };
        let mut turns = Vec::with_capacity(rows.len());
        for row in rows {
            let (_, turn) = side_chat_from_row(row)?;
            ensure_turn_scope(&turn, project_id, conversation_id)?;
            turns.push(turn);
        }
        tx.commit().await?;
        Ok(turns)
    }

    /// Complete a turn only while its request is still locally owned. A
    /// completed answer must cite at least one exact source ID.
    pub async fn complete_side_chat_turn(
        &self,
        project_id: Uuid,
        conversation_id: Uuid,
        request_id: Uuid,
        answer_markdown: &str,
        cited_source_ids: &[String],
        usage: Option<omicsops_protocol::UsageTotalsV4>,
    ) -> Result<SideChatTurnV4, StoreError> {
        validate_answer(answer_markdown)?;
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let result = async {
            ensure_conversation_owner_executor(&mut *tx, project_id, conversation_id).await?;
            let (_, current) = load_side_chat_in_tx(&mut *tx, request_id)
                .await?
                .ok_or_else(|| StoreError::InvalidInput("side chat turn was not found".into()))?;
            ensure_turn_scope(&current, project_id, conversation_id)?;
            ensure_active_turn(&current)?;
            validate_citations(&current, cited_source_ids)?;
            let mut completed = current;
            completed.status = SideChatTurnStatusV4::Completed;
            completed.answer_markdown = Some(answer_markdown.to_owned());
            completed.cited_source_ids = cited_source_ids.to_vec();
            completed.failure_code = None;
            completed.usage = usage;
            completed.updated_at = Utc::now();
            update_terminal_turn(&mut *tx, &completed, "queued", "running").await?;
            Ok(completed)
        }
        .await;
        match result {
            Ok(value) => {
                tx.commit().await?;
                Ok(value)
            }
            Err(error) => {
                let _ = tx.rollback().await;
                Err(error)
            }
        }
    }

    pub async fn complete_side_chat_no_evidence(
        &self,
        project_id: Uuid,
        conversation_id: Uuid,
        request_id: Uuid,
    ) -> Result<SideChatTurnV4, StoreError> {
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let result = async {
            ensure_conversation_owner_executor(&mut *tx, project_id, conversation_id).await?;
            let (_, current) = load_side_chat_in_tx(&mut *tx, request_id)
                .await?
                .ok_or_else(|| StoreError::InvalidInput("side chat turn was not found".into()))?;
            ensure_turn_scope(&current, project_id, conversation_id)?;
            ensure_active_turn(&current)?;
            let mut completed = current;
            completed.status = SideChatTurnStatusV4::NoEvidence;
            completed.answer_markdown = None;
            completed.cited_source_ids.clear();
            completed.failure_code = None;
            completed.usage = None;
            completed.updated_at = Utc::now();
            update_terminal_turn(&mut *tx, &completed, "queued", "running").await?;
            Ok(completed)
        }
        .await;
        match result {
            Ok(value) => {
                tx.commit().await?;
                Ok(value)
            }
            Err(error) => {
                let _ = tx.rollback().await;
                Err(error)
            }
        }
    }

    /// Persist only a bounded typed failure code. Provider details are never
    /// stored in the public turn.
    pub async fn fail_side_chat_turn(
        &self,
        project_id: Uuid,
        conversation_id: Uuid,
        request_id: Uuid,
        failure_code: SideChatFailureCodeV4,
    ) -> Result<SideChatTurnV4, StoreError> {
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let result = async {
            ensure_conversation_owner_executor(&mut *tx, project_id, conversation_id).await?;
            let (_, current) = load_side_chat_in_tx(&mut *tx, request_id)
                .await?
                .ok_or_else(|| StoreError::InvalidInput("side chat turn was not found".into()))?;
            ensure_turn_scope(&current, project_id, conversation_id)?;
            ensure_active_turn(&current)?;
            let mut failed = current;
            failed.status = SideChatTurnStatusV4::Failed;
            failed.answer_markdown = None;
            failed.cited_source_ids.clear();
            failed.failure_code = Some(failure_code);
            failed.usage = None;
            failed.updated_at = Utc::now();
            update_terminal_turn(&mut *tx, &failed, "queued", "running").await?;
            Ok(failed)
        }
        .await;
        match result {
            Ok(value) => {
                tx.commit().await?;
                Ok(value)
            }
            Err(error) => {
                let _ = tx.rollback().await;
                Err(error)
            }
        }
    }

    /// Host recovery view; the native layer must acquire the corresponding
    /// OS lease before abandoning an item.
    pub async fn list_running_side_chat_turns(&self) -> Result<Vec<SideChatTurnV4>, StoreError> {
        let rows = sqlx::query(&format!(
            "SELECT {SIDE_CHAT_COLUMNS} FROM side_chat_turns_v4
             WHERE status IN ('queued','running') ORDER BY request_id"
        ))
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(|row| side_chat_from_row(row).map(|(_, turn)| turn))
            .collect()
    }

    /// Abandon one accepted turn after the native layer proves that its OS
    /// lease is no longer held. Missing or already terminal rows are treated
    /// as an idempotent no-op.
    pub async fn abandon_side_chat_turn(
        &self,
        project_id: Uuid,
        conversation_id: Uuid,
        request_id: Uuid,
    ) -> Result<bool, StoreError> {
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let result = async {
            let Some((_, current)) = load_side_chat_in_tx(&mut *tx, request_id).await? else {
                return Ok(false);
            };
            ensure_conversation_owner_executor(&mut *tx, project_id, conversation_id).await?;
            ensure_turn_scope(&current, project_id, conversation_id)?;
            if !matches!(
                current.status,
                SideChatTurnStatusV4::Queued | SideChatTurnStatusV4::Running
            ) {
                return Ok(false);
            }
            let mut interrupted = current;
            interrupted.status = SideChatTurnStatusV4::Interrupted;
            interrupted.answer_markdown = None;
            interrupted.cited_source_ids.clear();
            interrupted.failure_code = Some(SideChatFailureCodeV4::Interrupted);
            interrupted.usage = None;
            interrupted.updated_at = Utc::now();
            update_terminal_turn(&mut *tx, &interrupted, "queued", "running").await?;
            Ok(true)
        }
        .await;
        match result {
            Ok(value) => {
                tx.commit().await?;
                Ok(value)
            }
            Err(error) => {
                let _ = tx.rollback().await;
                Err(error)
            }
        }
    }
}

async fn validate_parent_request(
    tx: &mut SqliteConnection,
    turn: &SideChatTurnV4,
) -> Result<(), StoreError> {
    let Some(parent_id) = turn.parent_request_id else {
        return Ok(());
    };
    if parent_id.is_nil() || parent_id == turn.request_id {
        return Err(StoreError::InvalidInput(
            "side chat parent request ID is invalid".into(),
        ));
    }
    let Some((_, parent)) = load_side_chat_in_tx(tx, parent_id).await? else {
        return Err(StoreError::InvalidInput(
            "side chat retry parent request was not found".into(),
        ));
    };
    if parent.project_id != turn.project_id || parent.conversation_id != turn.conversation_id {
        return Err(StoreError::InvalidInput(
            "side chat retry parent request belongs to another scope".into(),
        ));
    }
    if !matches!(
        parent.status,
        SideChatTurnStatusV4::Failed | SideChatTurnStatusV4::Interrupted
    ) {
        return Err(StoreError::InvalidInput(
            "side chat retry parent is not failed or interrupted".into(),
        ));
    }
    Ok(())
}

async fn update_terminal_turn(
    tx: &mut SqliteConnection,
    turn: &SideChatTurnV4,
    first_status: &str,
    second_status: &str,
) -> Result<(), StoreError> {
    let updated = sqlx::query(
        "UPDATE side_chat_turns_v4
         SET status=?1,value_json=?2,updated_at=?3
         WHERE request_id=?4 AND project_id=?5 AND conversation_id=?6
           AND status IN (?7,?8)",
    )
    .bind(status_string(turn.status))
    .bind(serde_json::to_string(turn)?)
    .bind(timestamp(turn.updated_at))
    .bind(turn.request_id.to_string())
    .bind(turn.project_id.to_string())
    .bind(turn.conversation_id.to_string())
    .bind(first_status)
    .bind(second_status)
    .execute(&mut *tx)
    .await?;
    if updated.rows_affected() != 1 {
        return Err(StoreError::InvalidInput(
            "side chat turn changed before its result could be committed".into(),
        ));
    }
    Ok(())
}

fn ensure_active_turn(turn: &SideChatTurnV4) -> Result<(), StoreError> {
    if matches!(
        turn.status,
        SideChatTurnStatusV4::Queued | SideChatTurnStatusV4::Running
    ) {
        Ok(())
    } else {
        Err(StoreError::InvalidInput(
            "side chat turn is no longer active".into(),
        ))
    }
}

async fn load_side_chat_in_tx(
    tx: &mut SqliteConnection,
    request_id: Uuid,
) -> Result<Option<(String, SideChatTurnV4)>, StoreError> {
    let row = sqlx::query(&format!(
        "SELECT {SIDE_CHAT_COLUMNS} FROM side_chat_turns_v4 WHERE request_id=?1"
    ))
    .bind(request_id.to_string())
    .fetch_optional(&mut *tx)
    .await?;
    row.map(side_chat_from_row).transpose()
}

fn side_chat_from_row(
    row: sqlx::sqlite::SqliteRow,
) -> Result<(String, SideChatTurnV4), StoreError> {
    let request_id = parse_uuid(row.try_get::<String, _>(0)?, "side chat request id")?;
    let project_id = parse_uuid(row.try_get::<String, _>(1)?, "side chat project id")?;
    let conversation_id = parse_uuid(row.try_get::<String, _>(2)?, "side chat conversation id")?;
    let request_hash = row.try_get::<String, _>(3)?;
    validate_hash(&request_hash, "side chat request hash")?;
    let status = parse_status(&row.try_get::<String, _>(4)?)?;
    let turn: SideChatTurnV4 =
        serde_json::from_str(&row.try_get::<String, _>(5)?).map_err(|error| {
            StoreError::InvalidInput(format!("stored side chat turn is invalid: {error}"))
        })?;
    let created_at = from_timestamp(row.try_get(6)?, "side chat created_at")?;
    let updated_at = from_timestamp(row.try_get(7)?, "side chat updated_at")?;
    if turn.id != request_id
        || turn.request_id != request_id
        || turn.project_id != project_id
        || turn.conversation_id != conversation_id
        || turn.status != status
        || timestamp(turn.created_at) != timestamp(created_at)
        || timestamp(turn.updated_at) != timestamp(updated_at)
        || side_chat_request_hash(&turn)
            .map_err(|_| {
                StoreError::InvalidInput("stored side chat request cannot be hashed".into())
            })?
            .ne(&request_hash)
    {
        return Err(StoreError::InvalidInput(
            "stored side chat columns do not match its record".into(),
        ));
    }
    validate_turn_shape(&turn)?;
    Ok((request_hash, turn))
}

fn ensure_turn_scope(
    turn: &SideChatTurnV4,
    project_id: Uuid,
    conversation_id: Uuid,
) -> Result<(), StoreError> {
    if turn.project_id != project_id || turn.conversation_id != conversation_id {
        return Err(StoreError::InvalidInput(
            "side chat turn does not belong to the requested scope".into(),
        ));
    }
    Ok(())
}

fn validate_request_identity(turn: &SideChatTurnV4) -> Result<(), StoreError> {
    if turn.id.is_nil() || turn.request_id.is_nil() || turn.id != turn.request_id {
        return Err(StoreError::InvalidInput(
            "side chat request ID must be non-nil and equal to turn ID".into(),
        ));
    }
    if turn.project_id.is_nil() || turn.conversation_id.is_nil() || turn.model_profile_id.is_nil() {
        return Err(StoreError::InvalidInput(
            "side chat scope and model IDs must be non-nil".into(),
        ));
    }
    Ok(())
}

fn validate_initial_turn(turn: &SideChatTurnV4) -> Result<(), StoreError> {
    validate_request_identity(turn)?;
    if turn.status != SideChatTurnStatusV4::Queued {
        return Err(StoreError::InvalidInput(
            "new side chat turns must start queued".into(),
        ));
    }
    if turn.answer_markdown.is_some()
        || !turn.cited_source_ids.is_empty()
        || turn.failure_code.is_some()
        || turn.usage.is_some()
    {
        return Err(StoreError::InvalidInput(
            "new side chat turns cannot contain a result".into(),
        ));
    }
    validate_turn_shape(turn)?;
    if turn.question_markdown.trim().is_empty()
        || turn.question_markdown.len() > SIDE_CHAT_MAX_QUESTION_BYTES
    {
        return Err(StoreError::InvalidInput(format!(
            "side chat question must be nonempty and at most {SIDE_CHAT_MAX_QUESTION_BYTES} bytes"
        )));
    }
    if turn.created_at > turn.updated_at {
        return Err(StoreError::InvalidInput(
            "side chat timestamps are out of order".into(),
        ));
    }
    validate_source_snapshot(turn)?;
    Ok(())
}

fn validate_turn_shape(turn: &SideChatTurnV4) -> Result<(), StoreError> {
    if turn.model_label.trim().is_empty()
        || turn.model_label.len() > SIDE_CHAT_MAX_MODEL_LABEL_BYTES
    {
        return Err(StoreError::InvalidInput(
            "side chat model label is invalid".into(),
        ));
    }
    if turn.references.len() > SIDE_CHAT_MAX_REFERENCES {
        return Err(StoreError::InvalidInput(format!(
            "side chat references exceed {SIDE_CHAT_MAX_REFERENCES}"
        )));
    }
    for reference in &turn.references {
        validate_reference(turn.project_id, reference)?;
    }
    if turn.attachments.len() > SIDE_CHAT_MAX_ATTACHMENTS {
        return Err(StoreError::InvalidInput(format!(
            "side chat attachments exceed {SIDE_CHAT_MAX_ATTACHMENTS}"
        )));
    }
    let mut attachment_ids = HashSet::with_capacity(turn.attachments.len());
    for id in &turn.attachments {
        if id.is_nil() || !attachment_ids.insert(*id) {
            return Err(StoreError::InvalidInput(
                "side chat attachment IDs must be unique and non-nil".into(),
            ));
        }
    }
    validate_source_snapshot(turn)
}

fn validate_reference(project_id: Uuid, reference: &ComposerReference) -> Result<(), StoreError> {
    match reference {
        ComposerReference::Skill { id } => {
            if id.is_nil() {
                return Err(StoreError::InvalidInput(
                    "side chat skill reference ID cannot be nil".into(),
                ));
            }
        }
        ComposerReference::Artifact {
            project_id: owner,
            id,
        }
        | ComposerReference::Session {
            project_id: owner,
            id,
        }
        | ComposerReference::Project {
            project_id: owner,
            id,
        }
        | ComposerReference::Workflow {
            project_id: owner,
            id,
        }
        | ComposerReference::Quote {
            project_id: owner,
            id,
        } => {
            if owner != &project_id || id.is_nil() {
                return Err(StoreError::InvalidInput(
                    "side chat reference does not belong to the project".into(),
                ));
            }
        }
        ComposerReference::ExecutionContext {
            project_id: owner,
            backend_id,
        } => {
            if owner != &project_id || invalid_token(backend_id, SIDE_CHAT_MAX_LABEL_BYTES) {
                return Err(StoreError::InvalidInput(
                    "side chat execution context reference is invalid".into(),
                ));
            }
        }
        ComposerReference::Runtime {
            project_id: owner,
            backend_id,
            language,
        } => {
            if owner != &project_id
                || invalid_token(backend_id, SIDE_CHAT_MAX_LABEL_BYTES)
                || !matches!(language.as_str(), "python" | "r")
            {
                return Err(StoreError::InvalidInput(
                    "side chat runtime reference is invalid".into(),
                ));
            }
        }
        ComposerReference::WorkspaceFile {
            project_id: owner,
            backend_id,
            relative_path,
        } => {
            if owner != &project_id
                || invalid_token(backend_id, SIDE_CHAT_MAX_LABEL_BYTES)
                || invalid_relative_path(relative_path)
            {
                return Err(StoreError::InvalidInput(
                    "side chat workspace file reference is invalid".into(),
                ));
            }
        }
    }
    Ok(())
}

fn invalid_token(value: &str, max_bytes: usize) -> bool {
    value.trim().is_empty()
        || value.len() > max_bytes
        || value.chars().any(|character| character.is_control())
}

fn invalid_relative_path(value: &str) -> bool {
    value.trim().is_empty()
        || value.len() > 4096
        || value.contains('\0')
        || value.starts_with('/')
        || value.starts_with('\\')
        || value.contains(':')
        || value
            .split(['/', '\\'])
            .any(|part| part == ".." || part.is_empty())
}

fn validate_source_snapshot(turn: &SideChatTurnV4) -> Result<(), StoreError> {
    if turn.sources.len() > SIDE_CHAT_MAX_SOURCES {
        return Err(StoreError::InvalidInput(format!(
            "side chat sources exceed {SIDE_CHAT_MAX_SOURCES}"
        )));
    }
    if turn.source_watermark.event_heads.len() > SIDE_CHAT_MAX_EVENT_HEADS {
        return Err(StoreError::InvalidInput(format!(
            "side chat event heads exceed {SIDE_CHAT_MAX_EVENT_HEADS}"
        )));
    }
    let mut source_ids = HashSet::with_capacity(turn.sources.len());
    for source in &turn.sources {
        if source.source_id.trim().is_empty()
            || source.source_id.len() > SIDE_CHAT_MAX_SOURCE_ID_BYTES
            || !source_ids.insert(source.source_id.clone())
            || source.label.trim().is_empty()
            || source.label.len() > SIDE_CHAT_MAX_LABEL_BYTES
            || source.excerpt.trim().is_empty()
            || source.excerpt.len() > SIDE_CHAT_MAX_SOURCE_TEXT_BYTES
            || source.role.trim().is_empty()
            || source.role.len() > SIDE_CHAT_MAX_ROLE_BYTES
            || source.role.chars().any(|character| character.is_control())
        {
            return Err(StoreError::InvalidInput(
                "side chat source metadata or excerpt is invalid".into(),
            ));
        }
        match (
            source.message_id,
            source.run_id,
            source.event_sequence,
            source.event_hash.as_deref(),
        ) {
            (Some(message_id), None, None, None) if !message_id.is_nil() => {
                if source.source_id != format!("message:{message_id}")
                    || !matches!(source.role.as_str(), "user" | "assistant" | "tool")
                    || !source
                        .message_content_sha256
                        .as_deref()
                        .is_some_and(|hash| {
                            hash.len() == 64 && hash.chars().all(|c| c.is_ascii_hexdigit())
                        })
                {
                    return Err(StoreError::InvalidInput(
                        "side chat message source identity is invalid".into(),
                    ));
                }
            }
            (None, Some(run_id), Some(sequence), Some(event_hash))
                if !run_id.is_nil() && sequence > 0 =>
            {
                validate_hash(event_hash, "side chat event source hash")?;
                if source.message_content_sha256.is_some()
                    || source.source_id != format!("event:{run_id}:{sequence}:{event_hash}")
                    || source.role != "evidence"
                {
                    return Err(StoreError::InvalidInput(
                        "side chat event source identity is invalid".into(),
                    ));
                }
            }
            (None, None, None, None)
                if source.role == "material"
                    && source.message_content_sha256.is_none()
                    && source.source_id
                        == format!(
                            "material:{}",
                            hex::encode(Sha256::digest(source.excerpt.as_bytes()))
                        )
                    && (!turn.references.is_empty() || !turn.attachments.is_empty()) => {}
            _ => {
                return Err(StoreError::InvalidInput(
                    "side chat source must be a message or evidence event".into(),
                ));
            }
        }
    }
    let mut event_heads = HashSet::with_capacity(turn.source_watermark.event_heads.len());
    for head in &turn.source_watermark.event_heads {
        if head.run_id.is_nil() || head.sequence == 0 || !event_heads.insert(head.run_id) {
            return Err(StoreError::InvalidInput(
                "side chat event watermark is invalid".into(),
            ));
        }
        validate_hash(&head.event_hash, "side chat event head hash")?;
    }
    let snapshot = side_chat_source_snapshot_value(&turn.source_watermark, &turn.sources);
    if serde_json::to_vec(&snapshot)?.len() > SIDE_CHAT_MAX_SOURCE_SNAPSHOT_BYTES {
        return Err(StoreError::InvalidInput(format!(
            "side chat source snapshot exceeds {SIDE_CHAT_MAX_SOURCE_SNAPSHOT_BYTES} bytes"
        )));
    }
    let expected = side_chat_source_snapshot_hash(&turn.source_watermark, &turn.sources)?;
    if !turn.source_snapshot_sha256.eq_ignore_ascii_case(&expected) {
        return Err(StoreError::InvalidInput(
            "side chat source snapshot hash does not match its sources".into(),
        ));
    }
    Ok(())
}

async fn validate_source_snapshot_in_tx(
    tx: &mut SqliteConnection,
    turn: &SideChatTurnV4,
) -> Result<(), StoreError> {
    let message_rows = sqlx::query(
        "SELECT id,seq,role,content FROM messages
         WHERE project_id=?1 AND conversation_id=?2
         ORDER BY seq,id",
    )
    .bind(turn.project_id.to_string())
    .bind(turn.conversation_id.to_string())
    .fetch_all(&mut *tx)
    .await?;
    let mut message_meta = HashMap::with_capacity(message_rows.len());
    let mut message_count = 0_u64;
    let mut message_head_sequence = None;
    for row in message_rows {
        let id = parse_uuid(row.try_get::<String, _>(0)?, "side chat source message id")?;
        let sequence = u64::try_from(row.try_get::<i64, _>(1)?).map_err(|_| {
            StoreError::InvalidInput("side chat source message sequence is invalid".into())
        })?;
        let role = row.try_get::<String, _>(2)?;
        let content = row.try_get::<String, _>(3)?;
        let eligible = role != "system" && !content.trim().is_empty();
        let content_sha256 = hex::encode(Sha256::digest(content.as_bytes()));
        message_meta.insert(id, (sequence, role.clone(), eligible, content_sha256));
        if eligible {
            message_count = message_count.saturating_add(1);
            message_head_sequence = Some(sequence);
        }
    }
    if turn.source_watermark.message_count != message_count
        || turn.source_watermark.message_head_sequence != message_head_sequence
    {
        return Err(StoreError::InvalidInput(
            "side chat message source watermark is stale".into(),
        ));
    }

    let event_rows = sqlx::query(
        "SELECT run_id,sequence,event_hash,value_json FROM agent_events_v4
         WHERE project_id=?1 AND conversation_id=?2 ORDER BY run_id,sequence",
    )
    .bind(turn.project_id.to_string())
    .bind(turn.conversation_id.to_string())
    .fetch_all(&mut *tx)
    .await?;
    let mut chains: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut columns = HashMap::with_capacity(event_rows.len());
    for row in event_rows {
        let run_id = row.try_get::<String, _>(0)?;
        let sequence = u64::try_from(row.try_get::<i64, _>(1)?).map_err(|_| {
            StoreError::InvalidInput("side chat source event sequence is invalid".into())
        })?;
        let event_hash = row.try_get::<String, _>(2)?;
        let value_json = row.try_get::<String, _>(3)?;
        chains.entry(run_id.clone()).or_default().push(value_json);
        columns.insert((run_id, sequence), event_hash);
    }
    let mut eligible_events = HashMap::<(Uuid, u64), AgentEventV4>::new();
    let mut event_heads = Vec::new();
    for (run_text, serialized) in chains {
        let run_id = parse_uuid(&run_text, "side chat source run id")?;
        let events = deserialize_event_chain_v4(&serialized).map_err(|error| {
            StoreError::InvalidInput(format!("side chat source event chain is invalid: {error}"))
        })?;
        let mut latest = None;
        for event in events {
            if event.run_id != run_id
                || event.project_id != turn.project_id
                || event.conversation_id != turn.conversation_id
            {
                return Err(StoreError::InvalidInput(
                    "side chat source event scope does not match its row".into(),
                ));
            }
            if is_eligible_evidence_event(&event.event) {
                latest = Some((event.sequence, event.event_hash.clone()));
                eligible_events.insert((run_id, event.sequence), event);
            }
        }
        if let Some((sequence, event_hash)) = latest {
            event_heads.push(SideChatEventHeadV4 {
                run_id,
                sequence,
                event_hash,
            });
        }
    }
    event_heads.sort_by_key(|head| head.run_id);
    if turn.source_watermark.event_count != eligible_events.len() as u64
        || turn.source_watermark.event_heads != event_heads
    {
        return Err(StoreError::InvalidInput(
            "side chat event source watermark is stale".into(),
        ));
    }

    for source in &turn.sources {
        if let Some(message_id) = source.message_id {
            let Some((sequence, role, eligible, content_sha256)) = message_meta.get(&message_id)
            else {
                return Err(StoreError::InvalidInput(
                    "side chat source message was not found".into(),
                ));
            };
            if !eligible
                || *sequence != source.sequence
                || role != &source.role
                || !source
                    .message_content_sha256
                    .as_deref()
                    .is_some_and(|hash| hash.eq_ignore_ascii_case(content_sha256))
            {
                return Err(StoreError::InvalidInput(
                    "side chat source message changed".into(),
                ));
            }
        } else if source.role == "material" {
            // Material sources are host-resolved reference/attachment
            // excerpts. Their stable identity is the digest of the exact
            // sanitized excerpt persisted in the turn; no client path or raw
            // material is trusted here.
            let expected = format!(
                "material:{}",
                hex::encode(Sha256::digest(source.excerpt.as_bytes()))
            );
            if source.source_id != expected {
                return Err(StoreError::InvalidInput(
                    "side chat material source changed".into(),
                ));
            }
        } else {
            let run_id = source.run_id.expect("validated event source run ID");
            let sequence = source
                .event_sequence
                .expect("validated event source sequence");
            let event_hash = source
                .event_hash
                .as_deref()
                .expect("validated event source hash");
            let Some(column_hash) = columns.get(&(run_id.to_string(), sequence)) else {
                return Err(StoreError::InvalidInput(
                    "side chat source event was not found".into(),
                ));
            };
            let Some(event) = eligible_events.get(&(run_id, sequence)) else {
                return Err(StoreError::InvalidInput(
                    "side chat source event is not completed evidence".into(),
                ));
            };
            if !column_hash.eq_ignore_ascii_case(event_hash)
                || !event.event_hash.eq_ignore_ascii_case(event_hash)
                || source.sequence != sequence
            {
                return Err(StoreError::InvalidInput(
                    "side chat source event changed".into(),
                ));
            }
        }
    }
    Ok(())
}

fn is_eligible_evidence_event(event: &AgentEventKindV4) -> bool {
    match event {
        AgentEventKindV4::ToolFinished { outcome }
        | AgentEventKindV4::ToolOutcomeReused { outcome, .. } => outcome.succeeded,
        AgentEventKindV4::ToolDispatchResolved { evidence, .. } => !evidence.trim().is_empty(),
        _ => false,
    }
}

fn validate_answer(answer: &str) -> Result<(), StoreError> {
    if answer.trim().is_empty() || answer.len() > SIDE_CHAT_MAX_ANSWER_BYTES {
        return Err(StoreError::InvalidInput(format!(
            "side chat answer must be nonempty and at most {SIDE_CHAT_MAX_ANSWER_BYTES} bytes"
        )));
    }
    Ok(())
}

fn validate_citations(
    turn: &SideChatTurnV4,
    cited_source_ids: &[String],
) -> Result<(), StoreError> {
    if cited_source_ids.is_empty() || cited_source_ids.len() > SIDE_CHAT_MAX_CITATIONS {
        return Err(StoreError::InvalidInput(
            "side chat answer must cite one or more bounded sources".into(),
        ));
    }
    let known = turn
        .sources
        .iter()
        .map(|source| source.source_id.as_str())
        .collect::<HashSet<_>>();
    let mut seen = HashSet::with_capacity(cited_source_ids.len());
    for source_id in cited_source_ids {
        if source_id.trim().is_empty()
            || source_id.len() > SIDE_CHAT_MAX_SOURCE_ID_BYTES
            || !seen.insert(source_id.as_str())
            || !known.contains(source_id.as_str())
        {
            return Err(StoreError::InvalidInput(
                "side chat answer cites an unknown or duplicate source".into(),
            ));
        }
    }
    Ok(())
}

fn parse_cursor_timestamp(value: &str) -> Result<i64, StoreError> {
    if let Ok(millis) = value.parse::<i64>() {
        return Ok(millis);
    }
    DateTime::parse_from_rfc3339(value)
        .map(|date| timestamp(date.with_timezone(&Utc)))
        .map_err(|_| StoreError::InvalidInput("side chat cursor timestamp is invalid".into()))
}

fn status_string(status: SideChatTurnStatusV4) -> &'static str {
    match status {
        SideChatTurnStatusV4::Queued => "queued",
        SideChatTurnStatusV4::Running => "running",
        SideChatTurnStatusV4::Completed => "completed",
        SideChatTurnStatusV4::NoEvidence => "no_evidence",
        SideChatTurnStatusV4::Failed => "failed",
        SideChatTurnStatusV4::Interrupted => "interrupted",
    }
}

fn parse_status(value: &str) -> Result<SideChatTurnStatusV4, StoreError> {
    match value {
        "queued" => Ok(SideChatTurnStatusV4::Queued),
        "running" => Ok(SideChatTurnStatusV4::Running),
        "completed" => Ok(SideChatTurnStatusV4::Completed),
        "no_evidence" => Ok(SideChatTurnStatusV4::NoEvidence),
        "failed" => Ok(SideChatTurnStatusV4::Failed),
        "interrupted" => Ok(SideChatTurnStatusV4::Interrupted),
        _ => Err(StoreError::InvalidInput(
            "stored side chat turn has an invalid status".into(),
        )),
    }
}

fn validate_hash(value: &str, label: &str) -> Result<(), StoreError> {
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(StoreError::InvalidInput(format!(
            "{label} must be a 64-character hexadecimal SHA-256"
        )));
    }
    Ok(())
}
