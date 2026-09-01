//! Clean-room real-browser bridge for OmicsOps.
//!
//! The bridge only accepts a fixed Manifest V3 extension over loopback. It
//! persists no page bodies, cookies, credentials, or screenshot bytes.

use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Component, Path, PathBuf},
    process::Stdio,
    sync::Arc,
    time::Duration,
};

use base64::Engine;
use futures_util::{SinkExt, StreamExt};
use omicsops_process::background_command;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use thiserror::Error;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    process::Child,
    sync::{Mutex, mpsc, oneshot},
};
use tokio_tungstenite::{
    WebSocketStream, accept_hdr_async,
    tungstenite::{
        Message,
        handshake::server::{ErrorResponse, Request, Response},
    },
};
use uuid::Uuid;

pub const PROTOCOL_VERSION: u16 = 1;
pub const SHARED_PORT: u16 = 18_775;
pub const WORKSPACE_PORT: u16 = 18_776;
/// Updated together with the packaged manifest `key`; never accept a wildcard
/// extension Origin at this privilege boundary.
pub const EXTENSION_ID: &str = "joifljknpalpoppceknociillogolbnb";

const REQUIRED_CAPABILITIES: &[&str] = &[
    "tabs",
    "scan",
    "search",
    "screenshot",
    "downloads",
    "debugger",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BrowserSessionKind {
    Shared,
    Workspace,
}

impl BrowserSessionKind {
    pub fn port(self) -> u16 {
        match self {
            Self::Shared => SHARED_PORT,
            Self::Workspace => WORKSPACE_PORT,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Shared => "shared",
            Self::Workspace => "workspace",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowserConfig {
    #[serde(default = "default_true")]
    pub auto_launch: bool,
    #[serde(default = "default_true")]
    pub auto_close_turn_tabs: bool,
    pub browser_path: Option<PathBuf>,
    #[serde(default = "default_search_provider")]
    pub default_search_provider: String,
    #[serde(default)]
    pub disabled_domains: BTreeSet<String>,
    #[serde(default)]
    pub preferred_domains: BTreeSet<String>,
}

impl Default for BrowserConfig {
    fn default() -> Self {
        Self {
            auto_launch: true,
            auto_close_turn_tabs: true,
            browser_path: None,
            default_search_provider: default_search_provider(),
            disabled_domains: BTreeSet::new(),
            preferred_domains: BTreeSet::new(),
        }
    }
}

fn default_true() -> bool {
    true
}

fn default_search_provider() -> String {
    "default".into()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowserStatus {
    pub session: BrowserSessionKind,
    pub port: u16,
    pub listening: bool,
    pub connected: bool,
    pub protocol_version: u16,
    pub extension_id: String,
    pub capabilities: BTreeSet<String>,
    pub tab_summaries: Vec<BrowserTabSummary>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowserTabSummary {
    pub tab_id: u64,
    pub run_id: Option<Uuid>,
    pub title: String,
    /// Sanitized origin only; paths, queries, fragments, and credentials are
    /// never surfaced through settings or persisted events.
    pub origin: String,
    pub created_by_run: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BrowserReply {
    pub request_id: String,
    pub ok: bool,
    #[serde(default)]
    pub data: Value,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedBrowserAsset {
    pub relative_path: String,
    pub size_bytes: u64,
    pub sha256: String,
}

#[derive(Debug, Error)]
pub enum BrowserError {
    #[error("browser {0} session is not connected")]
    NotConnected(&'static str),
    #[error("browser bridge port {port} is unavailable: {message}")]
    PortUnavailable { port: u16, message: String },
    #[error("browser extension handshake was rejected: {0}")]
    Handshake(String),
    #[error("browser bridge request timed out")]
    Timeout,
    #[error("browser bridge transport failed: {0}")]
    Transport(String),
    #[error("browser command failed: {0}")]
    Command(String),
    #[error("no supported browser executable was found")]
    BrowserNotFound,
    #[error("unsafe browser asset path: {0}")]
    UnsafePath(String),
    #[error("browser asset destination already exists: {0}")]
    DestinationExists(String),
    #[error("browser asset I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("browser payload is invalid: {0}")]
    InvalidPayload(String),
}

#[derive(Debug, Deserialize)]
struct BridgeHello {
    #[serde(rename = "type")]
    kind: String,
    protocol_version: u16,
    extension_id: String,
    session: BrowserSessionKind,
    capabilities: BTreeSet<String>,
    #[serde(default)]
    tab_summaries: Vec<BrowserTabSummary>,
}

#[derive(Debug, Serialize)]
struct BrowserCommand<'a> {
    #[serde(rename = "type")]
    kind: &'static str,
    request_id: &'a str,
    protocol_version: u16,
    run_id: Uuid,
    method: &'a str,
    payload: Value,
}

#[derive(Clone)]
pub struct BrowserRuntime {
    inner: Arc<RuntimeInner>,
}

struct RuntimeInner {
    data_root: PathBuf,
    extension_root: PathBuf,
    config: Mutex<BrowserConfig>,
    sessions: BTreeMap<BrowserSessionKind, Arc<SessionState>>,
}

struct SessionState {
    listening: Mutex<bool>,
    connection: Mutex<Option<Connection>>,
    /// Last validated tab ledger retained across transport disconnects so the
    /// host can still require cleanup confirmation for a terminal run.
    last_tab_summaries: Mutex<Vec<BrowserTabSummary>>,
    pending: Mutex<BTreeMap<String, oneshot::Sender<BrowserReply>>>,
    child: Mutex<Option<Child>>,
}

#[derive(Clone)]
struct Connection {
    id: Uuid,
    sender: mpsc::Sender<Message>,
    capabilities: BTreeSet<String>,
    tab_summaries: Vec<BrowserTabSummary>,
}

impl BrowserRuntime {
    pub fn new(data_root: impl Into<PathBuf>, extension_root: impl Into<PathBuf>) -> Self {
        let make = || {
            Arc::new(SessionState {
                listening: Mutex::new(false),
                connection: Mutex::new(None),
                last_tab_summaries: Mutex::new(Vec::new()),
                pending: Mutex::new(BTreeMap::new()),
                child: Mutex::new(None),
            })
        };
        Self {
            inner: Arc::new(RuntimeInner {
                data_root: data_root.into(),
                extension_root: extension_root.into(),
                config: Mutex::new(BrowserConfig::default()),
                sessions: BTreeMap::from([
                    (BrowserSessionKind::Shared, make()),
                    (BrowserSessionKind::Workspace, make()),
                ]),
            }),
        }
    }

    pub async fn config(&self) -> BrowserConfig {
        self.inner.config.lock().await.clone()
    }

    pub async fn set_config(&self, mut config: BrowserConfig) -> Result<(), BrowserError> {
        config.disabled_domains = config
            .disabled_domains
            .into_iter()
            .map(|domain| domain.trim_end_matches('.').to_ascii_lowercase())
            .collect();
        config.preferred_domains = config
            .preferred_domains
            .into_iter()
            .map(|domain| domain.trim_end_matches('.').to_ascii_lowercase())
            .collect();
        validate_config(&config)?;
        *self.inner.config.lock().await = config;
        Ok(())
    }

    pub async fn ensure_listener(&self, session: BrowserSessionKind) -> Result<(), BrowserError> {
        let state = self.session(session);
        let mut listening = state.listening.lock().await;
        if *listening {
            return Ok(());
        }
        let port = session.port();
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port))
            .await
            .map_err(|error| BrowserError::PortUnavailable {
                port,
                message: error.to_string(),
            })?;
        *listening = true;
        drop(listening);
        let runtime = self.clone();
        tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                let runtime = runtime.clone();
                tokio::spawn(async move {
                    let _ = runtime.accept_connection(session, stream).await;
                });
            }
            *runtime.session(session).listening.lock().await = false;
        });
        Ok(())
    }

    pub async fn setup(
        &self,
        session: BrowserSessionKind,
        launch_if_needed: bool,
    ) -> Result<BrowserStatus, BrowserError> {
        self.ensure_listener(session).await?;
        if self.is_connected(session).await {
            return Ok(self.status(session).await);
        }
        let config = self.config().await;
        if launch_if_needed && config.auto_launch {
            self.launch(session).await?;
            for _ in 0..40 {
                if self.is_connected(session).await {
                    return Ok(self.status(session).await);
                }
                tokio::time::sleep(Duration::from_millis(250)).await;
            }
            // A process launched by OmicsOps without the verified extension
            // is not a useful session. Stop only the child handle we spawned;
            // this does not enumerate or terminate unrelated browser windows.
            self.stop_process(session).await;
        }
        Err(BrowserError::NotConnected(session.as_str()))
    }

    pub async fn status(&self, session: BrowserSessionKind) -> BrowserStatus {
        let state = self.session(session);
        let listening = *state.listening.lock().await;
        let connection = state.connection.lock().await.clone();
        let tab_summaries = match connection.as_ref() {
            Some(value) => value.tab_summaries.clone(),
            None => state.last_tab_summaries.lock().await.clone(),
        };
        BrowserStatus {
            session,
            port: session.port(),
            listening,
            connected: connection.is_some(),
            protocol_version: PROTOCOL_VERSION,
            extension_id: EXTENSION_ID.into(),
            capabilities: connection
                .as_ref()
                .map_or_else(BTreeSet::new, |value| value.capabilities.clone()),
            tab_summaries,
        }
    }

    pub async fn call(
        &self,
        session: BrowserSessionKind,
        run_id: Uuid,
        method: &str,
        mut payload: Value,
    ) -> Result<BrowserReply, BrowserError> {
        apply_config_policy(method, &mut payload, &self.config().await)?;
        validate_method(method, &payload)?;
        let state = self.session(session);
        let sender = state
            .connection
            .lock()
            .await
            .as_ref()
            .map(|connection| connection.sender.clone())
            .ok_or(BrowserError::NotConnected(session.as_str()))?;
        let request_id = Uuid::new_v4().to_string();
        let command = BrowserCommand {
            kind: "command",
            request_id: &request_id,
            protocol_version: PROTOCOL_VERSION,
            run_id,
            method,
            payload,
        };
        let serialized = serde_json::to_string(&command)
            .map_err(|error| BrowserError::InvalidPayload(error.to_string()))?;
        let (tx, rx) = oneshot::channel();
        state.pending.lock().await.insert(request_id.clone(), tx);
        if sender.send(Message::Text(serialized.into())).await.is_err() {
            state.pending.lock().await.remove(&request_id);
            return Err(BrowserError::Transport("browser connection closed".into()));
        }
        let reply = match tokio::time::timeout(Duration::from_secs(30), rx).await {
            Ok(Ok(reply)) => reply,
            Ok(Err(_)) => {
                state.pending.lock().await.remove(&request_id);
                return Err(BrowserError::Transport(
                    "browser reply channel closed".into(),
                ));
            }
            Err(_) => {
                state.pending.lock().await.remove(&request_id);
                return Err(BrowserError::Timeout);
            }
        };
        if reply.ok {
            Ok(reply)
        } else {
            Err(BrowserError::Command(
                reply
                    .error
                    .unwrap_or_else(|| "unknown browser error".into()),
            ))
        }
    }

    pub async fn save_screenshot(
        &self,
        project_root: &Path,
        relative_path: &str,
        data_url: &str,
    ) -> Result<SavedBrowserAsset, BrowserError> {
        let encoded = data_url
            .strip_prefix("data:image/png;base64,")
            .ok_or_else(|| {
                BrowserError::InvalidPayload("screenshot must be a PNG data URL".into())
            })?;
        if encoded.len() > 32 * 1024 * 1024 {
            return Err(BrowserError::InvalidPayload(
                "screenshot exceeds the 24 MiB decoded limit".into(),
            ));
        }
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .map_err(|error| BrowserError::InvalidPayload(error.to_string()))?;
        write_project_asset(project_root, relative_path, &bytes).await
    }

    pub async fn save_staged_asset(
        &self,
        project_root: &Path,
        staged_id: &str,
        relative_path: &str,
    ) -> Result<SavedBrowserAsset, BrowserError> {
        validate_staged_id(staged_id)?;
        let staging_root = downloads_directory().join("OmicsOps-Staging");
        let staging_metadata = tokio::fs::symlink_metadata(&staging_root).await?;
        if !staging_metadata.is_dir() || staging_metadata.file_type().is_symlink() {
            return Err(BrowserError::UnsafePath(
                staging_root.to_string_lossy().into_owned(),
            ));
        }
        let canonical_staging_root = tokio::fs::canonicalize(&staging_root).await?;
        let source = canonical_staging_root.join(staged_id);
        let metadata = tokio::fs::symlink_metadata(&source).await?;
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            return Err(BrowserError::UnsafePath(staged_id.into()));
        }
        let canonical_source = tokio::fs::canonicalize(&source).await?;
        if canonical_source.parent() != Some(canonical_staging_root.as_path()) {
            return Err(BrowserError::UnsafePath(staged_id.into()));
        }
        if metadata.len() > 512 * 1024 * 1024 {
            return Err(BrowserError::InvalidPayload(
                "staged browser asset exceeds 512 MiB".into(),
            ));
        }
        copy_project_asset(
            project_root,
            relative_path,
            &canonical_source,
            metadata.len(),
        )
        .await
    }

    pub async fn close_run_tabs(
        &self,
        session: BrowserSessionKind,
        run_id: Uuid,
    ) -> Result<(), BrowserError> {
        if !self.config().await.auto_close_turn_tabs {
            return Ok(());
        }
        let targets = self.run_tab_summaries(session, run_id).await;
        self.close_run_tab_targets(session, run_id, targets).await
    }

    pub async fn force_close_run_tabs(
        &self,
        session: BrowserSessionKind,
        run_id: Uuid,
    ) -> Result<(), BrowserError> {
        let targets = self.run_tab_summaries(session, run_id).await;
        self.close_run_tab_targets(session, run_id, targets).await
    }

    /// Close a persistently recorded run ledger after an app or MV3 worker
    /// restart. The extension rechecks each current tab's origin before it can
    /// close it; a missing tab is accepted as already closed, while an ID whose
    /// origin changed fails closed.
    pub async fn force_close_run_tab_targets(
        &self,
        session: BrowserSessionKind,
        run_id: Uuid,
        targets: Vec<BrowserTabSummary>,
    ) -> Result<(), BrowserError> {
        self.close_run_tab_targets(session, run_id, targets).await
    }

    pub async fn run_tab_summaries(
        &self,
        session: BrowserSessionKind,
        run_id: Uuid,
    ) -> Vec<BrowserTabSummary> {
        self.status(session)
            .await
            .tab_summaries
            .into_iter()
            .filter(|tab| tab.created_by_run && tab.run_id == Some(run_id))
            .collect()
    }

    async fn is_connected(&self, session: BrowserSessionKind) -> bool {
        self.session(session).connection.lock().await.is_some()
    }

    async fn close_run_tab_targets(
        &self,
        session: BrowserSessionKind,
        run_id: Uuid,
        targets: Vec<BrowserTabSummary>,
    ) -> Result<(), BrowserError> {
        if targets.is_empty() {
            return Ok(());
        }
        if targets
            .iter()
            .any(|tab| !tab.created_by_run || tab.run_id != Some(run_id))
        {
            return Err(BrowserError::InvalidPayload(
                "browser cleanup target is not owned by this run".into(),
            ));
        }
        if !self.is_connected(session).await {
            return Err(BrowserError::NotConnected(session.as_str()));
        }
        let expected = targets
            .iter()
            .map(|tab| tab.tab_id)
            .collect::<BTreeSet<_>>();
        let payload = json!({
            "tab_targets": targets.iter().map(|tab| json!({
                "tab_id": tab.tab_id,
                "origin": tab.origin,
            })).collect::<Vec<_>>(),
        });
        let reply = self
            .call(session, run_id, "close_run_tabs", payload)
            .await?;
        let confirmed = ["closed_tab_ids", "already_closed_tab_ids"]
            .into_iter()
            .flat_map(|key| {
                reply
                    .data
                    .get(key)
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_u64)
            })
            .collect::<BTreeSet<_>>();
        if confirmed != expected
            || reply
                .data
                .get("failures")
                .and_then(Value::as_array)
                .is_some_and(|failures| !failures.is_empty())
        {
            return Err(BrowserError::Command(
                "browser could not confirm every recorded run tab was closed".into(),
            ));
        }
        let state = self.session(session);
        state
            .last_tab_summaries
            .lock()
            .await
            .retain(|tab| !expected.contains(&tab.tab_id) || tab.run_id != Some(run_id));
        if let Some(connection) = state.connection.lock().await.as_mut() {
            connection
                .tab_summaries
                .retain(|tab| !expected.contains(&tab.tab_id) || tab.run_id != Some(run_id));
        }
        Ok(())
    }

    async fn accept_connection(
        &self,
        expected_session: BrowserSessionKind,
        stream: TcpStream,
    ) -> Result<(), BrowserError> {
        let expected_path = format!("/v1/session/{}", expected_session.as_str());
        let socket = accept_hdr_async(stream, move |request: &Request, response: Response| {
            let origin = request
                .headers()
                .get("origin")
                .and_then(|value| value.to_str().ok());
            let expected_origin = format!("chrome-extension://{EXTENSION_ID}");
            if request.uri().path() != expected_path || origin != Some(expected_origin.as_str()) {
                return Err(http_error("forbidden browser extension origin or path"));
            }
            Ok(response)
        })
        .await
        .map_err(|error| BrowserError::Handshake(error.to_string()))?;
        self.run_connection(expected_session, socket).await
    }

    async fn run_connection(
        &self,
        expected_session: BrowserSessionKind,
        socket: WebSocketStream<TcpStream>,
    ) -> Result<(), BrowserError> {
        let (mut sink, mut source) = socket.split();
        let hello_message = tokio::time::timeout(Duration::from_secs(5), source.next())
            .await
            .map_err(|_| BrowserError::Handshake("hello timed out".into()))?
            .ok_or_else(|| BrowserError::Handshake("connection closed before hello".into()))?
            .map_err(|error| BrowserError::Handshake(error.to_string()))?;
        let hello: BridgeHello = serde_json::from_str(
            hello_message
                .to_text()
                .map_err(|error| BrowserError::Handshake(error.to_string()))?,
        )
        .map_err(|error| BrowserError::Handshake(error.to_string()))?;
        validate_hello(expected_session, &hello)?;
        let connection_id = Uuid::new_v4();
        let (tx, mut rx) = mpsc::channel::<Message>(64);
        let state = self.session(expected_session);
        let tab_summaries = merge_tab_summaries(
            state.last_tab_summaries.lock().await.clone(),
            sanitize_tab_summaries(hello.tab_summaries)?,
        )?;
        *state.last_tab_summaries.lock().await = tab_summaries.clone();
        *state.connection.lock().await = Some(Connection {
            id: connection_id,
            sender: tx,
            capabilities: hello.capabilities,
            tab_summaries,
        });
        sink.send(Message::Text(
            json!({
                "type":"hello_ack",
                "protocol_version":PROTOCOL_VERSION,
                "session":expected_session,
            })
            .to_string()
            .into(),
        ))
        .await
        .map_err(|error| BrowserError::Transport(error.to_string()))?;
        loop {
            tokio::select! {
                outgoing = rx.recv() => match outgoing {
                    Some(message) => sink.send(message).await.map_err(|error| BrowserError::Transport(error.to_string()))?,
                    None => break,
                },
                incoming = source.next() => match incoming {
                    Some(Ok(Message::Text(text))) => {
                        let value: Value = serde_json::from_str(&text)
                            .map_err(|error| BrowserError::InvalidPayload(error.to_string()))?;
                        if value.get("type").and_then(Value::as_str) == Some("tab_state") {
                            let summaries = serde_json::from_value(
                                value.get("tab_summaries").cloned().unwrap_or_else(|| json!([])),
                            )
                            .map_err(|error| BrowserError::InvalidPayload(error.to_string()))?;
                            let summaries = merge_tab_summaries(
                                state.last_tab_summaries.lock().await.clone(),
                                sanitize_tab_summaries(summaries)?,
                            )?;
                            let is_authoritative = {
                                let mut connection = state.connection.lock().await;
                                if let Some(connection) = connection
                                    .as_mut()
                                    .filter(|connection| connection.id == connection_id)
                                {
                                    connection.tab_summaries = summaries.clone();
                                    true
                                } else {
                                    false
                                }
                            };
                            if is_authoritative {
                                *state.last_tab_summaries.lock().await = summaries;
                            }
                        } else {
                            let reply: BrowserReply = serde_json::from_value(value)
                                .map_err(|error| BrowserError::InvalidPayload(error.to_string()))?;
                            if let Some(waiter) = state.pending.lock().await.remove(&reply.request_id) {
                                let _ = waiter.send(reply);
                            }
                        }
                    }
                    Some(Ok(Message::Ping(value))) => sink.send(Message::Pong(value)).await
                        .map_err(|error| BrowserError::Transport(error.to_string()))?,
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Ok(_)) => {}
                    Some(Err(error)) => return Err(BrowserError::Transport(error.to_string())),
                }
            }
        }
        let mut connection = state.connection.lock().await;
        if connection
            .as_ref()
            .is_some_and(|value| value.id == connection_id)
        {
            *connection = None;
            // Only the currently authoritative connection owns the pending
            // requests. A superseded socket must not cancel requests sent to
            // its replacement when it eventually closes.
            state.pending.lock().await.clear();
        }
        Ok(())
    }

    async fn launch(&self, session: BrowserSessionKind) -> Result<(), BrowserError> {
        let state = self.session(session);
        if state
            .child
            .lock()
            .await
            .as_mut()
            .is_some_and(|child| child.try_wait().ok().flatten().is_none())
        {
            return Ok(());
        }
        let config = self.config().await;
        let executable = discover_browser(config.browser_path.as_deref())?;
        let mut command = background_command(executable);
        command
            .arg("--no-first-run")
            .arg("--no-default-browser-check")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        if session == BrowserSessionKind::Workspace {
            if !self.inner.extension_root.join("manifest.json").is_file() {
                return Err(BrowserError::Handshake(
                    "packaged OmicsOps extension is missing".into(),
                ));
            }
            let profile = self
                .inner
                .data_root
                .join("browser")
                .join("workspace-profile");
            tokio::fs::create_dir_all(&profile).await?;
            command.arg(format!("--user-data-dir={}", profile.display()));
            command.arg(format!(
                "--load-extension={}",
                self.inner.extension_root.display()
            ));
        }
        command.arg(format!(
            "chrome-extension://{EXTENSION_ID}/session.html?session={}",
            session.as_str()
        ));
        let child = command.spawn()?;
        *state.child.lock().await = Some(child);
        Ok(())
    }

    async fn stop_process(&self, session: BrowserSessionKind) {
        if let Some(mut child) = self.session(session).child.lock().await.take() {
            let _ = child.kill().await;
        }
    }

    fn session(&self, session: BrowserSessionKind) -> Arc<SessionState> {
        self.inner.sessions[&session].clone()
    }
}

