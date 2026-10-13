//! Host-only bounded capture. W02 owns all source file opens and reads.
use crate::{
    commands::AppState,
    project_file_read::{FileReadError, ValidatedProjectFile},
    workspace_file_content::{
        SmallWorkspaceTextIssue, inspect_small_workspace_text, safe_workspace_public_text,
        sensitive_workspace_filename,
    },
};
use omicsops_core::workspace::Project;
use omicsops_dto::*;
use omicsops_store::{
    FileBaselineManifestEntryV1, FileBaselineManifestV1, SaveFileBaselineRecord, Store, StoreError,
    StoredFileBaseline,
};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, HashMap},
    fs::{self, File, OpenOptions},
    io::{Read, Seek, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
use tauri::{AppHandle, State};
use uuid::Uuid;
const MAX_ITEM: usize = 512 * 1024;
const MAX_CAPTURE: u64 = 4 * 1024 * 1024;
const MAX_PHYSICAL: u64 = 256 * 1024 * 1024;
const NAMESPACE: &str = "workspace-file-baselines";
pub(crate) struct FileBaselineStorage {
    root: PathBuf,
    locks: Mutex<HashMap<Uuid, Arc<tokio::sync::Mutex<()>>>>,
}
impl FileBaselineStorage {
    pub(crate) fn new(app_data_root: PathBuf) -> Self {
        Self {
            root: app_data_root,
            locks: Mutex::new(HashMap::new()),
        }
    }
    pub(crate) fn project_lock(
        &self,
        id: Uuid,
    ) -> Result<Arc<tokio::sync::Mutex<()>>, FileBaselineErrorV1> {
        Ok(self
            .locks
            .lock()
            .map_err(|_| error(FileBaselineErrorCodeV1::StorageFailed))?
            .entry(id)
            .or_default()
            .clone())
    }
    fn project_dir(&self, id: Uuid) -> PathBuf {
        self.root.join(NAMESPACE).join(id.to_string())
    }
    fn blob_path(&self, id: Uuid, hash: &str) -> Result<PathBuf, FileBaselineErrorV1> {
        if !valid_hash(hash) {
            return Err(error(FileBaselineErrorCodeV1::StorageFailed));
        }
        Ok(self
            .project_dir(id)
            .join("sha256")
            .join(&hash[..2])
            .join(hash))
    }

    fn prepare(&self, id: Uuid) -> Result<Vec<File>, FileBaselineErrorV1> {
        let mut guards = lock_directory(&self.root, true)?.unwrap();
        for dir in [
            self.root.join(NAMESPACE),
            self.project_dir(id),
            self.project_dir(id).join("sha256"),
        ] {
            guards.extend(lock_directory(&dir, true)?.unwrap());
        }
        Ok(guards)
    }
    /// Called only after committed DB deletion under the same project mutex.
    pub(crate) fn cleanup_deleted_project(&self, id: Uuid) -> Result<(), FileBaselineErrorV1> {
        self.cleanup_with_directory_probe(id, |p| verified_open(p, true))
    }
    fn cleanup_with_directory_probe(
        &self,
        id: Uuid,
        probe: impl Fn(&Path) -> Result<File, FileBaselineErrorV1>,
    ) -> Result<(), FileBaselineErrorV1> {
        let dir = self.project_dir(id);
        let Some(_guards) = lock_directory_with_probe(&dir, false, probe)? else {
            return Ok(());
        };
        let (blobs, _) = scan_blobs(&dir.join("sha256"))?;
        let (staged, _) = scan_staging(&dir.join("sha256"))?;
        for (_, path, _) in blobs {
            remove_owned_file(&path)?;
        }
        for path in staged {
            remove_owned_file(&path)?;
        }
        Ok(())
    }
}
pub(crate) fn error(code: FileBaselineErrorCodeV1) -> FileBaselineErrorV1 {
    let message = match code {
        FileBaselineErrorCodeV1::InvalidSelection => "file baseline selection is invalid",
        FileBaselineErrorCodeV1::NotFound => "file baseline or project was not found",
        FileBaselineErrorCodeV1::OwnershipMismatch => "file baseline ownership does not match",
        FileBaselineErrorCodeV1::RequestConflict => {
            "file baseline request conflicts with a saved request"
        }
        FileBaselineErrorCodeV1::SourceChanged => {
            "file baseline source could not be fully verified"
        }
        FileBaselineErrorCodeV1::ManifestChanged => "file baseline manifest does not match",
        FileBaselineErrorCodeV1::LimitExceeded => "file baseline storage limit was reached",
        FileBaselineErrorCodeV1::StorageFailed => "file baseline storage failed",
    };
    FileBaselineErrorV1 {
        code,
        message: message.into(),
    }
}
pub(crate) fn storage_error(v: StoreError) -> FileBaselineErrorV1 {
    use FileBaselineErrorCodeV1::*;
    error(match v {
        StoreError::InvalidInput(v) => match v.as_str() {
            "file_baseline_invalid_selection" => InvalidSelection,
            "file_baseline_manifest_changed" => ManifestChanged,
            "file_baseline_ownership_mismatch" => OwnershipMismatch,
            "file_baseline_request_conflict" => RequestConflict,
            "file_baseline_limit_exceeded" => LimitExceeded,
            "file_baseline_not_found" => NotFound,
            _ => StorageFailed,
        },
        _ => StorageFailed,
    })
}
pub(crate) fn sha256(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}
pub(crate) fn local_root_binding_sha256(project: &Project) -> String {
    sha256(project.local_root.as_bytes())
}
fn valid_hash(v: &str) -> bool {
    v.len() == 64
        && v.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn canonical_request(
    mut r: CreateFileBaselineRequestV1,
) -> Result<(CreateFileBaselineRequestV1, String), FileBaselineErrorV1> {
    let invalid = || error(FileBaselineErrorCodeV1::InvalidSelection);
    r.title = r.title.trim().to_owned();
    if r.request_id.is_nil()
        || r.project_id.is_nil()
        || r.branch_conversation_id.is_some_and(|id| id.is_nil())
        || r.title.is_empty()
        || r.title.chars().count() > 80
        || r.title.len() > 320
        || r.title.chars().any(char::is_control)
        || !safe_workspace_public_text(&r.title)
        || !(1..=32).contains(&r.files.len())
    {
        return Err(invalid());
    }
    let mut unique = BTreeMap::new();
    for mut f in r.files {
        if f.backend_id.is_empty()
            || f.backend_id.len() > 128
            || f.backend_id.chars().any(char::is_control)
            || !safe_workspace_public_text(&f.backend_id)
            || f.relative_path.len() > 1024
        {
            return Err(invalid());
        }
        f.relative_path =
            crate::composer_files::normalize_workspace_relative_path(&f.relative_path)
                .map_err(|_| invalid())?;
        if f.relative_path
            .split('/')
            .any(crate::project_file_read::is_reserved_component)
        {
            return Err(invalid());
        }
        if let Some(h) = &mut f.expected_full_sha256 {
            *h = h.to_ascii_lowercase();
            if !valid_hash(h) {
                return Err(invalid());
            }
        }
        let key = (f.backend_id.clone(), f.relative_path.clone());
        if unique.insert(key, f.clone()).is_some_and(|old| old != f) {
            return Err(invalid());
        }
    }
    r.files = unique.into_values().collect();
    let digest = sha256(&serde_json::to_vec(&r).map_err(|_| invalid())?);
    Ok((r, digest))
}
#[tauri::command]
pub async fn workspace_create_file_baseline(
    app: AppHandle,
    state: State<'_, AppState>,
    request: CreateFileBaselineRequestV1,
) -> Result<FileBaselineV1, FileBaselineErrorV1> {
    let _ = app;
    capture_file_baseline(&state.repository, &state.file_baseline_storage, request).await
}
#[tauri::command]
pub async fn workspace_read_baseline_text(
    app: AppHandle,
    state: State<'_, AppState>,
    request: ReadBaselineTextRequestV1,
) -> Result<BaselineTextSnapshotV1, FileBaselineErrorV1> {
    let _ = app;
    read_baseline_text(&state.repository, &state.file_baseline_storage, request).await
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum CaptureStage {
    AfterSourceOpen,
    BeforeTemp,
    BeforeSync,
    BeforePersist,
    BeforeCommit,
}
pub(crate) async fn capture_file_baseline(
    store: &Store,
    storage: &FileBaselineStorage,
    request: CreateFileBaselineRequestV1,
) -> Result<FileBaselineV1, FileBaselineErrorV1> {
    capture_with_checkpoint(store, storage, request, |_| Ok(())).await
}
async fn capture_with_checkpoint(
    store: &Store,
    storage: &FileBaselineStorage,
    request: CreateFileBaselineRequestV1,
    checkpoint: impl Fn(CaptureStage) -> Result<(), FileBaselineErrorV1>,
) -> Result<FileBaselineV1, FileBaselineErrorV1> {
    capture_with_source_open(
        store,
        storage,
        request,
        checkpoint,
        ValidatedProjectFile::open,
    )
    .await
}
async fn capture_with_source_open(
    store: &Store,
    storage: &FileBaselineStorage,
    request: CreateFileBaselineRequestV1,
    checkpoint: impl Fn(CaptureStage) -> Result<(), FileBaselineErrorV1>,
    open_source: impl Fn(&Project, &str) -> Result<ValidatedProjectFile, FileReadError>,
) -> Result<FileBaselineV1, FileBaselineErrorV1> {
    let (request, digest) = canonical_request(request)?;
    let lock = storage.project_lock(request.project_id)?;
    let _lock = lock.lock().await;
    if let Some(saved) = store
        .workspace_find_file_baseline_request(request.request_id, &digest)
        .await
        .map_err(storage_error)?
    {
        return Ok(saved);
    }
    let project = store
        .get_project(request.project_id)
        .await
        .map_err(storage_error)?
        .ok_or_else(|| error(FileBaselineErrorCodeV1::NotFound))?;
    let connections = store.list_connections().await.map_err(storage_error)?;
    for f in &request.files {
        crate::composer_files::validate_backend_binding(&project, &connections, &f.backend_id)
            .map_err(|_| error(FileBaselineErrorCodeV1::OwnershipMismatch))?;
    }
    let referenced = referenced_blobs(store, project.id).await?;
    let _guards = storage.prepare(project.id)?;
    let dir = storage.project_dir(project.id);
    let (blobs, _) = scan_blobs(&dir.join("sha256"))?;
    let (staged, _) = scan_staging(&dir.join("sha256"))?;
    // Validate history before granting permission to remove any orphan.
    for (hash, (size, path)) in &referenced {
        read_blob(storage, project.id, hash, *size, path)?;
    }
    for (hash, path, _) in blobs {
        if !referenced.contains_key(&hash) {
            remove_owned_file(&path)?;
        }
    }
    for path in staged {
        remove_owned_file(&path)?;
    }
    let (_, physical) = scan_blobs(&dir.join("sha256"))?;
    if physical > MAX_PHYSICAL {
        return Err(error(FileBaselineErrorCodeV1::LimitExceeded));
    }
    let now = chrono::Utc::now().to_rfc3339();
    let mut entries = Vec::new();
    let mut bodies = BTreeMap::new();
    let mut captured = 0;
    let mut read_bytes = 0;
    for f in &request.files {
        let path = &f.relative_path;
        let mut entry = FileBaselineEntryV1 {
            backend_id: f.backend_id.clone(),
            relative_path: Some(path.clone()),
            content_mode: WorkspaceContentModeV1::ReferenceOnly,
            size_bytes: None,
            hash: None,
            omission: None,
            captured_at: now.clone(),
        };
        if !safe_workspace_public_text(path) {
            entry.relative_path = None;
            entry.omission = Some(BaselineOmissionCodeV1::SensitiveContent);
        } else if sensitive_workspace_filename(path) {
            entry.omission = Some(BaselineOmissionCodeV1::SensitiveContent);
        } else if f.backend_id != "local" {
            let (size, hash) = store
                .workspace_file_recorded_reference(project.id, &f.backend_id, path)
                .await
                .map_err(storage_error)?;
            entry.size_bytes = size;
            entry.hash = hash.map(|sha256| FileReferenceHashV1 {
                sha256,
                scope: FileReferenceHashScopeV1::RecordedFile,
            });
            entry.omission = Some(BaselineOmissionCodeV1::RemoteReference);
        } else {
            match open_source(&project, path) {
                Err(FileReadError::SourceChanged) => {
                    return Err(error(FileBaselineErrorCodeV1::SourceChanged));
                }
                Err(issue) => entry.omission = Some(file_omission(issue)),
                Ok(mut file) => {
                    checkpoint(CaptureStage::AfterSourceOpen)?;
                    let size = file.metadata().size_bytes;
                    entry.size_bytes = Some(size);
                    if size > MAX_ITEM as u64 {
                        entry.omission = Some(BaselineOmissionCodeV1::TooLarge);
                    } else if inspect_small_workspace_text(path, b"")
                        == Err(SmallWorkspaceTextIssue::UnsupportedContent)
                    {
                        entry.omission = Some(BaselineOmissionCodeV1::UnsupportedContent);
                    } else if size > MAX_CAPTURE - read_bytes {
                        entry.omission = Some(BaselineOmissionCodeV1::AggregateBudget);
                    } else {
                        match file.read_complete(MAX_ITEM) {
                            Err(FileReadError::SourceChanged) => {
                                return Err(error(FileBaselineErrorCodeV1::SourceChanged));
                            }
                            Err(issue) => entry.omission = Some(file_omission(issue)),
                            Ok(read) => {
                                read_bytes += read.read_bytes;
                                if !read.eof
                                    || read.hash_scope
                                        != crate::project_file_read::FileHashScope::Full
                                {
                                    return Err(error(FileBaselineErrorCodeV1::SourceChanged));
                                }
                                entry.hash = Some(FileReferenceHashV1 {
                                    sha256: read.sha256.clone(),
                                    scope: FileReferenceHashScopeV1::FullFile,
                                });
                                match inspect_small_workspace_text(path, &read.bytes) {
                                    Ok(_) => {
                                        entry.content_mode = WorkspaceContentModeV1::TextSnapshot;
                                        captured += read.size_bytes;
                                        bodies.entry(read.sha256).or_insert(read.bytes);
                                    }
                                    Err(issue) => entry.omission = Some(text_omission(issue)),
                                }
                            }
                        }
                    }
                }
            }
        }
        if f.expected_full_sha256.as_ref().is_some_and(|expected| {
            !entry.hash.as_ref().is_some_and(|h| {
                h.scope == FileReferenceHashScopeV1::FullFile && &h.sha256 == expected
            })
        }) {
            return Err(error(FileBaselineErrorCodeV1::SourceChanged));
        }
        let blob_sha256 = if entry.content_mode == WorkspaceContentModeV1::TextSnapshot {
            entry.hash.as_ref().map(|h| h.sha256.clone())
        } else {
            None
        };
        entries.push(FileBaselineManifestEntryV1 { entry, blob_sha256 });
    }
    let current = store
        .get_project(project.id)
        .await
        .map_err(storage_error)?
        .ok_or_else(|| error(FileBaselineErrorCodeV1::NotFound))?;
    if current.local_root != project.local_root
        || current.connection_id != project.connection_id
        || current.remote_root != project.remote_root
    {
        return Err(error(FileBaselineErrorCodeV1::SourceChanged));
    }
    for (hash, bytes) in &bodies {
        let destination = storage.blob_path(project.id, hash)?;
        let path = entries
            .iter()
            .find(|e| e.blob_sha256.as_ref() == Some(hash))
            .and_then(|e| e.entry.relative_path.as_deref())
            .unwrap_or("");
        if destination
            .try_exists()
            .map_err(|_| error(FileBaselineErrorCodeV1::StorageFailed))?
        {
            read_blob(storage, project.id, hash, bytes.len() as u64, path)?;
            continue;
        }
        let (_, physical) = scan_blobs(&dir.join("sha256"))?;
        if physical > MAX_PHYSICAL || bytes.len() as u64 > MAX_PHYSICAL - physical {
            return Err(error(FileBaselineErrorCodeV1::LimitExceeded));
        }
        let _prefix = lock_directory(destination.parent().unwrap(), true)?.unwrap();
        checkpoint(CaptureStage::BeforeTemp)?;
        let (_, staging) = scan_staging(&dir.join("sha256"))?;
        if bytes.len() as u64 > MAX_CAPTURE - staging {
            return Err(error(FileBaselineErrorCodeV1::LimitExceeded));
        }
        let mut temp = tempfile::Builder::new()
            .prefix("baseline-")
            .suffix(".tmp")
            .rand_bytes(16)
            .tempfile_in(destination.parent().unwrap())
            .map_err(|_| error(FileBaselineErrorCodeV1::StorageFailed))?;
        temp.write_all(bytes)
            .map_err(|_| error(FileBaselineErrorCodeV1::StorageFailed))?;
        checkpoint(CaptureStage::BeforeSync)?;
        temp.as_file()
            .sync_all()
            .map_err(|_| error(FileBaselineErrorCodeV1::StorageFailed))?;
        temp.as_file_mut()
            .rewind()
            .map_err(|_| error(FileBaselineErrorCodeV1::StorageFailed))?;
        let mut check = Vec::new();
        temp.as_file_mut()
            .take(MAX_ITEM as u64 + 1)
            .read_to_end(&mut check)
            .map_err(|_| error(FileBaselineErrorCodeV1::StorageFailed))?;
        if check != *bytes || sha256(&check) != *hash {
            return Err(error(FileBaselineErrorCodeV1::StorageFailed));
        }
        checkpoint(CaptureStage::BeforePersist)?;
        let (_, physical) = scan_blobs(&dir.join("sha256"))?;
        if physical > MAX_PHYSICAL || bytes.len() as u64 > MAX_PHYSICAL - physical {
            return Err(error(FileBaselineErrorCodeV1::LimitExceeded));
        }
        scan_staging(&dir.join("sha256"))?;
        temp.persist_noclobber(destination)
            .map_err(|_| error(FileBaselineErrorCodeV1::StorageFailed))?;
    }
    let manifest = FileBaselineManifestV1 {
        schema_version: 1,
        id: Uuid::new_v4(),
        project_id: project.id,
        branch_conversation_id: request.branch_conversation_id,
        title: request.title,
        entries,
        captured_text_bytes: captured,
        created_at: now,
        local_root_binding_sha256: local_root_binding_sha256(&project),
    };
    let json = serde_json::to_string(&manifest)
        .map_err(|_| error(FileBaselineErrorCodeV1::StorageFailed))?;
    let baseline = FileBaselineV1 {
        schema_version: 1,
        id: manifest.id,
        project_id: project.id,
        branch_conversation_id: manifest.branch_conversation_id,
        title: manifest.title.clone(),
        entries: manifest.entries.iter().map(|e| e.entry.clone()).collect(),
        manifest_sha256: sha256(json.as_bytes()),
        captured_text_bytes: captured,
        created_at: manifest.created_at.clone(),
    };
    checkpoint(CaptureStage::BeforeCommit)?;
    store
        .workspace_save_file_baseline(SaveFileBaselineRecord {
            request_id: request.request_id,
            request_sha256: digest,
            baseline,
            internal_manifest_json: json,
        })
        .await
        .map_err(storage_error)
}
fn text_omission(i: SmallWorkspaceTextIssue) -> BaselineOmissionCodeV1 {
    match i {
        SmallWorkspaceTextIssue::TooLarge => BaselineOmissionCodeV1::TooLarge,
        SmallWorkspaceTextIssue::UnsupportedContent => BaselineOmissionCodeV1::UnsupportedContent,
        SmallWorkspaceTextIssue::SensitiveContent => BaselineOmissionCodeV1::SensitiveContent,
    }
}
pub(crate) fn file_omission(i: FileReadError) -> BaselineOmissionCodeV1 {
    match i {
        FileReadError::TooLarge => BaselineOmissionCodeV1::TooLarge,
        FileReadError::UnsafeFile | FileReadError::InvalidPath | FileReadError::NotRegular => {
            BaselineOmissionCodeV1::UnsafeFile
        }
        FileReadError::Unavailable => BaselineOmissionCodeV1::Unreadable,
        FileReadError::SourceChanged => BaselineOmissionCodeV1::Unreadable,
    }
}
async fn referenced_blobs(
    store: &Store,
    id: Uuid,
) -> Result<BTreeMap<String, (u64, String)>, FileBaselineErrorV1> {
    let page = store
        .workspace_list_file_baselines(&ListFileBaselinesRequestV1 {
            project_id: id,
            branch_conversation_id: None,
            offset: 0,
            limit: 64,
        })
        .await
        .map_err(storage_error)?;
    if page.next_offset.is_some() {
        return Err(error(FileBaselineErrorCodeV1::LimitExceeded));
    }
    let mut result = BTreeMap::new();
    for b in page.items {
        let stored = store
            .workspace_get_file_baseline(id, b.id)
            .await
            .map_err(storage_error)?
            .ok_or_else(|| error(FileBaselineErrorCodeV1::StorageFailed))?;
        for (entry, hash) in stored
            .baseline
            .entries
            .iter()
            .zip(stored.blob_sha256_by_entry)
        {
            if let Some(hash) = hash {
                let size = entry
                    .size_bytes
                    .ok_or_else(|| error(FileBaselineErrorCodeV1::StorageFailed))?;
                if result.get(&hash).is_some_and(|(old, _)| *old != size) {
                    return Err(error(FileBaselineErrorCodeV1::StorageFailed));
                }
                result.insert(
                    hash,
                    (
                        size,
                        entry
                            .relative_path
                            .clone()
                            .ok_or_else(|| error(FileBaselineErrorCodeV1::StorageFailed))?,
                    ),
                );
            }
        }
    }
    Ok(result)
}
pub(crate) async fn selected_baseline(
    store: &Store,
    project_id: Uuid,
    id: Uuid,
    expected: &str,
) -> Result<StoredFileBaseline, FileBaselineErrorV1> {
    if project_id.is_nil() || id.is_nil() || !valid_hash(expected) {
        return Err(error(FileBaselineErrorCodeV1::InvalidSelection));
    }
    let stored = store
        .workspace_get_file_baseline(project_id, id)
        .await
        .map_err(storage_error)?
        .ok_or_else(|| error(FileBaselineErrorCodeV1::NotFound))?;
    if stored.baseline.manifest_sha256 != expected {
        return Err(error(FileBaselineErrorCodeV1::ManifestChanged));
    }
    Ok(stored)
}
pub(crate) async fn read_baseline_text(
    store: &Store,
    storage: &FileBaselineStorage,
    q: ReadBaselineTextRequestV1,
) -> Result<BaselineTextSnapshotV1, FileBaselineErrorV1> {
    let lock = storage.project_lock(q.project_id)?;
    let _lock = lock.lock().await;
    let saved = selected_baseline(
        store,
        q.project_id,
        q.baseline_id,
        &q.expected_manifest_sha256,
    )
    .await?;
    let entry = saved
        .baseline
        .entries
        .get(q.entry_index as usize)
        .ok_or_else(|| error(FileBaselineErrorCodeV1::InvalidSelection))?;
    if entry.content_mode != WorkspaceContentModeV1::TextSnapshot {
        return Err(error(FileBaselineErrorCodeV1::InvalidSelection));
    }
    let path = entry
        .relative_path
        .as_deref()
        .ok_or_else(|| error(FileBaselineErrorCodeV1::StorageFailed))?;
    let hash = saved
        .blob_sha256_by_entry
        .get(q.entry_index as usize)
        .and_then(Option::as_deref)
        .ok_or_else(|| error(FileBaselineErrorCodeV1::StorageFailed))?;
    let bytes = read_blob(
        storage,
        q.project_id,
        hash,
        entry
            .size_bytes
            .ok_or_else(|| error(FileBaselineErrorCodeV1::StorageFailed))?,
        path,
    )?;
    Ok(BaselineTextSnapshotV1 {
        project_id: q.project_id,
        baseline_id: q.baseline_id,
        entry_index: q.entry_index,
        relative_path: path.into(),
        full_sha256: hash.into(),
        text: String::from_utf8(bytes)
            .map_err(|_| error(FileBaselineErrorCodeV1::StorageFailed))?,
    })
}
pub(crate) fn read_blob(
    storage: &FileBaselineStorage,
    id: Uuid,
    hash: &str,
    size: u64,
    relative: &str,
) -> Result<Vec<u8>, FileBaselineErrorV1> {
    if size > MAX_ITEM as u64 {
        return Err(error(FileBaselineErrorCodeV1::StorageFailed));
    }
    let path = storage.blob_path(id, hash)?;
    let _guards = lock_directory(path.parent().unwrap(), false)?
        .ok_or_else(|| error(FileBaselineErrorCodeV1::StorageFailed))?;
    let mut file = verified_open(&path, false)?;
    if file
        .metadata()
        .map_err(|_| error(FileBaselineErrorCodeV1::StorageFailed))?
        .len()
        != size
    {
        return Err(error(FileBaselineErrorCodeV1::StorageFailed));
    }
    let mut bytes = Vec::new();
    (&mut file)
        .take(MAX_ITEM as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| error(FileBaselineErrorCodeV1::StorageFailed))?;
    if bytes.len() as u64 != size
        || sha256(&bytes) != hash
        || inspect_small_workspace_text(relative, &bytes).is_err()
    {
        return Err(error(FileBaselineErrorCodeV1::StorageFailed));
    }
    Ok(bytes)
}
fn safe_metadata(path: &Path) -> Result<fs::Metadata, FileBaselineErrorV1> {
    let m =
        fs::symlink_metadata(path).map_err(|_| error(FileBaselineErrorCodeV1::StorageFailed))?;
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if m.file_attributes() & (0x400 | 0x40) != 0 {
            return Err(error(FileBaselineErrorCodeV1::StorageFailed));
        }
    }
    if m.file_type().is_symlink() {
        return Err(error(FileBaselineErrorCodeV1::StorageFailed));
    }
    Ok(m)
}
fn verified_open(path: &Path, directory: bool) -> Result<File, FileBaselineErrorV1> {
    let m = safe_metadata(path)?;
    if if directory { !m.is_dir() } else { !m.is_file() } {
        return Err(error(FileBaselineErrorCodeV1::StorageFailed));
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::*;
        options
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
            .custom_flags(
                FILE_FLAG_OPEN_REPARSE_POINT
                    | if directory {
                        FILE_FLAG_BACKUP_SEMANTICS
                    } else {
                        0
                    },
            );
    }
    let file = options
        .open(path)
        .map_err(|_| error(FileBaselineErrorCodeV1::StorageFailed))?;
    #[cfg(windows)]
    {
        use std::os::windows::{ffi::OsStringExt, io::AsRawHandle};
        use windows_sys::Win32::Storage::FileSystem::*;
        let mut info = BY_HANDLE_FILE_INFORMATION::default();
        let mut buffer = vec![0u16; 32768];
        if unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) } == 0
            || info.dwFileAttributes & (FILE_ATTRIBUTE_REPARSE_POINT | FILE_ATTRIBUTE_DEVICE) != 0
        {
            return Err(error(FileBaselineErrorCodeV1::StorageFailed));
        }
        let count = unsafe {
            GetFinalPathNameByHandleW(
                file.as_raw_handle(),
                buffer.as_mut_ptr(),
                buffer.len() as u32,
                0,
            )
        } as usize;
        if count == 0
            || count >= buffer.len()
            || PathBuf::from(std::ffi::OsString::from_wide(&buffer[..count]))
                != fs::canonicalize(path)
                    .map_err(|_| error(FileBaselineErrorCodeV1::StorageFailed))?
        {
            return Err(error(FileBaselineErrorCodeV1::StorageFailed));
        }
    }
    Ok(file)
}
// Holding Windows directory handles without FILE_SHARE_DELETE prevents namespace
// replacement while descendants are inspected, staged or removed.
fn lock_directory(path: &Path, create: bool) -> Result<Option<Vec<File>>, FileBaselineErrorV1> {
    lock_directory_with_probe(path, create, |p| verified_open(p, true))
}
fn lock_directory_with_probe(
    path: &Path,
    create: bool,
    probe: impl Fn(&Path) -> Result<File, FileBaselineErrorV1>,
) -> Result<Option<Vec<File>>, FileBaselineErrorV1> {
    if !path.is_absolute() {
        return Err(error(FileBaselineErrorCodeV1::StorageFailed));
    }
    let mut ancestors: Vec<_> = path
        .ancestors()
        .filter(|p| !p.as_os_str().is_empty())
        .collect();
    ancestors.reverse();
    let mut guards = Vec::new();
    for dir in ancestors {
        match fs::symlink_metadata(dir) {
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                if !create {
                    return Ok(None);
                }
                fs::create_dir(dir).map_err(|_| error(FileBaselineErrorCodeV1::StorageFailed))?;
            }
            Err(_) => return Err(error(FileBaselineErrorCodeV1::StorageFailed)),
        }
        guards.push(probe(dir)?);
    }
    Ok(Some(guards))
}

