//! Host-owned source resolution for workspace navigation and immutable collections.

use std::{collections::BTreeMap, io::Write};

use omicsops_core::workspace::Message;
use omicsops_dto::*;
use omicsops_protocol::{AgentEventKindV4, AgentEventV4};
use omicsops_store::{
    Store,
    workspace_navigation::{SaveLibraryRecord, SavePublicationRecord},
};
use serde_json::Value;
use sha2::{Digest, Sha256};
use tauri::State;
use tauri_plugin_dialog::DialogExt;
use uuid::Uuid;

use crate::{commands::AppState, composer_references::public_text};

const MAX_SNAPSHOT_BYTES: usize = 256 * 1024;

#[derive(Debug, Clone)]
enum SourceError {
    Missing,
    Changed,
    Invalid(&'static str),
    Unavailable,
}
impl std::fmt::Display for SourceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Missing => f.write_str("Source no longer exists"),
            Self::Changed => f.write_str("Source content or identity has changed"),
            Self::Invalid(reason) => write!(f, "Invalid source: {reason}"),
            Self::Unavailable => f.write_str("Source could not be loaded"),
        }
    }
}

fn digest(text: &str) -> String {
    hex::encode(Sha256::digest(text.as_bytes()))
}

#[cfg(test)]
fn source_ref(project_id: Uuid, kind: SourceKind, id: String) -> WorkspaceSourceRef {
    WorkspaceSourceRef {
        project_id,
        kind,
        id,
        conversation_id: None,
        run_id: None,
        sequence: None,
        event_hash: None,
        content_sha256: None,
        start: None,
        end: None,
    }
}

fn bounded(text: &str, bytes: usize) -> String {
    let mut end = text.len().min(bytes);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_owned()
}

/// Redact with the full message context before selecting bytes. Selecting only
/// the value of `password=...` must not remove the context that identifies it.
fn redacted_selection(raw: &str, start: usize, end: usize) -> String {
    let safe = public_text(raw);
    if safe == raw {
        return raw[start..end].to_owned();
    }
    let prefix: usize = raw
        .chars()
        .zip(safe.chars())
        .take_while(|(a, b)| a == b)
        .map(|(a, _)| a.len_utf8())
        .sum();
    let suffix: usize = raw[prefix..]
        .chars()
        .rev()
        .zip(safe[prefix..].chars().rev())
        .take_while(|(a, b)| a == b)
        .map(|(a, _)| a.len_utf8())
        .sum();
    let raw_changed_end = raw.len() - suffix;
    if end <= prefix || start >= raw_changed_end {
        return raw[start..end].to_owned();
    }
    if start <= prefix && end >= raw_changed_end {
        return safe[start..safe.len() - (raw.len() - end)].to_owned();
    }
    // A partial overlap has no safe one-to-one byte mapping. Keep no fragment of
    // the sensitive span (including when there are several redacted fields).
    "[REDACTED SELECTION]".into()
}

fn snapshot(
    source: &WorkspaceSourceRef,
    title: &str,
    text: &str,
    status: &str,
    metadata: BTreeMap<String, String>,
) -> Result<WorkspaceSourceSnapshot, SourceError> {
    let text = public_text(text);
    if text.len() > MAX_SNAPSHOT_BYTES {
        return Err(SourceError::Invalid("snapshot exceeds 256 KiB"));
    }
    Ok(WorkspaceSourceSnapshot {
        source: source.clone(),
        title: bounded(&public_text(title), 280),
        sha256: digest(&text),
        text,
        status: public_text(status),
        metadata: metadata
            .into_iter()
            .map(|(k, v)| (k, public_text(&v)))
            .collect(),
        availability: SourceAvailability::Available,
    })
}

fn message_snapshot(
    source: &WorkspaceSourceRef,
    message: &Message,
) -> Result<WorkspaceSourceSnapshot, SourceError> {
    if source.kind != SourceKind::Message
        || source.id != message.id.to_string()
        || source.project_id != message.project_id
        || source.conversation_id != Some(message.conversation_id)
        || source.run_id.is_some()
        || source.event_hash.is_some()
        || source.sequence.is_some_and(|seq| seq != message.sequence)
    {
        return Err(SourceError::Invalid("message owner or identity mismatch"));
    }
    if source.content_sha256.as_deref() != Some(digest(&message.markdown).as_str()) {
        return Err(SourceError::Changed);
    }
    let (Some(start), Some(end)) = (source.start, source.end) else {
        return Err(SourceError::Invalid("message byte range required"));
    };
    if start >= end
        || end > message.markdown.len()
        || !message.markdown.is_char_boundary(start)
        || !message.markdown.is_char_boundary(end)
    {
        return Err(SourceError::Invalid(
            "message range must use UTF-8 byte boundaries",
        ));
    }
    snapshot(
        source,
        "Conversation excerpt",
        &redacted_selection(&message.markdown, start, end),
        "recorded",
        BTreeMap::from([
            ("occurred_at".into(), message.created_at.to_rfc3339()),
            (
                "role".into(),
                serde_json::to_value(message.role)
                    .unwrap()
                    .as_str()
                    .unwrap()
                    .into(),
            ),
            ("snapshot_hash_scope".into(), "redacted_text".into()),
        ]),
    )
}

fn tool_snapshot(
    source: &WorkspaceSourceRef,
    event: &AgentEventV4,
) -> Result<WorkspaceSourceSnapshot, SourceError> {
    if source.kind != SourceKind::Tool
        || source.project_id != event.project_id
        || source.run_id != Some(event.run_id)
        || source.conversation_id != Some(event.conversation_id)
        || source.sequence != Some(event.sequence)
        || source.start.is_some()
        || source.end.is_some()
        || source.content_sha256.is_some()
    {
        return Err(SourceError::Invalid("tool owner or identity mismatch"));
    }
    if source.event_hash.as_deref() != Some(event.event_hash.as_str()) {
        return Err(SourceError::Changed);
    }
    event.verify().map_err(|_| SourceError::Changed)?;
    let AgentEventKindV4::ToolRequested { call } = &event.event else {
        return Err(SourceError::Invalid("tool_requested event required"));
    };
    if source.id != call.call_id {
        return Err(SourceError::Invalid("tool call identity mismatch"));
    }
    let text = call
        .arguments
        .get("code")
        .or_else(|| call.arguments.get("command"))
        .and_then(Value::as_str)
        .ok_or(SourceError::Invalid(
            "tool request has no persisted code or command",
        ))?;
    snapshot(
        source,
        &call.tool_id,
        text,
        "requested",
        BTreeMap::from([
            ("tool_id".into(), call.tool_id.clone()),
            ("occurred_at".into(), event.occurred_at.to_rfc3339()),
            (
                "execution_status".into(),
                "request_only; execution is not inferred".into(),
            ),
            ("snapshot_hash_scope".into(), "redacted_text".into()),
        ]),
    )
}