fn http_error(message: &str) -> ErrorResponse {
    Response::builder()
        .status(403)
        .body(Some(message.into()))
        .expect("static HTTP error response")
}

fn validate_hello(
    expected_session: BrowserSessionKind,
    hello: &BridgeHello,
) -> Result<(), BrowserError> {
    if hello.kind != "hello"
        || hello.protocol_version != PROTOCOL_VERSION
        || hello.extension_id != EXTENSION_ID
        || hello.session != expected_session
    {
        return Err(BrowserError::Handshake(
            "extension ID, session, or protocol version mismatch".into(),
        ));
    }
    for capability in REQUIRED_CAPABILITIES {
        if !hello.capabilities.contains(*capability) {
            return Err(BrowserError::Handshake(format!(
                "required capability {capability} is missing"
            )));
        }
    }
    Ok(())
}

fn sanitize_tab_summaries(
    summaries: Vec<BrowserTabSummary>,
) -> Result<Vec<BrowserTabSummary>, BrowserError> {
    if summaries.len() > 100 {
        return Err(BrowserError::InvalidPayload(
            "browser tab summary limit exceeded".into(),
        ));
    }
    summaries
        .into_iter()
        .map(|mut summary| {
            if summary.tab_id == 0 || summary.title.len() > 200 {
                return Err(BrowserError::InvalidPayload(
                    "invalid browser tab summary".into(),
                ));
            }
            let parsed = url::Url::parse(&summary.origin)
                .map_err(|error| BrowserError::InvalidPayload(error.to_string()))?;
            if !matches!(parsed.scheme(), "http" | "https")
                || !parsed.username().is_empty()
                || parsed.password().is_some()
            {
                return Err(BrowserError::InvalidPayload(
                    "tab summary origin must be credential-free HTTP(S)".into(),
                ));
            }
            let host = parsed
                .host_str()
                .ok_or_else(|| BrowserError::InvalidPayload("tab origin has no host".into()))?;
            summary.origin = match parsed.port() {
                Some(port) => format!("{}://{host}:{port}", parsed.scheme()),
                None => format!("{}://{host}", parsed.scheme()),
            };
            Ok(summary)
        })
        .collect()
}