struct NamespaceScan {
    blobs: Vec<(String, PathBuf, u64)>,
    staging: Vec<PathBuf>,
    persisted_bytes: u64,
    staging_bytes: u64,
}
fn scan_blobs(root: &Path) -> Result<(Vec<(String, PathBuf, u64)>, u64), FileBaselineErrorV1> {
    let scan = scan_namespace(root)?;
    Ok((scan.blobs, scan.persisted_bytes))
}
fn scan_staging(root: &Path) -> Result<(Vec<PathBuf>, u64), FileBaselineErrorV1> {
    let scan = scan_namespace(root)?;
    Ok((scan.staging, scan.staging_bytes))
}
fn is_staging_name(name: &str) -> bool {
    name.strip_prefix("baseline-")
        .and_then(|s| s.strip_suffix(".tmp"))
        .is_some_and(|s| s.len() == 16 && s.bytes().all(|b| b.is_ascii_alphanumeric()))
}
/// Both physical accounting domains are observed in one bounded namespace scan.
/// Unknown names, unsafe children or unreadable ancestry deny cleanup permission.
fn scan_namespace(root: &Path) -> Result<NamespaceScan, FileBaselineErrorV1> {
    let mut result = NamespaceScan {
        blobs: Vec::new(),
        staging: Vec::new(),
        persisted_bytes: 0,
        staging_bytes: 0,
    };
    let Some(_guards) = lock_directory(root, false)? else {
        return Ok(result);
    };
    let mut prefixes = 0;
    for prefix in fs::read_dir(root).map_err(|_| error(FileBaselineErrorCodeV1::StorageFailed))? {
        prefixes += 1;
        if prefixes > 256 {
            return Err(error(FileBaselineErrorCodeV1::StorageFailed));
        }
        let prefix = prefix.map_err(|_| error(FileBaselineErrorCodeV1::StorageFailed))?;
        let name = prefix
            .file_name()
            .to_str()
            .ok_or_else(|| error(FileBaselineErrorCodeV1::StorageFailed))?
            .to_owned();
        if name.len() != 2
            || !name
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(error(FileBaselineErrorCodeV1::StorageFailed));
        }
        let _prefix = lock_directory(&prefix.path(), false)?
            .ok_or_else(|| error(FileBaselineErrorCodeV1::StorageFailed))?;
        for entry in fs::read_dir(prefix.path())
            .map_err(|_| error(FileBaselineErrorCodeV1::StorageFailed))?
        {
            if result.blobs.len() + result.staging.len() >= 4096 {
                return Err(error(FileBaselineErrorCodeV1::StorageFailed));
            }
            let entry = entry.map_err(|_| error(FileBaselineErrorCodeV1::StorageFailed))?;
            let filename = entry
                .file_name()
                .to_str()
                .ok_or_else(|| error(FileBaselineErrorCodeV1::StorageFailed))?
                .to_owned();
            let metadata = safe_metadata(&entry.path())?;
            if !metadata.is_file() {
                return Err(error(FileBaselineErrorCodeV1::StorageFailed));
            }
            if valid_hash(&filename) && filename.starts_with(&name) {
                result.persisted_bytes = result
                    .persisted_bytes
                    .checked_add(metadata.len())
                    .ok_or_else(|| error(FileBaselineErrorCodeV1::LimitExceeded))?;
                result.blobs.push((filename, entry.path(), metadata.len()));
            } else if is_staging_name(&filename) {
                result.staging_bytes = result
                    .staging_bytes
                    .checked_add(metadata.len())
                    .ok_or_else(|| error(FileBaselineErrorCodeV1::LimitExceeded))?;
                if result.staging_bytes > MAX_CAPTURE || result.staging.len() >= 32 {
                    return Err(error(FileBaselineErrorCodeV1::LimitExceeded));
                }
                result.staging.push(entry.path());
            } else {
                return Err(error(FileBaselineErrorCodeV1::StorageFailed));
            }
        }
    }
    Ok(result)
}
fn remove_owned_file(path: &Path) -> Result<(), FileBaselineErrorV1> {
    let _guards = lock_directory(path.parent().unwrap(), false)?
        .ok_or_else(|| error(FileBaselineErrorCodeV1::StorageFailed))?;
    if !safe_metadata(path)?.is_file() {
        return Err(error(FileBaselineErrorCodeV1::StorageFailed));
    }
    fs::remove_file(path).map_err(|_| error(FileBaselineErrorCodeV1::StorageFailed))
}

