//! Project-local Markdown memory storage shared by saving, search and counts.
use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
};

use omicsops_dto::{
    CreateMemoryFileRequestV4, DeleteMemoryFileRequestV4, MemoryFileSummaryV4, MemoryFileV4,
    UpdateMemoryFileRequestV4,
};
use sha2::{Digest, Sha256};
use tauri::State;
use uuid::Uuid;

use crate::commands::AppState;

pub const MAX_MEMORY_BYTES: usize = 256 * 1024;
static MEMORY_MUTATION_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

fn validate_name(name: &str) -> Result<(), String> {
    let stem = name
        .strip_suffix(".md")
        .ok_or("memory filename must end with .md")?;
    let upper = stem.to_ascii_uppercase();
    if stem.is_empty()
        || stem.len() > 100
        || !stem
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        || ["CON", "PRN", "AUX", "NUL"].contains(&upper.as_str())
        || (upper.len() == 4
            && (upper.starts_with("COM") || upper.starts_with("LPT"))
            && upper.as_bytes()[3].is_ascii_digit())
    {
        return Err(
            "use a simple non-reserved filename containing letters, numbers, - or _".into(),
        );
    }
    Ok(())
}

fn validate_content(content: &str) -> Result<(), String> {
    if content.trim().is_empty() || content.len() > MAX_MEMORY_BYTES {
        return Err("memory content must be nonempty and at most 256 KiB".into());
    }
    Ok(())
}

#[cfg(windows)]
fn has_multiple_links(path: &Path, _metadata: &std::fs::Metadata) -> Result<bool, String> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle,
    };

    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut information = unsafe { std::mem::zeroed::<BY_HANDLE_FILE_INFORMATION>() };
    let succeeded = unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut information) };
    if succeeded == 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    Ok(information.nNumberOfLinks > 1)
}

#[cfg(unix)]
fn has_multiple_links(_path: &Path, metadata: &std::fs::Metadata) -> Result<bool, String> {
    use std::os::unix::fs::MetadataExt;
    Ok(metadata.nlink() > 1)
}

#[cfg(not(any(windows, unix)))]
fn has_multiple_links(_path: &Path, _metadata: &std::fs::Metadata) -> Result<bool, String> {
    Ok(false)
}

fn regular_unlinked_file(path: &Path) -> Result<std::fs::Metadata, String> {
    let metadata = std::fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if metadata.file_type().is_symlink()
        || !metadata.is_file()
        || has_multiple_links(path, &metadata)?
    {
        return Err("memory file must be a regular file and cannot be a link".into());
    }
    Ok(metadata)
}

fn directory(root: &Path, create: bool) -> Result<PathBuf, String> {
    let root = root.canonicalize().map_err(|e| e.to_string())?;
    let mut path = root.clone();
    for part in [".omicsops", "memory"] {
        path.push(part);
        match std::fs::symlink_metadata(&path) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink()
                    || !metadata.is_dir()
                    || !path
                        .canonicalize()
                        .map_err(|e| e.to_string())?
                        .starts_with(&root)
                {
                    return Err(
                        "memory directory must stay inside the project and cannot be a link".into(),
                    );
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                if create {
                    std::fs::create_dir(&path).map_err(|e| e.to_string())?;
                }
            }
            Err(e) => return Err(e.to_string()),
        }
    }
    Ok(path)
}

pub fn files(root: &Path) -> Result<Vec<PathBuf>, String> {
    if !root.exists() {
        return Ok(vec![]);
    }
    let dir = directory(root, false)?;
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
        Err(e) => return Err(e.to_string()),
    };
    let mut files = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|e| e.to_string())?;
        if entry.path().extension().and_then(|s| s.to_str()) == Some("md")
            && regular_unlinked_file(&entry.path()).is_ok()
        {
            files.push(entry.path());
        }
    }
    files.sort();
    Ok(files)
}