fn merge_tab_summaries(
    retained: Vec<BrowserTabSummary>,
    current: Vec<BrowserTabSummary>,
) -> Result<Vec<BrowserTabSummary>, BrowserError> {
    let mut merged = retained
        .into_iter()
        .map(|summary| ((summary.run_id, summary.tab_id), summary))
        .collect::<BTreeMap<_, _>>();
    for summary in current {
        merged.insert((summary.run_id, summary.tab_id), summary);
    }
    if merged.len() > 100 {
        return Err(BrowserError::InvalidPayload(
            "browser tab summary limit exceeded".into(),
        ));
    }
    Ok(merged.into_values().collect())
}

fn validate_config(config: &BrowserConfig) -> Result<(), BrowserError> {
    if !matches!(
        config.default_search_provider.as_str(),
        "default" | "google" | "bing" | "duckduckgo"
    ) {
        return Err(BrowserError::InvalidPayload(
            "unsupported search provider".into(),
        ));
    }
    for domain in config
        .disabled_domains
        .iter()
        .chain(config.preferred_domains.iter())
    {
        if domain.is_empty()
            || domain.len() > 253
            || !domain
                .chars()
                .all(|value| value.is_ascii_alphanumeric() || matches!(value, '.' | '-'))
        {
            return Err(BrowserError::InvalidPayload(format!(
                "invalid browser policy domain: {domain}"
            )));
        }
    }
    Ok(())
}