#[cfg(test)]
mod tests {
    use super::*;
    use omicsops_core::workspace::{Project, ProjectTemplate};
    use uuid::Uuid;
    async fn fixture() -> (
        tempfile::TempDir,
        Store,
        Project,
        FileBaselineStorage,
        CreateFileBaselineRequestV1,
    ) {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir(d.path().join("project")).unwrap();
        std::fs::write(d.path().join("project/a.txt"), b"hello\n").unwrap();
        let store = Store::open(d.path().join("test.db")).await.unwrap();
        let p = Project::new(
            Uuid::new_v4(),
            "fixture",
            d.path().join("project").to_str().unwrap(),
            ProjectTemplate::Blank,
            chrono::Utc::now(),
        );
        store.save_project(&p).await.unwrap();
        let storage = FileBaselineStorage::new(d.path().join("app"));
        let r = CreateFileBaselineRequestV1 {
            request_id: Uuid::new_v4(),
            project_id: p.id,
            branch_conversation_id: None,
            title: "baseline".into(),
            files: vec![ProjectFileSelectionV1 {
                backend_id: "local".into(),
                relative_path: "a.txt".into(),
                expected_full_sha256: None,
            }],
        };
        (d, store, p, storage, r)
    }
    #[tokio::test]
    async fn capture_safe_text_and_replay_before_source_read() {
        let (d, s, p, b, r) = fixture().await;
        let first = capture_file_baseline(&s, &b, r.clone()).await.unwrap();
        assert_eq!(
            first.entries[0].content_mode,
            WorkspaceContentModeV1::TextSnapshot
        );
        assert_eq!(first.captured_text_bytes, 6);
        std::fs::remove_file(std::path::Path::new(&p.local_root).join("a.txt")).unwrap();
        assert_eq!(capture_file_baseline(&s, &b, r).await.unwrap(), first);
        assert!(d.path().join("project").exists());
    }
    #[tokio::test]
    async fn capture_invalid_selection_and_expected_hash() {
        let (_d, s, _p, b, mut r) = fixture().await;
        r.files.clear();
        assert_eq!(
            capture_file_baseline(&s, &b, r.clone())
                .await
                .unwrap_err()
                .code,
            FileBaselineErrorCodeV1::InvalidSelection
        );
        r.files = vec![ProjectFileSelectionV1 {
            backend_id: "local".into(),
            relative_path: "a.txt".into(),
            expected_full_sha256: Some("a".repeat(64)),
        }];
        assert_eq!(
            capture_file_baseline(&s, &b, r).await.unwrap_err().code,
            FileBaselineErrorCodeV1::SourceChanged
        );
    }
    #[tokio::test]
    async fn saved_text_survives_root_change_and_validates_hash() {
        let (_d, s, mut p, b, r) = fixture().await;
        let first = capture_file_baseline(&s, &b, r).await.unwrap();
        p.local_root = "missing-root".into();
        s.save_project(&p).await.unwrap();
        let q = ReadBaselineTextRequestV1 {
            project_id: p.id,
            baseline_id: first.id,
            expected_manifest_sha256: first.manifest_sha256,
            entry_index: 0,
        };
        let text = read_baseline_text(&s, &b, q.clone()).await.unwrap();
        assert_eq!(text.text, "hello\n");
        assert_eq!(
            text.full_sha256,
            "5891b5b522d5df086d0ff0b110fbd9d21bb4fc7163af34d08286a2e846f6be03"
        );
        let mut invalid = q;
        invalid.entry_index = 1;
        assert_eq!(
            read_baseline_text(&s, &b, invalid).await.unwrap_err().code,
            FileBaselineErrorCodeV1::InvalidSelection
        );
    }
    #[tokio::test]
    async fn capture_sensitive_body_and_path_never_persisted() {
        let (d, s, p, b, mut r) = fixture().await;
        let secret = b"password='FAKE_SENTINEL_9342'";
        std::fs::write(std::path::Path::new(&p.local_root).join("a.txt"), secret).unwrap();
        let first = capture_file_baseline(&s, &b, r.clone()).await.unwrap();
        assert_eq!(
            first.entries[0].content_mode,
            WorkspaceContentModeV1::ReferenceOnly
        );
        assert_eq!(
            first.entries[0].omission,
            Some(BaselineOmissionCodeV1::SensitiveContent)
        );
        assert!(
            scan_blobs(&b.project_dir(p.id).join("sha256"))
                .unwrap()
                .0
                .is_empty()
        );
        assert_eq!(
            scan_staging(&b.project_dir(p.id).join("sha256")).unwrap().1,
            0
        );
        let sensitive = "password=FAKE_SENTINEL_9342.txt";
        std::fs::write(std::path::Path::new(&p.local_root).join(sensitive), b"safe").unwrap();
        r.request_id = Uuid::new_v4();
        r.files[0].relative_path = sensitive.into();
        let second = capture_file_baseline(&s, &b, r).await.unwrap();
        assert_eq!(second.entries[0].relative_path, None);
        let manifests: Vec<String> =
            sqlx::query_scalar("SELECT manifest_json FROM exploration_baselines_v4")
                .fetch_all(s.pool())
                .await
                .unwrap();
        assert!(manifests.iter().all(|v| !v.contains("FAKE_SENTINEL_9342")));
        assert!(
            std::fs::read(d.path().join("test.db"))
                .unwrap()
                .windows(18)
                .all(|w| w != b"FAKE_SENTINEL_9342")
        );
    }
    #[tokio::test]
    async fn capture_item_selection_and_aggregate_budgets() {
        let (_d, s, p, b, mut r) = fixture().await;
        let root = std::path::Path::new(&p.local_root);
        std::fs::write(root.join("large.txt"), vec![b'x'; 512 * 1024 + 1]).unwrap();
        r.files[0].relative_path = "large.txt".into();
        let large = capture_file_baseline(&s, &b, r.clone()).await.unwrap();
        assert_eq!(
            large.entries[0].omission,
            Some(BaselineOmissionCodeV1::TooLarge)
        );
        r.request_id = Uuid::new_v4();
        r.files.clear();
        for i in 0..32 {
            let path = format!("{i:02}.txt");
            std::fs::write(root.join(&path), vec![b'x'; 512 * 1024]).unwrap();
            r.files.push(ProjectFileSelectionV1 {
                backend_id: "local".into(),
                relative_path: path,
                expected_full_sha256: None,
            });
        }
        let max = capture_file_baseline(&s, &b, r.clone()).await.unwrap();
        assert_eq!(max.entries.len(), 32);
        assert_eq!(max.captured_text_bytes, 4 * 1024 * 1024);
        assert_eq!(
            max.entries
                .iter()
                .filter(|e| e.content_mode == WorkspaceContentModeV1::TextSnapshot)
                .count(),
            8
        );
        assert!(
            max.entries[8..]
                .iter()
                .all(|e| e.omission == Some(BaselineOmissionCodeV1::AggregateBudget))
        );
        r.request_id = Uuid::new_v4();
        r.files.push(r.files[0].clone());
        assert_eq!(
            capture_file_baseline(&s, &b, r).await.unwrap_err().code,
            FileBaselineErrorCodeV1::InvalidSelection
        );
    }
    #[tokio::test]
    async fn capture_canonical_replay_dedup_and_conflict() {
        let (_d, s, p, b, mut r) = fixture().await;
        r.files[0].relative_path = "./a.txt".into();
        r.files.push(ProjectFileSelectionV1 {
            backend_id: "local".into(),
            relative_path: "a.txt".into(),
            expected_full_sha256: None,
        });
        let first = capture_file_baseline(&s, &b, r.clone()).await.unwrap();
        assert_eq!(first.entries.len(), 1);
        std::fs::remove_file(std::path::Path::new(&p.local_root).join("a.txt")).unwrap();
        r.files.reverse();
        assert_eq!(
            capture_file_baseline(&s, &b, r.clone()).await.unwrap(),
            first
        );
        r.title = "other".into();
        assert_eq!(
            capture_file_baseline(&s, &b, r).await.unwrap_err().code,
            FileBaselineErrorCodeV1::RequestConflict
        );
    }
    #[tokio::test]
    async fn capture_expected_hash_unverifiable_missing_large_and_binary() {
        let (_d, s, p, b, mut r) = fixture().await;
        for (name, bytes) in [
            ("missing.txt", None),
            ("large.txt", Some(vec![b'x'; 512 * 1024 + 1])),
        ] {
            if let Some(bytes) = bytes {
                std::fs::write(std::path::Path::new(&p.local_root).join(name), bytes).unwrap();
            }
            r.request_id = Uuid::new_v4();
            r.files[0].relative_path = name.into();
            r.files[0].expected_full_sha256 = Some("a".repeat(64));
            assert_eq!(
                capture_file_baseline(&s, &b, r.clone())
                    .await
                    .unwrap_err()
                    .code,
                FileBaselineErrorCodeV1::SourceChanged
            );
        }
        std::fs::write(
            std::path::Path::new(&p.local_root).join("binary.txt"),
            b"\xff",
        )
        .unwrap();
        r.request_id = Uuid::new_v4();
        r.files[0].relative_path = "binary.txt".into();
        r.files[0].expected_full_sha256 = None;
        let binary = capture_file_baseline(&s, &b, r).await.unwrap();
        assert_eq!(
            binary.entries[0].omission,
            Some(BaselineOmissionCodeV1::UnsupportedContent)
        );
    }
    #[tokio::test]
    async fn saved_text_missing_and_bad_blob_fail_closed() {
        let (_d, s, p, b, r) = fixture().await;
        let first = capture_file_baseline(&s, &b, r).await.unwrap();
        let q = ReadBaselineTextRequestV1 {
            project_id: p.id,
            baseline_id: first.id,
            expected_manifest_sha256: first.manifest_sha256,
            entry_index: 0,
        };
        let path = b
            .root
            .join("workspace-file-baselines")
            .join(p.id.to_string())
            .join("sha256")
            .join("58")
            .join("5891b5b522d5df086d0ff0b110fbd9d21bb4fc7163af34d08286a2e846f6be03");
        std::fs::write(&path, b"wrong\n").unwrap();
        assert_eq!(
            read_baseline_text(&s, &b, q.clone())
                .await
                .unwrap_err()
                .code,
            FileBaselineErrorCodeV1::StorageFailed
        );
        std::fs::remove_file(path).unwrap();
        assert_eq!(
            read_baseline_text(&s, &b, q).await.unwrap_err().code,
            FileBaselineErrorCodeV1::StorageFailed
        );
    }
    #[tokio::test]
    async fn saved_text_reference_manifest_owner_and_content_recheck() {
        let (_d, s, p, b, mut r) = fixture().await;
        std::fs::write(
            std::path::Path::new(&p.local_root).join("a.txt"),
            b"password=FAKE_SENTINEL",
        )
        .unwrap();
        let first = capture_file_baseline(&s, &b, r.clone()).await.unwrap();
        let mut q = ReadBaselineTextRequestV1 {
            project_id: p.id,
            baseline_id: first.id,
            expected_manifest_sha256: first.manifest_sha256,
            entry_index: 0,
        };
        assert_eq!(
            read_baseline_text(&s, &b, q.clone())
                .await
                .unwrap_err()
                .code,
            FileBaselineErrorCodeV1::InvalidSelection
        );
        q.expected_manifest_sha256 = "a".repeat(64);
        assert_eq!(
            read_baseline_text(&s, &b, q.clone())
                .await
                .unwrap_err()
                .code,
            FileBaselineErrorCodeV1::ManifestChanged
        );
        let foreign = Project::new(
            Uuid::new_v4(),
            "foreign",
            p.local_root.clone(),
            ProjectTemplate::Blank,
            chrono::Utc::now(),
        );
        s.save_project(&foreign).await.unwrap();
        q.project_id = foreign.id;
        assert_eq!(
            read_baseline_text(&s, &b, q).await.unwrap_err().code,
            FileBaselineErrorCodeV1::OwnershipMismatch
        );
        r.request_id = Uuid::new_v4();
    }

