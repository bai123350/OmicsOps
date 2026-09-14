use std::{
    collections::HashSet,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Component, Path, PathBuf},
};

use base64::Engine as _;
use omicsops_core::workspace::Project;
use omicsops_dto::{ComposerAttachmentReceipt, StageComposerAttachmentRequest};
use omicsops_store::Store;
use sha2::{Digest, Sha256};
use tauri::State;
use tauri_plugin_dialog::DialogExt;
use uuid::Uuid;

use crate::commands::AppState;

pub const MAX_ATTACHMENT_BYTES: usize = 20 * 1024 * 1024;
pub const MAX_ATTACHMENT_NAME_BYTES: usize = 255;
pub const MAX_ATTACHMENTS: usize = 8;
pub const MAX_TOTAL_ATTACHMENT_BYTES: u64 = 40 * 1024 * 1024;

const MAX_ATTACHMENT_BASE64_BYTES: usize = ((MAX_ATTACHMENT_BYTES + 2) / 3) * 4 + 4;
const MAX_MANIFEST_BYTES: usize = 64 * 1024;
const MAX_IMAGE_DIMENSION: u32 = 16_384;
const MAX_IMAGE_PIXELS: u64 = 16_000_000;
const STAGING_DIRECTORY: &str = ".omicsops";
const ATTACHMENTS_DIRECTORY: &str = "attachments";
const PAYLOAD_BASENAME: &str = "bytes";
const PAYLOAD_TEMP_BASENAME: &str = ".bytes.tmp";
const BRANCH_COPY_TEMP_PREFIX: &str = ".branch-copy-";
const BRANCH_COPY_MARKER_BASENAME: &str = ".branch-copy-receipt.json";
const BRANCH_COPY_MARKER_TEMP_BASENAME: &str = ".branch-copy-receipt.json.tmp";

/// Host-only attachment material. The model layer receives verified bytes and
/// the stable receipt, never an arbitrary client-selected path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedComposerAttachment {
    pub receipt: ComposerAttachmentReceipt,
    pub bytes: Vec<u8>,
}

/// Copy already-validated local staged bytes into another conversation's
/// project-owned staging namespace. The target ID is supplied by the caller
/// so a branch retry can discover the same receipt instead of creating a
/// second attachment. The complete copy is built in a host-owned temporary
/// directory and renamed into place only after its manifest and bytes are
/// valid, so a process crash cannot leave a partial target that blocks retry.
pub(crate) fn copy_resolved_attachment_to_conversation(
    project: &Project,
    source: &ResolvedComposerAttachment,
    target_conversation_id: Uuid,
    target_id: Uuid,
) -> Result<ComposerAttachmentReceipt, String> {
    if target_id.is_nil() || target_id == source.receipt.id {
        return Err("branch attachment target identity is invalid".into());
    }
    let name = validate_attachment_name(&source.receipt.name)?;
    reject_obvious_credential_name(&name)?;
    reject_obvious_credentials(&source.bytes)?;
    let media_type = detect_attachment_media_type(&name, &source.bytes)?.to_owned();
    if media_type != source.receipt.media_type {
        return Err("source attachment media type changed".into());
    }
    if source.bytes.len() as u64 != source.receipt.size_bytes
        || !hex::encode(Sha256::digest(&source.bytes)).eq_ignore_ascii_case(&source.receipt.sha256)
    {
        return Err("source attachment bytes no longer match its receipt".into());
    }
    let root = canonical_project_root(project)?;
    staging_root(&root)?;
    let expected = branch_copy_receipt(
        project.id,
        target_conversation_id,
        target_id,
        &name,
        &source.bytes,
        &media_type,
    );
    let stage_dir = attachment_directory(&root, target_id);
    match fs::symlink_metadata(&stage_dir) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            Err("branch attachment target is not a regular staging directory".into())
        }
        Ok(_) => {
            if let Ok(receipt) = validate_existing_copied_attachment(&root, &expected) {
                return Ok(receipt);
            }
            if recoverable_branch_copy_directory(&root, &stage_dir, &expected)? {
                remove_branch_copy_directory(&root, &stage_dir)?;
                copy_branch_attachment_into_target(
                    &root,
                    &stage_dir,
                    &expected,
                    project,
                    target_conversation_id,
                    &source.bytes,
                )
            } else {
                Err("branch attachment target does not match its source receipt".into())
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            copy_branch_attachment_into_target(
                &root,
                &stage_dir,
                &expected,
                project,
                target_conversation_id,
                &source.bytes,
            )
        }
        Err(_) => Err("branch attachment target cannot be inspected".into()),
    }
}

fn branch_copy_receipt(
    project_id: Uuid,
    conversation_id: Uuid,
    id: Uuid,
    name: &str,
    bytes: &[u8],
    media_type: &str,
) -> ComposerAttachmentReceipt {
    ComposerAttachmentReceipt {
        id,
        project_id,
        conversation_id,
        name: name.to_owned(),
        relative_path: expected_relative_path(id, name),
        size_bytes: bytes.len() as u64,
        sha256: hex::encode(Sha256::digest(bytes)),
        media_type: media_type.to_owned(),
    }
}

fn copy_branch_attachment_into_target(
    root: &Path,
    target_dir: &Path,
    expected: &ComposerAttachmentReceipt,
    project: &Project,
    target_conversation_id: Uuid,
    bytes: &[u8],
) -> Result<ComposerAttachmentReceipt, String> {
    let temporary_dir = branch_copy_temporary_directory(root, expected.id);
    if fs::symlink_metadata(&temporary_dir).is_ok() {
        if !recoverable_branch_copy_directory(root, &temporary_dir, expected)? {
            return Err("branch attachment temporary copy cannot be recovered".into());
        }
        remove_branch_copy_directory(root, &temporary_dir)?;
    }
    create_branch_copy_directory(root, &temporary_dir)?;
    if let Err(error) = write_branch_copy_marker(&temporary_dir, expected)
        .and_then(|()| {
            stage_attachment_in_directory(
                &temporary_dir,
                expected.id,
                project.id,
                target_conversation_id,
                &expected.name,
                bytes,
                expected.media_type.clone(),
            )
        })
        .and_then(|actual| {
            if actual == *expected {
                validate_copy_directory(root, &temporary_dir, expected)
            } else {
                Err("branch attachment copy produced an unexpected receipt".into())
            }
        })
    {
        let _ = remove_branch_copy_directory(root, &temporary_dir);
        return Err(error);
    }

    ensure_no_symlink_ancestors(target_dir)?;
    match fs::rename(&temporary_dir, target_dir) {
        Ok(()) => validate_existing_copied_attachment(root, expected),
        Err(_) => {
            // A concurrent retry may have won the target directory race.
            // Re-validate the winner; never overwrite it.
            let _ = remove_branch_copy_directory(root, &temporary_dir);
            if let Ok(receipt) = validate_existing_copied_attachment(root, expected) {
                Ok(receipt)
            } else {
                Err("branch attachment target could not be committed".into())
            }
        }
    }
}

fn validate_existing_copied_attachment(
    root: &Path,
    expected: &ComposerAttachmentReceipt,
) -> Result<ComposerAttachmentReceipt, String> {
    validate_copy_directory(root, &attachment_directory(root, expected.id), expected)
}

