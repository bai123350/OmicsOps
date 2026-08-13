use std::{
    io::Read,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU8, Ordering},
    },
};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use chrono::Utc;
use omicsops_core::{
    project::shell_quote,
    sync::SyncManifest,
    workspace::{SyncDirection, SyncEntry, SyncState, conflict_sibling_path},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Emitter, State};
use uuid::Uuid;

use crate::commands::{AppState, connect_profile, find_profile, require_trusted_host};

#[tauri::command]
pub fn list_sync_entries(
    state: State<'_, AppState>,
    project_id: Uuid,
) -> Result<Vec<SyncEntry>, String> {
    state
        .repository
        .sync_entries_for_project(project_id)
        .map_err(|error| error.to_string())
}

pub fn mark_orphaned_sync_transfers_failed(
    repository: &omicsops_adapters::persistence::Repository,
) -> Result<usize, String> {
    let mut changed = 0;
    for project in repository
        .list_projects()
        .map_err(|error| error.to_string())?
    {
        for mut entry in repository
            .sync_entries_for_project(project.id)
            .map_err(|error| error.to_string())?
        {
            if entry.state == SyncState::Transferring {
                entry.state = SyncState::Failed;
                entry.error = Some("transfer was interrupted by an application restart; retry will resume from verified partial bytes".into());
                entry.updated_at = Utc::now();
                repository
                    .save_sync_entry(&entry)
                    .map_err(|error| error.to_string())?;
                changed += 1;
            }
        }
    }
    Ok(changed)
}

fn set_sync_control(state: &AppState, transfer_id: Uuid, value: u8) -> Result<(), String> {
    let controls = state
        .sync_controls
        .lock()
        .map_err(|_| "sync control lock poisoned")?;
    controls
        .get(&transfer_id)
        .ok_or("sync transfer is not active")?
        .store(value, Ordering::SeqCst);
    Ok(())
}

#[tauri::command]
pub fn pause_sync_transfer(state: State<'_, AppState>, transfer_id: Uuid) -> Result<(), String> {
    set_sync_control(&state, transfer_id, 1)
}

#[tauri::command]
pub fn cancel_sync_transfer(state: State<'_, AppState>, transfer_id: Uuid) -> Result<(), String> {
    set_sync_control(&state, transfer_id, 2)
}

#[tauri::command]
pub async fn retry_sync_transfer(
    app: AppHandle,
    state: State<'_, AppState>,
    transfer_id: Uuid,
) -> Result<SyncEntry, String> {
    let original = state
        .repository
        .list_projects()
        .map_err(|error| error.to_string())?
        .into_iter()
        .find_map(|project| {
            state
                .repository
                .sync_entries_for_project(project.id)
                .ok()?
                .into_iter()
                .find(|entry| entry.id == transfer_id)
        });
    let mut entry = original.ok_or("sync transfer not found")?;
    if !matches!(
        entry.state,
        SyncState::Paused | SyncState::Failed | SyncState::Canceled
    ) {
        return Err("only paused, failed, or canceled transfers can be retried".into());
    }
    entry.retry_count += 1;
    entry.state = SyncState::Pending;
    entry.error = None;
    entry.updated_at = Utc::now();
    state
        .repository
        .save_sync_entry(&entry)
        .map_err(|error| error.to_string())?;
    match entry.direction {
        SyncDirection::RemoteToLocal => resume_download(&app, &state, entry).await,
        SyncDirection::LocalToRemote => resume_upload(&app, &state, entry).await,
    }
}

async fn finish_transfer(
    app: &AppHandle,
    state: &AppState,
    mut entry: SyncEntry,
    outcome: omicsops_adapters::ssh::ControlledTransfer,
) -> Result<SyncEntry, String> {
    match outcome {
        omicsops_adapters::ssh::ControlledTransfer::Completed(bytes) => {
            entry.transferred_bytes = bytes;
            entry.state = SyncState::Synced;
            entry.error = None;
        }
        omicsops_adapters::ssh::ControlledTransfer::Paused(bytes) => {
            entry.transferred_bytes = bytes;
            entry.state = SyncState::Paused;
        }
        omicsops_adapters::ssh::ControlledTransfer::Canceled(bytes) => {
            entry.transferred_bytes = bytes;
            entry.state = SyncState::Canceled;
        }
    }
    entry.updated_at = Utc::now();
    state
        .repository
        .save_sync_entry(&entry)
        .map_err(|error| error.to_string())?;
    let _ = app.emit("artifact-event", &entry);
    state
        .sync_controls
        .lock()
        .map_err(|_| "sync control lock poisoned")?
        .remove(&entry.id);
    Ok(entry)
}

