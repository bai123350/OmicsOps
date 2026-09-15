use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use omicsops_dto::{
    StorageScanIssueV4, StorageScanLimitsV4, StorageUsageCategoryV4, StorageUsageEntryV4,
    StorageUsageScopeV4, StorageUsageSnapshotV4, StorageUsageStatusV4,
};
use omicsops_store::Store;
use tauri::{AppHandle, Manager, State};
use uuid::Uuid;

use crate::commands::AppState;

const STORAGE_SCAN_MAX_ENTRIES: u64 = 100_000;
const STORAGE_SCAN_MAX_DURATION: Duration = Duration::from_millis(2_000);

#[derive(Debug, Clone, Copy)]
struct ScanLimits {
    max_entries: u64,
    max_duration: Duration,
}

#[derive(Debug, Clone, Copy)]
enum MissingRoot {
    NotCreated,
    Unknown,
}

#[derive(Debug)]
struct StorageTarget {
    category: StorageUsageCategoryV4,
    project_id: Option<Uuid>,
    path: PathBuf,
    missing: MissingRoot,
}

type DirectoryEntries = Box<dyn Iterator<Item = io::Result<PathBuf>> + Send>;

trait StorageFileSystem {
    fn symlink_metadata(&self, path: &Path) -> io::Result<fs::Metadata>;
    fn canonicalize(&self, path: &Path) -> io::Result<PathBuf>;
    fn read_dir(&self, path: &Path) -> io::Result<DirectoryEntries>;
}

struct RealFileSystem;

impl StorageFileSystem for RealFileSystem {
    fn symlink_metadata(&self, path: &Path) -> io::Result<fs::Metadata> {
        fs::symlink_metadata(path)
    }

    fn canonicalize(&self, path: &Path) -> io::Result<PathBuf> {
        fs::canonicalize(path)
    }

    fn read_dir(&self, path: &Path) -> io::Result<DirectoryEntries> {
        Ok(Box::new(
            fs::read_dir(path)?.map(|entry| entry.map(|entry| entry.path())),
        ))
    }
}

struct ScanState {
    started: Instant,
    limits: ScanLimits,
    scanned_entries: u64,
    skipped_links: u64,
    visited: HashSet<String>,
    stopped: Option<StorageScanIssueV4>,
}

impl ScanState {
    fn new(limits: ScanLimits) -> Self {
        Self {
            started: Instant::now(),
            limits,
            scanned_entries: 0,
            skipped_links: 0,
            visited: HashSet::new(),
            stopped: None,
        }
    }

    fn next_entry_issue(&self) -> Option<StorageScanIssueV4> {
        if self.started.elapsed() >= self.limits.max_duration {
            Some(StorageScanIssueV4::TimeLimit)
        } else if self.scanned_entries >= self.limits.max_entries {
            Some(StorageScanIssueV4::EntryLimit)
        } else {
            None
        }
    }
}

fn scan_storage_targets(
    scope: StorageUsageScopeV4,
    project_id: Option<Uuid>,
    targets: Vec<StorageTarget>,
    limits: ScanLimits,
) -> StorageUsageSnapshotV4 {
    scan_storage_targets_with_file_system(scope, project_id, targets, limits, &RealFileSystem)
}

fn scan_storage_targets_with_file_system(
    scope: StorageUsageScopeV4,
    project_id: Option<Uuid>,
    targets: Vec<StorageTarget>,
    limits: ScanLimits,
    file_system: &dyn StorageFileSystem,
) -> StorageUsageSnapshotV4 {
    let mut state = ScanState::new(limits);
    let mut entries = Vec::with_capacity(targets.len());
    for target in targets {
        let before_entries = state.scanned_entries;
        let before_links = state.skipped_links;
        let (known_logical_bytes, issue) = if let Some(issue) = state.stopped {
            (None, Some(issue))
        } else {
            scan_target(&target, &mut state, file_system)
        };
        let status = if matches!(
            issue,
            Some(
                StorageScanIssueV4::Missing
                    | StorageScanIssueV4::Unreadable
                    | StorageScanIssueV4::EntryLimit
                    | StorageScanIssueV4::TimeLimit
            )
        ) {
            StorageUsageStatusV4::Partial
        } else {
            StorageUsageStatusV4::Complete
        };
        entries.push(StorageUsageEntryV4 {
            category: target.category,
            project_id: target.project_id,
            path: target.path.to_string_lossy().into_owned(),
            known_logical_bytes,
            status,
            scanned_entries: state.scanned_entries.saturating_sub(before_entries),
            skipped_links: state.skipped_links.saturating_sub(before_links),
            issue,
        });
    }
    let status = if entries
        .iter()
        .any(|entry| entry.status == StorageUsageStatusV4::Partial)
    {
        StorageUsageStatusV4::Partial
    } else {
        StorageUsageStatusV4::Complete
    };
    let known_logical_bytes = entries
        .iter()
        .filter_map(|entry| entry.known_logical_bytes)
        .fold(0_u64, u64::saturating_add);
    StorageUsageSnapshotV4 {
        scope,
        project_id,
        entries,
        known_logical_bytes,
        status,
        scanned_entries: state.scanned_entries,
        skipped_links: state.skipped_links,
        limits: StorageScanLimitsV4 {
            max_entries: limits.max_entries,
            max_duration_ms: u64::try_from(limits.max_duration.as_millis()).unwrap_or(u64::MAX),
        },
    }
}

