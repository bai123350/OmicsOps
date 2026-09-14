use std::collections::HashSet;

use chrono::Utc;
use omicsops_protocol::{
    ReviewerSettingsV4, SESSION_REVIEW_MAX_ERROR_BYTES, SESSION_REVIEW_MAX_FINDING_CODE_BYTES,
    SESSION_REVIEW_MAX_FINDING_MESSAGE_BYTES, SESSION_REVIEW_MAX_FINDING_SOURCES,
    SESSION_REVIEW_MAX_FINDINGS, SESSION_REVIEW_MAX_PER_PROJECT,
    SESSION_REVIEW_MAX_SOURCE_SNAPSHOT_BYTES, SESSION_REVIEW_MAX_SOURCE_TEXT_BYTES,
    SESSION_REVIEW_MAX_SOURCES, SessionReviewBeginResultV4, SessionReviewFindingV4,
    SessionReviewRecordV4, SessionReviewReportV4, SessionReviewSourceV4, SessionReviewStatusV4,
    session_review_source_snapshot_hash, session_review_source_snapshot_value,
};
use sqlx::{Row, SqliteConnection};
use uuid::Uuid;

use super::{Store, StoreError, ensure_conversation_owner_executor, from_timestamp, timestamp};

const REVIEWER_SETTINGS_KEY: &str = "reviewer_settings_v4";

/// Reject ordinary writes while a retrospective review owns this
/// conversation.  The acquire path checks an existing request id before
/// calling this helper so an idempotent retry can return its durable record.
pub(super) async fn ensure_no_active_session_review_executor(
    tx: &mut SqliteConnection,
    project_id: Uuid,
    conversation_id: Uuid,
) -> Result<(), StoreError> {
    let active: i64 = sqlx::query_scalar(
        "SELECT EXISTS(
             SELECT 1 FROM session_reviews
             WHERE frame_id=?1 AND status='running'
         )",
    )
    .bind(conversation_id.to_string())
    .fetch_one(&mut *tx)
    .await?;
    if active != 0 {
        return Err(StoreError::InvalidInput(format!(
            "conversation {conversation_id} in project {project_id} is locked by an active session review"
        )));
    }
    Ok(())
}

impl Store {
    /// Read the global reviewer selection.  Missing settings retain the
    /// protocol's FollowSession default and do not create a row.
    pub async fn get_reviewer_settings(&self) -> Result<ReviewerSettingsV4, StoreError> {
        let value = sqlx::query_scalar::<_, String>(
            "SELECT value_json FROM settings WHERE scope='global' AND key=?1",
        )
        .bind(REVIEWER_SETTINGS_KEY)
        .fetch_optional(&self.pool)
        .await?;
        value
            .map(|value| {
                serde_json::from_str(&value).map_err(|error| {
                    StoreError::InvalidInput(format!("invalid reviewer settings: {error}"))
                })
            })
            .transpose()
            .map(|settings| settings.unwrap_or_default())
    }