/// Sanitize structured metadata as well as strings: process identities are never UI data.
fn public_value(value: &Value) -> Value {
    match value {
        Value::String(value) => Value::String(public_text(value)),
        Value::Array(values) => Value::Array(values.iter().map(public_value).collect()),
        Value::Object(values) => Value::Object(
            values
                .iter()
                .filter_map(|(key, value)| {
                    let lower = key.to_ascii_lowercase();
                    if lower.contains("process_identity") {
                        return None;
                    }
                    let sensitive = lower == "token"
                        || [
                            "password",
                            "passphrase",
                            "api_key",
                            "apikey",
                            "access_token",
                            "refresh_token",
                            "authorization",
                            "private_key",
                            "secret",
                        ]
                        .iter()
                        .any(|name| lower.contains(name));
                    Some((
                        key.clone(),
                        if sensitive {
                            Value::String("[REDACTED]".into())
                        } else {
                            public_value(value)
                        },
                    ))
                })
                .collect(),
        ),
        value => value.clone(),
    }
}

fn text_field<'a>(record: &'a Value, key: &str) -> &'a str {
    record.get(key).and_then(Value::as_str).unwrap_or("")
}
fn uuid_field(record: &Value, key: &str) -> Option<Uuid> {
    Uuid::parse_str(text_field(record, key)).ok()
}

async fn resolve_source(
    repository: &Store,
    source: &WorkspaceSourceRef,
) -> Result<WorkspaceSourceSnapshot, SourceError> {
    if source.id.is_empty() || source.id.len() > 512 {
        return Err(SourceError::Invalid("source identifier required"));
    }
    if repository
        .get_project(source.project_id)
        .await
        .map_err(|_| SourceError::Unavailable)?
        .is_none()
    {
        return Err(SourceError::Missing);
    }
    if let Some(conversation) = source.conversation_id {
        if repository
            .workspace_source_conversation(source.project_id, conversation)
            .await
            .map_err(|_| SourceError::Unavailable)?
            .is_none()
        {
            return Err(SourceError::Missing);
        }
    }
    if source.kind == SourceKind::Message {
        let id =
            Uuid::parse_str(&source.id).map_err(|_| SourceError::Invalid("message identifier"))?;
        let message = repository
            .workspace_source_message(source.project_id, id)
            .await
            .map_err(|_| SourceError::Unavailable)?
            .ok_or(SourceError::Missing)?;
        return message_snapshot(source, &message);
    }
    if source.kind == SourceKind::Tool {
        let (Some(run), Some(sequence)) = (source.run_id, source.sequence) else {
            return Err(SourceError::Invalid("run and event sequence required"));
        };
        let event = repository
            .workspace_source_event(source.project_id, run, sequence)
            .await
            .map_err(|_| SourceError::Unavailable)?
            .ok_or(SourceError::Missing)?;
        return tool_snapshot(source, &event);
    }
    if source.start.is_some()
        || source.end.is_some()
        || source.content_sha256.is_some()
        || source.sequence.is_some()
        || source.event_hash.is_some()
    {
        return Err(SourceError::Invalid(
            "record source cannot include message or event selectors",
        ));
    }
    let mut record = repository
        .workspace_source_record(source.project_id, source.kind, &source.id)
        .await
        .map_err(|_| SourceError::Unavailable)?
        .ok_or(SourceError::Missing)?;
    if text_field(&record, "project_id") != source.project_id.to_string()
        || text_field(&record, "id") != source.id
    {
        return Err(SourceError::Invalid("record owner mismatch"));
    }
    let mut run_id = if source.kind == SourceKind::Run {
        Uuid::parse_str(&source.id).ok()
    } else {
        uuid_field(&record, "run_id")
    };
    let mut metadata = BTreeMap::from([("snapshot_hash_scope".into(), "redacted_text".into())]);
    let (title, status) = match source.kind {
        SourceKind::Dataset => (
            text_field(&record, "relative_path").to_owned(),
            if record["active"] == true {
                "active"
            } else {
                "superseded"
            }
            .into(),
        ),
        SourceKind::Analysis => (
            text_field(&record, "analysis_type").to_owned(),
            text_field(&record, "status").to_owned(),
        ),
        SourceKind::Artifact => {
            let producer = text_field(&record, "producer_analysis_id");
            if let Some(analysis) = repository
                .workspace_source_record(source.project_id, SourceKind::Analysis, producer)
                .await
                .map_err(|_| SourceError::Unavailable)?
            {
                run_id = uuid_field(&analysis, "run_id");
                // Historical producer identity; deliberately never consult current SSH/project binding.
                metadata.insert(
                    "backend_id".into(),
                    text_field(&analysis["runtime"], "backend_id").into(),
                );
                metadata.insert(
                    "location_status".into(),
                    "historical_backend_only; original_root_not_verified".into(),
                );
                record["producer_runtime"] = public_value(&analysis["runtime"]);
            } else {
                metadata.insert("location_status".into(), "producer_not_recorded".into());
            }
            metadata.insert("content_mode".into(), "reference_only".into());
            metadata.insert("file_availability".into(), "not_checked".into());
            metadata.insert("file_sha256".into(), text_field(&record, "sha256").into());
            metadata.insert(
                "relative_path".into(),
                text_field(&record, "relative_path").into(),
            );
            (
                text_field(&record, "relative_path").to_owned(),
                if record["valid"] == true {
                    "valid"
                } else {
                    "invalid"
                }
                .into(),
            )
        }
        SourceKind::LegacyArtifact => {
            // This ID belongs to the legacy `runs` table, not Agent V4. Even
            // an equal UUID must never associate an unrelated V4 conversation.
            if let Some(legacy_run) = run_id {
                metadata.insert("legacy_run_id".into(), legacy_run.to_string());
            }
            run_id = None;
            metadata.insert("content_mode".into(), "reference_only".into());
            metadata.insert(
                "location_status".into(),
                "legacy_backend_not_recorded".into(),
            );
            metadata.insert("file_availability".into(), "not_checked".into());
            metadata.insert("file_sha256".into(), text_field(&record, "sha256").into());
            metadata.insert(
                "relative_path".into(),
                text_field(&record, "relative_path").into(),
            );
            (
                text_field(&record, "relative_path").to_owned(),
                if record["verified"] == true {
                    "verified"
                } else {
                    "unverified"
                }
                .into(),
            )
        }
        SourceKind::Evidence => {
            // Evidence has only a call ID, which is not unique across runs. Never guess the run.
            run_id = None;
            (
                text_field(&record, "claim").to_owned(),
                if record["valid"] == true {
                    "valid"
                } else {
                    "invalid"
                }
                .into(),
            )
        }
        SourceKind::Provenance => {
            metadata.insert(
                "backend_id".into(),
                text_field(&record, "backend_id").into(),
            );
            (
                "Provenance".into(),
                if record["complete"] == true {
                    "complete"
                } else {
                    "incomplete"
                }
                .into(),
            )
        }
        SourceKind::Run => ("Agent run".into(), text_field(&record, "status").to_owned()),
        _ => return Err(SourceError::Invalid("unsupported source")),
    };
    let conversation = if let Some(run) = run_id {
        repository
            .workspace_run_conversation(source.project_id, run)
            .await
            .map_err(|_| SourceError::Unavailable)?
    } else {
        None
    };
    if source.run_id.is_some() && source.run_id != run_id
        || source.conversation_id.is_some() && source.conversation_id != conversation
    {
        return Err(SourceError::Invalid(
            "record run or conversation association mismatch",
        ));
    }
    if let Some(run) = run_id {
        metadata.insert("run_id".into(), run.to_string());
    }
    if let Some(conversation) = conversation {
        metadata.insert("conversation_id".into(), conversation.to_string());
    }
    let text = serde_json::to_string_pretty(&public_value(&record))
        .map_err(|_| SourceError::Unavailable)?;
    snapshot(source, &title, &text, &status, metadata)
}

