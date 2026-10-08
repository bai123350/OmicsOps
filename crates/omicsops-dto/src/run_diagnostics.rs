//! Public, metadata-only contracts for validated V4 run diagnostics.

use chrono::{DateTime, Utc};
use omicsops_protocol::UsageTotalsV4;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunDiagnosticsFilterV4 {
    pub run_id: Option<Uuid>,
    pub tool_id: Option<String>,
    pub failures_only: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunDiagnosticsRequestV4 {
    pub project_id: Uuid,
    pub conversation_id: Uuid,
    pub filter: RunDiagnosticsFilterV4,
    pub redaction_keywords: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunDiagnosticsFilterSummaryV4 {
    pub run_id: Option<Uuid>,
    pub tool_label: Option<String>,
    pub failures_only: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunDiagnosticsSourceHeadV4 {
    pub run_id: Uuid,
    pub event_count: u64,
    pub last_sequence: u64,
    pub event_hash: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunDiagnosticStatusV4 {
    Succeeded,
    Failed,
    Uncertain,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunDiagnosticEventV4 {
    pub run_id: Uuid,
    pub sequence: u64,
    pub event_hash: String,
    pub previous_hash: String,
    pub occurred_at: DateTime<Utc>,
    pub event_kind: String,
    pub status: Option<RunDiagnosticStatusV4>,
    pub tool_label: Option<String>,
    pub call_ref: Option<String>,
    pub duration_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunDiagnosticUsageV4 {
    pub run_id: Uuid,
    pub totals: UsageTotalsV4,
    pub missing_observation_attempts: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunDiagnosticsSnapshotV4 {
    pub schema_version: u8,
    pub project_id: Uuid,
    pub conversation_id: Uuid,
    pub filter_summary: RunDiagnosticsFilterSummaryV4,
    pub source_heads: Vec<RunDiagnosticsSourceHeadV4>,
    pub source_chain_validated: bool,
    pub total_event_count: u64,
    pub selected_event_count: u64,
    pub filtered: bool,
    pub events: Vec<RunDiagnosticEventV4>,
    pub usage: Vec<RunDiagnosticUsageV4>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunDiagnosticsExportFormatV4 {
    Html,
    Json,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SaveRunDiagnosticsRequestV4 {
    pub query: RunDiagnosticsRequestV4,
    pub expected_heads: Vec<RunDiagnosticsSourceHeadV4>,
    pub format: RunDiagnosticsExportFormatV4,
}