fn scan_target(
    target: &StorageTarget,
    state: &mut ScanState,
    file_system: &dyn StorageFileSystem,
) -> (Option<u64>, Option<StorageScanIssueV4>) {
    if let Some(issue) = state.next_entry_issue() {
        state.stopped = Some(issue);
        return (None, Some(issue));
    }
    // Each target root consumes one entry before metadata is inspected. Directory
    // entries yielded beneath it consume another entry each.
    state.scanned_entries = state.scanned_entries.saturating_add(1);
    let metadata = match file_system.symlink_metadata(&target.path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return match target.missing {
                MissingRoot::NotCreated => (Some(0), Some(StorageScanIssueV4::NotCreated)),
                MissingRoot::Unknown => (None, Some(StorageScanIssueV4::Missing)),
            };
        }
        Err(_) => return (None, Some(StorageScanIssueV4::Unreadable)),
    };
    if is_link_or_reparse(&metadata) {
        state.skipped_links = state.skipped_links.saturating_add(1);
        return (Some(0), None);
    }
    let canonical = match file_system.canonicalize(&target.path) {
        Ok(path) => path,
        Err(_) => return (None, Some(StorageScanIssueV4::Unreadable)),
    };
    if !state.visited.insert(normalized_path_key(&canonical)) {
        return (Some(0), None);
    }
    if metadata.is_file() {
        return (Some(metadata.len()), None);
    }
    if !metadata.is_dir() {
        return (Some(0), None);
    }

    let mut known_bytes = 0_u64;
    let starting_entries = state.scanned_entries;
    let mut stack = vec![canonical];
    while let Some(directory) = stack.pop() {
        if let Some(issue) = state.next_entry_issue() {
            state.stopped = Some(issue);
            return (Some(known_bytes), Some(issue));
        }
        let read_dir = match file_system.read_dir(&directory) {
            Ok(entries) => entries,
            Err(_) => {
                let known = if state.scanned_entries == starting_entries && known_bytes == 0 {
                    None
                } else {
                    Some(known_bytes)
                };
                return (known, Some(StorageScanIssueV4::Unreadable));
            }
        };
        for next in read_dir {
            if let Some(issue) = state.next_entry_issue() {
                state.stopped = Some(issue);
                return (Some(known_bytes), Some(issue));
            }
            let path = match next {
                Ok(path) => path,
                Err(_) => return (Some(known_bytes), Some(StorageScanIssueV4::Unreadable)),
            };
            state.scanned_entries = state.scanned_entries.saturating_add(1);
            let metadata = match file_system.symlink_metadata(&path) {
                Ok(metadata) => metadata,
                Err(_) => return (Some(known_bytes), Some(StorageScanIssueV4::Unreadable)),
            };
            if is_link_or_reparse(&metadata) {
                state.skipped_links = state.skipped_links.saturating_add(1);
                continue;
            }
            let canonical = match file_system.canonicalize(&path) {
                Ok(path) => path,
                Err(_) => return (Some(known_bytes), Some(StorageScanIssueV4::Unreadable)),
            };
            if !state.visited.insert(normalized_path_key(&canonical)) {
                continue;
            }
            if metadata.is_file() {
                known_bytes = known_bytes.saturating_add(metadata.len());
            } else if metadata.is_dir() {
                stack.push(canonical);
            }
        }
    }
    (Some(known_bytes), None)
}

