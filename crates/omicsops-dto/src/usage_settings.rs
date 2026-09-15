use omicsops_protocol::UsageTotalsV4;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct UsageFilter {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub until: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UsageGroup {
    pub key: String,
    pub label: String,
    pub totals: UsageTotalsV4,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UsageDay {
    pub date: String,
    pub attempts: u32,
    pub tools: u32,
    pub totals: UsageTotalsV4,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UsageTool {
    pub tool_id: String,
    pub dispatched: u64,
    pub succeeded: u64,
    pub failed: u64,
    pub uncertain: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UsageAggregatePage {
    pub totals: UsageTotalsV4,
    pub projects: Vec<UsageGroup>,
    pub models: Vec<UsageGroup>,
    pub days: Vec<UsageDay>,
    pub tools: Vec<UsageTool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
    pub scanned_runs: u32,
    pub omitted_runs: u32,
    pub unattributed_events: u32,
    pub snapshot_at: String,
    pub completeness: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UsageConversationRow {
    pub project_id: Uuid,
    pub conversation_id: Uuid,
    pub label: String,
    pub latest_activity: String,
    pub totals: UsageTotalsV4,
    pub incomplete: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UsageConversationPage {
    pub items: Vec<UsageConversationRow>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
    pub snapshot_at: String,
}
