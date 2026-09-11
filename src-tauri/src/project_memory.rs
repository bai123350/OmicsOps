//! Project-local Markdown memory storage shared by saving, search and counts.
use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
};

pub const MAX_MEMORY_BYTES: usize = 256 * 1024;

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
        if entry.file_type().map_err(|e| e.to_string())?.is_file()
            && entry.path().extension().and_then(|s| s.to_str()) == Some("md")
        {
            files.push(entry.path());
        }
    }
    files.sort();
    Ok(files)
}

pub fn read(path: &Path) -> Result<String, String> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .map_err(|e| e.to_string())?
        .take((MAX_MEMORY_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > MAX_MEMORY_BYTES {
        return Err("memory file exceeds 256 KiB".into());
    }
    let text = String::from_utf8(bytes).map_err(|_| "memory file must be UTF-8")?;
    Ok(omicsops_core::redaction::redact_secrets(
        &text,
        &[] as &[&str],
    ))
}

pub fn save(root: &Path, name: &str, content: &str) -> Result<PathBuf, String> {
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
    if content.trim().is_empty() || content.len() > MAX_MEMORY_BYTES {
        return Err("memory content must be nonempty and at most 256 KiB".into());
    }
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

#[cfg(test)]
mod tests {
    use super::*;
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
}
