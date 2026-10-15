//! Whitelisted metadata projection. No transcript, scientific JSON, profile, or file reads.
use crate::{Store, StoreError, project_from_row};
use chrono::SecondsFormat;
use omicsops_core::{
    redaction::public_text,
    workspace::{Project, ProjectTemplate},
};
use omicsops_dto::*;
use sqlx::{Row, SqliteConnection};
use uuid::Uuid;

/// Host-only context: deliberately not serializable. `project` is used for subsequent
/// validated file reads; only `public_project` may cross the UI boundary.
#[derive(Debug)]
pub struct MigrationMetadataReadContext {
    pub project: Project,
    pub public_project: MigrationProjectMetadataV1,
    pub categories: Vec<MigrationCategorySummaryV1>,
    pub scientific_revision: Option<u64>,
    pub scientific_kind_counts: Vec<(&'static str, u64)>,
    pub shelved_conversation_count: u64,
    pub rebind: Vec<MigrationRebindRequirementV1>,
    pub exclusions: Vec<String>,
}

// SQL fragments are exclusively host constants, never request/table input.
const SCIENCE: [(&str, &str, &str); 5] = [
    (
        "dataset",
        "scientific_datasets_v4",
        "CASE active WHEN 1 THEN 'active' ELSE 'inactive' END",
    ),
    (
        "analysis",
        "scientific_analyses_v4",
        "CASE status WHEN 'running' THEN 'running' WHEN 'succeeded' THEN 'succeeded' WHEN 'failed' THEN 'failed' WHEN 'cancelled' THEN 'cancelled' WHEN 'stale' THEN 'stale' ELSE 'unknown' END",
    ),
    (
        "artifact",
        "scientific_artifacts_v4",
        "CASE valid WHEN 1 THEN 'valid' ELSE 'invalid' END",
    ),
    (
        "evidence",
        "scientific_evidence_v4",
        "CASE valid WHEN 1 THEN 'valid' ELSE 'invalid' END",
    ),
    (
        "provenance",
        "scientific_provenance_v4",
        "CASE complete WHEN 1 THEN 'complete' ELSE 'incomplete' END",
    ),
];

impl Store {
    /// A single deferred SQLite read transaction; later file reads are not part of
    /// this observation. Every statement here is an explicit-column SELECT.
    pub async fn workspace_project_migration_metadata(
        &self,
        project_id: Uuid,
        categories: &[MigrationMetadataCategoryV1],
    ) -> Result<MigrationMetadataReadContext, StoreError> {
        let mut tx = self.pool.begin().await?;
        let owner = project_id.to_string();
        let row=sqlx::query("SELECT p.id,p.name,p.description,p.workspace_dir,p.created_at,p.updated_at,e.local_root,e.remote_root,e.connection_id,e.template,e.status,e.ollama_only FROM projects p LEFT JOIN project_omicsops e ON e.project_id=p.id WHERE p.id=?")
            .bind(&owner).fetch_optional(&mut *tx).await?
            .ok_or_else(||StoreError::InvalidInput("migration_preview_not_found".into()))?;
        let project = project_from_row(row)?;
        let mut exclusions=vec![
            "Raw large samples and unselected files are excluded.".into(),
            "Credentials and authorizations are not transferred.".into(),
            "Caches, logs, and the application database are excluded.".into(),
            "Run, approval, connection, session, and job recovery identities are not transferred.".into(),
            "Preview only; export and import have not been performed.".into(),
            "Database metadata and subsequent file observations are not an atomic project snapshot.".into(),
        ];
        let name = project_text(&project.name, 800, &mut exclusions);
        let description = project_text(&project.description, 4096, &mut exclusions);
        let public_project = MigrationProjectMetadataV1 {
            source_project_id: project.id,
            name,
            description,
            template: match project.template {
                ProjectTemplate::Blank => "blank",
                ProjectTemplate::SingleCellRnaSeq => "single_cell_rna_seq",
                ProjectTemplate::BulkRnaSeq => "bulk_rna_seq",
                ProjectTemplate::LiteratureReview => "literature_review",
            }
            .into(),
            created_at: project
                .created_at
                .to_rfc3339_opts(SecondsFormat::Millis, true),
        };
        let conversation_count = count(
            &mut tx,
            "SELECT COUNT(*) FROM conversation_records WHERE project_id=?",
            &owner,
        )
        .await?;
        let publication_count = count(
            &mut tx,
            "SELECT COUNT(*) FROM publications WHERE project_id=?",
            &owner,
        )
        .await?;
        let shelved_conversation_count=count(&mut tx,"SELECT COUNT(*) FROM conversation_records c JOIN conversation_visibility_v4 v ON v.conversation_id=c.frame_id AND v.project_id=c.project_id WHERE c.project_id=? AND v.visibility='shelved'",&owner).await?;
        exclusions.push(format!("All conversations are included in the category count; they include {shelved_conversation_count} shelved records. Legacy archived records are included independently of visibility."));
        let header: Option<i64> =
            sqlx::query_scalar("SELECT revision FROM scientific_states_v4 WHERE project_id=?")
                .bind(&owner)
                .fetch_optional(&mut *tx)
                .await?;
        let scientific_revision = header.map(nonnegative).transpose()?;
        let mut scientific_kind_counts = Vec::new();
        for (kind, table, _) in SCIENCE {
            scientific_kind_counts.push((
                kind,
                count(
                    &mut tx,
                    &format!("SELECT COUNT(*) FROM {table} WHERE project_id=?"),
                    &owner,
                )
                .await?,
            ));
        }
        let scientific_count = scientific_kind_counts.iter().map(|(_, n)| n).sum();
        let mut summaries = Vec::new();
        for (category, record_count) in [
            (
                MigrationMetadataCategoryV1::Conversations,
                conversation_count,
            ),
            (MigrationMetadataCategoryV1::Publications, publication_count),
            (
                MigrationMetadataCategoryV1::ScientificMetadata,
                scientific_count,
            ),
        ] {
            let selected = categories.contains(&category);
            let mut examples = Vec::new();
            if selected {
                match category {
                    MigrationMetadataCategoryV1::Conversations => {
                        let rows=sqlx::query("SELECT frame_id,title,CASE status WHEN 'idle' THEN 'idle' WHEN 'running' THEN 'running' WHEN 'waiting_for_input' THEN 'waiting_for_input' WHEN 'needs_attention' THEN 'needs_attention' WHEN 'completed' THEN 'completed' WHEN 'archived' THEN 'archived' ELSE 'unknown' END AS safe_status FROM conversation_records WHERE project_id=? ORDER BY updated_at DESC,frame_id ASC LIMIT 20").bind(&owner).fetch_all(&mut *tx).await?;
                        for row in rows {
                            examples.push(MigrationRecordSummaryV1 {
                                source_id: row.try_get("frame_id")?,
                                kind: "conversation".into(),
                                label: summary_text(row.try_get("title")?, &mut exclusions),
                                status: row.try_get("safe_status")?,
                                revision: None,
                            });
                        }
                    }
                    MigrationMetadataCategoryV1::Publications => {
                        let rows=sqlx::query("SELECT p.id,p.title,CASE p.status WHEN 'draft' THEN 'draft' WHEN 'ready' THEN 'ready' WHEN 'frozen' THEN 'frozen' WHEN 'published' THEN 'published' WHEN 'archived' THEN 'archived' ELSE 'unknown' END AS safe_status,r.revision,r.content_sha256 FROM publications p LEFT JOIN publication_revisions r ON r.publication_id=p.id AND r.revision=(SELECT MAX(pr.revision) FROM publication_revisions pr WHERE pr.publication_id=p.id) WHERE p.project_id=? ORDER BY p.updated_at DESC,p.id ASC LIMIT 20").bind(&owner).fetch_all(&mut *tx).await?;
                        for row in rows {
                            let mut label = summary_text(row.try_get("title")?, &mut exclusions);
                            if let Some(hash) = row
                                .try_get::<Option<String>, _>("content_sha256")?
                                .filter(|hash| {
                                    hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit())
                                })
                            {
                                label
                                    .push_str(&format!(" [sha256: {}]", hash.to_ascii_lowercase()));
                            }
                            examples.push(MigrationRecordSummaryV1 {
                                source_id: row.try_get("id")?,
                                kind: "publication".into(),
                                label,
                                status: row.try_get("safe_status")?,
                                revision: row
                                    .try_get::<Option<i64>, _>("revision")?
                                    .map(nonnegative)
                                    .transpose()?,
                            });
                        }
                    }
                    MigrationMetadataCategoryV1::ScientificMetadata => {
                        let mut per_kind = Vec::new();
                        for (kind, table, status) in SCIENCE {
                            let rows=sqlx::query(&format!("SELECT id,{status} AS safe_status FROM {table} WHERE project_id=? ORDER BY id ASC LIMIT 20")).bind(&owner).fetch_all(&mut *tx).await?;
                            let values = rows
                                .into_iter()
                                .map(|row| {
                                    Ok(MigrationRecordSummaryV1 {
                                        source_id: row.try_get("id")?,
                                        kind: kind.into(),
                                        label: match kind {
                                            "dataset" => "Dataset",
                                            "analysis" => "Analysis",
                                            "artifact" => "Artifact",
                                            "evidence" => "Evidence",
                                            _ => "Provenance",
                                        }
                                        .into(),
                                        status: row.try_get("safe_status")?,
                                        revision: None,
                                    })
                                })
                                .collect::<Result<Vec<_>, StoreError>>()?;
                            per_kind.push(values.into_iter());
                        }
                        // Round-robin represents all nonempty kinds within the common cap.
                        for _ in 0..20 {
                            for values in &mut per_kind {
                                if examples.len() < 20 {
                                    if let Some(value) = values.next() {
                                        examples.push(value);
                                    }
                                }
                            }
                        }
                    }
                }
            }
            let examples_truncated = selected && record_count > examples.len() as u64;
            let kind_counts = if category == MigrationMetadataCategoryV1::ScientificMetadata {
                scientific_kind_counts
                    .iter()
                    .map(|(kind, count)| MigrationKindCountV1 {
                        kind: (*kind).into(),
                        record_count: *count,
                    })
                    .collect()
            } else {
                Vec::new()
            };
            summaries.push(MigrationCategorySummaryV1 {
                category,
                selected,
                record_count,
                kind_counts,
                examples,
                examples_truncated,
            });
        }
        let model_count=count(&mut tx,"SELECT COUNT(DISTINCT model_profile_id) FROM conversation_records WHERE project_id=? AND model_profile_id IS NOT NULL",&owner).await?;
        let kernel_count = count(
            &mut tx,
            "SELECT COUNT(*) FROM execution_contexts WHERE project_id=?",
            &owner,
        )
        .await?;
        let job_count=count(&mut tx,"SELECT COUNT(*) FROM runtime_jobs_v4 j JOIN agent_runs_v4 r ON r.run_id=j.run_id WHERE r.project_id=?",&owner).await?;
        let mcp_count = nonnegative(
            sqlx::query_scalar("SELECT COUNT(*) FROM app_objects WHERE kind='mcp_server'")
                .fetch_one(&mut *tx)
                .await?,
        )?;
        let skill_count = nonnegative(
            sqlx::query_scalar("SELECT COUNT(*) FROM skill_packages")
                .fetch_one(&mut *tx)
                .await?,
        )?;
        let rebind = vec![
            rebind(
                MigrationRebindKindV1::ProjectPaths,
                1 + u64::from(project.remote_root.is_some()),
                "Select local and remote roots again in the target project.",
            ),
            rebind(
                MigrationRebindKindV1::SshConnection,
                u64::from(project.connection_id.is_some()),
                "Configure SSH connections, keys, and host trust again in the target environment.",
            ),
            rebind(
                MigrationRebindKindV1::ModelProfile,
                model_count,
                "Model credentials and endpoints are not transferred; select and configure model profiles again.",
            ),
            rebind(
                MigrationRebindKindV1::KernelEnvironment,
                kernel_count,
                "Probe kernel environment requirements again; finding an interpreter does not establish dependency availability.",
            ),
            rebind(
                MigrationRebindKindV1::McpServer,
                mcp_count,
                "Count of application-global registered MCP configurations, not project grants or enabled/authorized capabilities; reinstall and authorize again.",
            ),
            rebind(
                MigrationRebindKindV1::SkillGrant,
                skill_count,
                "Count of application-global registered Skill packages, not project grants or enabled/authorized capabilities; reinstall and authorize again.",
            ),
            rebind(
                MigrationRebindKindV1::RuntimeJobs,
                job_count,
                "Run, approval, connection, session, and background job recovery identities are not imported as executable state. Running jobs continue in the source project; preview does not cancel, reconnect, or redispatch them.",
            ),
        ];
        tx.rollback().await?;
        Ok(MigrationMetadataReadContext {
            project,
            public_project,
            categories: summaries,
            scientific_revision,
            scientific_kind_counts,
            shelved_conversation_count,
            rebind,
            exclusions,
        })
    }
}

