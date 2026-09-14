//! Atomic, durable conversation branch creation.
//!
//! A branch is a new conversation with a fresh frame and fresh message IDs.
//! Only the selected transcript prefix is copied; runs, approvals, tool
//! events, evidence, and other execution authority remain attached to the
//! source conversation.  The source boundary is checked again while holding
//! SQLite's immediate transaction lock so a paginated UI cannot create a
//! branch from a stale snapshot.

use chrono::Utc;
use omicsops_core::workspace::{Conversation, Message, MessageRole};
use omicsops_dto::{
    ComposerQueueModeV4, ComposerReference, ConversationBranchCheckpointKindV4,
    ConversationBranchCheckpointV4, ConversationBranchStateV4, ConversationBranchV4,
    CreateConversationBranchAndSendRequestV4, CreateConversationBranchRequestV4,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::{Row, Sqlite, Transaction};
use uuid::Uuid;

use crate::{Store, StoreError};

const MAX_BRANCH_TITLE_BYTES: usize = 256;
const MAX_QUEUE_MESSAGE_BYTES: usize = 64 * 1024;
const MAX_QUEUE_REFERENCES: usize = 12;
const MAX_QUEUE_SESSION_REFERENCES: usize = 3;
const MAX_QUEUE_ATTACHMENTS: usize = 8;
const ACTIVE_RUN_STATUSES: &str =
    "('planning','running','waiting_for_input','waiting_for_approval','awaiting_approval')";
const BRANCH_SEND_INTENT_KIND: &str = "conversation_branch_send_v4";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct BranchSendIntent {
    id: Uuid,
    project_id: Uuid,
    source_conversation_id: Uuid,
    branch_conversation_id: Uuid,
    request_hash: String,
}

impl Store {
    /// Return the source snapshot guard that a subsequent create request must
    /// carry. This is intentionally read-only; create validates it again in a
    /// `BEGIN IMMEDIATE` transaction.
    pub async fn get_conversation_branch_checkpoint_v4(
        &self,
        project_id: Uuid,
        source_conversation_id: Uuid,
        source_message_id: Uuid,
        checkpoint_kind: ConversationBranchCheckpointKindV4,
    ) -> Result<ConversationBranchCheckpointV4, StoreError> {
        let mut tx = self.pool.begin().await?;
        crate::ensure_conversation_owner_executor(&mut *tx, project_id, source_conversation_id)
            .await?;
        let messages = load_messages(&mut tx, project_id, source_conversation_id).await?;
        let checkpoint = make_checkpoint(&messages, source_message_id, checkpoint_kind)?;
        tx.commit().await?;
        Ok(checkpoint)
    }

    /// Create or retry one branch atomically. `request_id` is the stable
    /// client operation identity: an exact retry returns the already-created
    /// conversation, while a changed payload with the same ID is rejected.
    pub async fn create_conversation_branch_v4(
        &self,
        request: &CreateConversationBranchRequestV4,
    ) -> Result<ConversationBranchV4, StoreError> {
        if request.request_id.is_nil() {
            return Err(StoreError::InvalidInput(
                "branch request ID cannot be nil".into(),
            ));
        }
        let _ = to_sqlite_sequence(request.expected_source_sequence)?;
        let _ = to_sqlite_sequence(request.expected_head_sequence)?;
        let normalized_title = normalize_title(&request.title)?;
        let normalized_boundary_hash = normalize_hash(&request.expected_boundary_hash)?;
        let request_hash =
            create_request_hash(request, &normalized_title, &normalized_boundary_hash);
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;

        if let Some(existing) = load_branch_by_request_id(&mut tx, request.request_id).await? {
            if existing.project_id != request.project_id {
                return Err(StoreError::InvalidInput(
                    "branch request belongs to another project".into(),
                ));
            }
            if existing.request_hash != request_hash {
                return Err(StoreError::InvalidInput(
                    "branch request ID was reused with a different payload".into(),
                ));
            }
            tx.commit().await?;
            return Ok(existing);
        }
        let branch = create_branch_in_tx(
            &mut tx,
            request,
            &normalized_title,
            &normalized_boundary_hash,
            &request_hash,
        )
        .await?;
        tx.commit().await?;
        Ok(branch)
    }

    /// Create or retry the durable branch portion of a branch-and-send
    /// operation. Material is copied by the native host after this transaction
    /// commits; the operation intent keeps a changed request from attaching a
    /// different draft to the same branch after a lost response.
    pub async fn prepare_conversation_branch_send_v4(
        &self,
        request: &CreateConversationBranchAndSendRequestV4,
    ) -> Result<ConversationBranchV4, StoreError> {
        validate_branch_send_request(request)?;
        let _ = to_sqlite_sequence(request.branch.expected_source_sequence)?;
        let _ = to_sqlite_sequence(request.branch.expected_head_sequence)?;
        let normalized_title = normalize_title(&request.branch.title)?;
        let normalized_boundary_hash = normalize_hash(&request.branch.expected_boundary_hash)?;
        let base_hash = create_request_hash(
            &request.branch,
            &normalized_title,
            &normalized_boundary_hash,
        );
        let request_hash =
            create_branch_send_request_hash(request, &normalized_title, &normalized_boundary_hash)?;
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;

        if let Some(intent) = load_branch_send_intent(&mut tx, request.branch.request_id).await? {
            if intent.id != request.branch.request_id
                || intent.project_id != request.branch.project_id
                || intent.source_conversation_id != request.branch.source_conversation_id
                || intent.request_hash != request_hash
            {
                return Err(StoreError::InvalidInput(
                    "branch send request ID was reused with a different payload".into(),
                ));
            }
            let branch = load_branch_by_request_id(&mut tx, request.branch.request_id)
                .await?
                .ok_or_else(|| {
                    StoreError::InvalidInput("branch send intent has no branch relation".into())
                })?;
            if branch.branch_conversation_id != intent.branch_conversation_id
                || branch.project_id != request.branch.project_id
                || branch.source_conversation_id != request.branch.source_conversation_id
                || branch.request_hash != base_hash
            {
                return Err(StoreError::InvalidInput(
                    "branch send intent does not match its branch relation".into(),
                ));
            }
            tx.commit().await?;
            return Ok(branch);
        }

        let branch = if let Some(existing) =
            load_branch_by_request_id(&mut tx, request.branch.request_id).await?
        {
            if existing.project_id != request.branch.project_id
                || existing.request_hash != base_hash
            {
                return Err(StoreError::InvalidInput(
                    "branch request belongs to another scope or has a different payload".into(),
                ));
            }
            existing
        } else {
            create_branch_in_tx(
                &mut tx,
                &request.branch,
                &normalized_title,
                &normalized_boundary_hash,
                &base_hash,
            )
            .await?
        };
        let intent = BranchSendIntent {
            id: request.branch.request_id,
            project_id: request.branch.project_id,
            source_conversation_id: request.branch.source_conversation_id,
            branch_conversation_id: branch.branch_conversation_id,
            request_hash,
        };
        let value_json = serde_json::to_string(&intent)?;
        sqlx::query(
            "INSERT INTO app_objects(kind,id,value_json,updated_at)
             VALUES (?1,?2,?3,?4)",
        )
        .bind(BRANCH_SEND_INTENT_KIND)
        .bind(intent.id.to_string())
        .bind(value_json)
        .bind(crate::timestamp(branch.created_at))
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(branch)
    }

    /// Return one branch relation owned by `project_id`, distinguishing a
    /// foreign branch ID from an absent branch so callers cannot use this as a
    /// cross-project probe.
    pub async fn get_conversation_branch_v4(
        &self,
        project_id: Uuid,
        branch_conversation_id: Uuid,
    ) -> Result<Option<ConversationBranchV4>, StoreError> {
        let row = sqlx::query(
            "SELECT request_id,branch_conversation_id,project_id,source_conversation_id,
                    source_message_id,checkpoint_kind,source_sequence,source_head_sequence,
                    boundary_hash,request_hash,state,created_at,updated_at
             FROM conversation_branches_v4 WHERE branch_conversation_id=?1",
        )
        .bind(branch_conversation_id.to_string())
        .fetch_optional(&self.pool)
        .await?;
        let Some(row) = row else { return Ok(None) };
        let stored_project = crate::parse_uuid(row.try_get::<String, _>(2)?, "branch project id")?;
        if stored_project != project_id {
            return Err(StoreError::InvalidInput(
                "conversation branch does not belong to project".into(),
            ));
        }
        Ok(Some(branch_from_row(row)?))
    }

    pub async fn conversation_branches_for_source_v4(
        &self,
        project_id: Uuid,
        source_conversation_id: Uuid,
    ) -> Result<Vec<ConversationBranchV4>, StoreError> {
        let owns: i64 = sqlx::query_scalar(
            "SELECT EXISTS(
                SELECT 1 FROM conversation_records
                WHERE frame_id=?1 AND project_id=?2
            )",
        )
        .bind(source_conversation_id.to_string())
        .bind(project_id.to_string())
        .fetch_one(&self.pool)
        .await?;
        if owns == 0 {
            return Err(StoreError::InvalidInput(
                "source conversation does not belong to project".into(),
            ));
        }
        let rows = sqlx::query(
            "SELECT request_id,branch_conversation_id,project_id,source_conversation_id,
                    source_message_id,checkpoint_kind,source_sequence,source_head_sequence,
                    boundary_hash,request_hash,state,created_at,updated_at
             FROM conversation_branches_v4
             WHERE project_id=?1 AND source_conversation_id=?2
             ORDER BY created_at,branch_conversation_id",
        )
        .bind(project_id.to_string())
        .bind(source_conversation_id.to_string())
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(branch_from_row).collect()
    }
}