async fn resume_download(
    app: &AppHandle,
    state: &AppState,
    mut entry: SyncEntry,
) -> Result<SyncEntry, String> {
    let project = state
        .repository
        .get_project(entry.project_id)
        .map_err(|error| error.to_string())?
        .ok_or("project not found")?;
    let profile = find_profile(
        &state.repository,
        project
            .connection_id
            .ok_or("project has no remote connection")?,
    )?;
    require_trusted_host(&profile)?;
    let session = connect_profile(state, &profile).await?;
    let remote = entry
        .remote_path
        .clone()
        .ok_or("sync entry has no remote path")?;
    let root = Path::new(&project.local_root)
        .canonicalize()
        .map_err(|error| error.to_string())?;
    let local = root.join(&entry.relative_path);
    ensure_safe_download_parent(&root, &local)?;
    let part = local.with_extension(format!(
        "{}.part",
        local
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or_default()
    ));
    let offset = part
        .metadata()
        .map(|metadata| metadata.len())
        .unwrap_or(0)
        .min(entry.size_bytes);
    entry.transferred_bytes = offset;
    entry.state = SyncState::Transferring;
    state
        .repository
        .save_sync_entry(&entry)
        .map_err(|error| error.to_string())?;
    let control = Arc::new(AtomicU8::new(0));
    state
        .sync_controls
        .lock()
        .map_err(|_| "sync control lock poisoned")?
        .insert(entry.id, control.clone());
    let outcome = session
        .download_controlled_verified(&remote, &local, &entry.sha256, offset, control, |_| {})
        .await
        .map_err(|error| error.to_string());
    match outcome {
        Ok(value) => finish_transfer(app, state, entry, value).await,
        Err(error) => fail_transfer(app, state, entry, error),
    }
}

async fn resume_upload(
    app: &AppHandle,
    state: &AppState,
    mut entry: SyncEntry,
) -> Result<SyncEntry, String> {
    let project = state
        .repository
        .get_project(entry.project_id)
        .map_err(|error| error.to_string())?
        .ok_or("project not found")?;
    let profile = find_profile(
        &state.repository,
        project
            .connection_id
            .ok_or("project has no remote connection")?,
    )?;
    require_trusted_host(&profile)?;
    let session = connect_profile(state, &profile).await?;
    let local = Path::new(&project.local_root).join(
        entry
            .local_relative_path
            .as_deref()
            .unwrap_or(&entry.relative_path),
    );
    let remote = entry
        .remote_path
        .clone()
        .ok_or("sync entry has no remote path")?;
    let offset = session
        .execute(&format!(
            "stat -c %s -- {} 2>/dev/null || printf 0",
            shell_quote(&remote)
        ))
        .await
        .map_err(|error| error.to_string())?
        .stdout
        .trim()
        .parse::<u64>()
        .unwrap_or(0)
        .min(entry.size_bytes);
    entry.transferred_bytes = offset;
    entry.state = SyncState::Transferring;
    state
        .repository
        .save_sync_entry(&entry)
        .map_err(|error| error.to_string())?;
    let control = Arc::new(AtomicU8::new(0));
    state
        .sync_controls
        .lock()
        .map_err(|_| "sync control lock poisoned")?
        .insert(entry.id, control.clone());
    let outcome = session
        .upload_file_controlled(&local, &remote, offset, control, |_| {})
        .await
        .map_err(|error| error.to_string());
    match outcome {
        Ok(value) => finish_transfer(app, state, entry, value).await,
        Err(error) => fail_transfer(app, state, entry, error),
    }
}

