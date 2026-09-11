//! Long-lived stdio MCP client runtime used by the desktop Agent V4 gateway.
//!
//! The runtime deliberately keeps the persisted declaration separate from the
//! live process.  A process is only started after the caller has completed its
//! approval checks, and the process is keyed by `(project_id, server_id)` so a
//! server cannot accidentally share a project-specific working directory or
//! environment with another project.

use std::{
    collections::HashMap,
    process::Stdio,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use omicsops_process::background_command;
use rmcp::{
    ClientHandler, ServiceExt,
    model::{CallToolRequestParams, CallToolResult, ClientInfo, ContentBlock},
    service::{NotificationContext, Peer, RoleClient, RunningService, ServiceError},
    transport::{ConfigureCommandExt, TokioChildProcess},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use thiserror::Error;
use tokio::{
    io::{AsyncRead, AsyncReadExt},
    sync::{Mutex, OwnedMutexGuard, RwLock},
};
use uuid::Uuid;

const DEFAULT_TIMEOUT_SECS: u64 = 60;
const MAX_TIMEOUT_SECS: u64 = 3_600;
const STDERR_TAIL_BYTES: usize = 2 * 1024;

/// A persisted environment binding.  Literal values are supported for
/// non-sensitive configuration; credential references are resolved by the
/// desktop layer immediately before creating [`McpServerConfig`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub struct McpEnvBinding {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credential_reference: Option<String>,
}

impl McpEnvBinding {
    pub fn validate(&self) -> Result<(), McpRuntimeError> {
        if self.name.is_empty()
            || self.name.len() > 256
            || !self.name.bytes().enumerate().all(|(index, byte)| {
                if index == 0 {
                    byte == b'_' || byte.is_ascii_alphabetic()
                } else {
                    byte == b'_' || byte.is_ascii_alphanumeric()
                }
            })
        {
            return Err(McpRuntimeError::InvalidConfig(format!(
                "invalid MCP environment variable name: {}",
                self.name
            )));
        }
        if self.value.is_some() && self.credential_reference.is_some() {
            return Err(McpRuntimeError::InvalidConfig(format!(
                "MCP environment binding {} cannot contain both value and credential reference",
                self.name
            )));
        }
        Ok(())
    }
}

/// The resolved declaration used to launch one stdio server.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpServerConfig {
    pub project_id: Uuid,
    pub server_id: Uuid,
    pub name: String,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    /// Values in this field are already resolved in memory.  Callers must not
    /// persist a resolved secret in the profile or audit record.
    #[serde(default, skip_serializing)]
    pub env: Vec<(String, String)>,
    #[serde(default = "default_timeout_secs")]
    pub timeout_secs: u64,
}

impl McpServerConfig {
    pub fn normalized(mut self) -> Result<Self, McpRuntimeError> {
        self.name = self.name.trim().to_owned();
        self.command = self.command.trim().to_owned();
        if self.name.is_empty()
            || self.command.is_empty()
            || self.name.chars().any(|c| matches!(c, '\0' | '\n' | '\r'))
            || self
                .command
                .chars()
                .any(|c| matches!(c, '\0' | '\n' | '\r'))
            || self.args.iter().any(|arg| arg.contains('\0'))
        {
            return Err(McpRuntimeError::InvalidConfig(
                "invalid MCP stdio declaration".into(),
            ));
        }
        for binding in self.env.iter().map(|(name, value)| McpEnvBinding {
            name: name.clone(),
            value: Some(value.clone()),
            credential_reference: None,
        }) {
            binding.validate()?;
        }
        self.timeout_secs = self.timeout_secs.clamp(1, MAX_TIMEOUT_SECS);
        Ok(self)
    }

    pub fn timeout(&self) -> Duration {
        Duration::from_secs(self.timeout_secs.clamp(1, MAX_TIMEOUT_SECS))
    }

    pub fn fingerprint(&self) -> String {
        let bytes = serde_json::to_vec(&json!({
            "projectId": self.project_id,
            "serverId": self.server_id,
            "name": &self.name,
            "command": &self.command,
            "args": &self.args,
            "cwd": &self.cwd,
            "env": &self.env,
            "timeoutSecs": self.timeout_secs,
        }))
        .expect("MCP config is serializable");
        hex::encode(Sha256::digest(bytes))
    }
}