    /// Persist the global reviewer selection.  Profile existence and
    /// provider capability validation belong to the native command layer.
    pub async fn save_reviewer_settings(
        &self,
        settings: &ReviewerSettingsV4,
    ) -> Result<(), StoreError> {
        let value = serde_json::to_string(settings)?;
        sqlx::query(
            "INSERT INTO settings(scope,key,value_json,updated_at)
             VALUES('global',?1,?2,?3)
             ON CONFLICT(scope,key) DO UPDATE SET
               value_json=excluded.value_json,updated_at=excluded.updated_at",
        )
        .bind(REVIEWER_SETTINGS_KEY)
        .bind(value)
        .bind(timestamp(Utc::now()))
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Atomically acquire a retrospective session review.  A repeated
    /// request id in the same scope returns the durable record with
    /// `acquired=false`, even if the caller's later snapshot differs.
    pub async fn begin_session_review(
        &self,
        record: SessionReviewRecordV4,
    ) -> Result<SessionReviewBeginResultV4, StoreError> {
        validate_record_identity(&record)?;
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let result: Result<SessionReviewBeginResultV4, StoreError> = async {
            ensure_conversation_owner_executor(
                &mut *tx,
                record.project_id,
                record.conversation_id,
            )
            .await?;

            // Idempotency is intentionally checked before lifecycle locks.
            // A lost response must be recoverable without making the caller
            // resnapshot a conversation that has since changed.
            if let Some(existing) = load_session_review_in_tx(&mut *tx, record.id).await? {
                if existing.project_id != record.project_id
                    || existing.conversation_id != record.conversation_id
                {
                    return Err(StoreError::InvalidInput(
                        "session review request id belongs to another scope".into(),
                    ));
                }
                return Ok(SessionReviewBeginResultV4 {
                    record: existing,
                    acquired: false,
                });
            }

            validate_initial_record(&record)?;
            ensure_new_review_allowed(
                &mut *tx,
                record.project_id,
                record.conversation_id,
            )
            .await?;
            validate_source_snapshot_in_tx(&mut *tx, &record).await?;

            let project_count: i64 = sqlx::query_scalar(
                "SELECT COUNT(*)
                 FROM session_reviews r
                 JOIN frames f ON f.id=r.frame_id
                 WHERE f.project_id=?1",
            )
            .bind(record.project_id.to_string())
            .fetch_one(&mut *tx)
            .await?;
            if project_count >= SESSION_REVIEW_MAX_PER_PROJECT as i64 {
                return Err(StoreError::InvalidInput(format!(
                    "session review project cap has been reached (maximum {SESSION_REVIEW_MAX_PER_PROJECT})"
                )));
            }

            let value_json = serde_json::to_string(&record)?;
            sqlx::query(
                "INSERT INTO session_reviews
                 (id,frame_id,reviewer,status,value_json,created_at,updated_at)
                 VALUES (?1,?2,?3,?4,?5,?6,?7)",
            )
            .bind(record.id.to_string())
            .bind(record.conversation_id.to_string())
            .bind(record.reviewer_profile_id.to_string())
            .bind(status_string(record.status))
            .bind(value_json)
            .bind(timestamp(record.created_at))
            .bind(timestamp(record.updated_at))
            .execute(&mut *tx)
            .await?;
            Ok(SessionReviewBeginResultV4 {
                record,
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

    /// Get one review only within the supplied project/conversation scope.
    pub async fn get_session_review(
        &self,
        project_id: Uuid,
        conversation_id: Uuid,
        request_id: Uuid,
    ) -> Result<Option<SessionReviewRecordV4>, StoreError> {
        let mut tx = self.pool.begin().await?;
        ensure_conversation_owner_executor(&mut *tx, project_id, conversation_id).await?;
        let value = sqlx::query(
            "SELECT id,frame_id,reviewer,status,value_json,created_at,updated_at
             FROM session_reviews WHERE id=?1 AND frame_id=?2",
        )
        .bind(request_id.to_string())
        .bind(conversation_id.to_string())
        .fetch_optional(&mut *tx)
        .await?
        .map(session_review_from_row)
        .transpose()?;
        if let Some(record) = &value {
            ensure_record_scope(record, project_id, conversation_id)?;
        }
        tx.commit().await?;
        Ok(value)
    }

    /// List durable reviews for one owned conversation in newest-first order.
    pub async fn list_session_reviews(
        &self,
        project_id: Uuid,
        conversation_id: Uuid,
    ) -> Result<Vec<SessionReviewRecordV4>, StoreError> {
        let mut tx = self.pool.begin().await?;
        ensure_conversation_owner_executor(&mut *tx, project_id, conversation_id).await?;
        let rows = sqlx::query(
            "SELECT id,frame_id,reviewer,status,value_json,created_at,updated_at
             FROM session_reviews WHERE frame_id=?1
             ORDER BY created_at DESC,id DESC",
        )
        .bind(conversation_id.to_string())
        .fetch_all(&mut *tx)
        .await?;
        let mut records = Vec::with_capacity(rows.len());
        for row in rows {
            let record = session_review_from_row(row)?;
            ensure_record_scope(&record, project_id, conversation_id)?;
            records.push(record);
        }
        tx.commit().await?;
        Ok(records)
    }

    /// List every review that is still running on this Store.  This is a
    /// host-internal recovery view: callers must acquire the per-review
    /// runtime lease before deciding whether a record is interrupted.
    pub async fn list_running_session_reviews(
        &self,
    ) -> Result<Vec<SessionReviewRecordV4>, StoreError> {
        let rows = sqlx::query(
            "SELECT id,frame_id,reviewer,status,value_json,created_at,updated_at
             FROM session_reviews
             WHERE status='running'
             ORDER BY id ASC",
        )
        .fetch_all(&self.pool)
        .await?;
        let mut records = Vec::with_capacity(rows.len());
        for row in rows {
            let record = session_review_from_row(row)?;
            if record.status != SessionReviewStatusV4::Running {
                return Err(StoreError::InvalidInput(
                    "stored running session review has a non-running record".into(),
                ));
            }
            records.push(record);
        }
        Ok(records)
    }

    /// Complete a review only if its current durable state is Running.
    pub async fn complete_session_review(
        &self,
        project_id: Uuid,
        conversation_id: Uuid,
        request_id: Uuid,
        report: SessionReviewReportV4,
    ) -> Result<SessionReviewRecordV4, StoreError> {
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let result: Result<SessionReviewRecordV4, StoreError> = async {
            ensure_conversation_owner_executor(&mut *tx, project_id, conversation_id).await?;
            let current = load_session_review_in_tx(&mut *tx, request_id)
                .await?
                .ok_or_else(|| StoreError::InvalidInput("session review was not found".into()))?;
            ensure_record_scope(&current, project_id, conversation_id)?;
            if current.status != SessionReviewStatusV4::Running {
                return Err(StoreError::InvalidInput(
                    "session review is no longer running".into(),
                ));
            }
            validate_report(&report, &current.sources)?;

            let mut completed = current;
            completed.status = SessionReviewStatusV4::Completed;
            completed.report = Some(report);
            completed.error = None;
            completed.updated_at = Utc::now();
            let value_json = serde_json::to_string(&completed)?;
            let updated = sqlx::query(
                "UPDATE session_reviews
                 SET status=?1,value_json=?2,updated_at=?3
                 WHERE id=?4 AND frame_id=?5 AND status='running'",
            )
            .bind(status_string(completed.status))
            .bind(value_json)
            .bind(timestamp(completed.updated_at))
            .bind(request_id.to_string())
            .bind(conversation_id.to_string())
            .execute(&mut *tx)
            .await?;
            if updated.rows_affected() != 1 {
                return Err(StoreError::InvalidInput(
                    "session review changed before completion could be committed".into(),
                ));
            }
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

    /// Mark a review failed only if its current durable state is Running.
    /// Callers must pass a bounded, already-redacted public error.
    pub async fn fail_session_review(
        &self,
        project_id: Uuid,
        conversation_id: Uuid,
        request_id: Uuid,
        error: &str,
    ) -> Result<SessionReviewRecordV4, StoreError> {
        let error = error.trim();
        validate_error(error)?;
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let result: Result<SessionReviewRecordV4, StoreError> = async {
            ensure_conversation_owner_executor(&mut *tx, project_id, conversation_id).await?;
            let current = load_session_review_in_tx(&mut *tx, request_id)
                .await?
                .ok_or_else(|| StoreError::InvalidInput("session review was not found".into()))?;
            ensure_record_scope(&current, project_id, conversation_id)?;
            if current.status != SessionReviewStatusV4::Running {
                return Err(StoreError::InvalidInput(
                    "session review is no longer running".into(),
                ));
            }
            let mut failed = current;
            failed.status = SessionReviewStatusV4::Failed;
            failed.report = None;
            failed.error = Some(error.to_owned());
            failed.updated_at = Utc::now();
            let value_json = serde_json::to_string(&failed)?;
            let updated = sqlx::query(
                "UPDATE session_reviews
                 SET status=?1,value_json=?2,updated_at=?3
                 WHERE id=?4 AND frame_id=?5 AND status='running'",
            )
            .bind(status_string(failed.status))
            .bind(value_json)
            .bind(timestamp(failed.updated_at))
            .bind(request_id.to_string())
            .bind(conversation_id.to_string())
            .execute(&mut *tx)
            .await?;
            if updated.rows_affected() != 1 {
                return Err(StoreError::InvalidInput(
                    "session review changed before failure could be committed".into(),
                ));
            }
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

    /// Mark one review abandoned only if its current durable state is
    /// Running.  The caller must first establish that the review's runtime
    /// lease is no longer held; this method only performs the durable CAS.
    pub async fn abandon_session_review(
        &self,
        project_id: Uuid,
        conversation_id: Uuid,
        request_id: Uuid,
    ) -> Result<bool, StoreError> {
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let result: Result<bool, StoreError> = async {
            // A conversation delete cascades its review row.  Recovery may
            // be reconciling a stale running-list entry after that delete;
            // treat the missing request as already reconciled before asking
            // the owner check to resolve the now-missing conversation.
            let Some(current) = load_session_review_in_tx(&mut *tx, request_id).await? else {
                return Ok(false);
            };
            ensure_conversation_owner_executor(&mut *tx, project_id, conversation_id).await?;
            ensure_record_scope(&current, project_id, conversation_id)?;
            if current.status != SessionReviewStatusV4::Running {
                return Ok(false);
            }

            let mut abandoned = current;
            abandoned.status = SessionReviewStatusV4::Abandoned;
            abandoned.updated_at = Utc::now();
            let value_json = serde_json::to_string(&abandoned)?;
            let updated = sqlx::query(
                "UPDATE session_reviews
                 SET status=?1,value_json=?2,updated_at=?3
                 WHERE id=?4 AND frame_id=?5 AND status='running'",
            )
            .bind(status_string(abandoned.status))
            .bind(value_json)
            .bind(timestamp(abandoned.updated_at))
            .bind(request_id.to_string())
            .bind(conversation_id.to_string())
            .execute(&mut *tx)
            .await?;
            if updated.rows_affected() != 1 {
                return Err(StoreError::InvalidInput(
                    "session review changed before abandonment could be committed".into(),
                ));
            }
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

async fn ensure_new_review_allowed(
    tx: &mut SqliteConnection,
    project_id: Uuid,
    conversation_id: Uuid,
) -> Result<(), StoreError> {
    let plan: i64 = sqlx::query_scalar(
        "SELECT EXISTS(
             SELECT 1 FROM proposed_plans
             WHERE project_id=?1 AND frame_id=?2
               AND status IN ('generating','revising','pending')
         )",
    )
    .bind(project_id.to_string())
    .bind(conversation_id.to_string())
    .fetch_one(&mut *tx)
    .await?;
    if plan != 0 {
        return Err(StoreError::InvalidInput(
            "conversation is locked by an active plan".into(),
        ));
    }

    let run: i64 = sqlx::query_scalar(
        "SELECT EXISTS(
             SELECT 1 FROM agent_runs_v4
             WHERE project_id=?1 AND conversation_id=?2
               AND status IN ('running','waiting_for_input','waiting_for_approval','awaiting_approval')
         )",
    )
    .bind(project_id.to_string())
    .bind(conversation_id.to_string())
    .fetch_one(&mut *tx)
    .await?;
    if run != 0 {
        return Err(StoreError::InvalidInput(
            "conversation is locked by an active run".into(),
        ));
    }
    ensure_no_active_session_review_executor(&mut *tx, project_id, conversation_id).await
}

async fn validate_source_snapshot_in_tx(
    tx: &mut SqliteConnection,
    record: &SessionReviewRecordV4,
) -> Result<(), StoreError> {
    // SQLite's trim() only removes ASCII space by default.  The native
    // snapshot builder uses Rust's str::trim(), which also removes tabs,
    // newlines, and Unicode whitespace.  Read the small message metadata
    // projection and apply the same rule here so the authority check cannot
    // disagree with the snapshot it is validating.
    let rows = sqlx::query(
        "SELECT id,role,content FROM messages
         WHERE frame_id=?1 AND project_id=?2
         ORDER BY seq DESC,id DESC",
    )
    .bind(record.conversation_id.to_string())
    .bind(record.project_id.to_string())
    .fetch_all(&mut *tx)
    .await?;

    let mut count = 0_u64;
    let mut latest_id = None;
    for row in &rows {
        let role = row.try_get::<String, _>(1)?;
        let content = row.try_get::<Option<String>, _>(2)?;
        if role != "system" && content_is_nonblank(content.as_deref()) {
            count = count.saturating_add(1);
            if latest_id.is_none() {
                latest_id = Some(row.try_get::<String, _>(0)?);
            }
        }
    }
    if record.source_message_count != count {
        return Err(StoreError::InvalidInput(
            "session review source message count is stale".into(),
        ));
    }

    let latest_id = latest_id.ok_or_else(|| {
        StoreError::InvalidInput("there are no eligible messages to review".into())
    })?;
    let selected_latest = record
        .sources
        .last()
        .ok_or_else(|| StoreError::InvalidInput("session review has no sources".into()))?
        .message_id
        .to_string();
    if selected_latest != latest_id {
        return Err(StoreError::InvalidInput(
            "session review source snapshot is no longer current".into(),
        ));
    }

    for source in &record.sources {
        let row = sqlx::query(
            "SELECT project_id,conversation_id,seq,role,content
             FROM messages WHERE id=?1",
        )
        .bind(source.message_id.to_string())
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| {
            StoreError::InvalidInput(format!(
                "session review source message {} was not found",
                source.message_id
            ))
        })?;
        let source_project = row.try_get::<String, _>(0)?;
        let source_conversation = row.try_get::<String, _>(1)?;
        let sequence = row.try_get::<i64, _>(2)?;
        let role = row.try_get::<String, _>(3)?;
        let content = row.try_get::<Option<String>, _>(4)?;
        if source_project != record.project_id.to_string()
            || source_conversation != record.conversation_id.to_string()
            || sequence < 0
            || u64::try_from(sequence).ok() != Some(source.sequence)
            || role != source.role
            || role == "system"
            || !content_is_nonblank(content.as_deref())
        {
            return Err(StoreError::InvalidInput(format!(
                "session review source message {} does not match its scope or snapshot",
                source.message_id
            )));
        }
    }
    Ok(())
}

fn content_is_nonblank(content: Option<&str>) -> bool {
    content
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false)
}

fn validate_record_identity(record: &SessionReviewRecordV4) -> Result<(), StoreError> {
    for (label, value) in [
        ("session review id", record.id),
        ("session review project id", record.project_id),
        ("session review conversation id", record.conversation_id),
        (
            "session review reviewer profile id",
            record.reviewer_profile_id,
        ),
    ] {
        if value.is_nil() {
            return Err(StoreError::InvalidInput(format!("{label} cannot be nil")));
        }
    }
    Ok(())
}

fn validate_initial_record(record: &SessionReviewRecordV4) -> Result<(), StoreError> {
    validate_record_identity(record)?;
    if record.status != SessionReviewStatusV4::Running {
        return Err(StoreError::InvalidInput(
            "new session review must start in running status".into(),
        ));
    }
    if record.report.is_some() || record.error.is_some() {
        return Err(StoreError::InvalidInput(
            "new session review cannot contain a report or error".into(),
        ));
    }
    validate_sha256(
        &record.reviewer_configuration_hash,
        "reviewer configuration hash",
    )?;
    validate_sha256(&record.source_snapshot_sha256, "source snapshot hash")?;
    if record.created_at > record.updated_at {
        return Err(StoreError::InvalidInput(
            "session review timestamps are out of order".into(),
        ));
    }
    if record.sources.is_empty() || record.sources.len() > SESSION_REVIEW_MAX_SOURCES {
        return Err(StoreError::InvalidInput(format!(
            "session review source count must be between 1 and {SESSION_REVIEW_MAX_SOURCES}"
        )));
    }
    if record.source_message_count < record.sources.len() as u64 {
        return Err(StoreError::InvalidInput(
            "session review source count exceeds the conversation count".into(),
        ));
    }
    let mut ids = HashSet::with_capacity(record.sources.len());
    let mut previous_sequence = None;
    for source in &record.sources {
        if source.message_id.is_nil() || !ids.insert(source.message_id) {
            return Err(StoreError::InvalidInput(
                "session review source message ids must be unique and non-nil".into(),
            ));
        }
        if !matches!(source.role.as_str(), "user" | "assistant" | "tool") {
            return Err(StoreError::InvalidInput(
                "session review source role is not eligible".into(),
            ));
        }
        if source.text.trim().is_empty() || source.text.len() > SESSION_REVIEW_MAX_SOURCE_TEXT_BYTES
        {
            return Err(StoreError::InvalidInput(format!(
                "session review source text must be nonempty and at most {SESSION_REVIEW_MAX_SOURCE_TEXT_BYTES} bytes"
            )));
        }
        if previous_sequence.is_some_and(|previous| source.sequence <= previous) {
            return Err(StoreError::InvalidInput(
                "session review sources must be ordered by message sequence".into(),
            ));
        }
        previous_sequence = Some(source.sequence);
    }
    let snapshot =
        session_review_source_snapshot_value(record.source_message_count, &record.sources);
    let snapshot_bytes = serde_json::to_vec(&snapshot)?;
    if snapshot_bytes.len() > SESSION_REVIEW_MAX_SOURCE_SNAPSHOT_BYTES {
        return Err(StoreError::InvalidInput(format!(
            "session review source snapshot exceeds {SESSION_REVIEW_MAX_SOURCE_SNAPSHOT_BYTES} bytes"
        )));
    }
    let expected_hash =
        session_review_source_snapshot_hash(record.source_message_count, &record.sources)?;
    if record.source_snapshot_sha256 != expected_hash {
        return Err(StoreError::InvalidInput(
            "session review source snapshot hash does not match its sources".into(),
        ));
    }
    Ok(())
}

fn validate_report(
    report: &SessionReviewReportV4,
    sources: &[SessionReviewSourceV4],
) -> Result<(), StoreError> {
    if report.summary.trim().is_empty()
        || report.summary.len() > omicsops_protocol::SESSION_REVIEW_MAX_REPORT_SUMMARY_BYTES
        || report.findings.len() > SESSION_REVIEW_MAX_FINDINGS
    {
        return Err(StoreError::InvalidInput(
            "session review report summary or finding count is invalid".into(),
        ));
    }
    let known = sources
        .iter()
        .map(|source| source.message_id)
        .collect::<HashSet<_>>();
    for finding in &report.findings {
        validate_finding(finding, &known)?;
    }
    if serde_json::to_vec(report)?.len() > SESSION_REVIEW_MAX_SOURCE_SNAPSHOT_BYTES {
        return Err(StoreError::InvalidInput(
            "session review report exceeds its size limit".into(),
        ));
    }
    Ok(())
}

fn validate_finding(
    finding: &SessionReviewFindingV4,
    known: &HashSet<Uuid>,
) -> Result<(), StoreError> {
    if finding.code.trim().is_empty()
        || finding.code.len() > SESSION_REVIEW_MAX_FINDING_CODE_BYTES
        || finding.message.trim().is_empty()
        || finding.message.len() > SESSION_REVIEW_MAX_FINDING_MESSAGE_BYTES
        || finding.source_ids.is_empty()
        || finding.source_ids.len() > SESSION_REVIEW_MAX_FINDING_SOURCES
    {
        return Err(StoreError::InvalidInput(
            "session review finding content is invalid".into(),
        ));
    }
    if finding
        .source_ids
        .iter()
        .any(|id| id.is_nil() || !known.contains(id))
    {
        return Err(StoreError::InvalidInput(
            "session review finding cites an unknown source".into(),
        ));
    }
    let mut ids = HashSet::with_capacity(finding.source_ids.len());
    if finding.source_ids.iter().any(|id| !ids.insert(*id)) {
        return Err(StoreError::InvalidInput(
            "session review finding source ids must be unique".into(),
        ));
    }
    Ok(())
}

fn validate_error(error: &str) -> Result<(), StoreError> {
    if error.is_empty() || error.len() > SESSION_REVIEW_MAX_ERROR_BYTES {
        return Err(StoreError::InvalidInput(format!(
            "session review error must be nonempty and at most {SESSION_REVIEW_MAX_ERROR_BYTES} bytes"
        )));
    }
    Ok(())
}

fn validate_sha256(value: &str, label: &str) -> Result<(), StoreError> {
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(StoreError::InvalidInput(format!(
            "{label} must be a 64-character hexadecimal digest"
        )));
    }
    Ok(())
}

fn status_string(status: SessionReviewStatusV4) -> &'static str {
    match status {
        SessionReviewStatusV4::Running => "running",
        SessionReviewStatusV4::Completed => "completed",
        SessionReviewStatusV4::Failed => "failed",
        SessionReviewStatusV4::Abandoned => "abandoned",
    }
}

fn parse_status(value: &str) -> Result<SessionReviewStatusV4, StoreError> {
    match value {
        "running" => Ok(SessionReviewStatusV4::Running),
        "completed" => Ok(SessionReviewStatusV4::Completed),
        "failed" => Ok(SessionReviewStatusV4::Failed),
        "abandoned" => Ok(SessionReviewStatusV4::Abandoned),
        _ => Err(StoreError::InvalidInput(
            "stored session review has an invalid status".into(),
        )),
    }
}

fn session_review_from_row(
    row: sqlx::sqlite::SqliteRow,
) -> Result<SessionReviewRecordV4, StoreError> {
    let stored_id = Uuid::parse_str(&row.try_get::<String, _>(0)?).map_err(|error| {
        StoreError::InvalidInput(format!("stored session review id is invalid: {error}"))
    })?;
    let frame_id = Uuid::parse_str(&row.try_get::<String, _>(1)?).map_err(|error| {
        StoreError::InvalidInput(format!(
            "stored session review frame id is invalid: {error}"
        ))
    })?;
    let reviewer_id = Uuid::parse_str(&row.try_get::<String, _>(2)?).map_err(|error| {
        StoreError::InvalidInput(format!(
            "stored session review profile id is invalid: {error}"
        ))
    })?;
    let status = parse_status(&row.try_get::<String, _>(3)?)?;
    let value = row.try_get::<String, _>(4)?;
    let record: SessionReviewRecordV4 = serde_json::from_str(&value).map_err(|error| {
        StoreError::InvalidInput(format!("stored session review record is invalid: {error}"))
    })?;
    let created_at = from_timestamp(row.try_get(5)?, "session review created_at")?;
    let updated_at = from_timestamp(row.try_get(6)?, "session review updated_at")?;
    if record.id != stored_id
        || record.conversation_id != frame_id
        || record.reviewer_profile_id != reviewer_id
        || record.status != status
        || timestamp(record.created_at) != timestamp(created_at)
        || timestamp(record.updated_at) != timestamp(updated_at)
    {
        return Err(StoreError::InvalidInput(
            "stored session review columns do not match its record".into(),
        ));
    }
    Ok(record)
}

async fn load_session_review_in_tx(
    tx: &mut SqliteConnection,
    request_id: Uuid,
) -> Result<Option<SessionReviewRecordV4>, StoreError> {
    sqlx::query(
        "SELECT id,frame_id,reviewer,status,value_json,created_at,updated_at
         FROM session_reviews WHERE id=?1",
    )
    .bind(request_id.to_string())
    .fetch_optional(&mut *tx)
    .await?
    .map(session_review_from_row)
    .transpose()
}

fn ensure_record_scope(
    record: &SessionReviewRecordV4,
    project_id: Uuid,
    conversation_id: Uuid,
) -> Result<(), StoreError> {
    if record.project_id != project_id || record.conversation_id != conversation_id {
        return Err(StoreError::InvalidInput(
            "session review does not belong to the requested scope".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;
    use omicsops_core::workspace::{Conversation, Message, MessageRole, Project, ProjectTemplate};
    use omicsops_protocol::{
        ReviewerBackendChoiceV4, SessionReviewFindingV4, SessionReviewSeverityV4,
        session_review_source_snapshot_hash,
    };

    struct Fixture {
        store: Store,
        project_id: Uuid,
        conversation_id: Uuid,
        messages: Vec<Message>,
    }

    async fn fixture() -> Fixture {
        let store = Store::open_in_memory().await.expect("store");
        let now = Utc::now();
        let project_id = Uuid::new_v4();
        store
            .save_project(&Project::new(
                project_id,
                "review project",
                "review-root",
                ProjectTemplate::Blank,
                now,
            ))
            .await
            .expect("project");
        let conversation_id = Uuid::new_v4();
        store
            .save_conversation(&Conversation::new(
                conversation_id,
                project_id,
                "review conversation",
                now,
            ))
            .await
            .expect("conversation");
        let messages = vec![
            Message::markdown(
                Uuid::new_v4(),
                project_id,
                conversation_id,
                1,
                MessageRole::User,
                "Please inspect the sample table.",
                now,
            ),
            Message::markdown(
                Uuid::new_v4(),
                project_id,
                conversation_id,
                2,
                MessageRole::Assistant,
                "The sample table contains three rows.",
                now + Duration::milliseconds(1),
            ),
        ];
        for message in &messages {
            store.save_message(message).await.expect("message");
        }
        Fixture {
            store,
            project_id,
            conversation_id,
            messages,
        }
    }

    fn record(fixture: &Fixture, request_id: Uuid) -> SessionReviewRecordV4 {
        let sources = fixture
            .messages
            .iter()
            .map(|message| SessionReviewSourceV4 {
                message_id: message.id,
                sequence: message.sequence,
                role: match message.role {
                    MessageRole::User => "user",
                    MessageRole::Assistant => "assistant",
                    MessageRole::Tool => "tool",
                    MessageRole::System => "system",
                }
                .into(),
                text: message.markdown.clone(),
            })
            .collect::<Vec<_>>();
        let now = Utc::now();
        SessionReviewRecordV4 {
            id: request_id,
            project_id: fixture.project_id,
            conversation_id: fixture.conversation_id,
            reviewer_profile_id: Uuid::new_v4(),
            reviewer_configuration_hash: "a".repeat(64),
            source_snapshot_sha256: session_review_source_snapshot_hash(
                sources.len() as u64,
                &sources,
            )
            .expect("source hash"),
            source_message_count: sources.len() as u64,
            sources,
            status: SessionReviewStatusV4::Running,
            report: None,
            error: None,
            service_tier: None,
            created_at: now,
            updated_at: now,
        }
    }

    fn report(source_id: Uuid) -> SessionReviewReportV4 {
        SessionReviewReportV4 {
            summary: "The transcript is internally consistent.".into(),
            findings: vec![SessionReviewFindingV4 {
                severity: SessionReviewSeverityV4::Ok,
                code: "consistent".into(),
                message: "The assistant's statement is supported by the visible exchange.".into(),
                source_ids: vec![source_id],
            }],
        }
    }

    #[tokio::test]
    async fn acquire_is_idempotent_and_other_request_is_busy() {
        let fixture = fixture().await;
        let first = record(&fixture, Uuid::new_v4());
        let acquired = fixture
            .store
            .begin_session_review(first.clone())
            .await
            .expect("acquire");
        assert!(acquired.acquired);
        let retry = fixture
            .store
            .begin_session_review(first.clone())
            .await
            .expect("idempotent retry");
        assert!(!retry.acquired);
        assert_eq!(retry.record, acquired.record);

        let other = record(&fixture, Uuid::new_v4());
        let error = fixture
            .store
            .begin_session_review(other)
            .await
            .expect_err("other request must be busy");
        assert!(error.to_string().contains("active session review"));
    }

    #[tokio::test]
    async fn same_request_id_returns_cached_record_even_when_incoming_snapshot_differs() {
        let fixture = fixture().await;
        let first = record(&fixture, Uuid::new_v4());
        let acquired = fixture
            .store
            .begin_session_review(first.clone())
            .await
            .expect("acquire");
        let mut changed = first;
        changed.reviewer_profile_id = Uuid::new_v4();
        changed.reviewer_configuration_hash = "b".repeat(64);
        changed.service_tier = Some(omicsops_protocol::RunServiceTierV4 {
            fast_mode: Some(true),
        });
        let retry = fixture
            .store
            .begin_session_review(changed)
            .await
            .expect("cached retry");
        assert!(!retry.acquired);
        assert_eq!(retry.record, acquired.record);
    }

    #[tokio::test]
    async fn source_scope_latest_and_stale_snapshot_are_rejected() {
        let fixture = fixture().await;
        let mut foreign = record(&fixture, Uuid::new_v4());
        foreign.sources[0].message_id = Uuid::new_v4();
        foreign.source_snapshot_sha256 =
            session_review_source_snapshot_hash(foreign.source_message_count, &foreign.sources)
                .expect("hash");
        let error = fixture
            .store
            .begin_session_review(foreign)
            .await
            .expect_err("foreign source must fail");
        assert!(
            error.to_string().contains("source message") || error.to_string().contains("source")
        );

        let stale = record(&fixture, Uuid::new_v4());
        let added = Message::markdown(
            Uuid::new_v4(),
            fixture.project_id,
            fixture.conversation_id,
            3,
            MessageRole::User,
            "A later message makes the snapshot stale.",
            Utc::now(),
        );
        fixture.store.save_message(&added).await.expect("message");
        let error = fixture
            .store
            .begin_session_review(stale)
            .await
            .expect_err("stale source must fail");
        assert!(error.to_string().contains("stale") || error.to_string().contains("current"));
    }

    #[tokio::test]
    async fn unicode_and_control_whitespace_messages_match_native_snapshot_eligibility() {
        let fixture = fixture().await;
        for (sequence, whitespace) in [(3, "\n\t"), (4, "\u{2003}\u{00a0}")] {
            fixture
                .store
                .save_message(&Message::markdown(
                    Uuid::new_v4(),
                    fixture.project_id,
                    fixture.conversation_id,
                    sequence,
                    MessageRole::User,
                    whitespace,
                    Utc::now(),
                ))
                .await
                .expect("whitespace message");
        }

        // Native review_sources filters both entries with str::trim(), so
        // the original two-message snapshot remains current and acquirable.
        let acquired = fixture
            .store
            .begin_session_review(record(&fixture, Uuid::new_v4()))
            .await
            .expect("whitespace-only messages do not make the snapshot stale");
        assert!(acquired.acquired);
    }

    #[tokio::test]
    async fn complete_and_fail_are_bounded_compare_and_set_transitions() {
        let fixture = fixture().await;
        let first = record(&fixture, Uuid::new_v4());
        let source_id = first.sources[0].message_id;
        fixture
            .store
            .begin_session_review(first.clone())
            .await
            .expect("acquire");
        let completed = fixture
            .store
            .complete_session_review(
                fixture.project_id,
                fixture.conversation_id,
                first.id,
                report(source_id),
            )
            .await
            .expect("complete");
        assert_eq!(completed.status, SessionReviewStatusV4::Completed);
        assert!(
            fixture
                .store
                .complete_session_review(
                    fixture.project_id,
                    fixture.conversation_id,
                    first.id,
                    report(source_id),
                )
                .await
                .is_err()
        );
        assert!(
            !fixture
                .store
                .abandon_session_review(fixture.project_id, fixture.conversation_id, first.id)
                .await
                .expect("completed review remains terminal")
        );
        let completed_stored = fixture
            .store
            .get_session_review(fixture.project_id, fixture.conversation_id, first.id)
            .await
            .expect("completed review lookup")
            .expect("completed review");
        assert_eq!(completed_stored.status, SessionReviewStatusV4::Completed);
        assert!(completed_stored.report.is_some());

        let second = record(&fixture, Uuid::new_v4());
        fixture
            .store
            .begin_session_review(second.clone())
            .await
            .expect("second acquire");
        let failed = fixture
            .store
            .fail_session_review(
                fixture.project_id,
                fixture.conversation_id,
                second.id,
                "provider timed out",
            )
            .await
            .expect("fail");
        assert_eq!(failed.status, SessionReviewStatusV4::Failed);
        assert_eq!(failed.error.as_deref(), Some("provider timed out"));
        assert!(
            !fixture
                .store
                .abandon_session_review(fixture.project_id, fixture.conversation_id, second.id)
                .await
                .expect("failed review remains terminal")
        );
        let failed_stored = fixture
            .store
            .get_session_review(fixture.project_id, fixture.conversation_id, second.id)
            .await
            .expect("failed review lookup")
            .expect("failed review");
        assert_eq!(failed_stored.status, SessionReviewStatusV4::Failed);
        assert_eq!(failed_stored.error.as_deref(), Some("provider timed out"));
    }

    #[tokio::test]
    async fn active_run_blocks_acquire_and_review_blocks_ordinary_message() {
        let fixture = fixture().await;
        let run_id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO agent_runs_v4(run_id,project_id,conversation_id,status,value_json)
             VALUES(?1,?2,?3,'running','{}')",
        )
        .bind(run_id.to_string())
        .bind(fixture.project_id.to_string())
        .bind(fixture.conversation_id.to_string())
        .execute(fixture.store.pool())
        .await
        .expect("active run");
        let error = fixture
            .store
            .begin_session_review(record(&fixture, Uuid::new_v4()))
            .await
            .expect_err("run lock");
        assert!(error.to_string().contains("active run"));
        sqlx::query("DELETE FROM agent_runs_v4 WHERE run_id=?1")
            .bind(run_id.to_string())
            .execute(fixture.store.pool())
            .await
            .expect("delete run");

        let plan_run_id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO agent_runs_v4(run_id,project_id,conversation_id,status,value_json)
             VALUES(?1,?2,?3,'running','{}')",
        )
        .bind(plan_run_id.to_string())
        .bind(fixture.project_id.to_string())
        .bind(fixture.conversation_id.to_string())
        .execute(fixture.store.pool())
        .await
        .expect("plan run");
        sqlx::query(
            "INSERT INTO proposed_plans
             (id,project_id,frame_id,revision,plan_hash,status,plan_json,run_id)
             VALUES(?1,?2,?3,1,'plan-hash','pending','{}',?4)",
        )
        .bind(Uuid::new_v4().to_string())
        .bind(fixture.project_id.to_string())
        .bind(fixture.conversation_id.to_string())
        .bind(plan_run_id.to_string())
        .execute(fixture.store.pool())
        .await
        .expect("active plan");
        let error = fixture
            .store
            .begin_session_review(record(&fixture, Uuid::new_v4()))
            .await
            .expect_err("plan lock");
        assert!(error.to_string().contains("active plan"));
        sqlx::query("DELETE FROM proposed_plans WHERE run_id=?1")
            .bind(plan_run_id.to_string())
            .execute(fixture.store.pool())
            .await
            .expect("delete plan");
        sqlx::query("DELETE FROM agent_runs_v4 WHERE run_id=?1")
            .bind(plan_run_id.to_string())
            .execute(fixture.store.pool())
            .await
            .expect("delete plan run");

        let review = record(&fixture, Uuid::new_v4());
        fixture
            .store
            .begin_session_review(review)
            .await
            .expect("review");
        let message = Message::markdown(
            Uuid::new_v4(),
            fixture.project_id,
            fixture.conversation_id,
            3,
            MessageRole::User,
            "ordinary send",
            Utc::now(),
        );
        let error = fixture
            .store
            .save_message(&message)
            .await
            .expect_err("review lock");
        assert!(error.to_string().contains("active session review"));
    }

    #[tokio::test]
    async fn settings_round_trip_and_single_review_abandon_are_durable() {
        let fixture = fixture().await;
        assert_eq!(
            fixture.store.get_reviewer_settings().await.unwrap(),
            ReviewerSettingsV4::default()
        );
        let settings = ReviewerSettingsV4 {
            backend: ReviewerBackendChoiceV4::DefaultHttp,
            default_http_profile_id: Some(Uuid::new_v4()),
        };
        fixture
            .store
            .save_reviewer_settings(&settings)
            .await
            .expect("settings");
        assert_eq!(
            fixture.store.get_reviewer_settings().await.unwrap(),
            settings
        );

        let review = record(&fixture, Uuid::new_v4());
        fixture
            .store
            .begin_session_review(review.clone())
            .await
            .expect("review");
        let running = fixture
            .store
            .list_running_session_reviews()
            .await
            .expect("running reviews");
        assert_eq!(
            running.iter().map(|item| item.id).collect::<Vec<_>>(),
            vec![review.id]
        );
        assert_eq!(
            fixture
                .store
                .abandon_session_review(fixture.project_id, fixture.conversation_id, review.id)
                .await
                .expect("abandon"),
            true
        );
        assert_eq!(
            fixture
                .store
                .abandon_session_review(fixture.project_id, fixture.conversation_id, review.id)
                .await
                .expect("terminal abandon is idempotent"),
            false
        );
        let stored = fixture
            .store
            .get_session_review(fixture.project_id, fixture.conversation_id, review.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(stored.status, SessionReviewStatusV4::Abandoned);
    }

    #[tokio::test]
    async fn running_review_listing_is_cross_scope_and_abandon_is_scoped() {
        let fixture = fixture().await;
        let second_id = Uuid::new_v4();
        fixture
            .store
            .save_conversation(&Conversation::new(
                second_id,
                fixture.project_id,
                "second",
                Utc::now(),
            ))
            .await
            .expect("second conversation");
        let second_message = Message::markdown(
            Uuid::new_v4(),
            fixture.project_id,
            second_id,
            1,
            MessageRole::User,
            "second conversation",
            Utc::now(),
        );
        fixture
            .store
            .save_message(&second_message)
            .await
            .expect("second message");
        let second_fixture = Fixture {
            store: fixture.store.clone(),
            project_id: fixture.project_id,
            conversation_id: second_id,
            messages: vec![second_message],
        };
        let first = record(&fixture, Uuid::new_v4());
        let second = record(&second_fixture, Uuid::new_v4());
        fixture
            .store
            .begin_session_review(first.clone())
            .await
            .expect("first review");
        fixture
            .store
            .begin_session_review(second.clone())
            .await
            .expect("second review");

        let mut running = fixture
            .store
            .list_running_session_reviews()
            .await
            .expect("all running reviews");
        running.sort_by_key(|item| item.id);
        let mut expected = vec![first.id, second.id];
        expected.sort();
        assert_eq!(
            running.iter().map(|item| item.id).collect::<Vec<_>>(),
            expected
        );

        let error = fixture
            .store
            .abandon_session_review(fixture.project_id, second_id, first.id)
            .await
            .expect_err("a review cannot be abandoned through another conversation");
        assert!(error.to_string().contains("requested scope"));

        assert!(
            fixture
                .store
                .abandon_session_review(fixture.project_id, fixture.conversation_id, first.id)
                .await
                .expect("first abandon")
        );
        assert!(
            !fixture
                .store
                .abandon_session_review(fixture.project_id, fixture.conversation_id, first.id)
                .await
                .expect("repeated first abandon")
        );
        let remaining = fixture
            .store
            .list_running_session_reviews()
            .await
            .expect("remaining running review");
        assert_eq!(
            remaining.iter().map(|item| item.id).collect::<Vec<_>>(),
            vec![second.id]
        );
    }

    #[tokio::test]
    async fn project_cap_and_terminal_payload_bounds_are_enforced() {
        let fixture = fixture().await;
        for _ in 0..SESSION_REVIEW_MAX_PER_PROJECT {
            let mut stored = record(&fixture, Uuid::new_v4());
            stored.status = SessionReviewStatusV4::Completed;
            stored.report = Some(report(stored.sources[0].message_id));
            let value_json = serde_json::to_string(&stored).expect("record JSON");
            sqlx::query(
                "INSERT INTO session_reviews
                 (id,frame_id,reviewer,status,value_json,created_at,updated_at)
                 VALUES(?1,?2,?3,'completed',?4,?5,?6)",
            )
            .bind(stored.id.to_string())
            .bind(stored.conversation_id.to_string())
            .bind(stored.reviewer_profile_id.to_string())
            .bind(value_json)
            .bind(timestamp(stored.created_at))
            .bind(timestamp(stored.updated_at))
            .execute(fixture.store.pool())
            .await
            .expect("seed review");
        }
        let error = fixture
            .store
            .begin_session_review(record(&fixture, Uuid::new_v4()))
            .await
            .expect_err("project cap");
        assert!(error.to_string().contains("project cap"));

        let mut bounded = record(&fixture, Uuid::new_v4());
        bounded.status = SessionReviewStatusV4::Running;
        // The cap check is reached after source validation, so remove the
        // seeded rows before checking terminal payload limits.
        sqlx::query("DELETE FROM session_reviews")
            .execute(fixture.store.pool())
            .await
            .expect("clear seeded reviews");
        fixture
            .store
            .begin_session_review(bounded.clone())
            .await
            .expect("bounded review");
        let mut oversized = report(bounded.sources[0].message_id);
        oversized.summary =
            "x".repeat(omicsops_protocol::SESSION_REVIEW_MAX_REPORT_SUMMARY_BYTES + 1);
        assert!(
            fixture
                .store
                .complete_session_review(
                    fixture.project_id,
                    fixture.conversation_id,
                    bounded.id,
                    oversized,
                )
                .await
                .is_err()
        );
        assert!(
            fixture
                .store
                .fail_session_review(
                    fixture.project_id,
                    fixture.conversation_id,
                    bounded.id,
                    &"x".repeat(SESSION_REVIEW_MAX_ERROR_BYTES + 1),
                )
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn deletion_cascades_session_reviews_and_other_conversation_is_independent() {
        let fixture = fixture().await;
        let second_id = Uuid::new_v4();
        fixture
            .store
            .save_conversation(&Conversation::new(
                second_id,
                fixture.project_id,
                "second",
                Utc::now(),
            ))
            .await
            .expect("second conversation");
        let second_message = Message::markdown(
            Uuid::new_v4(),
            fixture.project_id,
            second_id,
            1,
            MessageRole::User,
            "second conversation",
            Utc::now(),
        );
        fixture
            .store
            .save_message(&second_message)
            .await
            .expect("second message");
        let second_fixture = Fixture {
            store: fixture.store.clone(),
            project_id: fixture.project_id,
            conversation_id: second_id,
            messages: vec![second_message],
        };
        let deleted_review_id = Uuid::new_v4();
        fixture
            .store
            .begin_session_review(record(&fixture, deleted_review_id))
            .await
            .expect("first review");
        fixture
            .store
            .begin_session_review(record(&second_fixture, Uuid::new_v4()))
            .await
            .expect("second review");
        fixture
            .store
            .delete_conversation(fixture.project_id, fixture.conversation_id)
            .await
            .expect("delete conversation");
        let remaining: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM session_reviews")
            .fetch_one(fixture.store.pool())
            .await
            .expect("count reviews");
        assert_eq!(remaining, 1);
        assert!(
            !fixture
                .store
                .abandon_session_review(
                    fixture.project_id,
                    fixture.conversation_id,
                    deleted_review_id
                )
                .await
                .expect("deleted review reconciliation is idempotent")
        );
        assert_eq!(
            fixture
                .store
                .list_session_reviews(fixture.project_id, second_id)
                .await
                .unwrap()
                .len(),
            1
        );
    }
}