fn read_bytes(path: &Path) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .map_err(|e| e.to_string())?
        .take((MAX_MEMORY_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > MAX_MEMORY_BYTES {
        return Err("memory file exceeds 256 KiB".into());
    }
    Ok(bytes)
}

fn redact_memory_bytes(bytes: Vec<u8>) -> Result<String, String> {
    let text = String::from_utf8(bytes).map_err(|_| "memory file must be UTF-8")?;
    Ok(omicsops_core::redaction::redact_secrets(
        &text,
        &[] as &[&str],
    ))
}

pub fn read(path: &Path) -> Result<String, String> {
    redact_memory_bytes(read_bytes(path)?)
}

pub fn save(root: &Path, name: &str, content: &str) -> Result<PathBuf, String> {
    validate_name(name)?;
    validate_content(content)?;
    let content = omicsops_core::redaction::redact_secrets(content, &[] as &[&str]);
    let dir = directory(root, true)?;
    let target = dir.join(name);
    let mut temporary = tempfile::NamedTempFile::new_in(&dir).map_err(|e| e.to_string())?;
    temporary
        .write_all(content.as_bytes())
        .map_err(|e| e.to_string())?;
    temporary.as_file().sync_all().map_err(|e| e.to_string())?;
    temporary
        .persist_noclobber(&target)
        .map_err(|e| format!("cannot save memory (existing files are never overwritten): {e}"))?;
    Ok(target)
}

async fn project_root(store: &omicsops_store::Store, project_id: Uuid) -> Result<PathBuf, String> {
    let project = store
        .get_project(project_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("project {project_id} was not found"))?;
    Ok(PathBuf::from(project.local_root))
}

fn read_named(root: &Path, project_id: Uuid, name: &str) -> Result<MemoryFileV4, String> {
    validate_name(name)?;
    let path = directory(root, false)?.join(name);
    regular_unlinked_file(&path)?;
    let raw = read_bytes(&path)?;
    let size_bytes = raw.len() as u64;
    let sha256 = hex::encode(Sha256::digest(&raw));
    let content = redact_memory_bytes(raw)?;
    Ok(MemoryFileV4 {
        project_id,
        name: name.into(),
        content,
        size_bytes,
        sha256,
    })
}

pub async fn list_for_project(
    store: &omicsops_store::Store,
    project_id: Uuid,
) -> Result<Vec<MemoryFileSummaryV4>, String> {
    let root = project_root(store, project_id).await?;
    let mut summaries = Vec::new();
    for path in files(&root)? {
        let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
            continue;
        };
        let file = read_named(&root, project_id, name)?;
        summaries.push(MemoryFileSummaryV4 {
            project_id,
            name: file.name,
            size_bytes: file.size_bytes,
            sha256: file.sha256,
        });
    }
    Ok(summaries)
}

pub async fn read_for_project(
    store: &omicsops_store::Store,
    project_id: Uuid,
    name: &str,
) -> Result<MemoryFileV4, String> {
    let root = project_root(store, project_id).await?;
    read_named(&root, project_id, name)
}

pub async fn create_for_project(
    store: &omicsops_store::Store,
    request: CreateMemoryFileRequestV4,
) -> Result<MemoryFileV4, String> {
    let _guard = MEMORY_MUTATION_LOCK.lock().await;
    let root = project_root(store, request.project_id).await?;
    save(&root, &request.name, &request.content)?;
    read_named(&root, request.project_id, &request.name)
}

fn ensure_expected(file: &MemoryFileV4, expected_sha256: &str) -> Result<(), String> {
    if file.sha256 != expected_sha256 {
        return Err("memory_conflict: memory file changed on disk; reload before retrying".into());
    }
    Ok(())
}

fn replace_content_atomically<F>(
    directory: &Path,
    target: &Path,
    content: &str,
    persist: F,
) -> Result<(), String>
where
    F: FnOnce(tempfile::NamedTempFile, &Path) -> Result<(), String>,
{
    let mut temporary = tempfile::NamedTempFile::new_in(directory).map_err(|e| e.to_string())?;
    temporary
        .write_all(content.as_bytes())
        .map_err(|e| e.to_string())?;
    temporary.as_file().sync_all().map_err(|e| e.to_string())?;
    persist(temporary, target)
}

