//! Durable workspace groups, append-only drafts and immutable personal collections.
use crate::{Store, StoreError};
use chrono::{DateTime, Utc};
use omicsops_core::redaction::redact_secrets;
use omicsops_dto::*;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use sha2::{Digest, Sha256};
use sqlx::{Row, SqliteConnection, sqlite::SqliteRow};
use uuid::Uuid;

const MAX_TEXT_BYTES: usize = 1_048_576;
const MAX_SNAPSHOT_BYTES: usize = 262_144;
const MAX_REFERENCES: usize = 32;
const MAX_HISTORY_REVISIONS: i64 = 1_000;
const MAX_HISTORY_BYTES: i64 = 16_777_216;

#[derive(Debug, Clone)]
pub struct SavePublicationRecord {
    pub request: SavePublicationRequest,
    pub references: Vec<WorkspaceSourceSnapshot>,
}
#[derive(Debug, Clone)]
pub struct SaveLibraryRecord {
    pub request: SaveLibraryItemRequest,
    pub snapshot: WorkspaceSourceSnapshot,
    pub source_project_name: String,
    pub source_conversation_title: Option<String>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PublicationContent {
    schema_version: u32,
    title: String,
    markdown: String,
    references: Vec<WorkspaceSourceSnapshot>,
}
#[derive(Serialize, Deserialize)]
struct PublicationReceipt {
    project_id: Uuid,
    publication_id: Uuid,
    revision: u64,
}

impl Store {
    pub async fn workspace_list_groups(
        &self,
        project_id: Uuid,
    ) -> Result<ConversationGroupState, StoreError> {
        let mut tx = self.pool.begin().await?;
        ensure_project(&mut tx, project_id).await?;
        let rows = sqlx::query("SELECT * FROM conversation_groups WHERE project_id=? ORDER BY created_at,id LIMIT 1001")
            .bind(project_id.to_string()).fetch_all(&mut *tx).await?;
        if rows.len() > 1000 {
            return Err(invalid("group list exceeds 1000 groups"));
        }
        let groups = rows.iter().map(group_from_row).collect::<Result<_, _>>()?;
        let rows = sqlx::query("SELECT conversation_id,group_id FROM conversation_group_members WHERE project_id=? ORDER BY conversation_id LIMIT 10001")
            .bind(project_id.to_string()).fetch_all(&mut *tx).await?;
        if rows.len() > 10_000 {
            return Err(invalid("membership list exceeds 10000 conversations"));
        }
        let memberships = rows
            .iter()
            .map(|row| {
                Ok(ConversationGroupMembership {
                    conversation_id: row_uuid(row, "conversation_id")?,
                    group_id: row_uuid(row, "group_id")?,
                })
            })
            .collect::<Result<_, StoreError>>()?;
        tx.commit().await?;
        Ok(ConversationGroupState {
            groups,
            memberships,
        })
    }

    pub async fn workspace_save_group(
        &self,
        request: &SaveGroupRequest,
    ) -> Result<ConversationGroup, StoreError> {
        let name = normalized_label(&request.name, 80, "group name")?;
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        if let Some(result) = replay(&mut tx, request.request_id, "save_group", request).await? {
            return Ok(result);
        }
        ensure_project(&mut tx, request.project_id).await?;
        let now = Utc::now().timestamp_millis();
        let id = request.group_id.unwrap_or_else(Uuid::new_v4);
        if request.group_id.is_some() {
            ensure_group(&mut tx, request.project_id, id).await?;
            sqlx::query(
                "UPDATE conversation_groups SET name=?,updated_at=? WHERE id=? AND project_id=?",
            )
            .bind(&name)
            .bind(now)
            .bind(id.to_string())
            .bind(request.project_id.to_string())
            .execute(&mut *tx)
            .await?;
        } else {
            sqlx::query("INSERT INTO conversation_groups(id,project_id,name,created_at,updated_at) VALUES(?,?,?,?,?)")
                .bind(id.to_string()).bind(request.project_id.to_string()).bind(name).bind(now).bind(now).execute(&mut *tx).await?;
        }
        let row = sqlx::query("SELECT * FROM conversation_groups WHERE id=?")
            .bind(id.to_string())
            .fetch_one(&mut *tx)
            .await?;
        let group = group_from_row(&row)?;
        remember(&mut tx, request.request_id, "save_group", request, &group).await?;
        tx.commit().await?;
        Ok(group)
    }

