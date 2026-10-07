//! Host-only bounded reads. The retained handle is checked before and after each
//! read; detected changes are rejected. This is not an atomic snapshot against
//! hostile concurrent writers.

use omicsops_core::workspace::Project;
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    time::SystemTime,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub(crate) enum FileReadError {
    #[error("project file path is invalid")]
    InvalidPath,
    #[error("project file is unsafe")]
    UnsafeFile,
    #[error("project file is not a regular file")]
    NotRegular,
    #[error("project file is unavailable")]
    Unavailable,
    #[error("project file exceeds the read limit")]
    TooLarge,
    #[error("project file changed while reading")]
    SourceChanged,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FileHashScope {
    Full,
    Prefix,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PublicFileMetadata {
    pub size_bytes: u64,
    pub modified_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BoundedFileRead {
    pub bytes: Vec<u8>,
    pub size_bytes: u64,
    pub read_bytes: u64,
    pub eof: bool,
    pub sha256: String,
    pub hash_scope: FileHashScope,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FileSnapshot {
    identity: (u64, u64),
    size_bytes: u64,
    modified: Option<SystemTime>,
    #[cfg(windows)]
    modified_ticks: u64,
    final_path: PathBuf,
}

/// Read access is kept private; no path is reopened to obtain content.
#[derive(Debug)]
pub(crate) struct ValidatedProjectFile {
    source: LocalFileSource,
    opened: FileSnapshot,
    metadata: PublicFileMetadata,
}

impl ValidatedProjectFile {
    pub(crate) fn open(project: &Project, relative_path: &str) -> Result<Self, FileReadError> {
        let relative = crate::composer_files::normalize_workspace_relative_path(relative_path)
            .map_err(|_| FileReadError::InvalidPath)?;
        if relative.split('/').any(is_reserved_component) {
            return Err(FileReadError::InvalidPath);
        }
        let root_text = project.local_root.trim();
        if root_text.is_empty() || root_text.starts_with("\\\\") || root_text.starts_with("//") {
            return Err(FileReadError::InvalidPath);
        }
        let root = PathBuf::from(root_text);
        let root = if root.is_absolute() {
            root
        } else {
            std::env::current_dir()
                .map_err(|_| FileReadError::Unavailable)?
                .join(root)
        };
        validate_ancestors_with(&root, path_is_unsafe)?;
        if !fs::symlink_metadata(&root)
            .map_err(|_| FileReadError::Unavailable)?
            .is_dir()
        {
            return Err(FileReadError::UnsafeFile);
        }
        let canonical_root = fs::canonicalize(&root).map_err(|_| FileReadError::Unavailable)?;
        let candidate = root.join(relative);
        validate_ancestors_with(&candidate, path_is_unsafe)?;
        if !fs::symlink_metadata(&candidate)
            .map_err(|_| FileReadError::Unavailable)?
            .is_file()
        {
            return Err(FileReadError::NotRegular);
        }
        let canonical = fs::canonicalize(&candidate).map_err(|_| FileReadError::Unavailable)?;
        validate_final_path(&canonical_root, &canonical)?;
        let mut options = OpenOptions::new();
        options.read(true);
        // Do not follow a target reparse point introduced between lstat and open.
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            options.custom_flags(
                windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT,
            );
        }
        let file = options
            .open(&candidate)
            .map_err(|_| FileReadError::Unavailable)?;
        let mut source = LocalFileSource { file, candidate };
        let opened = source.observe()?;
        validate_final_path(&canonical_root, &opened.final_path)?;
        if opened.final_path != canonical {
            return Err(FileReadError::SourceChanged);
        }
        let metadata = PublicFileMetadata {
            size_bytes: opened.size_bytes,
            modified_at: opened
                .modified
                .map(|time| chrono::DateTime::<chrono::Utc>::from(time).to_rfc3339()),
        };
        Ok(Self {
            source,
            opened,
            metadata,
        })
    }

    pub(crate) fn metadata(&self) -> PublicFileMetadata {
        self.metadata.clone()
    }

    pub(crate) fn read_prefix(
        &mut self,
        max_bytes: usize,
    ) -> Result<BoundedFileRead, FileReadError> {
        read_bounded(&mut self.source, &self.opened, max_bytes, false)
    }

    pub(crate) fn read_complete(
        &mut self,
        max_bytes: usize,
    ) -> Result<BoundedFileRead, FileReadError> {
        read_bounded(&mut self.source, &self.opened, max_bytes, true)
    }
}

fn is_reserved_component(component: &str) -> bool {
    let base = component
        .split('.')
        .next()
        .unwrap_or_default()
        .trim_end_matches(' ')
        .to_uppercase();
    matches!(
        base.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
    ) || ["COM", "LPT"].iter().any(|prefix| {
        base.strip_prefix(prefix).is_some_and(|suffix| {
            matches!(
                suffix,
                "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
            )
        })
    })
}

// Shared by Windows lstat/handle checks and platform-independent injected tests.
fn attributes_are_unsafe(attributes: u32) -> bool {
    attributes & (0x400 | 0x40) != 0 // REPARSE_POINT or DEVICE
}

fn path_is_unsafe(path: &Path) -> Result<bool, FileReadError> {
    let metadata = fs::symlink_metadata(path).map_err(|_| FileReadError::Unavailable)?;
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if attributes_are_unsafe(metadata.file_attributes()) {
            return Ok(true);
        }
    }
    Ok(metadata.file_type().is_symlink())
}

fn validate_ancestors_with(
    path: &Path,
    mut probe: impl FnMut(&Path) -> Result<bool, FileReadError>,
) -> Result<(), FileReadError> {
    for ancestor in path.ancestors() {
        if !ancestor.as_os_str().is_empty() && probe(ancestor)? {
            return Err(FileReadError::UnsafeFile);
        }
    }
    Ok(())
}

fn validate_final_path(root: &Path, final_path: &Path) -> Result<(), FileReadError> {
    if final_path == root || !final_path.starts_with(root) {
        return Err(FileReadError::UnsafeFile);
    }
    Ok(())
}

trait FileSource {
    fn observe(&mut self) -> Result<FileSnapshot, FileReadError>;
    fn rewind(&mut self) -> Result<(), FileReadError>;
    fn read_chunk(&mut self, buffer: &mut [u8]) -> Result<usize, FileReadError>;
}

#[derive(Debug)]
struct LocalFileSource {
    file: File,
    candidate: PathBuf,
}

impl FileSource for LocalFileSource {
    fn observe(&mut self) -> Result<FileSnapshot, FileReadError> {
        validate_ancestors_with(&self.candidate, path_is_unsafe)?;
        snapshot_handle(&self.file, &self.candidate)
    }

    fn rewind(&mut self) -> Result<(), FileReadError> {
        self.file
            .seek(SeekFrom::Start(0))
            .map(|_| ())
            .map_err(|_| FileReadError::Unavailable)
    }

    fn read_chunk(&mut self, buffer: &mut [u8]) -> Result<usize, FileReadError> {
        self.file
            .read(buffer)
            .map_err(|_| FileReadError::Unavailable)
    }
}

fn read_bounded(
    source: &mut impl FileSource,
    opened: &FileSnapshot,
    max_bytes: usize,
    complete: bool,
) -> Result<BoundedFileRead, FileReadError> {
    if source.observe()? != *opened {
        return Err(FileReadError::SourceChanged);
    }
    if complete && opened.size_bytes > max_bytes as u64 {
        return Err(FileReadError::TooLarge);
    }
    source.rewind()?;
    let mut bytes = Vec::new();
    let mut buffer = [0u8; 8192];
    let mut eof = false;
    while bytes.len() < max_bytes {
        let limit = buffer.len().min(max_bytes - bytes.len());
        let count = source.read_chunk(&mut buffer[..limit])?;
        if count == 0 {
            eof = true;
            break;
        }
        bytes.extend_from_slice(&buffer[..count]);
    }
    if !eof {
        // The probe confirms EOF, but never contributes to bytes or hash.
        eof = source.read_chunk(&mut buffer[..1])? == 0;
    }
    if source.observe()? != *opened {
        return Err(FileReadError::SourceChanged);
    }
    if complete && !eof {
        return Err(FileReadError::TooLarge);
    }
    Ok(BoundedFileRead {
        size_bytes: opened.size_bytes,
        read_bytes: bytes.len() as u64,
        eof,
        sha256: hex::encode(Sha256::digest(&bytes)),
        hash_scope: if eof {
            FileHashScope::Full
        } else {
            FileHashScope::Prefix
        },
        bytes,
    })
}

#[cfg(windows)]
fn snapshot_handle(file: &File, _candidate: &Path) -> Result<FileSnapshot, FileReadError> {
    use std::os::windows::{ffi::OsStringExt, io::AsRawHandle};
    use windows_sys::Win32::Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle, GetFinalPathNameByHandleW,
    };
    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    // SAFETY: the File owns a live handle; info and the UTF-16 buffer remain
    // writable for the duration of each call. No ownership is transferred.
    if unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) } == 0 {
        return Err(FileReadError::Unavailable);
    }
    if attributes_are_unsafe(info.dwFileAttributes) {
        return Err(FileReadError::UnsafeFile);
    }
    let metadata = file.metadata().map_err(|_| FileReadError::Unavailable)?;
    if !metadata.is_file() {
        return Err(FileReadError::NotRegular);
    }
    let mut buffer = vec![0u16; 32768];
    let length = unsafe {
        GetFinalPathNameByHandleW(
            file.as_raw_handle(),
            buffer.as_mut_ptr(),
            buffer.len() as u32,
            0,
        )
    } as usize;
    if length == 0 || length >= buffer.len() {
        return Err(FileReadError::Unavailable);
    }
    let final_path = PathBuf::from(std::ffi::OsString::from_wide(&buffer[..length]));
    // Canonical DOS paths must remain local. UNC and volume/device namespace
    // paths returned by the handle are never accepted as project files.
    if !matches!(final_path.components().next(), Some(std::path::Component::Prefix(prefix)) if matches!(prefix.kind(), std::path::Prefix::VerbatimDisk(_)))
    {
        return Err(FileReadError::UnsafeFile);
    }
    let size_bytes = (u64::from(info.nFileSizeHigh) << 32) | u64::from(info.nFileSizeLow);
    if size_bytes != metadata.len() {
        return Err(FileReadError::SourceChanged);
    }
    Ok(FileSnapshot {
        identity: (
            u64::from(info.dwVolumeSerialNumber),
            (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow),
        ),
        size_bytes,
        modified: metadata.modified().ok(),
        modified_ticks: (u64::from(info.ftLastWriteTime.dwHighDateTime) << 32)
            | u64::from(info.ftLastWriteTime.dwLowDateTime),
        final_path,
    })
}

