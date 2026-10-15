//! Read-only observations, not an atomic DB/file snapshot or an approval token.
use crate::{
    commands::AppState,
    project_file_read::{BoundedFileRead, FileHashScope, FileReadError, ValidatedProjectFile},
    workspace_file_content::{inspect_small_workspace_text, safe_workspace_public_text, SmallWorkspaceTextIssue},
};
use omicsops_core::workspace::Project;
use omicsops_dto::*;
use omicsops_store::{Store, StoreError};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use tauri::State;

const MAX_ITEM: u64 = 512 * 1024;
const MAX_REQUEST: u64 = 4 * 1024 * 1024;

#[tauri::command]
pub async fn workspace_preview_project_migration(
    state: State<'_, AppState>,
    request: ProjectMigrationPreviewRequestV1,
) -> Result<ProjectMigrationPreviewV1, MigrationPreviewErrorV1> {
    preview_project_migration(&state.repository, &request, &chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)).await
}

pub(crate) async fn preview_project_migration(
    repository: &Store,
    request: &ProjectMigrationPreviewRequestV1,
    observed_at: &str,
) -> Result<ProjectMigrationPreviewV1, MigrationPreviewErrorV1> {
    preview_with_source_io(repository, request, observed_at, ValidatedProjectFile::open, ValidatedProjectFile::read_complete).await
}

fn error(code: MigrationPreviewErrorCodeV1) -> MigrationPreviewErrorV1 {
    use MigrationPreviewErrorCodeV1::*;
    MigrationPreviewErrorV1 { code, message: match code {
        InvalidSelection => "Migration preview selection is invalid.",
        NotFound => "Migration preview project was not found.",
        OwnershipMismatch => "Selected file backend does not belong to this project.",
        SourceChanged => "Selected source could not be fully verified or changed during observation.",
        LimitExceeded => "Migration preview selection exceeds the read limits.",
        StorageReadFailed => "Migration metadata could not be read.",
    }.into() }
}

fn storage_error(issue: StoreError) -> MigrationPreviewErrorV1 {
    let code = if matches!(issue, StoreError::InvalidInput(ref value) if value == "migration_preview_not_found") { MigrationPreviewErrorCodeV1::NotFound } else { MigrationPreviewErrorCodeV1::StorageReadFailed };
    error(code)
}

fn canonical_files(request: &ProjectMigrationPreviewRequestV1) -> Result<Vec<ProjectFileSelectionV1>, MigrationPreviewErrorV1> {
    if request.files.len() > 32 { return Err(error(MigrationPreviewErrorCodeV1::LimitExceeded)); }
    if request.project_id.is_nil() { return Err(error(MigrationPreviewErrorCodeV1::InvalidSelection)); }
    let mut files = BTreeMap::new();
    for mut file in request.files.clone() {
        let invalid = || error(MigrationPreviewErrorCodeV1::InvalidSelection);
        if file.backend_id.is_empty() || file.backend_id.len() > 128 || file.backend_id.chars().any(char::is_control) || !safe_workspace_public_text(&file.backend_id) || file.relative_path.len() > 1024 { return Err(invalid()); }
        file.relative_path = crate::composer_files::normalize_workspace_relative_path(&file.relative_path).map_err(|_| invalid())?;
        if file.relative_path.split('/').any(crate::project_file_read::is_reserved_component) { return Err(invalid()); }
        if let Some(hash) = &mut file.expected_full_sha256 {
            if hash.len() != 64 || !hash.bytes().all(|b| b.is_ascii_hexdigit()) { return Err(invalid()); }
            hash.make_ascii_lowercase();
        }
        let key = (file.backend_id.clone(), file.relative_path.clone());
        if files.insert(key, file.clone()).is_some_and(|prior| prior != file) { return Err(invalid()); }
    }
    Ok(files.into_values().collect())
}

// Only the internal owner binding is needed. Loading connection profiles would
// unnecessarily read private JSON; saved-reference lookup checks row existence.
fn validate_backend(project: &Project, backend: &str) -> Result<(), MigrationPreviewErrorV1> {
    if backend == "local" { return Ok(()); }
    let valid = backend.strip_prefix("ssh:").and_then(|value| uuid::Uuid::parse_str(value).ok()).is_some_and(|id| {
        backend == format!("ssh:{id}") && project.connection_id == Some(id) && project.remote_root.as_ref().is_some_and(|root| !root.trim().is_empty())
    });
    if valid { Ok(()) } else { Err(error(MigrationPreviewErrorCodeV1::OwnershipMismatch)) }
}