    #[tokio::test]
    async fn capture_temp_sync_persist_and_db_faults_leave_no_bad_commit() {
        for stage in [
            CaptureStage::BeforeTemp,
            CaptureStage::BeforeSync,
            CaptureStage::BeforePersist,
            CaptureStage::BeforeCommit,
        ] {
            let (_d, s, p, b, mut r) = fixture().await;
            capture_file_baseline(&s, &b, r.clone()).await.unwrap();
            r.request_id = Uuid::new_v4();
            std::fs::write(
                std::path::Path::new(&p.local_root).join("a.txt"),
                b"changed\n",
            )
            .unwrap();
            let result = capture_with_checkpoint(&s, &b, r, |actual| {
                if actual == stage {
                    Err(FileBaselineErrorV1 {
                        code: FileBaselineErrorCodeV1::StorageFailed,
                        message: "baseline storage failed".into(),
                    })
                } else {
                    Ok(())
                }
            })
            .await;
            assert_eq!(
                result.unwrap_err().code,
                FileBaselineErrorCodeV1::StorageFailed
            );
            let count: i64 = sqlx::query_scalar("SELECT count(*) FROM exploration_baselines_v4")
                .fetch_one(s.pool())
                .await
                .unwrap();
            assert_eq!(count, 1);
            assert_eq!(
                std::fs::read(std::path::Path::new(&p.local_root).join("a.txt")).unwrap(),
                b"changed\n"
            );
        }
    }
    #[tokio::test]
    async fn capture_orphan_physical_budget_and_corrupt_manifest_fail_closed() {
        let (_d, s, p, b, r) = fixture().await;
        let first = capture_file_baseline(&s, &b, r.clone()).await.unwrap();
        let dir = b
            .root
            .join("workspace-file-baselines")
            .join(p.id.to_string())
            .join("sha256");
        std::fs::create_dir_all(dir.join("aa")).unwrap();
        let orphan = dir.join("aa").join("a".repeat(64));
        std::fs::File::create(&orphan)
            .unwrap()
            .set_len(256 * 1024 * 1024)
            .unwrap();
        let mut next = r.clone();
        next.request_id = Uuid::new_v4();
        capture_file_baseline(&s, &b, next).await.unwrap();
        assert!(!orphan.exists());
        std::fs::write(&orphan, b"orphan").unwrap();
        sqlx::query("UPDATE exploration_baselines_v4 SET manifest_json='{}' WHERE id=?")
            .bind(first.id.to_string())
            .execute(s.pool())
            .await
            .unwrap();
        let mut next = r;
        next.request_id = Uuid::new_v4();
        assert_eq!(
            capture_file_baseline(&s, &b, next).await.unwrap_err().code,
            FileBaselineErrorCodeV1::ManifestChanged
        );
        assert!(orphan.exists());
    }