fn validate_copy_directory(
    root: &Path,
    stage_dir: &Path,
    expected: &ComposerAttachmentReceipt,
) -> Result<ComposerAttachmentReceipt, String> {
    ensure_no_symlink_ancestors(stage_dir)?;
    let metadata = fs::symlink_metadata(stage_dir)
        .map_err(|_| "branch attachment target was not found".to_owned())?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err("branch attachment target is not a regular staging directory".into());
    }
    let manifest = read_manifest(&stage_dir.join("manifest.json"))?;
    if manifest != *expected {
        return Err("branch attachment target does not match its source receipt".into());
    }
    let payload_path = stage_dir.join(generated_payload_name(&expected.name));
    ensure_no_symlink_ancestors(&payload_path)?;
    let payload_metadata = fs::symlink_metadata(&payload_path)
        .map_err(|_| "branch attachment target bytes were not found".to_owned())?;
    if payload_metadata.file_type().is_symlink() || !payload_metadata.is_file() {
        return Err("branch attachment target bytes are not a regular file".into());
    }
    let attachments_root = checked_attachments_root(root)?;
    let canonical_stage = fs::canonicalize(stage_dir)
        .map_err(|_| "branch attachment staging directory cannot be canonicalized")?;
    let canonical_payload = fs::canonicalize(&payload_path)
        .map_err(|_| "branch attachment bytes cannot be canonicalized")?;
    if canonical_stage.parent() != Some(attachments_root.as_path())
        || canonical_payload.parent() != Some(canonical_stage.as_path())
        || canonical_payload.file_name().and_then(|name| name.to_str())
            != Some(generated_payload_name(&expected.name).as_str())
    {
        return Err("branch attachment target is outside the expected staging root".into());
    }
    let bytes = read_staged_bytes(&canonical_payload)?;
    if bytes.len() as u64 != expected.size_bytes
        || !hex::encode(Sha256::digest(&bytes)).eq_ignore_ascii_case(&expected.sha256)
    {
        return Err("branch attachment target bytes do not match its receipt".into());
    }
    Ok(manifest)
}

fn branch_copy_temporary_directory(root: &Path, id: Uuid) -> PathBuf {
    root.join(STAGING_DIRECTORY)
        .join(ATTACHMENTS_DIRECTORY)
        .join(format!("{BRANCH_COPY_TEMP_PREFIX}{id}"))
}

fn create_branch_copy_directory(root: &Path, path: &Path) -> Result<(), String> {
    let attachments_root = checked_attachments_root(root)?;
    ensure_no_symlink_ancestors(path)?;
    if path.parent() != Some(attachments_root.as_path()) {
        return Err("branch attachment temporary path is outside the staging root".into());
    }
    match fs::symlink_metadata(path) {
        Ok(_) => Err("branch attachment temporary copy already exists".into()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            fs::create_dir(path).map_err(|_| "unable to create branch attachment copy")?;
            set_private_permissions(path, true)
        }
        Err(_) => Err("unable to inspect branch attachment temporary copy".into()),
    }
}

fn write_branch_copy_marker(
    stage_dir: &Path,
    expected: &ComposerAttachmentReceipt,
) -> Result<(), String> {
    let marker = serde_json::to_vec(expected)
        .map_err(|_| "unable to encode branch attachment copy marker".to_owned())?;
    write_atomic_private_file(
        &stage_dir.join(BRANCH_COPY_MARKER_TEMP_BASENAME),
        &stage_dir.join(BRANCH_COPY_MARKER_BASENAME),
        &marker,
    )
}

fn recoverable_branch_copy_directory(
    root: &Path,
    stage_dir: &Path,
    expected: &ComposerAttachmentReceipt,
) -> Result<bool, String> {
    let attachments_root = checked_attachments_root(root)?;
    ensure_no_symlink_ancestors(stage_dir)?;
    if stage_dir.parent() != Some(attachments_root.as_path()) {
        return Ok(false);
    }
    let Ok(metadata) = fs::symlink_metadata(stage_dir) else {
        return Ok(false);
    };
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Ok(false);
    }
    let expected_temporary_name = format!("{BRANCH_COPY_TEMP_PREFIX}{}", expected.id);
    let is_temporary = stage_dir.file_name().and_then(|name| name.to_str())
        == Some(expected_temporary_name.as_str());
    let marker_path = stage_dir.join(BRANCH_COPY_MARKER_BASENAME);
    let marker = read_bounded_file(&marker_path, MAX_MANIFEST_BYTES)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<ComposerAttachmentReceipt>(&bytes).ok());
    if marker.as_ref().is_some_and(|marker| marker != expected) || marker.is_none() && !is_temporary
    {
        return Ok(false);
    }
    let payload_name = generated_payload_name(&expected.name);
    let allowed = [
        "manifest.json",
        payload_name.as_str(),
        BRANCH_COPY_MARKER_BASENAME,
        BRANCH_COPY_MARKER_TEMP_BASENAME,
        "manifest.json.tmp",
        PAYLOAD_TEMP_BASENAME,
    ];
    for entry in fs::read_dir(stage_dir)
        .map_err(|_| "unable to inspect branch attachment copy".to_owned())?
    {
        let entry = entry.map_err(|_| "unable to inspect branch attachment copy".to_owned())?;
        let metadata = fs::symlink_metadata(entry.path())
            .map_err(|_| "unable to inspect branch attachment copy".to_owned())?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Ok(false);
        }
        let name = entry.file_name();
        if !allowed.iter().any(|allowed| name == *allowed) {
            return Ok(false);
        }
    }
    Ok(true)
}

fn remove_branch_copy_directory(root: &Path, path: &Path) -> Result<(), String> {
    let attachments_root = checked_attachments_root(root)?;
    ensure_no_symlink_ancestors(path)?;
    if path.parent() != Some(attachments_root.as_path()) {
        return Err("branch attachment temporary path is outside the staging root".into());
    }
    let metadata = fs::symlink_metadata(path)
        .map_err(|_| "branch attachment temporary copy was not found".to_owned())?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err("branch attachment temporary copy is invalid".into());
    }
    fs::remove_dir_all(path).map_err(|_| "unable to remove branch attachment temporary copy".into())
}

/// Resolve staged attachments after re-checking project ownership, manifest
/// ownership, canonical paths, size limits, and content hashes.
pub async fn resolve_composer_attachments(
    repository: &Store,
    project_id: Uuid,
    conversation_id: Uuid,
    attachment_ids: &[Uuid],
) -> Result<Vec<ResolvedComposerAttachment>, String> {
    let project = load_project_and_conversation(repository, project_id, conversation_id).await?;
    if attachment_ids.len() > MAX_ATTACHMENTS {
        return Err(format!(
            "composer attachments exceed the maximum of {MAX_ATTACHMENTS}"
        ));
    }

    let mut seen = HashSet::new();
    let ids = attachment_ids
        .iter()
        .copied()
        .filter(|id| seen.insert(*id))
        .collect::<Vec<_>>();
    if ids.len() > MAX_ATTACHMENTS {
        return Err(format!(
            "composer attachments exceed the maximum of {MAX_ATTACHMENTS}"
        ));
    }
    if ids.is_empty() {
        return Ok(Vec::new());
    }

    let root = canonical_project_root(&project)?;
    let mut pending = Vec::with_capacity(ids.len());
    let mut total_size = 0_u64;
    for id in ids {
        let (receipt, payload_path) =
            read_and_validate_manifest(&root, project_id, conversation_id, id)?;
        if receipt.size_bytes > MAX_ATTACHMENT_BYTES as u64 {
            return Err("attachment exceeds the 20 MiB per-file limit".into());
        }
        total_size = total_size
            .checked_add(receipt.size_bytes)
            .ok_or_else(|| "composer attachments exceed the 40 MiB total limit".to_owned())?;
        if total_size > MAX_TOTAL_ATTACHMENT_BYTES {
            return Err("composer attachments exceed the 40 MiB total limit".into());
        }
        pending.push((receipt, payload_path));
    }

    let mut resolved = Vec::with_capacity(pending.len());
    for (receipt, payload_path) in pending {
        let bytes = read_staged_bytes(&payload_path)?;
        if bytes.len() as u64 != receipt.size_bytes {
            return Err("attachment size does not match its manifest".into());
        }
        let actual_hash = hex::encode(Sha256::digest(&bytes));
        if !actual_hash.eq_ignore_ascii_case(&receipt.sha256) {
            return Err("attachment hash does not match its manifest".into());
        }
        let actual_media_type = detect_attachment_media_type(&receipt.name, &bytes)?;
        if actual_media_type != receipt.media_type {
            return Err("attachment media type does not match its manifest".into());
        }
        resolved.push(ResolvedComposerAttachment { receipt, bytes });
    }
    Ok(resolved)
}