async fn create_branch_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    request: &CreateConversationBranchRequestV4,
    normalized_title: &str,
    normalized_boundary_hash: &str,
    request_hash: &str,
) -> Result<ConversationBranchV4, StoreError> {
    crate::ensure_conversation_unlocked_executor(
        &mut *tx,
        request.project_id,
        request.source_conversation_id,
    )
    .await?;
    ensure_no_active_run(tx, request.project_id, request.source_conversation_id).await?;
    let source_branch: i64 = sqlx::query_scalar(
        "SELECT EXISTS(
            SELECT 1 FROM conversation_branches_v4
            WHERE branch_conversation_id=?1
        )",
    )
    .bind(request.source_conversation_id.to_string())
    .fetch_one(&mut **tx)
    .await?;
    if source_branch != 0 {
        return Err(StoreError::InvalidInput(
            "branching a branch conversation is not supported".into(),
        ));
    }

    let source = load_conversation(tx, request.project_id, request.source_conversation_id).await?;
    let messages = load_messages(tx, request.project_id, request.source_conversation_id).await?;
    let checkpoint = make_checkpoint(
        &messages,
        request.source_message_id,
        request.checkpoint_kind,
    )?;
    if checkpoint.source_sequence != request.expected_source_sequence
        || checkpoint.source_head_sequence != request.expected_head_sequence
        || checkpoint.boundary_hash != normalized_boundary_hash
    {
        return Err(StoreError::InvalidInput(
            "conversation branch checkpoint is stale or changed".into(),
        ));
    }

    let branch_conversation_id = Uuid::new_v4();
    let now = Utc::now();
    let stored_now = crate::from_timestamp(crate::timestamp(now), "branch timestamp")?;
    let branch_conversation = Conversation {
        id: branch_conversation_id,
        project_id: request.project_id,
        title: normalized_title.to_owned(),
        status: omicsops_core::workspace::ConversationStatus::Idle,
        model_profile_id: source.model_profile_id,
        created_at: stored_now,
        updated_at: stored_now,
    };
    crate::insert_conversation(tx, &branch_conversation).await?;
    sqlx::query(
        "UPDATE frames
         SET parent_frame_id=?1,
             root_frame_id=(SELECT root_frame_id FROM frames WHERE id=?1)
         WHERE id=?2 AND project_id=?3",
    )
    .bind(request.source_conversation_id.to_string())
    .bind(branch_conversation_id.to_string())
    .bind(request.project_id.to_string())
    .execute(&mut **tx)
    .await?;

    let copy_end = prefix_end(
        &messages,
        request.source_message_id,
        request.checkpoint_kind,
    )?;
    for (sequence, message) in messages.iter().take(copy_end).enumerate() {
        let copied = Message::markdown(
            Uuid::new_v4(),
            request.project_id,
            branch_conversation_id,
            u64::try_from(sequence)
                .map_err(|_| StoreError::InvalidInput("branch message sequence overflow".into()))?,
            message.role,
            message.markdown.clone(),
            message.created_at,
        );
        crate::insert_message(tx, &copied).await?;
    }

    sqlx::query(
        "INSERT INTO conversation_branches_v4
         (request_id,branch_conversation_id,project_id,source_conversation_id,
          source_message_id,checkpoint_kind,source_sequence,source_head_sequence,
          boundary_hash,request_hash,state,created_at,updated_at)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,'active',?11,?11)",
    )
    .bind(request.request_id.to_string())
    .bind(branch_conversation_id.to_string())
    .bind(request.project_id.to_string())
    .bind(request.source_conversation_id.to_string())
    .bind(request.source_message_id.to_string())
    .bind(crate::enum_string(&request.checkpoint_kind)?)
    .bind(to_sqlite_sequence(checkpoint.source_sequence)?)
    .bind(to_sqlite_sequence(checkpoint.source_head_sequence)?)
    .bind(&checkpoint.boundary_hash)
    .bind(request_hash)
    .bind(crate::timestamp(stored_now))
    .execute(&mut **tx)
    .await?;
    Ok(ConversationBranchV4 {
        request_id: request.request_id,
        branch_conversation_id,
        project_id: request.project_id,
        source_conversation_id: request.source_conversation_id,
        source_message_id: request.source_message_id,
        checkpoint_kind: request.checkpoint_kind,
        source_sequence: checkpoint.source_sequence,
        source_head_sequence: checkpoint.source_head_sequence,
        boundary_hash: checkpoint.boundary_hash,
        request_hash: request_hash.to_owned(),
        state: ConversationBranchStateV4::Active,
        created_at: stored_now,
        updated_at: stored_now,
    })
}

