use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;
use uuid::Uuid;

pub mod provider;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum AgentError {
    #[error("model provider error: {0}")]
    Model(String),
    #[error("kernel protocol error: {0}")]
    KernelProtocol(String),
}

pub type AgentResult<T> = Result<T, AgentError>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelMessage {
    pub role: String,
    pub content: String,
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
