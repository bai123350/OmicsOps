use std::collections::HashSet;

use omicsops_dto::{ComposerAttachmentReceipt, ComposerReference};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{
    commands::AppState,
    composer_attachments::{
        copy_resolved_attachment_to_conversation, resolve_composer_attachments,
    },
    composer_quotes::copy_composer_quote_to_conversation,
};

const MAX_ATTACHMENTS: usize = 8;

/// Material resolved for the branch-scoped queue request. Source IDs are not
/// exposed after copying; all returned receipts and quote references belong
/// to the new conversation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CopiedBranchMaterial {
    pub attachments: Vec<ComposerAttachmentReceipt>,
    pub references: Vec<ComposerReference>,
}

pub(crate) async fn copy_branch_material(
    state: &AppState,
    project_id: Uuid,
    source_conversation_id: Uuid,
    branch_conversation_id: Uuid,
    source_attachment_ids: &[Uuid],
    source_references: &[ComposerReference],
) -> Result<CopiedBranchMaterial, String> {
    if source_attachment_ids.len() > MAX_ATTACHMENTS {
        return Err("branch attachments exceed the maximum of 8".into());
    }
    let mut attachment_ids = HashSet::with_capacity(source_attachment_ids.len());
    if source_attachment_ids
        .iter()
        .any(|id| id.is_nil() || !attachment_ids.insert(*id))
    {
        return Err("branch attachments must be unique non-nil IDs".into());
    }
    for reference in source_references {
        if let ComposerReference::Quote {
            project_id: reference_project,
            id,
        } = reference
        {
            if *reference_project != project_id || id.is_nil() {
                return Err("branch quote reference does not belong to its source project".into());
            }
        }
    }
    let branch = state
        .repository
        .get_conversation_branch_v4(project_id, branch_conversation_id)
        .await
        .map_err(|_| "branch material scope could not be confirmed".to_owned())?
        .ok_or_else(|| "branch material scope could not be confirmed".to_owned())?;
    if branch.source_conversation_id != source_conversation_id {
        return Err("branch material source does not match its branch relation".into());
    }
    let project = state
        .repository
        .get_project(project_id)
        .await
        .map_err(|_| "branch project could not be loaded".to_owned())?
        .ok_or_else(|| "branch project could not be loaded".to_owned())?;

    // Resolve every source attachment before writing any target directory, so
    // a missing or changed source cannot silently produce a partial queue
    // payload. Target copies remain safe to retry if a later write fails.
    let resolved = resolve_composer_attachments(
        &state.repository,
        project_id,
        source_conversation_id,
        source_attachment_ids,
    )
    .await?;
    let mut attachments = Vec::with_capacity(resolved.len());
    for source in &resolved {
        let target_id =
            stable_branch_material_id(branch_conversation_id, source.receipt.id, "attachment");
        attachments.push(copy_resolved_attachment_to_conversation(
            &project,
            source,
            branch_conversation_id,
            target_id,
        )?);
    }

    let mut references = source_references.to_vec();
    for reference in &mut references {
        let (reference_project, source_id) = match reference {
            ComposerReference::Quote { project_id, id } => (*project_id, *id),
            _ => continue,
        };
        let target_id = stable_branch_material_id(branch_conversation_id, source_id, "quote");
        let copied_id = copy_composer_quote_to_conversation(
            &state.repository,
            project_id,
            source_conversation_id,
            branch_conversation_id,
            source_id,
            target_id,
        )
        .await?;
        if copied_id != target_id {
            return Err("branch quote copy did not preserve its target identity".into());
        }
        *reference = ComposerReference::Quote {
            project_id: reference_project,
            id: copied_id,
        };
    }

    Ok(CopiedBranchMaterial {
        attachments,
        references,
    })
}