#[cfg(unix)]
fn snapshot_handle(file: &File, candidate: &Path) -> Result<FileSnapshot, FileReadError> {
    use std::os::unix::fs::MetadataExt;
    let metadata = file.metadata().map_err(|_| FileReadError::Unavailable)?;
    if !metadata.is_file() {
        return Err(FileReadError::NotRegular);
    }
    let final_path = fs::canonicalize(candidate).map_err(|_| FileReadError::Unavailable)?;
    let path_metadata = fs::metadata(&final_path).map_err(|_| FileReadError::Unavailable)?;
    if (metadata.dev(), metadata.ino()) != (path_metadata.dev(), path_metadata.ino()) {
        return Err(FileReadError::SourceChanged);
    }
    Ok(FileSnapshot {
        identity: (metadata.dev(), metadata.ino()),
        size_bytes: metadata.len(),
        modified: metadata.modified().ok(),
        final_path,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use omicsops_core::workspace::{Project, ProjectTemplate};
    use sha2::Digest;
    use std::io::Cursor;
    use std::path::PathBuf;
    use uuid::Uuid;

    fn fixture(bytes: &[u8]) -> (tempfile::TempDir, Project) {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("sample.csv"), bytes).unwrap();
        let project = Project::new(
            Uuid::new_v4(),
            "preview",
            root.path().to_str().unwrap(),
            ProjectTemplate::Blank,
            chrono::Utc::now(),
        );
        (root, project)
    }

    #[test]
    fn prefix_excludes_probe_byte() {
        let (_root, project) = fixture(b"abcd");
        let mut source = ValidatedProjectFile::open(&project, "sample.csv").unwrap();
        let read = source.read_prefix(3).unwrap();
        assert_eq!(read.bytes, b"abc");
        assert_eq!(read.read_bytes, 3);
        assert_eq!(read.size_bytes, 4);
        assert!(!read.eof);
        assert_eq!(read.hash_scope, FileHashScope::Prefix);
        assert_eq!(read.sha256, hex::encode(sha2::Sha256::digest(b"abc")));
    }

    #[test]
    fn prefix_and_complete_rewind_the_same_handle() {
        let (_root, project) = fixture(b"abcd");
        let mut source = ValidatedProjectFile::open(&project, "sample.csv").unwrap();
        let metadata = source.metadata();
        assert_eq!(metadata.size_bytes, 4);
        assert!(metadata.modified_at.is_some());
        assert_eq!(source.read_prefix(2).unwrap().bytes, b"ab");
        let full = source.read_complete(4).unwrap();
        assert_eq!(full.bytes, b"abcd");
        assert!(full.eof);
        assert_eq!(full.hash_scope, FileHashScope::Full);
        assert_eq!(full.sha256, hex::encode(sha2::Sha256::digest(b"abcd")));
    }

    #[test]
    fn exact_budget_and_empty_files_confirm_eof() {
        for (bytes, budget) in [(b"abc".as_slice(), 3), (b"".as_slice(), 0)] {
            let (_root, project) = fixture(bytes);
            let read = ValidatedProjectFile::open(&project, "sample.csv")
                .unwrap()
                .read_prefix(budget)
                .unwrap();
            assert_eq!(read.bytes, bytes);
            assert!(read.eof);
            assert_eq!(read.hash_scope, FileHashScope::Full);
        }
    }

    #[test]
    fn complete_over_budget_is_rejected() {
        let (_root, project) = fixture(b"abcd");
        let result = ValidatedProjectFile::open(&project, "sample.csv")
            .unwrap()
            .read_complete(3);
        assert_eq!(result.unwrap_err(), FileReadError::TooLarge);
    }

    #[test]
    fn zero_budget_nonempty_prefix_is_empty_and_not_full() {
        let (_root, project) = fixture(b"a");
        let read = ValidatedProjectFile::open(&project, "sample.csv")
            .unwrap()
            .read_prefix(0)
            .unwrap();
        assert!(read.bytes.is_empty());
        assert!(!read.eof);
        assert_eq!(read.read_bytes, 0);
        assert_eq!(read.hash_scope, FileHashScope::Prefix);
    }

    #[test]
    fn invalid_and_reserved_relative_paths_are_rejected() {
        let (_root, project) = fixture(b"abc");
        for path in [
            "",
            "../sample.csv",
            "a/../sample.csv",
            "/sample.csv",
            "C:\\sample.csv",
            "\\\\server\\share\\sample.csv",
            "\\\\?\\C:\\sample.csv",
            "\\\\.\\NUL",
            "sample.csv:stream",
            "sample.csv.",
            "sample.csv ",
            "sample\0.csv",
            "CON",
            "con.csv",
            "NUL.txt",
            "COM1.csv",
            "LPT9",
            "COM¹.txt",
            "LPT².txt",
            "a/AUX.csv",
            ".omicsops/a.csv",
            ".git/a.csv",
        ] {
            assert_eq!(
                ValidatedProjectFile::open(&project, path).unwrap_err(),
                FileReadError::InvalidPath,
                "{path:?}"
            );
        }
    }

    #[test]
    fn ordinary_device_like_names_and_normalized_separators_work() {
        let (root, project) = fixture(b"abc");
        std::fs::create_dir(root.path().join("nested")).unwrap();
        std::fs::write(root.path().join("nested/COM10.csv"), b"ok").unwrap();
        assert_eq!(
            ValidatedProjectFile::open(&project, "./nested\\COM10.csv")
                .unwrap()
                .read_complete(2)
                .unwrap()
                .bytes,
            b"ok"
        );
    }

    #[test]
    fn directories_and_unavailable_files_use_fixed_errors() {
        let (root, project) = fixture(b"abc");
        std::fs::create_dir(root.path().join("directory")).unwrap();
        assert_eq!(
            ValidatedProjectFile::open(&project, "directory").unwrap_err(),
            FileReadError::NotRegular
        );
        assert_eq!(
            ValidatedProjectFile::open(&project, "missing.csv").unwrap_err(),
            FileReadError::Unavailable
        );
        assert!(
            !FileReadError::Unavailable
                .to_string()
                .contains(root.path().to_str().unwrap())
        );
    }

    #[test]
    fn changes_since_open_are_rejected() {
        let (root, project) = fixture(b"abc");
        let mut source = ValidatedProjectFile::open(&project, "sample.csv").unwrap();
        std::fs::write(root.path().join("sample.csv"), b"changed").unwrap();
        assert_eq!(
            source.read_prefix(3).unwrap_err(),
            FileReadError::SourceChanged
        );
        assert_eq!(source.metadata().size_bytes, 3);
    }

    #[test]
    fn injected_reparse_ancestor_and_target_are_rejected() {
        let (root, project) = fixture(b"abc");
        for blocked in [root.path().to_path_buf(), root.path().join("sample.csv")] {
            assert_eq!(
                validate_ancestors_with(&root.path().join("sample.csv"), |path| {
                    if path == blocked {
                        Ok(true)
                    } else {
                        path_is_unsafe(path)
                    }
                })
                .unwrap_err(),
                FileReadError::UnsafeFile
            );
        }
        assert!(attributes_are_unsafe(0x400));
        assert!(attributes_are_unsafe(0x410));
        assert!(!attributes_are_unsafe(0x20));
        assert!(ValidatedProjectFile::open(&project, "sample.csv").is_ok());
    }

    #[test]
    fn final_path_requires_component_containment() {
        let root = PathBuf::from("workspace");
        assert!(validate_final_path(&root, &root.join("data.csv")).is_ok());
        assert_eq!(
            validate_final_path(&root, &PathBuf::from("workspace-other/data.csv")),
            Err(FileReadError::UnsafeFile)
        );
        assert_eq!(
            validate_final_path(&root, &root),
            Err(FileReadError::UnsafeFile)
        );
    }

    struct FakeSource {
        bytes: Cursor<Vec<u8>>,
        snapshots: std::collections::VecDeque<FileSnapshot>,
    }

    impl FileSource for FakeSource {
        fn observe(&mut self) -> Result<FileSnapshot, FileReadError> {
            Ok(self.snapshots.pop_front().unwrap())
        }
        fn rewind(&mut self) -> Result<(), FileReadError> {
            self.bytes.set_position(0);
            Ok(())
        }
        fn read_chunk(&mut self, buffer: &mut [u8]) -> Result<usize, FileReadError> {
            std::io::Read::read(&mut self.bytes, buffer).map_err(|_| FileReadError::Unavailable)
        }
    }

    #[test]
    fn same_handle_post_read_identity_size_and_time_changes_are_rejected() {
        let (_root, project) = fixture(b"abc");
        let source = ValidatedProjectFile::open(&project, "sample.csv").unwrap();
        let baseline = source.opened.clone();
        for variant in 0..3 {
            let mut changed = baseline.clone();
            match variant {
                0 => changed.identity.1 += 1,
                1 => changed.size_bytes += 1,
                _ => changed.modified = Some(std::time::UNIX_EPOCH),
            }
            let mut fake = FakeSource {
                bytes: Cursor::new(b"abc".to_vec()),
                snapshots: [baseline.clone(), changed].into(),
            };
            assert_eq!(
                read_bounded(&mut fake, &baseline, 3, false).unwrap_err(),
                FileReadError::SourceChanged
            );
        }
    }

    #[test]
    fn same_handle_final_path_change_is_rejected() {
        let (_root, project) = fixture(b"abc");
        let source = ValidatedProjectFile::open(&project, "sample.csv").unwrap();
        let baseline = source.opened.clone();
        let mut changed = baseline.clone();
        changed.final_path = changed.final_path.parent().unwrap().join("other.csv");
        let mut fake = FakeSource {
            bytes: Cursor::new(b"abc".to_vec()),
            snapshots: [baseline.clone(), changed].into(),
        };
        assert_eq!(
            read_bounded(&mut fake, &baseline, 3, false).unwrap_err(),
            FileReadError::SourceChanged
        );
    }

    #[cfg(windows)]
    #[test]
    fn same_handle_native_last_write_time_change_is_rejected() {
        let (_root, project) = fixture(b"abc");
        let source = ValidatedProjectFile::open(&project, "sample.csv").unwrap();
        let baseline = source.opened.clone();
        let mut changed = baseline.clone();
        changed.modified_ticks += 1;
        let mut fake = FakeSource {
            bytes: Cursor::new(b"abc".to_vec()),
            snapshots: [baseline.clone(), changed].into(),
        };
        assert_eq!(
            read_bounded(&mut fake, &baseline, 3, false).unwrap_err(),
            FileReadError::SourceChanged
        );
    }
}