/// Validate attachments without returning bytes to the frontend. This is used
/// as the final preflight immediately before a composer submission.
#[tauri::command]
pub async fn validate_composer_attachments(
    state: State<'_, AppState>,
    project_id: Uuid,
    conversation_id: Uuid,
    attachments: Vec<Uuid>,
    model_profile_id: Option<Uuid>,
) -> Result<(), String> {
    let resolved =
        resolve_composer_attachments(&state.repository, project_id, conversation_id, &attachments)
            .await?;
    let preferences = crate::conversation_preferences::load_conversation_agent_preferences(
        &state.repository,
        project_id,
        conversation_id,
    )
    .await?;
    crate::agent_v4::validate_attachment_model(
        &state.repository,
        model_profile_id,
        &resolved,
        Some(&preferences),
    )
    .await
}

/// Stage bytes supplied by a paste/drop operation. The request intentionally
/// contains no path; only the host reads or writes the resulting staging file.
#[tauri::command]
pub async fn stage_composer_attachment(
    state: State<'_, AppState>,
    request: StageComposerAttachmentRequest,
) -> Result<ComposerAttachmentReceipt, String> {
    let project = load_project_and_conversation(
        &state.repository,
        request.project_id,
        request.conversation_id,
    )
    .await?;
    let bytes = decode_attachment_content(&request)?;
    let conversation_id = request.conversation_id;
    tauri::async_runtime::spawn_blocking(move || {
        stage_attachment_bytes(&project, conversation_id, &request.name, &bytes)
    })
    .await
    .map_err(|error| error.to_string())?
}

/// Open the native file picker and stage the selected files. The browser only
/// receives receipts; source paths never cross the Tauri command boundary.
#[tauri::command]
pub async fn choose_composer_attachments(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    project_id: Uuid,
    conversation_id: Uuid,
    max_files: Option<usize>,
    max_bytes: Option<u64>,
) -> Result<Vec<ComposerAttachmentReceipt>, String> {
    let project =
        load_project_and_conversation(&state.repository, project_id, conversation_id).await?;
    tauri::async_runtime::spawn_blocking(move || {
        let Some(selected) = app.dialog().file().blocking_pick_files() else {
            return Ok(Vec::new());
        };
        let paths = selected
            .into_iter()
            .map(|file| {
                file.into_path()
                    .map_err(|_| "selected attachment path is unavailable".to_owned())
            })
            .collect::<Result<Vec<_>, _>>()?;
        stage_selected_paths_with_limits(&project, conversation_id, &paths, max_files, max_bytes)
    })
    .await
    .map_err(|error| error.to_string())?
}

async fn load_project_and_conversation(
    repository: &Store,
    project_id: Uuid,
    conversation_id: Uuid,
) -> Result<Project, String> {
    let project = repository
        .get_project(project_id)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "project was not found".to_owned())?;
    let owns_conversation = repository
        .conversations_for_project(project_id)
        .await
        .map_err(|error| error.to_string())?
        .iter()
        .any(|conversation| conversation.id == conversation_id);
    if !owns_conversation {
        return Err("conversation does not belong to the requested project".into());
    }
    Ok(project)
}

fn canonical_project_root(project: &Project) -> Result<PathBuf, String> {
    let root = PathBuf::from(project.local_root.trim());
    if root.as_os_str().is_empty() {
        return Err("project local root is empty".into());
    }
    ensure_no_symlink_ancestors(&root)?;
    let metadata = fs::symlink_metadata(&root).map_err(|_| "project local root is unavailable")?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err("project local root is not a directory".into());
    }
    Ok(fs::canonicalize(&root).map_err(|_| "project local root cannot be canonicalized")?)
}

fn staging_root(root: &Path) -> Result<PathBuf, String> {
    ensure_directory(
        &root.join(STAGING_DIRECTORY),
        "attachment staging directory",
    )?;
    let attachments = root.join(STAGING_DIRECTORY).join(ATTACHMENTS_DIRECTORY);
    ensure_directory(&attachments, "attachment directory")?;
    Ok(attachments)
}

fn attachment_directory(root: &Path, id: Uuid) -> PathBuf {
    root.join(STAGING_DIRECTORY)
        .join(ATTACHMENTS_DIRECTORY)
        .join(id.to_string())
}

fn expected_relative_path(id: Uuid, name: &str) -> String {
    format!(
        "{STAGING_DIRECTORY}/{ATTACHMENTS_DIRECTORY}/{id}/{}",
        generated_payload_name(name)
    )
}

fn generated_payload_name(name: &str) -> String {
    let extension = Path::new(name)
        .extension()
        .and_then(|value| value.to_str())
        .filter(|value| {
            !value.is_empty()
                && value.len() <= 32
                && value
                    .chars()
                    .all(|character| character.is_ascii_alphanumeric())
        });
    extension
        .map(|extension| format!("{PAYLOAD_BASENAME}.{extension}"))
        .unwrap_or_else(|| PAYLOAD_BASENAME.to_owned())
}

fn validate_attachment_name(name: &str) -> Result<String, String> {
    if name.is_empty()
        || name.trim() != name
        || name.as_bytes().len() > MAX_ATTACHMENT_NAME_BYTES
        || name == "."
        || name == ".."
    {
        return Err("attachment name is invalid".into());
    }
    if name.chars().any(|character| {
        character.is_control()
            || matches!(
                character,
                '/' | '\\' | ':' | '<' | '>' | '"' | '|' | '?' | '*'
            )
    }) {
        return Err("attachment name is invalid".into());
    }
    if name.ends_with('.') || name.ends_with(' ') {
        return Err("attachment name is invalid".into());
    }
    let device_name = name
        .split('.')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    if matches!(
        device_name.as_str(),
        "con"
            | "prn"
            | "aux"
            | "nul"
            | "com1"
            | "com2"
            | "com3"
            | "com4"
            | "com5"
            | "com6"
            | "com7"
            | "com8"
            | "com9"
            | "lpt1"
            | "lpt2"
            | "lpt3"
            | "lpt4"
            | "lpt5"
            | "lpt6"
            | "lpt7"
            | "lpt8"
            | "lpt9"
    ) {
        return Err("attachment name is invalid".into());
    }
    Ok(name.to_owned())
}

fn validate_generated_relative_path(relative_path: &str) -> Result<(), String> {
    if relative_path.is_empty() || relative_path.contains('\\') {
        return Err("attachment manifest path is invalid".into());
    }
    let path = Path::new(relative_path);
    if path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err("attachment manifest path is invalid".into());
    }
    Ok(())
}

fn validate_sha256(value: &str) -> Result<(), String> {
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("attachment manifest hash is invalid".into());
    }
    Ok(())
}