fn set_file_state(file: &mut MigrationFilePreviewV1, state: MigrationFileStateV1, reason: &'static str) {
    file.state = state; file.reason = reason.into(); file.hash = None;
}

fn file_issue(file: &mut MigrationFilePreviewV1, issue: FileReadError) -> Result<(), MigrationPreviewErrorV1> {
    use FileReadError::*;
    match issue {
        SourceChanged => return Err(error(MigrationPreviewErrorCodeV1::SourceChanged)),
        InvalidPath | UnsafeFile => set_file_state(file, MigrationFileStateV1::UnsafeFile, "Source file is unsafe."),
        NotRegular => set_file_state(file, MigrationFileStateV1::ReferenceOnly, "Only regular small text files can be read."),
        Unavailable => set_file_state(file, MigrationFileStateV1::Unreadable, "Source file is unavailable."),
        TooLarge => set_file_state(file, MigrationFileStateV1::ReferenceOnly, "Source exceeds the bounded read allowance."),
    }
    Ok(())
}

fn text_issue(file: &mut MigrationFilePreviewV1, issue: SmallWorkspaceTextIssue) {
    match issue {
        SmallWorkspaceTextIssue::SensitiveContent => set_file_state(file, MigrationFileStateV1::SensitiveContent, "Sensitive content is excluded."),
        SmallWorkspaceTextIssue::UnsupportedContent => set_file_state(file, MigrationFileStateV1::UnsupportedContent, "Unsupported content is not read or included."),
        SmallWorkspaceTextIssue::TooLarge => set_file_state(file, MigrationFileStateV1::ReferenceOnly, "Source exceeds the small text limit."),
    }
}