async fn count(conn: &mut SqliteConnection, sql: &str, owner: &str) -> Result<u64, StoreError> {
    nonnegative(sqlx::query_scalar(sql).bind(owner).fetch_one(conn).await?)
}
fn nonnegative(value: i64) -> Result<u64, StoreError> {
    u64::try_from(value)
        .map_err(|_| StoreError::InvalidInput("migration_preview_invalid_count".into()))
}
fn rebind(kind: MigrationRebindKindV1, count: u64, message: &str) -> MigrationRebindRequirementV1 {
    MigrationRebindRequirementV1 {
        kind,
        count,
        message: message.into(),
    }
}
fn bounded_public(value: &str, limit: usize) -> String {
    let mut value = public_text(value);
    let mut end = value.len().min(limit);
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    value.truncate(end);
    value
}
fn project_text(value: &str, limit: usize, exclusions: &mut Vec<String>) -> String {
    let safe = public_text(value);
    if safe != value
        && !exclusions
            .iter()
            .any(|e| e == "Project public metadata was redacted.")
    {
        exclusions.push("Project public metadata was redacted.".into());
    }
    if safe.len() > limit
        && !exclusions
            .iter()
            .any(|e| e == "Project public metadata was truncated at a UTF-8 boundary.")
    {
        exclusions.push("Project public metadata was truncated at a UTF-8 boundary.".into());
    }
    bounded_public(&safe, limit)
}
fn summary_text(value: &str, exclusions: &mut Vec<String>) -> String {
    let safe = bounded_public(value, 800);
    if safe != value
        && !exclusions
            .iter()
            .any(|e| e == "Selected record labels were redacted or truncated.")
    {
        exclusions.push("Selected record labels were redacted or truncated.".into());
    }
    safe
}