pub(crate) fn ensure_no_symlink_ancestors(path: &Path) -> Result<(), String> {
    // Walking from the leaf avoids probing a bare Windows drive prefix (for
    // example `E:`), which is not itself a valid filesystem path.
    let mut current = path;
    loop {
        match fs::symlink_metadata(current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err("attachment path contains a symlink".into());
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err("attachment path cannot be inspected".into()),
        }
        let Some(parent) = current.parent() else {
            break;
        };
        if parent == current {
            break;
        }
        current = parent;
    }
    Ok(())
}

fn ensure_directory(path: &Path, description: &str) -> Result<(), String> {
    ensure_no_symlink_ancestors(path)?;
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            Err(format!("{description} contains a symlink"))
        }
        Ok(metadata) if metadata.is_dir() => Ok(()),
        Ok(_) => Err(format!("{description} is not a directory")),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            fs::create_dir(path).map_err(|_| format!("unable to create {description}"))?;
            set_private_permissions(path, true)?;
            Ok(())
        }
        Err(_) => Err(format!("unable to inspect {description}")),
    }
}

fn create_attachment_directory(path: &Path) -> Result<(), String> {
    ensure_no_symlink_ancestors(path)?;
    match fs::symlink_metadata(path) {
        Ok(_) => Err("attachment staging id already exists".into()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            fs::create_dir(path).map_err(|_| "unable to create attachment staging directory")?;
            set_private_permissions(path, true)?;
            Ok(())
        }
        Err(_) => Err("unable to inspect attachment staging directory".into()),
    }
}

fn set_private_permissions(path: &Path, directory: bool) -> Result<(), String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = if directory { 0o700 } else { 0o600 };
        let mut permissions = fs::metadata(path)
            .map_err(|_| "unable to inspect staged attachment permissions")?
            .permissions();
        permissions.set_mode(mode);
        fs::set_permissions(path, permissions)
            .map_err(|_| "unable to set staged attachment permissions")?;
    }
    #[cfg(not(unix))]
    {
        let _ = (path, directory);
    }
    Ok(())
}

fn reject_obvious_credentials(bytes: &[u8]) -> Result<(), String> {
    let Ok(text) = std::str::from_utf8(bytes) else {
        return Ok(());
    };
    let lower = text.to_ascii_lowercase();
    if lower.contains("-----begin ") && lower.contains("private key-----") {
        return Err("attachment contains a private key and was not staged".into());
    }
    Ok(())
}

fn reject_obvious_credential_name(name: &str) -> Result<(), String> {
    let lower = name.to_ascii_lowercase();
    if lower == ".env"
        || lower.starts_with(".env.")
        || matches!(
            lower.as_str(),
            "id_rsa"
                | "id_dsa"
                | "id_ecdsa"
                | "id_ed25519"
                | "credentials"
                | "credentials.json"
                | "secrets.json"
                | "secret"
                | "secret.json"
        )
    {
        return Err("attachment looks like a credential file and was not staged".into());
    }
    Ok(())
}

fn stage_attachment_bytes(
    project: &Project,
    conversation_id: Uuid,
    name: &str,
    bytes: &[u8],
) -> Result<ComposerAttachmentReceipt, String> {
    let name = validate_attachment_name(name)?;
    reject_obvious_credential_name(&name)?;
    if bytes.len() > MAX_ATTACHMENT_BYTES {
        return Err("attachment exceeds the 20 MiB per-file limit".into());
    }
    reject_obvious_credentials(bytes)?;
    let media_type = detect_attachment_media_type(&name, bytes)?.to_owned();
    let root = canonical_project_root(project)?;
    staging_root(&root)?;
    let id = Uuid::new_v4();
    let stage_dir = attachment_directory(&root, id);
    create_attachment_directory(&stage_dir)?;
    let result = stage_attachment_in_directory(
        &stage_dir,
        id,
        project.id,
        conversation_id,
        &name,
        bytes,
        media_type,
    );
    if result.is_err() {
        remove_staged_directory(&root, id);
    }
    result
}

fn stage_attachment_in_directory(
    stage_dir: &Path,
    id: Uuid,
    project_id: Uuid,
    conversation_id: Uuid,
    name: &str,
    bytes: &[u8],
    media_type: String,
) -> Result<ComposerAttachmentReceipt, String> {
    let relative_path = expected_relative_path(id, name);
    let payload_name = generated_payload_name(name);
    let payload_path = stage_dir.join(payload_name);
    let payload_tmp = stage_dir.join(PAYLOAD_TEMP_BASENAME);
    write_atomic_private_file(&payload_tmp, &payload_path, bytes)?;

    let receipt = ComposerAttachmentReceipt {
        id,
        project_id,
        conversation_id,
        name: name.to_owned(),
        relative_path,
        size_bytes: bytes.len() as u64,
        sha256: hex::encode(Sha256::digest(bytes)),
        media_type,
    };
    let manifest_tmp = stage_dir.join("manifest.json.tmp");
    let manifest =
        serde_json::to_vec(&receipt).map_err(|_| "unable to encode attachment manifest")?;
    write_atomic_private_file(&manifest_tmp, &stage_dir.join("manifest.json"), &manifest)?;
    Ok(receipt)
}

fn write_atomic_private_file(tmp: &Path, destination: &Path, bytes: &[u8]) -> Result<(), String> {
    ensure_no_symlink_ancestors(tmp)?;
    ensure_no_symlink_ancestors(destination)?;
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(tmp)
        .map_err(|_| "unable to create staged attachment file")?;
    file.write_all(bytes)
        .map_err(|_| "unable to write staged attachment file")?;
    file.sync_all()
        .map_err(|_| "unable to flush staged attachment file")?;
    drop(file);
    set_private_permissions(tmp, false)?;
    fs::rename(tmp, destination).map_err(|_| "unable to commit staged attachment file")?;
    Ok(())
}

#[cfg(test)]
fn stage_selected_paths(
    project: &Project,
    conversation_id: Uuid,
    paths: &[PathBuf],
) -> Result<Vec<ComposerAttachmentReceipt>, String> {
    stage_selected_paths_with_limits(project, conversation_id, paths, None, None)
}

fn stage_selected_paths_with_limits(
    project: &Project,
    conversation_id: Uuid,
    paths: &[PathBuf],
    max_files: Option<usize>,
    max_bytes: Option<u64>,
) -> Result<Vec<ComposerAttachmentReceipt>, String> {
    let file_limit = max_files.unwrap_or(MAX_ATTACHMENTS).min(MAX_ATTACHMENTS);
    let byte_limit = max_bytes
        .unwrap_or(MAX_TOTAL_ATTACHMENT_BYTES)
        .min(MAX_TOTAL_ATTACHMENT_BYTES);
    if paths.len() > file_limit {
        return Err(format!(
            "selected attachments exceed the maximum of {file_limit}"
        ));
    }
    let mut selected = Vec::with_capacity(paths.len());
    let mut total_size = 0_u64;
    for path in paths {
        ensure_no_symlink_ancestors(path)?;
        let metadata =
            fs::symlink_metadata(path).map_err(|_| "selected attachment is unavailable")?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err("selected attachment is not a regular file".into());
        }
        if metadata.len() > MAX_ATTACHMENT_BYTES as u64 {
            return Err("attachment exceeds the 20 MiB per-file limit".into());
        }
        let remaining_bytes = byte_limit.saturating_sub(total_size);
        if metadata.len() > remaining_bytes {
            return Err("selected attachments exceed the requested attachment size limit".into());
        }
        let name = path
            .file_name()
            .and_then(|value| value.to_str())
            .ok_or_else(|| "selected attachment name is invalid".to_owned())?;
        let name = validate_attachment_name(name)?;
        let bytes = read_bounded_file(path, MAX_ATTACHMENT_BYTES)?;
        if bytes.len() as u64 > remaining_bytes {
            return Err("selected attachments exceed the requested attachment size limit".into());
        }
        reject_obvious_credential_name(&name)?;
        reject_obvious_credentials(&bytes)?;
        detect_attachment_media_type(&name, &bytes)?;
        total_size = total_size
            .checked_add(bytes.len() as u64)
            .ok_or_else(|| "selected attachments exceed the 40 MiB total limit".to_owned())?;
        if total_size > byte_limit {
            return Err("selected attachments exceed the requested attachment size limit".into());
        }
        selected.push((name, bytes));
    }

    let mut receipts = Vec::with_capacity(selected.len());
    for (name, bytes) in selected {
        match stage_attachment_bytes(project, conversation_id, &name, &bytes) {
            Ok(receipt) => receipts.push(receipt),
            Err(error) => {
                for receipt in &receipts {
                    remove_staged_attachment(project, receipt.id);
                }
                return Err(error);
            }
        }
    }
    Ok(receipts)
}