fn fail_transfer(
    app: &AppHandle,
    state: &AppState,
    mut entry: SyncEntry,
    error: String,
) -> Result<SyncEntry, String> {
    entry.state = SyncState::Failed;
    entry.error = Some(error.clone());
    entry.updated_at = Utc::now();
    state
        .repository
        .save_sync_entry(&entry)
        .map_err(|e| e.to_string())?;
    let _ = app.emit("artifact-event", &entry);
    state
        .sync_controls
        .lock()
        .map_err(|_| "sync control lock poisoned")?
        .remove(&entry.id);
    Err(error)
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RemoteFileEntry {
    pub relative_path: String,
    pub directory: bool,
    pub size_bytes: u64,
    pub modified_unix_seconds: f64,
}

pub fn parse_remote_index(raw: &str) -> Result<Vec<RemoteFileEntry>, String> {
    let fields: Vec<_> = raw.split('\0').collect();
    let meaningful = fields.strip_suffix(&[""]).unwrap_or(&fields);
    if meaningful.len() % 4 != 0 {
        return Err("remote index contained an incomplete record".into());
    }
    meaningful
        .chunks_exact(4)
        .map(|record| {
            let path = record[0].replace('\\', "/");
            if path.starts_with('/') || path.contains("/../") || path.starts_with("../") {
                return Err(format!("unsafe remote index path: {path}"));
            }
            Ok(RemoteFileEntry {
                relative_path: path,
                directory: record[1] == "d",
                size_bytes: record[2].parse().map_err(|_| "invalid remote file size")?,
                modified_unix_seconds: record[3]
                    .parse()
                    .map_err(|_| "invalid remote modification time")?,
            })
        })
        .collect()
}

pub fn resolve_selected_uploads(
    root: &Path,
    relative_paths: &[String],
) -> Result<Vec<PathBuf>, String> {
    let root = root.canonicalize().map_err(|error| error.to_string())?;
    let manifest =
        SyncManifest::for_uploads("selection", relative_paths.iter().map(String::as_str))
            .map_err(|error| error.to_string())?;
    manifest
        .paths()
        .into_iter()
        .map(|relative| {
            let path = root
                .join(relative)
                .canonicalize()
                .map_err(|error| error.to_string())?;
            if !path.starts_with(&root) || !path.is_file() {
                return Err(format!(
                    "selected upload is not a regular project file: {relative}"
                ));
            }
            Ok(path)
        })
        .collect()
}

pub fn choose_download_relative_path(
    root: &Path,
    relative_path: &str,
    remote_sha256: &str,
) -> Result<PathBuf, String> {
    let root = root.canonicalize().map_err(|error| error.to_string())?;
    let manifest = SyncManifest::for_uploads("download", [relative_path])
        .map_err(|error| error.to_string())?;
    let normalized = manifest
        .paths()
        .into_iter()
        .next()
        .ok_or_else(|| "download path is empty".to_string())?;
    let target = root.join(normalized);
    if target.exists() {
        let resolved = target.canonicalize().map_err(|error| error.to_string())?;
        if !resolved.starts_with(&root) || !resolved.is_file() {
            return Err(format!(
                "download target escapes the project workspace: {normalized}"
            ));
        }
    }
    if !target.exists() || sha256_file(&target)?.eq_ignore_ascii_case(remote_sha256) {
        return Ok(PathBuf::from(normalized));
    }
    for version in 1..=10_000 {
        let candidate = PathBuf::from(conflict_sibling_path(normalized, version));
        if !root.join(&candidate).exists() {
            return Ok(candidate);
        }
    }
    Err("too many download conflict versions".into())
}

pub fn sha256_file(path: &Path) -> Result<String, String> {
    let mut file = std::fs::File::open(path).map_err(|error| error.to_string())?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; 1024 * 1024];
    loop {
        let read = file.read(&mut buffer).map_err(|error| error.to_string())?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hex::encode(hasher.finalize()))
}

