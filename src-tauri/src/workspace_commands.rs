use std::path::Path;

use chrono::{DateTime, Utc};
use omicsops_core::workspace::{Conversation, MessageRole, Project, ProjectTemplate};
use serde::{Deserialize, Serialize};
use tauri::State;
use uuid::Uuid;

use crate::commands::AppState;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateProjectRequest {
    pub name: String,
    pub description: String,
    pub local_root: String,
    pub template: String,
    #[serde(default)]
    pub connection_id: Option<Uuid>,
    #[serde(default)]
    pub remote_root: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateConversationRequest {
    pub project_id: Uuid,
    #[serde(default)]
    pub title: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateProjectRemoteRequest {
    pub project_id: Uuid,
    pub connection_id: Option<Uuid>,
    pub remote_root: Option<String>,
}

pub fn project_from_request(
    request: CreateProjectRequest,
    id: Uuid,
    now: DateTime<Utc>,
) -> Result<Project, String> {
    if request.name.trim().is_empty() {
        return Err("project name is required".into());
    }
    if request.local_root.trim().is_empty() {
        return Err("local project directory is required".into());
    }
    let template = match request.template.as_str() {
        "blank" => ProjectTemplate::Blank,
        "single_cell_rna_seq" => ProjectTemplate::SingleCellRnaSeq,
        "bulk_rna_seq" => ProjectTemplate::BulkRnaSeq,
        "literature_review" => ProjectTemplate::LiteratureReview,
        other => return Err(format!("unsupported project template: {other}")),
    };
    let mut project = Project::new(id, request.name.trim(), request.local_root, template, now);
    project.description = request.description.trim().into();
    Ok(project)
}

pub fn write_project_manifest(project: &Project) -> Result<(), String> {
    let root = Path::new(&project.local_root);
    std::fs::create_dir_all(root).map_err(|error| error.to_string())?;
    let metadata_dir = root.join(".omicsops");
    if metadata_dir.exists()
        && std::fs::symlink_metadata(&metadata_dir)
            .map_err(|error| error.to_string())?
            .file_type()
            .is_symlink()
    {
        return Err("project metadata directory cannot be a symbolic link".into());
    }
    std::fs::create_dir_all(&metadata_dir).map_err(|error| error.to_string())?;
    let manifest = serde_json::to_vec_pretty(project).map_err(|error| error.to_string())?;
    std::fs::write(metadata_dir.join("project.json"), manifest).map_err(|error| error.to_string())
}

#[tauri::command]
pub fn list_projects(state: State<'_, AppState>) -> Result<Vec<Project>, String> {
    state
        .repository
        .list_projects()
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn create_project(
    state: State<'_, AppState>,
    request: CreateProjectRequest,
) -> Result<Project, String> {
    let connection_id = request.connection_id;
    let remote_root = request.remote_root.clone();
    let now = Utc::now();
    let mut project = project_from_request(request, Uuid::new_v4(), now)?;
    let connection_exists = if let Some(connection_id) = connection_id {
        state
            .repository
            .list_connections()
            .map_err(|error| error.to_string())?
            .iter()
            .any(|profile| profile.id == connection_id && profile.host_key_fingerprint.is_some())
    } else {
        false
    };
    let project_id = project.id;
    apply_remote_binding(
        &mut project,
        UpdateProjectRemoteRequest {
            project_id,
            connection_id,
            remote_root,
        },
        connection_exists,
        now,
    )?;
    write_project_manifest(&project)?;
    state
        .repository
        .save_project(&project)
        .map_err(|error| error.to_string())?;
    Ok(project)
}

#[tauri::command]
pub fn update_project_remote(
    state: State<'_, AppState>,
    request: UpdateProjectRemoteRequest,
) -> Result<Project, String> {
    let mut project = state
        .repository
        .list_projects()
        .map_err(|error| error.to_string())?
        .into_iter()
        .find(|project| project.id == request.project_id)
        .ok_or_else(|| "project was not found".to_string())?;
    let connection_exists = if let Some(connection_id) = request.connection_id {
        state
            .repository
            .list_connections()
            .map_err(|error| error.to_string())?
            .iter()
            .any(|profile| profile.id == connection_id)
    } else {
        false
    };
    apply_remote_binding(&mut project, request, connection_exists, Utc::now())?;
    write_project_manifest(&project)?;
    state
        .repository
        .save_project(&project)
        .map_err(|error| error.to_string())?;
    Ok(project)
}

pub fn apply_remote_binding(
    project: &mut Project,
    request: UpdateProjectRemoteRequest,
    connection_exists: bool,
    now: DateTime<Utc>,
) -> Result<(), String> {
    match (request.connection_id, request.remote_root) {
        (Some(connection_id), Some(remote_root)) => {
            let remote_root = remote_root.trim();
            if !remote_root.starts_with('/')
                || remote_root.contains('\n')
                || remote_root.contains('\r')
                || remote_root.contains('\0')
            {
                return Err("remote project root must be an absolute Linux path".into());
            }
            if !connection_exists {
                return Err("remote connection was not found".into());
            }
            project.connection_id = Some(connection_id);
            project.remote_root = Some(remote_root.trim_end_matches('/').to_owned());
        }
        (None, None) => {
            project.connection_id = None;
            project.remote_root = None;
        }
        _ => return Err("connection and remote root must be configured together".into()),
    }
    project.updated_at = now;
    Ok(())
}

#[tauri::command]
pub fn list_conversations(
    state: State<'_, AppState>,
    project_id: Uuid,
) -> Result<Vec<Conversation>, String> {
    let mut conversations = state
        .repository
        .conversations_for_project(project_id)
        .map_err(|error| error.to_string())?;
    for conversation in &mut conversations {
        if !conversation_title_needs_first_message(&conversation.title) {
            continue;
        }
        let first_question = state
            .repository
            .messages_for_conversation(conversation.id)
            .map_err(|error| error.to_string())?
            .into_iter()
            .find(|message| message.role == MessageRole::User)
            .map(|message| {
                message
                    .markdown
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ")
            });
        if let Some(title) = first_question.filter(|title| !title.is_empty()) {
            conversation.title = title;
            state
                .repository
                .save_conversation(conversation)
                .map_err(|error| error.to_string())?;
        }
    }
    Ok(conversations)
}

pub fn conversation_title_needs_first_message(title: &str) -> bool {
    matches!(
        title.trim(),
        "" | "QC 与聚类"
            | "QC and clustering"
            | "文献证据"
            | "Literature evidence"
            | "报告生成"
            | "Report drafting"
    )
}

#[tauri::command]
pub fn create_conversation(
    state: State<'_, AppState>,
    request: CreateConversationRequest,
) -> Result<Conversation, String> {
    let title = request.title.as_deref().unwrap_or_default().trim();
    let conversation = Conversation::new(Uuid::new_v4(), request.project_id, title, Utc::now());
    state
        .repository
        .save_conversation(&conversation)
        .map_err(|error| error.to_string())?;
    Ok(conversation)
}

#[tauri::command]
pub fn delete_conversation(
    state: State<'_, AppState>,
    project_id: Uuid,
    conversation_id: Uuid,
) -> Result<(), String> {
    let deleted = state
        .repository
        .delete_conversation(project_id, conversation_id)
        .map_err(|error| error.to_string())?;
    if !deleted {
        return Err("conversation was not found in this project".into());
    }
    Ok(())
}
