use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Metadata for one immutable local composer attachment staged under a
/// project-owned `.omicsops/attachments/<id>/` directory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ComposerAttachmentReceipt {
    pub id: Uuid,
    pub project_id: Uuid,
    pub conversation_id: Uuid,
    pub name: String,
    pub relative_path: String,
    pub size_bytes: u64,
    pub sha256: String,
    pub media_type: String,
}

/// Explicit bytes supplied by a local drop or paste operation. The host
/// decodes and stages the bytes; callers never provide a path to read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StageComposerAttachmentRequest {
    pub project_id: Uuid,
    pub conversation_id: Uuid,
    pub name: String,
    pub content_base64: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn attachment_receipt_wire_shape_is_stable() {
        let receipt = ComposerAttachmentReceipt {
            id: Uuid::from_u128(1),
            project_id: Uuid::from_u128(2),
            conversation_id: Uuid::from_u128(3),
            name: "qc.csv".into(),
            relative_path: ".omicsops/attachments/00000000-0000-0000-0000-000000000001/bytes.csv"
                .into(),
            size_bytes: 7,
            sha256: "a".repeat(64),
            media_type: "text/csv".into(),
        };
        assert_eq!(
            serde_json::to_value(receipt).unwrap(),
            json!({
                "id": Uuid::from_u128(1),
                "project_id": Uuid::from_u128(2),
                "conversation_id": Uuid::from_u128(3),
                "name": "qc.csv",
                "relative_path": ".omicsops/attachments/00000000-0000-0000-0000-000000000001/bytes.csv",
                "size_bytes": 7,
                "sha256": "a".repeat(64),
                "media_type": "text/csv",
            })
        );
    }

    #[test]
    fn stage_request_rejects_unknown_fields() {
        let value = json!({
            "project_id": Uuid::from_u128(1),
            "conversation_id": Uuid::from_u128(2),
            "name": "drop.txt",
            "content_base64": "dGVzdA==",
            "path": "C:/secret.txt",
        });
        assert!(serde_json::from_value::<StageComposerAttachmentRequest>(value).is_err());
    }
}
