//! Fixed synthetic content and bounded Windows-only workspace publication.
//! Creation timestamps are preset metadata, never execution evidence.
use crate::project_file_read::ValidatedProjectFile;
use omicsops_core::workspace::{Project, ProjectTemplate};
use omicsops_dto::{
    CreateOfflineExampleWorkspaceRequest, CreateOfflineExampleWorkspaceResult,
    OfflineExampleAvailability, OfflineExampleEntry, OfflineExampleFileAvailability,
    OfflineExampleFileView, OfflineExampleSourceKind, OfflineExampleText,
    OfflineExampleUnavailableReason, OfflineExampleWorkspaceView,
};
use omicsops_store::{
    OfflineExampleProjectMarkerV1, Store, StoreError, offline_example_root_binding,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Component, Path, PathBuf},
};
use uuid::Uuid;

pub(crate) const EXAMPLE_ID: &str = "synthetic-evidence-report-v1";
pub(crate) const MAX_FILE_BYTES: usize = 64 * 1024;
const MANIFEST: &str = include_str!("../../examples/offline/synthetic-evidence-report-v1.json");
const LICENSE: &str = include_str!("../../LICENSE");
const FIXED_FILES: [&str; 7] = [
    "data/observations.csv",
    "results/summary.csv",
    "report.zh.md",
    "report.en.md",
    "README.md",
    "SOURCE.json",
    "LICENSE.txt",
];
const RECEIPT: &str = ".omicsops/offline-example.json";
const PROJECT_MANIFEST: &str = ".omicsops/project.json";

/// Identity always comes from Store; a disk receipt cannot turn an ordinary
/// project into an example. Sequential file reads are not an atomic snapshot.
pub(crate) async fn load_offline_example_workspace(
    repository: &Store,
    project_id: Uuid,
) -> Result<Option<OfflineExampleWorkspaceView>, String> {
    let Some(marker) = repository
        .get_offline_example_project_marker(project_id)
        .await
        .map_err(|error| match error {
            StoreError::InvalidInput(_) | StoreError::Json(_) => "offline_example_marker_invalid",
            _ => "example_read_failed",
        })?
    else {
        return Ok(None);
    };
    let fixture = load_bundled_example()?;
    if marker.fixture_sha256 != fixture_sha256(&fixture)? {
        return Err("offline_example_marker_invalid".into());
    }
    let mut view = OfflineExampleWorkspaceView {
        schema_version: 1,
        project_id,
        example_id: EXAMPLE_ID.into(),
        source_kind: OfflineExampleSourceKind::Synthetic,
        availability: OfflineExampleAvailability::Unavailable,
        unavailable_reason: Some(OfflineExampleUnavailableReason::IdentityMismatch),
        title: fixture.title.clone(),
        disclaimer: fixture.disclaimer.clone(),
        source_description: fixture.source_description.clone(),
        license_spdx: fixture.license_spdx.clone(),
        fixture_sha256: marker.fixture_sha256.clone(),
        entries: Vec::new(),
        files: Vec::new(),
    };
    let Some(project) = repository
        .get_project(project_id)
        .await
        .map_err(|_| "example_read_failed")?
    else {
        return Ok(Some(view));
    };
    if offline_example_root_binding(&project) != marker.root_binding_sha256 {
        return Ok(Some(view));
    }
    // Keep the root chain pinned on Windows while consuming the fixed receipt
    // and file list. Every ordinary file still passes the W02 handle checks.
    let root = match validate_parent_spelling(&project.local_root) {
        Ok(root) => root,
        Err(_) => {
            view.unavailable_reason = Some(OfflineExampleUnavailableReason::RootUnavailable);
            return Ok(Some(view));
        }
    };
    let _root_guards = match lock_directory_chain(&root) {
        Ok(guards) => guards,
        Err(_) => {
            view.unavailable_reason = Some(OfflineExampleUnavailableReason::RootUnavailable);
            return Ok(Some(view));
        }
    };
    let receipt = match read_example_receipt(&project) {
        Ok(receipt) => receipt,
        Err(error) => {
            view.unavailable_reason = Some(if error == "example_receipt_missing" {
                OfflineExampleUnavailableReason::ReceiptMissing
            } else {
                OfflineExampleUnavailableReason::ReceiptInvalid
            });
            return Ok(Some(view));
        }
    };
    if receipt.project_id != project_id || receipt.request_id != marker.request_id {
        view.unavailable_reason = Some(OfflineExampleUnavailableReason::IdentityMismatch);
        return Ok(Some(view));
    }
    if receipt != expected_receipt(marker.request_id, project_id, &fixture)? {
        view.unavailable_reason = Some(OfflineExampleUnavailableReason::ReceiptInvalid);
        return Ok(Some(view));
    }
    view.availability = OfflineExampleAvailability::Available;
    view.unavailable_reason = None;
    view.entries = fixture.entries;
    view.files = FIXED_FILES
        .iter()
        .map(|path| read_example_file(&project, path, &sha256(fixture.files[*path].as_bytes())))
        .collect();
    Ok(Some(view))
}

/// Fixed internal exception only; ordinary W02 reads still reject .omicsops.
pub(crate) fn read_example_receipt(project: &Project) -> Result<OfflineExampleReceiptV1, String> {
    let root =
        validate_parent_spelling(&project.local_root).map_err(|_| "example_receipt_invalid")?;
    let _guards = lock_directory_chain(&root).map_err(|_| "example_receipt_invalid")?;
    // Only a safely checked normal internal directory can identify a missing
    // receipt. A replaced/junction directory remains an invalid receipt.
    let internal = root.join(".omicsops");
    match reject_reparse(&internal) {
        Ok(metadata) if metadata.is_dir() => (),
        _ => {
            return Err(if fs::symlink_metadata(&internal)
                .is_err_and(|error| error.kind() == std::io::ErrorKind::NotFound)
            {
                "example_receipt_missing"
            } else {
                "example_receipt_invalid"
            }
            .into());
        }
    }
    let _internal_guards =
        lock_directory_chain(&internal).map_err(|_| "example_receipt_invalid")?;
    if fs::symlink_metadata(root.join(RECEIPT))
        .is_err_and(|error| error.kind() == std::io::ErrorKind::NotFound)
    {
        return Err("example_receipt_missing".into());
    }
    let bytes = read_internal_file(&root, RECEIPT).map_err(|_| "example_receipt_invalid")?;
    serde_json::from_slice(&bytes).map_err(|_| "example_receipt_invalid".into())
}

fn read_example_file(
    project: &Project,
    path: &str,
    expected_sha256: &str,
) -> OfflineExampleFileView {
    let media_type = match Path::new(path)
        .extension()
        .and_then(|extension| extension.to_str())
    {
        Some("csv") => "text/csv",
        Some("md") => "text/markdown",
        Some("json") => "application/json",
        _ => "text/plain",
    };
    let mut view = OfflineExampleFileView {
        relative_path: path.into(),
        media_type: media_type.into(),
        expected_sha256: expected_sha256.into(),
        current_sha256: None,
        availability: OfflineExampleFileAvailability::Unavailable,
        preview_text: None,
    };
    // A NotFound probe is only used for status. Content always comes from W02.
    let read = ValidatedProjectFile::open(project, path)
        .and_then(|mut file| file.read_complete(MAX_FILE_BYTES));
    match read {
        Ok(read) => {
            if let Ok(text) = std::str::from_utf8(&read.bytes) {
                view.availability = if read.sha256 == expected_sha256 {
                    OfflineExampleFileAvailability::Available
                } else {
                    OfflineExampleFileAvailability::Changed
                };
                view.current_sha256 = Some(read.sha256);
                view.preview_text = Some(crate::composer_references::public_text(text));
            }
        }
        Err(crate::project_file_read::FileReadError::Unavailable)
            if fs::symlink_metadata(Path::new(&project.local_root).join(path))
                .is_err_and(|error| error.kind() == std::io::ErrorKind::NotFound) =>
        {
            // W02 can encounter NotFound before reaching a junction ancestor.
            // Check the nearest existing directory chain for status only; do
            // not classify an inaccessible/reparse-backed target as missing.
            let candidate = Path::new(&project.local_root).join(path);
            let mut parent = candidate.parent();
            while parent.is_some_and(|parent| {
                fs::symlink_metadata(parent)
                    .is_err_and(|error| error.kind() == std::io::ErrorKind::NotFound)
            }) {
                parent = parent.and_then(Path::parent);
            }
            if parent.is_some_and(|parent| lock_directory_chain(parent).is_ok()) {
                view.availability = OfflineExampleFileAvailability::Missing;
            }
        }
        Err(_) => (),
    }
    view
}

async fn create_offline_example_for_repository(
    repository: &Store,
    request: &CreateOfflineExampleWorkspaceRequest,
) -> Result<CreateOfflineExampleWorkspaceResult, String> {
    let fixture = load_bundled_example()?;
    let prepared = materialize_offline_example(request, &fixture, write_new_example_file)?;
    repository
        .save_offline_example_project(&prepared.project, &prepared.marker)
        .await
        .map_err(|_| "example_registration_failed")?;
    // Reload Store's project so an identical retry retains user metadata.
    let workspace = load_offline_example_workspace(repository, prepared.project.id)
        .await?
        .ok_or("example_read_failed")?;
    Ok(CreateOfflineExampleWorkspaceResult {
        project_id: prepared.project.id,
        workspace,
    })
}

