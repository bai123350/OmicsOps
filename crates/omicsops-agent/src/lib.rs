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
    pub content: ModelMessageContent,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ModelMessageContent {
    Text(String),
    Parts(Vec<ModelContentPart>),
}

impl From<String> for ModelMessageContent {
    fn from(value: String) -> Self {
        Self::Text(value)
    }
}

impl From<&str> for ModelMessageContent {
    fn from(value: &str) -> Self {
        Self::Text(value.into())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ModelContentPart {
    Text {
        text: String,
    },
    Image {
        media_type: String,
        data_base64: String,
    },
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
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum KernelRequest {
    Inspect {
        session_id: Uuid,
        request_id: Uuid,
    },
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
#[serde(
    tag = "kind",
    content = "payload",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum KernelEventKind {
    Inspection {
        objects: Vec<omicsops_protocol::KernelObjectMetadataV4>,
        truncated: bool,
    },
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
    terminal: bool,
}

impl KernelEventDecoder {
    pub fn new(project_id: Uuid, session_id: Uuid, request_id: Uuid) -> Self {
        Self {
            project_id,
            session_id,
            request_id,
            next_sequence: 1,
            terminal: false,
        }
    }

    pub fn accept(&mut self, event: KernelEvent) -> AgentResult<KernelEvent> {
        if self.terminal {
            return Err(AgentError::KernelProtocol(
                "kernel event followed a terminal".into(),
            ));
        }
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
        self.terminal = matches!(
            event.event,
            KernelEventKind::Inspection { .. }
                | KernelEventKind::Completed
                | KernelEventKind::Failed { .. }
                | KernelEventKind::Stopped
        );
        if let KernelEventKind::Inspection { objects, truncated } = &event.event {
            omicsops_protocol::validate_inspection_payload(
                &omicsops_protocol::KernelInspectionPayloadV4 {
                    objects: objects.clone(),
                    truncated: *truncated,
                },
            )
            .map_err(|_| AgentError::KernelProtocol("invalid kernel inspection".into()))?;
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

#[cfg(test)]
mod inspection_tests {
    use super::*;
    use serde_json::json;

    fn frame(project_id: Uuid, session_id: Uuid, request_id: Uuid, sequence: u64) -> KernelEvent {
        serde_json::from_value(json!({"project_id":project_id,"session_id":session_id,"request_id":request_id, "sequence":sequence,"occurred_at":"2026-10-04T00:00:00Z", "event":{"kind":"inspection","payload":{"objects":[],"truncated":false}}})).unwrap()
    }

    #[test]
    fn inspect_typed_events_reject_identity_sequence_and_duplicate_terminal() {
        let (p, s, r) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
        let mut decoder = KernelEventDecoder::new(p, s, r);
        assert!(decoder.accept(frame(p, Uuid::new_v4(), r, 1)).is_err());
        assert!(decoder.accept(frame(p, s, Uuid::new_v4(), 1)).is_err());
        assert!(decoder.accept(frame(p, s, r, 2)).is_err());
        assert!(decoder.accept(frame(p, s, r, 1)).is_ok());
        assert!(decoder.accept(frame(p, s, r, 2)).is_err());
    }

    #[test]
    fn inspection_preserves_execute_shutdown_serialization() {
        let s = Uuid::nil();
        let r = Uuid::nil();
        assert_eq!(
            serde_json::to_value(KernelRequest::Execute {
                session_id: s,
                request_id: r,
                code: "x=1".into(),
                capture_paths: vec![]
            })
            .unwrap(),
            json!({"action":"execute","session_id":s,"request_id":r,"code":"x=1","capture_paths":[]})
        );
        assert_eq!(
            serde_json::to_value(KernelRequest::Shutdown {
                session_id: s,
                request_id: r
            })
            .unwrap(),
            json!({"action":"shutdown","session_id":s,"request_id":r})
        );
        let request: KernelRequest =
            serde_json::from_value(json!({"action":"inspect","session_id":s,"request_id":r}))
                .unwrap();
        assert_eq!(
            serde_json::to_value(request).unwrap(),
            json!({"action":"inspect","session_id":s,"request_id":r})
        );
        assert!(
            serde_json::from_value::<KernelRequest>(
                json!({"action":"inspect","session_id":s,"request_id":r,"code":"secret"})
            )
            .is_err()
        );
    }
    #[test]
    fn inspection_closed_payload_and_budget() {
        let (p, s, r) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
        let mut value = serde_json::to_value(frame(p, s, r, 1)).unwrap();
        value["event"]["payload"]["value"] = json!("SECRET_SENTINEL");
        assert!(serde_json::from_value::<KernelEvent>(value).is_err());
        let object = omicsops_protocol::KernelObjectMetadataV4 {
            name: "x".into(),
            type_label: "list".into(),
            dimensions: None,
            element_count: Some(0),
            summary: "list, metadata only".into(),
        };
        let event = KernelEvent {
            event: KernelEventKind::Inspection {
                objects: vec![object; 65],
                truncated: true,
            },
            ..frame(p, s, r, 1)
        };
        assert!(KernelEventDecoder::new(p, s, r).accept(event).is_err());
        assert!(
            KernelEventDecoder::new(p, s, r)
                .accept(frame(Uuid::new_v4(), s, r, 1))
                .is_err()
        );
    }
}