async fn refresh_snapshot(repository: &Store, saved: &mut WorkspaceSourceSnapshot) {
    // Preserve the immutable saved content, metadata, status and digest in every branch.
    let current = resolve_source(repository, &saved.source).await;
    saved.availability = source_availability(saved, &current);
}

fn source_availability(
    saved: &WorkspaceSourceSnapshot,
    current: &Result<WorkspaceSourceSnapshot, SourceError>,
) -> SourceAvailability {
    match current {
        Ok(current)
            if current.sha256 == saved.sha256
                && current.status == saved.status
                && current.metadata == saved.metadata =>
        {
            SourceAvailability::Available
        }
        Ok(_) | Err(SourceError::Changed) => SourceAvailability::Changed,
        Err(SourceError::Missing) => SourceAvailability::Missing,
        Err(_) => SourceAvailability::Unavailable,
    }
}

async fn refresh_publication(
    repository: &Store,
    mut detail: PublicationDetail,
) -> PublicationDetail {
    detail.publication.title = public_text(&detail.publication.title);
    // The same reference can recur across hundreds of versions. Resolve it once
    // per detail request, then compare separately against every historical hash.
    let mut current_sources = BTreeMap::new();
    for revision in &mut detail.revisions {
        revision.title = public_text(&revision.title);
        if revision.legacy {
            // Leave historical plain content unchanged on disk; its UI checksum
            // describes the redacted Markdown exposed at this boundary.
            revision.markdown = public_text(&revision.markdown);
            revision.sha256 = digest(&revision.markdown);
        }
        for snapshot in &mut revision.references {
            let key =
                serde_json::to_string(&snapshot.source).expect("source identifiers serialize");
            if !current_sources.contains_key(&key) {
                current_sources.insert(
                    key.clone(),
                    resolve_source(repository, &snapshot.source).await,
                );
            }
            snapshot.availability = source_availability(snapshot, &current_sources[&key]);
        }
    }
    detail
}

#[tauri::command]
pub async fn workspace_list_groups(
    state: State<'_, AppState>,
    project_id: Uuid,
) -> Result<ConversationGroupState, String> {
    state
        .repository
        .workspace_list_groups(project_id)
        .await
        .map_err(|e| e.to_string())
}
#[tauri::command]
pub async fn workspace_save_group(
    state: State<'_, AppState>,
    request: SaveGroupRequest,
) -> Result<ConversationGroup, String> {
    state
        .repository
        .workspace_save_group(&request)
        .await
        .map_err(|e| e.to_string())
}
#[tauri::command]
pub async fn workspace_delete_group(
    state: State<'_, AppState>,
    project_id: Uuid,
    group_id: Uuid,
) -> Result<(), String> {
    state
        .repository
        .workspace_delete_group(project_id, group_id)
        .await
        .map_err(|e| e.to_string())
}
#[tauri::command]
pub async fn workspace_move_conversations(
    state: State<'_, AppState>,
    request: MoveConversationsRequest,
) -> Result<(), String> {
    state
        .repository
        .workspace_move_conversations(&request)
        .await
        .map_err(|e| e.to_string())
}
#[tauri::command]
pub async fn workspace_journey(
    state: State<'_, AppState>,
    request: JourneyRequest,
) -> Result<JourneyPage, String> {
    if state
        .repository
        .get_project(request.project_id)
        .await
        .map_err(|_| "Project could not be loaded")?
        .is_none()
    {
        return Err("Project no longer exists".into());
    }
    let mut page = state
        .repository
        .workspace_journey(&request)
        .await
        .map_err(|_| "Research journey could not be loaded")?;
    for entry in &mut page.entries {
        entry.title = bounded(&public_text(&entry.title), 512);
        entry.summary = bounded(&public_text(&entry.summary), 2048);
        entry.status = public_text(&entry.status);
    }
    Ok(page)
}
#[tauri::command]
pub async fn workspace_source_detail(
    state: State<'_, AppState>,
    source: WorkspaceSourceRef,
) -> Result<WorkspaceSourceSnapshot, String> {
    resolve_source(&state.repository, &source)
        .await
        .map_err(|e| e.to_string())
}
#[tauri::command]
pub async fn workspace_list_publications(
    state: State<'_, AppState>,
    project_id: Uuid,
) -> Result<Vec<PublicationSummary>, String> {
    let mut publications = state
        .repository
        .workspace_list_publications(project_id)
        .await
        .map_err(|e| e.to_string())?;
    for publication in &mut publications {
        publication.title = public_text(&publication.title);
    }
    Ok(publications)
}
#[tauri::command]
pub async fn workspace_get_publication(
    state: State<'_, AppState>,
    project_id: Uuid,
    publication_id: Uuid,
) -> Result<PublicationDetail, String> {
    let detail = state
        .repository
        .workspace_get_publication(project_id, publication_id)
        .await
        .map_err(|e| e.to_string())?;
    Ok(refresh_publication(&state.repository, detail).await)
}