fn validate_branch_send_request(
    request: &CreateConversationBranchAndSendRequestV4,
) -> Result<(), StoreError> {
    if request.branch.request_id.is_nil()
        || request.branch.project_id.is_nil()
        || request.branch.source_conversation_id.is_nil()
        || request.branch.source_message_id.is_nil()
    {
        return Err(StoreError::InvalidInput(
            "branch source and project IDs cannot be nil".into(),
        ));
    }
    if request.model_profile_id.is_nil()
        || request.queue_request_id.is_nil()
        || request.queue_message_id.is_nil()
        || request.queue_run_id.is_nil()
    {
        return Err(StoreError::InvalidInput(
            "branch send model and queue IDs cannot be nil".into(),
        ));
    }
    if request.queue_request_id == request.queue_message_id
        || request.queue_request_id == request.queue_run_id
        || request.queue_message_id == request.queue_run_id
    {
        return Err(StoreError::InvalidInput(
            "branch send queue IDs must be distinct".into(),
        ));
    }
    if request.message_markdown.as_bytes().len() > MAX_QUEUE_MESSAGE_BYTES {
        return Err(StoreError::InvalidInput(
            "branch send markdown exceeds the 64 KiB limit".into(),
        ));
    }
    if request.message_markdown.trim().is_empty()
        && request.references.is_empty()
        && request.attachments.is_empty()
    {
        return Err(StoreError::InvalidInput(
            "branch send request cannot be empty".into(),
        ));
    }
    if request.references.len() > MAX_QUEUE_REFERENCES {
        return Err(StoreError::InvalidInput(
            "branch send references exceed the maximum of 12".into(),
        ));
    }
    let sessions = request
        .references
        .iter()
        .filter(|reference| matches!(reference, ComposerReference::Session { .. }))
        .count();
    if sessions > MAX_QUEUE_SESSION_REFERENCES {
        return Err(StoreError::InvalidInput(
            "branch send session references exceed the maximum of 3".into(),
        ));
    }
    if request
        .references
        .iter()
        .any(|reference| !reference_belongs_to_project(request.branch.project_id, reference))
    {
        return Err(StoreError::InvalidInput(
            "branch send reference does not belong to the requested project".into(),
        ));
    }
    if request.attachments.len() > MAX_QUEUE_ATTACHMENTS {
        return Err(StoreError::InvalidInput(
            "branch send attachments exceed the maximum of 8".into(),
        ));
    }
    let mut seen = std::collections::HashSet::with_capacity(request.attachments.len());
    if request
        .attachments
        .iter()
        .any(|id| id.is_nil() || !seen.insert(*id))
    {
        return Err(StoreError::InvalidInput(
            "branch send attachments must be unique non-nil IDs".into(),
        ));
    }
    request.compute_selection.validate().map_err(|error| {
        StoreError::InvalidInput(format!("invalid branch compute selection: {error}"))
    })?;
    Ok(())
}