/// Map source material identities to the deterministic IDs used by the
/// branch-scoped queue. This contains no source reads, so a retry can verify
/// an already-persisted queue row before touching mutable source material.
pub(crate) fn branch_queue_references(
    branch_conversation_id: Uuid,
    source_references: &[ComposerReference],
) -> Vec<ComposerReference> {
    source_references
        .iter()
        .map(|reference| match reference {
            ComposerReference::Quote { project_id, id } => ComposerReference::Quote {
                project_id: *project_id,
                id: stable_branch_material_id(branch_conversation_id, *id, "quote"),
            },
            _ => reference.clone(),
        })
        .collect()
}

pub(crate) fn branch_queue_attachments(
    branch_conversation_id: Uuid,
    source_attachment_ids: &[Uuid],
) -> Vec<Uuid> {
    source_attachment_ids
        .iter()
        .map(|id| stable_branch_material_id(branch_conversation_id, *id, "attachment"))
        .collect()
}

/// Derive a collision-resistant UUID from the destination branch, source
/// material ID and material kind. UUID bits are marked as version 4/variant 1
/// so the result remains valid for all existing receipt/quote validators.
pub(crate) fn stable_branch_material_id(
    branch_conversation_id: Uuid,
    source_id: Uuid,
    kind: &str,
) -> Uuid {
    let mut hasher = Sha256::new();
    hasher.update(b"omicsops-branch-material-v4\0");
    hasher.update((kind.len() as u64).to_le_bytes());
    hasher.update(kind.as_bytes());
    hasher.update(branch_conversation_id.as_bytes());
    hasher.update(source_id.as_bytes());
    let digest = hasher.finalize();
    let mut bytes = [0_u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Uuid::from_bytes(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use omicsops_core::workspace::{Conversation, Project, ProjectTemplate};
    use omicsops_store::Store;
    use serde_json::json;
    use std::fs;
    use tempfile::tempdir;
    use uuid::Uuid;

    #[test]
    fn branch_material_ids_are_stable_and_namespace_separated() {
        let branch = Uuid::from_u128(1);
        let source = Uuid::from_u128(2);
        let attachment = stable_branch_material_id(branch, source, "attachment");
        let retry = stable_branch_material_id(branch, source, "attachment");
        let quote = stable_branch_material_id(branch, source, "quote");
        assert_eq!(attachment, retry);
        assert_ne!(attachment, quote);
        assert_ne!(attachment, Uuid::nil());
    }

    #[test]
    fn branch_queue_material_mapping_is_deterministic_without_source_reads() {
        let branch = Uuid::from_u128(10);
        let project = Uuid::from_u128(11);
        let quote = Uuid::from_u128(12);
        let references = vec![
            ComposerReference::Quote {
                project_id: project,
                id: quote,
            },
            ComposerReference::Skill {
                id: Uuid::from_u128(13),
            },
        ];
        let mapped = branch_queue_references(branch, &references);
        assert_eq!(
            mapped[0],
            ComposerReference::Quote {
                project_id: project,
                id: stable_branch_material_id(branch, quote, "quote"),
            }
        );
        assert_eq!(mapped[1], references[1]);
        assert_eq!(
            branch_queue_attachments(branch, &[Uuid::from_u128(14)]),
            vec![stable_branch_material_id(
                branch,
                Uuid::from_u128(14),
                "attachment"
            )]
        );
    }

    #[test]
    fn attachment_copy_is_idempotent_and_rewrites_scope() {
        let directory = tempdir().unwrap();
        let root = directory.path().join("project");
        fs::create_dir_all(&root).unwrap();
        let project = Project::new(
            Uuid::new_v4(),
            "branch material project",
            root.to_string_lossy(),
            ProjectTemplate::Blank,
            Utc::now(),
        );
        let source_conversation = Uuid::new_v4();
        let target_conversation = Uuid::new_v4();
        let source_id = Uuid::new_v4();
        let source_bytes = b"gene,value\nA,1\n".to_vec();
        let source = crate::composer_attachments::ResolvedComposerAttachment {
            receipt: ComposerAttachmentReceipt {
                id: source_id,
                project_id: project.id,
                conversation_id: source_conversation,
                name: "results.csv".into(),
                relative_path: format!(".omicsops/attachments/{source_id}/bytes.csv"),
                size_bytes: source_bytes.len() as u64,
                sha256: hex::encode(Sha256::digest(&source_bytes)),
                media_type: "text/csv".into(),
            },
            bytes: source_bytes,
        };
        let target_id = stable_branch_material_id(target_conversation, source_id, "attachment");
        let first = crate::composer_attachments::copy_resolved_attachment_to_conversation(
            &project,
            &source,
            target_conversation,
            target_id,
        )
        .unwrap();
        let retry = crate::composer_attachments::copy_resolved_attachment_to_conversation(
            &project,
            &source,
            target_conversation,
            target_id,
        )
        .unwrap();
        assert_eq!(first, retry);
        assert_eq!(first.conversation_id, target_conversation);
        assert_eq!(first.id, target_id);
    }

    #[tokio::test]
    async fn quote_copy_is_scoped_and_idempotent() {
        let store = Store::open_in_memory().await.unwrap();
        let project = Project::new(
            Uuid::new_v4(),
            "branch quote project",
            r"C:\data\branch-quote",
            ProjectTemplate::Blank,
            Utc::now(),
        );
        store.save_project(&project).await.unwrap();
        let source = Conversation::new(Uuid::new_v4(), project.id, "source", Utc::now());
        let target = Conversation::new(Uuid::new_v4(), project.id, "target", Utc::now());
        store.save_conversation(&source).await.unwrap();
        store.save_conversation(&target).await.unwrap();
        let source_id = Uuid::new_v4();
        store
            .put_json(
                "composer_quote_v1",
                &source_id.to_string(),
                &json!({
                    "id": source_id,
                    "project_id": project.id,
                    "conversation_id": source.id,
                    "backend_id": "local",
                    "relative_path": "results.csv",
                    "sha256": "a".repeat(64),
                    "text": "gene,value"
                }),
            )
            .await
            .unwrap();
        let target_id = stable_branch_material_id(target.id, source_id, "quote");
        let first = crate::composer_quotes::copy_composer_quote_to_conversation(
            &store, project.id, source.id, target.id, source_id, target_id,
        )
        .await
        .unwrap();
        let retry = crate::composer_quotes::copy_composer_quote_to_conversation(
            &store, project.id, source.id, target.id, source_id, target_id,
        )
        .await
        .unwrap();
        assert_eq!(first, retry);
        assert_eq!(
            store
                .list_json::<serde_json::Value>("composer_quote_v1")
                .await
                .unwrap()
                .len(),
            2
        );
        let duplicate_source_id = Uuid::new_v4();
        store
            .put_json(
                "composer_quote_v1",
                &duplicate_source_id.to_string(),
                &json!({
                    "id": duplicate_source_id,
                    "project_id": project.id,
                    "conversation_id": source.id,
                    "backend_id": "local",
                    "relative_path": "results.csv",
                    "sha256": "a".repeat(64),
                    "text": "gene,value"
                }),
            )
            .await
            .unwrap();
        let duplicate_target_id =
            stable_branch_material_id(target.id, duplicate_source_id, "quote");
        let duplicate = crate::composer_quotes::copy_composer_quote_to_conversation(
            &store,
            project.id,
            source.id,
            target.id,
            duplicate_source_id,
            duplicate_target_id,
        )
        .await
        .unwrap();
        assert_eq!(duplicate, duplicate_target_id);
        assert_eq!(
            store
                .list_json::<serde_json::Value>("composer_quote_v1")
                .await
                .unwrap()
                .len(),
            4
        );
        let copied = store
            .get_json::<serde_json::Value>("composer_quote_v1", &first.to_string())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(copied["conversation_id"], target.id.to_string());
    }
}