pub async fn save_publication(
    repository: &Store,
    mut request: SavePublicationRequest,
) -> Result<PublicationDetail, String> {
    request.title = public_text(&request.title);
    request.markdown = public_text(&request.markdown);
    if let Some(detail) = repository
        .workspace_replay_publication(&request)
        .await
        .map_err(|e| e.to_string())?
    {
        return Ok(refresh_publication(repository, detail).await);
    }
    if request.sources.len() > 32 {
        return Err("At most 32 publication references may be saved".into());
    }
    let previous = if let Some(id) = request.publication_id {
        repository
            .workspace_get_publication(request.project_id, id)
            .await
            .map_err(|e| e.to_string())?
            .revisions
            .into_iter()
            .find(|revision| revision.revision == request.expected_revision)
    } else {
        None
    };
    let mut references = Vec::new();
    for source in &request.sources {
        if source.project_id != request.project_id {
            return Err("Publication reference belongs to another project".into());
        }
        match resolve_source(repository, source).await {
            Ok(snapshot) => references.push(snapshot),
            Err(error @ (SourceError::Missing | SourceError::Changed)) => {
                // Only the exact reference already saved in this publication's
                // expected revision can use its original immutable snapshot.
                let Some(mut saved) = previous
                    .as_ref()
                    .and_then(|revision| {
                        revision
                            .references
                            .iter()
                            .find(|saved| saved.source == *source)
                    })
                    .cloned()
                else {
                    return Err(error.to_string());
                };
                refresh_snapshot(repository, &mut saved).await;
                references.push(saved);
            }
            Err(error) => return Err(error.to_string()),
        }
    }
    repository
        .workspace_save_publication(&SavePublicationRecord {
            request,
            references,
        })
        .await
        .map_err(|e| e.to_string())
}
#[tauri::command]
pub async fn workspace_save_publication(
    state: State<'_, AppState>,
    request: SavePublicationRequest,
) -> Result<PublicationDetail, String> {
    save_publication(&state.repository, request).await
}
#[tauri::command]
pub async fn workspace_restore_publication(
    state: State<'_, AppState>,
    request: RestorePublicationRequest,
) -> Result<PublicationDetail, String> {
    restore_publication(&state.repository, &request).await
}

async fn restore_publication(
    repository: &Store,
    request: &RestorePublicationRequest,
) -> Result<PublicationDetail, String> {
    let detail = repository
        .workspace_restore_publication_with_redaction(request, public_text)
        .await
        .map_err(|e| e.to_string())?;
    Ok(refresh_publication(repository, detail).await)
}
#[tauri::command]
pub async fn workspace_list_library(
    state: State<'_, AppState>,
    request: ListLibraryRequest,
) -> Result<LibraryPage, String> {
    state
        .repository
        .workspace_list_library(&request)
        .await
        .map_err(|e| e.to_string())
}
pub async fn get_library_item(repository: &Store, item_id: Uuid) -> Result<LibraryDetail, String> {
    let mut detail = repository
        .workspace_get_library_item(item_id)
        .await
        .map_err(|e| e.to_string())?;
    refresh_snapshot(repository, &mut detail.snapshot).await;
    Ok(detail)
}
#[tauri::command]
pub async fn workspace_get_library_item(
    state: State<'_, AppState>,
    item_id: Uuid,
) -> Result<LibraryDetail, String> {
    get_library_item(&state.repository, item_id).await
}

pub async fn save_library_item(
    repository: &Store,
    mut request: SaveLibraryItemRequest,
) -> Result<LibraryDetail, String> {
    request.title = public_text(&request.title);
    if let Some(mut detail) = repository
        .workspace_replay_library_item(&request)
        .await
        .map_err(|e| e.to_string())?
    {
        refresh_snapshot(repository, &mut detail.snapshot).await;
        return Ok(detail);
    }
    if request.kind == LibraryKind::Artifact
        && !matches!(
            request.source.kind,
            SourceKind::Artifact | SourceKind::LegacyArtifact
        )
        || request.kind == LibraryKind::Code
            && !matches!(request.source.kind, SourceKind::Message | SourceKind::Tool)
    {
        return Err("Collection kind does not match its source".into());
    }
    let snapshot = resolve_source(repository, &request.source)
        .await
        .map_err(|e| e.to_string())?;
    let project = repository
        .get_project(request.source.project_id)
        .await
        .map_err(|_| "Source project could not be loaded")?
        .ok_or("Source project no longer exists")?;
    let conversation_title = if let Some(id) = request.source.conversation_id {
        repository
            .workspace_source_conversation(request.source.project_id, id)
            .await
            .map_err(|_| "Source conversation could not be loaded")?
            .map(|c| public_text(&c.title))
    } else {
        None
    };
    repository
        .workspace_save_library_item(&SaveLibraryRecord {
            request,
            source_project_name: public_text(&project.name),
            source_conversation_title: conversation_title,
            snapshot,
        })
        .await
        .map_err(|e| e.to_string())
}
#[tauri::command]
pub async fn workspace_save_library_item(
    state: State<'_, AppState>,
    request: SaveLibraryItemRequest,
) -> Result<LibraryDetail, String> {
    save_library_item(&state.repository, request).await
}
#[tauri::command]
pub async fn workspace_delete_library_item(
    state: State<'_, AppState>,
    item_id: Uuid,
) -> Result<(), String> {
    state
        .repository
        .workspace_delete_library_item(item_id)
        .await
        .map_err(|e| e.to_string())
}

