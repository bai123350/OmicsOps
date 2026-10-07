use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use super::{
    AgentEventKindV4, AgentEventV4, CompactContextRequestV4, ContextArchiveV4, ContextCheckpointV4,
    ContextCompactionStatusV4, RunSpecV4,
};

/// Provider-shaped admission evidence captured during preparation. Missing
/// request bytes mean the complete provider payload could not be measured.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ContextCompactionAdmissionV4 {
    pub configuration_hash: String,
    pub prompt_sha256: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_request_bytes: Option<u64>,
}

/// One transaction-consistent, verified source view for pure preparation.
/// Store-specific run records stay outside the protocol boundary.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ContextCompactionSnapshotV4 {
    pub request: CompactContextRequestV4,
    pub spec: RunSpecV4,
    pub events: Vec<AgentEventV4>,
    pub scientific_state: Value,
    pub scientific_revision: u64,
    pub scientific_sha256: String,
    pub source_through_sequence: u64,
    pub source_head_hash: String,
}

/// Prospective data only. A Store commit must recheck the current database
/// head, science and bindings before persisting any planned event verbatim.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PreparedContextCompactionV4 {
    pub request: CompactContextRequestV4,
    pub source_through_sequence: u64,
    pub source_head_hash: String,
    pub frozen_spec_hash: String,
    pub scientific_revision: u64,
    pub scientific_sha256: String,
    pub transcript: String,
    pub candidate_context: String,
    pub checkpoint: Option<ContextCheckpointV4>,
    pub archive: Option<ContextArchiveV4>,
    pub planned_events: Vec<AgentEventV4>,
    pub status: ContextCompactionStatusV4,
    pub before_bytes: u64,
    pub after_bytes: u64,
    pub admission: ContextCompactionAdmissionV4,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ContextCompactionValidationErrorV4 {
    #[error("invalid context compaction scope")]
    InvalidScope,
    #[error("invalid context compaction status")]
    InvalidStatus,
    #[error("invalid context compaction archive")]
    InvalidArchive,
    #[error("invalid context compaction event chain")]
    InvalidEventChain,
}

impl PreparedContextCompactionV4 {
    /// Checks internal consistency only; it does not authorize a database
    /// write or claim that the captured source is still current.
    pub fn validate_structure(&self) -> Result<(), ContextCompactionValidationErrorV4> {
        use ContextCompactionValidationErrorV4 as Error;

        if self.request.request_id.is_nil()
            || self.request.run_id.is_nil()
            || self.request.project_id.is_nil()
            || self.request.conversation_id.is_nil()
            || self.frozen_spec_hash.is_empty()
            || self.scientific_sha256.is_empty()
            || !is_sha256(&self.admission.configuration_hash)
            || !is_sha256(&self.admission.prompt_sha256)
            || self.planned_events.iter().any(|event| {
                event.run_id != self.request.run_id
                    || event.project_id != self.request.project_id
                    || event.conversation_id != self.request.conversation_id
            })
        {
            return Err(Error::InvalidScope);
        }
        if !matches!(
            self.status,
            ContextCompactionStatusV4::Completed | ContextCompactionStatusV4::NotNeeded
        ) {
            return Err(Error::InvalidStatus);
        }
        if self.before_bytes != self.transcript.len() as u64
            || self.after_bytes != self.candidate_context.len() as u64
        {
            return Err(Error::InvalidArchive);
        }

        match self.status {
            ContextCompactionStatusV4::Completed => {
                let (Some(archive), Some(checkpoint)) = (&self.archive, &self.checkpoint) else {
                    return Err(Error::InvalidArchive);
                };
                if archive.through_sequence != self.source_through_sequence
                    || checkpoint.through_sequence != self.source_through_sequence
                    || archive.size_bytes != self.before_bytes
                    || archive.sha256 != hex::encode(Sha256::digest(self.transcript.as_bytes()))
                {
                    return Err(Error::InvalidArchive);
                }
            }
            ContextCompactionStatusV4::NotNeeded => {
                if self.archive.is_some() || self.checkpoint.is_some() {
                    return Err(Error::InvalidArchive);
                }
            }
            ContextCompactionStatusV4::Attention => unreachable!(),
        }

        if self.planned_events.is_empty() {
            return Err(Error::InvalidEventChain);
        }
        let mut previous_hash = self.source_head_hash.as_str();
        let mut sequence = self.source_through_sequence;
        for event in &self.planned_events {
            sequence = sequence.checked_add(1).ok_or(Error::InvalidEventChain)?;
            if event.sequence != sequence
                || event.previous_hash != previous_hash
                || event.verify().is_err()
            {
                return Err(Error::InvalidEventChain);
            }
            previous_hash = &event.event_hash;
        }

        match self.status {
            ContextCompactionStatusV4::Completed => self.validate_completed_events(),
            ContextCompactionStatusV4::NotNeeded => self.validate_not_needed_events(),
            ContextCompactionStatusV4::Attention => unreachable!(),
        }
    }