#[derive(Debug, Clone, Deserialize)]
pub struct UploadSelectionRequest {
    pub project_id: Uuid,
    pub relative_paths: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DownloadProjectFileRequest {
    pub project_id: Uuid,
    pub relative_path: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct DownloadResult {
    pub entry: SyncEntry,
    pub conflict: bool,
}

const MAX_IMAGE_PREVIEW_BYTES: u64 = 20 * 1024 * 1024;

#[derive(Debug, Clone, Deserialize)]
pub struct PreviewProjectImageRequest {
    pub project_id: Uuid,
    pub relative_path: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProjectImagePreview {
    pub relative_path: String,
    pub mime_type: String,
    pub size_bytes: u64,
    pub sha256: String,
    pub data_url: String,
}

pub fn preview_image_mime(relative_path: &str, bytes: &[u8]) -> Result<&'static str, String> {
    let extension = Path::new(relative_path)
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    match extension.as_str() {
        "png" if bytes.starts_with(b"\x89PNG\r\n\x1a\n") => Ok("image/png"),
        "jpg" | "jpeg" if bytes.starts_with(&[0xff, 0xd8, 0xff]) => Ok("image/jpeg"),
        "gif" if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") => Ok("image/gif"),
        "webp" if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" => {
            Ok("image/webp")
        }
        "bmp" if bytes.starts_with(b"BM") => Ok("image/bmp"),
        _ => Err("selected file is not a supported PNG, JPEG, GIF, WebP, or BMP image".into()),
    }
}

#[tauri::command]
pub async fn preview_project_image(
    state: State<'_, AppState>,
    request: PreviewProjectImageRequest,
) -> Result<ProjectImagePreview, String> {
    let project = state
        .repository
        .get_project(request.project_id)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "project not found".to_string())?;
    let profile_id = project
        .connection_id
        .ok_or_else(|| "project has no remote connection".to_string())?;
    let remote_root = project
        .remote_root
        .ok_or_else(|| "project has no remote root".to_string())?;
    let manifest =
        SyncManifest::for_uploads(project.id.to_string(), [request.relative_path.as_str()])
            .map_err(|error| error.to_string())?;
    let relative = manifest.paths()[0];
    let profile = find_profile(&state.repository, profile_id)?;
    require_trusted_host(&profile)?;
    let session = connect_profile(&state, &profile).await?;
    let root = canonical_remote_root(&session, &remote_root).await?;
    let remote_path = canonical_remote_file(&session, &root, relative).await?;
    let size_bytes = session
        .execute_checked(&format!("stat -c %s -- {}", shell_quote(&remote_path)))
        .await
        .map_err(|error| error.to_string())?
        .stdout
        .trim()
        .parse::<u64>()
        .map_err(|_| "invalid remote image size".to_string())?;
    if size_bytes > MAX_IMAGE_PREVIEW_BYTES {
        return Err(format!(
            "image preview is limited to {} MiB",
            MAX_IMAGE_PREVIEW_BYTES / 1024 / 1024
        ));
    }
    let sha256 = remote_sha256(&session, &remote_path).await?;
    let bytes = session
        .read_file_limited(&remote_path, MAX_IMAGE_PREVIEW_BYTES)
        .await
        .map_err(|error| error.to_string())?;
    session
        .disconnect()
        .await
        .map_err(|error| error.to_string())?;
    let received_sha256 = hex::encode(Sha256::digest(&bytes));
    if !received_sha256.eq_ignore_ascii_case(&sha256) {
        return Err("remote image checksum changed while loading the preview".into());
    }
    let mime_type = preview_image_mime(relative, &bytes)?.to_owned();
    Ok(ProjectImagePreview {
        relative_path: relative.to_owned(),
        mime_type: mime_type.clone(),
        size_bytes,
        sha256,
        data_url: format!("data:{mime_type};base64,{}", STANDARD.encode(bytes)),
    })
}

#[tauri::command]
pub async fn list_remote_files(
    state: State<'_, AppState>,
    project_id: Uuid,
) -> Result<Vec<RemoteFileEntry>, String> {
    let project = state
        .repository
        .get_project(project_id)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "project not found".to_string())?;
    let profile_id = project
        .connection_id
        .ok_or_else(|| "project has no remote connection".to_string())?;
    let remote_root = project
        .remote_root
        .ok_or_else(|| "project has no remote root".to_string())?;
    let profile = find_profile(&state.repository, profile_id)?;
    require_trusted_host(&profile)?;
    let session = connect_profile(&state, &profile).await?;
    let root = canonical_remote_root(&session, &remote_root).await?;
    let output = session.execute_checked(&format!(
        "find {} -mindepth 1 -maxdepth 8 -not -path '*/.omicsops/*' -not -path '*/.omicsops' -printf '%P\\0%y\\0%s\\0%T@\\0'",
        shell_quote(&root)
    )).await.map_err(|error| error.to_string())?;
    session
        .disconnect()
        .await
        .map_err(|error| error.to_string())?;
    parse_remote_index(&output.stdout)
}