fn apply_config_policy(
    method: &str,
    payload: &mut Value,
    config: &BrowserConfig,
) -> Result<(), BrowserError> {
    let object = payload.as_object_mut().ok_or_else(|| {
        BrowserError::InvalidPayload("browser command payload must be an object".into())
    })?;
    if method == "web_search" {
        let requested = object
            .get("provider")
            .and_then(Value::as_str)
            .unwrap_or("default");
        if requested == "default" && config.default_search_provider != "default" {
            object.insert(
                "provider".into(),
                Value::String(config.default_search_provider.clone()),
            );
        }
        let provider = object
            .get("provider")
            .and_then(Value::as_str)
            .unwrap_or("default");
        let target_host = match provider {
            "bing" => "www.bing.com",
            "duckduckgo" => "duckduckgo.com",
            "default" | "google" => "www.google.com",
            _ => {
                return Err(BrowserError::InvalidPayload(
                    "unsupported search provider".into(),
                ));
            }
        };
        object.insert("target_host".into(), Value::String(target_host.into()));
    }
    if matches!(
        method,
        "web_search" | "web_scan" | "web_open_tab" | "web_save_assets"
    ) {
        object.insert(
            "disabled_domains".into(),
            serde_json::to_value(&config.disabled_domains)
                .map_err(|error| BrowserError::InvalidPayload(error.to_string()))?,
        );
        object.insert(
            "preferred_domains".into(),
            serde_json::to_value(&config.preferred_domains)
                .map_err(|error| BrowserError::InvalidPayload(error.to_string()))?,
        );
    }
    if method == "web_open_tab" {
        let url = object
            .get("url")
            .and_then(Value::as_str)
            .ok_or_else(|| BrowserError::InvalidPayload("url is required".into()))?;
        let parsed = parse_browser_http_url(url)?;
        let host = parsed
            .host_str()
            .ok_or_else(|| BrowserError::InvalidPayload("URL host is required".into()))?
            .trim_end_matches('.')
            .to_ascii_lowercase();
        object.insert("target_host".into(), Value::String(host.clone()));
        if config
            .disabled_domains
            .iter()
            .any(|domain| host == domain.as_str() || host.ends_with(&format!(".{domain}")))
        {
            return Err(BrowserError::InvalidPayload(format!(
                "browser policy disables {host}"
            )));
        }
    }
    if method == "web_save_assets" {
        let target_host = normalized_browser_host(
            object
                .get("target_host")
                .and_then(Value::as_str)
                .ok_or_else(|| BrowserError::InvalidPayload("target_host is required".into()))?,
        )?;
        let assets = object
            .get("assets")
            .and_then(Value::as_array)
            .filter(|assets| !assets.is_empty() && assets.len() <= 100)
            .ok_or_else(|| {
                BrowserError::InvalidPayload("assets must contain between 1 and 100 items".into())
            })?;
        for asset in assets {
            let source = asset
                .get("source_url")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    BrowserError::InvalidPayload("asset source_url is required".into())
                })?;
            let source_host = parse_browser_http_url(source)?
                .host_str()
                .ok_or_else(|| BrowserError::InvalidPayload("asset URL has no host".into()))?
                .trim_end_matches('.')
                .to_ascii_lowercase();
            if source_host != target_host {
                return Err(BrowserError::InvalidPayload(
                    "asset source_url does not match target_host".into(),
                ));
            }
            if config.disabled_domains.iter().any(|domain| {
                source_host == domain.as_str() || source_host.ends_with(&format!(".{domain}"))
            }) {
                return Err(BrowserError::InvalidPayload(format!(
                    "browser policy disables {source_host}"
                )));
            }
        }
    }
    Ok(())
}