async fn preview_with_source_io(
    repository: &Store,
    request: &ProjectMigrationPreviewRequestV1,
    observed_at: &str,
    open_source: impl Fn(&Project, &str) -> Result<ValidatedProjectFile, FileReadError>,
    mut read_source: impl FnMut(&mut ValidatedProjectFile, usize) -> Result<BoundedFileRead, FileReadError>,
) -> Result<ProjectMigrationPreviewV1, MigrationPreviewErrorV1> {
    let selections = canonical_files(request)?;
    let observed_at = chrono::DateTime::parse_from_rfc3339(observed_at).map_err(|_| error(MigrationPreviewErrorCodeV1::InvalidSelection))?.with_timezone(&chrono::Utc).to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let context = repository.workspace_project_migration_metadata(request.project_id, &request.categories).await.map_err(storage_error)?;
    // Validate every identity before reading even the first selected source.
    for selection in &selections { validate_backend(&context.project, &selection.backend_id)?; }
    let mut files = Vec::new();
    let mut reserved_payload = 0u64;
    let mut known_bytes = 0u64;
    let mut unknown_count = 0;
    for selection in selections {
        let path = &selection.relative_path;
        let mut file = MigrationFilePreviewV1 { backend_id: selection.backend_id, relative_path: Some(path.clone()), size_bytes: None, hash: None, state: MigrationFileStateV1::ReferenceOnly, reason: "Saved remote reference; current source has not been observed.".into() };
        let mut limited = false;
        // The shared policy runs before payload. Sensitive filenames need no
        // file opening; other unsupported files may still report metadata.
        let candidate = inspect_small_workspace_text(path, b"");
        if !safe_workspace_public_text(path) || candidate == Err(SmallWorkspaceTextIssue::SensitiveContent) {
            text_issue(&mut file, SmallWorkspaceTextIssue::SensitiveContent);
            if !safe_workspace_public_text(path) { file.relative_path = None; }
        } else if file.backend_id != "local" {
            let (size, hash) = repository.workspace_file_recorded_reference(context.project.id, &file.backend_id, path).await.map_err(storage_error)?;
            file.size_bytes = size;
            file.hash = hash.map(|sha256| FileReferenceHashV1 { sha256, scope: FileReferenceHashScopeV1::RecordedFile });
        } else {
            match open_source(&context.project, path) {
                Err(issue) => file_issue(&mut file, issue)?,
                Ok(mut source) => {
                    let size = source.metadata().size_bytes;
                    file.size_bytes = Some(size);
                    if size > MAX_ITEM || size > MAX_REQUEST - reserved_payload {
                        limited = true;
                        set_file_state(&mut file, MigrationFileStateV1::ReferenceOnly, "Source exceeds the item or remaining request read budget.");
                    } else if let Err(issue) = candidate {
                        text_issue(&mut file, issue);
                    } else {
                        // Charge attempts before reading, without refunds on
                        // recoverable failures. Pass this SAME payload allowance
                        // to W02. Its EOF probe is separate from payload bytes.
                        reserved_payload += size;
                        match read_source(&mut source, size as usize) {
                            Err(issue) => file_issue(&mut file, issue)?,
                            Ok(read) => {
                                if !read.eof || read.hash_scope != FileHashScope::Full || read.size_bytes != size || read.read_bytes != size || read.bytes.len() as u64 != size { return Err(error(MigrationPreviewErrorCodeV1::SourceChanged)); }
                                match inspect_small_workspace_text(path, &read.bytes) {
                                    Err(issue) => text_issue(&mut file, issue),
                                    Ok(_) => { file.state = MigrationFileStateV1::SmallTextCandidate; file.reason = "Safe small text observed completely; body is not included.".into(); file.hash = Some(FileReferenceHashV1 { sha256: read.sha256, scope: FileReferenceHashScopeV1::FullFile }); }
                                }
                                // read.bytes is dropped here and never stored.
                            }
                        }
                    }
                }
            }
        }
        if selection.expected_full_sha256.as_ref().is_some_and(|expected| !file.hash.as_ref().is_some_and(|hash| hash.scope == FileReferenceHashScopeV1::FullFile && &hash.sha256 == expected)) {
            return Err(error(if limited { MigrationPreviewErrorCodeV1::LimitExceeded } else { MigrationPreviewErrorCodeV1::SourceChanged }));
        }
        if let Some(size) = file.size_bytes { known_bytes = known_bytes.checked_add(size).ok_or_else(|| error(MigrationPreviewErrorCodeV1::LimitExceeded))?; } else { unknown_count += 1; }
        files.push(file);
    }
    let mut preview = ProjectMigrationPreviewV1 { schema_version: 1, project: context.public_project, observed_at, scientific_revision: context.scientific_revision, categories: context.categories, files, selected_known_file_bytes: known_bytes, unknown_size_file_count: unknown_count, rebind: context.rebind, exclusions: context.exclusions, preview_sha256: String::new() };
    let mut stable = serde_json::to_value(&preview).map_err(|_| error(MigrationPreviewErrorCodeV1::StorageReadFailed))?;
    let object = stable.as_object_mut().ok_or_else(|| error(MigrationPreviewErrorCodeV1::StorageReadFailed))?;
    object.remove("observed_at"); object.remove("preview_sha256");
    preview.preview_sha256 = hex::encode(Sha256::digest(serde_json::to_vec(&stable).map_err(|_| error(MigrationPreviewErrorCodeV1::StorageReadFailed))?));
    Ok(preview)
}

#[cfg(test)]
mod tests {
    use super::*;
    use omicsops_core::workspace::{Project, ProjectTemplate};
    use omicsops_store::Store;
    use std::fs;
    use uuid::Uuid;