    #[tokio::test]
    async fn capture_detects_retained_handle_identity_or_length_change() {
        let (_d, s, p, b, r) = fixture().await;
        let source = std::path::Path::new(&p.local_root).join("a.txt");
        let result = capture_with_checkpoint(&s, &b, r, |stage| {
            if stage == CaptureStage::AfterSourceOpen {
                std::fs::write(&source, b"changed after open\n").unwrap();
            }
            Ok(())
        })
        .await;
        assert_eq!(
            result.unwrap_err().code,
            FileBaselineErrorCodeV1::SourceChanged
        );
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM exploration_baselines_v4")
            .fetch_one(s.pool())
            .await
            .unwrap();
        assert_eq!(count, 0);
    }
    #[tokio::test]
    async fn capture_unknown_and_recorded_ssh_are_references_and_expected_hash_fails() {
        let (_d, s, mut p, b, mut r) = fixture().await;
        let id = Uuid::new_v4();
        p.connection_id = Some(id);
        p.remote_root = Some("/project".into());
        s.save_connection(&omicsops_core::domain::ConnectionProfile {
            id,
            label: "fixture".into(),
            host: "example.invalid".into(),
            port: 22,
            username: "fixture".into(),
            authentication: omicsops_core::domain::AuthenticationMethod::Password,
            authentication_reference: "fixture-ref".into(),
            host_key_fingerprint: None,
        })
        .await
        .unwrap();
        s.save_project(&p).await.unwrap();
        r.files[0].backend_id = format!("ssh:{id}");
        let unknown = capture_file_baseline(&s, &b, r.clone()).await.unwrap();
        assert_eq!(unknown.entries[0].size_bytes, None);
        assert_eq!(unknown.entries[0].hash, None);
        assert_eq!(
            unknown.entries[0].omission,
            Some(BaselineOmissionCodeV1::RemoteReference)
        );
        let analysis = Uuid::new_v4();
        let artifact = Uuid::new_v4();
        let producer = omicsops_science::AnalysisRecordV4 {
            schema_version: 4,
            id: analysis,
            project_id: p.id,
            run_id: Uuid::new_v4(),
            source_call_id: "fixture".into(),
            analysis_type: "file".into(),
            input_dataset_ids: Default::default(),
            sample_ids: Default::default(),
            method: "fixture".into(),
            parameters: serde_json::json!({}),
            software_requirements: Default::default(),
            database_versions: Default::default(),
            random_seed: Some(1),
            status: omicsops_science::AnalysisStatusV4::Succeeded,
            runtime: omicsops_science::RuntimeIdentityV4 {
                backend_id: format!("ssh:{id}"),
                language: "python".into(),
                environment: "system".into(),
                session_id: None,
                process_identity: None,
            },
            started_at: chrono::Utc::now(),
            ended_at: Some(chrono::Utc::now()),
            validation_issues: vec![],
        };
        let record = omicsops_science::ArtifactRecordV4 {
            schema_version: 4,
            id: artifact,
            project_id: p.id,
            artifact_type: "file".into(),
            relative_path: "a.txt".into(),
            producer_analysis_id: analysis,
            size_bytes: 9,
            sha256: "a".repeat(64),
            preview: None,
            metadata: serde_json::json!({}),
            valid: true,
            created_at: chrono::Utc::now(),
        };
        let a = serde_json::to_value(record).unwrap();
        let n = serde_json::to_value(producer).unwrap();
        sqlx::query("INSERT INTO scientific_analyses_v4(id,project_id,status,value_json) VALUES(?,?,'succeeded',?)").bind(analysis.to_string()).bind(p.id.to_string()).bind(n.to_string()).execute(s.pool()).await.unwrap();
        sqlx::query("INSERT INTO scientific_artifacts_v4(id,project_id,producer_analysis_id,valid,value_json) VALUES(?,?,?,1,?)").bind(artifact.to_string()).bind(p.id.to_string()).bind(analysis.to_string()).bind(a.to_string()).execute(s.pool()).await.unwrap();
        r.request_id = Uuid::new_v4();
        let recorded = capture_file_baseline(&s, &b, r.clone()).await.unwrap();
        assert_eq!(recorded.entries[0].size_bytes, Some(9));
        assert_eq!(
            recorded.entries[0].hash.as_ref().unwrap().scope,
            FileReferenceHashScopeV1::RecordedFile
        );
        r.request_id = Uuid::new_v4();
        r.files[0].expected_full_sha256 = Some("a".repeat(64));
        assert_eq!(
            capture_file_baseline(&s, &b, r).await.unwrap_err().code,
            FileBaselineErrorCodeV1::SourceChanged
        );
    }