pub async fn update_for_project(
    store: &omicsops_store::Store,
    request: UpdateMemoryFileRequestV4,
) -> Result<MemoryFileV4, String> {
    let _guard = MEMORY_MUTATION_LOCK.lock().await;
    validate_content(&request.content)?;
    let root = project_root(store, request.project_id).await?;
    let current = read_named(&root, request.project_id, &request.name)?;
    ensure_expected(&current, &request.expected_sha256)?;
    let content = omicsops_core::redaction::redact_secrets(&request.content, &[] as &[&str]);
    let dir = directory(&root, false)?;
    let target = dir.join(&request.name);
    regular_unlinked_file(&target)?;
    replace_content_atomically(&dir, &target, &content, |temporary, target| {
        temporary
            .persist(target)
            .map(|_| ())
            .map_err(|e| format!("cannot replace memory file: {e}"))
    })?;
    read_named(&root, request.project_id, &request.name)
}

pub async fn delete_for_project(
    store: &omicsops_store::Store,
    request: DeleteMemoryFileRequestV4,
) -> Result<bool, String> {
    let _guard = MEMORY_MUTATION_LOCK.lock().await;
    let root = project_root(store, request.project_id).await?;
    let current = read_named(&root, request.project_id, &request.name)?;
    ensure_expected(&current, &request.expected_sha256)?;
    let target = directory(&root, false)?.join(&request.name);
    regular_unlinked_file(&target)?;
    std::fs::remove_file(target).map_err(|e| e.to_string())?;
    Ok(true)
}

#[tauri::command]
pub async fn list_project_memory_files(
    state: State<'_, AppState>,
    project_id: Uuid,
) -> Result<Vec<MemoryFileSummaryV4>, String> {
    list_for_project(&state.repository, project_id).await
}

#[tauri::command]
pub async fn read_project_memory_file(
    state: State<'_, AppState>,
    project_id: Uuid,
    name: String,
) -> Result<MemoryFileV4, String> {
    read_for_project(&state.repository, project_id, &name).await
}

#[tauri::command]
pub async fn create_project_memory_file(
    state: State<'_, AppState>,
    request: CreateMemoryFileRequestV4,
) -> Result<MemoryFileV4, String> {
    create_for_project(&state.repository, request).await
}

#[tauri::command]
pub async fn update_project_memory_file(
    state: State<'_, AppState>,
    request: UpdateMemoryFileRequestV4,
) -> Result<MemoryFileV4, String> {
    update_for_project(&state.repository, request).await
}