fn validate_method(method: &str, payload: &Value) -> Result<(), BrowserError> {
    let allowed = [
        "web_search",
        "web_open_tab",
        "web_scan",
        "web_execute_js",
        "web_screenshot",
        "web_save_assets",
        "close_run_tabs",
    ];
    if !allowed.contains(&method) || !payload.is_object() {
        return Err(BrowserError::InvalidPayload(
            "unknown browser method or non-object payload".into(),
        ));
    }
    if method == "web_open_tab" {
        let url = payload
            .get("url")
            .and_then(Value::as_str)
            .ok_or_else(|| BrowserError::InvalidPayload("url is required".into()))?;
        parse_browser_http_url(url)?;
    }
    if matches!(
        method,
        "web_scan" | "web_execute_js" | "web_screenshot" | "web_save_assets"
    ) {
        normalized_browser_host(
            payload
                .get("target_host")
                .and_then(Value::as_str)
                .ok_or_else(|| BrowserError::InvalidPayload("target_host is required".into()))?,
        )?;
    }
    if method == "web_save_assets" {
        let target_host = normalized_browser_host(
            payload
                .get("target_host")
                .and_then(Value::as_str)
                .ok_or_else(|| BrowserError::InvalidPayload("target_host is required".into()))?,
        )?;
        let assets = payload
            .get("assets")
            .and_then(Value::as_array)
            .filter(|assets| !assets.is_empty() && assets.len() <= 100)
            .ok_or_else(|| {
                BrowserError::InvalidPayload("assets must contain between 1 and 100 items".into())
            })?;
        for asset in assets {
            safe_relative(
                asset
                    .get("relative_path")
                    .and_then(Value::as_str)
                    .ok_or_else(|| {
                        BrowserError::InvalidPayload("asset relative_path is required".into())
                    })?,
            )?;
            let source_host = parse_browser_http_url(
                asset
                    .get("source_url")
                    .and_then(Value::as_str)
                    .ok_or_else(|| {
                        BrowserError::InvalidPayload("asset source_url is required".into())
                    })?,
            )?
            .host_str()
            .ok_or_else(|| BrowserError::InvalidPayload("asset URL has no host".into()))?
            .trim_end_matches('.')
            .to_ascii_lowercase();
            if source_host != target_host {
                return Err(BrowserError::InvalidPayload(
                    "asset source_url does not match target_host".into(),
                ));
            }
        }
    }
    if method == "web_execute_js"
        && payload
            .get("script")
            .and_then(Value::as_str)
            .is_some_and(|script| {
                script.len() > 16_000
                    || ["chatgpt.com", "gemini.google.com", "claude.ai"]
                        .iter()
                        .any(|host| script.to_ascii_lowercase().contains(host))
            })
    {
        return Err(BrowserError::InvalidPayload(
            "script exceeds the limit or targets a prohibited web AI".into(),
        ));
    }
    Ok(())
}

