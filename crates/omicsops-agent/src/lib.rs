use std::collections::{BTreeMap, BTreeSet};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use thiserror::Error;
use uuid::Uuid;

pub mod harness_v3;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum AgentError {
    #[error("tool {0} is not registered")]
    UnknownTool(String),
    #[error("tool {tool} did not declare capability {capability:?}")]
    UndeclaredCapability {
        tool: String,
        capability: Capability,
    },
    #[error("at most three specialist tasks may be active")]
    SpecialistLimit,
    #[error("specialist task {0} is already active")]
    SpecialistAlreadyActive(String),
    #[error("at most three conversation turns may be active")]
    ActiveTurnLimit,
    #[error("conversation turn {0} is already active")]
    TurnAlreadyActive(Uuid),
    #[error("model provider error: {0}")]
    Model(String),
    #[error("kernel protocol error: {0}")]
    KernelProtocol(String),
}

pub type AgentResult<T> = Result<T, AgentError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Capability {
    ReadLocal,
    WriteLocal,
    ExecuteRemote,
    QueryResearchSource,
    LaunchMcp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApprovalDecision {
    Allowed,
    Required,
}

#[derive(Debug, Default, Clone)]
pub struct ApprovalGate;

impl ApprovalGate {
    pub fn decision(&self, capability: Capability) -> ApprovalDecision {
        match capability {
            Capability::ReadLocal => ApprovalDecision::Allowed,
            Capability::WriteLocal
            | Capability::ExecuteRemote
            | Capability::QueryResearchSource
            | Capability::LaunchMcp => ApprovalDecision::Required,
        }
    }
}

#[derive(Debug, Default, Clone)]
pub struct ToolRegistry {
    tools: BTreeMap<String, BTreeSet<Capability>>,
}

impl ToolRegistry {
    pub fn register(
        &mut self,
        name: impl Into<String>,
        capabilities: impl IntoIterator<Item = Capability>,
    ) {
        self.tools
            .insert(name.into(), capabilities.into_iter().collect());
    }

    pub fn authorize(&self, tool: &str, capability: Capability) -> AgentResult<()> {
        let capabilities = self
            .tools
            .get(tool)
            .ok_or_else(|| AgentError::UnknownTool(tool.into()))?;
        if capabilities.contains(&capability) {
            Ok(())
        } else {
            Err(AgentError::UndeclaredCapability {
                tool: tool.into(),
                capability,
            })
        }
    }
}

#[derive(Debug, Default, Clone)]
pub struct SpecialistDispatcher {
    active: BTreeSet<String>,
}

impl SpecialistDispatcher {
    pub fn start(&mut self, specialist: impl Into<String>) -> AgentResult<()> {
        let specialist = specialist.into();
        if self.active.contains(&specialist) {
            return Err(AgentError::SpecialistAlreadyActive(specialist));
        }
        if self.active.len() >= 3 {
            return Err(AgentError::SpecialistLimit);
        }
        self.active.insert(specialist);
        Ok(())
    }

    pub fn finish(&mut self, specialist: &str) {
        self.active.remove(specialist);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpecialistKind {
    Literature,
    Statistics,
    CodeReview,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpecialistFinding {
    pub summary: String,
    pub evidence: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpecialistProposedAction {
    pub tool: String,
    pub capability: Capability,
    pub arguments: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpecialistReport {
    pub task_id: Uuid,
    pub kind: SpecialistKind,
    pub findings: Vec<SpecialistFinding>,
    pub proposed_actions: Vec<SpecialistProposedAction>,
}

impl SpecialistReport {
    pub fn actions_requiring_main_approval(
        &self,
        gate: &ApprovalGate,
    ) -> Vec<&SpecialistProposedAction> {
        self.proposed_actions
            .iter()
            .filter(|action| gate.decision(action.capability) == ApprovalDecision::Required)
            .collect()
    }
}

#[derive(Debug, Default, Clone)]
pub struct ActiveTurnCoordinator {
    active: BTreeSet<Uuid>,
}

impl ActiveTurnCoordinator {
    pub fn start(&mut self, turn_id: Uuid) -> AgentResult<()> {
        if self.active.contains(&turn_id) {
            return Err(AgentError::TurnAlreadyActive(turn_id));
        }
        if self.active.len() >= 3 {
            return Err(AgentError::ActiveTurnLimit);
        }
        self.active.insert(turn_id);
        Ok(())
    }

    pub fn finish(&mut self, turn_id: Uuid) {
        self.active.remove(&turn_id);
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "payload", rename_all = "kebab-case")]
pub enum AgentEventKind {
    TurnStarted,
    TextDelta(String),
    ToolArgumentsDelta {
        name: String,
        json_fragment: String,
    },
    ProviderRetrying {
        attempt: u8,
        delay_ms: u64,
        message: String,
    },
    ToolProposed {
        tool: String,
        arguments: Value,
    },
    ApprovalRequired {
        approval_id: Uuid,
        summary: String,
    },
    PlanReady {
        plan_id: Uuid,
        plan_hash: String,
    },
    TurnCompleted,
    TurnFailed {
        message: String,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentEvent {
    pub project_id: Uuid,
    pub conversation_id: Uuid,
    pub turn_id: Uuid,
    pub sequence: u64,
    pub occurred_at: DateTime<Utc>,
    pub event: AgentEventKind,
}

impl AgentEvent {
    pub fn text_delta(
        project_id: Uuid,
        conversation_id: Uuid,
        turn_id: Uuid,
        text: impl Into<String>,
    ) -> Self {
        Self {
            project_id,
            conversation_id,
            turn_id,
            sequence: 0,
            occurred_at: Utc::now(),
            event: AgentEventKind::TextDelta(text.into()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelRequest {
    pub system: String,
    pub messages: Vec<ModelMessage>,
    pub tool_name: Option<String>,
    pub tool_schema: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ModelStreamEvent {
    TextDelta(String),
    ToolArgumentsDelta {
        name: String,
        json_fragment: String,
    },
    Retrying {
        attempt: u8,
        delay_ms: u64,
        message: String,
    },
    Completed,
}

#[derive(Debug, Clone)]
pub struct ToolArgumentBuffer {
    expected_name: String,
    arguments: String,
}

impl ToolArgumentBuffer {
    pub fn new(expected_name: impl Into<String>) -> Self {
        Self {
            expected_name: expected_name.into(),
            arguments: String::new(),
        }
    }

    pub fn push(&mut self, event: ModelStreamEvent) -> AgentResult<()> {
        if let ModelStreamEvent::ToolArgumentsDelta {
            name,
            json_fragment,
        } = event
        {
            if !name.is_empty() && name != self.expected_name {
                return Err(AgentError::Model(format!(
                    "expected tool {}, received {name}",
                    self.expected_name
                )));
            }
            self.arguments.push_str(&json_fragment);
        }
        Ok(())
    }

    pub fn finish(self) -> AgentResult<Value> {
        if self.arguments.is_empty() {
            return Err(AgentError::Model(format!(
                "model did not call {}",
                self.expected_name
            )));
        }
        serde_json::from_str(&self.arguments).map_err(|error| AgentError::Model(error.to_string()))
    }
}

pub fn structured_value_from_text(text: &str) -> AgentResult<Value> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err(AgentError::Model(
            "model returned no structured text".into(),
        ));
    }
    if let Ok(value) = serde_json::from_str(trimmed) {
        return Ok(value);
    }
    for block in trimmed.split("```").skip(1).step_by(2) {
        let candidate = block
            .trim()
            .strip_prefix("json")
            .unwrap_or(block.trim())
            .trim();
        if let Ok(value) = serde_json::from_str(candidate) {
            return Ok(value);
        }
    }
    if let (Some(start), Some(end)) = (trimmed.find('{'), trimmed.rfind('}')) {
        if start < end {
            if let Ok(value) = serde_json::from_str(&trimmed[start..=end]) {
                return Ok(value);
            }
        }
    }
    Err(AgentError::Model(
        "model text did not contain a valid JSON object".into(),
    ))
}

#[async_trait]
pub trait ModelProvider: Send + Sync {
    fn profile_id(&self) -> Uuid;
    async fn stream(&self, request: ModelRequest) -> AgentResult<Vec<ModelStreamEvent>>;
}

pub struct AgentSession<P> {
    pub project_id: Uuid,
    pub conversation_id: Uuid,
    pub provider: P,
    pub tools: ToolRegistry,
    pub approval_gate: ApprovalGate,
}

impl<P> AgentSession<P> {
    pub fn new(project_id: Uuid, conversation_id: Uuid, provider: P) -> Self {
        Self {
            project_id,
            conversation_id,
            provider,
            tools: ToolRegistry::default(),
            approval_gate: ApprovalGate,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "transport", rename_all = "snake_case")]
pub enum McpTransport {
    Stdio { command: String, args: Vec<String> },
    Http { url: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpServerDeclaration {
    pub name: String,
    pub transport: McpTransport,
}

impl McpServerDeclaration {
    pub fn new(name: impl Into<String>, transport: McpTransport) -> Self {
        Self {
            name: name.into(),
            transport,
        }
    }

    pub fn validate(&self) -> AgentResult<()> {
        match &self.transport {
            McpTransport::Stdio { command, .. } if !command.trim().is_empty() => Ok(()),
            McpTransport::Stdio { .. } => Err(AgentError::Model(
                "MCP stdio command cannot be empty".into(),
            )),
            McpTransport::Http { .. } => Err(AgentError::Model(
                "MCP HTTP transport is not supported in the first release".into(),
            )),
        }
    }

    pub fn approval(&self, gate: &ApprovalGate) -> ApprovalDecision {
        gate.decision(Capability::LaunchMcp)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KernelLanguage {
    Python,
    R,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KernelState {
    Created,
    Running,
    Interrupted,
    Stopped,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum KernelRequest {
    Execute {
        session_id: Uuid,
        request_id: Uuid,
        code: String,
        capture_paths: Vec<String>,
    },
    Shutdown {
        session_id: Uuid,
        request_id: Uuid,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "payload", rename_all = "snake_case")]
pub enum KernelEventKind {
    Started,
    Stdout(String),
    Stderr(String),
    Artifact {
        relative_path: String,
        size_bytes: u64,
        sha256: String,
    },
    Completed,
    Failed {
        message: String,
    },
    Stopped,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KernelEvent {
    pub project_id: Uuid,
    pub session_id: Uuid,
    pub request_id: Uuid,
    pub sequence: u64,
    pub occurred_at: DateTime<Utc>,
    pub event: KernelEventKind,
}

#[derive(Debug, Clone)]
pub struct KernelEventDecoder {
    project_id: Uuid,
    session_id: Uuid,
    request_id: Uuid,
    next_sequence: u64,
}

impl KernelEventDecoder {
    pub fn new(project_id: Uuid, session_id: Uuid, request_id: Uuid) -> Self {
        Self {
            project_id,
            session_id,
            request_id,
            next_sequence: 1,
        }
    }

    pub fn accept(&mut self, event: KernelEvent) -> AgentResult<KernelEvent> {
        if event.project_id != self.project_id
            || event.session_id != self.session_id
            || event.request_id != self.request_id
        {
            return Err(AgentError::KernelProtocol(
                "kernel event identity did not match the active request".into(),
            ));
        }
        if event.sequence != self.next_sequence {
            return Err(AgentError::KernelProtocol(format!(
                "expected kernel sequence {}, received {}",
                self.next_sequence, event.sequence
            )));
        }
        self.next_sequence += 1;
        Ok(event)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FormalStepProposal {
    pub name: String,
    pub version: u32,
    pub language: KernelLanguage,
    pub code: String,
    pub code_sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KernelSession {
    pub id: Uuid,
    pub project_id: Uuid,
    pub language: KernelLanguage,
    pub state: KernelState,
    saved_cells: Vec<String>,
    ephemeral_cells: Vec<String>,
}

impl KernelSession {
    pub fn new(id: Uuid, project_id: Uuid, language: KernelLanguage) -> Self {
        Self {
            id,
            project_id,
            language,
            state: KernelState::Created,
            saved_cells: Vec::new(),
            ephemeral_cells: Vec::new(),
        }
    }

    pub fn start(&mut self) -> AgentResult<()> {
        if self.state != KernelState::Created && self.state != KernelState::Interrupted {
            return Err(AgentError::Model(
                "kernel cannot start from its current state".into(),
            ));
        }
        self.state = KernelState::Running;
        Ok(())
    }

    pub fn save_cell(&mut self, code: impl Into<String>) {
        self.saved_cells.push(code.into());
    }
    pub fn note_ephemeral_cell(&mut self, code: impl Into<String>) {
        self.ephemeral_cells.push(code.into());
    }
    pub fn interrupt(&mut self) {
        self.state = KernelState::Interrupted;
        self.ephemeral_cells.clear();
    }
    pub fn rebuild_cells(&self) -> Vec<&str> {
        self.saved_cells.iter().map(String::as_str).collect()
    }
    pub fn saved_cells_owned(&self) -> Vec<String> {
        self.saved_cells.clone()
    }

    pub fn record_executed_cell(
        &mut self,
        code: impl Into<String>,
        saved: bool,
    ) -> AgentResult<Option<usize>> {
        if self.state != KernelState::Running {
            return Err(AgentError::KernelProtocol(
                "kernel must be running before executing a cell".into(),
            ));
        }
        let code = code.into();
        if saved {
            self.saved_cells.push(code);
            Ok(Some(self.saved_cells.len() - 1))
        } else {
            self.ephemeral_cells.push(code);
            Ok(None)
        }
    }

    pub fn stop(&mut self) {
        self.state = KernelState::Stopped;
        self.ephemeral_cells.clear();
    }

    pub fn promote_cell(
        &self,
        index: usize,
        name: impl Into<String>,
        version: u32,
    ) -> AgentResult<FormalStepProposal> {
        let code = self
            .saved_cells
            .get(index)
            .ok_or_else(|| AgentError::Model("only saved code cells can be promoted".into()))?;
        let mut hasher = Sha256::new();
        hasher.update(code.as_bytes());
        Ok(FormalStepProposal {
            name: name.into(),
            version,
            language: self.language,
            code: code.clone(),
            code_sha256: hex::encode(hasher.finalize()),
        })
    }
}