#[tauri::command]
pub async fn get_offline_example_workspace(
    state: tauri::State<'_, crate::commands::AppState>,
    project_id: Uuid,
) -> Result<Option<OfflineExampleWorkspaceView>, String> {
    load_offline_example_workspace(&state.repository, project_id).await
}

#[tauri::command]
pub async fn create_offline_example_workspace(
    state: tauri::State<'_, crate::commands::AppState>,
    request: CreateOfflineExampleWorkspaceRequest,
) -> Result<CreateOfflineExampleWorkspaceResult, String> {
    create_offline_example_for_repository(&state.repository, &request).await
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct OfflineExampleFixtureV1 {
    pub schema_version: u8,
    pub example_id: String,
    pub source_kind: OfflineExampleSourceKind,
    pub title: OfflineExampleText,
    pub disclaimer: OfflineExampleText,
    pub source_description: OfflineExampleText,
    pub license_spdx: String,
    pub entries: Vec<OfflineExampleEntry>,
    pub files: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct OfflineExampleReceiptFile {
    pub relative_path: String,
    pub expected_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct OfflineExampleReceiptV1 {
    pub schema_version: u8,
    pub project_id: Uuid,
    pub request_id: Uuid,
    pub example_id: String,
    pub fixture_sha256: String,
    pub files: Vec<OfflineExampleReceiptFile>,
}

#[derive(Debug)]
pub(crate) struct PreparedOfflineExampleProject {
    pub project: Project,
    pub marker: OfflineExampleProjectMarkerV1,
}

pub(crate) fn sha256(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn parse_fixture_manifest(raw: &str) -> Result<OfflineExampleFixtureV1, String> {
    if raw.len() > 256 * 1024 {
        return Err("example_fixture_invalid".into());
    }
    serde_json::from_str(raw).map_err(|_| "example_fixture_invalid".into())
}

pub(crate) fn load_bundled_example() -> Result<OfflineExampleFixtureV1, String> {
    let mut fixture = parse_fixture_manifest(MANIFEST)?;
    let declared: BTreeSet<_> = fixture.files.keys().map(String::as_str).collect();
    let expected: BTreeSet<_> = FIXED_FILES
        .iter()
        .copied()
        .filter(|p| !matches!(*p, "LICENSE.txt" | "SOURCE.json"))
        .collect();
    if declared != expected {
        return Err("example_fixture_invalid".into());
    }
    fixture.files.insert("LICENSE.txt".into(), LICENSE.into());
    let hashes: BTreeMap<_, _> = fixture
        .files
        .iter()
        .map(|(path, content)| (path.clone(), sha256(content.as_bytes())))
        .collect();
    fixture.files.insert(
        "SOURCE.json".into(),
        serde_json::to_string_pretty(&serde_json::json!({
            "origin": "Original OmicsOps repository content",
            "source_kind": "synthetic",
            "example_id": EXAMPLE_ID,
            "fixture_schema": "OfflineExampleFixtureV1",
            "schema_version": 1,
            "license_spdx": "AGPL-3.0-only",
            "hash_scope": "The other six fixed files; SOURCE.json excludes itself",
            "files": hashes
        }))
        .map_err(|_| "example_fixture_invalid")?,
    );
    validate_fixture(&fixture)?;
    Ok(fixture)
}

fn validate_fixture(fixture: &OfflineExampleFixtureV1) -> Result<(), String> {
    let bilingual =
        |text: &OfflineExampleText| !text.zh.trim().is_empty() && !text.en.trim().is_empty();
    if fixture.schema_version != 1
        || fixture.example_id != EXAMPLE_ID
        || fixture.license_spdx != "AGPL-3.0-only"
        || fixture.source_kind != OfflineExampleSourceKind::Synthetic
        || !bilingual(&fixture.title)
        || !bilingual(&fixture.disclaimer)
        || !bilingual(&fixture.source_description)
        || fixture.entries.len() != 4
        || fixture.entries.iter().any(|entry| !bilingual(&entry.text))
        || fixture.files.len() > 12
        || fixture.files.len() != FIXED_FILES.len()
        || fixture
            .files
            .keys()
            .any(|path| !FIXED_FILES.contains(&path.as_str()))
        || fixture
            .files
            .values()
            .any(|content| content.len() > MAX_FILE_BYTES)
        || fixture.files.values().map(String::len).sum::<usize>() > 512 * 1024
    {
        return Err("example_fixture_invalid".into());
    }
    Ok(())
}

pub(crate) fn fixture_sha256(fixture: &OfflineExampleFixtureV1) -> Result<String, String> {
    serde_json::to_vec(fixture)
        .map(|bytes| sha256(&bytes))
        .map_err(|_| "example_fixture_invalid".into())
}

pub(crate) fn expected_receipt(
    request_id: Uuid,
    project_id: Uuid,
    fixture: &OfflineExampleFixtureV1,
) -> Result<OfflineExampleReceiptV1, String> {
    Ok(OfflineExampleReceiptV1 {
        schema_version: 1,
        project_id,
        request_id,
        example_id: EXAMPLE_ID.into(),
        fixture_sha256: fixture_sha256(fixture)?,
        files: fixture
            .files
            .iter()
            .map(|(path, content)| OfflineExampleReceiptFile {
                relative_path: path.clone(),
                expected_sha256: sha256(content.as_bytes()),
            })
            .collect(),
    })
}

// This boundary only accepts normal absolute local paths, never a namespace alias.
fn validate_parent_spelling(text: &str) -> Result<PathBuf, String> {
    if text.is_empty()
        || text != text.trim()
        || text.starts_with("\\\\")
        || text.starts_with("//")
        || !Path::new(text).is_absolute()
        || text.split(['/', '\\']).any(|part| {
            part == "."
                || part == ".."
                || part.ends_with([' ', '.'])
                || crate::project_file_read::is_reserved_component(part)
        })
        || text.chars().any(char::is_control)
    {
        return Err("example_parent_invalid".into());
    }
    #[cfg(windows)]
    if !matches!(Path::new(text).components().next(), Some(Component::Prefix(prefix)) if matches!(prefix.kind(), std::path::Prefix::Disk(_)))
    {
        return Err("example_parent_invalid".into());
    }
    Ok(PathBuf::from(text))
}

fn reject_reparse(path: &Path) -> Result<fs::Metadata, String> {
    let metadata = fs::symlink_metadata(path).map_err(|_| "example_path_unavailable")?;
    if metadata.file_type().is_symlink() {
        return Err("example_path_unsafe".into());
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & (0x400 | 0x40) != 0 {
            return Err("example_path_unsafe".into());
        }
    }
    Ok(metadata)
}

// Canonical DOS spelling remains accepted by the shared W02 file reader.
fn canonical_local(path: &Path) -> Result<PathBuf, String> {
    let canonical = fs::canonicalize(path).map_err(|_| "example_path_unavailable")?;
    #[cfg(windows)]
    {
        let mut components = canonical.components();
        match components.next() {
            Some(Component::Prefix(prefix)) => match prefix.kind() {
                std::path::Prefix::VerbatimDisk(drive) => {
                    let mut result = PathBuf::from(format!("{}:\\", char::from(drive)));
                    for part in components {
                        if let Component::Normal(name) = part {
                            result.push(name);
                        }
                    }
                    return Ok(result);
                }
                _ => return Err("example_path_unsafe".into()),
            },
            _ => return Err("example_path_unsafe".into()),
        }
    }
    #[cfg(not(windows))]
    Ok(canonical)
}

// Holding non-delete-sharing Windows directory handles prevents parent rename
// during staging/publication. No directory is created before these checks.
fn lock_directory_chain(path: &Path) -> Result<Vec<File>, String> {
    let mut guards = Vec::new();
    for ancestor in path.ancestors().filter(|p| !p.as_os_str().is_empty()) {
        if !reject_reparse(ancestor)?.is_dir() {
            return Err("example_path_unsafe".into());
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            use windows_sys::Win32::Storage::FileSystem::*;
            let file = OpenOptions::new()
                .read(true)
                .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
                .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
                .open(ancestor)
                .map_err(|_| "example_path_unavailable")?;
            let snapshot = internal_snapshot(&file)?;
            if snapshot.0 != fs::canonicalize(ancestor).map_err(|_| "example_path_unavailable")? {
                return Err("example_path_unsafe".into());
            }
            guards.push(file);
        }
        #[cfg(not(windows))]
        guards.push(File::open(ancestor).map_err(|_| "example_path_unavailable")?);
    }
    Ok(guards)
}

pub(crate) fn write_new_example_file(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if bytes.len() > MAX_FILE_BYTES {
        return Err("example_file_too_large".into());
    }
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options
            .share_mode(0)
            .custom_flags(windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT);
    }
    let mut file = options.open(path).map_err(|_| "example_write_failed")?;
    file.write_all(bytes)
        .and_then(|_| file.sync_all())
        .map_err(|_| "example_write_failed".into())
}

#[cfg(windows)]
fn internal_snapshot(file: &File) -> Result<(PathBuf, u32, u64, u64, u64), String> {
    use std::os::windows::{ffi::OsStringExt, io::AsRawHandle};
    use windows_sys::Win32::Storage::FileSystem::*;
    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    if unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) } == 0
        || info.dwFileAttributes & (FILE_ATTRIBUTE_REPARSE_POINT | FILE_ATTRIBUTE_DEVICE) != 0
    {
        return Err("example_path_unsafe".into());
    }
    let mut buffer = vec![0u16; 32768];
    let count = unsafe {
        GetFinalPathNameByHandleW(
            file.as_raw_handle(),
            buffer.as_mut_ptr(),
            buffer.len() as u32,
            0,
        )
    } as usize;
    if count == 0 || count >= buffer.len() {
        return Err("example_path_unsafe".into());
    }
    let path = PathBuf::from(std::ffi::OsString::from_wide(&buffer[..count]));
    if !matches!(path.components().next(), Some(Component::Prefix(prefix)) if matches!(prefix.kind(), std::path::Prefix::VerbatimDisk(_)))
    {
        return Err("example_path_unsafe".into());
    }
    Ok((
        path,
        info.dwVolumeSerialNumber,
        (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow),
        (u64::from(info.nFileSizeHigh) << 32) | u64::from(info.nFileSizeLow),
        (u64::from(info.ftLastWriteTime.dwHighDateTime) << 32)
            | u64::from(info.ftLastWriteTime.dwLowDateTime),
    ))
}

/// Narrow internal-file exception: never accepts arbitrary protected paths.
/// The same opened handle supplies the complete bounded bytes and snapshots.
pub(crate) fn read_internal_file(root: &Path, relative: &str) -> Result<Vec<u8>, String> {
    if !matches!(relative, RECEIPT | PROJECT_MANIFEST) {
        return Err("example_path_unsafe".into());
    }
    let _guards = lock_directory_chain(&root.join(".omicsops"))?;
    let candidate = root.join(relative);
    if !reject_reparse(&candidate)?.is_file() {
        return Err("example_path_unsafe".into());
    }
    let canonical_root = fs::canonicalize(root).map_err(|_| "example_path_unavailable")?;
    let canonical = fs::canonicalize(&candidate).map_err(|_| "example_path_unavailable")?;
    if canonical == canonical_root || !canonical.starts_with(&canonical_root) {
        return Err("example_path_unsafe".into());
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::*;
        options
            .share_mode(FILE_SHARE_READ)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
    }
    let mut file = options
        .open(&candidate)
        .map_err(|_| "example_path_unavailable")?;
    #[cfg(windows)]
    let before = internal_snapshot(&file)?;
    #[cfg(windows)]
    if before.0 != canonical {
        return Err("example_path_unsafe".into());
    }
    let metadata = file.metadata().map_err(|_| "example_path_unavailable")?;
    if !metadata.is_file() || metadata.len() > MAX_FILE_BYTES as u64 {
        return Err("example_receipt_invalid".into());
    }
    let mut bytes = Vec::new();
    (&mut file)
        .take((MAX_FILE_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| "example_path_unavailable")?;
    #[cfg(windows)]
    if internal_snapshot(&file)? != before {
        return Err("example_receipt_invalid".into());
    }
    if bytes.len() > MAX_FILE_BYTES
        || bytes.len() as u64 != metadata.len()
        || std::str::from_utf8(&bytes).is_err()
    {
        return Err("example_receipt_invalid".into());
    }
    Ok(bytes)
}

fn check_tree(root: &Path) -> Result<(), String> {
    let files: BTreeSet<_> = FIXED_FILES
        .into_iter()
        .chain([RECEIPT, PROJECT_MANIFEST])
        .map(str::to_owned)
        .collect();
    let dirs: BTreeSet<_> = ["data", "results", ".omicsops"]
        .into_iter()
        .map(str::to_owned)
        .collect();
    let mut seen = BTreeSet::new();
    let mut pending = vec![root.to_owned()];
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(directory).map_err(|_| "example_target_conflict")? {
            let path = entry.map_err(|_| "example_target_conflict")?.path();
            let relative = path
                .strip_prefix(root)
                .map_err(|_| "example_target_conflict")?
                .to_str()
                .ok_or("example_target_conflict")?
                .replace('\\', "/");
            let metadata = reject_reparse(&path)?;
            if metadata.is_dir() && dirs.contains(&relative) {
                pending.push(path);
            } else if metadata.is_file() && files.contains(&relative) {
                seen.insert(relative);
            } else {
                return Err("example_target_conflict".into());
            }
        }
    }
    if seen != files {
        return Err("example_target_conflict".into());
    }
    Ok(())
}

fn verify_materialized(
    root: &Path,
    project: &Project,
    fixture: &OfflineExampleFixtureV1,
    receipt: &OfflineExampleReceiptV1,
) -> Result<(), String> {
    let _guards = lock_directory_chain(root)?;
    check_tree(root)?;
    if read_internal_file(root, PROJECT_MANIFEST)?
        != serde_json::to_vec_pretty(project).map_err(|_| "example_fixture_invalid")?
        || read_internal_file(root, RECEIPT)?
            != serde_json::to_vec_pretty(receipt).map_err(|_| "example_fixture_invalid")?
    {
        return Err("example_target_conflict".into());
    }
    let mut read_project = project.clone();
    read_project.local_root = root.to_str().ok_or("example_path_unsafe")?.into();
    for item in &receipt.files {
        let bytes = ValidatedProjectFile::open(&read_project, &item.relative_path)
            .and_then(|mut file| file.read_complete(MAX_FILE_BYTES))
            .map_err(|_| "example_target_conflict")?;
        if bytes.sha256 != item.expected_sha256
            || bytes.bytes != fixture.files[&item.relative_path].as_bytes()
        {
            return Err("example_target_conflict".into());
        }
    }
    Ok(())
}

// This read-attributes handle survives guard transitions and keeps the original
// file ID alive. It permits rename, but every mutation must bind its own handle
// to this retained identity before touching the filesystem.
fn retain_staging_identity(path: &Path) -> Result<File, String> {
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::*;
        let file = OpenOptions::new()
            .access_mode(FILE_READ_ATTRIBUTES)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
            .open(path)
            .map_err(|_| "example_path_unavailable")?;
        if !file.metadata().is_ok_and(|metadata| metadata.is_dir())
            || internal_snapshot(&file)?.0
                != fs::canonicalize(path).map_err(|_| "example_path_unavailable")?
        {
            return Err("example_path_unsafe".into());
        }
        Ok(file)
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        Err("example_platform_unavailable".into())
    }
}