fn normalized_path_key(path: &Path) -> String {
    let value = path.to_string_lossy();
    #[cfg(windows)]
    {
        value.replace('/', "\\").to_lowercase()
    }
    #[cfg(not(windows))]
    {
        value.into_owned()
    }
}

fn is_link_or_reparse(metadata: &fs::Metadata) -> bool {
    if metadata.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
        metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
    }
    #[cfg(not(windows))]
    {
        false
    }
}

pub(crate) async fn settings_storage_usage_response(
    repository: &Store,
    app_data_dir: PathBuf,
    project_id: Option<Uuid>,
) -> Result<StorageUsageSnapshotV4, String> {
    let mut projects = repository
        .list_projects()
        .await
        .map_err(|error| error.to_string())?;
    projects.sort_by(|left, right| {
        normalized_path_key(Path::new(&left.local_root))
            .cmp(&normalized_path_key(Path::new(&right.local_root)))
            .then_with(|| left.id.cmp(&right.id))
    });
    let selected = project_id
        .map(|id| {
            projects
                .iter()
                .find(|project| project.id == id)
                .cloned()
                .ok_or_else(|| "project was not found".to_string())
        })
        .transpose()?;
    let scope = if selected.is_some() {
        StorageUsageScopeV4::Project
    } else {
        StorageUsageScopeV4::Managed
    };
    tauri::async_runtime::spawn_blocking(move || {
        let targets = storage_targets(&app_data_dir, &projects, selected.as_ref());
        scan_storage_targets(
            scope,
            project_id,
            targets,
            ScanLimits {
                max_entries: STORAGE_SCAN_MAX_ENTRIES,
                max_duration: STORAGE_SCAN_MAX_DURATION,
            },
        )
    })
    .await
    .map_err(|error| format!("storage scan task failed: {error}"))
}

fn storage_targets(
    app_data_dir: &Path,
    projects: &[omicsops_core::workspace::Project],
    selected: Option<&omicsops_core::workspace::Project>,
) -> Vec<StorageTarget> {
    let mut targets = ["omicsops.db", "omicsops.db-wal", "omicsops.db-shm"]
        .into_iter()
        .map(|name| StorageTarget {
            category: StorageUsageCategoryV4::Database,
            project_id: None,
            path: app_data_dir.join(name),
            missing: MissingRoot::NotCreated,
        })
        .collect::<Vec<_>>();
    // BrowserRuntime stores its isolated workspace profile below this exact
    // app-data root. It never includes a user's ordinary browser profile.
    targets.extend([
        StorageTarget {
            category: StorageUsageCategoryV4::Skills,
            project_id: None,
            path: app_data_dir.join("skills"),
            missing: MissingRoot::NotCreated,
        },
        StorageTarget {
            category: StorageUsageCategoryV4::Browser,
            project_id: None,
            path: app_data_dir.join("browser"),
            missing: MissingRoot::NotCreated,
        },
    ]);
    if let Some(project) = selected {
        targets.push(StorageTarget {
            category: StorageUsageCategoryV4::ProjectRoot,
            project_id: Some(project.id),
            path: PathBuf::from(&project.local_root),
            missing: MissingRoot::Unknown,
        });
    } else {
        targets.extend(projects.iter().map(project_metadata_target));
    }
    targets
}

fn project_metadata_target(project: &omicsops_core::workspace::Project) -> StorageTarget {
    let project_root = PathBuf::from(&project.local_root);
    let metadata = fs::symlink_metadata(&project_root);
    let root_is_link = metadata.as_ref().is_ok_and(is_link_or_reparse);
    if root_is_link {
        return StorageTarget {
            category: StorageUsageCategoryV4::ProjectMetadata,
            project_id: Some(project.id),
            path: project_root,
            missing: MissingRoot::Unknown,
        };
    }
    let root_is_directory = metadata.as_ref().is_ok_and(fs::Metadata::is_dir);
    StorageTarget {
        category: StorageUsageCategoryV4::ProjectMetadata,
        project_id: Some(project.id),
        path: project_root.join(".omicsops"),
        missing: if root_is_directory {
            MissingRoot::NotCreated
        } else {
            MissingRoot::Unknown
        },
    }
}

