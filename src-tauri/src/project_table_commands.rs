//! Authoritative bounded table projection. Raw source bytes stay host-only.
use crate::{
    commands::{AppState, connect_profile, find_profile, require_trusted_host},
    composer_files::{FileBackend, normalize_workspace_relative_path, validate_backend_binding},
    project_file_read::{FileReadError, ValidatedProjectFile, is_reserved_component},
    project_table::{TABLE_PREVIEW_MAX_BYTES, TableDelimiter, parse_project_table},
    sync_commands::{canonical_remote_file_for_composer, canonical_remote_root_for_composer},
};
use async_trait::async_trait;
use omicsops_adapters::{
    AdapterError,
    ssh::{REMOTE_FILE_PREFIX_CHANGED, SshSession},
};
use omicsops_core::{project::shell_quote, workspace::Project};
use omicsops_dto::{
    ProjectTablePreview, ProjectTablePreviewRequest, TablePreviewErrorCode, TablePreviewHashScope,
    TablePreviewStability, TablePreviewTruncation,
};
use omicsops_store::Store;
use sha2::{Digest, Sha256};
use std::time::Duration;
use tauri::State;

const SSH_SOURCE_TIMEOUT: Duration = Duration::from_secs(20);
const CLEANUP_TIMEOUT: Duration = Duration::from_secs(1);

#[derive(Debug)]
pub(crate) struct TableSourceRead {
    pub bytes: Vec<u8>,
    pub size_bytes: Option<u64>,
    pub eof: bool,
    pub stable: bool,
}

#[async_trait]
pub(crate) trait ProjectTableSource: Send + Sync {
    async fn read_prefix(
        &self,
        project: &Project,
        backend: &FileBackend,
        relative_path: &str,
        max_bytes: usize,
    ) -> Result<TableSourceRead, TablePreviewErrorCode>;
}

struct HostTableSource<'a>(&'a AppState);
#[async_trait]
impl ProjectTableSource for HostTableSource<'_> {
    async fn read_prefix(
        &self,
        project: &Project,
        backend: &FileBackend,
        relative_path: &str,
        max_bytes: usize,
    ) -> Result<TableSourceRead, TablePreviewErrorCode> {
        match backend {
            FileBackend::Local => {
                let project = project.clone();
                let path = relative_path.to_owned();
                tokio::task::spawn_blocking(move || {
                    let read = ValidatedProjectFile::open(&project, &path)
                        .and_then(|mut file| file.read_prefix(max_bytes))
                        .map_err(local_error)?;
                    Ok(TableSourceRead {
                        bytes: read.bytes,
                        size_bytes: Some(read.size_bytes),
                        eof: read.eof,
                        stable: true,
                    })
                })
                .await
                .map_err(|_| TablePreviewErrorCode::ReadFailed)?
            }
            FileBackend::Ssh(connection_id) => {
                let connector = HostRemoteConnector {
                    state: self.0,
                    connection_id: *connection_id,
                };
                read_remote_table(
                    &connector,
                    project
                        .remote_root
                        .as_deref()
                        .ok_or(TablePreviewErrorCode::UnavailableBackend)?,
                    relative_path,
                    max_bytes,
                )
                .await
            }
        }
    }
}