fn retained_identity(file: &File) -> Result<(u32, u64), String> {
    #[cfg(windows)]
    {
        let snapshot = internal_snapshot(file)?;
        Ok((snapshot.1, snapshot.2))
    }
    #[cfg(not(windows))]
    {
        let _ = file;
        Err("example_platform_unavailable".into())
    }
}

// No delete sharing: the actual handle being renamed/deleted pins its name until
// that operation completes. Ancestor handles must already protect containment.
fn open_mutation_handle(path: &Path) -> Result<File, String> {
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::*;
        let file = OpenOptions::new()
            .access_mode(FILE_READ_ATTRIBUTES | DELETE)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
            .open(path)
            .map_err(|_| "example_path_unavailable")?;
        if internal_snapshot(&file)?.0
            != fs::canonicalize(path).map_err(|_| "example_path_unavailable")?
        {
            return Err("example_path_unsafe".into());
        }
        Ok(file)
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        Err("example_platform_unavailable".into())
    }
}

fn publish_directory(staging: &Path, child: &Path, trusted: &File) -> Result<(), String> {
    let parent = child.parent().ok_or("example_path_unsafe")?;
    let _parent_guards = lock_directory_chain(parent)?;
    if staging.parent() != Some(parent) {
        return Err("example_path_unsafe".into());
    }
    let source = open_mutation_handle(staging)?;
    if retained_identity(&source)? != retained_identity(trusted)? {
        return Err("example_path_unsafe".into());
    }
    if fs::symlink_metadata(child).is_ok() {
        return Err("example_target_conflict".into());
    }
    #[cfg(windows)]
    {
        use std::os::windows::{ffi::OsStrExt, io::AsRawHandle};
        use windows_sys::Win32::Storage::FileSystem::*;
        let to: Vec<u16> = child.as_os_str().encode_wide().chain(Some(0)).collect();
        let size = std::mem::size_of::<FILE_RENAME_INFO>() + to.len() * 2;
        // usize backing preserves FILE_RENAME_INFO alignment and includes the
        // variable-length UTF-16 tail. Zero initialization means no replacement.
        let mut buffer = vec![0usize; size.div_ceil(std::mem::size_of::<usize>())];
        let info = buffer.as_mut_ptr().cast::<FILE_RENAME_INFO>();
        unsafe {
            (*info).FileNameLength = ((to.len() - 1) * 2) as u32;
            std::ptr::copy_nonoverlapping(to.as_ptr(), (*info).FileName.as_mut_ptr(), to.len());
        }
        #[cfg(test)]
        tests::mutation_boundary("rename", child);
        if unsafe {
            SetFileInformationByHandle(
                source.as_raw_handle(),
                FileRenameInfo,
                info.cast(),
                size as u32,
            )
        } == 0
        {
            return Err("example_publish_failed".into());
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let _ = source;
        Err("example_platform_unavailable".into())
    }
}

fn directory_identity(path: &Path) -> Result<(u32, u64), String> {
    let guards = lock_directory_chain(path)?;
    #[cfg(windows)]
    {
        let snapshot = internal_snapshot(guards.first().ok_or("example_path_unsafe")?)?;
        Ok((snapshot.1, snapshot.2))
    }
    #[cfg(not(windows))]
    {
        let _ = guards;
        Err("example_platform_unavailable".into())
    }
}

pub(crate) fn materialize_offline_example(
    request: &CreateOfflineExampleWorkspaceRequest,
    fixture: &OfflineExampleFixtureV1,
    write_file: fn(&Path, &[u8]) -> Result<(), String>,
) -> Result<PreparedOfflineExampleProject, String> {
    validate_fixture(fixture)?;
    if request.example_id != EXAMPLE_ID || *fixture != load_bundled_example()? {
        return Err("example_fixture_invalid".into());
    }
    let requested_parent = validate_parent_spelling(&request.parent_directory)?;
    let _parent_guards = lock_directory_chain(&requested_parent)?;
    let parent = canonical_local(&requested_parent)?;
    #[cfg(not(windows))]
    return Err("example_platform_unavailable".into());
    let child = parent.join(format!("omicsops-example-{}", request.request_id));
    if child.parent() != Some(parent.as_path()) {
        return Err("example_path_unsafe".into());
    }
    let project = Project::new(
        request.request_id,
        "Offline synthetic example / 离线合成示例",
        child.to_str().ok_or("example_path_unsafe")?,
        ProjectTemplate::Blank,
        chrono::DateTime::from_timestamp(1791072000, 0).ok_or("example_fixture_invalid")?,
    );
    let receipt = expected_receipt(request.request_id, project.id, fixture)?;
    let prepared = PreparedOfflineExampleProject {
        marker: OfflineExampleProjectMarkerV1 {
            schema_version: 1,
            project_id: project.id,
            request_id: request.request_id,
            example_id: EXAMPLE_ID.into(),
            fixture_sha256: receipt.fixture_sha256.clone(),
            root_binding_sha256: offline_example_root_binding(&project),
        },
        project,
    };
    match fs::symlink_metadata(&child) {
        Ok(_) => {
            verify_materialized(&child, &prepared.project, fixture, &receipt)?;
            return Ok(prepared);
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
        Err(_) => return Err("example_path_unavailable".into()),
    }
    let staging = parent.join(format!(
        ".omicsops-example-{}-{}.staging",
        request.request_id,
        Uuid::new_v4()
    ));
    fs::create_dir(&staging).map_err(|_| "example_write_failed")?;
    let staging_identity = retain_staging_identity(&staging)?;
    let operation: Result<(), String> = (|| {
        let staging_guards = lock_directory_chain(&staging)?;
        if canonical_local(&staging)? != staging
            || directory_identity(&staging)? != retained_identity(&staging_identity)?
        {
            return Err("example_path_unsafe".into());
        }
        for directory in ["data", "results", ".omicsops"] {
            fs::create_dir(staging.join(directory)).map_err(|_| "example_write_failed")?;
        }
        for (path, content) in &fixture.files {
            let _guards =
                lock_directory_chain(staging.join(path).parent().ok_or("example_path_unsafe")?)?;
            write_file(&staging.join(path), content.as_bytes())?;
        }
        for (path, bytes) in [
            (
                PROJECT_MANIFEST,
                serde_json::to_vec_pretty(&prepared.project)
                    .map_err(|_| "example_fixture_invalid")?,
            ),
            (
                RECEIPT,
                serde_json::to_vec_pretty(&receipt).map_err(|_| "example_fixture_invalid")?,
            ),
        ] {
            let _guards = lock_directory_chain(&staging.join(".omicsops"))?;
            write_file(&staging.join(path), &bytes)?;
        }
        verify_materialized(&staging, &prepared.project, fixture, &receipt)?;
        if canonical_local(&requested_parent)? != parent
            || directory_identity(&staging)? != retained_identity(&staging_identity)?
        {
            return Err("example_path_unsafe".into());
        }
        let _guards = lock_directory_chain(&parent)?;
        drop(staging_guards);
        #[cfg(test)]
        tests::mutation_boundary("publish", &staging);
        publish_directory(&staging, &child, &staging_identity)?;
        // From publication onward never delete the final directory, even if verification
        // or the later atomic Store registration fails. Exact request retry can recover.
        verify_materialized(&child, &prepared.project, fixture, &receipt)?;
        Ok(())
    })();
    if operation.is_err() {
        cleanup_staging(&parent, &staging, &staging_identity);
    }
    operation?;
    Ok(prepared)
}

fn cleanup_staging(parent: &Path, staging: &Path, trusted: &File) {
    // Only a staging we created may reach this function. Never follow a replaced
    // directory or recursively delete unknown entries. A suspicious tree is retained.
    let Ok(_parent_guards) = lock_directory_chain(parent) else {
        return;
    };
    let Ok(identity) = retained_identity(trusted) else {
        return;
    };
    if staging.parent() != Some(parent) || canonical_local(staging).ok().as_deref() != Some(staging)
    {
        return;
    }
    #[cfg(test)]
    tests::mutation_boundary("cleanup", staging);
    let Ok(root_guard) = open_mutation_handle(staging) else {
        return;
    };
    if retained_identity(&root_guard).ok() != Some(identity) {
        return;
    }
    let allowed: BTreeSet<_> = FIXED_FILES
        .into_iter()
        .chain([RECEIPT, PROJECT_MANIFEST])
        .collect();
    let mut files = Vec::new();
    let mut nested_guards = Vec::new();
    for directory in [
        staging.to_owned(),
        staging.join("data"),
        staging.join("results"),
        staging.join(".omicsops"),
    ] {
        if !directory.exists() {
            continue;
        }
        if !reject_reparse(&directory).is_ok_and(|metadata| metadata.is_dir()) {
            return;
        }
        if directory != staging {
            let Ok(guard) = open_mutation_handle(&directory) else {
                return;
            };
            if !guard.metadata().is_ok_and(|metadata| metadata.is_dir()) {
                return;
            }
            nested_guards.push(guard);
        }
        let Ok(entries) = fs::read_dir(&directory) else {
            return;
        };
        for entry in entries {
            let Ok(entry) = entry else {
                return;
            };
            let path = entry.path();
            let Ok(metadata) = reject_reparse(&path) else {
                return;
            };
            if metadata.is_dir()
                && directory == staging
                && ["data", "results", ".omicsops"]
                    .iter()
                    .any(|name| path == staging.join(name))
            {
                // Its handle is acquired before enumerating that directory.
            } else if metadata.is_file()
                && path
                    .strip_prefix(staging)
                    .ok()
                    .and_then(Path::to_str)
                    .is_some_and(|relative| allowed.contains(relative.replace('\\', "/").as_str()))
            {
                let Ok(file) = open_mutation_handle(&path) else {
                    return;
                };
                if !file.metadata().is_ok_and(|metadata| metadata.is_file()) {
                    return;
                }
                files.push(file);
            } else {
                return;
            }
        }
    }
    for file in files {
        if delete_by_handle(&file).is_err() {
            return;
        }
    }
    for directory in nested_guards {
        if delete_by_handle(&directory).is_err() {
            return;
        }
    }
    let _ = delete_by_handle(&root_guard);
}

fn delete_by_handle(file: &File) -> Result<(), String> {
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Storage::FileSystem::*;
        let info = FILE_DISPOSITION_INFO { DeleteFile: true };
        if unsafe {
            SetFileInformationByHandle(
                file.as_raw_handle(),
                FileDispositionInfo,
                (&info as *const FILE_DISPOSITION_INFO).cast(),
                std::mem::size_of::<FILE_DISPOSITION_INFO>() as u32,
            )
        } == 0
        {
            return Err("example_cleanup_failed".into());
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let _ = file;
        Err("example_platform_unavailable".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use omicsops_dto::{
        OfflineExampleAvailability, OfflineExampleFileAvailability, OfflineExampleUnavailableReason,
    };

    #[cfg(windows)]
    async fn view_setup(
        parent: &Path,
        db: &Path,
    ) -> (omicsops_store::Store, PreparedOfflineExampleProject) {
        let store = omicsops_store::Store::open(db).await.unwrap();
        let prepared = materialize_offline_example(
            &request(parent),
            &load_bundled_example().unwrap(),
            write_new_example_file,
        )
        .unwrap();
        store
            .save_offline_example_project(&prepared.project, &prepared.marker)
            .await
            .unwrap();
        (store, prepared)
    }

    async fn table_counts(store: &omicsops_store::Store) -> BTreeMap<String, i64> {
        let tables: Vec<String> =
            sqlx::query_scalar("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name")
                .fetch_all(store.pool())
                .await
                .unwrap();
        let mut counts = BTreeMap::new();
        for table in tables {
            let count: i64 = sqlx::query_scalar(&format!(
                "SELECT COUNT(*) FROM \"{}\"",
                table.replace('"', "\"\"")
            ))
            .fetch_one(store.pool())
            .await
            .unwrap();
            counts.insert(table, count);
        }
        counts
    }

    #[tokio::test]
    async fn offline_example_view_without_marker_never_consumes_disk_receipt() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join(".omicsops")).unwrap();
        fs::write(dir.path().join(RECEIPT), b"not a receipt").unwrap();
        let store = omicsops_store::Store::open_in_memory().await.unwrap();
        let project = Project::new(
            Uuid::new_v4(),
            "ordinary",
            dir.path().to_str().unwrap(),
            ProjectTemplate::Blank,
            chrono::Utc::now(),
        );
        store.save_project(&project).await.unwrap();
        let without_marker = load_offline_example_workspace(&store, project.id)
            .await
            .unwrap();
        assert!(without_marker.is_none());
        assert!(
            load_offline_example_workspace(&store, Uuid::new_v4())
                .await
                .unwrap()
                .is_none()
        );
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn offline_example_view_repeated_and_reopened_reads_write_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("test.sqlite");
        let (store, prepared) = view_setup(dir.path(), &db).await;
        let before_counts = table_counts(&store).await;
        let root = Path::new(&prepared.project.local_root);
        let before: Vec<_> = FIXED_FILES
            .into_iter()
            .chain([RECEIPT, PROJECT_MANIFEST])
            .map(|p| {
                (
                    p,
                    fs::read(root.join(p)).unwrap(),
                    fs::metadata(root.join(p)).unwrap().modified().unwrap(),
                )
            })
            .collect();
        for table in before_counts
            .keys()
            .filter(|table| !table.starts_with("sqlite_"))
        {
            for operation in ["INSERT", "UPDATE", "DELETE"] {
                sqlx::query(&format!("CREATE TRIGGER \"no_view_{table}_{operation}\" BEFORE {operation} ON \"{table}\" BEGIN SELECT RAISE(ABORT, 'view must not write'); END")).execute(store.pool()).await.unwrap();
            }
        }
        sqlx::query("PRAGMA query_only = ON")
            .execute(store.pool())
            .await
            .unwrap();
        for _ in 0..2 {
            let view = load_offline_example_workspace(&store, prepared.project.id)
                .await
                .unwrap()
                .unwrap();
            assert_eq!(view.availability, OfflineExampleAvailability::Available);
            assert_eq!(view.entries.len(), 4);
            assert_eq!(view.files.len(), 7);
            assert!(
                view.files
                    .iter()
                    .all(|file| file.availability == OfflineExampleFileAvailability::Available)
            );
        }
        assert_eq!(before_counts, table_counts(&store).await);
        store.pool().close().await;
        let reopened = omicsops_store::Store::open(&db).await.unwrap();
        let reopened_counts = table_counts(&reopened).await;
        sqlx::query("PRAGMA query_only = ON")
            .execute(reopened.pool())
            .await
            .unwrap();
        assert!(
            load_offline_example_workspace(&reopened, prepared.project.id)
                .await
                .unwrap()
                .is_some()
        );
        assert_eq!(before_counts, reopened_counts);
        assert_eq!(reopened_counts, table_counts(&reopened).await);
        for (path, bytes, modified) in before {
            assert_eq!(fs::read(root.join(path)).unwrap(), bytes);
            assert_eq!(
                fs::metadata(root.join(path)).unwrap().modified().unwrap(),
                modified
            );
        }
        check_tree(root).unwrap();
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn offline_example_view_marker_survives_missing_invalid_and_mismatched_receipt() {
        for mutation in [
            "missing", "json", "project", "request", "hash", "files", "large", "utf8",
        ] {
            let dir = tempfile::tempdir().unwrap();
            let (store, prepared) = view_setup(dir.path(), &dir.path().join("test.sqlite")).await;
            let path = Path::new(&prepared.project.local_root).join(RECEIPT);
            let reason = match mutation {
                "missing" => {
                    fs::remove_file(&path).unwrap();
                    OfflineExampleUnavailableReason::ReceiptMissing
                }
                "json" => {
                    fs::write(&path, b"not json").unwrap();
                    OfflineExampleUnavailableReason::ReceiptInvalid
                }
                "large" => {
                    fs::write(&path, vec![b'x'; MAX_FILE_BYTES + 1]).unwrap();
                    OfflineExampleUnavailableReason::ReceiptInvalid
                }
                "utf8" => {
                    fs::write(&path, [0xff]).unwrap();
                    OfflineExampleUnavailableReason::ReceiptInvalid
                }
                _ => {
                    let mut receipt = expected_receipt(
                        prepared.marker.request_id,
                        prepared.project.id,
                        &load_bundled_example().unwrap(),
                    )
                    .unwrap();
                    match mutation {
                        "project" => receipt.project_id = Uuid::new_v4(),
                        "request" => receipt.request_id = Uuid::new_v4(),
                        "hash" => receipt.fixture_sha256 = "0".repeat(64),
                        "files" => receipt.files[0].relative_path = "outside.txt".into(),
                        _ => unreachable!(),
                    }
                    fs::write(&path, serde_json::to_vec(&receipt).unwrap()).unwrap();
                    if matches!(mutation, "project" | "request") {
                        OfflineExampleUnavailableReason::IdentityMismatch
                    } else {
                        OfflineExampleUnavailableReason::ReceiptInvalid
                    }
                }
            };
            let bytes = fs::read(&path).ok();
            let after_receipt_removed = load_offline_example_workspace(&store, prepared.project.id)
                .await
                .unwrap();
            let view = after_receipt_removed.unwrap();
            assert_eq!(view.availability, OfflineExampleAvailability::Unavailable);
            assert_eq!(view.unavailable_reason, Some(reason));
            assert_eq!(view.source_kind, OfflineExampleSourceKind::Synthetic);
            assert!(!view.disclaimer.zh.is_empty() && !view.disclaimer.en.is_empty());
            assert!(view.entries.is_empty() && view.files.is_empty());
            assert_eq!(fs::read(&path).ok(), bytes);
        }
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn offline_example_view_missing_and_rebound_root_keep_identity() {
        for missing in [true, false] {
            let dir = tempfile::tempdir().unwrap();
            let (store, prepared) = view_setup(dir.path(), &dir.path().join("test.sqlite")).await;
            if missing {
                fs::rename(&prepared.project.local_root, dir.path().join("moved")).unwrap();
            } else {
                let mut project = prepared.project.clone();
                project.local_root = dir.path().to_str().unwrap().into();
                store.save_project(&project).await.unwrap();
            }
            let view = load_offline_example_workspace(&store, prepared.project.id)
                .await
                .unwrap()
                .unwrap();
            assert_eq!(view.availability, OfflineExampleAvailability::Unavailable);
            assert_eq!(
                view.unavailable_reason,
                Some(if missing {
                    OfflineExampleUnavailableReason::RootUnavailable
                } else {
                    OfflineExampleUnavailableReason::IdentityMismatch
                })
            );
            assert!(view.entries.is_empty() && view.files.is_empty());
        }
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn offline_example_view_changed_missing_large_and_invalid_utf8_are_not_repaired() {
        let dir = tempfile::tempdir().unwrap();
        let (store, prepared) = view_setup(dir.path(), &dir.path().join("test.sqlite")).await;
        let root = Path::new(&prepared.project.local_root);
        let edited_path = root.join("report.en.md");
        let edited_bytes = b"# Changed\n<img src=\"https://invalid.test/x\"><script>literal()</script>\n![image](https://invalid.test/y)\ntoken=controlled-secret\n-----BEGIN PRIVATE KEY-----\ncontrolled-key-value\n-----END PRIVATE KEY-----\n";
        fs::write(&edited_path, edited_bytes).unwrap();
        fs::remove_file(root.join("results/summary.csv")).unwrap();
        fs::write(root.join("README.md"), vec![b'x'; MAX_FILE_BYTES + 1]).unwrap();
        fs::write(root.join("report.zh.md"), [0xff]).unwrap();
        let before = table_counts(&store).await;
        for _ in 0..2 {
            let view = load_offline_example_workspace(&store, prepared.project.id)
                .await
                .unwrap()
                .unwrap();
            assert_eq!(view.availability, OfflineExampleAvailability::Available);
            let file = |path: &str| {
                view.files
                    .iter()
                    .find(|file| file.relative_path == path)
                    .unwrap()
            };
            let edited_file = file("report.en.md");
            assert_eq!(
                edited_file.availability,
                OfflineExampleFileAvailability::Changed
            );
            assert_eq!(edited_file.current_sha256, Some(sha256(edited_bytes)));
            let preview = edited_file.preview_text.as_ref().unwrap();
            assert!(
                preview.contains("<img src=\"https://invalid.test/x\"><script>literal()</script>")
            );
            assert!(preview.contains("![image](https://invalid.test/y)"));
            assert!(
                !preview.contains("controlled-secret") && !preview.contains("controlled-key-value")
            );
            assert!(preview.contains("[REDACTED]"));
            assert_eq!(
                file("results/summary.csv").availability,
                OfflineExampleFileAvailability::Missing
            );
            for path in ["README.md", "report.zh.md"] {
                assert_eq!(
                    file(path).availability,
                    OfflineExampleFileAvailability::Unavailable
                );
                assert!(file(path).preview_text.is_none());
                assert!(file(path).current_sha256.is_none());
            }
        }
        assert_eq!(table_counts(&store).await, before);
        assert_eq!(std::fs::read(&edited_path).unwrap(), edited_bytes);
        assert!(!root.join("results/summary.csv").exists());
        assert_eq!(
            fs::metadata(root.join("README.md")).unwrap().len(),
            (MAX_FILE_BYTES + 1) as u64
        );
        assert_eq!(fs::read(root.join("report.zh.md")).unwrap(), [0xff]);
        store.pool().close().await;
        let reopened = omicsops_store::Store::open(dir.path().join("test.sqlite"))
            .await
            .unwrap();
        let view = load_offline_example_workspace(&reopened, prepared.project.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            view.files
                .iter()
                .find(|file| file.relative_path == "report.en.md")
                .unwrap()
                .availability,
            OfflineExampleFileAvailability::Changed
        );
        assert_eq!(
            view.files
                .iter()
                .find(|file| file.relative_path == "results/summary.csv")
                .unwrap()
                .availability,
            OfflineExampleFileAvailability::Missing
        );
        assert_eq!(table_counts(&reopened).await, before);
        assert_eq!(std::fs::read(&edited_path).unwrap(), edited_bytes);
        assert!(!root.join("results/summary.csv").exists());
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn offline_example_view_unknown_and_corrupt_markers_are_fixed_errors() {
        for value in [
            serde_json::json!({}),
            serde_json::json!({"example_id":"unknown"}),
        ] {
            let dir = tempfile::tempdir().unwrap();
            let (store, prepared) = view_setup(dir.path(), &dir.path().join("test.sqlite")).await;
            let mut marker = serde_json::to_value(&prepared.marker).unwrap();
            if value.as_object().unwrap().is_empty() {
                marker = value;
            } else {
                marker["example_id"] = value["example_id"].clone();
            }
            store
                .put_json(
                    "offline_example_workspace_v1",
                    &prepared.project.id.to_string(),
                    &marker,
                )
                .await
                .unwrap();
            assert_eq!(
                load_offline_example_workspace(&store, prepared.project.id)
                    .await
                    .unwrap_err(),
                "offline_example_marker_invalid"
            );
        }
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn offline_example_view_junction_files_and_receipt_fail_closed() {
        for (relative, outside_file) in [("data", true), ("data", false), (".omicsops", false)] {
            let dir = tempfile::tempdir().unwrap();
            let outside = tempfile::tempdir().unwrap();
            let (store, prepared) = view_setup(dir.path(), &dir.path().join("test.sqlite")).await;
            let path = Path::new(&prepared.project.local_root).join(relative);
            fs::rename(&path, outside.path().join("original")).unwrap();
            if outside_file {
                fs::write(outside.path().join("observations.csv"), b"outside bytes").unwrap();
            }
            let status = std::process::Command::new("cmd")
                .args(["/C", "mklink", "/J"])
                .arg(&path)
                .arg(outside.path())
                .output()
                .unwrap();
            assert!(status.status.success());
            let view = load_offline_example_workspace(&store, prepared.project.id)
                .await
                .unwrap()
                .unwrap();
            if relative == "data" {
                let file = view
                    .files
                    .iter()
                    .find(|file| file.relative_path == "data/observations.csv")
                    .unwrap();
                assert_eq!(
                    file.availability,
                    OfflineExampleFileAvailability::Unavailable
                );
                assert!(file.preview_text.is_none());
            } else {
                assert_eq!(view.availability, OfflineExampleAvailability::Unavailable);
                assert_eq!(
                    view.unavailable_reason,
                    Some(OfflineExampleUnavailableReason::ReceiptInvalid)
                );
                assert!(view.entries.is_empty() && view.files.is_empty());
            }
            fs::remove_dir(&path).unwrap();
        }
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn offline_example_view_locked_file_and_replaced_root_are_unavailable() {
        use std::os::windows::fs::OpenOptionsExt;
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let (store, prepared) = view_setup(dir.path(), &dir.path().join("test.sqlite")).await;
        let root = Path::new(&prepared.project.local_root);
        let locked = OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(root.join("README.md"))
            .unwrap();
        let view = load_offline_example_workspace(&store, prepared.project.id)
            .await
            .unwrap()
            .unwrap();
        let file = view
            .files
            .iter()
            .find(|file| file.relative_path == "README.md")
            .unwrap();
        assert_eq!(
            file.availability,
            OfflineExampleFileAvailability::Unavailable
        );
        assert!(file.preview_text.is_none());
        drop(locked);
        assert_eq!(
            load_offline_example_workspace(&store, prepared.project.id)
                .await
                .unwrap()
                .unwrap()
                .files
                .iter()
                .find(|file| file.relative_path == "README.md")
                .unwrap()
                .availability,
            OfflineExampleFileAvailability::Available
        );
        fs::rename(root, outside.path().join("original")).unwrap();
        let status = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(root)
            .arg(outside.path().join("original"))
            .output()
            .unwrap();
        assert!(status.status.success());
        let view = load_offline_example_workspace(&store, prepared.project.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(view.availability, OfflineExampleAvailability::Unavailable);
        assert_eq!(
            view.unavailable_reason,
            Some(OfflineExampleUnavailableReason::RootUnavailable)
        );
        assert!(view.entries.is_empty() && view.files.is_empty());
        fs::remove_dir(root).unwrap();
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn offline_example_view_create_registration_error_retains_recoverable_final() {
        let dir = tempfile::tempdir().unwrap();
        let store = omicsops_store::Store::open_in_memory().await.unwrap();
        let request = request(dir.path());
        sqlx::query("CREATE TRIGGER fail_example BEFORE INSERT ON app_objects BEGIN SELECT RAISE(ABORT, 'injected'); END").execute(store.pool()).await.unwrap();
        assert_eq!(
            create_offline_example_for_repository(&store, &request)
                .await
                .unwrap_err(),
            "example_registration_failed"
        );
        assert!(store.list_projects().await.unwrap().is_empty());
        assert!(
            store
                .get_offline_example_project_marker(request.request_id)
                .await
                .unwrap()
                .is_none()
        );
        let root = dir
            .path()
            .join(format!("omicsops-example-{}", request.request_id));
        assert!(root.join(RECEIPT).exists());
        let before = fs::read(root.join(RECEIPT)).unwrap();
        sqlx::query("DROP TRIGGER fail_example")
            .execute(store.pool())
            .await
            .unwrap();
        let created = create_offline_example_for_repository(&store, &request)
            .await
            .unwrap();
        assert_eq!(created.project_id, request.request_id);
        assert_eq!(
            created.workspace.availability,
            OfflineExampleAvailability::Available
        );
        let mut project = store
            .get_project(created.project_id)
            .await
            .unwrap()
            .unwrap();
        project.name = "User metadata".into();
        store.save_project(&project).await.unwrap();
        assert_eq!(
            create_offline_example_for_repository(&store, &request)
                .await
                .unwrap(),
            created
        );
        assert_eq!(
            store
                .get_project(created.project_id)
                .await
                .unwrap()
                .unwrap()
                .name,
            "User metadata"
        );
        assert_eq!(fs::read(root.join(RECEIPT)).unwrap(), before);
        let counts = table_counts(&store).await;
        for table in [
            "conversation_records",
            "runs",
            "agent_events_v4",
            "approvals_v3",
            "scientific_evidence_v4",
        ] {
            assert_eq!(counts[table], 0);
        }
    }
    use omicsops_dto::CreateOfflineExampleWorkspaceRequest;
    use std::{fs, path::Path};
    use uuid::Uuid;

    type BoundaryHook = (&'static str, Box<dyn FnOnce(&Path)>);
    thread_local! {
        static BOUNDARY_HOOK: std::cell::RefCell<Option<BoundaryHook>> = const {
            std::cell::RefCell::new(None)
        };
    }

    pub(super) fn mutation_boundary(name: &str, path: &Path) {
        let hook = BOUNDARY_HOOK.with(|slot| {
            let mut slot = slot.borrow_mut();
            if slot.as_ref().is_some_and(|hook| hook.0 == name) {
                slot.take()
            } else {
                None
            }
        });
        if let Some((_, hook)) = hook {
            hook(path);
        }
    }

    #[cfg(windows)]
    #[test]
    fn offline_example_create_publication_rejects_boundary_source_replacement() {
        let dir = tempfile::tempdir().unwrap();
        let parent = canonical_local(dir.path()).unwrap();
        let original = parent.join("original.staging");
        let replacement = std::rc::Rc::new(std::cell::RefCell::new(None));
        let observed = replacement.clone();
        let saved = original.clone();
        BOUNDARY_HOOK.with(|slot| {
            *slot.borrow_mut() = Some((
                "publish",
                Box::new(move |staging| {
                    let identity = directory_identity(staging).unwrap();
                    fs::rename(staging, &saved).unwrap();
                    fs::create_dir(staging).unwrap();
                    fs::write(staging.join("README.md"), b"user-owned replacement").unwrap();
                    *observed.borrow_mut() = Some((
                        staging.to_owned(),
                        identity,
                        directory_identity(staging).unwrap(),
                    ));
                }),
            ));
        });
        let request = request(&parent);
        let result = materialize_offline_example(
            &request,
            &load_bundled_example().unwrap(),
            write_new_example_file,
        );
        assert!(result.is_err());
        let (staging, identity, replacement_identity) = replacement.borrow_mut().take().unwrap();
        assert_eq!(
            fs::read(staging.join("README.md")).unwrap(),
            b"user-owned replacement"
        );
        assert_eq!(directory_identity(&staging).unwrap(), replacement_identity);
        assert_eq!(directory_identity(&original).unwrap(), identity);
        assert!(
            !parent
                .join(format!("omicsops-example-{}", request.request_id))
                .exists()
        );
    }

    #[cfg(windows)]
    #[test]
    fn offline_example_create_cleanup_rejects_boundary_guard_replacement() {
        let dir = tempfile::tempdir().unwrap();
        let parent = canonical_local(dir.path()).unwrap();
        let staging = parent.join("owned.staging");
        let original = parent.join("original.staging");
        fs::create_dir(&staging).unwrap();
        fs::write(staging.join("README.md"), b"original owned bytes").unwrap();
        let identity = directory_identity(&staging).unwrap();
        let trusted = retain_staging_identity(&staging).unwrap();
        let replacement = std::rc::Rc::new(std::cell::Cell::new(None));
        let observed = replacement.clone();
        let saved = original.clone();
        BOUNDARY_HOOK.with(|slot| {
            *slot.borrow_mut() = Some((
                "cleanup",
                Box::new(move |staging| {
                    fs::rename(staging, &saved).unwrap();
                    fs::create_dir(staging).unwrap();
                    fs::write(staging.join("README.md"), b"user-owned replacement").unwrap();
                    observed.set(Some(directory_identity(staging).unwrap()));
                }),
            ));
        });
        cleanup_staging(&parent, &staging, &trusted);
        assert_eq!(
            fs::read(staging.join("README.md")).unwrap(),
            b"user-owned replacement"
        );
        assert_eq!(
            fs::read(original.join("README.md")).unwrap(),
            b"original owned bytes"
        );
        assert_eq!(
            directory_identity(&staging).unwrap(),
            replacement.get().unwrap()
        );
        assert_eq!(directory_identity(&original).unwrap(), identity);
    }

    fn request(parent: &Path) -> CreateOfflineExampleWorkspaceRequest {
        CreateOfflineExampleWorkspaceRequest {
            request_id: Uuid::new_v4(),
            example_id: "synthetic-evidence-report-v1".into(),
            parent_directory: parent.to_string_lossy().into_owned(),
        }
    }

    #[test]
    fn offline_example_create_fixture_content_and_budgets() {
        let fixture = load_bundled_example().unwrap();
        assert_eq!(fixture.files.len(), 7);
        assert_eq!(
            fixture.files["data/observations.csv"],
            "group,value\nA,2\nA,4\nB,5\nB,7\n"
        );
        assert_eq!(
            fixture.files["results/summary.csv"],
            "measure,value\nA_mean,3\nB_mean,6\nB_minus_A,3\n"
        );
        for name in ["README.md", "report.zh.md", "report.en.md"] {
            assert!(fixture.files[name].contains("合成数据，预置结果，未在本机执行"));
            assert!(
                fixture.files[name]
                    .contains("Synthetic data, preset results, not executed on this computer")
            );
        }
        assert_eq!(fixture.files["LICENSE.txt"], include_str!("../../LICENSE"));
        let source: serde_json::Value =
            serde_json::from_str(&fixture.files["SOURCE.json"]).unwrap();
        assert_eq!(source["license_spdx"], "AGPL-3.0-only");
        assert_eq!(source["files"].as_object().unwrap().len(), 6);
        assert!(source["files"].get("SOURCE.json").is_none());
        for (path, content) in &fixture.files {
            assert!(content.len() <= 64 * 1024);
            if path != "SOURCE.json" {
                assert_eq!(source["files"][path], sha256(content.as_bytes()));
            }
        }
        assert!(fixture.files.values().map(String::len).sum::<usize>() <= 512 * 1024);
        assert!(
            fixture
                .entries
                .iter()
                .all(|e| !e.text.zh.is_empty() && !e.text.en.is_empty())
        );
    }

    #[test]
    #[cfg(windows)]
    fn offline_example_create_fixed_files_retry_and_new_request() {
        let dir = tempfile::tempdir().unwrap();
        let request = request(dir.path());
        let fixture = load_bundled_example().unwrap();
        let first =
            materialize_offline_example(&request, &fixture, write_new_example_file).unwrap();
        let child = Path::new(&first.project.local_root);
        assert_eq!(
            child.file_name().unwrap(),
            format!("omicsops-example-{}", request.request_id).as_str()
        );
        assert!(child.starts_with(dir.path()));
        let repeated =
            materialize_offline_example(&request, &fixture, write_new_example_file).unwrap();
        assert_eq!(first.project, repeated.project);
        assert_eq!(first.marker, repeated.marker);
        let mut other = request.clone();
        other.request_id = Uuid::new_v4();
        let other = materialize_offline_example(&other, &fixture, write_new_example_file).unwrap();
        assert_ne!(first.project.id, other.project.id);
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 2);
    }

    #[test]
    fn offline_example_create_rejects_fixture_paths_and_limits() {
        let dir = tempfile::tempdir().unwrap();
        let request = request(dir.path());
        for path in [
            "../escape",
            "/absolute",
            "C:/drive",
            "//host/share",
            "data//extra",
            "data\\extra",
            "NUL",
            "extra.txt",
        ] {
            let mut fixture = load_bundled_example().unwrap();
            fixture.files.insert(path.into(), "bad".into());
            assert!(
                materialize_offline_example(&request, &fixture, write_new_example_file).is_err(),
                "{path}"
            );
        }
        let mut fixture = load_bundled_example().unwrap();
        fixture
            .files
            .insert("README.md".into(), "x".repeat(64 * 1024 + 1));
        assert!(materialize_offline_example(&request, &fixture, write_new_example_file).is_err());
        assert!(parse_fixture_manifest(&" ".repeat(256 * 1024 + 1)).is_err());
        assert!(parse_fixture_manifest("{\"schema_version\":1,\"unexpected\":true}").is_err());
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
    }

    #[test]
    fn offline_example_create_rejects_parent_spellings() {
        let fixture = load_bundled_example().unwrap();
        let dir = tempfile::tempdir().unwrap();
        for parent in [
            "\\\\host\\share",
            "//host/share",
            "\\\\?\\C:\\",
            "\\\\.\\C:\\",
            "relative",
            "C:\\temp\\..\\temp",
            "C:\\temp\\NUL",
            "",
        ] {
            let mut request = request(dir.path());
            request.parent_directory = parent.into();
            assert!(
                materialize_offline_example(&request, &fixture, write_new_example_file).is_err(),
                "{parent}"
            );
        }
    }

    #[test]
    fn offline_example_create_conflict_preserves_user_files() {
        let dir = tempfile::tempdir().unwrap();
        let request = request(dir.path());
        let fixture = load_bundled_example().unwrap();
        let child = dir
            .path()
            .join(format!("omicsops-example-{}", request.request_id));
        fs::create_dir(&child).unwrap();
        let user_file = child.join("notes.txt");
        let original_user_bytes = b"user-owned notes";
        fs::write(&user_file, original_user_bytes).unwrap();
        assert!(materialize_offline_example(&request, &fixture, write_new_example_file).is_err());
        assert_eq!(fs::read(&user_file).unwrap(), original_user_bytes);
    }

    #[test]
    #[cfg(windows)]
    fn offline_example_create_retry_rejects_edits_extras_and_identity_changes() {
        for relative in [
            "README.md",
            ".omicsops/project.json",
            ".omicsops/offline-example.json",
            "extra.txt",
        ] {
            let dir = tempfile::tempdir().unwrap();
            let request = request(dir.path());
            let fixture = load_bundled_example().unwrap();
            let first =
                materialize_offline_example(&request, &fixture, write_new_example_file).unwrap();
            let path = Path::new(&first.project.local_root).join(relative);
            fs::write(&path, b"user edit").unwrap();
            assert!(
                materialize_offline_example(&request, &fixture, write_new_example_file).is_err(),
                "{relative}"
            );
            assert_eq!(fs::read(&path).unwrap(), b"user edit");
        }
    }

    #[test]
    fn offline_example_create_writer_failure_cleans_only_own_staging() {
        fn fail(path: &Path, bytes: &[u8]) -> Result<(), String> {
            if path.ends_with("summary.csv") {
                return Err("injected".into());
            }
            write_new_example_file(path, bytes)
        }
        let dir = tempfile::tempdir().unwrap();
        let request = request(dir.path());
        fs::write(dir.path().join("user.txt"), b"preserve").unwrap();
        assert!(
            materialize_offline_example(&request, &load_bundled_example().unwrap(), fail).is_err()
        );
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
        assert_eq!(fs::read(dir.path().join("user.txt")).unwrap(), b"preserve");
    }

    #[tokio::test]
    #[cfg(windows)]
    async fn offline_example_create_registration_failure_preserves_and_recovers() {
        let dir = tempfile::tempdir().unwrap();
        let db = tempfile::tempdir().unwrap();
        let store = omicsops_store::Store::open(db.path().join("test.sqlite"))
            .await
            .unwrap();
        let request = request(dir.path());
        let fixture = load_bundled_example().unwrap();
        let first =
            materialize_offline_example(&request, &fixture, write_new_example_file).unwrap();
        sqlx::query("CREATE TRIGGER fail_example BEFORE INSERT ON app_objects BEGIN SELECT RAISE(ABORT, 'injected'); END").execute(store.pool()).await.unwrap();
        let result = store
            .save_offline_example_project(&first.project, &first.marker)
            .await
            .map_err(|_| "example_registration_failed");
        assert_eq!(result.unwrap_err(), "example_registration_failed");
        assert!(
            Path::new(&first.project.local_root)
                .join("SOURCE.json")
                .exists()
        );
        assert!(store.get_project(first.project.id).await.unwrap().is_none());
        assert!(
            store
                .get_offline_example_project_marker(first.project.id)
                .await
                .unwrap()
                .is_none()
        );
        sqlx::query("DROP TRIGGER fail_example")
            .execute(store.pool())
            .await
            .unwrap();
        let repeated =
            materialize_offline_example(&request, &fixture, write_new_example_file).unwrap();
        assert_eq!(first.project, repeated.project);
        store
            .save_offline_example_project(&repeated.project, &repeated.marker)
            .await
            .unwrap();
        store
            .save_offline_example_project(&repeated.project, &repeated.marker)
            .await
            .unwrap();
        assert_eq!(store.list_projects().await.unwrap().len(), 1);
    }

    #[cfg(windows)]
    #[test]
    fn offline_example_create_windows_junction_rejected() {
        use std::process::Command;
        let dir = tempfile::tempdir().unwrap();
        let target = tempfile::tempdir().unwrap();
        let junction = dir.path().join("junction");
        let status = Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(&junction)
            .arg(target.path())
            .output()
            .unwrap();
        assert!(status.status.success(), "junction fixture creation failed");
        let result = materialize_offline_example(
            &request(&junction),
            &load_bundled_example().unwrap(),
            write_new_example_file,
        );
        assert!(result.is_err());
        assert_eq!(fs::read_dir(target.path()).unwrap().count(), 0);
        fs::remove_dir(&junction).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn offline_example_create_publication_never_replaces_existing_target() {
        let parent = tempfile::tempdir().unwrap();
        let staging = parent.path().join("staging");
        let child = parent.path().join("child");
        fs::create_dir(&staging).unwrap();
        fs::write(staging.join("new.txt"), b"new").unwrap();
        let trusted = retain_staging_identity(&staging).unwrap();
        fs::create_dir(&child).unwrap();
        assert!(publish_directory(&staging, &child, &trusted).is_err());
        assert_eq!(fs::read_dir(&child).unwrap().count(), 0);
        fs::write(child.join("user.txt"), b"user").unwrap();
        assert!(publish_directory(&staging, &child, &trusted).is_err());
        assert_eq!(fs::read(child.join("user.txt")).unwrap(), b"user");
        assert!(staging.join("new.txt").exists());
    }

    #[cfg(windows)]
    #[test]
    fn offline_example_create_publication_handles_late_target_without_replacement() {
        for populated in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let parent = canonical_local(dir.path()).unwrap();
            let staging = parent.join("owned.staging");
            let child = parent.join("child");
            fs::create_dir(&staging).unwrap();
            fs::write(staging.join("README.md"), b"original owned bytes").unwrap();
            let trusted = retain_staging_identity(&staging).unwrap();
            let identity = retained_identity(&trusted).unwrap();
            let captured = staging.clone();
            BOUNDARY_HOOK.with(|slot| {
                *slot.borrow_mut() = Some((
                    "rename",
                    Box::new(move |child| {
                        // At the actual Win32 operation boundary the source mutation
                        // handle prevents replacement, while the target can appear.
                        assert!(fs::rename(&captured, captured.with_extension("moved")).is_err());
                        fs::create_dir(child).unwrap();
                        if populated {
                            fs::write(child.join("README.md"), b"user-owned target").unwrap();
                        }
                    }),
                ));
            });
            assert!(publish_directory(&staging, &child, &trusted).is_err());
            assert_eq!(directory_identity(&staging).unwrap(), identity);
            assert_eq!(
                fs::read(staging.join("README.md")).unwrap(),
                b"original owned bytes"
            );
            if populated {
                assert_eq!(
                    fs::read(child.join("README.md")).unwrap(),
                    b"user-owned target"
                );
            } else {
                assert_eq!(fs::read_dir(&child).unwrap().count(), 0);
            }
        }
    }

    #[cfg(windows)]
    #[test]
    fn offline_example_create_cleanup_retains_replaced_staging_identity() {
        let dir = tempfile::tempdir().unwrap();
        let parent = canonical_local(dir.path()).unwrap();
        let staging = parent.join("owned.staging");
        fs::create_dir(&staging).unwrap();
        let trusted = retain_staging_identity(&staging).unwrap();
        fs::rename(&staging, parent.join("original.staging")).unwrap();
        fs::create_dir(&staging).unwrap();
        fs::write(staging.join("README.md"), b"user-owned replacement").unwrap();
        cleanup_staging(&parent, &staging, &trusted);
        assert_eq!(
            fs::read(staging.join("README.md")).unwrap(),
            b"user-owned replacement"
        );
    }

    #[cfg(windows)]
    #[test]
    fn offline_example_create_internal_reader_rejects_unknown_large_and_invalid_utf8() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join(".omicsops")).unwrap();
        let receipt = dir.path().join(RECEIPT);
        fs::write(&receipt, b"{}\n").unwrap();
        assert_eq!(read_internal_file(dir.path(), RECEIPT).unwrap(), b"{}\n");
        assert!(read_internal_file(dir.path(), ".omicsops/private.json").is_err());
        fs::write(&receipt, vec![b'x'; MAX_FILE_BYTES + 1]).unwrap();
        assert!(read_internal_file(dir.path(), RECEIPT).is_err());
        fs::write(&receipt, [0xff, 0xfe]).unwrap();
        assert!(read_internal_file(dir.path(), RECEIPT).is_err());
    }

    #[cfg(windows)]
    #[test]
    fn offline_example_create_retry_rejects_nested_junction() {
        for relative in ["data", ".omicsops"] {
            let dir = tempfile::tempdir().unwrap();
            let target = tempfile::tempdir().unwrap();
            let request = request(dir.path());
            let fixture = load_bundled_example().unwrap();
            let first =
                materialize_offline_example(&request, &fixture, write_new_example_file).unwrap();
            let path = Path::new(&first.project.local_root).join(relative);
            fs::rename(&path, target.path().join("original")).unwrap();
            let status = std::process::Command::new("cmd")
                .args(["/C", "mklink", "/J"])
                .arg(&path)
                .arg(target.path())
                .output()
                .unwrap();
            assert!(status.status.success());
            assert!(
                materialize_offline_example(&request, &fixture, write_new_example_file).is_err()
            );
            if relative == ".omicsops" {
                assert!(read_internal_file(Path::new(&first.project.local_root), RECEIPT).is_err());
            }
            assert!(target.path().join("original").is_dir());
            fs::remove_dir(&path).unwrap();
        }
    }

    #[cfg(windows)]
    #[test]
    fn offline_example_create_writer_cannot_overwrite_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("user.txt");
        fs::write(&path, b"original").unwrap();
        assert!(write_new_example_file(&path, b"replacement").is_err());
        assert_eq!(fs::read(&path).unwrap(), b"original");
    }

    #[cfg(windows)]
    #[test]
    fn offline_example_create_valid_json_identity_mutations_conflict() {
        for (relative, field, replacement) in [
            (
                PROJECT_MANIFEST,
                "created_at",
                serde_json::json!("2026-10-05T00:00:00Z"),
            ),
            (PROJECT_MANIFEST, "id", serde_json::json!(Uuid::new_v4())),
            (RECEIPT, "request_id", serde_json::json!(Uuid::new_v4())),
            (RECEIPT, "fixture_sha256", serde_json::json!("0".repeat(64))),
        ] {
            let dir = tempfile::tempdir().unwrap();
            let request = request(dir.path());
            let fixture = load_bundled_example().unwrap();
            let first =
                materialize_offline_example(&request, &fixture, write_new_example_file).unwrap();
            let path = Path::new(&first.project.local_root).join(relative);
            let mut value: serde_json::Value =
                serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
            value[field] = replacement;
            let edited = serde_json::to_vec_pretty(&value).unwrap();
            fs::write(&path, &edited).unwrap();
            assert!(
                materialize_offline_example(&request, &fixture, write_new_example_file).is_err()
            );
            assert_eq!(fs::read(&path).unwrap(), edited);
        }
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn offline_example_create_registration_conflict_preserves_existing_project() {
        let dir = tempfile::tempdir().unwrap();
        let db = tempfile::tempdir().unwrap();
        let store = omicsops_store::Store::open(db.path().join("test.sqlite"))
            .await
            .unwrap();
        let request = request(dir.path());
        let fixture = load_bundled_example().unwrap();
        let first =
            materialize_offline_example(&request, &fixture, write_new_example_file).unwrap();
        let mut ordinary = first.project.clone();
        ordinary.name = "ordinary user project".into();
        store.save_project(&ordinary).await.unwrap();
        assert!(
            store
                .save_offline_example_project(&first.project, &first.marker)
                .await
                .is_err()
        );
        assert_eq!(
            store.get_project(ordinary.id).await.unwrap(),
            Some(ordinary)
        );
        assert!(
            store
                .get_offline_example_project_marker(first.project.id)
                .await
                .unwrap()
                .is_none()
        );
        assert!(
            Path::new(&first.project.local_root)
                .join("README.md")
                .exists()
        );
    }
}