#[tauri::command]
pub async fn upload_selected_files(
    app: AppHandle,
    state: State<'_, AppState>,
    request: UploadSelectionRequest,
) -> Result<Vec<SyncEntry>, String> {
    let project = state
        .repository
        .get_project(request.project_id)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "project not found".to_string())?;
    let profile_id = project
        .connection_id
        .ok_or_else(|| "project has no remote connection".to_string())?;
    let remote_root = project
        .remote_root
        .clone()
        .ok_or_else(|| "project has no remote root".to_string())?;
    let selected =
        resolve_selected_uploads(Path::new(&project.local_root), &request.relative_paths)?;
    let manifest = SyncManifest::for_uploads(
        project.id.to_string(),
        request.relative_paths.iter().map(String::as_str),
    )
    .map_err(|error| error.to_string())?;
    let profile = find_profile(&state.repository, profile_id)?;
    require_trusted_host(&profile)?;
    let session = connect_profile(&state, &profile).await?;
    let root = canonical_remote_root(&session, &remote_root).await?;
    let mut entries = Vec::new();
    for (relative, local_path) in manifest.paths().into_iter().zip(selected) {
        let local_sha256 = sha256_file(&local_path)?;
        let (chosen_relative, remote_path, already_synced) =
            choose_remote_upload_destination(&session, &root, relative, &local_sha256).await?;
        let parent = remote_path
            .rsplit_once('/')
            .map(|value| value.0)
            .unwrap_or(&root);
        ensure_remote_upload_parent(&session, &root, parent).await?;
        let mut entry = SyncEntry {
            id: Uuid::new_v4(),
            project_id: project.id,
            relative_path: chosen_relative.clone(),
            local_relative_path: Some(relative.to_owned()),
            remote_path: Some(remote_path.clone()),
            direction: SyncDirection::LocalToRemote,
            size_bytes: local_path
                .metadata()
                .map_err(|error| error.to_string())?
                .len(),
            sha256: local_sha256.clone(),
            state: SyncState::Pending,
            transferred_bytes: 0,
            retry_count: 0,
            error: None,
            updated_at: Utc::now(),
        };
        state
            .repository
            .save_sync_entry(&entry)
            .map_err(|error| error.to_string())?;
        let _ = app.emit("artifact-event", &entry);
        if already_synced {
            entry.transferred_bytes = entry.size_bytes;
            entry.state = if chosen_relative == relative {
                SyncState::Synced
            } else {
                SyncState::Conflict
            };
            state
                .repository
                .save_sync_entry(&entry)
                .map_err(|error| error.to_string())?;
            entries.push(entry);
            continue;
        }
        entry.state = SyncState::Transferring;
        state
            .repository
            .save_sync_entry(&entry)
            .map_err(|error| error.to_string())?;
        let _ = app.emit("artifact-event", &entry);
        let control = Arc::new(AtomicU8::new(0));
        state
            .sync_controls
            .lock()
            .map_err(|_| "sync control lock poisoned")?
            .insert(entry.id, control.clone());
        let progress_repository = state.repository.clone();
        let progress_app = app.clone();
        let progress_entry = entry.clone();
        let outcome = session
            .upload_file_controlled(&local_path, &remote_path, 0, control, move |bytes| {
                let mut update = progress_entry.clone();
                update.transferred_bytes = bytes;
                update.updated_at = Utc::now();
                let _ = progress_repository.save_sync_entry(&update);
                let _ = progress_app.emit("artifact-event", &update);
            })
            .await
            .map_err(|error| error.to_string());
        let completed = match outcome {
            Ok(value) => finish_transfer(&app, &state, entry, value).await?,
            Err(error) => {
                let _ = fail_transfer(&app, &state, entry, error.clone());
                return Err(error);
            }
        };
        if matches!(completed.state, SyncState::Synced) {
            let remote_sha256 = remote_sha256(&session, &remote_path).await?;
            if !local_sha256.eq_ignore_ascii_case(&remote_sha256) {
                return Err(format!("uploaded file checksum mismatch: {relative}"));
            }
        }
        entries.push(completed);
    }
    session
        .disconnect()
        .await
        .map_err(|error| error.to_string())?;
    Ok(entries)
}