fn local_error(error: FileReadError) -> TablePreviewErrorCode {
    match error {
        FileReadError::InvalidPath | FileReadError::UnsafeFile | FileReadError::NotRegular => {
            TablePreviewErrorCode::InvalidPath
        }
        FileReadError::SourceChanged => TablePreviewErrorCode::SourceChanged,
        _ => TablePreviewErrorCode::ReadFailed,
    }
}
fn remote_error(error: AdapterError) -> TablePreviewErrorCode {
    match error {
        AdapterError::Ssh(code) if code == REMOTE_FILE_PREFIX_CHANGED => {
            TablePreviewErrorCode::SourceChanged
        }
        _ => TablePreviewErrorCode::ReadFailed,
    }
}
fn error_code(code: TablePreviewErrorCode) -> String {
    // Explicit stable codes; never serialize native errors or source text.
    match code {
        TablePreviewErrorCode::InvalidPath => "invalid_path",
        TablePreviewErrorCode::UnsupportedFormat => "unsupported_format",
        TablePreviewErrorCode::InvalidUtf8 => "invalid_utf8",
        TablePreviewErrorCode::MalformedTable => "malformed_table",
        TablePreviewErrorCode::ColumnLimit => "column_limit",
        TablePreviewErrorCode::SourceChanged => "source_changed",
        TablePreviewErrorCode::ReadFailed => "read_failed",
        TablePreviewErrorCode::Timeout => "timeout",
        TablePreviewErrorCode::UnavailableBackend => "unavailable_backend",
    }
    .into()
}

#[tauri::command]
pub async fn preview_project_table(
    state: State<'_, AppState>,
    request: ProjectTablePreviewRequest,
) -> Result<ProjectTablePreview, String> {
    preview_project_table_with_source(&state.repository, &HostTableSource(&state), request).await
}

pub(crate) async fn preview_project_table_with_source(
    store: &Store,
    source: &dyn ProjectTableSource,
    request: ProjectTablePreviewRequest,
) -> Result<ProjectTablePreview, String> {
    preview_with_source(store, source, request)
        .await
        .map_err(error_code)
}
async fn preview_with_source(
    store: &Store,
    source: &dyn ProjectTableSource,
    request: ProjectTablePreviewRequest,
) -> Result<ProjectTablePreview, TablePreviewErrorCode> {
    let project = store
        .get_project(request.project_id)
        .await
        .map_err(|_| TablePreviewErrorCode::UnavailableBackend)?
        .ok_or(TablePreviewErrorCode::UnavailableBackend)?;
    let connections = store
        .list_connections()
        .await
        .map_err(|_| TablePreviewErrorCode::UnavailableBackend)?;
    let backend = validate_backend_binding(&project, &connections, &request.backend_id)
        .map_err(|_| TablePreviewErrorCode::UnavailableBackend)?;
    let path = normalize_workspace_relative_path(&request.relative_path)
        .map_err(|_| TablePreviewErrorCode::InvalidPath)?;
    if path.split('/').any(is_reserved_component) {
        return Err(TablePreviewErrorCode::InvalidPath);
    }
    let delimiter = match path
        .rsplit('.')
        .next()
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("csv") => TableDelimiter::Csv,
        Some("tsv") => TableDelimiter::Tsv,
        _ => return Err(TablePreviewErrorCode::UnsupportedFormat),
    };
    let read = source
        .read_prefix(&project, &backend, &path, TABLE_PREVIEW_MAX_BYTES)
        .await?;
    if read.bytes.len() > TABLE_PREVIEW_MAX_BYTES {
        return Err(TablePreviewErrorCode::ReadFailed);
    }
    let read_bytes = read.bytes.len() as u64;
    if read.size_bytes.is_some_and(|size| {
        size < read_bytes || (read.eof && size != read_bytes) || (!read.eof && size == read_bytes)
    }) {
        return Err(TablePreviewErrorCode::SourceChanged);
    }
    let mut parsed = parse_project_table(&read.bytes, delimiter, read.eof)?;
    for cell in parsed
        .headers
        .iter_mut()
        .chain(parsed.rows.iter_mut().flatten())
    {
        *cell = project_cell(cell, &mut parsed.truncation_reasons);
    }
    Ok(ProjectTablePreview {
        project_id: request.project_id,
        backend_id: request.backend_id,
        relative_path: path,
        size_bytes: read.size_bytes,
        read_bytes,
        parsed_bytes: parsed.parsed_bytes,
        sha256: hex::encode(Sha256::digest(&read.bytes)),
        hash_scope: if read.eof && read.stable {
            TablePreviewHashScope::Full
        } else {
            TablePreviewHashScope::Prefix
        },
        displayed_rows: parsed.rows.len() as u32,
        headers: parsed.headers,
        rows: parsed.rows,
        truncation_reasons: parsed.truncation_reasons,
        source_stability: if read.stable {
            TablePreviewStability::Checked
        } else {
            TablePreviewStability::Unknown
        },
    })
}
fn project_cell(raw: &str, reasons: &mut Vec<TablePreviewTruncation>) -> String {
    let mut text = crate::composer_references::public_text(raw);
    if text.len() > 4096 {
        let mut end = 4096;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        text.truncate(end);
        if !reasons.contains(&TablePreviewTruncation::CellLimit) {
            reasons.push(TablePreviewTruncation::CellLimit);
        }
    }
    text
}