fn reference_belongs_to_project(project_id: Uuid, reference: &ComposerReference) -> bool {
    match reference {
        ComposerReference::Artifact {
            project_id: owner, ..
        }
        | ComposerReference::Session {
            project_id: owner, ..
        }
        | ComposerReference::Project {
            project_id: owner, ..
        }
        | ComposerReference::ExecutionContext {
            project_id: owner, ..
        }
        | ComposerReference::Runtime {
            project_id: owner, ..
        }
        | ComposerReference::WorkspaceFile {
            project_id: owner, ..
        }
        | ComposerReference::Workflow {
            project_id: owner, ..
        }
        | ComposerReference::Quote {
            project_id: owner, ..
        } => *owner == project_id,
        ComposerReference::Skill { .. } => true,
    }
}

fn create_branch_send_request_hash(
    request: &CreateConversationBranchAndSendRequestV4,
    normalized_title: &str,
    normalized_boundary_hash: &str,
) -> Result<String, StoreError> {
    #[derive(Serialize)]
    struct Canonical<'a> {
        branch_request_id: Uuid,
        project_id: Uuid,
        source_conversation_id: Uuid,
        source_message_id: Uuid,
        checkpoint_kind: ConversationBranchCheckpointKindV4,
        expected_source_sequence: u64,
        expected_head_sequence: u64,
        expected_boundary_hash: &'a str,
        title: &'a str,
        message_markdown: &'a str,
        mode: ComposerQueueModeV4,
        model_profile_id: Uuid,
        compute_selection: &'a omicsops_protocol::ComputeSelectionV4,
        queue_request_id: Uuid,
        queue_message_id: Uuid,
        queue_run_id: Uuid,
        references: &'a [ComposerReference],
        attachments: &'a [Uuid],
    }
    let branch = &request.branch;
    let canonical = Canonical {
        branch_request_id: branch.request_id,
        project_id: branch.project_id,
        source_conversation_id: branch.source_conversation_id,
        source_message_id: branch.source_message_id,
        checkpoint_kind: branch.checkpoint_kind,
        expected_source_sequence: branch.expected_source_sequence,
        expected_head_sequence: branch.expected_head_sequence,
        expected_boundary_hash: normalized_boundary_hash,
        title: normalized_title,
        message_markdown: &request.message_markdown,
        mode: request.mode,
        model_profile_id: request.model_profile_id,
        compute_selection: &request.compute_selection,
        queue_request_id: request.queue_request_id,
        queue_message_id: request.queue_message_id,
        queue_run_id: request.queue_run_id,
        references: &request.references,
        attachments: &request.attachments,
    };
    let mut hasher = Sha256::new();
    hasher.update(b"omicsops-conversation-branch-send-request-v4\0");
    hasher.update(serde_json::to_vec(&canonical)?);
    Ok(hex::encode(hasher.finalize()))
}