#[tauri::command]
pub async fn delete_project_memory_file(
    state: State<'_, AppState>,
    request: DeleteMemoryFileRequestV4,
) -> Result<bool, String> {
    delete_for_project(&state.repository, request).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use omicsops_core::workspace::{Project, ProjectTemplate};
    use omicsops_dto::{
        CreateMemoryFileRequestV4, DeleteMemoryFileRequestV4, UpdateMemoryFileRequestV4,
    };
    use sha2::{Digest, Sha256};
    use uuid::Uuid;

    async fn saved_project(store: &omicsops_store::Store, root: &Path, name: &str) -> Project {
        let project = Project::new(
            Uuid::new_v4(),
            name,
            root.to_string_lossy(),
            ProjectTemplate::Blank,
            chrono::Utc::now(),
        );
        store.save_project(&project).await.unwrap();
        project
    }

    #[tokio::test]
    async fn saved_memory_is_the_search_source_and_updates_from_disk() {
        use crate::p1_commands::{MemorySearchRequest, memory_facts};
        use omicsops_core::workspace::{Project, ProjectTemplate};
        let root = tempfile::tempdir().unwrap();
        let store = omicsops_store::Store::open_in_memory().await.unwrap();
        let project = Project::new(
            uuid::Uuid::new_v4(),
            "memory-test",
            root.path().to_string_lossy(),
            ProjectTemplate::Blank,
            chrono::Utc::now(),
        );
        store.save_project(&project).await.unwrap();
        let request = MemorySearchRequest {
            project_id: project.id,
            conversation_id: None,
            query: "donor".into(),
            dimension: None,
        };
        assert!(memory_facts(&store, &request).await.unwrap().is_empty());
        let path = save(root.path(), "study.md", "Use donor-level comparisons").unwrap();
        let hits = memory_facts(&store, &request).await.unwrap();
        assert_eq!(hits.len(), files(root.path()).unwrap().len());
        assert_eq!(hits[0].evidence[0].source_kind, "memory_file");
        assert_eq!(hits[0].key, ".omicsops/memory/study.md");
        std::fs::write(&path, "Updated methods").unwrap();
        assert!(memory_facts(&store, &request).await.unwrap().is_empty());
        std::fs::remove_file(path).unwrap();
        assert!(files(root.path()).unwrap().is_empty());
        std::fs::write(root.path().join(".omicsops/memory/broken.md"), [0xff]).unwrap();
        let error = memory_facts(&store, &request).await.unwrap_err();
        assert!(error.contains(".omicsops/memory/broken.md"));
        assert!(error.contains("UTF-8"));
    }
    #[test]
    fn save_and_enumerate_share_one_project_directory() {
        let root = tempfile::tempdir().unwrap();
        assert!(files(root.path()).unwrap().is_empty());
        assert!(!root.path().join(".omicsops").exists());
        let path = save(
            root.path(),
            "study.md",
            "# Study\nUse donor-level comparisons.",
        )
        .unwrap();
        assert_eq!(
            path,
            root.path()
                .join(".omicsops/memory/study.md")
                .canonicalize()
                .unwrap()
        );
        assert_eq!(files(root.path()).unwrap(), vec![path.clone()]);
        assert!(save(root.path(), "study.md", "overwrite").is_err());
        assert!(
            std::fs::read_to_string(path)
                .unwrap()
                .contains("donor-level")
        );
        for invalid in [
            "../escape.md",
            "sub/note.md",
            "C:\\note.md",
            "note.txt",
            "CON.md",
        ] {
            assert!(save(root.path(), invalid, "note").is_err());
        }
    }
    #[test]
    fn only_local_top_level_markdown_files_are_counted() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join(".omicsops/memory");
        std::fs::create_dir_all(dir.join("nested")).unwrap();
        std::fs::create_dir_all(root.path().join(".wisp/memory")).unwrap();
        for name in ["a.md", "b.txt", "c.MD", "nested/d.md"] {
            std::fs::write(dir.join(name), "test").unwrap();
        }
        std::fs::write(root.path().join(".wisp/memory/old.md"), "old").unwrap();
        assert_eq!(
            files(root.path()).unwrap(),
            vec![dir.join("a.md").canonicalize().unwrap()]
        );
    }

    #[tokio::test]
    async fn project_file_lifecycle_is_scoped_and_content_addressed() {
        let store = omicsops_store::Store::open_in_memory().await.unwrap();
        let first_root = tempfile::tempdir().unwrap();
        let second_root = tempfile::tempdir().unwrap();
        let first = saved_project(&store, first_root.path(), "first").await;
        let second = saved_project(&store, second_root.path(), "second").await;

        let created = create_for_project(
            &store,
            CreateMemoryFileRequestV4 {
                project_id: first.id,
                name: "study.md".into(),
                content: "Donor-level analysis".into(),
            },
        )
        .await
        .unwrap();
        assert_eq!(created.project_id, first.id);
        assert_eq!(
            created.sha256,
            hex::encode(Sha256::digest(b"Donor-level analysis"))
        );
        assert!(
            list_for_project(&store, second.id)
                .await
                .unwrap()
                .is_empty()
        );

        let listed = list_for_project(&store, first.id).await.unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].name, "study.md");
        assert_eq!(
            read_for_project(&store, first.id, "study.md")
                .await
                .unwrap(),
            created
        );
        let disk_snapshot = read_for_project(&store, first.id, "study.md")
            .await
            .unwrap();
        assert_eq!(
            disk_snapshot.sha256,
            hex::encode(Sha256::digest(disk_snapshot.content.as_bytes()))
        );

        let updated = update_for_project(
            &store,
            UpdateMemoryFileRequestV4 {
                project_id: first.id,
                name: "study.md".into(),
                content: "Updated methods".into(),
                expected_sha256: listed[0].sha256.clone(),
            },
        )
        .await
        .unwrap();
        assert_eq!(updated.content, "Updated methods");
        save(first_root.path(), "other.md", "Keep this file").unwrap();
        assert!(
            delete_for_project(
                &store,
                DeleteMemoryFileRequestV4 {
                    project_id: first.id,
                    name: "study.md".into(),
                    expected_sha256: updated.sha256,
                },
            )
            .await
            .unwrap()
        );
        let remaining = list_for_project(&store, first.id).await.unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].name, "other.md");
    }

    #[tokio::test]
    async fn rejects_unsafe_names_links_oversize_content_and_unknown_projects() {
        let store = omicsops_store::Store::open_in_memory().await.unwrap();
        let root = tempfile::tempdir().unwrap();
        let project = saved_project(&store, root.path(), "safe").await;
        for name in [
            "../escape.md",
            "folder/note.md",
            r"C:\note.md",
            "CON.md",
            "LPT1.md",
        ] {
            assert!(read_for_project(&store, project.id, name).await.is_err());
        }
        let oversized = "x".repeat(MAX_MEMORY_BYTES + 1);
        assert!(
            create_for_project(
                &store,
                CreateMemoryFileRequestV4 {
                    project_id: project.id,
                    name: "large.md".into(),
                    content: oversized,
                },
            )
            .await
            .is_err()
        );
        assert!(list_for_project(&store, Uuid::new_v4()).await.is_err());

        let memory_dir = root.path().join(".omicsops/memory");
        std::fs::create_dir_all(&memory_dir).unwrap();
        let oversized_path = memory_dir.join("oversized.md");
        std::fs::write(&oversized_path, vec![b'x'; MAX_MEMORY_BYTES + 1]).unwrap();
        assert!(
            read_for_project(&store, project.id, "oversized.md")
                .await
                .unwrap_err()
                .contains("exceeds 256 KiB")
        );
        std::fs::remove_file(oversized_path).unwrap();
        let outside = root.path().join("outside.md");
        std::fs::write(&outside, "outside").unwrap();
        std::fs::hard_link(&outside, memory_dir.join("linked.md")).unwrap();
        assert!(
            list_for_project(&store, project.id)
                .await
                .unwrap()
                .is_empty()
        );
        assert!(
            read_for_project(&store, project.id, "linked.md")
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn conflicts_do_not_overwrite_and_same_hash_mutations_are_serialized() {
        let store = omicsops_store::Store::open_in_memory().await.unwrap();
        let root = tempfile::tempdir().unwrap();
        let project = saved_project(&store, root.path(), "race").await;
        let created = create_for_project(
            &store,
            CreateMemoryFileRequestV4 {
                project_id: project.id,
                name: "race.md".into(),
                content: "version one".into(),
            },
        )
        .await
        .unwrap();
        let first = update_for_project(
            &store,
            UpdateMemoryFileRequestV4 {
                project_id: project.id,
                name: "race.md".into(),
                content: "first winner".into(),
                expected_sha256: created.sha256.clone(),
            },
        );
        let second = update_for_project(
            &store,
            UpdateMemoryFileRequestV4 {
                project_id: project.id,
                name: "race.md".into(),
                content: "second winner".into(),
                expected_sha256: created.sha256.clone(),
            },
        );
        let (first, second) = tokio::join!(first, second);
        assert_eq!(usize::from(first.is_ok()) + usize::from(second.is_ok()), 1);
        let winner = read_for_project(&store, project.id, "race.md")
            .await
            .unwrap();
        assert!(winner.content == "first winner" || winner.content == "second winner");

        let stale_delete = delete_for_project(
            &store,
            DeleteMemoryFileRequestV4 {
                project_id: project.id,
                name: "race.md".into(),
                expected_sha256: created.sha256,
            },
        )
        .await
        .unwrap_err();
        assert!(stale_delete.contains("memory_conflict"));
        assert_eq!(
            read_for_project(&store, project.id, "race.md")
                .await
                .unwrap(),
            winner
        );
    }

    #[test]
    fn failed_atomic_replace_keeps_the_original_file() {
        let root = tempfile::tempdir().unwrap();
        let target = root.path().join("study.md");
        std::fs::write(&target, "original").unwrap();

        let error = replace_content_atomically(root.path(), &target, "replacement", |_, _| {
            Err("forced replacement failure".into())
        })
        .unwrap_err();

        assert!(error.contains("forced replacement failure"));
        assert_eq!(std::fs::read_to_string(target).unwrap(), "original");
    }
}