#[async_trait]
trait RemoteTableConnector: Send + Sync {
    async fn connect(&self) -> Result<Box<dyn RemoteTableSession>, TablePreviewErrorCode>;
}
#[async_trait]
trait RemoteTableSession: Send + Sync {
    async fn canonical_root(&self, root: &str) -> Result<String, TablePreviewErrorCode>;
    async fn canonical_file(&self, root: &str, path: &str)
    -> Result<String, TablePreviewErrorCode>;
    async fn read_prefix(
        &self,
        path: &str,
        budget: usize,
    ) -> Result<TableSourceRead, TablePreviewErrorCode>;
    async fn disconnect(self: Box<Self>) -> Result<(), TablePreviewErrorCode>;
}
struct HostRemoteConnector<'a> {
    state: &'a AppState,
    connection_id: uuid::Uuid,
}
#[async_trait]
impl RemoteTableConnector for HostRemoteConnector<'_> {
    async fn connect(&self) -> Result<Box<dyn RemoteTableSession>, TablePreviewErrorCode> {
        let profile = find_profile(&self.state.repository, self.connection_id)
            .await
            .map_err(|_| TablePreviewErrorCode::UnavailableBackend)?;
        require_trusted_host(&profile).map_err(|_| TablePreviewErrorCode::UnavailableBackend)?;
        Ok(Box::new(
            connect_profile(self.state, &profile)
                .await
                .map_err(|_| TablePreviewErrorCode::ReadFailed)?,
        ))
    }
}
#[async_trait]
impl RemoteTableSession for SshSession {
    async fn canonical_root(&self, root: &str) -> Result<String, TablePreviewErrorCode> {
        validate_remote_table_root(self, root).await
    }
    async fn canonical_file(
        &self,
        root: &str,
        path: &str,
    ) -> Result<String, TablePreviewErrorCode> {
        let resolved = canonical_remote_file_for_composer(self, root, path)
            .await
            .map_err(|_| TablePreviewErrorCode::InvalidPath)?;
        // Enforce the internal-path rule on the resolved spelling as well.
        let relative = resolved
            .strip_prefix(&format!("{}/", root.trim_end_matches('/')))
            .ok_or(TablePreviewErrorCode::InvalidPath)?;
        normalize_workspace_relative_path(relative)
            .map_err(|_| TablePreviewErrorCode::InvalidPath)?;
        Ok(resolved)
    }
    async fn read_prefix(
        &self,
        path: &str,
        budget: usize,
    ) -> Result<TableSourceRead, TablePreviewErrorCode> {
        let read = self
            .read_file_prefix(path, budget as u64)
            .await
            .map_err(remote_error)?;
        Ok(TableSourceRead {
            bytes: read.bytes,
            size_bytes: read.size_bytes,
            eof: read.eof,
            stable: read.metadata_stable,
        })
    }
    async fn disconnect(self: Box<Self>) -> Result<(), TablePreviewErrorCode> {
        (*self).disconnect().await.map_err(remote_error)
    }
}