async fn load_branch_send_intent(
    tx: &mut Transaction<'_, Sqlite>,
    request_id: Uuid,
) -> Result<Option<BranchSendIntent>, StoreError> {
    let row = sqlx::query("SELECT value_json FROM app_objects WHERE kind=?1 AND id=?2")
        .bind(BRANCH_SEND_INTENT_KIND)
        .bind(request_id.to_string())
        .fetch_optional(&mut **tx)
        .await?;
    row.map(|row| {
        let value = row.try_get::<String, _>(0)?;
        serde_json::from_str(&value).map_err(|error| {
            StoreError::InvalidInput(format!("invalid branch send intent: {error}"))
        })
    })
    .transpose()
}

async fn load_conversation(
    tx: &mut Transaction<'_, Sqlite>,
    project_id: Uuid,
    conversation_id: Uuid,
) -> Result<Conversation, StoreError> {
    let row = sqlx::query(
        "SELECT frame_id,project_id,title,status,model_profile_id,created_at,updated_at
         FROM conversation_records WHERE frame_id=?1 AND project_id=?2",
    )
    .bind(conversation_id.to_string())
    .bind(project_id.to_string())
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| {
        StoreError::InvalidInput(format!(
            "conversation {conversation_id} does not belong to project {project_id}"
        ))
    })?;
    crate::conversation_from_row(row)
}