    async fn fixture() -> (tempfile::TempDir, Store, Project, ProjectMigrationPreviewRequestV1) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open_in_memory().await.unwrap();
        let project = Project::new(Uuid::new_v4(), "Synthetic", dir.path().to_str().unwrap(), ProjectTemplate::Blank, chrono::Utc::now());
        store.save_project(&project).await.unwrap();
        fs::write(dir.path().join("a.txt"), b"hello\n").unwrap();
        let request = ProjectMigrationPreviewRequestV1 { project_id: project.id, categories: vec![], files: vec![ProjectFileSelectionV1 { backend_id: "local".into(), relative_path: "a.txt".into(), expected_full_sha256: None }] };
        (dir, store, project, request)
    }

    #[tokio::test]
    async fn migration_preview_stable_digest_and_full_hash() {
        let (_dir, store, _project, request) = fixture().await;
        let first = preview_project_migration(&store, &request, "2026-10-15T00:00:00Z").await.unwrap();
        let second = preview_project_migration(&store, &request, "2026-10-16T00:00:00Z").await.unwrap();
        assert_eq!(first.preview_sha256, second.preview_sha256);
        assert_eq!(first.files[0].state, MigrationFileStateV1::SmallTextCandidate);
        assert_eq!(first.files[0].hash.as_ref().unwrap().sha256, "5891b5b522d5df086d0ff0b110fbd9d21bb4fc7163af34d08286a2e846f6be03");
        assert_eq!(first.files[0].hash.as_ref().unwrap().scope, FileReferenceHashScopeV1::FullFile);
        assert_eq!(first.selected_known_file_bytes, 6);
    }

    fn selections(request: &mut ProjectMigrationPreviewRequestV1, project: &Project, sizes: &[usize]) {
        request.files.clear();
        for (index, size) in sizes.iter().enumerate() {
            let path = format!("{index:02}.txt");
            fs::write(std::path::Path::new(&project.local_root).join(&path), vec![b'x'; *size]).unwrap();
            request.files.push(ProjectFileSelectionV1 { backend_id: "local".into(), relative_path: path, expected_full_sha256: None });
        }
    }

    #[tokio::test]
    async fn migration_preview_hash_budget_skips_oversized_and_aggregate() {
        let (_dir, store, project, mut request) = fixture().await;
        selections(&mut request, &project, &[512 * 1024 + 1]);
        let large = preview_with_source_io(&store, &request, "2026-10-15T00:00:00Z", ValidatedProjectFile::open, |_, _| panic!("oversized payload must not be read")).await.unwrap();
        assert_eq!(large.files[0].hash, None);
        assert_eq!(large.files[0].size_bytes, Some(524289));
        assert_eq!(large.files[0].state, MigrationFileStateV1::ReferenceOnly);
        request.files[0].expected_full_sha256 = Some("a".repeat(64));
        assert_eq!(preview_project_migration(&store, &request, "2026-10-15T00:00:00Z").await.unwrap_err().code, MigrationPreviewErrorCodeV1::LimitExceeded);
        selections(&mut request, &project, &[512 * 1024; 9]);
        let mut allowances = vec![];
        let result = preview_with_source_io(&store, &request, "2026-10-15T00:00:00Z", ValidatedProjectFile::open, |file, limit| { allowances.push(limit); file.read_complete(limit) }).await.unwrap();
        assert_eq!(allowances, vec![524288; 8]);
        assert_eq!(result.files[8].hash, None);
        assert_eq!(result.selected_known_file_bytes, 4718592);
        assert_eq!(result.unknown_size_file_count, 0);
        request.files[8].expected_full_sha256 = Some("a".repeat(64));
        assert_eq!(preview_project_migration(&store, &request, "2026-10-15T00:00:00Z").await.unwrap_err().code, MigrationPreviewErrorCodeV1::LimitExceeded);
    }

    #[tokio::test]
    async fn migration_preview_hash_budget_faults_never_refund_actual_payload() {
        let (_dir, store, project, mut request) = fixture().await;
        selections(&mut request, &project, &[512 * 1024; 9]);
        let mut allowances = vec![];
        let mut actual_payload = 0;
        let result = preview_with_source_io(&store, &request, "2026-10-15T00:00:00Z", ValidatedProjectFile::open, |file, limit| {
            allowances.push(limit);
            actual_payload += file.read_complete(limit)?.read_bytes;
            Err(FileReadError::Unavailable)
        }).await.unwrap();
        assert_eq!(actual_payload, 4194304);
        assert_eq!(allowances, vec![524288; 8]);
        assert!(result.files[..8].iter().all(|f| f.state == MigrationFileStateV1::Unreadable && f.hash.is_none()));
        assert_eq!(result.files[8].state, MigrationFileStateV1::ReferenceOnly);
    }

    #[tokio::test]
    async fn migration_preview_hash_budget_remaining_allowance_is_exact() {
        let (_dir, store, project, mut request) = fixture().await;
        let mut sizes = vec![512 * 1024; 9]; sizes[7] = 256 * 1024;
        selections(&mut request, &project, &sizes);
        let mut allowances = vec![];
        let result = preview_with_source_io(&store, &request, "2026-10-15T00:00:00Z", ValidatedProjectFile::open, |file, limit| { allowances.push(limit); file.read_complete(limit) }).await.unwrap();
        assert_eq!(allowances, sizes[..8]);
        assert!(result.files[..8].iter().all(|f| f.hash.is_some()));
        assert_eq!(result.files[8].hash, None);
    }

    #[tokio::test]
    async fn migration_preview_source_changed_growth_uses_actual_w02() {
        let (dir, store, _project, request) = fixture().await;
        let mut observed = None;
        let result = preview_with_source_io(&store, &request, "2026-10-15T00:00:00Z", ValidatedProjectFile::open, |file, limit| {
            fs::write(dir.path().join("a.txt"), vec![b'x'; 512 * 1024]).unwrap();
            let issue = file.read_complete(limit).unwrap_err();
            observed = Some((limit, issue)); Err(issue)
        }).await.unwrap_err();
        assert_eq!(observed, Some((6, FileReadError::SourceChanged)));
        assert_eq!(result.code, MigrationPreviewErrorCodeV1::SourceChanged);
    }

    #[tokio::test]
    async fn migration_preview_source_changed_expected_hash_is_whole_failure() {
        let (_dir, store, _project, mut request) = fixture().await;
        request.files[0].expected_full_sha256 = Some("a".repeat(64));
        assert_eq!(preview_project_migration(&store, &request, "2026-10-15T00:00:00Z").await.unwrap_err().code, MigrationPreviewErrorCodeV1::SourceChanged);
        request.files[0].expected_full_sha256 = Some("5891B5B522D5DF086D0FF0B110FBD9D21BB4FC7163AF34D08286A2E846F6BE03".into());
        assert!(preview_project_migration(&store, &request, "2026-10-15T00:00:00Z").await.is_ok());
    }

    #[tokio::test]
    async fn migration_preview_unsafe_windows_file_identity_rejects_before_open() {
        let (_dir, store, _project, mut request) = fixture().await;
        for path in ["../a.txt", "C:\\a.txt", "\\\\host\\a.txt", "NUL.txt", "a.txt:stream", ".omicsops./db", "a. /x"] {
            request.files[0].relative_path = path.into();
            let result = preview_with_source_io(&store, &request, "2026-10-15T00:00:00Z", |_, _| panic!("invalid identity must reject before open"), ValidatedProjectFile::read_complete).await.unwrap_err();
            assert_eq!(result.code, MigrationPreviewErrorCodeV1::InvalidSelection, "{path}");
        }
        request.files[0].relative_path = "a.txt".into();
        request.files[0].backend_id = format!("ssh:{}", Uuid::new_v4());
        assert_eq!(preview_with_source_io(&store, &request, "2026-10-15T00:00:00Z", |_, _| panic!("wrong owner before open"), ValidatedProjectFile::read_complete).await.unwrap_err().code, MigrationPreviewErrorCodeV1::OwnershipMismatch);
    }

    #[tokio::test]
    async fn migration_preview_sensitive_content_and_preflight_skip_payload() {
        let (dir, store, _project, mut request) = fixture().await;
        fs::write(dir.path().join("a.txt"), b"password = 'FAKE_SENTINEL'").unwrap();
        let secret = preview_project_migration(&store, &request, "2026-10-15T00:00:00Z").await.unwrap();
        assert_eq!(secret.files[0].state, MigrationFileStateV1::SensitiveContent);
        assert_eq!(secret.files[0].hash, None);
        assert!(!serde_json::to_string(&secret).unwrap().contains("FAKE_SENTINEL"));
        for (path, state) in [("credentials.json", MigrationFileStateV1::SensitiveContent), ("password=FAKE_SENTINEL.txt", MigrationFileStateV1::SensitiveContent)] {
            request.files[0].relative_path = path.into();
            let result = preview_with_source_io(&store, &request, "2026-10-15T00:00:00Z", |_, _| panic!("pure policy must reject before opening"), |_, _| panic!("no payload")).await.unwrap();
            assert_eq!(result.files[0].state, state);
            assert!(!serde_json::to_string(&result).unwrap().contains("FAKE_SENTINEL"));
        }
    }

    #[tokio::test]
    async fn migration_preview_missing_unreadable_directory_and_unsafe() {
        let (dir, store, _project, mut request) = fixture().await;
        fs::remove_file(dir.path().join("a.txt")).unwrap();
        let missing = preview_project_migration(&store, &request, "2026-10-15T00:00:00Z").await.unwrap();
        assert_eq!(missing.files[0].state, MigrationFileStateV1::Unreadable);
        assert_eq!(missing.files[0].size_bytes, None);
        fs::create_dir(dir.path().join("a.txt")).unwrap();
        assert_eq!(preview_project_migration(&store, &request, "2026-10-15T00:00:00Z").await.unwrap().files[0].state, MigrationFileStateV1::ReferenceOnly);
        let unsafe_file = preview_with_source_io(&store, &request, "2026-10-15T00:00:00Z", |_, _| Err(FileReadError::UnsafeFile), ValidatedProjectFile::read_complete).await.unwrap();
        assert_eq!(unsafe_file.files[0].state, MigrationFileStateV1::UnsafeFile);
        request.files[0].expected_full_sha256 = Some("a".repeat(64));
        assert_eq!(preview_project_migration(&store, &request, "2026-10-15T00:00:00Z").await.unwrap_err().code, MigrationPreviewErrorCodeV1::SourceChanged);
    }

    #[tokio::test]
    async fn migration_preview_stable_digest_dedup_empty_and_public_fields() {
        let (dir, store, project, mut request) = fixture().await;
        request.files.push(request.files[0].clone());
        request.categories = vec![MigrationMetadataCategoryV1::ScientificMetadata; 2];
        let duplicated = preview_project_migration(&store, &request, "2026-10-15T00:00:00Z").await.unwrap();
        request.files.pop(); request.categories.pop();
        let unique = preview_project_migration(&store, &request, "2026-10-15T00:00:00Z").await.unwrap();
        assert_eq!(duplicated.preview_sha256, unique.preview_sha256);
        assert_eq!(unique.categories[2].kind_counts.iter().map(|v| v.kind.as_str()).collect::<Vec<_>>(), ["dataset", "analysis", "artifact", "evidence", "provenance"]);
        assert!(unique.categories[..2].iter().all(|c| c.kind_counts.is_empty()));
        fs::write(dir.path().join("b.txt"), b"second").unwrap();
        request.files.push(ProjectFileSelectionV1 { backend_id: "local".into(), relative_path: "b.txt".into(), expected_full_sha256: None });
        request.categories.push(MigrationMetadataCategoryV1::Conversations);
        let ordered = preview_project_migration(&store, &request, "2026-10-15T00:00:00Z").await.unwrap();
        request.files.reverse(); request.categories.reverse();
        let reordered = preview_project_migration(&store, &request, "2026-10-15T00:00:00Z").await.unwrap();
        assert_eq!(ordered.preview_sha256, reordered.preview_sha256);
        fs::write(dir.path().join("a.txt"), b"changed").unwrap();
        assert_ne!(ordered.preview_sha256, preview_project_migration(&store, &request, "2026-10-15T00:00:00Z").await.unwrap().preview_sha256);
        request.files.clear(); request.categories.clear();
        let empty = preview_project_migration(&store, &request, "2026-10-15T00:00:00Z").await.unwrap();
        assert!(empty.files.is_empty()); assert!(empty.categories.iter().all(|c| !c.selected && c.examples.is_empty()));
        assert_eq!(empty.project.source_project_id, project.id);
        assert!(!serde_json::to_string(&empty).unwrap().contains(&project.local_root.replace('\\', "\\\\")));
        assert!(empty.exclusions.iter().any(|s| s.contains("0 shelved")));
    }

    #[tokio::test]
    async fn migration_preview_selection_limits_and_safe_store_errors() {
        let (_dir, store, _project, mut request) = fixture().await;
        request.files = vec![request.files[0].clone(); 33];
        assert_eq!(preview_project_migration(&store, &request, "2026-10-15T00:00:00Z").await.unwrap_err().code, MigrationPreviewErrorCodeV1::LimitExceeded);
        request.files.clear(); request.project_id = Uuid::new_v4();
        assert_eq!(preview_project_migration(&store, &request, "2026-10-15T00:00:00Z").await.unwrap_err().code, MigrationPreviewErrorCodeV1::NotFound);
        assert_eq!(storage_error(omicsops_store::StoreError::InvalidInput("SQL private-root FAKE_SENTINEL".into())).message, "Migration metadata could not be read.");
        request.project_id = Uuid::nil();
        assert_eq!(preview_project_migration(&store, &request, "2026-10-15T00:00:00Z").await.unwrap_err().code, MigrationPreviewErrorCodeV1::InvalidSelection);
        let original = fixture().await;
        let mut bounded = original.3;
        for selection in [ProjectFileSelectionV1 { backend_id: "x".repeat(129), relative_path: "a.txt".into(), expected_full_sha256: None }, ProjectFileSelectionV1 { backend_id: "local".into(), relative_path: "x".repeat(1025), expected_full_sha256: None }, ProjectFileSelectionV1 { backend_id: "local".into(), relative_path: "a.txt".into(), expected_full_sha256: Some("invalid".into()) }] {
            bounded.files = vec![selection];
            assert_eq!(preview_project_migration(&original.1, &bounded, "2026-10-15T00:00:00Z").await.unwrap_err().code, MigrationPreviewErrorCodeV1::InvalidSelection);
        }
        bounded.files.clear();
        sqlx::query("DROP TABLE publication_revisions").execute(original.1.pool()).await.unwrap();
        bounded.categories = vec![MigrationMetadataCategoryV1::Publications];
        let failed = preview_project_migration(&original.1, &bounded, "2026-10-15T00:00:00Z").await.unwrap_err();
        assert_eq!(failed.code, MigrationPreviewErrorCodeV1::StorageReadFailed);
        assert_eq!(failed.message, "Migration metadata could not be read.");
    }

    #[tokio::test]
    async fn migration_preview_no_effects_actual_readonly_store_and_files() {
        let (dir, store, _project, request) = fixture().await;
        let before = fs::read(dir.path().join("a.txt")).unwrap();
        let changes: i64 = sqlx::query_scalar("SELECT total_changes()").fetch_one(store.pool()).await.unwrap();
        sqlx::query("PRAGMA query_only=ON").execute(store.pool()).await.unwrap();
        assert!(sqlx::query("UPDATE projects SET name='forbidden'").execute(store.pool()).await.is_err());
        let result = preview_project_migration(&store, &request, "2026-10-15T00:00:00Z").await.unwrap();
        assert_eq!(result.files[0].state, MigrationFileStateV1::SmallTextCandidate);
        assert_eq!(changes, sqlx::query_scalar::<_, i64>("SELECT total_changes()").fetch_one(store.pool()).await.unwrap());
        assert_eq!(fs::read(dir.path().join("a.txt")).unwrap(), before);
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
        // The actual entry has only a Store and validated read functions: no
        // model/kernel/SSH/MCP/keyring capability is accepted or obtained.
    }

    #[tokio::test]
    async fn migration_preview_remote_unknown_recorded_and_expected_never_connect() {
        let (_dir, store, mut project, mut request) = fixture().await;
        let connection = Uuid::new_v4();
        project.connection_id = Some(connection); project.remote_root = Some("/private-root".into());
        // Model a legacy dangling binding explicitly, outside normal FK writes.
        sqlx::query("PRAGMA foreign_keys=OFF").execute(store.pool()).await.unwrap();
        store.save_project(&project).await.unwrap();
        sqlx::query("PRAGMA foreign_keys=ON").execute(store.pool()).await.unwrap();
        request.files[0].backend_id = format!("ssh:{connection}");
        // A dangling exact project binding stays an unknown reference.
        let unknown = preview_with_source_io(&store, &request, "2026-10-15T00:00:00Z", |_, _| panic!("SSH must never open a local or remote file"), |_, _| panic!("SSH must never read payload")).await.unwrap();
        assert_eq!(unknown.files[0].size_bytes, None); assert_eq!(unknown.files[0].hash, None);
        assert_eq!(unknown.unknown_size_file_count, 1);
        assert_eq!(unknown.files[0].state, MigrationFileStateV1::ReferenceOnly);
        // Poisoned private profile JSON proves the actual path never loads it.
        sqlx::query("INSERT INTO connections(id,label,profile_json) VALUES(?,'fixture','NOT JSON FAKE_SENTINEL')").bind(connection.to_string()).execute(store.pool()).await.unwrap();
        let analysis = Uuid::new_v4(); let artifact = Uuid::new_v4();
        let producer = omicsops_science::AnalysisRecordV4 {
            schema_version: 4, id: analysis, project_id: project.id, run_id: Uuid::new_v4(), source_call_id: "fixture".into(), analysis_type: "file".into(), input_dataset_ids: Default::default(), sample_ids: Default::default(), method: "fixture".into(), parameters: serde_json::json!({}), software_requirements: Default::default(), database_versions: Default::default(), random_seed: Some(1), status: omicsops_science::AnalysisStatusV4::Succeeded,
            runtime: omicsops_science::RuntimeIdentityV4 { backend_id: format!("ssh:{connection}"), language: "python".into(), environment: "system".into(), session_id: None, process_identity: None }, started_at: chrono::Utc::now(), ended_at: Some(chrono::Utc::now()), validation_issues: vec![],
        };
        let record = omicsops_science::ArtifactRecordV4 { schema_version: 4, id: artifact, project_id: project.id, artifact_type: "file".into(), relative_path: "a.txt".into(), producer_analysis_id: analysis, size_bytes: 9, sha256: "a".repeat(64), preview: None, metadata: serde_json::json!({}), valid: true, created_at: chrono::Utc::now() };
        sqlx::query("INSERT INTO scientific_analyses_v4(id,project_id,status,value_json) VALUES(?,?,'succeeded',?)").bind(analysis.to_string()).bind(project.id.to_string()).bind(serde_json::to_string(&producer).unwrap()).execute(store.pool()).await.unwrap();
        sqlx::query("INSERT INTO scientific_artifacts_v4(id,project_id,producer_analysis_id,valid,value_json) VALUES(?,?,?,1,?)").bind(artifact.to_string()).bind(project.id.to_string()).bind(analysis.to_string()).bind(serde_json::to_string(&record).unwrap()).execute(store.pool()).await.unwrap();
        let saved = preview_with_source_io(&store, &request, "2026-10-15T00:00:00Z", |_, _| panic!("no SSH open"), |_, _| panic!("no SSH payload")).await.unwrap();
        assert_eq!(saved.files[0].size_bytes, Some(9));
        assert_eq!(saved.files[0].hash, Some(FileReferenceHashV1 { sha256: "a".repeat(64), scope: FileReferenceHashScopeV1::RecordedFile }));
        assert!(!serde_json::to_string(&saved).unwrap().contains("FAKE_SENTINEL"));
        request.files[0].expected_full_sha256 = Some("a".repeat(64));
        assert_eq!(preview_project_migration(&store, &request, "2026-10-15T00:00:00Z").await.unwrap_err().code, MigrationPreviewErrorCodeV1::SourceChanged);
        request.files[0].expected_full_sha256 = None;
        sqlx::query("UPDATE scientific_analyses_v4 SET value_json='{}'").execute(store.pool()).await.unwrap();
        let malformed = preview_project_migration(&store, &request, "2026-10-15T00:00:00Z").await.unwrap();
        assert_eq!(malformed.files[0].hash, None); assert_eq!(malformed.files[0].size_bytes, None);
        request.files[0].backend_id = format!("ssh:{}", connection.to_string().to_uppercase());
        assert_eq!(preview_project_migration(&store, &request, "2026-10-15T00:00:00Z").await.unwrap_err().code, MigrationPreviewErrorCodeV1::OwnershipMismatch);
    }

    #[tokio::test]
    async fn migration_preview_unsupported_metadata_without_payload_and_remote_reference() {
        let (dir, store, mut project, mut request) = fixture().await;
        fs::write(dir.path().join("sample.bin"), b"\0binary").unwrap();
        request.files[0].relative_path = "sample.bin".into();
        let local = preview_with_source_io(&store, &request, "2026-10-15T00:00:00Z", ValidatedProjectFile::open, |_, _| panic!("unsupported payload must not be read")).await.unwrap();
        assert_eq!(local.files[0].size_bytes, Some(7));
        assert_eq!(local.files[0].hash, None);
        assert_eq!(local.files[0].state, MigrationFileStateV1::UnsupportedContent);
        let id = Uuid::new_v4();
        sqlx::query("INSERT INTO connections(id,label,profile_json) VALUES(?,'fixture','private malformed JSON')").bind(id.to_string()).execute(store.pool()).await.unwrap();
        project.connection_id = Some(id); project.remote_root = Some("/project".into()); store.save_project(&project).await.unwrap();
        request.files[0].backend_id = format!("ssh:{id}");
        let remote = preview_with_source_io(&store, &request, "2026-10-15T00:00:00Z", |_, _| panic!("no remote open"), |_, _| panic!("no remote payload")).await.unwrap();
        assert_eq!(remote.files[0].state, MigrationFileStateV1::ReferenceOnly);
        assert_eq!(remote.files[0].size_bytes, None);
    }

    #[tokio::test]
    async fn migration_preview_all_ownership_checked_before_first_read() {
        let (_dir, store, _project, mut request) = fixture().await;
        request.files.push(ProjectFileSelectionV1 { backend_id: format!("ssh:{}", Uuid::new_v4()), relative_path: "z.txt".into(), expected_full_sha256: None });
        assert_eq!(preview_with_source_io(&store, &request, "2026-10-15T00:00:00Z", |_, _| panic!("must validate later selection before first open"), ValidatedProjectFile::read_complete).await.unwrap_err().code, MigrationPreviewErrorCodeV1::OwnershipMismatch);
    }
}