    #[tokio::test]
    async fn capture_app_data_reparse_probe_refuses_cleanup_and_preserves_sources() {
        let (_d, s, p, b, r) = fixture().await;
        let first = capture_file_baseline(&s, &b, r).await.unwrap();
        let blob = b
            .blob_path(
                p.id,
                first.entries[0].hash.as_ref().unwrap().sha256.as_str(),
            )
            .unwrap();
        let namespace = b.root.join(NAMESPACE);
        let result = b.cleanup_with_directory_probe(p.id, |path| {
            if path == namespace {
                Err(error(FileBaselineErrorCodeV1::StorageFailed))
            } else {
                verified_open(path, true)
            }
        });
        assert_eq!(
            result.unwrap_err().code,
            FileBaselineErrorCodeV1::StorageFailed
        );
        assert!(blob.exists());
        assert_eq!(
            std::fs::read(std::path::Path::new(&p.local_root).join("a.txt")).unwrap(),
            b"hello\n"
        );
    }
    #[tokio::test]
    async fn capture_real_db_rejection_leaves_only_bounded_orphan_and_retry_recovers() {
        let (_d, s, p, b, r) = fixture().await;
        sqlx::query("CREATE TRIGGER reject_baseline BEFORE INSERT ON exploration_baselines_v4 BEGIN SELECT RAISE(FAIL,'synthetic failure'); END").execute(s.pool()).await.unwrap();
        assert_eq!(
            capture_file_baseline(&s, &b, r.clone())
                .await
                .unwrap_err()
                .code,
            FileBaselineErrorCodeV1::StorageFailed
        );
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM exploration_baselines_v4")
            .fetch_one(s.pool())
            .await
            .unwrap();
        assert_eq!(count, 0);
        let (_, physical) = scan_blobs(&b.project_dir(p.id).join("sha256")).unwrap();
        assert_eq!(physical, 6);
        let (_, staging) = scan_staging(&b.project_dir(p.id).join("sha256")).unwrap();
        assert_eq!(staging, 0);
        sqlx::query("DROP TRIGGER reject_baseline")
            .execute(s.pool())
            .await
            .unwrap();
        capture_file_baseline(&s, &b, r).await.unwrap();
        assert_eq!(
            scan_blobs(&b.project_dir(p.id).join("sha256")).unwrap().1,
            6
        );
    }
    #[tokio::test]
    async fn saved_text_rechecks_utf8_and_sensitive_body_even_with_valid_manifest_hash() {
        for body in [b"\xff".as_slice(), b"password=FAKE_SENTINEL".as_slice()] {
            let (_d, s, p, b, r) = fixture().await;
            let baseline = capture_file_baseline(&s, &b, r).await.unwrap();
            let mut manifest: FileBaselineManifestV1 = serde_json::from_str(
                &sqlx::query_scalar::<_, String>(
                    "SELECT manifest_json FROM exploration_baselines_v4 WHERE id=?",
                )
                .bind(baseline.id.to_string())
                .fetch_one(s.pool())
                .await
                .unwrap(),
            )
            .unwrap();
            let hash = sha256(body);
            manifest.entries[0].entry.hash = Some(FileReferenceHashV1 {
                sha256: hash.clone(),
                scope: FileReferenceHashScopeV1::FullFile,
            });
            manifest.entries[0].entry.size_bytes = Some(body.len() as u64);
            manifest.entries[0].blob_sha256 = Some(hash.clone());
            manifest.captured_text_bytes = body.len() as u64;
            let json = serde_json::to_string(&manifest).unwrap();
            let digest = sha256(json.as_bytes());
            sqlx::query("UPDATE exploration_baselines_v4 SET manifest_json=?,manifest_sha256=?,captured_text_bytes=? WHERE id=?").bind(json).bind(&digest).bind(body.len()as i64).bind(baseline.id.to_string()).execute(s.pool()).await.unwrap();
            let path = b.blob_path(p.id, &hash).unwrap();
            lock_directory(path.parent().unwrap(), true).unwrap();
            std::fs::write(path, body).unwrap();
            let q = ReadBaselineTextRequestV1 {
                project_id: p.id,
                baseline_id: baseline.id,
                expected_manifest_sha256: digest,
                entry_index: 0,
            };
            assert_eq!(
                read_baseline_text(&s, &b, q).await.unwrap_err().code,
                FileBaselineErrorCodeV1::StorageFailed
            );
        }
    }
    #[tokio::test]
    async fn capture_staging_hard_limit_and_unsafe_namespace_fail_without_source_changes() {
        let (d, s, p, b, mut r) = fixture().await;
        capture_file_baseline(&s, &b, r.clone()).await.unwrap();
        let staging = b
            .project_dir(p.id)
            .join("sha256/58/baseline-0123456789abcdef.tmp");
        std::fs::File::create(&staging)
            .unwrap()
            .set_len(4 * 1024 * 1024 + 1)
            .unwrap();
        r.request_id = Uuid::new_v4();
        assert_eq!(
            capture_file_baseline(&s, &b, r.clone())
                .await
                .unwrap_err()
                .code,
            FileBaselineErrorCodeV1::LimitExceeded
        );
        assert!(staging.exists());
        assert_eq!(
            std::fs::read(std::path::Path::new(&p.local_root).join("a.txt")).unwrap(),
            b"hello\n"
        );
        let unsafe_storage = FileBaselineStorage::new(d.path().join("test.db"));
        assert_eq!(
            capture_file_baseline(&s, &unsafe_storage, r)
                .await
                .unwrap_err()
                .code,
            FileBaselineErrorCodeV1::StorageFailed
        );
    }
    #[tokio::test]
    async fn capture_blob_temp_is_in_destination_directory_and_staging_is_classified() {
        let (_d, s, p, b, r) = fixture().await;
        let prefix = b.project_dir(p.id).join("sha256/58");
        capture_with_checkpoint(&s, &b, r.clone(), |stage| {
            if stage == CaptureStage::BeforeSync {
                let names = std::fs::read_dir(&prefix)
                    .unwrap()
                    .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
                    .collect::<Vec<_>>();
                assert_eq!(names.len(), 1);
                assert!(names[0].starts_with("baseline-") && names[0].ends_with(".tmp"));
            }
            Ok(())
        })
        .await
        .unwrap();
        let orphan = prefix.join("baseline-0123456789abcdef.tmp");
        std::fs::write(&orphan, b"temporary bytes").unwrap();
        let mut next = r;
        next.request_id = Uuid::new_v4();
        capture_file_baseline(&s, &b, next).await.unwrap();
        assert!(!orphan.exists());
        assert_eq!(
            scan_staging(&b.project_dir(p.id).join("sha256")).unwrap().1,
            0
        );
    }