async fn load_messages(
    tx: &mut Transaction<'_, Sqlite>,
    project_id: Uuid,
    conversation_id: Uuid,
) -> Result<Vec<Message>, StoreError> {
    let rows = sqlx::query(
        "SELECT m.id,m.project_id,m.conversation_id,m.seq,m.role,m.content,m.ts,
                COALESCE(m.project_id,c.project_id) AS resolved_project_id
         FROM messages m LEFT JOIN conversation_records c ON c.frame_id=m.frame_id
         WHERE m.frame_id=?1 AND COALESCE(m.project_id,c.project_id)=?2
         ORDER BY m.seq,m.id",
    )
    .bind(conversation_id.to_string())
    .bind(project_id.to_string())
    .fetch_all(&mut **tx)
    .await?;
    rows.into_iter().map(crate::message_from_row).collect()
}

async fn ensure_no_active_run(
    tx: &mut Transaction<'_, Sqlite>,
    project_id: Uuid,
    conversation_id: Uuid,
) -> Result<(), StoreError> {
    let active: i64 = sqlx::query_scalar(&format!(
        "SELECT EXISTS(
            SELECT 1 FROM agent_runs_v4
            WHERE project_id=?1 AND conversation_id=?2 AND status IN {ACTIVE_RUN_STATUSES}
        )"
    ))
    .bind(project_id.to_string())
    .bind(conversation_id.to_string())
    .fetch_one(&mut **tx)
    .await?;
    if active != 0 {
        return Err(StoreError::InvalidInput(
            "conversation is locked by an active run".into(),
        ));
    }
    Ok(())
}

async fn load_branch_by_request_id(
    tx: &mut Transaction<'_, Sqlite>,
    request_id: Uuid,
) -> Result<Option<ConversationBranchV4>, StoreError> {
    let row = sqlx::query(
        "SELECT request_id,branch_conversation_id,project_id,source_conversation_id,
                source_message_id,checkpoint_kind,source_sequence,source_head_sequence,
                boundary_hash,request_hash,state,created_at,updated_at
         FROM conversation_branches_v4 WHERE request_id=?1",
    )
    .bind(request_id.to_string())
    .fetch_optional(&mut **tx)
    .await?;
    row.map(branch_from_row).transpose()
}