#[tauri::command]
pub async fn settings_storage_usage(
    app: AppHandle,
    state: State<'_, AppState>,
    project_id: Option<Uuid>,
) -> Result<StorageUsageSnapshotV4, String> {
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?;
    settings_storage_usage_response(&state.repository, app_data_dir, project_id).await
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::Duration;

    use omicsops_core::workspace::{Project, ProjectTemplate};
    use omicsops_dto::{
        StorageScanIssueV4, StorageUsageCategoryV4, StorageUsageScopeV4, StorageUsageStatusV4,
    };
    use omicsops_store::Store;
    use uuid::Uuid;

    use super::*;

    fn limits(max_entries: u64) -> ScanLimits {
        ScanLimits {
            max_entries,
            max_duration: Duration::from_secs(30),
        }
    }

    fn target(
        category: StorageUsageCategoryV4,
        path: impl Into<PathBuf>,
        missing: MissingRoot,
    ) -> StorageTarget {
        StorageTarget {
            category,
            project_id: None,
            path: path.into(),
            missing,
        }
    }

    #[test]
    fn scanner_counts_logical_bytes_once_across_duplicate_and_nested_roots() {
        let directory = tempfile::tempdir().unwrap();
        let nested = directory.path().join("nested");
        fs::create_dir(&nested).unwrap();
        fs::write(directory.path().join("root.bin"), [1_u8; 3]).unwrap();
        fs::write(nested.join("nested.bin"), [2_u8; 5]).unwrap();

        let snapshot = scan_storage_targets(
            StorageUsageScopeV4::Managed,
            None,
            vec![
                target(
                    StorageUsageCategoryV4::ProjectMetadata,
                    directory.path(),
                    MissingRoot::Unknown,
                ),
                target(
                    StorageUsageCategoryV4::ProjectMetadata,
                    &nested,
                    MissingRoot::Unknown,
                ),
                target(
                    StorageUsageCategoryV4::ProjectMetadata,
                    directory.path(),
                    MissingRoot::Unknown,
                ),
            ],
            limits(100),
        );

        assert_eq!(snapshot.known_logical_bytes, 8);
        assert_eq!(
            snapshot
                .entries
                .iter()
                .filter_map(|entry| entry.known_logical_bytes)
                .sum::<u64>(),
            8
        );
        assert_eq!(snapshot.status, StorageUsageStatusV4::Complete);
    }

    #[test]
    fn scanner_reports_missing_managed_roots_as_known_not_created() {
        let directory = tempfile::tempdir().unwrap();
        let missing = directory.path().join("optional-browser-data");

        let snapshot = scan_storage_targets(
            StorageUsageScopeV4::Managed,
            None,
            vec![target(
                StorageUsageCategoryV4::Browser,
                missing,
                MissingRoot::NotCreated,
            )],
            limits(100),
        );

        assert_eq!(snapshot.status, StorageUsageStatusV4::Complete);
        assert_eq!(snapshot.entries[0].known_logical_bytes, Some(0));
        assert_eq!(
            snapshot.entries[0].issue,
            Some(StorageScanIssueV4::NotCreated)
        );
    }

    #[test]
    fn scanner_reports_a_missing_project_root_as_partial_unknown() {
        let directory = tempfile::tempdir().unwrap();
        let project_id = Uuid::from_u128(1);
        let missing = directory.path().join("missing-project");
        let mut project_root = target(
            StorageUsageCategoryV4::ProjectRoot,
            missing,
            MissingRoot::Unknown,
        );
        project_root.project_id = Some(project_id);

        let snapshot = scan_storage_targets(
            StorageUsageScopeV4::Project,
            Some(project_id),
            vec![project_root],
            limits(100),
        );

        assert_eq!(snapshot.status, StorageUsageStatusV4::Partial);
        assert_eq!(snapshot.entries[0].known_logical_bytes, None);
        assert_eq!(snapshot.entries[0].issue, Some(StorageScanIssueV4::Missing));
    }

    #[test]
    fn scanner_stops_at_the_global_entry_limit_with_a_known_subtotal() {
        let directory = tempfile::tempdir().unwrap();
        for name in ["a.bin", "b.bin", "c.bin"] {
            fs::write(directory.path().join(name), [1_u8]).unwrap();
        }

        let snapshot = scan_storage_targets(
            StorageUsageScopeV4::Project,
            Some(Uuid::from_u128(1)),
            vec![target(
                StorageUsageCategoryV4::ProjectRoot,
                directory.path(),
                MissingRoot::Unknown,
            )],
            limits(3),
        );

        assert_eq!(snapshot.status, StorageUsageStatusV4::Partial);
        assert_eq!(snapshot.known_logical_bytes, 2);
        assert_eq!(snapshot.scanned_entries, 3);
        assert_eq!(
            snapshot.entries[0].issue,
            Some(StorageScanIssueV4::EntryLimit)
        );
    }

    #[test]
    fn scanner_stops_at_the_time_limit_without_claiming_completeness() {
        let directory = tempfile::tempdir().unwrap();
        fs::write(directory.path().join("data.bin"), [1_u8; 4]).unwrap();

        let snapshot = scan_storage_targets(
            StorageUsageScopeV4::Project,
            Some(Uuid::from_u128(1)),
            vec![target(
                StorageUsageCategoryV4::ProjectRoot,
                directory.path(),
                MissingRoot::Unknown,
            )],
            ScanLimits {
                max_entries: 100,
                max_duration: Duration::ZERO,
            },
        );

        assert_eq!(snapshot.status, StorageUsageStatusV4::Partial);
        assert_eq!(snapshot.entries[0].known_logical_bytes, None);
        assert_eq!(
            snapshot.entries[0].issue,
            Some(StorageScanIssueV4::TimeLimit)
        );
    }

    #[test]
    fn empty_target_roots_still_consume_the_global_entry_budget() {
        let directory = tempfile::tempdir().unwrap();
        let first = directory.path().join("first");
        let second = directory.path().join("second");
        fs::create_dir(&first).unwrap();
        fs::create_dir(&second).unwrap();

        let snapshot = scan_storage_targets(
            StorageUsageScopeV4::Managed,
            None,
            vec![
                target(StorageUsageCategoryV4::Skills, first, MissingRoot::Unknown),
                target(
                    StorageUsageCategoryV4::Browser,
                    second,
                    MissingRoot::Unknown,
                ),
            ],
            limits(2),
        );

        assert_eq!(snapshot.status, StorageUsageStatusV4::Partial);
        assert_eq!(snapshot.scanned_entries, 2);
        assert_eq!(
            snapshot.entries[1].issue,
            Some(StorageScanIssueV4::EntryLimit)
        );
    }

    #[test]
    fn scanner_turns_directory_read_failures_into_partial_unknown_entries() {
        let directory = tempfile::tempdir().unwrap();
        let denied = directory.path().join("denied");
        fs::create_dir(&denied).unwrap();
        let file_system = DeniedFileSystem {
            denied: fs::canonicalize(&denied).unwrap(),
        };

        let snapshot = scan_storage_targets_with_file_system(
            StorageUsageScopeV4::Project,
            Some(Uuid::from_u128(1)),
            vec![target(
                StorageUsageCategoryV4::ProjectRoot,
                denied,
                MissingRoot::Unknown,
            )],
            limits(100),
            &file_system,
        );

        assert_eq!(snapshot.status, StorageUsageStatusV4::Partial);
        assert_eq!(snapshot.entries[0].known_logical_bytes, None);
        assert_eq!(
            snapshot.entries[0].issue,
            Some(StorageScanIssueV4::Unreadable)
        );
    }

    #[test]
    fn earlier_categories_do_not_make_an_unreadable_root_look_like_known_zero() {
        let directory = tempfile::tempdir().unwrap();
        let known_file = directory.path().join("known.bin");
        let denied = directory.path().join("denied");
        fs::write(&known_file, [1_u8; 7]).unwrap();
        fs::create_dir(&denied).unwrap();
        let file_system = DeniedFileSystem {
            denied: fs::canonicalize(&denied).unwrap(),
        };

        let snapshot = scan_storage_targets_with_file_system(
            StorageUsageScopeV4::Project,
            Some(Uuid::from_u128(1)),
            vec![
                target(
                    StorageUsageCategoryV4::Database,
                    known_file,
                    MissingRoot::Unknown,
                ),
                target(
                    StorageUsageCategoryV4::ProjectRoot,
                    denied,
                    MissingRoot::Unknown,
                ),
            ],
            limits(100),
            &file_system,
        );

        assert_eq!(snapshot.entries[0].known_logical_bytes, Some(7));
        assert_eq!(snapshot.entries[1].known_logical_bytes, None);
        assert_eq!(snapshot.known_logical_bytes, 7);
    }

    #[test]
    fn scanner_does_not_follow_directory_links() {
        let directory = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        fs::write(directory.path().join("inside.bin"), [1_u8; 3]).unwrap();
        fs::write(outside.path().join("outside.bin"), [2_u8; 11]).unwrap();
        let link = directory.path().join("linked-outside");
        if create_directory_link(outside.path(), &link).is_err() {
            return;
        }

        let snapshot = scan_storage_targets(
            StorageUsageScopeV4::Project,
            Some(Uuid::from_u128(1)),
            vec![target(
                StorageUsageCategoryV4::ProjectRoot,
                directory.path(),
                MissingRoot::Unknown,
            )],
            limits(100),
        );

        assert_eq!(snapshot.known_logical_bytes, 3);
        assert_eq!(snapshot.skipped_links, 1);
        assert_eq!(snapshot.status, StorageUsageStatusV4::Complete);
    }

    #[tokio::test]
    async fn managed_scope_counts_only_app_data_and_project_metadata() {
        let app_data = tempfile::tempdir().unwrap();
        let project_root = tempfile::tempdir().unwrap();
        let metadata = project_root.path().join(".omicsops");
        fs::create_dir(&metadata).unwrap();
        fs::write(metadata.join("meta.bin"), [1_u8; 4]).unwrap();
        fs::write(project_root.path().join("sample.bin"), [1_u8; 100]).unwrap();
        write_managed_app_data(app_data.path());
        let store = Store::open_in_memory().await.unwrap();
        let project = Project::new(
            Uuid::from_u128(1),
            "managed project",
            project_root.path().to_string_lossy(),
            ProjectTemplate::Blank,
            chrono::Utc::now(),
        );
        store.save_project(&project).await.unwrap();

        let snapshot = settings_storage_usage_response(&store, app_data.path().into(), None)
            .await
            .unwrap();

        assert_eq!(snapshot.scope, StorageUsageScopeV4::Managed);
        assert_eq!(snapshot.known_logical_bytes, 21);
        assert_eq!(snapshot.status, StorageUsageStatusV4::Complete);
        assert!(snapshot.entries.iter().any(|entry| {
            entry.category == StorageUsageCategoryV4::ProjectMetadata
                && entry.project_id == Some(project.id)
                && entry.known_logical_bytes == Some(4)
        }));
    }

    #[tokio::test]
    async fn managed_scope_marks_metadata_unknown_when_the_project_root_is_missing() {
        let app_data = tempfile::tempdir().unwrap();
        let missing_root = app_data.path().join("missing-project-root");
        let store = Store::open_in_memory().await.unwrap();
        let project = Project::new(
            Uuid::from_u128(1),
            "missing project",
            missing_root.to_string_lossy(),
            ProjectTemplate::Blank,
            chrono::Utc::now(),
        );
        store.save_project(&project).await.unwrap();

        let snapshot = settings_storage_usage_response(&store, app_data.path().into(), None)
            .await
            .unwrap();
        let entry = snapshot
            .entries
            .iter()
            .find(|entry| entry.category == StorageUsageCategoryV4::ProjectMetadata)
            .unwrap();

        assert_eq!(snapshot.status, StorageUsageStatusV4::Partial);
        assert_eq!(entry.known_logical_bytes, None);
        assert_eq!(entry.issue, Some(StorageScanIssueV4::Missing));
    }

    #[tokio::test]
    async fn managed_scope_does_not_follow_a_linked_project_root_to_metadata() {
        let app_data = tempfile::tempdir().unwrap();
        let projects = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        fs::create_dir(outside.path().join(".omicsops")).unwrap();
        fs::write(
            outside.path().join(".omicsops").join("outside.bin"),
            [1_u8; 17],
        )
        .unwrap();
        let linked_root = projects.path().join("linked-project");
        if create_directory_link(outside.path(), &linked_root).is_err() {
            return;
        }
        let store = Store::open_in_memory().await.unwrap();
        let project = Project::new(
            Uuid::from_u128(1),
            "linked project",
            linked_root.to_string_lossy(),
            ProjectTemplate::Blank,
            chrono::Utc::now(),
        );
        store.save_project(&project).await.unwrap();

        let snapshot = settings_storage_usage_response(&store, app_data.path().into(), None)
            .await
            .unwrap();
        let entry = snapshot
            .entries
            .iter()
            .find(|entry| entry.category == StorageUsageCategoryV4::ProjectMetadata)
            .unwrap();

        assert_eq!(entry.known_logical_bytes, Some(0));
        assert_eq!(entry.skipped_links, 1);
        assert_ne!(snapshot.known_logical_bytes, 17);
    }

    #[tokio::test]
    async fn project_scope_scans_only_the_selected_full_project_root() {
        let app_data = tempfile::tempdir().unwrap();
        let selected_root = tempfile::tempdir().unwrap();
        let other_root = tempfile::tempdir().unwrap();
        fs::create_dir(selected_root.path().join(".omicsops")).unwrap();
        fs::write(selected_root.path().join("sample.bin"), [1_u8; 100]).unwrap();
        fs::write(
            selected_root.path().join(".omicsops").join("meta.bin"),
            [1_u8; 4],
        )
        .unwrap();
        fs::write(other_root.path().join("other.bin"), [1_u8; 90]).unwrap();
        write_managed_app_data(app_data.path());
        let store = Store::open_in_memory().await.unwrap();
        let selected = Project::new(
            Uuid::from_u128(1),
            "selected",
            selected_root.path().to_string_lossy(),
            ProjectTemplate::Blank,
            chrono::Utc::now(),
        );
        let other = Project::new(
            Uuid::from_u128(2),
            "other",
            other_root.path().to_string_lossy(),
            ProjectTemplate::Blank,
            chrono::Utc::now(),
        );
        store.save_project(&selected).await.unwrap();
        store.save_project(&other).await.unwrap();

        let snapshot =
            settings_storage_usage_response(&store, app_data.path().into(), Some(selected.id))
                .await
                .unwrap();

        assert_eq!(snapshot.scope, StorageUsageScopeV4::Project);
        assert_eq!(snapshot.project_id, Some(selected.id));
        assert_eq!(snapshot.known_logical_bytes, 121);
        assert_eq!(
            snapshot
                .entries
                .iter()
                .filter(|entry| entry.category == StorageUsageCategoryV4::ProjectRoot)
                .count(),
            1
        );
    }

    #[tokio::test]
    async fn project_scope_rejects_an_unknown_project_id() {
        let app_data = tempfile::tempdir().unwrap();
        let store = Store::open_in_memory().await.unwrap();

        let error = settings_storage_usage_response(
            &store,
            app_data.path().into(),
            Some(Uuid::from_u128(99)),
        )
        .await
        .unwrap_err();

        assert_eq!(error, "project was not found");
    }

    fn write_managed_app_data(app_data: &Path) {
        fs::write(app_data.join("omicsops.db"), [1_u8; 3]).unwrap();
        fs::write(app_data.join("omicsops.db-wal"), [1_u8; 2]).unwrap();
        fs::write(app_data.join("omicsops.db-shm"), [1_u8; 1]).unwrap();
        fs::create_dir(app_data.join("skills")).unwrap();
        fs::write(app_data.join("skills").join("skill.bin"), [1_u8; 5]).unwrap();
        fs::create_dir_all(app_data.join("browser").join("workspace-profile")).unwrap();
        fs::write(
            app_data
                .join("browser")
                .join("workspace-profile")
                .join("state.bin"),
            [1_u8; 6],
        )
        .unwrap();
    }

    struct DeniedFileSystem {
        denied: PathBuf,
    }

    impl StorageFileSystem for DeniedFileSystem {
        fn symlink_metadata(&self, path: &Path) -> std::io::Result<fs::Metadata> {
            fs::symlink_metadata(path)
        }

        fn canonicalize(&self, path: &Path) -> std::io::Result<PathBuf> {
            fs::canonicalize(path)
        }

        fn read_dir(&self, path: &Path) -> std::io::Result<DirectoryEntries> {
            if path == self.denied {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    "denied for test",
                ));
            }
            RealFileSystem.read_dir(path)
        }
    }

    #[cfg(unix)]
    fn create_directory_link(target: &Path, link: &Path) -> std::io::Result<()> {
        std::os::unix::fs::symlink(target, link)
    }

    #[cfg(windows)]
    fn create_directory_link(target: &Path, link: &Path) -> std::io::Result<()> {
        std::os::windows::fs::symlink_dir(target, link)
    }
}
