use serde::{Deserialize, Serialize};

use super::RunStatusV3;

pub const MAX_REVIEW_FINDINGS_V3: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FindingSeverityV3 {
    Error,
    Warn,
    Ok,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewFindingV3 {
    pub severity: FindingSeverityV3,
    pub summary: String,
    pub evidence: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewReportV3 {
    pub cycle: u8,
    pub findings: Vec<ReviewFindingV3>,
}

impl ReviewReportV3 {
    pub fn new(cycle: u8, mut findings: Vec<ReviewFindingV3>) -> Self {
        findings.truncate(MAX_REVIEW_FINDINGS_V3);
        Self { cycle, findings }
    }

    pub fn has_errors(&self) -> bool {
        self.findings
            .iter()
            .any(|finding| finding.severity == FindingSeverityV3::Error)
    }

    pub fn next_status(&self, completed_corrections: u8, max_corrections: u8) -> RunStatusV3 {
        if !self.has_errors() {
            RunStatusV3::Completed
        } else if completed_corrections < max_corrections {
            RunStatusV3::Running
        } else {
            RunStatusV3::NeedsAttention
        }
    }
}