fn validate_staged_id(value: &str) -> Result<(), BrowserError> {
    if value.is_empty()
        || value.len() > 128
        || !value.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.')
        })
        || value == "."
        || value == ".."
    {
        return Err(BrowserError::UnsafePath(value.into()));
    }
    Ok(())
}

fn normalized_browser_host(value: &str) -> Result<String, BrowserError> {
    let host = value.trim_end_matches('.').to_ascii_lowercase();
    if value.trim() != value
        || host.is_empty()
        || host.len() > 253
        || host.contains('*')
        || !host
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '.' | '-'))
    {
        return Err(BrowserError::InvalidPayload(
            "target_host must be a concrete host".into(),
        ));
    }
    Ok(host)
}

fn parse_browser_http_url(value: &str) -> Result<url::Url, BrowserError> {
    if value.trim() != value || value.chars().any(char::is_control) {
        return Err(BrowserError::InvalidPayload(
            "browser URL contains whitespace or control characters".into(),
        ));
    }
    let parsed =
        url::Url::parse(value).map_err(|error| BrowserError::InvalidPayload(error.to_string()))?;
    if !matches!(parsed.scheme(), "http" | "https")
        || parsed.host_str().is_none()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
    {
        return Err(BrowserError::InvalidPayload(
            "browser URL must be credential-free HTTP(S)".into(),
        ));
    }
    if parsed
        .query_pairs()
        .any(|(key, _)| sensitive_browser_query_key(&key))
    {
        return Err(BrowserError::InvalidPayload(
            "browser URL contains a sensitive query parameter".into(),
        ));
    }
    Ok(parsed)
}

fn sensitive_browser_query_key(value: &str) -> bool {
    let normalized = value.to_ascii_lowercase().replace('-', "_");
    matches!(normalized.as_str(), "access_token" | "api_key")
        || normalized.split('_').any(|part| {
            matches!(
                part,
                "auth"
                    | "authorization"
                    | "code"
                    | "key"
                    | "password"
                    | "secret"
                    | "signature"
                    | "token"
            )
        })
}

fn safe_relative(value: &str) -> Result<PathBuf, BrowserError> {
    let path = Path::new(value);
    if value.trim().is_empty()
        || path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
        || path.components().any(|component| match component {
            Component::Normal(value) => !safe_project_component(&value.to_string_lossy()),
            _ => true,
        })
    {
        return Err(BrowserError::UnsafePath(value.into()));
    }
    Ok(path.to_path_buf())
}