async fn choose_remote_upload_destination(
    session: &omicsops_adapters::ssh::SshSession,
    root: &str,
    relative: &str,
    local_sha256: &str,
) -> Result<(String, String, bool), String> {
    let original = format!("{}/{}", root.trim_end_matches('/'), relative);
    if !remote_path_exists(session, &original).await? {
        return Ok((relative.to_owned(), original, false));
    }
    if remote_sha256(session, &original)
        .await?
        .eq_ignore_ascii_case(local_sha256)
    {
        return Ok((relative.to_owned(), original, true));
    }
    for version in 1..=10_000 {
        let chosen = conflict_sibling_path(relative, version);
        let candidate = format!("{}/{}", root.trim_end_matches('/'), chosen);
        if !remote_path_exists(session, &candidate).await? {
            return Ok((chosen, candidate, false));
        }
    }
    Err("too many remote upload conflict versions".into())
}

async fn remote_path_exists(
    session: &omicsops_adapters::ssh::SshSession,
    path: &str,
) -> Result<bool, String> {
    let output = session
        .execute(&format!("test -e {}", shell_quote(path)))
        .await
        .map_err(|error| error.to_string())?;
    match output.status {
        0 => Ok(true),
        1 => Ok(false),
        status => Err(format!(
            "could not inspect remote upload target ({status}): {}",
            output.stderr
        )),
    }
}

