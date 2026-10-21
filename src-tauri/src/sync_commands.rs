use std::{
    io::Read,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering},
    },
};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use chrono::Utc;
use omicsops_core::{
    project::shell_quote,
    sync::SyncManifest,
    workspace::{SyncDirection, SyncEntry, SyncState, conflict_sibling_path},
};
use omicsops_store::Store;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Emitter, State};
use tokio::task::JoinHandle;
use uuid::Uuid;

use crate::commands::{AppState, connect_profile, find_profile, require_trusted_host};

#[tauri::command]
pub async fn list_sync_entries(
    state: State<'_, AppState>,
    project_id: Uuid,
) -> Result<Vec<SyncEntry>, String> {
    state
        .repository
        .sync_entries_for_project(project_id)
        .await
        .map_err(|error| error.to_string())
}

pub async fn mark_orphaned_sync_transfers_failed(repository: &Store) -> Result<usize, String> {
    let mut changed = 0;
    for project in repository
        .list_projects()
        .await
        .map_err(|error| error.to_string())?
    {
        for mut entry in repository
            .sync_entries_for_project(project.id)
            .await
            .map_err(|error| error.to_string())?
        {
            if entry.state == SyncState::Transferring {
                entry.state = SyncState::Failed;
                entry.error = Some("transfer was interrupted by an application restart; retry will resume from verified partial bytes".into());
                entry.updated_at = Utc::now();
                repository
                    .save_sync_entry(&entry)
                    .await
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

struct SyncProgressState {
    latest_bytes: AtomicU64,
    closed: AtomicBool,
    notify: tokio::sync::Notify,
}

impl SyncProgressState {
    fn report(&self, bytes: u64) {
        if self.closed.load(Ordering::Acquire) {
            return;
        }
        self.latest_bytes.fetch_max(bytes, Ordering::AcqRel);
        self.notify.notify_one();
    }
}

fn retain_latest_progress(entry: &mut SyncEntry, state: &SyncProgressState) {
    entry.transferred_bytes = entry
        .transferred_bytes
        .max(state.latest_bytes.load(Ordering::Acquire));
}

struct SyncProgressPersistence {
    state: Arc<SyncProgressState>,
    worker: Option<JoinHandle<Result<(), String>>>,
}

impl SyncProgressPersistence {
    // Progress callbacks are synchronous, so they only publish the latest
    // byte count. A single awaited worker owns all Store writes and is
    // drained before the transfer writes any terminal state.
    fn start(repository: Store, entry: &SyncEntry) -> Self {
        let state = Arc::new(SyncProgressState {
            latest_bytes: AtomicU64::new(entry.transferred_bytes),
            closed: AtomicBool::new(false),
            notify: tokio::sync::Notify::new(),
        });
        let worker_state = state.clone();
        let worker_entry = entry.clone();
        let worker = tokio::spawn(async move {
            persist_sync_progress(repository, worker_entry, worker_state).await
        });
        Self {
            state,
            worker: Some(worker),
        }
    }

    async fn finish(mut self) -> Result<u64, String> {
        self.state.closed.store(true, Ordering::Release);
        self.state.notify.notify_one();
        self.worker
            .take()
            .ok_or_else(|| "sync progress worker was already finished".to_owned())?
            .await
            .map_err(|error| format!("sync progress worker failed: {error}"))??;
        Ok(self.state.latest_bytes.load(Ordering::Acquire))
    }
}

impl Drop for SyncProgressPersistence {
    fn drop(&mut self) {
        if let Some(worker) = self.worker.take() {
            worker.abort();
        }
    }
}

async fn persist_sync_progress(
    repository: Store,
    entry: SyncEntry,
    state: Arc<SyncProgressState>,
) -> Result<(), String> {
    let mut persisted_bytes = entry.transferred_bytes;
    loop {
        let latest_bytes = state.latest_bytes.load(Ordering::Acquire);
        if latest_bytes > persisted_bytes {
            let mut update = entry.clone();
            update.transferred_bytes = latest_bytes;
            update.updated_at = Utc::now();
            repository
                .save_sync_entry(&update)
                .await
                .map_err(|error| format!("failed to persist sync progress: {error}"))?;
            persisted_bytes = latest_bytes;
            continue;
        }
        if state.closed.load(Ordering::Acquire) {
            return Ok(());
        }
        state.notify.notified().await;
    }
}

#[tauri::command]
pub async fn pause_sync_transfer(
    state: State<'_, AppState>,
    transfer_id: Uuid,
) -> Result<(), String> {
    set_sync_control(&state, transfer_id, 1)
}

#[tauri::command]
pub async fn cancel_sync_transfer(
    state: State<'_, AppState>,
    transfer_id: Uuid,
) -> Result<(), String> {
    set_sync_control(&state, transfer_id, 2)
}

#[tauri::command]
pub async fn retry_sync_transfer(
    app: AppHandle,
    state: State<'_, AppState>,
    transfer_id: Uuid,
) -> Result<SyncEntry, String> {
    let mut original = None;
    for project in state
        .repository
        .list_projects()
        .await
        .map_err(|error| error.to_string())?
    {
        if let Some(entry) = state
            .repository
            .sync_entries_for_project(project.id)
            .await
            .map_err(|error| error.to_string())?
            .into_iter()
            .find(|entry| entry.id == transfer_id)
        {
            original = Some(entry);
            break;
        }
    }
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
        .await
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
        .await
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
        .await
        .map_err(|error| error.to_string())?
        .ok_or("project not found")?;
    let profile = find_profile(
        &state.repository,
        project
            .connection_id
            .ok_or("project has no remote connection")?,
    )
    .await?;
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
        .await
        .map_err(|error| error.to_string())?;
    let control = Arc::new(AtomicU8::new(0));
    state
        .sync_controls
        .lock()
        .map_err(|_| "sync control lock poisoned")?
        .insert(entry.id, control.clone());
    let progress = SyncProgressPersistence::start(state.repository.clone(), &entry);
    let progress_state = progress.state.clone();
    let callback_state = progress_state.clone();
    let progress_app = app.clone();
    let progress_entry = entry.clone();
    let outcome = session
        .download_controlled_verified(
            &remote,
            &local,
            &entry.sha256,
            offset,
            control,
            move |bytes| {
                let mut update = progress_entry.clone();
                update.transferred_bytes = bytes;
                update.updated_at = Utc::now();
                callback_state.report(bytes);
                let _ = progress_app.emit("artifact-event", &update);
            },
        )
        .await
        .map_err(|error| error.to_string());
    let progress_result = progress.finish().await;
    retain_latest_progress(&mut entry, &progress_state);
    let terminal_result = match outcome {
        Ok(value) => finish_transfer(app, state, entry, value).await,
        Err(error) => fail_transfer(app, state, entry, error).await,
    };
    match (terminal_result, progress_result) {
        (Ok(entry), Ok(_)) => Ok(entry),
        (Ok(_), Err(progress_error)) => Err(progress_error),
        (Err(terminal_error), Ok(_)) => Err(terminal_error),
        (Err(terminal_error), Err(progress_error)) => Err(format!(
            "{terminal_error}; sync progress persistence failed: {progress_error}"
        )),
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
        .await
        .map_err(|error| error.to_string())?
        .ok_or("project not found")?;
    let profile = find_profile(
        &state.repository,
        project
            .connection_id
            .ok_or("project has no remote connection")?,
    )
    .await?;
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
        .await
        .map_err(|error| error.to_string())?;
    let control = Arc::new(AtomicU8::new(0));
    state
        .sync_controls
        .lock()
        .map_err(|_| "sync control lock poisoned")?
        .insert(entry.id, control.clone());
    let progress = SyncProgressPersistence::start(state.repository.clone(), &entry);
    let progress_state = progress.state.clone();
    let callback_state = progress_state.clone();
    let progress_app = app.clone();
    let progress_entry = entry.clone();
    let outcome = session
        .upload_file_controlled(&local, &remote, offset, control, move |bytes| {
            let mut update = progress_entry.clone();
            update.transferred_bytes = bytes;
            update.updated_at = Utc::now();
            callback_state.report(bytes);
            let _ = progress_app.emit("artifact-event", &update);
        })
        .await
        .map_err(|error| error.to_string());
    let progress_result = progress.finish().await;
    retain_latest_progress(&mut entry, &progress_state);
    let terminal_result = match outcome {
        Ok(value) => finish_transfer(app, state, entry, value).await,
        Err(error) => fail_transfer(app, state, entry, error).await,
    };
    match (terminal_result, progress_result) {
        (Ok(entry), Ok(_)) => Ok(entry),
        (Ok(_), Err(progress_error)) => Err(progress_error),
        (Err(terminal_error), Ok(_)) => Err(terminal_error),
        (Err(terminal_error), Err(progress_error)) => Err(format!(
            "{terminal_error}; sync progress persistence failed: {progress_error}"
        )),
    }
}

async fn fail_transfer(
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
        .await
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
                return Err("unsafe remote index path".into());
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
        .await
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
    let profile = find_profile(&state.repository, profile_id).await?;
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
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "project not found".to_string())?;
    let profile_id = project
        .connection_id
        .ok_or_else(|| "project has no remote connection".to_string())?;
    let remote_root = project
        .remote_root
        .ok_or_else(|| "project has no remote root".to_string())?;
    let profile = find_profile(&state.repository, profile_id).await?;
    require_trusted_host(&profile)?;
    let session = connect_profile(&state, &profile).await?;
    list_remote_files_in_session(session, &remote_root).await
}

#[async_trait::async_trait]
trait RemoteFileCommandSession: Send + Sync {
    async fn execute(&self, command: &str)
    -> Result<omicsops_adapters::ssh::CommandOutput, String>;
    async fn disconnect(self) -> Result<(), String>;
}

#[async_trait::async_trait]
impl RemoteFileCommandSession for omicsops_adapters::ssh::SshSession {
    async fn execute(
        &self,
        command: &str,
    ) -> Result<omicsops_adapters::ssh::CommandOutput, String> {
        Self::execute(self, command)
            .await
            .map_err(|error| error.to_string())
    }

    async fn disconnect(self) -> Result<(), String> {
        Self::disconnect(self)
            .await
            .map_err(|error| error.to_string())
    }
}

async fn checked_remote_file_command(
    session: &impl RemoteFileCommandSession,
    command: &str,
) -> Result<omicsops_adapters::ssh::CommandOutput, String> {
    let output = session
        .execute(command)
        .await
        .map_err(|_| "remote command transport failed".to_string())?;
    if output.status != 0 {
        let diagnostic = if output.stderr.trim().is_empty() {
            "no diagnostic output"
        } else {
            "diagnostic withheld"
        };
        return Err(format!(
            "remote command failed with status {}: {}",
            output.status, diagnostic
        ));
    }
    Ok(output)
}

async fn list_remote_files_in_session(
    session: impl RemoteFileCommandSession,
    remote_root: &str,
) -> Result<Vec<RemoteFileEntry>, String> {
    let result = async {
        let root = canonical_remote_root(&session, remote_root).await?;
        let output = checked_remote_file_command(&session, &format!(
            "find {} -mindepth 1 -maxdepth 8 -name '.omicsops' -prune -o -printf '%P\\0%y\\0%s\\0%T@\\0'",
            shell_quote(&root)
        )).await.map_err(|error| format!("remote_listing_failed: {error}"))?;
        parse_remote_index(&output.stdout)
            .map_err(|error| format!("remote_listing_failed: {error}"))
    }
    .await;
    // A resolution, directory, or listing failure remains the primary error.
    // Cleanup is attempted even when no usable index was produced.
    let disconnected = session.disconnect().await;
    match result {
        Err(error) => Err(error),
        Ok(entries) => {
            disconnected.map_err(|_| "remote_listing_failed: disconnect failed".to_string())?;
            Ok(entries)
        }
    }
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
        .await
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
    let profile = find_profile(&state.repository, profile_id).await?;
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
            .await
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
                .await
                .map_err(|error| error.to_string())?;
            entries.push(entry);
            continue;
        }
        entry.state = SyncState::Transferring;
        state
            .repository
            .save_sync_entry(&entry)
            .await
            .map_err(|error| error.to_string())?;
        let _ = app.emit("artifact-event", &entry);
        let control = Arc::new(AtomicU8::new(0));
        state
            .sync_controls
            .lock()
            .map_err(|_| "sync control lock poisoned")?
            .insert(entry.id, control.clone());
        let progress = SyncProgressPersistence::start(state.repository.clone(), &entry);
        let progress_state = progress.state.clone();
        let callback_state = progress_state.clone();
        let progress_app = app.clone();
        let progress_entry = entry.clone();
        let outcome = session
            .upload_file_controlled(&local_path, &remote_path, 0, control, move |bytes| {
                let mut update = progress_entry.clone();
                update.transferred_bytes = bytes;
                update.updated_at = Utc::now();
                callback_state.report(bytes);
                let _ = progress_app.emit("artifact-event", &update);
            })
            .await
            .map_err(|error| error.to_string());
        let progress_result = progress.finish().await;
        // `entry` predates the synchronous callback stream and still carries
        // the initial offset.  Always copy the drained worker's latest value
        // before terminal failure persistence so retry resumes from the
        // verified partial upload rather than byte zero.
        retain_latest_progress(&mut entry, &progress_state);
        let terminal_result = match outcome {
            Ok(value) => finish_transfer(&app, &state, entry, value).await,
            Err(error) => fail_transfer(&app, &state, entry, error).await,
        };
        let completed = match (terminal_result, progress_result) {
            (Ok(completed), Ok(_)) => completed,
            (Ok(_), Err(progress_error)) => return Err(progress_error),
            (Err(terminal_error), Ok(_)) => return Err(terminal_error),
            (Err(terminal_error), Err(progress_error)) => {
                return Err(format!(
                    "{terminal_error}; sync progress persistence failed: {progress_error}"
                ));
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
        .await
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
    let profile = find_profile(&state.repository, profile_id).await?;
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
        .await
        .map_err(|error| error.to_string())?;
    let _ = app.emit("artifact-event", &entry);
    let control = Arc::new(AtomicU8::new(0));
    state
        .sync_controls
        .lock()
        .map_err(|_| "sync control lock poisoned")?
        .insert(entry.id, control.clone());
    let progress = SyncProgressPersistence::start(state.repository.clone(), &entry);
    let progress_state = progress.state.clone();
    let callback_state = progress_state.clone();
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
                callback_state.report(bytes);
                let _ = progress_app.emit("artifact-event", &update);
            },
        )
        .await
        .map_err(|error| error.to_string());
    let progress_result = progress.finish().await;
    // Preserve the latest callback progress on failure; `entry` otherwise
    // still contains the initial zero offset and would make retry restart
    // from the beginning.
    retain_latest_progress(&mut entry, &progress_state);
    let terminal_result = match outcome {
        Ok(value) => finish_transfer(&app, &state, entry, value).await,
        Err(error) => fail_transfer(&app, &state, entry, error).await,
    };
    entry = match (terminal_result, progress_result) {
        (Ok(entry), Ok(_)) => entry,
        (Ok(_), Err(progress_error)) => return Err(progress_error),
        (Err(terminal_error), Ok(_)) => return Err(terminal_error),
        (Err(terminal_error), Err(progress_error)) => {
            return Err(format!(
                "{terminal_error}; sync progress persistence failed: {progress_error}"
            ));
        }
    };
    if conflict && matches!(entry.state, SyncState::Synced) {
        entry.state = SyncState::Conflict;
        state
            .repository
            .save_sync_entry(&entry)
            .await
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
    session: &impl RemoteFileCommandSession,
    remote_root: &str,
) -> Result<String, String> {
    let output = checked_remote_file_command(
        session,
        &format!("realpath -- {}", shell_quote(remote_root)),
    )
    .await
    .map_err(|error| format!("remote_root_resolution_failed: {error}"))?;
    let root = parse_canonical_remote_root(&output.stdout)
        .map_err(|error| format!("remote_root_resolution_failed: {error}"))?;
    checked_remote_file_command(session, &format!("test -d {}", shell_quote(&root)))
        .await
        .map_err(|error| format!("remote_root_unavailable: {error}"))?;
    Ok(root)
}

fn parse_canonical_remote_root(stdout: &str) -> Result<String, String> {
    // realpath adds one line terminator. Spaces are part of the resolved path;
    // startup chatter and paths containing control characters are not trusted.
    let root = stdout.strip_suffix('\n').unwrap_or(stdout);
    if !root.starts_with('/')
        || root.starts_with("//")
        || root.chars().any(char::is_control)
        || root.split('/').any(|part| matches!(part, "." | ".."))
    {
        Err("remote canonical root was not a single absolute normalized path".into())
    } else {
        Ok(root.to_owned())
    }
}

/// Composer file references use the same trusted remote canonicalization as
/// sync and preview operations, but keep the implementation private to this
/// crate so callers cannot construct arbitrary SSH paths.
pub(crate) async fn canonical_remote_root_for_composer(
    session: &omicsops_adapters::ssh::SshSession,
    remote_root: &str,
) -> Result<String, String> {
    canonical_remote_root(session, remote_root).await
}

async fn canonical_remote_file(
    session: &omicsops_adapters::ssh::SshSession,
    root: &str,
    relative: &str,
) -> Result<String, String> {
    let candidate = format!("{}/{}", root.trim_end_matches('/'), relative);
    let output = session
        .execute_checked(&format!(
            "root={} && raw={} && test ! -L \"$raw\" && file=$(realpath -- \"$raw\") && case \"$file\" in \"$root\"/*) test -f \"$file\" && printf '%s' \"$file\";; *) exit 73;; esac",
            shell_quote(root),
            shell_quote(&candidate),
        ))
        .await
        .map_err(|error| error.to_string())?;
    let path = output.stdout.trim().to_owned();
    if path.is_empty() {
        Err("remote file did not resolve".into())
    } else {
        Ok(path)
    }
}

/// Validate one project-relative remote file without reading or downloading
/// its contents.  The caller must have validated the relative path against
/// its project binding before invoking this wrapper.
pub(crate) async fn canonical_remote_file_for_composer(
    session: &omicsops_adapters::ssh::SshSession,
    root: &str,
    relative: &str,
) -> Result<String, String> {
    let mut current = root.trim_end_matches('/').to_owned();
    for component in relative.split('/') {
        current.push('/');
        current.push_str(component);
        session
            .execute_checked(&format!("test ! -L {}", shell_quote(&current)))
            .await
            .map_err(|error| error.to_string())?;
    }
    canonical_remote_file(session, root, relative).await
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

#[cfg(test)]
mod tests {
    use super::*;

    struct MockRemoteFileSession {
        outputs: std::sync::Mutex<
            std::collections::VecDeque<Result<omicsops_adapters::ssh::CommandOutput, String>>,
        >,
        commands: Arc<std::sync::Mutex<Vec<String>>>,
        disconnects: Arc<AtomicU64>,
        disconnect_error: Option<String>,
    }

    #[async_trait::async_trait]
    impl RemoteFileCommandSession for MockRemoteFileSession {
        async fn execute(
            &self,
            command: &str,
        ) -> Result<omicsops_adapters::ssh::CommandOutput, String> {
            self.commands.lock().unwrap().push(command.into());
            self.outputs
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(|| Err("unexpected command after fixture exhausted".into()))
        }

        async fn disconnect(self) -> Result<(), String> {
            self.disconnects.fetch_add(1, Ordering::SeqCst);
            self.disconnect_error.map_or(Ok(()), Err)
        }
    }

    fn remote_command_output(
        status: u32,
        stdout: &str,
        stderr: &str,
    ) -> Result<omicsops_adapters::ssh::CommandOutput, String> {
        Ok(omicsops_adapters::ssh::CommandOutput {
            status,
            stdout: stdout.into(),
            stderr: stderr.into(),
        })
    }

    fn remote_file_session(
        outputs: Vec<Result<omicsops_adapters::ssh::CommandOutput, String>>,
    ) -> MockRemoteFileSession {
        MockRemoteFileSession {
            outputs: std::sync::Mutex::new(outputs.into()),
            commands: Arc::new(std::sync::Mutex::new(vec![])),
            disconnects: Arc::new(AtomicU64::new(0)),
            disconnect_error: None,
        }
    }

    #[test]
    fn remote_listing_canonical_root_preserves_path_spaces() {
        for (output, expected) in [
            ("/", "/"),
            ("/project\n", "/project"),
            ("/project with spaces \n", "/project with spaces "),
            ("/project with spaces ", "/project with spaces "),
        ] {
            assert_eq!(parse_canonical_remote_root(output).unwrap(), expected);
        }
    }

    #[test]
    fn remote_listing_canonical_root_rejects_ambiguous_or_noncanonical_output() {
        for output in [
            "",
            "\n",
            "relative/path\n",
            " /project\n",
            "/project\nunexpected output\n",
            "\n/project\n",
            "/project\n\n",
            "/project\r\n",
            "/project\0suffix\n",
            "/project\tname\n",
            "/project/../outside\n",
            "/project/./child\n",
            "//project\n",
        ] {
            assert!(parse_canonical_remote_root(output).is_err(), "{output:?}");
        }
    }

    #[tokio::test]
    async fn remote_listing_root_commands_are_independent_and_preserve_quoted_paths() {
        let session = remote_file_session(vec![
            remote_command_output(0, "/data/study 'one' $value \n", ""),
            remote_command_output(0, "", ""),
        ]);
        let root = canonical_remote_root(&session, "/configured project")
            .await
            .unwrap();
        assert_eq!(root, "/data/study 'one' $value ");
        assert_eq!(
            *session.commands.lock().unwrap(),
            vec![
                "realpath -- '/configured project'",
                "test -d '/data/study '\"'\"'one'\"'\"' $value '",
            ]
        );
    }

    #[tokio::test]
    async fn remote_listing_resolution_failure_stops_before_directory_check_and_disconnects() {
        for output in [
            remote_command_output(1, "/ignored partial root\n", ""),
            remote_command_output(1, "", "PRIVATE_HOST_DETAILS"),
            Err("transport unavailable: PRIVATE_HOST_DETAILS".into()),
            remote_command_output(0, "PRIVATE_HOST_DETAILS\n/project\n", ""),
        ] {
            let mut session = remote_file_session(vec![output]);
            session.disconnect_error = Some("cleanup failure".into());
            let commands = session.commands.clone();
            let disconnects = session.disconnects.clone();
            let error = list_remote_files_in_session(session, "/configured")
                .await
                .unwrap_err();
            assert!(
                error.starts_with("remote_root_resolution_failed:"),
                "{error}"
            );
            assert!(!error.contains("cleanup failure"), "{error}");
            assert!(!error.contains("PRIVATE_HOST_DETAILS"), "{error}");
            assert!(!error.contains("disconnect"), "{error}");
            assert_eq!(*commands.lock().unwrap(), vec!["realpath -- '/configured'"]);
            assert_eq!(disconnects.load(Ordering::SeqCst), 1);
        }
    }

    #[tokio::test]
    async fn remote_listing_unavailable_directory_has_a_stage_even_without_stderr() {
        for (stderr, diagnostic) in [
            ("", "no diagnostic output"),
            ("PRIVATE_HOST_DETAILS", "diagnostic withheld"),
        ] {
            let mut session = remote_file_session(vec![
                remote_command_output(0, "/resolved\n", ""),
                remote_command_output(1, "", stderr),
            ]);
            session.disconnect_error = Some("cleanup failure".into());
            let commands = session.commands.clone();
            let disconnects = session.disconnects.clone();
            let error = list_remote_files_in_session(session, "/configured")
                .await
                .unwrap_err();
            assert!(error.starts_with("remote_root_unavailable:"), "{error}");
            assert!(error.contains("status 1"), "{error}");
            assert!(error.contains(diagnostic), "{error}");
            assert!(!error.contains("cleanup failure"), "{error}");
            assert!(!error.contains("PRIVATE_HOST_DETAILS"), "{error}");
            assert!(!error.contains("disconnect"), "{error}");
            assert_eq!(
                *commands.lock().unwrap(),
                vec!["realpath -- '/configured'", "test -d '/resolved'"]
            );
            assert_eq!(disconnects.load(Ordering::SeqCst), 1);
        }
    }

    #[tokio::test]
    async fn remote_listing_prunes_internal_directories_and_accepts_only_complete_results() {
        for index in ["", "result.tsv\0f\012\01700000000.5\0"] {
            let session = remote_file_session(vec![
                remote_command_output(0, "/resolved project\n", ""),
                remote_command_output(0, "", ""),
                remote_command_output(0, index, ""),
            ]);
            let commands = session.commands.clone();
            let disconnects = session.disconnects.clone();
            let entries = list_remote_files_in_session(session, "/configured")
                .await
                .unwrap();
            if index.is_empty() {
                assert!(entries.is_empty());
            } else {
                assert_eq!(
                    entries,
                    vec![RemoteFileEntry {
                        relative_path: "result.tsv".into(),
                        directory: false,
                        size_bytes: 12,
                        modified_unix_seconds: 1_700_000_000.5,
                    }]
                );
            }
            assert_eq!(
                *commands.lock().unwrap(),
                vec![
                    "realpath -- '/configured'",
                    "test -d '/resolved project'",
                    "find '/resolved project' -mindepth 1 -maxdepth 8 -name '.omicsops' -prune -o -printf '%P\\0%y\\0%s\\0%T@\\0'",
                ]
            );
            assert_eq!(disconnects.load(Ordering::SeqCst), 1);
        }
    }

    #[tokio::test]
    async fn remote_listing_failure_discards_partial_results_and_preserves_primary_error() {
        for output in [
            remote_command_output(1, "result.tsv\0f\012\01700000000.5\0", ""),
            remote_command_output(
                1,
                "result.tsv\0f\012\01700000000.5\0",
                "PRIVATE_HOST_DETAILS",
            ),
            Err("listing transport failure: PRIVATE_HOST_DETAILS".into()),
            remote_command_output(0, "incomplete record\0f\0", ""),
            remote_command_output(0, "../PRIVATE_HOST_DETAILS\0f\012\01700000000.5\0", ""),
        ] {
            let mut session = remote_file_session(vec![
                remote_command_output(0, "/resolved\n", ""),
                remote_command_output(0, "", ""),
                output,
            ]);
            session.disconnect_error = Some("cleanup failure".into());
            let disconnects = session.disconnects.clone();
            let error = list_remote_files_in_session(session, "/configured")
                .await
                .unwrap_err();
            assert!(error.starts_with("remote_listing_failed:"), "{error}");
            assert!(!error.contains("cleanup failure"), "{error}");
            assert!(!error.contains("result.tsv"), "{error}");
            assert!(!error.contains("PRIVATE_HOST_DETAILS"), "{error}");
            assert!(!error.contains("disconnect"), "{error}");
            assert_eq!(disconnects.load(Ordering::SeqCst), 1);
        }
    }

    #[tokio::test]
    async fn remote_listing_disconnect_failure_is_reported_after_successful_enumeration() {
        let mut session = remote_file_session(vec![
            remote_command_output(0, "/resolved\n", ""),
            remote_command_output(0, "", ""),
            remote_command_output(0, "", ""),
        ]);
        session.disconnect_error = Some("cleanup failure: PRIVATE_HOST_DETAILS".into());
        let commands = session.commands.clone();
        let disconnects = session.disconnects.clone();
        let error = list_remote_files_in_session(session, "/configured")
            .await
            .unwrap_err();
        assert_eq!(error, "remote_listing_failed: disconnect failed");
        assert!(!error.contains("PRIVATE_HOST_DETAILS"), "{error}");
        assert_eq!(commands.lock().unwrap().len(), 3);
        assert_eq!(disconnects.load(Ordering::SeqCst), 1);
    }

    fn test_sync_entry(project_id: Uuid) -> SyncEntry {
        SyncEntry {
            id: Uuid::new_v4(),
            project_id,
            relative_path: "results/example.txt".into(),
            local_relative_path: Some("results/example.txt".into()),
            remote_path: Some("/remote/results/example.txt".into()),
            direction: SyncDirection::RemoteToLocal,
            size_bytes: 100,
            sha256: "sha256".into(),
            state: SyncState::Transferring,
            transferred_bytes: 0,
            retry_count: 0,
            error: None,
            updated_at: Utc::now(),
        }
    }

    #[test]
    fn progress_reports_after_worker_close_are_ignored() {
        let state = SyncProgressState {
            latest_bytes: AtomicU64::new(0),
            closed: AtomicBool::new(false),
            notify: tokio::sync::Notify::new(),
        };

        state.report(64);
        state.closed.store(true, Ordering::Release);
        state.report(96);

        assert_eq!(state.latest_bytes.load(Ordering::Acquire), 64);
    }

    #[tokio::test]
    async fn terminal_state_is_saved_after_progress_worker_drains() {
        let repository = Store::open_in_memory().await.unwrap();
        let project_id = Uuid::new_v4();
        repository
            .save_project(&omicsops_core::workspace::Project::new(
                project_id,
                "sync progress test",
                "C:\\OmicsOps\\sync-progress-test",
                omicsops_core::workspace::ProjectTemplate::Blank,
                Utc::now(),
            ))
            .await
            .unwrap();
        let entry = test_sync_entry(project_id);
        repository.save_sync_entry(&entry).await.unwrap();

        let progress = SyncProgressPersistence::start(repository.clone(), &entry);
        let progress_state = progress.state.clone();
        progress_state.report(64);
        progress.finish().await.unwrap();

        // A callback racing with terminal cleanup must be ignored once the
        // progress worker has drained and closed.
        progress_state.report(96);
        let mut terminal = entry.clone();
        terminal.transferred_bytes = terminal.size_bytes;
        terminal.state = SyncState::Synced;
        repository.save_sync_entry(&terminal).await.unwrap();

        let stored = repository
            .sync_entries_for_project(project_id)
            .await
            .unwrap()
            .into_iter()
            .find(|candidate| candidate.id == entry.id)
            .unwrap();
        assert_eq!(stored.state, SyncState::Synced);
        assert_eq!(stored.transferred_bytes, terminal.size_bytes);
    }

    async fn assert_failed_direction_retains_progress(direction: SyncDirection) {
        let repository = Store::open_in_memory().await.unwrap();
        let project_id = Uuid::new_v4();
        repository
            .save_project(&omicsops_core::workspace::Project::new(
                project_id,
                "sync failure progress",
                "C:\\OmicsOps\\sync-failure-progress",
                omicsops_core::workspace::ProjectTemplate::Blank,
                Utc::now(),
            ))
            .await
            .unwrap();
        let mut entry = test_sync_entry(project_id);
        entry.direction = direction;
        repository.save_sync_entry(&entry).await.unwrap();
        let progress = SyncProgressPersistence::start(repository.clone(), &entry);
        let progress_state = progress.state.clone();
        progress_state.report(64);
        progress.finish().await.unwrap();

        retain_latest_progress(&mut entry, &progress_state);
        entry.state = SyncState::Failed;
        entry.error = Some("deterministic transfer failure".into());
        repository.save_sync_entry(&entry).await.unwrap();
        let stored = repository
            .sync_entries_for_project(project_id)
            .await
            .unwrap()
            .into_iter()
            .find(|candidate| candidate.id == entry.id)
            .unwrap();
        assert_eq!(stored.state, SyncState::Failed);
        assert_eq!(stored.transferred_bytes, 64);
    }

    #[tokio::test]
    async fn upload_failure_retains_latest_persisted_progress() {
        assert_failed_direction_retains_progress(SyncDirection::LocalToRemote).await;
    }

    #[tokio::test]
    async fn download_failure_retains_latest_persisted_progress() {
        assert_failed_direction_retains_progress(SyncDirection::RemoteToLocal).await;
    }
}
