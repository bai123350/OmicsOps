use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::CoreResult;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RunEventKindV2 {
    RunStarted,
    RunResumed,
    StepStarted,
    StepSucceeded,
    StepFailed,
    ApprovalRequired,
    VerificationFailed,
    NeedsAttention,
    RunSucceeded,
    RunCanceled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct RunEventV2 {
    pub sequence: u64,
    pub timestamp: DateTime<Utc>,
    pub run_id: Uuid,
    pub kind: RunEventKindV2,
    pub message: String,
    pub prev_hash: Option<String>,
    pub details: BTreeMap<String, String>,
    pub event_hash: String,
}

#[derive(Serialize)]
struct EventHashPayload<'a> {
    sequence: u64,
    timestamp: DateTime<Utc>,
    run_id: Uuid,
    kind: RunEventKindV2,
    message: &'a str,
    prev_hash: &'a Option<String>,
    details: &'a BTreeMap<String, String>,
}

impl RunEventV2 {
    pub fn new(
        sequence: u64,
        run_id: Uuid,
        kind: RunEventKindV2,
        message: impl Into<String>,
        prev_hash: Option<String>,
        details: BTreeMap<String, String>,
    ) -> CoreResult<Self> {
        let mut event = Self {
            sequence,
            timestamp: Utc::now(),
            run_id,
            kind,
            message: message.into(),
            prev_hash,
            details,
            event_hash: String::new(),
        };
        event.event_hash = event.computed_hash()?;
        Ok(event)
    }

    pub fn verify_hash(&self) -> bool {
        self.computed_hash()
            .is_ok_and(|computed| computed == self.event_hash)
    }

    fn computed_hash(&self) -> CoreResult<String> {
        let payload = EventHashPayload {
            sequence: self.sequence,
            timestamp: self.timestamp,
            run_id: self.run_id,
            kind: self.kind,
            message: &self.message,
            prev_hash: &self.prev_hash,
            details: &self.details,
        };
        Ok(format!(
            "{:x}",
            Sha256::digest(serde_json::to_vec(&payload)?)
        ))
    }
}