fn default_timeout_secs() -> u64 {
    DEFAULT_TIMEOUT_SECS
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpSessionKey {
    pub project_id: Uuid,
    pub server_id: Uuid,
}

impl McpSessionKey {
    pub fn new(project_id: Uuid, server_id: Uuid) -> Self {
        Self {
            project_id,
            server_id,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum McpSessionState {
    Disconnected,
    Connecting,
    Ready,
    Stale,
    Failed,
    Stopping,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpToolCatalog {
    pub tools: Vec<Value>,
    pub sha256: String,
    pub generation: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpInspection {
    pub server_name: String,
    pub server_info: Value,
    pub capabilities: Value,
    pub tools: Vec<Value>,
    pub tool_catalog_sha256: String,
    pub generation: u64,
    pub protocol_version: Option<String>,
    pub stderr_tail: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpInvocation {
    pub server_name: String,
    pub result: Value,
    pub tool_catalog_sha256: String,
    pub generation: u64,
    pub stderr_tail: String,
}

#[derive(Debug, Error)]
pub enum McpRuntimeError {
    #[error("invalid MCP configuration: {0}")]
    InvalidConfig(String),
    #[error("MCP server launch failed: {0}")]
    Launch(String),
    #[error("MCP server initialization failed: {0}")]
    Initialize(String),
    #[error("MCP server request timed out after {0} seconds")]
    Timeout(u64),
    #[error("MCP server transport closed: {0}")]
    Transport(String),
    #[error("MCP tool {0} is not advertised by the server")]
    ToolNotFound(String),
    #[error("MCP tool schema changed since approval")]
    SchemaChanged,
    #[error("MCP tool readOnlyHint annotation is missing")]
    ReadOnlyHintMissing,
    #[error("MCP tool readOnlyHint annotation is not true")]
    ReadOnlyHintNotTrue,
    #[error("MCP session is stale; inspect and approve the server again")]
    Stale,
    #[error("MCP call arguments must be a JSON object")]
    InvalidArguments,
    #[error("MCP response could not be serialized: {0}")]
    Serialization(String),
}

#[derive(Clone)]
struct NotificationHandler {
    stale: Arc<AtomicBool>,
}

impl ClientHandler for NotificationHandler {
    fn get_info(&self) -> ClientInfo {
        ClientInfo::default()
    }

    fn on_tool_list_changed(
        &self,
        _context: NotificationContext<RoleClient>,
    ) -> impl std::future::Future<Output = ()> + Send + '_ {
        self.stale.store(true, Ordering::Release);
        std::future::ready(())
    }

    fn on_custom_notification(
        &self,
        _notification: rmcp::model::CustomNotification,
        _context: NotificationContext<RoleClient>,
    ) -> impl std::future::Future<Output = ()> + Send + '_ {
        std::future::ready(())
    }
}

type ClientService = RunningService<RoleClient, NotificationHandler>;

struct McpSession {
    config_fingerprint: String,
    peer: Peer<RoleClient>,
    service: Mutex<ClientService>,
    state: RwLock<McpSessionState>,
    stale: Arc<AtomicBool>,
    catalog: RwLock<McpToolCatalog>,
    stderr_tail: Arc<Mutex<String>>,
}

/// Shared app-lifetime MCP process pool.
#[derive(Clone, Default)]
pub struct McpSessionManager {
    sessions: Arc<Mutex<HashMap<McpSessionKey, Arc<McpSession>>>>,
    server_locks: Arc<Mutex<HashMap<Uuid, Arc<Mutex<()>>>>>,
}

impl McpSessionManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// Serialize profile reads, calls, and invalidation for one server. The
    /// desktop uses this guard around a profile-backed operation so a caller
    /// that read an old enabled profile cannot recreate a session after a
    /// revoke has been durably persisted.
    pub async fn lock_server(&self, server_id: Uuid) -> OwnedMutexGuard<()> {
        let lock = {
            let mut locks = self.server_locks.lock().await;
            locks
                .entry(server_id)
                .or_insert_with(|| Arc::new(Mutex::new(())))
                .clone()
        };
        lock.lock_owned().await
    }

    /// Inspect using an isolated process.  The process is always closed before
    /// this method returns, regardless of whether discovery succeeds.
    pub async fn inspect(&self, config: McpServerConfig) -> Result<McpInspection, McpRuntimeError> {
        let config = config.normalized()?;
        let session = self.connect(&config).await?;
        let result = self.refresh_catalog(&session, &config).await;
        let inspection = match result {
            Ok(catalog) => {
                let server_info = server_info(&session.peer)?;
                let capabilities = server_info
                    .get("capabilities")
                    .cloned()
                    .unwrap_or_else(|| json!({}));
                let protocol_version = server_info
                    .get("protocolVersion")
                    .and_then(Value::as_str)
                    .map(str::to_owned);
                Ok(McpInspection {
                    server_name: config.name.clone(),
                    server_info,
                    capabilities,
                    tools: catalog.tools,
                    tool_catalog_sha256: catalog.sha256,
                    generation: catalog.generation,
                    protocol_version,
                    stderr_tail: session.stderr_tail().await,
                })
            }
            Err(error) => Err(error),
        };
        close_session(&session).await;
        inspection
    }

    /// Return a tool call using a session shared by all calls for this project
    /// and server.  There is intentionally no retry around a tool call.
    pub async fn call(
        &self,
        config: McpServerConfig,
        tool: &str,
        arguments: Value,
        expected_catalog_sha256: Option<&str>,
        expected_schema_sha256: Option<&str>,
    ) -> Result<McpInvocation, McpRuntimeError> {
        self.call_with_policy(
            config,
            tool,
            arguments,
            expected_catalog_sha256,
            expected_schema_sha256,
            false,
        )
        .await
    }

    /// Plan-mode MCP call. In addition to catalog/schema snapshots, the live
    /// advertisement must carry the exact `annotations.readOnlyHint=true`
    /// value. Execute-mode callers intentionally use [`Self::call`] so the
    /// historical network authorization semantics remain compatible.
    pub async fn call_read_only(
        &self,
        config: McpServerConfig,
        tool: &str,
        arguments: Value,
        expected_catalog_sha256: Option<&str>,
        expected_schema_sha256: Option<&str>,
    ) -> Result<McpInvocation, McpRuntimeError> {
        self.call_with_policy(
            config,
            tool,
            arguments,
            expected_catalog_sha256,
            expected_schema_sha256,
            true,
        )
        .await
    }

    async fn call_with_policy(
        &self,
        config: McpServerConfig,
        tool: &str,
        arguments: Value,
        expected_catalog_sha256: Option<&str>,
        expected_schema_sha256: Option<&str>,
        require_read_only_hint: bool,
    ) -> Result<McpInvocation, McpRuntimeError> {
        if require_read_only_hint
            && (expected_catalog_sha256.is_none() || expected_schema_sha256.is_none())
        {
            return Err(McpRuntimeError::SchemaChanged);
        }
        let config = config.normalized()?;
        let key = McpSessionKey::new(config.project_id, config.server_id);
        let session = self.session_for(&config).await?;
        let catalog = match self.refresh_catalog(&session, &config).await {
            Ok(catalog) => catalog,
            Err(error) => {
                self.invalidate(&key).await;
                return Err(error);
            }
        };
        if expected_catalog_sha256.is_some_and(|expected| expected != catalog.sha256) {
            session.set_state(McpSessionState::Stale).await;
            return Err(McpRuntimeError::SchemaChanged);
        }
        let advertised = catalog
            .tools
            .iter()
            .find(|entry| entry.get("name").and_then(Value::as_str) == Some(tool))
            .ok_or_else(|| McpRuntimeError::ToolNotFound(tool.to_owned()))?;
        // Supplying both frozen digests is the read-only target path. The
        // raw third-party annotation is a necessary current fact, but never a
        // host guarantee; the user approval policy remains authoritative.
        if require_read_only_hint {
            match advertised
                .get("annotations")
                .and_then(Value::as_object)
                .and_then(|annotations| annotations.get("readOnlyHint").and_then(Value::as_bool))
            {
                None => return Err(McpRuntimeError::ReadOnlyHintMissing),
                Some(false) => return Err(McpRuntimeError::ReadOnlyHintNotTrue),
                Some(true) => {}
            }
        }
        if let Some(expected) = expected_schema_sha256 {
            let schema = advertised
                .get("inputSchema")
                .or_else(|| advertised.get("input_schema"))
                .cloned()
                .unwrap_or_else(|| json!({"type":"object"}));
            if schema_digest(&schema) != expected {
                session.set_state(McpSessionState::Stale).await;
                return Err(McpRuntimeError::SchemaChanged);
            }
        }
        let args = arguments
            .as_object()
            .cloned()
            .ok_or(McpRuntimeError::InvalidArguments)?;
        let params = CallToolRequestParams::new(tool.to_owned()).with_arguments(args);
        // A tools/list_changed notification can arrive after the refresh
        // snapshot but before the request is sent. Re-check the live session
        // immediately before crossing the peer boundary and fail closed
        // without clearing the stale marker, so the next attempt must refresh
        // again instead of dispatching against a changed catalog.
        if session.stale.load(Ordering::Acquire)
            || matches!(session.state().await, McpSessionState::Stale)
        {
            session.set_state(McpSessionState::Stale).await;
            return Err(McpRuntimeError::Stale);
        }
        let result = tokio::time::timeout(config.timeout(), session.peer.call_tool(params)).await;
        let result = match result {
            Ok(Ok(result)) => result,
            // A JSON-RPC error is a completed server response, not a broken
            // transport. Preserve the diagnostic and let the agent correct its
            // call without killing the session or asking to verify side effects.
            Ok(Err(ServiceError::McpError(error))) => {
                CallToolResult::error(vec![ContentBlock::text(format!(
                    "MCP tool failed: {error}"
                ))])
            }
            Ok(Err(error)) => {
                tokio::task::yield_now().await;
                let diagnostic = append_stderr(error.to_string(), &session.stderr_tail().await);
                self.invalidate(&key).await;
                return Err(McpRuntimeError::Transport(diagnostic));
            }
            Err(_) => {
                self.invalidate(&key).await;
                return Err(McpRuntimeError::Timeout(config.timeout_secs));
            }
        };
        let result = serde_json::to_value(result)
            .map_err(|error| McpRuntimeError::Serialization(error.to_string()))?;
        Ok(McpInvocation {
            server_name: config.name,
            result,
            tool_catalog_sha256: catalog.sha256,
            generation: catalog.generation,
            stderr_tail: session.stderr_tail().await,
        })
    }

    pub async fn state(&self, key: &McpSessionKey) -> McpSessionState {
        let session = self.sessions.lock().await.get(key).cloned();
        match session {
            Some(session) => *session.state.read().await,
            None => McpSessionState::Disconnected,
        }
    }

    pub async fn catalog(&self, key: &McpSessionKey) -> Option<McpToolCatalog> {
        let session = self.sessions.lock().await.get(key).cloned();
        match session {
            Some(session) => Some(session.catalog.read().await.clone()),
            None => None,
        }
    }

    pub async fn invalidate(&self, key: &McpSessionKey) {
        let session = self.sessions.lock().await.remove(key);
        if let Some(session) = session {
            close_session(&session).await;
        }
    }

    pub async fn invalidate_server(&self, server_id: Uuid) {
        let sessions = {
            let mut sessions = self.sessions.lock().await;
            let keys = sessions
                .keys()
                .filter(|key| key.server_id == server_id)
                .cloned()
                .collect::<Vec<_>>();
            keys.into_iter()
                .filter_map(|key| sessions.remove(&key))
                .collect::<Vec<_>>()
        };
        for session in sessions {
            close_session(&session).await;
        }
    }

    pub async fn shutdown(&self) {
        let sessions = self
            .sessions
            .lock()
            .await
            .drain()
            .map(|(_, session)| session)
            .collect::<Vec<_>>();
        for session in sessions {
            close_session(&session).await;
        }
    }

    async fn session_for(
        &self,
        config: &McpServerConfig,
    ) -> Result<Arc<McpSession>, McpRuntimeError> {
        let key = McpSessionKey::new(config.project_id, config.server_id);
        let fingerprint = config.fingerprint();
        if let Some(session) = self.sessions.lock().await.get(&key).cloned() {
            if session.config_fingerprint == fingerprint
                && !matches!(
                    session.state().await,
                    McpSessionState::Failed | McpSessionState::Stopping
                )
            {
                return Ok(session);
            }
            self.invalidate(&key).await;
        }
        let session = self.connect(config).await?;
        let mut sessions = self.sessions.lock().await;
        if let Some(existing) = sessions.get(&key).cloned() {
            drop(sessions);
            close_session(&session).await;
            return Ok(existing);
        }
        sessions.insert(key, session.clone());
        Ok(session)
    }

    async fn connect(&self, config: &McpServerConfig) -> Result<Arc<McpSession>, McpRuntimeError> {
        let stale = Arc::new(AtomicBool::new(false));
        let stderr_tail = Arc::new(Mutex::new(String::new()));
        let mut command = background_command(&config.command);
        command.args(&config.args);
        if let Some(cwd) = config.cwd.as_deref() {
            command.current_dir(cwd);
        }
        for (name, value) in &config.env {
            if name.trim().is_empty() {
                return Err(McpRuntimeError::InvalidConfig(
                    "MCP environment variable name is empty".into(),
                ));
            }
            command.env(name, value);
        }
        let (transport, stderr) = TokioChildProcess::builder(command.configure(|command| {
            command.kill_on_drop(true);
        }))
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| McpRuntimeError::Launch(error.to_string()))?;
        if let Some(stderr) = stderr {
            spawn_stderr_tail(stderr, stderr_tail.clone());
        }
        let handler = NotificationHandler {
            stale: stale.clone(),
        };
        let client = tokio::time::timeout(config.timeout(), handler.serve(transport))
            .await
            .map_err(|_| McpRuntimeError::Timeout(config.timeout_secs))?
            .map_err(|error| McpRuntimeError::Initialize(error.to_string()))?;
        let peer = client.peer().clone();
        let session = Arc::new(McpSession {
            config_fingerprint: config.fingerprint(),
            peer,
            service: Mutex::new(client),
            state: RwLock::new(McpSessionState::Ready),
            stale,
            catalog: RwLock::new(McpToolCatalog {
                tools: Vec::new(),
                sha256: catalog_digest(&[]),
                generation: 0,
            }),
            stderr_tail,
        });
        Ok(session)
    }

    async fn refresh_catalog(
        &self,
        session: &Arc<McpSession>,
        config: &McpServerConfig,
    ) -> Result<McpToolCatalog, McpRuntimeError> {
        if session.stale.swap(false, Ordering::AcqRel)
            || matches!(session.state().await, McpSessionState::Stale)
        {
            session.set_state(McpSessionState::Stale).await;
            return Err(McpRuntimeError::Stale);
        }
        let tools = tokio::time::timeout(config.timeout(), session.peer.list_all_tools())
            .await
            .map_err(|_| McpRuntimeError::Timeout(config.timeout_secs))?
            .map_err(|error| McpRuntimeError::Transport(error.to_string()))?;
        let tools = tools
            .into_iter()
            .map(|tool| {
                serde_json::to_value(tool)
                    .map_err(|error| McpRuntimeError::Serialization(error.to_string()))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let digest = catalog_digest(&tools);
        let mut catalog = session.catalog.write().await;
        let generation = if catalog.sha256 == digest {
            catalog.generation
        } else {
            catalog.generation.saturating_add(1)
        };
        *catalog = McpToolCatalog {
            tools,
            sha256: digest,
            generation,
        };
        session.set_state(McpSessionState::Ready).await;
        Ok(catalog.clone())
    }
}

impl McpSession {
    async fn state(&self) -> McpSessionState {
        *self.state.read().await
    }

    async fn set_state(&self, state: McpSessionState) {
        *self.state.write().await = state;
    }

    async fn stderr_tail(&self) -> String {
        self.stderr_tail.lock().await.clone()
    }
}

async fn close_session(session: &Arc<McpSession>) {
    session.set_state(McpSessionState::Stopping).await;
    let mut service = session.service.lock().await;
    let _ = tokio::time::timeout(Duration::from_secs(3), service.close()).await;
    session.set_state(McpSessionState::Disconnected).await;
}

fn server_info(peer: &Peer<RoleClient>) -> Result<Value, McpRuntimeError> {
    peer.peer_info()
        .as_deref()
        .map(serde_json::to_value)
        .transpose()
        .map_err(|error| McpRuntimeError::Serialization(error.to_string()))?
        .ok_or_else(|| McpRuntimeError::Initialize("MCP server did not return server info".into()))
}

fn schema_digest(value: &Value) -> String {
    hex::encode(Sha256::digest(
        serde_json::to_vec(value).expect("MCP schema is serializable"),
    ))
}

fn catalog_digest(tools: &[Value]) -> String {
    let mut tools = tools.to_vec();
    tools.sort_by(|left, right| {
        left.get("name")
            .and_then(Value::as_str)
            .cmp(&right.get("name").and_then(Value::as_str))
    });
    hex::encode(Sha256::digest(
        serde_json::to_vec(&tools).expect("MCP tool catalog is serializable"),
    ))
}

fn append_stderr(message: String, stderr_tail: &str) -> String {
    if stderr_tail.trim().is_empty() {
        message
    } else {
        format!("{message}; server stderr: {}", stderr_tail.trim())
    }
}

fn spawn_stderr_tail<R>(mut stderr: R, tail: Arc<Mutex<String>>)
where
    R: AsyncRead + Unpin + Send + 'static,
{
    tokio::spawn(async move {
        let mut buffer = [0_u8; 1024];
        loop {
            let read = match stderr.read(&mut buffer).await {
                Ok(0) | Err(_) => break,
                Ok(read) => read,
            };
            let mut current = tail.lock().await;
            let mut bytes = current.as_bytes().to_vec();
            bytes.extend_from_slice(&buffer[..read]);
            if bytes.len() > STDERR_TAIL_BYTES {
                bytes = bytes[bytes.len() - STDERR_TAIL_BYTES..].to_vec();
            }
            *current = String::from_utf8_lossy(&bytes).into_owned();
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_binding_rejects_ambiguous_secret_sources() {
        let binding = McpEnvBinding {
            name: "NCBI_API_KEY".into(),
            value: Some("literal".into()),
            credential_reference: Some("keyring://ncbi".into()),
        };
        assert!(binding.validate().is_err());
    }

    #[test]
    fn resolved_environment_secrets_are_not_serialized() {
        let config = McpServerConfig {
            project_id: Uuid::new_v4(),
            server_id: Uuid::new_v4(),
            name: "demo".into(),
            command: "demo".into(),
            args: vec![],
            cwd: None,
            env: vec![("NCBI_API_KEY".into(), "super-secret".into())],
            timeout_secs: 60,
        };
        let serialized = serde_json::to_string(&config).unwrap();
        assert!(!serialized.contains("super-secret"));
        assert!(!serialized.contains("NCBI_API_KEY"));
        let mut rotated = config.clone();
        rotated.env[0].1 = "rotated-secret".into();
        assert_ne!(config.fingerprint(), rotated.fingerprint());
    }

    #[test]
    fn config_fingerprint_includes_project_and_server_scope() {
        let project = Uuid::new_v4();
        let mut left = McpServerConfig {
            project_id: project,
            server_id: Uuid::new_v4(),
            name: "demo".into(),
            command: "demo".into(),
            args: vec![],
            cwd: None,
            env: vec![],
            timeout_secs: 60,
        };
        let right = left.clone();
        assert_eq!(
            left.clone().normalized().unwrap().fingerprint(),
            right.clone().normalized().unwrap().fingerprint()
        );
        left.project_id = Uuid::new_v4();
        assert_ne!(
            left.clone().normalized().unwrap().fingerprint(),
            right.clone().normalized().unwrap().fingerprint()
        );
    }

    #[test]
    fn catalog_digest_is_order_independent() {
        let left = vec![json!({"name":"b"}), json!({"name":"a"})];
        let right = vec![json!({"name":"a"}), json!({"name":"b"})];
        assert_eq!(catalog_digest(&left), catalog_digest(&right));
    }
}
