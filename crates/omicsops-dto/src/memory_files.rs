use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryFileSummaryV4 {
    pub project_id: Uuid,
    pub name: String,
    pub size_bytes: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryFileV4 {
    pub project_id: Uuid,
    pub name: String,
    pub content: String,
    pub size_bytes: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateMemoryFileRequestV4 {
    pub project_id: Uuid,
    pub name: String,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateMemoryFileRequestV4 {
    pub project_id: Uuid,
    pub name: String,
    pub content: String,
    pub expected_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeleteMemoryFileRequestV4 {
    pub project_id: Uuid,
    pub name: String,
    pub expected_sha256: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn requests_reject_paths_and_fields_outside_the_explicit_contract() {
        let project_id = Uuid::from_u128(7);
        let create: CreateMemoryFileRequestV4 = serde_json::from_value(json!({
            "project_id": project_id,
            "name": "study.md",
            "content": "# Study"
        }))
        .unwrap();
        assert_eq!(create.name, "study.md");
        assert!(
            serde_json::from_value::<CreateMemoryFileRequestV4>(json!({
                "project_id": project_id,
                "name": "study.md",
                "content": "# Study",
                "absolute_path": "C:/escape.md"
            }))
            .is_err()
        );
    }
}