fn make_checkpoint(
    messages: &[Message],
    source_message_id: Uuid,
    checkpoint_kind: ConversationBranchCheckpointKindV4,
) -> Result<ConversationBranchCheckpointV4, StoreError> {
    let Some(anchor_index) = messages
        .iter()
        .position(|message| message.id == source_message_id)
    else {
        return Err(StoreError::InvalidInput(
            "branch checkpoint message was not found in the source conversation".into(),
        ));
    };
    let anchor = &messages[anchor_index];
    if anchor.markdown.trim().is_empty()
        || (checkpoint_kind == ConversationBranchCheckpointKindV4::BeforeUser
            && anchor.role != MessageRole::User)
        || (checkpoint_kind == ConversationBranchCheckpointKindV4::AfterResponse
            && !matches!(anchor.role, MessageRole::User | MessageRole::Assistant))
    {
        return Err(StoreError::InvalidInput(
            "branch checkpoint must identify a non-empty user or assistant message".into(),
        ));
    }
    if checkpoint_kind == ConversationBranchCheckpointKindV4::AfterResponse
        && anchor.role == MessageRole::Assistant
        && !messages[..anchor_index]
            .iter()
            .rev()
            .any(|message| message.role == MessageRole::User && !message.markdown.trim().is_empty())
    {
        return Err(StoreError::InvalidInput(
            "assistant checkpoint has no preceding user message".into(),
        ));
    }
    let source_head_sequence = messages
        .last()
        .map(|message| message.sequence)
        .ok_or_else(|| StoreError::InvalidInput("source conversation has no messages".into()))?;
    Ok(ConversationBranchCheckpointV4 {
        source_message_id,
        source_sequence: anchor.sequence,
        source_head_sequence,
        checkpoint_kind,
        boundary_hash: boundary_hash(messages, source_message_id, checkpoint_kind)?,
    })
}

fn prefix_end(
    messages: &[Message],
    source_message_id: Uuid,
    checkpoint_kind: ConversationBranchCheckpointKindV4,
) -> Result<usize, StoreError> {
    let anchor_index = messages
        .iter()
        .position(|message| message.id == source_message_id)
        .ok_or_else(|| {
            StoreError::InvalidInput("branch checkpoint message was not found".into())
        })?;
    Ok(match checkpoint_kind {
        ConversationBranchCheckpointKindV4::BeforeUser => anchor_index,
        ConversationBranchCheckpointKindV4::AfterResponse => {
            let turn_start = if messages[anchor_index].role == MessageRole::Assistant {
                messages[..anchor_index]
                    .iter()
                    .rposition(|message| {
                        message.role == MessageRole::User && !message.markdown.trim().is_empty()
                    })
                    .ok_or_else(|| {
                        StoreError::InvalidInput(
                            "assistant checkpoint has no preceding user message".into(),
                        )
                    })?
            } else {
                anchor_index
            };
            messages
                .iter()
                .enumerate()
                .skip(turn_start + 1)
                .find(|(_, message)| {
                    message.role == MessageRole::User && !message.markdown.trim().is_empty()
                })
                .map(|(index, _)| index)
                .unwrap_or(messages.len())
        }
    })
}

fn boundary_hash(
    messages: &[Message],
    source_message_id: Uuid,
    checkpoint_kind: ConversationBranchCheckpointKindV4,
) -> Result<String, StoreError> {
    let mut hasher = Sha256::new();
    hasher.update(b"omicsops-conversation-branch-boundary-v4\0");
    hasher.update(source_message_id.as_bytes());
    hasher.update(crate::enum_string(&checkpoint_kind)?.as_bytes());
    for message in messages {
        hasher.update(message.id.as_bytes());
        hasher.update(message.sequence.to_le_bytes());
        let role = crate::enum_string(&message.role)?;
        hash_bytes(&mut hasher, role.as_bytes());
        hash_bytes(&mut hasher, message.markdown.as_bytes());
    }
    Ok(hex::encode(hasher.finalize()))
}