#[async_trait]
trait RemoteRootInspector: Send + Sync {
    async fn reject_symlink(&self, path: &str) -> Result<(), TablePreviewErrorCode>;
    async fn canonicalize(&self, root: &str) -> Result<String, TablePreviewErrorCode>;
}
#[async_trait]
impl RemoteRootInspector for SshSession {
    async fn reject_symlink(&self, path: &str) -> Result<(), TablePreviewErrorCode> {
        self.execute_checked(&format!("test ! -L {}", shell_quote(path)))
            .await
            .map_err(|_| TablePreviewErrorCode::InvalidPath)?;
        Ok(())
    }
    async fn canonicalize(&self, root: &str) -> Result<String, TablePreviewErrorCode> {
        canonical_remote_root_for_composer(self, root)
            .await
            .map_err(|_| TablePreviewErrorCode::InvalidPath)
    }
}
async fn validate_remote_table_root(
    inspector: &impl RemoteRootInspector,
    root: &str,
) -> Result<String, TablePreviewErrorCode> {
    // Inspect the supplied spelling before realpath, which would conceal links.
    for ancestor in remote_root_ancestors(root)? {
        inspector.reject_symlink(&ancestor).await?;
    }
    inspector.canonicalize(root).await
}
fn remote_root_ancestors(root: &str) -> Result<Vec<String>, TablePreviewErrorCode> {
    if !root.starts_with('/') || root.starts_with("//") || root.chars().any(char::is_control) {
        return Err(TablePreviewErrorCode::InvalidPath);
    }
    if root == "/" {
        return Ok(vec!["/".into()]);
    }
    let mut paths = Vec::new();
    let mut current = String::new();
    for component in root.trim_end_matches('/')[1..].split('/') {
        if component.is_empty() || component == "." || component == ".." {
            return Err(TablePreviewErrorCode::InvalidPath);
        }
        current.push('/');
        current.push_str(component);
        paths.push(current.clone());
    }
    Ok(paths)
}
async fn read_remote_table(
    connector: &dyn RemoteTableConnector,
    root: &str,
    path: &str,
    budget: usize,
) -> Result<TableSourceRead, TablePreviewErrorCode> {
    let deadline = tokio::time::Instant::now() + SSH_SOURCE_TIMEOUT;
    let mut session: Option<Box<dyn RemoteTableSession>> = None;
    let read = tokio::time::timeout_at(deadline, async {
        session = Some(connector.connect().await?);
        let connected = session.as_ref().expect("connected session");
        let root = connected.canonical_root(root).await?;
        let file = connected.canonical_file(&root, path).await?;
        connected.read_prefix(&file, budget).await
    })
    .await;
    match read {
        Err(_) => {
            if let Some(session) = session.take() {
                // Stop local waiting at the deadline. Cleanup is a bounded
                // connection close, never a remote job cancellation or retry.
                tokio::spawn(async move {
                    let _ = tokio::time::timeout(CLEANUP_TIMEOUT, session.disconnect()).await;
                });
            }
            Err(TablePreviewErrorCode::Timeout)
        }
        Ok(result) => {
            if let Some(session) = session.take() {
                let closed = tokio::time::timeout_at(deadline, session.disconnect())
                    .await
                    .map_err(|_| TablePreviewErrorCode::Timeout)?;
                result.and_then(|read| closed.map(|_| read))
            } else {
                result
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use omicsops_core::workspace::ProjectTemplate;
    use std::sync::Mutex;
    use uuid::Uuid;

    struct FakeSource {
        result: Mutex<Option<Result<TableSourceRead, TablePreviewErrorCode>>>,
        calls: Mutex<Vec<(FileBackend, String, usize)>>,
    }
    #[async_trait::async_trait]
    impl ProjectTableSource for FakeSource {
        async fn read_prefix(
            &self,
            _project: &Project,
            backend: &FileBackend,
            path: &str,
            budget: usize,
        ) -> Result<TableSourceRead, TablePreviewErrorCode> {
            self.calls
                .lock()
                .unwrap()
                .push((*backend, path.into(), budget));
            self.result.lock().unwrap().take().unwrap()
        }
    }
    fn fake(bytes: Vec<u8>, eof: bool, stable: bool, size: Option<u64>) -> FakeSource {
        FakeSource {
            result: Mutex::new(Some(Ok(TableSourceRead {
                bytes,
                eof,
                stable,
                size_bytes: size,
            }))),
            calls: Mutex::new(vec![]),
        }
    }
    async fn fixture() -> (tempfile::TempDir, Store, ProjectTablePreviewRequest) {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(root.path().join("state.sqlite")).await.unwrap();
        let project = Project::new(
            Uuid::new_v4(),
            "table",
            root.path().to_str().unwrap(),
            ProjectTemplate::Blank,
            chrono::Utc::now(),
        );
        store.save_project(&project).await.unwrap();
        let request = ProjectTablePreviewRequest {
            project_id: project.id,
            backend_id: "local".into(),
            relative_path: "table.csv".into(),
        };
        (root, store, request)
    }
    #[tokio::test]
    async fn host_budget_binding_and_raw_hash_are_authoritative() {
        let (_root, store, request) = fixture().await;
        let bytes = b"\xef\xbb\xbfa,b\n1,2\n".to_vec();
        let source = fake(bytes.clone(), true, true, Some(bytes.len() as u64));
        let preview = preview_project_table_with_source(&store, &source, request)
            .await
            .unwrap();
        assert_eq!(
            *source.calls.lock().unwrap(),
            vec![(FileBackend::Local, "table.csv".into(), 256 * 1024)]
        );
        assert_eq!(preview.read_bytes, bytes.len() as u64);
        assert_eq!(preview.sha256, hex::encode(Sha256::digest(&bytes)));
        assert_eq!(preview.hash_scope, TablePreviewHashScope::Full);
        assert_eq!(preview.source_stability, TablePreviewStability::Checked);
        assert_eq!(preview.displayed_rows, 1);
    }
    #[tokio::test]
    async fn invalid_binding_path_and_format_never_call_source() {
        let (_root, store, request) = fixture().await;
        for (backend, path, code) in [
            (
                format!("ssh:{}", Uuid::new_v4()),
                "table.csv",
                TablePreviewErrorCode::UnavailableBackend,
            ),
            (
                "local".into(),
                "../table.csv",
                TablePreviewErrorCode::InvalidPath,
            ),
            (
                "local".into(),
                "table.txt",
                TablePreviewErrorCode::UnsupportedFormat,
            ),
            (
                "local".into(),
                "CON.csv",
                TablePreviewErrorCode::InvalidPath,
            ),
        ] {
            let source = fake(vec![], true, true, Some(0));
            let mut request = request.clone();
            request.backend_id = backend;
            request.relative_path = path.into();
            assert_eq!(
                preview_project_table_with_source(&store, &source, request)
                    .await
                    .unwrap_err(),
                error_code(code)
            );
            assert!(source.calls.lock().unwrap().is_empty());
        }
    }
    #[tokio::test]
    async fn oversized_and_inconsistent_source_results_are_rejected() {
        let (_root, store, request) = fixture().await;
        for source in [
            fake(vec![b'x'; 257 * 1024], false, true, None),
            fake(b"a\n1\n".to_vec(), true, true, Some(9)),
            fake(b"a\n1\n".to_vec(), false, true, Some(2)),
        ] {
            assert!(
                preview_project_table_with_source(&store, &source, request.clone())
                    .await
                    .is_err()
            );
        }
    }
    #[tokio::test]
    async fn unknown_stability_and_non_eof_are_prefix_hashes() {
        let (_root, store, request) = fixture().await;
        for (eof, stable, size) in [(true, false, None), (false, true, Some(100))] {
            let preview = preview_project_table_with_source(
                &store,
                &fake(b"a\n1\n".to_vec(), eof, stable, size),
                request.clone(),
            )
            .await
            .unwrap();
            assert_eq!(preview.hash_scope, TablePreviewHashScope::Prefix);
            assert_eq!(
                preview.source_stability,
                if stable {
                    TablePreviewStability::Checked
                } else {
                    TablePreviewStability::Unknown
                }
            );
        }
    }
    #[tokio::test]
    async fn source_errors_are_fixed_codes_and_adapter_change_code_is_exact() {
        let (_root, store, request) = fixture().await;
        let source = FakeSource {
            result: Mutex::new(Some(Err(TablePreviewErrorCode::ReadFailed))),
            calls: Mutex::new(vec![]),
        };
        assert_eq!(
            preview_project_table_with_source(&store, &source, request)
                .await
                .unwrap_err(),
            "read_failed"
        );
        assert_eq!(
            remote_error(AdapterError::Ssh(REMOTE_FILE_PREFIX_CHANGED.into())),
            TablePreviewErrorCode::SourceChanged
        );
        for error in [
            AdapterError::Ssh("SENTINEL-private-server-output".into()),
            AdapterError::RemoteCommand {
                status: 1,
                stderr: "SENTINEL-secret".into(),
            },
        ] {
            assert_eq!(error_code(remote_error(error)), "read_failed");
        }
    }
    #[tokio::test]
    async fn public_text_precedes_utf8_display_limit() {
        let (_root, store, request) = fixture().await;
        let field = format!("{} sk-proj-SENTINEL-secret-value", "中".repeat(1400));
        let bytes = format!("{field}\n{field}\n").into_bytes();
        let preview = preview_project_table_with_source(
            &store,
            &fake(bytes.clone(), true, true, Some(bytes.len() as u64)),
            request,
        )
        .await
        .unwrap();
        assert!(preview.headers[0].len() <= 4096);
        assert!(preview.rows[0][0].len() <= 4096);
        assert!(
            preview
                .truncation_reasons
                .contains(&TablePreviewTruncation::CellLimit)
        );
        assert!(!format!("{preview:?}").contains("SENTINEL"));
        assert_eq!(preview.sha256, hex::encode(Sha256::digest(&bytes)));
        let mut reasons = vec![];
        assert_eq!(
            project_cell("sk-proj-SENTINEL-secret-value", &mut reasons),
            crate::composer_references::public_text("sk-proj-SENTINEL-secret-value")
        );
        let raw_secret = format!("password={}", "SENTINEL".repeat(1000));
        let mut reasons = vec![];
        let public = project_cell(&raw_secret, &mut reasons);
        assert!(!public.contains("SENTINEL"));
        assert!(public.len() < 4096);
        assert!(reasons.is_empty(), "display limit applies after redaction");
    }
    #[tokio::test]
    async fn raw_hash_includes_unshown_rows_and_partial_utf8_bytes() {
        let (_root, store, request) = fixture().await;
        let bytes = format!("a\n{}unshown", "1\n".repeat(200)).into_bytes();
        let preview = preview_project_table_with_source(
            &store,
            &fake(bytes.clone(), true, true, Some(bytes.len() as u64)),
            request.clone(),
        )
        .await
        .unwrap();
        assert_eq!(preview.rows.len(), 200);
        assert!(preview.parsed_bytes < preview.read_bytes);
        assert_eq!(preview.hash_scope, TablePreviewHashScope::Full);
        assert_eq!(preview.sha256, hex::encode(Sha256::digest(&bytes)));
        let bytes = b"\xef\xbb\xbfa\n1\n\xe4\xb8".to_vec();
        let preview = preview_project_table_with_source(
            &store,
            &fake(bytes.clone(), false, false, None),
            request,
        )
        .await
        .unwrap();
        assert_eq!(preview.sha256, hex::encode(Sha256::digest(&bytes)));
        assert_eq!(preview.parsed_bytes, 7);
        assert_eq!(preview.read_bytes, 9);
    }
    struct FakeConnector {
        connect_delay: std::time::Duration,
        root_delay: std::time::Duration,
        fail_read: bool,
        closes: std::sync::Arc<std::sync::atomic::AtomicUsize>,
        close_delay: std::time::Duration,
        fail_close: bool,
        close_dropped: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    }
    struct FakeSession {
        root_delay: std::time::Duration,
        fail_read: bool,
        closes: std::sync::Arc<std::sync::atomic::AtomicUsize>,
        close_delay: std::time::Duration,
        fail_close: bool,
        close_dropped: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    }
    #[async_trait::async_trait]
    impl RemoteTableConnector for FakeConnector {
        async fn connect(&self) -> Result<Box<dyn RemoteTableSession>, TablePreviewErrorCode> {
            tokio::time::sleep(self.connect_delay).await;
            Ok(Box::new(FakeSession {
                root_delay: self.root_delay,
                fail_read: self.fail_read,
                closes: self.closes.clone(),
                close_delay: self.close_delay,
                fail_close: self.fail_close,
                close_dropped: self.close_dropped.clone(),
            }))
        }
    }
    #[async_trait::async_trait]
    impl RemoteTableSession for FakeSession {
        async fn canonical_root(&self, _root: &str) -> Result<String, TablePreviewErrorCode> {
            tokio::time::sleep(self.root_delay).await;
            Ok("/project".into())
        }
        async fn canonical_file(
            &self,
            _root: &str,
            _path: &str,
        ) -> Result<String, TablePreviewErrorCode> {
            Ok("/project/table.csv".into())
        }
        async fn read_prefix(
            &self,
            _path: &str,
            _budget: usize,
        ) -> Result<TableSourceRead, TablePreviewErrorCode> {
            if self.fail_read {
                Err(TablePreviewErrorCode::ReadFailed)
            } else {
                Ok(TableSourceRead {
                    bytes: b"a\n1\n".to_vec(),
                    size_bytes: Some(4),
                    eof: true,
                    stable: true,
                })
            }
        }
        async fn disconnect(self: Box<Self>) -> Result<(), TablePreviewErrorCode> {
            struct CloseGuard(std::sync::Arc<std::sync::atomic::AtomicUsize>);
            impl Drop for CloseGuard {
                fn drop(&mut self) {
                    self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                }
            }
            let _guard = CloseGuard(self.close_dropped.clone());
            self.closes
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            tokio::time::sleep(self.close_delay).await;
            if self.fail_close {
                Err(TablePreviewErrorCode::ReadFailed)
            } else {
                Ok(())
            }
        }
    }
    #[tokio::test(start_paused = true)]
    async fn whole_ssh_deadline_includes_connect_root_and_failure_cleanup() {
        use std::{
            sync::{
                Arc,
                atomic::{AtomicUsize, Ordering},
            },
            time::Duration,
        };
        for (connect_delay, root_delay, fail_read, expected, closes) in [
            (21, 0, false, TablePreviewErrorCode::Timeout, 0),
            (0, 21, false, TablePreviewErrorCode::Timeout, 1),
            (11, 11, false, TablePreviewErrorCode::Timeout, 1),
            (0, 0, true, TablePreviewErrorCode::ReadFailed, 1),
        ] {
            let count = Arc::new(AtomicUsize::new(0));
            let connector = FakeConnector {
                connect_delay: Duration::from_secs(connect_delay),
                root_delay: Duration::from_secs(root_delay),
                fail_read,
                closes: count.clone(),
                close_delay: Duration::ZERO,
                fail_close: false,
                close_dropped: Arc::new(AtomicUsize::new(0)),
            };
            let started = tokio::time::Instant::now();
            assert_eq!(
                read_remote_table(&connector, "/project", "table.csv", 256 * 1024)
                    .await
                    .unwrap_err(),
                expected
            );
            assert!(started.elapsed() <= Duration::from_secs(20));
            tokio::task::yield_now().await;
            assert_eq!(count.load(Ordering::SeqCst), closes);
        }
    }
    #[tokio::test(start_paused = true)]
    async fn disconnect_failure_and_expiry_are_bounded_without_retry() {
        use std::{
            sync::{
                Arc,
                atomic::{AtomicUsize, Ordering},
            },
            time::Duration,
        };
        for (root_delay, close_delay, fail_close, expected) in [
            (0, 0, true, TablePreviewErrorCode::ReadFailed),
            (0, 30, false, TablePreviewErrorCode::Timeout),
            (21, 30, false, TablePreviewErrorCode::Timeout),
        ] {
            let count = Arc::new(AtomicUsize::new(0));
            let dropped = Arc::new(AtomicUsize::new(0));
            let connector = FakeConnector {
                connect_delay: Duration::ZERO,
                root_delay: Duration::from_secs(root_delay),
                fail_read: false,
                closes: count.clone(),
                close_delay: Duration::from_secs(close_delay),
                fail_close,
                close_dropped: dropped.clone(),
            };
            assert_eq!(
                read_remote_table(&connector, "/project", "table.csv", 256 * 1024)
                    .await
                    .unwrap_err(),
                expected
            );
            tokio::task::yield_now().await;
            tokio::time::advance(Duration::from_secs(2)).await;
            tokio::task::yield_now().await;
            assert_eq!(count.load(Ordering::SeqCst), 1);
            assert_eq!(dropped.load(Ordering::SeqCst), 1);
        }
    }
    #[test]
    fn supplied_remote_root_ancestors_are_checked_without_shell_interpolation() {
        assert_eq!(
            remote_root_ancestors("/parent/project").unwrap(),
            vec!["/parent", "/parent/project"]
        );
        for invalid in [
            "relative",
            "/parent/../project",
            "/parent/./project",
            "/parent\nproject",
            "//parent/project",
        ] {
            assert!(remote_root_ancestors(invalid).is_err());
        }
        let paths = remote_root_ancestors("/a'b/project").unwrap();
        assert_eq!(
            omicsops_core::project::shell_quote(&paths[0]),
            "'/a'\"'\"'b'"
        );
    }
    struct FakeRootInspector {
        blocked: String,
        inspected: Mutex<Vec<String>>,
        canonicalized: std::sync::atomic::AtomicBool,
    }
    #[async_trait]
    impl RemoteRootInspector for FakeRootInspector {
        async fn reject_symlink(&self, path: &str) -> Result<(), TablePreviewErrorCode> {
            self.inspected.lock().unwrap().push(path.into());
            if path == self.blocked {
                Err(TablePreviewErrorCode::InvalidPath)
            } else {
                Ok(())
            }
        }
        async fn canonicalize(&self, _root: &str) -> Result<String, TablePreviewErrorCode> {
            self.canonicalized
                .store(true, std::sync::atomic::Ordering::SeqCst);
            Ok("/canonical/project".into())
        }
    }
    #[tokio::test]
    async fn root_symlink_ancestors_are_rejected_before_canonicalization() {
        for blocked in ["/parent", "/parent/project"] {
            let inspector = FakeRootInspector {
                blocked: blocked.into(),
                inspected: Mutex::new(vec![]),
                canonicalized: std::sync::atomic::AtomicBool::new(false),
            };
            assert_eq!(
                validate_remote_table_root(&inspector, "/parent/project")
                    .await
                    .unwrap_err(),
                TablePreviewErrorCode::InvalidPath
            );
            assert!(
                !inspector
                    .canonicalized
                    .load(std::sync::atomic::Ordering::SeqCst)
            );
            assert_eq!(inspector.inspected.lock().unwrap().last().unwrap(), blocked);
        }
    }
}