async fn ensure_remote_upload_parent(
    session: &omicsops_adapters::ssh::SshSession,
    root: &str,
    parent: &str,
) -> Result<(), String> {
    session
        .execute_checked(&format!(
            "root={} && parent=$(realpath -m -- {}) && case \"$parent\" in \"$root\"|\"$root\"/*) mkdir -p -- \"$parent\";; *) exit 73;; esac",
            shell_quote(root),
            shell_quote(parent)
        ))
        .await
        .map_err(|error| error.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn download_project_file(
    app: AppHandle,
    state: State<'_, AppState>,
    request: DownloadProjectFileRequest,
) -> Result<DownloadResult, String> {
    let project = state
        .repository
        .get_project(request.project_id)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "project not found".to_string())?;
    let profile_id = project
        .connection_id
        .ok_or_else(|| "project has no remote connection".to_string())?;
    let remote_root = project
        .remote_root
        .clone()
        .ok_or_else(|| "project has no remote root".to_string())?;
    let manifest =
        SyncManifest::for_uploads(project.id.to_string(), [request.relative_path.as_str()])
            .map_err(|error| error.to_string())?;
    let relative = manifest.paths()[0];
    let profile = find_profile(&state.repository, profile_id)?;
    require_trusted_host(&profile)?;
    let session = connect_profile(&state, &profile).await?;
    let root = canonical_remote_root(&session, &remote_root).await?;
    let remote_path = canonical_remote_file(&session, &root, relative).await?;
    let sha256 = remote_sha256(&session, &remote_path).await?;
    let size_bytes = session
        .execute_checked(&format!("stat -c %s -- {}", shell_quote(&remote_path)))
        .await
        .map_err(|error| error.to_string())?
        .stdout
        .trim()
        .parse()
        .map_err(|_| "invalid remote file size".to_string())?;
    let chosen = choose_download_relative_path(Path::new(&project.local_root), relative, &sha256)?;
    let conflict = chosen.to_string_lossy().replace('\\', "/") != relative;
    let local_root = Path::new(&project.local_root)
        .canonicalize()
        .map_err(|error| error.to_string())?;
    let local_path = local_root.join(&chosen);
    ensure_safe_download_parent(&local_root, &local_path)?;
    let mut entry = SyncEntry {
        id: Uuid::new_v4(),
        project_id: project.id,
        relative_path: chosen.to_string_lossy().replace('\\', "/"),
        local_relative_path: Some(chosen.to_string_lossy().replace('\\', "/")),
        remote_path: Some(remote_path.clone()),
        direction: SyncDirection::RemoteToLocal,
        size_bytes,
        sha256: sha256.clone(),
        state: SyncState::Transferring,
        transferred_bytes: 0,
        retry_count: 0,
        error: None,
        updated_at: Utc::now(),
    };
    state
        .repository
        .save_sync_entry(&entry)
        .map_err(|error| error.to_string())?;
    let _ = app.emit("artifact-event", &entry);
    let control = Arc::new(AtomicU8::new(0));
    state
        .sync_controls
        .lock()
        .map_err(|_| "sync control lock poisoned")?
        .insert(entry.id, control.clone());
    let progress_repository = state.repository.clone();
    let progress_app = app.clone();
    let progress_entry = entry.clone();
    let outcome = session
        .download_controlled_verified(
            &remote_path,
            &local_path,
            &sha256,
            0,
            control,
            move |bytes| {
                let mut update = progress_entry.clone();
                update.transferred_bytes = bytes;
                update.updated_at = Utc::now();
                let _ = progress_repository.save_sync_entry(&update);
                let _ = progress_app.emit("artifact-event", &update);
            },
        )
        .await
        .map_err(|error| error.to_string());
    entry = match outcome {
        Ok(value) => finish_transfer(&app, &state, entry, value).await?,
        Err(error) => {
            let _ = fail_transfer(&app, &state, entry, error.clone());
            return Err(error);
        }
    };
    if conflict && matches!(entry.state, SyncState::Synced) {
        entry.state = SyncState::Conflict;
        state
            .repository
            .save_sync_entry(&entry)
            .map_err(|error| error.to_string())?;
    }
    let _ = session.disconnect().await;
    Ok(DownloadResult { entry, conflict })
}

fn ensure_safe_download_parent(root: &Path, target: &Path) -> Result<(), String> {
    let parent = target
        .parent()
        .ok_or_else(|| "download target has no parent".to_string())?;
    let mut existing = parent;
    while !existing.exists() {
        existing = existing
            .parent()
            .ok_or_else(|| "download parent did not resolve".to_string())?;
    }
    let resolved_existing = existing.canonicalize().map_err(|error| error.to_string())?;
    if !resolved_existing.starts_with(root) {
        return Err("download parent escapes the project workspace".into());
    }
    std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    let resolved_parent = parent.canonicalize().map_err(|error| error.to_string())?;
    if !resolved_parent.starts_with(root) {
        return Err("download parent escapes the project workspace".into());
    }
    Ok(())
}

async fn canonical_remote_root(
    session: &omicsops_adapters::ssh::SshSession,
    remote_root: &str,
) -> Result<String, String> {
    let output = session
        .execute_checked(&format!(
            "root=$(realpath -- {}) && test -d \"$root\" && printf '%s' \"$root\"",
            shell_quote(remote_root)
        ))
        .await
        .map_err(|error| error.to_string())?;
    let root = output.stdout.trim().to_owned();
    if root.is_empty() {
        Err("remote project root did not resolve".into())
    } else {
        Ok(root)
    }
}

async fn canonical_remote_file(
    session: &omicsops_adapters::ssh::SshSession,
    root: &str,
    relative: &str,
) -> Result<String, String> {
    let candidate = format!("{}/{}", root.trim_end_matches('/'), relative);
    let output = session.execute_checked(&format!("root={} && file=$(realpath -- {}) && case \"$file\" in \"$root\"/*) test -f \"$file\" && printf '%s' \"$file\";; *) exit 73;; esac", shell_quote(root), shell_quote(&candidate))).await.map_err(|error| error.to_string())?;
    let path = output.stdout.trim().to_owned();
    if path.is_empty() {
        Err("remote file did not resolve".into())
    } else {
        Ok(path)
    }
}

async fn remote_sha256(
    session: &omicsops_adapters::ssh::SshSession,
    remote_path: &str,
) -> Result<String, String> {
    session
        .execute_checked(&format!("sha256sum -- {}", shell_quote(remote_path)))
        .await
        .map_err(|error| error.to_string())?
        .stdout
        .split_whitespace()
        .next()
        .map(str::to_owned)
        .ok_or_else(|| "remote checksum was empty".into())
}