fn publication_markdown(revision: &PublicationRevision) -> String {
    let mut text = format!("# {}\n\n{}\n", revision.title, revision.markdown);
    if !revision.references.is_empty() {
        text.push_str("\n## Source references\n\nSaved summaries are references, not copies of source files or proof of scientific verification.\n");
        for reference in &revision.references {
            text.push_str(&format!("\n- {} ({:?}:{}) — saved status: {}; source availability: {:?}; redacted snapshot SHA-256: {}\n",
                reference.title,reference.source.kind,reference.source.id,reference.status,reference.availability,reference.sha256));
            if let Some(hash) = reference.metadata.get("file_sha256") {
                text.push_str(&format!("  Recorded source file SHA-256: {hash}\n"));
            }
        }
    }
    public_text(&text)
}

fn write_publication_export(
    selected: Option<std::path::PathBuf>,
    text: &str,
) -> Result<Option<String>, String> {
    let Some(mut path) = selected else {
        return Ok(None);
    };
    if path.extension().is_none() {
        path.set_extension("md");
    }
    if !path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("md"))
    {
        return Err("Choose a Markdown (.md) save path".into());
    }
    let parent = path.parent().ok_or("Invalid save directory")?;
    let mut file =
        tempfile::NamedTempFile::new_in(parent).map_err(|_| "Export file could not be created")?;
    file.write_all(public_text(text).as_bytes())
        .map_err(|_| "Export file could not be written")?;
    file.as_file()
        .sync_all()
        .map_err(|_| "Export file could not be synced")?;
    file.persist(&path)
        .map_err(|_| "Export file could not be saved")?;
    Ok(Some(path.to_string_lossy().into_owned()))
}