    fn validate_started(&self, event: &AgentEventV4) -> bool {
        matches!(&event.event, AgentEventKindV4::ContextCompactionStarted {
            request_id, source_through_sequence, source_head_hash, frozen_spec_hash,
        } if *request_id == self.request.request_id
            && *source_through_sequence == self.source_through_sequence
            && source_head_hash == &self.source_head_hash
            && frozen_spec_hash.as_deref() == Some(self.frozen_spec_hash.as_str()))
    }

    fn validate_not_needed_events(&self) -> Result<(), ContextCompactionValidationErrorV4> {
        use ContextCompactionValidationErrorV4::InvalidEventChain;
        let events = &self.planned_events;
        let outcome = match events.as_slice() {
            [outcome] => outcome,
            [started, outcome] if self.validate_started(started) => outcome,
            _ => return Err(InvalidEventChain),
        };
        if matches!(&outcome.event, AgentEventKindV4::ContextCompactionNotNeeded {
            request_id, before_bytes,
        } if *request_id == self.request.request_id && *before_bytes == self.before_bytes)
        {
            Ok(())
        } else {
            Err(InvalidEventChain)
        }
    }

    fn validate_completed_events(&self) -> Result<(), ContextCompactionValidationErrorV4> {
        use ContextCompactionValidationErrorV4::InvalidEventChain;
        let [started, archived, checkpointed, completed] = self.planned_events.as_slice() else {
            return Err(InvalidEventChain);
        };
        if !self.validate_started(started) {
            return Err(InvalidEventChain);
        }
        let archive = self.archive.as_ref().ok_or(InvalidEventChain)?;
        let checkpoint = self.checkpoint.as_ref().ok_or(InvalidEventChain)?;
        let checkpoint_hash = hex::encode(Sha256::digest(
            serde_json::to_vec(checkpoint).map_err(|_| InvalidEventChain)?,
        ));
        if !matches!(&archived.event, AgentEventKindV4::ContextArchived { archive: value } if value == archive)
            || !matches!(&checkpointed.event, AgentEventKindV4::ContextCheckpointed { checkpoint: value } if value == checkpoint)
            || !matches!(&completed.event, AgentEventKindV4::ContextCompactionCompleted {
                request_id, archive: value, checkpoint_through_sequence,
                checkpoint_sha256, before_bytes, after_bytes, admission,
            } if *request_id == self.request.request_id
                && value == archive
                && *checkpoint_through_sequence == checkpoint.through_sequence
                && checkpoint_sha256 == &checkpoint_hash
                && *before_bytes == self.before_bytes
                && *after_bytes == self.after_bytes
                && admission.as_ref() == Some(&self.admission))
        {
            return Err(InvalidEventChain);
        }
        Ok(())
    }
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}