    pub async fn workspace_delete_group(
        &self,
        project_id: Uuid,
        group_id: Uuid,
    ) -> Result<(), StoreError> {
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        ensure_group(&mut tx, project_id, group_id).await?;
        sqlx::query("DELETE FROM conversation_groups WHERE id=? AND project_id=?")
            .bind(group_id.to_string())
            .bind(project_id.to_string())
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn workspace_move_conversations(
        &self,
        request: &MoveConversationsRequest,
    ) -> Result<(), StoreError> {
        if request.conversation_ids.len() > 1000 {
            return Err(invalid("at most 1000 conversations may be moved at once"));
        }
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        ensure_project(&mut tx, request.project_id).await?;
        if let Some(group_id) = request.group_id {
            ensure_group(&mut tx, request.project_id, group_id).await?;
        }
        // Complete every ownership check before the first membership mutation.
        for id in &request.conversation_ids {
            ensure_conversation(&mut tx, request.project_id, *id).await?;
        }
        for id in &request.conversation_ids {
            if let Some(group_id) = request.group_id {
                sqlx::query("INSERT INTO conversation_group_members(conversation_id,project_id,group_id) VALUES(?,?,?) ON CONFLICT(conversation_id) DO UPDATE SET group_id=excluded.group_id,project_id=excluded.project_id")
                    .bind(id.to_string()).bind(request.project_id.to_string()).bind(group_id.to_string()).execute(&mut *tx).await?;
            } else {
                sqlx::query("DELETE FROM conversation_group_members WHERE conversation_id=? AND project_id=?")
                    .bind(id.to_string()).bind(request.project_id.to_string()).execute(&mut *tx).await?;
            }
        }
        tx.commit().await?;
        Ok(())
    }

    pub async fn workspace_list_publications(
        &self,
        project_id: Uuid,
    ) -> Result<Vec<PublicationSummary>, StoreError> {
        let mut tx = self.pool.begin().await?;
        ensure_project(&mut tx, project_id).await?;
        let rows = sqlx::query("SELECT p.*,COALESCE((SELECT MAX(revision) FROM publication_revisions WHERE publication_id=p.id),0) AS revision FROM publications p WHERE project_id=? ORDER BY updated_at DESC,id DESC LIMIT 1001")
            .bind(project_id.to_string()).fetch_all(&mut *tx).await?;
        if rows.len() > 1000 {
            return Err(invalid("publication list exceeds 1000 drafts"));
        }
        let result = rows.iter().map(publication_summary_from_row).collect();
        tx.commit().await?;
        result
    }

    pub async fn workspace_get_publication(
        &self,
        project_id: Uuid,
        publication_id: Uuid,
    ) -> Result<PublicationDetail, StoreError> {
        let mut tx = self.pool.begin().await?;
        let detail = publication_detail(&mut tx, project_id, publication_id, None).await?;
        tx.commit().await?;
        Ok(detail)
    }

    /// Replay before source resolution: a source may disappear after commit.
    pub async fn workspace_replay_publication(
        &self,
        request: &SavePublicationRequest,
    ) -> Result<Option<PublicationDetail>, StoreError> {
        let mut tx = self.pool.begin().await?;
        let receipt: Option<PublicationReceipt> =
            replay(&mut tx, request.request_id, "save_publication", request).await?;
        let result = match receipt {
            Some(receipt) => Some(
                publication_detail(
                    &mut tx,
                    receipt.project_id,
                    receipt.publication_id,
                    Some(receipt.revision),
                )
                .await?,
            ),
            None => None,
        };
        tx.commit().await?;
        Ok(result)
    }

    pub async fn workspace_save_publication(
        &self,
        record: &SavePublicationRecord,
    ) -> Result<PublicationDetail, StoreError> {
        let request = &record.request;
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        if let Some(receipt) = replay::<_, PublicationReceipt>(
            &mut tx,
            request.request_id,
            "save_publication",
            request,
        )
        .await?
        {
            return publication_detail(
                &mut tx,
                receipt.project_id,
                receipt.publication_id,
                Some(receipt.revision),
            )
            .await;
        }
        ensure_project(&mut tx, request.project_id).await?;
        let title = normalized_label(&request.title, 200, "publication title")?;
        bounded(&request.markdown, MAX_TEXT_BYTES, "publication text")?;
        let markdown = public_text(&request.markdown);
        if request.sources.len() > MAX_REFERENCES
            || request.sources.len() != record.references.len()
        {
            return Err(invalid(
                "publication references must match sources and contain at most 32 entries",
            ));
        }
        let mut references = Vec::with_capacity(record.references.len());
        for (source, snapshot) in request.sources.iter().zip(&record.references) {
            if source.project_id != request.project_id || &snapshot.source != source {
                return Err(invalid(
                    "publication reference ownership or identity mismatch",
                ));
            }
            let normalized = normalize_snapshot(snapshot)?;
            if let Err(scope_error) = ensure_source_scope(&mut tx, source).await {
                // A missing source may remain cited only by copying the exact
                // snapshot already committed in this draft's expected version.
                let Some(id) = request.publication_id else {
                    return Err(scope_error);
                };
                let previous = publication_detail(
                    &mut tx,
                    request.project_id,
                    id,
                    Some(request.expected_revision),
                )
                .await?;
                let matches_saved = previous.revisions.first().is_some_and(|revision| {
                    revision.references.iter().any(|saved| {
                        let mut saved = saved.clone();
                        saved.availability = normalized.availability;
                        saved == normalized
                    })
                });
                if !matches_saved {
                    return Err(scope_error);
                }
            }
            references.push(normalized);
        }
        let content = PublicationContent {
            schema_version: 1,
            title,
            markdown,
            references,
        };
        let id = request.publication_id.unwrap_or_else(Uuid::new_v4);
        let revision = append_publication(
            &mut tx,
            request.project_id,
            id,
            request.publication_id.is_none(),
            request.expected_revision,
            content,
        )
        .await?;
        let receipt = PublicationReceipt {
            project_id: request.project_id,
            publication_id: id,
            revision,
        };
        remember(
            &mut tx,
            request.request_id,
            "save_publication",
            request,
            &receipt,
        )
        .await?;
        let detail = publication_detail(&mut tx, request.project_id, id, Some(revision)).await?;
        tx.commit().await?;
        Ok(detail)
    }

    pub async fn workspace_restore_publication(
        &self,
        request: &RestorePublicationRequest,
    ) -> Result<PublicationDetail, StoreError> {
        self.workspace_restore_publication_with_redaction(request, public_text)
            .await
    }

    /// The host supplies its complete public-text policy when upgrading legacy
    /// content into a new structured revision.
    pub async fn workspace_restore_publication_with_redaction(
        &self,
        request: &RestorePublicationRequest,
        redact: fn(&str) -> String,
    ) -> Result<PublicationDetail, StoreError> {
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        if let Some(receipt) = replay::<_, PublicationReceipt>(
            &mut tx,
            request.request_id,
            "restore_publication",
            request,
        )
        .await?
        {
            return publication_detail(
                &mut tx,
                receipt.project_id,
                receipt.publication_id,
                Some(receipt.revision),
            )
            .await;
        }
        let detail =
            publication_detail(&mut tx, request.project_id, request.publication_id, None).await?;
        let source = detail
            .revisions
            .iter()
            .find(|revision| revision.revision == request.revision)
            .ok_or_else(|| invalid("publication revision not found"))?;
        let mut references = source.references.clone();
        for reference in &mut references {
            reference.title = redact(&reference.title);
            reference.text = redact(&reference.text);
            reference.status = redact(&reference.status);
            reference.sha256 = hash(&reference.text);
            reference.metadata = reference
                .metadata
                .iter()
                .map(|(key, value)| (redact(key), redact(value)))
                .collect();
        }
        let content = PublicationContent {
            schema_version: 1,
            title: redact(&source.title),
            markdown: redact(&source.markdown),
            references,
        };
        // Restoration copies committed snapshots even when their sources vanished.
        let revision = append_publication(
            &mut tx,
            request.project_id,
            request.publication_id,
            false,
            request.expected_revision,
            content,
        )
        .await?;
        let receipt = PublicationReceipt {
            project_id: request.project_id,
            publication_id: request.publication_id,
            revision,
        };
        remember(
            &mut tx,
            request.request_id,
            "restore_publication",
            request,
            &receipt,
        )
        .await?;
        let restored = publication_detail(
            &mut tx,
            request.project_id,
            request.publication_id,
            Some(revision),
        )
        .await?;
        tx.commit().await?;
        Ok(restored)
    }

    pub async fn workspace_list_library(
        &self,
        request: &ListLibraryRequest,
    ) -> Result<LibraryPage, StoreError> {
        bounded(&request.query, 512, "library query")?;
        if request.limit == 0 || request.limit > 100 || request.offset > 1_000_000 {
            return Err(invalid(
                "library pagination requires limit 1..100 and offset <=1000000",
            ));
        }
        let query = public_text(request.query.trim());
        let kind = request.kind.map(library_kind);
        let project_id = request.project_id.map(|id| id.to_string());
        let mut tx = self.pool.begin().await?;
        // Catalog choices describe the entire saved library, including deleted
        // source projects and items outside the current page or search filters.
        // A renamed source uses its most recently collected saved display name.
        let project_rows = sqlx::query("SELECT l.source_project_id,(SELECT names.source_project_name FROM workspace_library names WHERE names.source_project_id=l.source_project_id ORDER BY names.created_at DESC,names.id DESC LIMIT 1) AS source_project_name FROM workspace_library l GROUP BY l.source_project_id ORDER BY source_project_name COLLATE NOCASE,l.source_project_id LIMIT 1001")
            .fetch_all(&mut *tx).await?;
        if project_rows.len() > 1000 {
            return Err(invalid("library catalog exceeds 1000 source projects"));
        }
        let source_projects = project_rows
            .iter()
            .map(|row| {
                Ok(LibrarySourceProject {
                    id: row_uuid(row, "source_project_id")?,
                    name: public_text(row.try_get("source_project_name")?),
                })
            })
            .collect::<Result<Vec<_>, StoreError>>()?;
        let rows = sqlx::query("SELECT id,kind,title,source_project_id,source_project_name,source_conversation_id,source_conversation_title,text_preview,created_at FROM workspace_library WHERE (?1 IS NULL OR kind=?1) AND (?2 IS NULL OR source_project_id=?2) AND (?3='' OR instr(lower(title),lower(?3))>0 OR instr(lower(snapshot_text),lower(?3))>0) ORDER BY created_at DESC,id DESC LIMIT ?4 OFFSET ?5")
            .bind(kind).bind(project_id).bind(query).bind(i64::from(request.limit)+1).bind(i64::from(request.offset)).fetch_all(&mut *tx).await?;
        let more = rows.len() > request.limit as usize;
        let items = rows
            .iter()
            .take(request.limit as usize)
            .map(library_summary_from_row)
            .collect::<Result<_, _>>()?;
        tx.commit().await?;
        Ok(LibraryPage {
            items,
            next_offset: more.then_some(request.offset + request.limit),
            source_projects,
        })
    }

    pub async fn workspace_get_library_item(
        &self,
        item_id: Uuid,
    ) -> Result<LibraryDetail, StoreError> {
        let mut connection = self.pool.acquire().await?;
        library_detail(&mut connection, item_id).await
    }

    pub async fn workspace_replay_library_item(
        &self,
        request: &SaveLibraryItemRequest,
    ) -> Result<Option<LibraryDetail>, StoreError> {
        let mut tx = self.pool.begin().await?;
        let id: Option<Uuid> = replay(&mut tx, request.request_id, "save_library", request).await?;
        let result = match id {
            Some(id) => Some(library_detail(&mut tx, id).await?),
            None => None,
        };
        tx.commit().await?;
        Ok(result)
    }

    pub async fn workspace_save_library_item(
        &self,
        record: &SaveLibraryRecord,
    ) -> Result<LibraryDetail, StoreError> {
        let request = &record.request;
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        if let Some(id) =
            replay::<_, Uuid>(&mut tx, request.request_id, "save_library", request).await?
        {
            return library_detail(&mut tx, id).await;
        }
        if request.source != record.snapshot.source {
            return Err(invalid("library snapshot identity mismatch"));
        }
        ensure_source_scope(&mut tx, &request.source).await?;
        let title = normalized_label(&request.title, 200, "library title")?;
        let project_name =
            normalized_label(&record.source_project_name, 200, "source project name")?;
        let conversation_title = record
            .source_conversation_title
            .as_ref()
            .map(|title| normalized_label(title, 300, "source conversation title"))
            .transpose()?;
        let snapshot = normalize_snapshot(&record.snapshot)?;
        let snapshot_json = serde_json::to_string(&snapshot)?;
        bounded(&snapshot_json, MAX_TEXT_BYTES, "library snapshot")?;
        let preview: String = snapshot.text.chars().take(240).collect();
        let id = Uuid::new_v4();
        sqlx::query("INSERT INTO workspace_library(id,kind,title,source_project_id,source_project_name,source_conversation_id,source_conversation_title,text_preview,snapshot_text,snapshot_json,created_at) VALUES(?,?,?,?,?,?,?,?,?,?,?)")
            .bind(id.to_string()).bind(library_kind(request.kind)).bind(title)
            .bind(request.source.project_id.to_string()).bind(project_name)
            .bind(request.source.conversation_id.map(|id| id.to_string())).bind(conversation_title)
            .bind(preview).bind(&snapshot.text).bind(snapshot_json).bind(Utc::now().timestamp_millis())
            .execute(&mut *tx).await?;
        remember(&mut tx, request.request_id, "save_library", request, &id).await?;
        let detail = library_detail(&mut tx, id).await?;
        tx.commit().await?;
        Ok(detail)
    }

    pub async fn workspace_delete_library_item(&self, item_id: Uuid) -> Result<(), StoreError> {
        sqlx::query("DELETE FROM workspace_library WHERE id=?")
            .bind(item_id.to_string())
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}

fn invalid(message: &str) -> StoreError {
    StoreError::InvalidInput(message.into())
}
fn public_text(text: &str) -> String {
    redact_secrets(text, &[] as &[&str])
}
fn hash(text: &str) -> String {
    hex::encode(Sha256::digest(text.as_bytes()))
}
fn bounded(text: &str, max: usize, label: &str) -> Result<(), StoreError> {
    if text.len() > max {
        return Err(invalid(&format!("{label} exceeds {max} bytes")));
    }
    Ok(())
}
fn normalized_label(text: &str, max: usize, label: &str) -> Result<String, StoreError> {
    bounded(text, max * 4 + 1024, label)?;
    let text = public_text(text.trim());
    if text.is_empty() || text.chars().count() > max || text.contains('\0') {
        return Err(invalid(&format!(
            "{label} must contain 1..{max} characters"
        )));
    }
    Ok(text)
}
fn timestamp(value: i64) -> Result<String, StoreError> {
    DateTime::<Utc>::from_timestamp_millis(value)
        .map(|value| value.to_rfc3339())
        .ok_or_else(|| invalid("invalid workspace timestamp"))
}
fn row_uuid(row: &SqliteRow, key: &str) -> Result<Uuid, StoreError> {
    Uuid::parse_str(row.try_get::<&str, _>(key)?)
        .map_err(|_| invalid("invalid persisted workspace identifier"))
}
fn group_from_row(row: &SqliteRow) -> Result<ConversationGroup, StoreError> {
    Ok(ConversationGroup {
        id: row_uuid(row, "id")?,
        project_id: row_uuid(row, "project_id")?,
        name: row.try_get("name")?,
        created_at: timestamp(row.try_get("created_at")?)?,
        updated_at: timestamp(row.try_get("updated_at")?)?,
    })
}
async fn ensure_project(
    connection: &mut SqliteConnection,
    project_id: Uuid,
) -> Result<(), StoreError> {
    let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM projects WHERE id=?)")
        .bind(project_id.to_string())
        .fetch_one(connection)
        .await?;
    if !exists {
        return Err(invalid("project not found"));
    }
    Ok(())
}
async fn ensure_group(
    connection: &mut SqliteConnection,
    project_id: Uuid,
    group_id: Uuid,
) -> Result<(), StoreError> {
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM conversation_groups WHERE id=? AND project_id=?)",
    )
    .bind(group_id.to_string())
    .bind(project_id.to_string())
    .fetch_one(connection)
    .await?;
    if !exists {
        return Err(invalid("group not found in project"));
    }
    Ok(())
}
async fn ensure_conversation(
    connection: &mut SqliteConnection,
    project_id: Uuid,
    conversation_id: Uuid,
) -> Result<(), StoreError> {
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM conversation_records WHERE frame_id=? AND project_id=?)",
    )
    .bind(conversation_id.to_string())
    .bind(project_id.to_string())
    .fetch_one(connection)
    .await?;
    if !exists {
        return Err(invalid("conversation not found in project"));
    }
    Ok(())
}
async fn ensure_source_scope(
    connection: &mut SqliteConnection,
    source: &WorkspaceSourceRef,
) -> Result<(), StoreError> {
    bounded(&source.id, 512, "source identifier")?;
    if source.id.trim().is_empty() {
        return Err(invalid("source identifier is empty"));
    }
    for value in [&source.event_hash, &source.content_sha256]
        .into_iter()
        .flatten()
    {
        if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(invalid("invalid source digest"));
        }
    }
    match (source.start, source.end) {
        (None, None) => {}
        (Some(start), Some(end)) if start <= end => {}
        _ => return Err(invalid("invalid source byte range")),
    }
    ensure_project(connection, source.project_id).await?;
    if let Some(id) = source.conversation_id {
        ensure_conversation(connection, source.project_id, id).await?;
    }
    if let Some(id) = source.run_id {
        let row = sqlx::query(
            "SELECT conversation_id FROM agent_runs_v4 WHERE run_id=? AND project_id=?",
        )
        .bind(id.to_string())
        .bind(source.project_id.to_string())
        .fetch_optional(&mut *connection)
        .await?
        .ok_or_else(|| invalid("source run not found in project"))?;
        if let Some(conversation_id) = source.conversation_id {
            if row.try_get::<String, _>("conversation_id")? != conversation_id.to_string() {
                return Err(invalid("source run conversation mismatch"));
            }
        }
    }
    Ok(())
}
fn normalize_snapshot(
    snapshot: &WorkspaceSourceSnapshot,
) -> Result<WorkspaceSourceSnapshot, StoreError> {
    bounded(&snapshot.text, MAX_SNAPSHOT_BYTES, "source snapshot text")?;
    if snapshot.metadata.len() > 64 {
        return Err(invalid("source metadata exceeds 64 fields"));
    }
    let mut result = snapshot.clone();
    result.title = normalized_label(&snapshot.title, 300, "source title")?;
    result.text = public_text(&snapshot.text);
    result.status = normalized_label(&snapshot.status, 100, "source status")?;
    result.sha256 = hash(&result.text);
    result.metadata.clear();
    for (key, value) in &snapshot.metadata {
        bounded(key, 128, "metadata key")?;
        bounded(value, 8192, "metadata value")?;
        result.metadata.insert(public_text(key), public_text(value));
    }
    bounded(
        &serde_json::to_string(&result)?,
        MAX_TEXT_BYTES,
        "source snapshot",
    )?;
    Ok(result)
}
async fn replay<Q: Serialize, R: DeserializeOwned>(
    connection: &mut SqliteConnection,
    id: Uuid,
    operation: &str,
    request: &Q,
) -> Result<Option<R>, StoreError> {
    let row = sqlx::query("SELECT operation,request_sha256,result_json FROM workspace_request_ledger WHERE request_id=?").bind(id.to_string()).fetch_optional(connection).await?;
    let Some(row) = row else {
        return Ok(None);
    };
    if row.try_get::<&str, _>("operation")? != operation
        || row.try_get::<&str, _>("request_sha256")? != hash(&serde_json::to_string(request)?)
    {
        return Err(invalid(
            "request ID conflict: this operation ID was used with different content",
        ));
    }
    Ok(Some(serde_json::from_str(row.try_get("result_json")?)?))
}
async fn remember<Q: Serialize, R: Serialize>(
    connection: &mut SqliteConnection,
    id: Uuid,
    operation: &str,
    request: &Q,
    result: &R,
) -> Result<(), StoreError> {
    sqlx::query("INSERT INTO workspace_request_ledger(request_id,operation,request_sha256,result_json,created_at) VALUES(?,?,?,?,?)")
        .bind(id.to_string()).bind(operation).bind(hash(&serde_json::to_string(request)?)).bind(serde_json::to_string(result)?).bind(Utc::now().timestamp_millis()).execute(connection).await?;
    Ok(())
}
fn publication_summary_from_row(row: &SqliteRow) -> Result<PublicationSummary, StoreError> {
    Ok(PublicationSummary {
        id: row_uuid(row, "id")?,
        project_id: row_uuid(row, "project_id")?,
        title: public_text(row.try_get("title")?),
        revision: row
            .try_get::<i64, _>("revision")?
            .try_into()
            .map_err(|_| invalid("invalid publication revision"))?,
        updated_at: timestamp(row.try_get("updated_at")?)?,
    })
}
async fn publication_detail(
    connection: &mut SqliteConnection,
    project_id: Uuid,
    publication_id: Uuid,
    at_revision: Option<u64>,
) -> Result<PublicationDetail, StoreError> {
    let row = sqlx::query("SELECT p.*,COALESCE((SELECT MAX(revision) FROM publication_revisions WHERE publication_id=p.id),0) AS revision FROM publications p WHERE id=? AND project_id=?")
        .bind(publication_id.to_string()).bind(project_id.to_string()).fetch_optional(&mut *connection).await?.ok_or_else(|| invalid("publication not found in project"))?;
    let mut publication = publication_summary_from_row(&row)?;
    let head = at_revision.unwrap_or(publication.revision);
    let head = i64::try_from(head).map_err(|_| invalid("invalid publication revision"))?;
    let sizes = sqlx::query("SELECT COUNT(*) AS count,COALESCE(SUM(length(CAST(content AS BLOB))),0) AS bytes FROM publication_revisions WHERE publication_id=? AND revision<=?")
        .bind(publication_id.to_string()).bind(head).fetch_one(&mut *connection).await?;
    if sizes.try_get::<i64, _>("count")? > MAX_HISTORY_REVISIONS
        || sizes.try_get::<i64, _>("bytes")? > MAX_HISTORY_BYTES
    {
        return Err(invalid("publication history exceeds safe detail size"));
    }
    let rows = sqlx::query("SELECT r.*,l.title AS legacy_title FROM publication_revisions r LEFT JOIN publication_legacy_titles l ON l.revision_id=r.id WHERE r.publication_id=? AND r.revision<=? ORDER BY r.revision DESC LIMIT 1000")
        .bind(publication_id.to_string()).bind(head).fetch_all(&mut *connection).await?;
    let mut revisions = Vec::with_capacity(rows.len());
    for row in rows {
        let content: String = row.try_get("content")?;
        let structured = serde_json::from_str::<PublicationContent>(&content)
            .ok()
            .filter(|body| body.schema_version == 1);
        let (title, markdown, references, legacy) = match structured {
            Some(body) => (body.title, body.markdown, body.references, false),
            None => (
                row.try_get::<Option<String>, _>("legacy_title")?
                    .unwrap_or_else(|| publication.title.clone()),
                public_text(&content),
                vec![],
                true,
            ),
        };
        revisions.push(PublicationRevision {
            id: row_uuid(&row, "id")?,
            publication_id,
            revision: row
                .try_get::<i64, _>("revision")?
                .try_into()
                .map_err(|_| invalid("invalid publication revision"))?,
            title: public_text(&title),
            markdown,
            references,
            sha256: hash(&content),
            created_at: timestamp(row.try_get("created_at")?)?,
            legacy,
        });
    }
    if at_revision.is_some() {
        let revision = revisions
            .first()
            .filter(|revision| revision.revision == head as u64)
            .ok_or_else(|| invalid("committed publication revision is missing"))?;
        publication.title = revision.title.clone();
        publication.revision = revision.revision;
        publication.updated_at = revision.created_at.clone();
    }
    Ok(PublicationDetail {
        publication,
        revisions,
    })
}
async fn append_publication(
    connection: &mut SqliteConnection,
    project_id: Uuid,
    id: Uuid,
    create: bool,
    expected: u64,
    content: PublicationContent,
) -> Result<u64, StoreError> {
    let now = Utc::now().timestamp_millis();
    if create {
        if expected != 0 {
            return Err(invalid(
                "publication revision conflict: new drafts require revision 0",
            ));
        }
        sqlx::query("INSERT INTO publications(id,project_id,title,status,created_at,updated_at) VALUES(?,?,?,'draft',?,?)")
            .bind(id.to_string()).bind(project_id.to_string()).bind(&content.title).bind(now).bind(now).execute(&mut *connection).await?;
    }
    let row = sqlx::query("SELECT p.title,COALESCE(MAX(r.revision),0) AS head,COALESCE(SUM(length(CAST(r.content AS BLOB))),0) AS bytes FROM publications p LEFT JOIN publication_revisions r ON r.publication_id=p.id WHERE p.id=? AND p.project_id=? GROUP BY p.id")
        .bind(id.to_string()).bind(project_id.to_string()).fetch_optional(&mut *connection).await?.ok_or_else(|| invalid("publication not found in project"))?;
    let head: i64 = row.try_get("head")?;
    if u64::try_from(head).ok() != Some(expected) {
        return Err(invalid(
            "publication revision conflict: reload the latest saved draft",
        ));
    }
    let json = serde_json::to_string(&content)?;
    bounded(&json, MAX_TEXT_BYTES * 2, "publication revision")?;
    if head >= MAX_HISTORY_REVISIONS
        || row.try_get::<i64, _>("bytes")? + json.len() as i64 > MAX_HISTORY_BYTES
    {
        return Err(invalid(
            "publication history limit reached (1000 revisions or 16 MiB)",
        ));
    }
    // Preserve the fallback title of old unstructured rows without changing their bytes.
    sqlx::query("INSERT OR IGNORE INTO publication_legacy_titles(revision_id,title) SELECT id,? FROM publication_revisions WHERE publication_id=?")
        .bind(row.try_get::<String, _>("title")?).bind(id.to_string()).execute(&mut *connection).await?;
    let revision = head + 1;
    sqlx::query("INSERT INTO publication_revisions(id,publication_id,revision,content,content_sha256,created_at) VALUES(?,?,?,?,?,?)")
        .bind(Uuid::new_v4().to_string()).bind(id.to_string()).bind(revision).bind(&json).bind(hash(&json)).bind(now).execute(&mut *connection).await?;
    sqlx::query("UPDATE publications SET title=?,updated_at=? WHERE id=? AND project_id=?")
        .bind(&content.title)
        .bind(now)
        .bind(id.to_string())
        .bind(project_id.to_string())
        .execute(&mut *connection)
        .await?;
    Ok(revision as u64)
}
fn library_kind(kind: LibraryKind) -> &'static str {
    match kind {
        LibraryKind::Code => "code",
        LibraryKind::Excerpt => "excerpt",
        LibraryKind::Artifact => "artifact",
    }
}
fn library_summary_from_row(row: &SqliteRow) -> Result<LibrarySummary, StoreError> {
    let kind = match row.try_get::<&str, _>("kind")? {
        "code" => LibraryKind::Code,
        "excerpt" => LibraryKind::Excerpt,
        "artifact" => LibraryKind::Artifact,
        _ => return Err(invalid("invalid saved library kind")),
    };
    let conversation_id = row
        .try_get::<Option<String>, _>("source_conversation_id")?
        .map(|id| {
            Uuid::parse_str(&id).map_err(|_| invalid("invalid source conversation identifier"))
        })
        .transpose()?;
    Ok(LibrarySummary {
        id: row_uuid(row, "id")?,
        kind,
        title: row.try_get("title")?,
        source_project_id: row_uuid(row, "source_project_id")?,
        source_project_name: row.try_get("source_project_name")?,
        source_conversation_id: conversation_id,
        source_conversation_title: row.try_get("source_conversation_title")?,
        text_preview: row.try_get("text_preview")?,
        created_at: timestamp(row.try_get("created_at")?)?,
    })
}
async fn library_detail(
    connection: &mut SqliteConnection,
    item_id: Uuid,
) -> Result<LibraryDetail, StoreError> {
    let row = sqlx::query("SELECT * FROM workspace_library WHERE id=?")
        .bind(item_id.to_string())
        .fetch_optional(connection)
        .await?
        .ok_or_else(|| invalid("library item not found or removed"))?;
    let json: &str = row.try_get("snapshot_json")?;
    bounded(json, MAX_TEXT_BYTES, "stored library snapshot")?;
    Ok(LibraryDetail {
        item: library_summary_from_row(&row)?,
        snapshot: serde_json::from_str(json)?,
    })
}