#[tauri::command]
pub async fn workspace_export_publication(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    project_id: Uuid,
    publication_id: Uuid,
    revision: u64,
) -> Result<Option<String>, String> {
    let detail = state
        .repository
        .workspace_get_publication(project_id, publication_id)
        .await
        .map_err(|e| e.to_string())?;
    let mut saved = detail
        .revisions
        .into_iter()
        .find(|r| r.revision == revision)
        .ok_or("Saved publication revision was not found")?;
    for reference in &mut saved.references {
        refresh_snapshot(&state.repository, reference).await;
    }
    let text = publication_markdown(&saved);
    tauri::async_runtime::spawn_blocking(move || {
        let selected = app
            .dialog()
            .file()
            .add_filter("Markdown", &["md"])
            .set_file_name(format!("omicsops-publication-v{revision}.md"))
            .blocking_save_file();
        let path = selected
            .map(|p| {
                p.into_path()
                    .map_err(|_| "Export path is not a filesystem path")
            })
            .transpose()?;
        write_publication_export(path, &text)
    })
    .await
    .map_err(|_| "Export dialog could not be opened")?
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use omicsops_core::workspace::{Message, MessageRole};
    use omicsops_protocol::{AgentEventKindV4, AgentEventV4, ToolCallV4};
    use serde_json::json;

    fn message() -> Message {
        Message::markdown(
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            1,
            MessageRole::Assistant,
            "前言\nprint('细胞')\npassword=secret-value",
            Utc::now(),
        )
    }

    #[test]
    fn message_selection_requires_exact_digest_and_utf8_boundaries() {
        let message = message();
        let mut source = source_ref(
            message.project_id,
            SourceKind::Message,
            message.id.to_string(),
        );
        source.conversation_id = Some(message.conversation_id);
        source.content_sha256 = Some(digest(&message.markdown));
        source.start = Some("前言\n".len());
        source.end = Some("前言\nprint('细胞')".len());
        assert_eq!(
            message_snapshot(&source, &message).unwrap().text,
            "print('细胞')"
        );
        source.start = Some(1);
        assert!(message_snapshot(&source, &message).is_err());
        source.start = Some(0);
        source.content_sha256 = Some("0".repeat(64));
        assert!(message_snapshot(&source, &message).is_err());
    }

    #[test]
    fn message_snapshot_redacts_before_hashing_and_rejects_other_project() {
        let message = message();
        let mut source = source_ref(
            message.project_id,
            SourceKind::Message,
            message.id.to_string(),
        );
        source.conversation_id = Some(message.conversation_id);
        source.content_sha256 = Some(digest(&message.markdown));
        source.start = Some(0);
        source.end = Some(message.markdown.len());
        let snapshot = message_snapshot(&source, &message).unwrap();
        assert!(!snapshot.text.contains("secret-value"));
        assert_eq!(snapshot.sha256, digest(&snapshot.text));
        source.start = Some(message.markdown.find("secret-value").unwrap());
        let value_only = message_snapshot(&source, &message).unwrap();
        assert!(
            !value_only.text.contains("secret-value"),
            "full-message context must redact a value-only selection"
        );
        source.project_id = Uuid::new_v4();
        assert!(message_snapshot(&source, &message).is_err());
    }

    #[test]
    fn tool_source_binds_run_sequence_hash_and_call_even_when_call_ids_repeat() {
        let project = Uuid::new_v4();
        let conversation = Uuid::new_v4();
        let event = AgentEventV4::first(
            Uuid::new_v4(),
            project,
            conversation,
            Utc::now(),
            AgentEventKindV4::ToolRequested {
                call: ToolCallV4 {
                    call_id: "reused-call".into(),
                    tool_id: "python".into(),
                    arguments: json!({"code":"print(1)"}),
                },
            },
        );
        let mut source = source_ref(project, SourceKind::Tool, "reused-call".into());
        source.conversation_id = Some(conversation);
        source.run_id = Some(event.run_id);
        source.sequence = Some(event.sequence);
        source.event_hash = Some(event.event_hash.clone());
        assert_eq!(tool_snapshot(&source, &event).unwrap().text, "print(1)");
        let second = AgentEventV4::first(
            Uuid::new_v4(),
            project,
            conversation,
            Utc::now(),
            event.event.clone(),
        );
        assert!(tool_snapshot(&source, &second).is_err());
        source.event_hash = Some("f".repeat(64));
        assert!(tool_snapshot(&source, &event).is_err());
    }

    async fn fixture() -> (
        Store,
        omicsops_core::workspace::Project,
        omicsops_core::workspace::Conversation,
    ) {
        use omicsops_core::workspace::{Conversation, Project, ProjectTemplate};
        let store = Store::open_in_memory().await.unwrap();
        let project = Project::new(
            Uuid::new_v4(),
            "Research",
            r"C:\not-an-existing-project",
            ProjectTemplate::Blank,
            Utc::now(),
        );
        store.save_project(&project).await.unwrap();
        let conversation = Conversation::new(
            Uuid::new_v4(),
            project.id,
            "Source conversation",
            Utc::now(),
        );
        store.save_conversation(&conversation).await.unwrap();
        (store, project, conversation)
    }

    async fn science_fixture(
        store: &Store,
        project_id: Uuid,
        conversation_id: Uuid,
    ) -> (Uuid, Uuid, Uuid) {
        use omicsops_science::ScientificStateV4;
        let run_id = Uuid::new_v4();
        store
            .save_agent_run_v4(
                run_id,
                project_id,
                conversation_id,
                "needs_attention",
                &json!({}),
            )
            .await
            .unwrap();
        let now = chrono::DateTime::parse_from_rfc3339("2026-09-21T10:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let event = AgentEventV4::first(
            run_id,
            project_id,
            conversation_id,
            now,
            AgentEventKindV4::RunCreated {
                mode: omicsops_protocol::RunModeV4::Execute,
            },
        );
        store.append_agent_event_v4(&event).await.unwrap();
        let mut science = ScientificStateV4::new(project_id);
        let analysis = Uuid::new_v4();
        let artifact = Uuid::new_v4();
        let evidence = Uuid::new_v4();
        science.analyses.insert(analysis,serde_json::from_value(json!({"schema_version":4,"id":analysis,"project_id":project_id,
            "run_id":run_id,"source_call_id":"same-call","analysis_type":"QC","input_dataset_ids":[],"sample_ids":[],
            "method":"Python","parameters":{"api_key":"secret-value"},"software_requirements":[],"database_versions":{},"random_seed":null,
            "status":"failed","runtime":{"backend_id":"ssh:original-host","language":"python","environment":"system","session_id":null,"process_identity":"sensitive-process-identity"},
            "started_at":now,"ended_at":null,"validation_issues":[]})).unwrap());
        science.artifacts.insert(artifact,serde_json::from_value(json!({"schema_version":4,"id":artifact,"project_id":project_id,
            "artifact_type":"table","relative_path":"results/qc.tsv","producer_analysis_id":analysis,"size_bytes":12,
            "sha256":"raw-file-checksum","preview":null,"metadata":{"password":"secret-value"},"valid":false,"created_at":now})).unwrap());
        science.evidence.insert(evidence,serde_json::from_value(json!({"schema_version":4,"id":evidence,"project_id":project_id,
            "source_call_id":"same-call","claim":"Exploratory observation","sources":[],"strength":"exploratory","conflicts_with":[],
            "valid":false,"invalid_reason":"Analysis failed","recorded_at":now})).unwrap());
        store.save_scientific_state_v4(&science).await.unwrap();
        (analysis, artifact, evidence)
    }

    #[tokio::test]
    async fn records_use_historical_backend_and_never_guess_evidence_run() {
        let (store, mut project, conversation) = fixture().await;
        let (analysis, artifact, evidence) =
            science_fixture(&store, project.id, conversation.id).await;
        let mut source = source_ref(project.id, SourceKind::Artifact, artifact.to_string());
        let saved = resolve_source(&store, &source).await.unwrap();
        assert_eq!(saved.metadata["backend_id"], "ssh:original-host");
        assert_eq!(saved.metadata["file_availability"], "not_checked");
        assert_eq!(saved.status, "invalid");
        assert!(!saved.text.contains("sensitive-process-identity"));
        assert!(!saved.text.contains("secret-value"));
        let replacement = omicsops_core::domain::ConnectionProfile {
            id: Uuid::new_v4(),
            label: "Replacement".into(),
            host: "replacement.invalid".into(),
            port: 22,
            username: "researcher".into(),
            authentication: omicsops_core::domain::AuthenticationMethod::Password,
            authentication_reference: "test-credential-reference".into(),
            host_key_fingerprint: None,
        };
        store.save_connection(&replacement).await.unwrap();
        project.connection_id = Some(replacement.id);
        project.remote_root = Some("/other/root".into());
        store.save_project(&project).await.unwrap();
        let after = resolve_source(&store, &source).await.unwrap();
        assert_eq!(after.text, saved.text);
        assert_eq!(after.metadata["backend_id"], "ssh:original-host");
        source.kind = SourceKind::Evidence;
        source.id = evidence.to_string();
        let saved = resolve_source(&store, &source).await.unwrap();
        assert!(!saved.metadata.contains_key("run_id"));
        assert!(!saved.metadata.contains_key("conversation_id"));
        source.conversation_id = Some(conversation.id);
        assert!(resolve_source(&store, &source).await.is_err());
        source.kind = SourceKind::Analysis;
        source.id = analysis.to_string();
        source.conversation_id = None;
        let saved = resolve_source(&store, &source).await.unwrap();
        assert!(!saved.text.contains("sensitive-process-identity"));
        assert!(!saved.text.contains("secret-value"));
        source.project_id = Uuid::new_v4();
        assert!(resolve_source(&store, &source).await.is_err());
    }

    #[tokio::test]
    async fn journey_paginates_real_v4_and_distinct_legacy_sources() {
        let (store, project, conversation) = fixture().await;
        let (_, artifact, _) = science_fixture(&store, project.id, conversation.id).await;
        let collision = store
            .scientific_state_v4(project.id)
            .await
            .unwrap()
            .unwrap()
            .analyses
            .values()
            .next()
            .unwrap()
            .run_id;
        sqlx::query("INSERT INTO runs(id,project_id,status) VALUES(?,?,'failed')")
            .bind(collision.to_string())
            .bind(project.id.to_string())
            .execute(store.pool())
            .await
            .unwrap();
        let legacy = omicsops_core::workspace::Artifact {
            id: artifact,
            project_id: project.id,
            run_id: Some(collision),
            relative_path: "legacy.tsv".into(),
            remote_path: None,
            media_type: "text/tab-separated-values".into(),
            size_bytes: 4,
            sha256: "file-digest".into(),
            verified: false,
            created_at: Utc::now(),
        };
        store.save_artifact_v3(&legacy).await.unwrap();
        store
            .save_agent_run_v4(
                Uuid::new_v4(),
                project.id,
                conversation.id,
                "running",
                &json!({}),
            )
            .await
            .unwrap();
        let mut request = JourneyRequest {
            project_id: project.id,
            query: String::new(),
            kind: None,
            status: None,
            offset: 0,
            limit: 2,
        };
        let first = store.workspace_journey(&request).await.unwrap();
        assert_eq!(first.entries.len(), 2);
        assert_eq!(first.next_offset, Some(2));
        assert_eq!(first.entries[0].source.kind, SourceKind::LegacyArtifact);
        assert!(
            first.entries[0].source.run_id.is_none(),
            "legacy/orphan run IDs must not become unverifiable V4 run selectors"
        );
        let saved = save_library_item(
            &store,
            SaveLibraryItemRequest {
                request_id: Uuid::new_v4(),
                title: "Legacy reference".into(),
                kind: LibraryKind::Artifact,
                source: first.entries[0].source.clone(),
            },
        )
        .await
        .unwrap();
        assert_eq!(
            saved.snapshot.metadata["legacy_run_id"],
            legacy.run_id.unwrap().to_string()
        );
        assert!(!saved.snapshot.metadata.contains_key("conversation_id"));
        let mut entries = first.entries;
        request.offset = 2;
        request.limit = 100;
        entries.extend(store.workspace_journey(&request).await.unwrap().entries);
        assert_eq!(entries.len(), 5);
        assert!(
            entries
                .iter()
                .any(|entry| entry.source.kind == SourceKind::Artifact
                    && entry.source.id == artifact.to_string())
        );
        let run = entries
            .iter()
            .find(|entry| entry.source.kind == SourceKind::Run)
            .unwrap();
        assert_eq!(run.occurred_at, "2026-09-21T10:00:00Z");
        assert_eq!(run.status, "needs_attention");
        let evidence = entries
            .iter()
            .find(|entry| entry.source.kind == SourceKind::Evidence)
            .unwrap();
        assert!(evidence.source.run_id.is_none() && evidence.source.conversation_id.is_none());
        request.kind = Some(SourceKind::Artifact);
        request.offset = 0;
        assert_eq!(
            store
                .workspace_journey(&request)
                .await
                .unwrap()
                .entries
                .len(),
            1
        );
        request.query = "%".into();
        assert!(
            store
                .workspace_journey(&request)
                .await
                .unwrap()
                .entries
                .is_empty()
        );
    }

    #[tokio::test]
    async fn persisted_tool_lookup_never_crosses_runs_with_repeated_call_ids() {
        let (store, project, conversation) = fixture().await;
        let mut events = Vec::new();
        for code in ["print('first')", "print('second')"] {
            let run = Uuid::new_v4();
            store
                .save_agent_run_v4(run, project.id, conversation.id, "running", &json!({}))
                .await
                .unwrap();
            let event = AgentEventV4::first(
                run,
                project.id,
                conversation.id,
                Utc::now(),
                AgentEventKindV4::ToolRequested {
                    call: ToolCallV4 {
                        call_id: "repeated-call".into(),
                        tool_id: "python".into(),
                        arguments: json!({"code":code}),
                    },
                },
            );
            store.append_agent_event_v4(&event).await.unwrap();
            events.push(event);
        }
        let mut source = source_ref(project.id, SourceKind::Tool, "repeated-call".into());
        source.conversation_id = Some(conversation.id);
        source.run_id = Some(events[1].run_id);
        source.sequence = Some(1);
        source.event_hash = Some(events[1].event_hash.clone());
        assert_eq!(
            resolve_source(&store, &source).await.unwrap().text,
            "print('second')"
        );
        source.run_id = Some(events[0].run_id);
        assert!(matches!(
            resolve_source(&store, &source).await,
            Err(SourceError::Changed)
        ));
    }

    #[tokio::test]
    async fn saved_snapshot_survives_changed_record_then_deleted_source_and_replays() {
        let (store, project, conversation) = fixture().await;
        let (_, artifact, _) = science_fixture(&store, project.id, conversation.id).await;
        let request = SaveLibraryItemRequest {
            request_id: Uuid::new_v4(),
            title: "Saved QC".into(),
            kind: LibraryKind::Artifact,
            source: source_ref(project.id, SourceKind::Artifact, artifact.to_string()),
        };
        let original = save_library_item(&store, request.clone()).await.unwrap();
        let mut science = store
            .scientific_state_v4(project.id)
            .await
            .unwrap()
            .unwrap();
        science.artifacts.get_mut(&artifact).unwrap().sha256 = "changed-file-digest".into();
        store.save_scientific_state_v4(&science).await.unwrap();
        let changed = get_library_item(&store, original.item.id).await.unwrap();
        assert_eq!(changed.snapshot.availability, SourceAvailability::Changed);
        assert_eq!(changed.snapshot.text, original.snapshot.text);
        assert_eq!(changed.snapshot.sha256, original.snapshot.sha256);
        assert_eq!(changed.snapshot.metadata, original.snapshot.metadata);
        store.delete_project(project.id).await.unwrap();
        let missing = get_library_item(&store, original.item.id).await.unwrap();
        assert_eq!(missing.snapshot.availability, SourceAvailability::Missing);
        assert_eq!(missing.snapshot.text, original.snapshot.text);
        assert_eq!(
            save_library_item(&store, request).await.unwrap().item.id,
            original.item.id
        );
    }

    #[tokio::test]
    async fn publication_rejects_cross_project_and_preserves_saved_reference_when_missing() {
        let (store, project, conversation) = fixture().await;
        let (_, artifact, _) = science_fixture(&store, project.id, conversation.id).await;
        let mut request = SavePublicationRequest {
            request_id: Uuid::new_v4(),
            project_id: project.id,
            publication_id: None,
            expected_revision: 0,
            title: "Methods".into(),
            markdown: "password=secret-value\nMethods draft".into(),
            sources: vec![source_ref(
                Uuid::new_v4(),
                SourceKind::Artifact,
                artifact.to_string(),
            )],
        };
        assert!(save_publication(&store, request.clone()).await.is_err());
        request.sources[0].project_id = project.id;
        let detail = save_publication(&store, request.clone()).await.unwrap();
        assert!(!detail.revisions[0].markdown.contains("secret-value"));
        assert_eq!(
            save_publication(&store, request)
                .await
                .unwrap()
                .publication
                .id,
            detail.publication.id
        );
    }

    #[tokio::test]
    async fn editing_saved_publication_keeps_exact_reference_after_conversation_deletion() {
        let (store, project, conversation) = fixture().await;
        let message = Message::markdown(
            Uuid::new_v4(),
            project.id,
            conversation.id,
            1,
            MessageRole::Assistant,
            "Original observation",
            Utc::now(),
        );
        store.save_message(&message).await.unwrap();
        let mut source = source_ref(project.id, SourceKind::Message, message.id.to_string());
        source.conversation_id = Some(conversation.id);
        source.content_sha256 = Some(digest(&message.markdown));
        source.start = Some(0);
        source.end = Some(message.markdown.len());
        let mut request = SavePublicationRequest {
            request_id: Uuid::new_v4(),
            project_id: project.id,
            publication_id: None,
            expected_revision: 0,
            title: "Draft".into(),
            markdown: "First draft".into(),
            sources: vec![source],
        };
        let first = save_publication(&store, request.clone()).await.unwrap();
        store
            .delete_conversation(project.id, conversation.id)
            .await
            .unwrap();
        request.request_id = Uuid::new_v4();
        request.publication_id = Some(first.publication.id);
        request.expected_revision = 1;
        request.markdown = "Revised text".into();
        let revised = save_publication(&store, request.clone()).await.unwrap();
        assert_eq!(
            revised.revisions[0].references[0].text,
            first.revisions[0].references[0].text
        );
        assert_eq!(
            revised.revisions[0].references[0].sha256,
            first.revisions[0].references[0].sha256
        );
        assert_eq!(
            revised.revisions[0].references[0].availability,
            SourceAvailability::Missing
        );
        request.request_id = Uuid::new_v4();
        request.expected_revision = 2;
        request.sources[0].id = Uuid::new_v4().to_string();
        assert!(
            save_publication(&store, request).await.is_err(),
            "deleted-source fallback must never accept a new unverified reference"
        );
    }

    #[test]
    fn export_uses_selected_historical_title_and_supports_cancel_and_write_errors() {
        let revision = PublicationRevision {
            id: Uuid::new_v4(),
            publication_id: Uuid::new_v4(),
            revision: 1,
            title: "Historical title".into(),
            markdown: "Saved text".into(),
            references: vec![],
            sha256: "hash".into(),
            created_at: Utc::now().to_rfc3339(),
            legacy: false,
        };
        let text = publication_markdown(&revision);
        assert!(text.starts_with("# Historical title\n"));
        assert_eq!(write_publication_export(None, &text).unwrap(), None);
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("draft.md");
        write_publication_export(Some(path.clone()), &text).unwrap();
        assert_eq!(std::fs::read_to_string(path).unwrap(), text);
        assert!(write_publication_export(Some(directory.path().join("bad.exe")), &text).is_err());
        assert!(
            write_publication_export(
                Some(directory.path().join("missing").join("draft.md")),
                &text
            )
            .is_err()
        );
    }

    #[tokio::test]
    async fn restoring_legacy_publication_redacts_before_creating_structured_revision() {
        let (store, project, _) = fixture().await;
        let id = Uuid::new_v4();
        let revision = Uuid::new_v4();
        let legacy = "password=legacy-password\n-----BEGIN OPENSSH PRIVATE KEY-----\nprivate-key-value\n-----END OPENSSH PRIVATE KEY-----";
        sqlx::query("INSERT INTO publications(id,project_id,title,status) VALUES(?,?,?,'draft')")
            .bind(id.to_string())
            .bind(project.id.to_string())
            .bind("Legacy")
            .execute(store.pool())
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO publication_revisions(id,publication_id,revision,content) VALUES(?,?,1,?)",
        )
        .bind(revision.to_string())
        .bind(id.to_string())
        .bind(legacy)
        .execute(store.pool())
        .await
        .unwrap();
        let request = RestorePublicationRequest {
            request_id: Uuid::new_v4(),
            project_id: project.id,
            publication_id: id,
            expected_revision: 1,
            revision: 1,
        };
        let restored = restore_publication(&store, &request).await.unwrap();
        assert!(!restored.revisions[0].legacy);
        assert!(!restored.revisions[0].markdown.contains("legacy-password"));
        assert!(!restored.revisions[0].markdown.contains("private-key-value"));
        let stored = store
            .workspace_get_publication(project.id, id)
            .await
            .unwrap();
        assert!(!stored.revisions[0].markdown.contains("legacy-password"));
        assert!(!stored.revisions[0].markdown.contains("private-key-value"));
        let original: String =
            sqlx::query_scalar("SELECT content FROM publication_revisions WHERE id=?")
                .bind(revision.to_string())
                .fetch_one(store.pool())
                .await
                .unwrap();
        assert_eq!(original, legacy);
        assert_eq!(
            restore_publication(&store, &request)
                .await
                .unwrap()
                .publication
                .revision,
            2
        );
    }
}