fn safe_project_component(value: &str) -> bool {
    if value.is_empty()
        || value.trim() != value
        || value.ends_with('.')
        || value
            .chars()
            .any(|character| character.is_control() || r#"<>:"/\|?*"#.contains(character))
    {
        return false;
    }
    let device_stem = value
        .split('.')
        .next()
        .unwrap_or_default()
        .to_ascii_uppercase();
    !matches!(device_stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        && !(device_stem.len() == 4
            && (device_stem.starts_with("COM") || device_stem.starts_with("LPT"))
            && matches!(device_stem.as_bytes()[3], b'1'..=b'9'))
}

async fn write_project_asset(
    project_root: &Path,
    relative_path: &str,
    bytes: &[u8],
) -> Result<SavedBrowserAsset, BrowserError> {
    let (relative, destination) = project_asset_destination(project_root, relative_path).await?;
    let mut output = tokio::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&destination)
        .await
        .map_err(|error| {
            if error.kind() == std::io::ErrorKind::AlreadyExists {
                BrowserError::DestinationExists(relative_path.into())
            } else {
                BrowserError::Io(error)
            }
        })?;
    if let Err(error) = output.write_all(bytes).await {
        drop(output);
        let _ = tokio::fs::remove_file(&destination).await;
        return Err(BrowserError::Io(error));
    }
    if let Err(error) = output.flush().await {
        drop(output);
        let _ = tokio::fs::remove_file(&destination).await;
        return Err(BrowserError::Io(error));
    }
    Ok(SavedBrowserAsset {
        relative_path: relative.to_string_lossy().replace('\\', "/"),
        size_bytes: bytes.len() as u64,
        sha256: hex::encode(Sha256::digest(bytes)),
    })
}

async fn project_asset_destination(
    project_root: &Path,
    relative_path: &str,
) -> Result<(PathBuf, PathBuf), BrowserError> {
    let relative = safe_relative(relative_path)?;
    let canonical_root = tokio::fs::canonicalize(project_root).await?;
    let parent_relative = relative.parent().unwrap_or_else(|| Path::new("."));
    let mut canonical_parent = canonical_root.clone();
    for component in parent_relative.components() {
        let Component::Normal(component) = component else {
            continue;
        };
        let candidate = canonical_parent.join(component);
        match tokio::fs::symlink_metadata(&candidate).await {
            Ok(metadata) => {
                if !metadata.is_dir() || metadata.file_type().is_symlink() {
                    return Err(BrowserError::UnsafePath(relative_path.into()));
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                tokio::fs::create_dir(&candidate).await?;
            }
            Err(error) => return Err(BrowserError::Io(error)),
        }
        canonical_parent = tokio::fs::canonicalize(&candidate).await?;
        if !canonical_parent.starts_with(&canonical_root) {
            return Err(BrowserError::UnsafePath(relative_path.into()));
        }
    }
    let destination = canonical_parent.join(
        relative
            .file_name()
            .ok_or_else(|| BrowserError::UnsafePath(relative_path.into()))?,
    );
    Ok((relative, destination))
}

async fn copy_project_asset(
    project_root: &Path,
    relative_path: &str,
    source: &Path,
    expected_size: u64,
) -> Result<SavedBrowserAsset, BrowserError> {
    let (relative, destination) = project_asset_destination(project_root, relative_path).await?;
    let mut input = tokio::fs::File::open(source).await?;
    let mut output = tokio::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&destination)
        .await
        .map_err(|error| {
            if error.kind() == std::io::ErrorKind::AlreadyExists {
                BrowserError::DestinationExists(relative_path.into())
            } else {
                BrowserError::Io(error)
            }
        })?;
    let mut buffer = vec![0_u8; 64 * 1024];
    let mut hasher = Sha256::new();
    let mut size_bytes = 0_u64;
    loop {
        let read = match input.read(&mut buffer).await {
            Ok(read) => read,
            Err(error) => {
                drop(output);
                let _ = tokio::fs::remove_file(&destination).await;
                return Err(BrowserError::Io(error));
            }
        };
        if read == 0 {
            break;
        }
        size_bytes = size_bytes.saturating_add(read as u64);
        if size_bytes > expected_size || size_bytes > 512 * 1024 * 1024 {
            drop(output);
            let _ = tokio::fs::remove_file(&destination).await;
            return Err(BrowserError::InvalidPayload(
                "staged browser asset changed while it was copied".into(),
            ));
        }
        hasher.update(&buffer[..read]);
        if let Err(error) = output.write_all(&buffer[..read]).await {
            drop(output);
            let _ = tokio::fs::remove_file(&destination).await;
            return Err(BrowserError::Io(error));
        }
    }
    if size_bytes != expected_size {
        drop(output);
        let _ = tokio::fs::remove_file(&destination).await;
        return Err(BrowserError::InvalidPayload(
            "staged browser asset changed while it was copied".into(),
        ));
    }
    if let Err(error) = output.flush().await {
        drop(output);
        let _ = tokio::fs::remove_file(&destination).await;
        return Err(BrowserError::Io(error));
    }
    Ok(SavedBrowserAsset {
        relative_path: relative.to_string_lossy().replace('\\', "/"),
        size_bytes,
        sha256: hex::encode(hasher.finalize()),
    })
}

fn discover_browser(explicit: Option<&Path>) -> Result<PathBuf, BrowserError> {
    if let Some(path) = explicit {
        if path.is_file() {
            return Ok(path.to_path_buf());
        }
        return Err(BrowserError::BrowserNotFound);
    }
    let mut candidates = Vec::new();
    #[cfg(target_os = "windows")]
    {
        for root in ["PROGRAMFILES", "PROGRAMFILES(X86)", "LOCALAPPDATA"] {
            if let Some(root) = std::env::var_os(root) {
                let root = PathBuf::from(root);
                candidates.extend([
                    root.join("Google/Chrome/Application/chrome.exe"),
                    root.join("Microsoft/Edge/Application/msedge.exe"),
                    root.join("Chromium/Application/chrome.exe"),
                    root.join("Google/Chrome for Testing/Application/chrome.exe"),
                ]);
            }
        }
    }
    #[cfg(target_os = "macos")]
    {
        let mut roots = vec![PathBuf::from("/Applications")];
        if let Some(home) = std::env::var_os("HOME") {
            roots.push(PathBuf::from(home).join("Applications"));
        }
        for root in roots {
            candidates.extend([
                root.join("Google Chrome.app/Contents/MacOS/Google Chrome"),
                root.join("Microsoft Edge.app/Contents/MacOS/Microsoft Edge"),
                root.join("Chromium.app/Contents/MacOS/Chromium"),
                root.join("Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing"),
            ]);
        }
    }
    candidates
        .into_iter()
        .find(|candidate| candidate.is_file())
        .ok_or(BrowserError::BrowserNotFound)
}

fn downloads_directory() -> PathBuf {
    #[cfg(target_os = "windows")]
    if let Some(profile) = std::env::var_os("USERPROFILE") {
        return PathBuf::from(profile).join("Downloads");
    }
    #[cfg(not(target_os = "windows"))]
    if let Some(home) = std::env::var_os("HOME") {
        return PathBuf::from(home).join("Downloads");
    }
    PathBuf::from("Downloads")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_non_http_urls_and_web_ai_scripts() {
        assert!(validate_method("web_open_tab", &json!({"url":"file:///secret"})).is_err());
        assert!(
            validate_method(
                "web_execute_js",
                &json!({"target_host":"example.org","script":"fetch('https://chatgpt.com')"})
            )
            .is_err()
        );
        assert!(
            validate_method(
                "web_open_tab",
                &json!({"url":"https://user:secret@example.org/paper"})
            )
            .is_err()
        );
        assert!(
            validate_method(
                "web_open_tab",
                &json!({"url":"https://example.org/paper?session_token=secret"})
            )
            .is_err()
        );
        assert!(
            validate_method(
                "web_save_assets",
                &json!({
                    "target_host":"example.org",
                    "assets":[{"relative_path":"paper.pdf","source_url":"https://other.example/paper.pdf"}]
                })
            )
            .is_err()
        );
    }

    #[test]
    fn config_policy_applies_provider_and_blocks_disabled_subdomains() {
        let config = BrowserConfig {
            default_search_provider: "bing".into(),
            disabled_domains: BTreeSet::from(["example.org".into()]),
            ..BrowserConfig::default()
        };
        let mut search = json!({"query":"single cell"});
        apply_config_policy("web_search", &mut search, &config).unwrap();
        assert_eq!(search["provider"], "bing");
        assert_eq!(search["disabled_domains"], json!(["example.org"]));
        let mut open = json!({"url":"https://papers.example.org/article"});
        assert!(apply_config_policy("web_open_tab", &mut open, &config).is_err());
    }

    #[test]
    fn handshake_requires_exact_protocol_session_id_and_capabilities() {
        let capabilities = REQUIRED_CAPABILITIES
            .iter()
            .map(|value| (*value).to_owned())
            .collect();
        let valid = BridgeHello {
            kind: "hello".into(),
            protocol_version: PROTOCOL_VERSION,
            extension_id: EXTENSION_ID.into(),
            session: BrowserSessionKind::Shared,
            capabilities,
            tab_summaries: vec![],
        };
        assert!(validate_hello(BrowserSessionKind::Shared, &valid).is_ok());
        let mut invalid = valid;
        invalid.capabilities.remove("debugger");
        assert!(validate_hello(BrowserSessionKind::Shared, &invalid).is_err());
    }

    #[tokio::test]
    async fn project_assets_reject_traversal_and_hash_written_bytes() {
        let directory = tempfile::tempdir().unwrap();
        assert!(
            write_project_asset(directory.path(), "../escape", b"bad")
                .await
                .is_err()
        );
        for unsafe_path in [
            "browser/result.txt:stream",
            "browser/CON.txt",
            "browser/trailing. ",
        ] {
            assert!(
                write_project_asset(directory.path(), unsafe_path, b"bad")
                    .await
                    .is_err()
            );
        }
        let saved = write_project_asset(directory.path(), "browser/result.txt", b"evidence")
            .await
            .unwrap();
        assert_eq!(saved.relative_path, "browser/result.txt");
        assert_eq!(saved.size_bytes, 8);
        assert_eq!(saved.sha256, hex::encode(Sha256::digest(b"evidence")));
        assert!(matches!(
            write_project_asset(directory.path(), "browser/result.txt", b"overwrite").await,
            Err(BrowserError::DestinationExists(_))
        ));
    }

    #[tokio::test]
    async fn failed_staged_copy_removes_the_partial_destination() {
        let source_directory = tempfile::tempdir().unwrap();
        let project_directory = tempfile::tempdir().unwrap();
        let source = source_directory.path().join("staged.bin");
        tokio::fs::write(&source, b"larger than expected")
            .await
            .unwrap();

        assert!(
            copy_project_asset(project_directory.path(), "browser/copied.bin", &source, 1,)
                .await
                .is_err()
        );
        assert!(!project_directory.path().join("browser/copied.bin").exists());
    }

    #[tokio::test]
    async fn disconnected_session_retains_run_tabs_and_requires_cleanup_confirmation() {
        let directory = tempfile::tempdir().unwrap();
        let runtime = BrowserRuntime::new(directory.path(), directory.path());
        let run_id = Uuid::new_v4();
        runtime
            .session(BrowserSessionKind::Shared)
            .last_tab_summaries
            .lock()
            .await
            .push(BrowserTabSummary {
                tab_id: 42,
                run_id: Some(run_id),
                title: "Research result".into(),
                origin: "https://example.org".into(),
                created_by_run: true,
            });

        let status = runtime.status(BrowserSessionKind::Shared).await;
        assert!(!status.connected);
        assert_eq!(status.tab_summaries.len(), 1);
        assert!(matches!(
            runtime
                .close_run_tabs(BrowserSessionKind::Shared, run_id)
                .await,
            Err(BrowserError::NotConnected("shared"))
        ));
        assert!(matches!(
            runtime
                .force_close_run_tab_targets(
                    BrowserSessionKind::Shared,
                    run_id,
                    vec![BrowserTabSummary {
                        tab_id: 99,
                        run_id: Some(Uuid::new_v4()),
                        title: "other run".into(),
                        origin: "https://example.org".into(),
                        created_by_run: true,
                    }],
                )
                .await,
            Err(BrowserError::InvalidPayload(_))
        ));
    }

    #[tokio::test]
    async fn occupied_port_fails_closed() {
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, SHARED_PORT))
            .await
            .unwrap();
        let directory = tempfile::tempdir().unwrap();
        let runtime = BrowserRuntime::new(directory.path(), directory.path());
        let error = runtime
            .ensure_listener(BrowserSessionKind::Shared)
            .await
            .unwrap_err();
        assert!(matches!(error, BrowserError::PortUnavailable { .. }));
        drop(listener);
    }
}