fn remove_staged_attachment(project: &Project, id: Uuid) {
    if let Ok(root) = canonical_project_root(project) {
        remove_staged_directory(&root, id);
    }
}

fn remove_staged_directory(root: &Path, id: Uuid) {
    let Ok(attachments_root) = checked_attachments_root(root) else {
        return;
    };
    let stage_dir = attachments_root.join(id.to_string());
    let Ok(stage_dir) = checked_staged_directory(&attachments_root, &stage_dir, id) else {
        return;
    };
    let _ = fs::remove_dir_all(stage_dir);
}

fn checked_attachments_root(root: &Path) -> Result<PathBuf, String> {
    let attachments = root.join(STAGING_DIRECTORY).join(ATTACHMENTS_DIRECTORY);
    ensure_no_symlink_ancestors(&attachments)?;
    let metadata =
        fs::symlink_metadata(&attachments).map_err(|_| "attachment directory is unavailable")?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err("attachment directory is invalid".into());
    }
    let canonical = fs::canonicalize(&attachments)
        .map_err(|_| "attachment directory cannot be canonicalized")?;
    if !path_is_within(&canonical, root) {
        return Err("attachment directory is outside the project root".into());
    }
    Ok(canonical)
}

fn checked_staged_directory(
    attachments_root: &Path,
    stage_dir: &Path,
    id: Uuid,
) -> Result<PathBuf, String> {
    ensure_no_symlink_ancestors(stage_dir)?;
    let metadata = fs::symlink_metadata(stage_dir)
        .map_err(|_| "attachment staging directory is unavailable")?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err("attachment staging directory is invalid".into());
    }
    let canonical = fs::canonicalize(stage_dir)
        .map_err(|_| "attachment staging directory cannot be canonicalized")?;
    let expected_name = id.to_string();
    if canonical.parent() != Some(attachments_root)
        || canonical.file_name().and_then(|name| name.to_str()) != Some(expected_name.as_str())
    {
        return Err("attachment staging directory is outside the expected root".into());
    }
    Ok(canonical)
}

fn read_bounded_file(path: &Path, maximum: usize) -> Result<Vec<u8>, String> {
    ensure_no_symlink_ancestors(path)?;
    let metadata = fs::symlink_metadata(path).map_err(|_| "attachment file is unavailable")?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err("attachment file is not a regular file".into());
    }
    let mut file = File::open(path).map_err(|_| "attachment file cannot be read")?;
    let mut bytes = Vec::new();
    Read::by_ref(&mut file)
        .take(maximum as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "attachment file cannot be read")?;
    if bytes.len() > maximum {
        return Err("attachment exceeds the 20 MiB per-file limit".into());
    }
    Ok(bytes)
}

fn read_staged_bytes(path: &Path) -> Result<Vec<u8>, String> {
    read_bounded_file(path, MAX_ATTACHMENT_BYTES)
}

fn read_manifest(path: &Path) -> Result<ComposerAttachmentReceipt, String> {
    ensure_no_symlink_ancestors(path)?;
    let metadata = fs::symlink_metadata(path).map_err(|_| "attachment manifest was not found")?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err("attachment manifest is not a regular file".into());
    }
    let bytes = read_bounded_file(path, MAX_MANIFEST_BYTES)?;
    serde_json::from_slice(&bytes).map_err(|_| "attachment manifest is invalid".into())
}

fn read_and_validate_manifest(
    root: &Path,
    project_id: Uuid,
    conversation_id: Uuid,
    id: Uuid,
) -> Result<(ComposerAttachmentReceipt, PathBuf), String> {
    let stage_dir = attachment_directory(root, id);
    ensure_no_symlink_ancestors(&stage_dir)?;
    let stage_metadata =
        fs::symlink_metadata(&stage_dir).map_err(|_| "attachment manifest was not found")?;
    if stage_metadata.file_type().is_symlink() || !stage_metadata.is_dir() {
        return Err("attachment staging directory is invalid".into());
    }
    let manifest = read_manifest(&stage_dir.join("manifest.json"))?;
    if manifest.id != id {
        return Err("attachment manifest id does not match the requested id".into());
    }
    if manifest.project_id != project_id {
        return Err("attachment manifest project does not match the requested project".into());
    }
    if manifest.conversation_id != conversation_id {
        return Err(
            "attachment manifest conversation does not match the requested conversation".into(),
        );
    }
    validate_attachment_name(&manifest.name)?;
    reject_obvious_credential_name(&manifest.name)?;
    validate_sha256(&manifest.sha256)?;
    if !matches!(
        manifest.media_type.as_str(),
        "image/png"
            | "image/jpeg"
            | "image/webp"
            | "text/plain"
            | "text/csv"
            | "application/octet-stream"
    ) {
        return Err("attachment manifest media type is not allowed".into());
    }
    let expected_relative = expected_relative_path(id, &manifest.name);
    validate_generated_relative_path(&manifest.relative_path)?;
    if manifest.relative_path != expected_relative {
        return Err("attachment manifest path does not match the generated staging path".into());
    }

    let payload_path = root.join(&manifest.relative_path);
    ensure_no_symlink_ancestors(&payload_path)?;
    let payload_metadata = fs::symlink_metadata(&payload_path)
        .map_err(|_| "staged attachment bytes were not found")?;
    if payload_metadata.file_type().is_symlink() || !payload_metadata.is_file() {
        return Err("staged attachment bytes are not a regular file".into());
    }

    let attachments_root = root.join(STAGING_DIRECTORY).join(ATTACHMENTS_DIRECTORY);
    let canonical_attachments = fs::canonicalize(&attachments_root)
        .map_err(|_| "attachment directory cannot be canonicalized")?;
    let canonical_stage = fs::canonicalize(&stage_dir)
        .map_err(|_| "attachment staging directory cannot be canonicalized")?;
    let canonical_payload = fs::canonicalize(&payload_path)
        .map_err(|_| "staged attachment bytes cannot be canonicalized")?;
    let expected_payload_name = generated_payload_name(&manifest.name);
    if !path_is_within(&canonical_stage, &canonical_attachments)
        || canonical_payload.parent() != Some(canonical_stage.as_path())
        || canonical_payload.file_name().and_then(|name| name.to_str())
            != Some(expected_payload_name.as_str())
    {
        return Err("attachment path is outside the project staging directory".into());
    }
    Ok((manifest, canonical_payload))
}

fn path_is_within(child: &Path, parent: &Path) -> bool {
    #[cfg(windows)]
    {
        let child = child.to_string_lossy().to_ascii_lowercase();
        let parent = parent.to_string_lossy().to_ascii_lowercase();
        child == parent
            || child.starts_with(&format!("{parent}\\"))
            || child.starts_with(&format!("{parent}/"))
    }
    #[cfg(not(windows))]
    {
        child.starts_with(parent)
    }
}