    #[tokio::test]
    async fn capture_missing_source_is_unreadable_without_inventing_absence_from_w02() {
        let (_d, s, p, b, r) = fixture().await;
        std::fs::remove_file(Path::new(&p.local_root).join("a.txt")).unwrap();
        let baseline = capture_file_baseline(&s, &b, r).await.unwrap();
        assert_eq!(
            baseline.entries[0].omission,
            Some(BaselineOmissionCodeV1::Unreadable)
        );
        assert_eq!(baseline.entries[0].size_bytes, None);
        assert_eq!(baseline.entries[0].hash, None);
    }
    #[tokio::test]
    async fn capture_sensitive_filename_downgrades_before_source_open_and_never_persists_body() {
        for path in [
            "credentials.json",
            "CrEdEnTiAlS.JsOn",
            "AUTH/data.txt",
            "private_key.txt",
            "PRIVATE-KEY/value.R",
            "id_ed25519.txt",
            "config/.ENV/values.json",
        ] {
            let (_d, s, p, b, mut r) = fixture().await;
            let source = Path::new(&p.local_root).join(path);
            std::fs::create_dir_all(source.parent().unwrap()).unwrap();
            std::fs::write(source, b"harmless UTF8").unwrap();
            r.files[0].relative_path = path.into();
            let opens = std::cell::Cell::new(0);
            let baseline = capture_with_source_open(
                &s,
                &b,
                r,
                |_| Ok(()),
                |project, path| {
                    opens.set(opens.get() + 1);
                    ValidatedProjectFile::open(project, path)
                },
            )
            .await
            .unwrap();
            assert_eq!(
                opens.get(),
                0,
                "credential filenames must be rejected before opening sources"
            );
            assert_eq!(
                baseline.entries[0].content_mode,
                WorkspaceContentModeV1::ReferenceOnly
            );
            assert_eq!(
                baseline.entries[0].omission,
                Some(BaselineOmissionCodeV1::SensitiveContent)
            );
            assert_eq!(baseline.entries[0].relative_path.as_deref(), Some(path));
            assert!(
                scan_blobs(&b.project_dir(p.id).join("sha256"))
                    .unwrap()
                    .0
                    .is_empty()
            );
            assert_eq!(baseline.captured_text_bytes, 0);
        }
    }
    #[tokio::test]
    async fn capture_open_time_source_change_aborts_entire_request_but_unavailable_is_unreadable() {
        let (_d, s, p, b, mut r) = fixture().await;
        std::fs::write(Path::new(&p.local_root).join("z.txt"), b"later source").unwrap();
        r.files.push(ProjectFileSelectionV1 {
            backend_id: "local".into(),
            relative_path: "z.txt".into(),
            expected_full_sha256: None,
        });
        let result = capture_with_source_open(
            &s,
            &b,
            r.clone(),
            |_| Ok(()),
            |project, path| {
                if path == "z.txt" {
                    Err(FileReadError::SourceChanged)
                } else {
                    ValidatedProjectFile::open(project, path)
                }
            },
        )
        .await;
        assert_eq!(
            result.unwrap_err().code,
            FileBaselineErrorCodeV1::SourceChanged
        );
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM exploration_baselines_v4")
            .fetch_one(s.pool())
            .await
            .unwrap();
        assert_eq!(count, 0);
        assert!(
            scan_blobs(&b.project_dir(p.id).join("sha256"))
                .unwrap()
                .0
                .is_empty()
        );
        let baseline = capture_with_source_open(
            &s,
            &b,
            r,
            |_| Ok(()),
            |_, _| Err(FileReadError::Unavailable),
        )
        .await
        .unwrap();
        assert_eq!(
            baseline.entries[0].omission,
            Some(BaselineOmissionCodeV1::Unreadable)
        );
    }
}
