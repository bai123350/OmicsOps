use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::{AgentRunSpecV3, RunStatusV3};

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum CompletionErrorV3 {
    #[error("unknown completion criterion {0}")]
    UnknownCriterion(String),
    #[error("completion criterion {0} has no event evidence")]
    MissingEvidence(String),
    #[error("unresolved errors remain")]
    UnresolvedErrors,
    #[error("uncertain side effects remain")]
    UncertainSideEffects,
    #[error("artifact {0} is not verified")]
    InvalidArtifact(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CriterionStateV3 {
    pub id: String,
    pub description: String,
    pub evidence_sequences: Vec<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactEvidenceV3 {
    pub path: String,
    pub size_bytes: u64,
    pub sha256: String,
    pub evidence_sequence: u64,
}

impl ArtifactEvidenceV3 {
    pub fn validate(&self) -> Result<(), CompletionErrorV3> {
        let valid_hash =
            self.sha256.len() == 64 && self.sha256.bytes().all(|byte| byte.is_ascii_hexdigit());
        let valid_path = !self.path.trim().is_empty()
            && !self.path.starts_with('/')
            && !self.path.starts_with('\\')
            && !self.path.split(['/', '\\']).any(|part| part == "..");
        if valid_hash && valid_path && self.evidence_sequence > 0 {
            Ok(())
        } else {
            Err(CompletionErrorV3::InvalidArtifact(self.path.clone()))
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompletionLedgerV3 {
    pub criteria: Vec<CriterionStateV3>,
    pub unresolved_errors: Vec<String>,
    pub uncertain_side_effects: Vec<String>,
    pub verified_artifacts: Vec<ArtifactEvidenceV3>,
}

impl CompletionLedgerV3 {
    pub fn from_spec(spec: &AgentRunSpecV3) -> Self {
        Self {
            criteria: spec
                .completion_criteria
                .iter()
                .map(|criterion| CriterionStateV3 {
                    id: criterion.id.clone(),
                    description: criterion.description.clone(),
                    evidence_sequences: Vec::new(),
                })
                .collect(),
            unresolved_errors: Vec::new(),
            uncertain_side_effects: Vec::new(),
            verified_artifacts: Vec::new(),
        }
    }

    pub fn satisfy(
        &mut self,
        criterion_id: &str,
        evidence_sequences: Vec<u64>,
    ) -> Result<(), CompletionErrorV3> {
        let criterion = self
            .criteria
            .iter_mut()
            .find(|criterion| criterion.id == criterion_id)
            .ok_or_else(|| CompletionErrorV3::UnknownCriterion(criterion_id.into()))?;
        if evidence_sequences.is_empty() || evidence_sequences.contains(&0) {
            return Err(CompletionErrorV3::MissingEvidence(criterion_id.into()));
        }
        criterion.evidence_sequences = evidence_sequences;
        Ok(())
    }

    pub fn can_complete(&self) -> Result<(), CompletionErrorV3> {
        if !self.unresolved_errors.is_empty() {
            return Err(CompletionErrorV3::UnresolvedErrors);
        }
        if !self.uncertain_side_effects.is_empty() {
            return Err(CompletionErrorV3::UncertainSideEffects);
        }
        for criterion in &self.criteria {
            if criterion.evidence_sequences.is_empty() {
                return Err(CompletionErrorV3::MissingEvidence(criterion.id.clone()));
            }
        }
        for artifact in &self.verified_artifacts {
            artifact.validate()?;
        }
        Ok(())
    }

    pub fn terminal_status(&self) -> RunStatusV3 {
        if self.can_complete().is_ok() {
            RunStatusV3::Reviewing
        } else {
            RunStatusV3::NeedsAttention
        }
    }
}