fn decode_attachment_content(request: &StageComposerAttachmentRequest) -> Result<Vec<u8>, String> {
    if request.content_base64.len() > MAX_ATTACHMENT_BASE64_BYTES {
        return Err("attachment exceeds the 20 MiB per-file limit".into());
    }
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(&request.content_base64)
        .map_err(|_| "invalid attachment encoding")?;
    if bytes.len() > MAX_ATTACHMENT_BYTES {
        return Err("attachment exceeds the 20 MiB per-file limit".into());
    }
    Ok(bytes)
}

fn detect_attachment_media_type(name: &str, bytes: &[u8]) -> Result<&'static str, String> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        if bytes.len() < 24 || &bytes[12..16] != b"IHDR" {
            return Err("PNG image header is invalid".into());
        }
        let width = u32::from_be_bytes(bytes[16..20].try_into().unwrap());
        let height = u32::from_be_bytes(bytes[20..24].try_into().unwrap());
        if width == 0
            || height == 0
            || width > MAX_IMAGE_DIMENSION
            || height > MAX_IMAGE_DIMENSION
            || u64::from(width) * u64::from(height) > MAX_IMAGE_PIXELS
        {
            return Err("PNG image dimensions exceed attachment limits".into());
        }
        return Ok("image/png");
    }
    if bytes.starts_with(b"\xff\xd8\xff") {
        return Ok("image/jpeg");
    }
    if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        return Ok("image/webp");
    }

    let extension = Path::new(name)
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| value.to_ascii_lowercase());
    let text_extension = extension.as_deref().is_some_and(|value| {
        matches!(
            value,
            "txt"
                | "text"
                | "md"
                | "csv"
                | "tsv"
                | "tab"
                | "json"
                | "yaml"
                | "yml"
                | "xml"
                | "html"
                | "htm"
                | "log"
                | "py"
                | "r"
                | "rs"
                | "js"
                | "jsx"
                | "ts"
                | "tsx"
                | "fasta"
                | "fa"
                | "fastq"
                | "fq"
                | "gff"
                | "gtf"
                | "vcf"
                | "bed"
        )
    });
    if text_extension && !bytes.contains(&0) && std::str::from_utf8(bytes).is_ok() {
        if matches!(extension.as_deref(), Some("csv" | "tsv" | "tab")) {
            return Ok("text/csv");
        }
        return Ok("text/plain");
    }
    Ok("application/octet-stream")
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use omicsops_core::workspace::{Conversation, Project, ProjectTemplate};
    use omicsops_store::Store;
    use std::fs;
    use tempfile::{TempDir, tempdir};
    use uuid::Uuid;

    struct Fixture {
        _directory: TempDir,
        store: Store,
        project: Project,
        conversation: Conversation,
    }

    async fn fixture() -> Fixture {
        let directory = tempdir().unwrap();
        let project_root = directory.path().join("project");
        fs::create_dir_all(&project_root).unwrap();
        let store = Store::open(directory.path().join("state.sqlite"))
            .await
            .unwrap();
        let project = Project::new(
            Uuid::new_v4(),
            "attachment test",
            project_root.to_string_lossy(),
            ProjectTemplate::Blank,
            Utc::now(),
        );
        store.save_project(&project).await.unwrap();
        let conversation = Conversation::new(Uuid::new_v4(), project.id, "composer", Utc::now());
        store.save_conversation(&conversation).await.unwrap();
        Fixture {
            _directory: directory,
            store,
            project,
            conversation,
        }
    }

    #[test]
    fn rejects_empty_bounded_or_path_like_names() {
        for name in [
            "",
            " ",
            "a/b.txt",
            r"a\b.txt",
            "C:\\secret.txt",
            "..",
            ".",
            "bad\u{0000}.txt",
            "bad?.txt",
            &"x".repeat(MAX_ATTACHMENT_NAME_BYTES + 1),
        ] {
            assert!(validate_attachment_name(name).is_err(), "{name:?}");
        }
        assert!(validate_attachment_name("report.csv").is_ok());
        assert!(validate_attachment_name("结果 1.txt").is_ok());
    }

    #[tokio::test]
    async fn stages_atomic_manifest_and_resolves_bytes_by_id() {
        let fixture = fixture().await;
        let receipt = stage_attachment_bytes(
            &fixture.project,
            fixture.conversation.id,
            "results.csv",
            b"gene,value\nA,1\n",
        )
        .unwrap();
        assert_eq!(receipt.project_id, fixture.project.id);
        assert_eq!(receipt.conversation_id, fixture.conversation.id);
        assert_eq!(receipt.media_type, "text/csv");
        assert!(!receipt.relative_path.contains(".."));
        let staged = fixture.project_root().join(&receipt.relative_path);
        assert_eq!(fs::read(&staged).unwrap(), b"gene,value\nA,1\n");
        assert!(staged.parent().unwrap().join("manifest.json").is_file());
        assert!(!staged.parent().unwrap().join("bytes.tmp").exists());
        assert!(!staged.parent().unwrap().join("manifest.json.tmp").exists());

        let resolved = resolve_composer_attachments(
            &fixture.store,
            fixture.project.id,
            fixture.conversation.id,
            &[receipt.id],
        )
        .await
        .unwrap();
        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].receipt, receipt);
        assert_eq!(resolved[0].bytes, b"gene,value\nA,1\n");
    }

    #[tokio::test]
    async fn generated_payload_preserves_safe_extension_without_traversal() {
        let fixture = fixture().await;
        let receipt = stage_attachment_bytes(
            &fixture.project,
            fixture.conversation.id,
            "sample.tar.gz",
            b"archive",
        )
        .unwrap();
        assert!(receipt.relative_path.ends_with("/bytes.gz"));
        assert!(!receipt.relative_path.contains("sample"));
        assert!(!receipt.relative_path.contains(".."));
    }

    #[test]
    fn branch_copy_recovers_verified_partial_destination() {
        let directory = tempdir().unwrap();
        let project_root = directory.path().join("project");
        fs::create_dir_all(&project_root).unwrap();
        let project = Project::new(
            Uuid::new_v4(),
            "branch copy recovery",
            project_root.to_string_lossy(),
            ProjectTemplate::Blank,
            Utc::now(),
        );
        let source_id = Uuid::new_v4();
        let target_id = Uuid::new_v4();
        let source_conversation_id = Uuid::new_v4();
        let target_conversation_id = Uuid::new_v4();
        let bytes = b"recovered branch material\n".to_vec();
        let source = ResolvedComposerAttachment {
            receipt: ComposerAttachmentReceipt {
                id: source_id,
                project_id: project.id,
                conversation_id: source_conversation_id,
                name: "notes.txt".into(),
                relative_path: expected_relative_path(source_id, "notes.txt"),
                size_bytes: bytes.len() as u64,
                sha256: hex::encode(Sha256::digest(&bytes)),
                media_type: "text/plain".into(),
            },
            bytes: bytes.clone(),
        };
        let root = canonical_project_root(&project).unwrap();
        staging_root(&root).unwrap();
        let expected = branch_copy_receipt(
            project.id,
            target_conversation_id,
            target_id,
            "notes.txt",
            &bytes,
            "text/plain",
        );
        let partial = attachment_directory(&root, target_id);
        create_attachment_directory(&partial).unwrap();
        write_branch_copy_marker(&partial, &expected).unwrap();
        fs::write(
            partial.join("manifest.json.tmp"),
            b"crashed before manifest",
        )
        .unwrap();

        let copied = copy_resolved_attachment_to_conversation(
            &project,
            &source,
            target_conversation_id,
            target_id,
        )
        .unwrap();
        assert_eq!(copied, expected);
        assert!(!partial.join("manifest.json.tmp").exists());
        assert!(partial.join("manifest.json").is_file());
        assert_eq!(fs::read(partial.join("bytes.txt")).unwrap(), bytes);

        let second_target_id = Uuid::new_v4();
        let second_expected = branch_copy_receipt(
            project.id,
            target_conversation_id,
            second_target_id,
            "notes.txt",
            &bytes,
            "text/plain",
        );
        let temporary = branch_copy_temporary_directory(&root, second_target_id);
        fs::create_dir(&temporary).unwrap();
        fs::write(
            temporary.join(BRANCH_COPY_MARKER_TEMP_BASENAME),
            b"crashed while writing marker",
        )
        .unwrap();
        let copied = copy_resolved_attachment_to_conversation(
            &project,
            &source,
            target_conversation_id,
            second_target_id,
        )
        .unwrap();
        assert_eq!(copied, second_expected);
        assert!(
            attachment_directory(&root, second_target_id)
                .join("manifest.json")
                .is_file()
        );

        let protected_target_id = Uuid::new_v4();
        let protected = attachment_directory(&root, protected_target_id);
        create_attachment_directory(&protected).unwrap();
        fs::write(protected.join("manifest.json.tmp"), b"unowned partial").unwrap();
        assert!(
            copy_resolved_attachment_to_conversation(
                &project,
                &source,
                target_conversation_id,
                protected_target_id,
            )
            .is_err()
        );
        assert!(protected.join("manifest.json.tmp").is_file());
    }

    #[tokio::test]
    async fn stages_tmp_extension_without_colliding_with_payload_temp_file() {
        let fixture = fixture().await;
        let receipt = stage_attachment_bytes(
            &fixture.project,
            fixture.conversation.id,
            "notes.tmp",
            b"temporary-looking data",
        )
        .unwrap();
        assert!(receipt.relative_path.ends_with("/bytes.tmp"));
        let payload = fixture.project_root().join(&receipt.relative_path);
        assert_eq!(fs::read(payload).unwrap(), b"temporary-looking data");
        assert!(
            !fixture
                .project_root()
                .join(".omicsops/attachments")
                .join(receipt.id.to_string())
                .join(PAYLOAD_TEMP_BASENAME)
                .exists()
        );
    }

    #[tokio::test]
    async fn stages_selected_native_paths_without_exposing_source_path() {
        let fixture = fixture().await;
        let source = fixture.directory().join("picked.txt");
        fs::write(&source, b"picked locally").unwrap();
        let receipts =
            stage_selected_paths(&fixture.project, fixture.conversation.id, &[source.clone()])
                .unwrap();
        assert_eq!(receipts.len(), 1);
        assert_eq!(receipts[0].name, "picked.txt");
        assert!(
            !serde_json::to_string(&receipts[0])
                .unwrap()
                .contains(&source.to_string_lossy().to_string())
        );
    }

    #[tokio::test]
    async fn chooser_limits_reject_before_any_staging() {
        let limited_fixture = fixture().await;
        let first = limited_fixture.directory().join("first.txt");
        let second = limited_fixture.directory().join("second.txt");
        fs::write(&first, b"first").unwrap();
        fs::write(&second, b"second").unwrap();
        let error = stage_selected_paths_with_limits(
            &limited_fixture.project,
            limited_fixture.conversation.id,
            &[first.clone(), second.clone()],
            Some(1),
            None,
        )
        .unwrap_err();
        assert!(error.contains("maximum of 1"));
        assert!(!limited_fixture.project_root().join(".omicsops").exists());

        let size_fixture = fixture().await;
        let first = size_fixture.directory().join("first.txt");
        let second = size_fixture.directory().join("second.txt");
        fs::write(&first, b"first").unwrap();
        fs::write(&second, b"second").unwrap();
        let error = stage_selected_paths_with_limits(
            &size_fixture.project,
            size_fixture.conversation.id,
            &[first, second],
            None,
            Some(5),
        )
        .unwrap_err();
        assert!(error.contains("size limit"));
        assert!(!size_fixture.project_root().join(".omicsops").exists());

        let credential_fixture = fixture().await;
        let valid = credential_fixture.directory().join("valid.txt");
        let credential = credential_fixture.directory().join(".env");
        fs::write(&valid, b"valid").unwrap();
        fs::write(&credential, b"TOKEN=do-not-stage").unwrap();
        let error = stage_selected_paths_with_limits(
            &credential_fixture.project,
            credential_fixture.conversation.id,
            &[valid, credential],
            None,
            None,
        )
        .unwrap_err();
        assert!(error.contains("credential"));
        assert!(!credential_fixture.project_root().join(".omicsops").exists());
    }

    #[tokio::test]
    async fn rejects_oversized_explicit_bytes_before_staging() {
        let fixture = fixture().await;
        let bytes = vec![0_u8; MAX_ATTACHMENT_BYTES + 1];
        let error = stage_attachment_bytes(
            &fixture.project,
            fixture.conversation.id,
            "too-large.bin",
            &bytes,
        )
        .unwrap_err();
        assert!(error.contains("20 MiB"));
        assert!(!fixture.project_root().join(".omicsops").exists());
    }

    #[tokio::test]
    async fn rejects_obvious_private_key_payloads_before_staging() {
        let fixture = fixture().await;
        let error = stage_attachment_bytes(
            &fixture.project,
            fixture.conversation.id,
            "notes.txt",
            b"-----BEGIN OPENSSH PRIVATE KEY-----\nsecret\n-----END OPENSSH PRIVATE KEY-----\n",
        )
        .unwrap_err();
        assert!(error.to_ascii_lowercase().contains("private key"));
        assert!(!fixture.project_root().join(".omicsops").exists());

        for name in [".env", "id_ed25519", "credentials.json"] {
            let error = stage_attachment_bytes(
                &fixture.project,
                fixture.conversation.id,
                name,
                b"ordinary-looking contents",
            )
            .unwrap_err();
            assert!(error.to_ascii_lowercase().contains("credential"));
        }
    }

    #[test]
    fn detects_supported_images_and_safe_generic_text() {
        let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
        png.extend_from_slice(&13_u32.to_be_bytes());
        png.extend_from_slice(b"IHDR");
        png.extend_from_slice(&100_u32.to_be_bytes());
        png.extend_from_slice(&50_u32.to_be_bytes());
        assert_eq!(
            detect_attachment_media_type("plot.png", &png).unwrap(),
            "image/png"
        );
        assert_eq!(
            detect_attachment_media_type("plot.jpeg", &[0xff, 0xd8, 0xff, 0xe0]).unwrap(),
            "image/jpeg"
        );
        assert_eq!(
            detect_attachment_media_type("plot.webp", b"RIFF\x10\0\0\0WEBPVP8 ").unwrap(),
            "image/webp"
        );
        assert_eq!(
            detect_attachment_media_type("notes.txt", b"plain text\n").unwrap(),
            "text/plain"
        );
        assert_eq!(
            detect_attachment_media_type("data.tsv", b"gene\tvalue\n").unwrap(),
            "text/csv"
        );
        assert_eq!(
            detect_attachment_media_type("unknown.bin", &[0, 159, 146, 150]).unwrap(),
            "application/octet-stream"
        );
    }

    #[test]
    fn rejects_image_dimensions_that_exceed_signature_bounds() {
        let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
        png.extend_from_slice(&13_u32.to_be_bytes());
        png.extend_from_slice(b"IHDR");
        png.extend_from_slice(&20_000_u32.to_be_bytes());
        png.extend_from_slice(&20_000_u32.to_be_bytes());
        assert!(detect_attachment_media_type("huge.png", &png).is_err());
    }

    #[tokio::test]
    async fn rejects_manifest_tampering_hash_change_and_missing_bytes() {
        let fixture = fixture().await;
        let receipt = stage_attachment_bytes(
            &fixture.project,
            fixture.conversation.id,
            "safe.txt",
            b"original",
        )
        .unwrap();
        let directory = fixture.project_root().join(&receipt.relative_path);
        fs::write(&directory, b"changed!").unwrap();
        let error = resolve_composer_attachments(
            &fixture.store,
            fixture.project.id,
            fixture.conversation.id,
            &[receipt.id],
        )
        .await
        .unwrap_err();
        assert!(error.to_ascii_lowercase().contains("hash"));

        fs::write(&directory, b"original").unwrap();
        fs::write(
            directory.parent().unwrap().join("manifest.json"),
            serde_json::to_vec(&ComposerAttachmentReceipt {
                relative_path: "../../outside.txt".into(),
                ..receipt.clone()
            })
            .unwrap(),
        )
        .unwrap();
        let error = resolve_composer_attachments(
            &fixture.store,
            fixture.project.id,
            fixture.conversation.id,
            &[receipt.id],
        )
        .await
        .unwrap_err();
        assert!(error.to_ascii_lowercase().contains("path"));

        fs::remove_dir_all(directory.parent().unwrap()).unwrap();
        let error = resolve_composer_attachments(
            &fixture.store,
            fixture.project.id,
            fixture.conversation.id,
            &[receipt.id],
        )
        .await
        .unwrap_err();
        assert!(error.to_ascii_lowercase().contains("manifest") || error.contains("attachment"));
    }

    #[tokio::test]
    async fn rejects_manifest_owner_and_conversation_mismatch() {
        let fixture = fixture().await;
        let receipt = stage_attachment_bytes(
            &fixture.project,
            fixture.conversation.id,
            "safe.txt",
            b"original",
        )
        .unwrap();
        let manifest_path = fixture
            .project_root()
            .join(&receipt.relative_path)
            .parent()
            .unwrap()
            .join("manifest.json");
        let owner_tamper = ComposerAttachmentReceipt {
            project_id: Uuid::new_v4(),
            ..receipt.clone()
        };
        fs::write(&manifest_path, serde_json::to_vec(&owner_tamper).unwrap()).unwrap();
        let error = resolve_composer_attachments(
            &fixture.store,
            fixture.project.id,
            fixture.conversation.id,
            &[receipt.id],
        )
        .await
        .unwrap_err();
        assert!(error.to_ascii_lowercase().contains("project"));

        let conversation_tamper = ComposerAttachmentReceipt {
            project_id: fixture.project.id,
            conversation_id: Uuid::new_v4(),
            ..receipt
        };
        fs::write(
            &manifest_path,
            serde_json::to_vec(&conversation_tamper).unwrap(),
        )
        .unwrap();
        let error = resolve_composer_attachments(
            &fixture.store,
            fixture.project.id,
            fixture.conversation.id,
            &[conversation_tamper.id],
        )
        .await
        .unwrap_err();
        assert!(error.to_ascii_lowercase().contains("conversation"));
    }

    #[tokio::test]
    async fn enforces_attachment_count_and_total_manifest_size_bounds() {
        let fixture = fixture().await;
        let too_many = (0..=MAX_ATTACHMENTS)
            .map(|_| Uuid::new_v4())
            .collect::<Vec<_>>();
        let error = resolve_composer_attachments(
            &fixture.store,
            fixture.project.id,
            fixture.conversation.id,
            &too_many,
        )
        .await
        .unwrap_err();
        assert!(error.contains("8"));

        let mut receipts = Vec::new();
        for index in 0..3 {
            receipts.push(
                stage_attachment_bytes(
                    &fixture.project,
                    fixture.conversation.id,
                    &format!("{index}.bin"),
                    b"small",
                )
                .unwrap(),
            );
        }
        for receipt in &receipts {
            let manifest_path = fixture
                .project_root()
                .join(&receipt.relative_path)
                .parent()
                .unwrap()
                .join("manifest.json");
            let tampered = ComposerAttachmentReceipt {
                size_bytes: 14 * 1024 * 1024,
                ..receipt.clone()
            };
            fs::write(manifest_path, serde_json::to_vec(&tampered).unwrap()).unwrap();
        }
        let ids = receipts
            .iter()
            .map(|receipt| receipt.id)
            .collect::<Vec<_>>();
        let error = resolve_composer_attachments(
            &fixture.store,
            fixture.project.id,
            fixture.conversation.id,
            &ids,
        )
        .await
        .unwrap_err();
        assert!(error.contains("40 MiB"));
    }

    #[tokio::test]
    async fn validates_project_and_conversation_before_resolving_ids() {
        let fixture = fixture().await;
        let error = resolve_composer_attachments(
            &fixture.store,
            Uuid::new_v4(),
            fixture.conversation.id,
            &[],
        )
        .await
        .unwrap_err();
        assert!(error.to_ascii_lowercase().contains("project"));

        let error =
            resolve_composer_attachments(&fixture.store, fixture.project.id, Uuid::new_v4(), &[])
                .await
                .unwrap_err();
        assert!(error.to_ascii_lowercase().contains("conversation"));
    }

    #[test]
    fn decodes_bounded_base64_request_without_accepting_paths() {
        let bytes = b"paste me";
        let request = StageComposerAttachmentRequest {
            project_id: Uuid::new_v4(),
            conversation_id: Uuid::new_v4(),
            name: "paste.txt".into(),
            content_base64: base64::engine::general_purpose::STANDARD.encode(bytes),
        };
        assert_eq!(decode_attachment_content(&request).unwrap(), bytes);
        assert!(
            decode_attachment_content(&StageComposerAttachmentRequest {
                content_base64: "not base64".into(),
                ..request.clone()
            })
            .is_err()
        );
        assert!(
            decode_attachment_content(&StageComposerAttachmentRequest {
                content_base64: "a".repeat(MAX_ATTACHMENT_BASE64_BYTES + 1),
                ..request
            })
            .is_err()
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn rejects_symlinked_attachment_ancestor() {
        use std::os::unix::fs::symlink;

        let fixture = fixture().await;
        let receipt = stage_attachment_bytes(
            &fixture.project,
            fixture.conversation.id,
            "safe.txt",
            b"original",
        )
        .unwrap();
        let attachment_dir = fixture.project_root().join(".omicsops/attachments");
        let stage_dir = fixture
            .project_root()
            .join(&receipt.relative_path)
            .parent()
            .unwrap()
            .to_path_buf();
        let outside = fixture.directory().join("outside");
        fs::create_dir_all(&outside).unwrap();
        fs::remove_dir_all(&stage_dir).unwrap();
        symlink(&outside, &stage_dir).unwrap();
        let error = resolve_composer_attachments(
            &fixture.store,
            fixture.project.id,
            fixture.conversation.id,
            &[receipt.id],
        )
        .await
        .unwrap_err();
        assert!(error.to_ascii_lowercase().contains("symlink") || error.contains("path"));
        assert!(attachment_dir.is_dir());
    }

    impl Fixture {
        fn project_root(&self) -> std::path::PathBuf {
            std::path::PathBuf::from(&self.project.local_root)
        }

        fn directory(&self) -> &std::path::Path {
            self._directory.path()
        }
    }
}
