use std::{
    io::Read,
    path::{Path, PathBuf},
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
        if !already_synced {
            session
                .upload_file(&local_path, &remote_path)
                .await
                .map_err(|error| error.to_string())?;
        }
        let remote_sha256 = remote_sha256(&session, &remote_path).await?;
        if !local_sha256.eq_ignore_ascii_case(&remote_sha256) {
            return Err(format!("uploaded file checksum mismatch: {relative}"));
        }
        let entry = SyncEntry {
            id: Uuid::new_v4(),
            project_id: project.id,
            relative_path: chosen_relative.clone(),
            remote_path: Some(remote_path),
            direction: SyncDirection::LocalToRemote,
            size_bytes: local_path
                .metadata()
                .map_err(|error| error.to_string())?
                .len(),
            sha256: local_sha256,
            state: if chosen_relative == relative {
                SyncState::Synced
            } else {
                SyncState::Conflict
            },
            updated_at: Utc::now(),
        };
        state
            .repository
            .save_sync_entry(&entry)
            .map_err(|error| error.to_string())?;
        app.emit("artifact-event", &entry)
            .map_err(|error| error.to_string())?;
        entries.push(entry);
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
    session
        .download_atomic_verified(&remote_path, &local_path, &sha256)
        .await
        .map_err(|error| error.to_string())?;
    session
        .disconnect()
        .await
        .map_err(|error| error.to_string())?;
    let entry = SyncEntry {
        id: Uuid::new_v4(),
        project_id: project.id,
        relative_path: chosen.to_string_lossy().replace('\\', "/"),
        remote_path: Some(remote_path),
        direction: SyncDirection::RemoteToLocal,
        size_bytes,
        sha256,
        state: if conflict {
            SyncState::Conflict
        } else {
            SyncState::Synced
        },
        updated_at: Utc::now(),
    };
    state
        .repository
        .save_sync_entry(&entry)
        .map_err(|error| error.to_string())?;
    app.emit("artifact-event", &entry)
        .map_err(|error| error.to_string())?;
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
