use serde::{Deserialize, Serialize};

use super::{AgentRunSpecV3, CompletionLedgerV3};

pub const CONTEXT_COMPACTION_PERCENT_V3: u64 = 75;
pub const RECENT_TOOL_STEPS_V3: usize = 8;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "source", rename_all = "snake_case")]
pub enum ContextSourceV3 {
    VerifiedFact {
        sequence: u64,
        content: String,
    },
    ToolStep {
        sequence: u64,
        tool_id: String,
        content: String,
    },
    Skill {
        package_id: String,
        reference: String,
        content: String,
    },
    McpDescription {
        server_id: String,
        tool_id: String,
        content: String,
    },
}

impl ContextSourceV3 {
    pub fn sequence(&self) -> Option<u64> {
        match self {
            Self::VerifiedFact { sequence, .. } | Self::ToolStep { sequence, .. } => {
                Some(*sequence)
            }
            Self::Skill { .. } | Self::McpDescription { .. } => None,
        }
    }

    fn render(&self) -> String {
        match self {
            Self::VerifiedFact { sequence, content } => {
                format!("[VERIFIED_EVENT:{sequence}] {content}")
            }
            Self::ToolStep {
                sequence,
                tool_id,
                content,
            } => format!(
                "[UNTRUSTED_TOOL_OUTPUT event={sequence} tool={tool_id}] {content} [/UNTRUSTED_TOOL_OUTPUT]"
            ),
            Self::Skill {
                package_id,
                reference,
                content,
            } => format!(
                "[UNTRUSTED_SKILL package={package_id} reference={reference}] {content} [/UNTRUSTED_SKILL]"
            ),
            Self::McpDescription {
                server_id,
                tool_id,
                content,
            } => format!(
                "[UNTRUSTED_MCP_DESCRIPTION server={server_id} tool={tool_id}] {content} [/UNTRUSTED_MCP_DESCRIPTION]"
            ),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StructuredContextV3 {
    pub compaction_required: bool,
    pub recent_tool_steps: Vec<ContextSourceV3>,
    pub rendered: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextSnapshotV3 {
    pub first_sequence: u64,
    pub last_sequence: u64,
    pub last_event_hash: String,
    pub ledger: CompletionLedgerV3,
    pub evidence_references: Vec<u64>,
    pub narrative: String,
}

pub fn build_context(
    spec: &AgentRunSpecV3,
    ledger: &CompletionLedgerV3,
    sources: Vec<ContextSourceV3>,
    context_budget_tokens: u64,
    estimated_input_tokens: u64,
) -> StructuredContextV3 {
    let threshold = context_budget_tokens.saturating_mul(CONTEXT_COMPACTION_PERCENT_V3) / 100;
    let compaction_required = estimated_input_tokens >= threshold;

    let mut tool_steps = sources
        .iter()
        .filter(|source| matches!(source, ContextSourceV3::ToolStep { .. }))
        .cloned()
        .collect::<Vec<_>>();
    let keep_from = tool_steps.len().saturating_sub(RECENT_TOOL_STEPS_V3);
    tool_steps = tool_steps.split_off(keep_from);

    let criteria = ledger
        .criteria
        .iter()
        .map(|criterion| {
            format!(
                "- {}: {} evidence={:?}",
                criterion.id, criterion.description, criterion.evidence_sequences
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let artifacts = ledger
        .verified_artifacts
        .iter()
        .map(|artifact| {
            format!(
                "- {} bytes={} sha256={} evidence={}",
                artifact.path, artifact.size_bytes, artifact.sha256, artifact.evidence_sequence
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let recent = tool_steps
        .iter()
        .map(ContextSourceV3::render)
        .collect::<Vec<_>>()
        .join("\n");
    let rendered = format!(
        "OBJECTIVE\n{}\nCOMPLETION_CRITERIA\n{}\nUNRESOLVED_ERRORS\n{}\nUNCERTAIN_SIDE_EFFECTS\n{}\nVERIFIED_ARTIFACTS\n{}\nRECENT_TOOL_STEPS\n{}",
        spec.objective,
        criteria,
        ledger.unresolved_errors.join("\n"),
        ledger.uncertain_side_effects.join("\n"),
        artifacts,
        recent
    );

    StructuredContextV3 {
        compaction_required,
        recent_tool_steps: tool_steps,
        rendered,
    }
}