fn hash_bytes(hasher: &mut Sha256, bytes: &[u8]) {
    hasher.update((bytes.len() as u64).to_le_bytes());
    hasher.update(bytes);
}

fn create_request_hash(
    request: &CreateConversationBranchRequestV4,
    normalized_title: &str,
    normalized_boundary_hash: &str,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"omicsops-conversation-branch-request-v4\0");
    hasher.update(request.project_id.as_bytes());
    hasher.update(request.source_conversation_id.as_bytes());
    hasher.update(request.source_message_id.as_bytes());
    hasher.update(
        crate::enum_string(&request.checkpoint_kind)
            .unwrap_or_else(|_| "invalid".into())
            .as_bytes(),
    );
    hasher.update(request.expected_source_sequence.to_le_bytes());
    hasher.update(request.expected_head_sequence.to_le_bytes());
    hash_bytes(&mut hasher, normalized_boundary_hash.as_bytes());
    hash_bytes(&mut hasher, normalized_title.as_bytes());
    hex::encode(hasher.finalize())
}

fn normalize_hash(value: &str) -> Result<String, StoreError> {
    let normalized = value.trim().to_ascii_lowercase();
    if normalized.len() != 64 || hex::decode(&normalized).is_err() {
        return Err(StoreError::InvalidInput(
            "branch boundary hash must be 64 hexadecimal characters".into(),
        ));
    }
    Ok(normalized)
}

fn normalize_title(value: &str) -> Result<String, StoreError> {
    let title = value.trim();
    let title = if title.is_empty() { "Branch" } else { title };
    if title.len() > MAX_BRANCH_TITLE_BYTES {
        return Err(StoreError::InvalidInput(format!(
            "branch title exceeds {MAX_BRANCH_TITLE_BYTES} UTF-8 bytes"
        )));
    }
    Ok(title.to_owned())
}

fn to_sqlite_sequence(sequence: u64) -> Result<i64, StoreError> {
    i64::try_from(sequence)
        .map_err(|_| StoreError::InvalidInput("branch source sequence exceeds SQLite range".into()))
}

fn branch_from_row(row: sqlx::sqlite::SqliteRow) -> Result<ConversationBranchV4, StoreError> {
    Ok(ConversationBranchV4 {
        request_id: crate::parse_uuid(row.try_get::<String, _>(0)?, "branch request id")?,
        branch_conversation_id: crate::parse_uuid(
            row.try_get::<String, _>(1)?,
            "branch conversation id",
        )?,
        project_id: crate::parse_uuid(row.try_get::<String, _>(2)?, "branch project id")?,
        source_conversation_id: crate::parse_uuid(
            row.try_get::<String, _>(3)?,
            "branch source conversation id",
        )?,
        source_message_id: crate::parse_uuid(
            row.try_get::<String, _>(4)?,
            "branch source message id",
        )?,
        checkpoint_kind: crate::parse_json_enum(
            row.try_get::<String, _>(5)?.as_str(),
            "branch checkpoint kind",
        )?,
        source_sequence: u64::try_from(row.try_get::<i64, _>(6)?)
            .map_err(|_| StoreError::InvalidInput("branch source sequence is negative".into()))?,
        source_head_sequence: u64::try_from(row.try_get::<i64, _>(7)?).map_err(|_| {
            StoreError::InvalidInput("branch source head sequence is negative".into())
        })?,
        boundary_hash: row.try_get(8)?,
        request_hash: row.try_get(9)?,
        state: crate::parse_json_enum(row.try_get::<String, _>(10)?.as_str(), "branch state")?,
        created_at: crate::from_timestamp(row.try_get(11)?, "branch created_at")?,
        updated_at: crate::from_timestamp(row.try_get(12)?, "branch updated_at")?,
    })
}
