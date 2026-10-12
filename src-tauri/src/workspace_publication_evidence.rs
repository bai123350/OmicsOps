//! Read-only inspection of one immutable, project-owned publication revision.

use omicsops_dto::*;
use omicsops_store::Store;
use omicsops_store::{StoreError, workspace_navigation::PublicationReadContext};
use serde_json::Value;
use sha2::Digest;
use tauri::State;
use uuid::Uuid;

use crate::{
    commands::AppState,
    composer_references::public_text,
    workspace_navigation::{
        SourceError, message_snapshot, record_run_id, record_snapshot, source_availability,
        tool_snapshot,
    },
};

const MAX_FINDINGS: usize = 128;
const MAX_REPORT_BYTES: usize = 512 * 1024;

fn error(code: PublicationEvidenceErrorCodeV1) -> PublicationEvidenceErrorV1 {
    let message = match code {
        PublicationEvidenceErrorCodeV1::InvalidSelection => "Invalid saved publication selection",
        PublicationEvidenceErrorCodeV1::NotFound => {
            "Saved publication revision not found in project"
        }
        PublicationEvidenceErrorCodeV1::RevisionChanged => {
            "Saved publication revision hash changed"
        }
        PublicationEvidenceErrorCodeV1::LimitExceeded => {
            "Publication inspection exceeds safe limits"
        }
        PublicationEvidenceErrorCodeV1::UnsafePublicContent => {
            "Publication contains unsafe public content"
        }
        _ => "Publication evidence could not be read",
    };
    PublicationEvidenceErrorV1 {
        code,
        message: message.into(),
    }
}

fn valid_hash(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// Existing redaction plus an explicit authentication-URI check. No credential reads.
pub(crate) fn ensure_safe_public_text(value: &str) -> Result<(), PublicationEvidenceErrorV1> {
    let authentication_uri = value.match_indices("://").any(|(start, _)| {
        value[start + 3..]
            .split(['/', '?', '#', '\\', '"', '\'', ' ', '\n', '\r', '\t'])
            .next()
            .is_some_and(|authority| authority.contains('@'))
    });
    // The established redactor stops unquoted assignments at `]` and can append
    // another `]` to an existing placeholder. Accept only this harmless known
    // transformation, without altering the legacy/public checksum semantics.
    fn placeholders(value: &str) -> String {
        let mut result = value.to_owned();
        while result.contains("[REDACTED]]") {
            result = result.replace("[REDACTED]]", "[REDACTED]");
        }
        result
    }
    if placeholders(&public_text(value)) != placeholders(value) || authentication_uri {
        return Err(error(PublicationEvidenceErrorCodeV1::UnsafePublicContent));
    }
    Ok(())
}

fn storage_error(value: StoreError) -> PublicationEvidenceErrorV1 {
    // Only fixed, known Store reasons select public codes. Never expose SQL/paths.
    let code = match value {
        StoreError::InvalidInput(reason)
            if matches!(
                reason.as_str(),
                "publication not found in project"
                    | "publication revision not found"
                    | "project not found"
            ) =>
        {
            PublicationEvidenceErrorCodeV1::NotFound
        }
        StoreError::InvalidInput(reason)
            if matches!(
                reason.as_str(),
                "publication revision exceeds safe size"
                    | "publication references exceed safe limit"
            ) =>
        {
            PublicationEvidenceErrorCodeV1::LimitExceeded
        }
        _ => PublicationEvidenceErrorCodeV1::StorageReadFailed,
    };
    error(code)
}

#[tauri::command]
pub async fn workspace_check_publication_readiness(
    state: State<'_, AppState>,
    request: PublicationSelectionV1,
) -> Result<PublicationReadinessReportV1, PublicationEvidenceErrorV1> {
    check_publication_readiness(
        &state.repository,
        &request,
        &chrono::Utc::now().to_rfc3339(),
    )
    .await
}

/// The only effectful dependency is the bounded, read-only Store observation.
pub(crate) async fn check_publication_readiness(
    repository: &Store,
    request: &PublicationSelectionV1,
    observed_at: &str,
) -> Result<PublicationReadinessReportV1, PublicationEvidenceErrorV1> {
    validate_selection(request)?;
    let context = repository
        .workspace_publication_read_context(
            request.project_id,
            request.publication_id,
            request.revision,
        )
        .await
        .map_err(storage_error)?;
    publication_report_from_context(&context, request, observed_at)
}

fn validate_selection(request: &PublicationSelectionV1) -> Result<(), PublicationEvidenceErrorV1> {
    if request.project_id.is_nil()
        || request.publication_id.is_nil()
        || request.revision == 0
        || request.revision > i64::MAX as u64
        || !valid_hash(&request.expected_revision_sha256)
    {
        return Err(error(PublicationEvidenceErrorCodeV1::InvalidSelection));
    }
    Ok(())
}

pub(crate) fn publication_report_from_context(
    context: &PublicationReadContext,
    request: &PublicationSelectionV1,
    observed_at: &str,
) -> Result<PublicationReadinessReportV1, PublicationEvidenceErrorV1> {
    validate_selection(request)?;
    let date = chrono::DateTime::parse_from_rfc3339(observed_at)
        .map_err(|_| error(PublicationEvidenceErrorCodeV1::InvalidSelection))?;
    if date.offset().local_minus_utc() != 0 {
        return Err(error(PublicationEvidenceErrorCodeV1::InvalidSelection));
    }
    let revision = &context.revision;
    if revision.publication_id != request.publication_id || revision.revision != request.revision {
        return Err(error(PublicationEvidenceErrorCodeV1::NotFound));
    }
    let (hash, scope) = if revision.legacy {
        (
            hex::encode(sha2::Sha256::digest(
                public_text(&revision.markdown).as_bytes(),
            )),
            PublicationRevisionHashScopeV1::PublicLegacyMarkdown,
        )
    } else {
        (
            revision.sha256.clone(),
            PublicationRevisionHashScopeV1::StoredRevisionContent,
        )
    };
    if hash != request.expected_revision_sha256 {
        return Err(error(PublicationEvidenceErrorCodeV1::RevisionChanged));
    }
    ensure_safe_public_text(&public_text(&revision.title))?;
    ensure_safe_public_text(&public_text(&revision.markdown))?;
    if !revision.legacy {
        ensure_safe_public_text(&revision.markdown)?;
    }
    if revision.references.len() != context.current_sources.len()
        || revision.references.len() > 32
        || context.related_records.len() > 64
        || revision
            .references
            .iter()
            .zip(&context.current_sources)
            .any(|(saved, current)| saved.source != current.source)
    {
        return Err(error(PublicationEvidenceErrorCodeV1::StorageReadFailed));
    }
    for saved in &revision.references {
        if saved.source.project_id != request.project_id
            || saved.source.id.is_empty()
            || saved.source.id.len() > 512
        {
            return Err(error(PublicationEvidenceErrorCodeV1::UnsafePublicContent));
        }
        // Check original strings before JSON escaping can hide assignment
        // delimiters. Findings retain these source fields verbatim.
        for value in std::iter::once(saved.source.id.as_str())
            .chain(saved.source.event_hash.as_deref())
            .chain(saved.source.content_sha256.as_deref())
        {
            ensure_safe_public_text(value)?;
        }
        let encoded = serde_json::to_string(&saved.source)
            .map_err(|_| error(PublicationEvidenceErrorCodeV1::UnsafePublicContent))?;
        ensure_safe_public_text(&encoded)?;
        ensure_safe_public_text(&saved.text)?;
        ensure_safe_public_text(&saved.title)?;
    }
    if context
        .scientific_state_sha256
        .as_deref()
        .is_some_and(|hash| !valid_hash(hash))
    {
        return Err(error(PublicationEvidenceErrorCodeV1::StorageReadFailed));
    }
    let mut report = PublicationReadinessReportV1 {
        schema_version: 1,
        selection: request.clone(),
        revision_hash_scope: scope,
        observed_at: observed_at.into(),
        scientific_revision: context.scientific_revision,
        scientific_state_sha256: context.scientific_state_sha256.clone(),
        references_checked: 0,
        related_records_checked: 0,
        attention_count: 0,
        unverified_count: 0,
        findings: vec![],
        truncated: context.truncated,
        reproduction_status: ReproductionStatusV1::NotExecuted,
    };
    if revision.legacy {
        finding(
            &mut report,
            ReadinessFindingCodeV1::LegacyRevision,
            None,
            None,
        );
    }
    for (saved, current) in revision.references.iter().zip(&context.current_sources) {
        report.references_checked += 1;
        let availability = if let Some(availability) = current.availability {
            availability
        } else {
            let projection = current_projection(context, &saved.source, current.value.as_ref());
            if matches!(
                &projection,
                Err(SourceError::Invalid("snapshot exceeds 256 KiB"))
            ) {
                report.truncated = true;
            }
            source_availability(saved, &projection)
        };
        let code = match availability {
            SourceAvailability::Available => None,
            SourceAvailability::Changed => Some(ReadinessFindingCodeV1::SourceChanged),
            SourceAvailability::Missing => Some(ReadinessFindingCodeV1::SourceMissing),
            SourceAvailability::Unavailable => Some(ReadinessFindingCodeV1::SourceUnavailable),
        };
        if let Some(code) = code {
            finding(&mut report, code, Some(&saved.source), None);
        }
        if matches!(
            saved.source.kind,
            SourceKind::Artifact | SourceKind::LegacyArtifact | SourceKind::Dataset
        ) {
            finding(
                &mut report,
                ReadinessFindingCodeV1::OriginalFileNotChecked,
                Some(&saved.source),
                None,
            );
            finding(
                &mut report,
                ReadinessFindingCodeV1::HistoricalLocationUnverified,
                Some(&saved.source),
                None,
            );
        }
    }
    for record in &context.related_records {
        report.related_records_checked += 1;
        classify_record(&mut report, context, record);
    }
    if report.truncated {
        // Reserve one slot for the limit disclosure even if classification filled the budget.
        if report.findings.len() >= MAX_FINDINGS {
            report.findings.truncate(MAX_FINDINGS - 1);
        }
        finding(
            &mut report,
            ReadinessFindingCodeV1::InspectionLimit,
            None,
            None,
        );
    }
    report.attention_count = report
        .findings
        .iter()
        .filter(|f| f.severity == ReadinessFindingSeverityV1::Attention)
        .count() as u32;
    report.unverified_count = report
        .findings
        .iter()
        .filter(|f| f.severity == ReadinessFindingSeverityV1::Unverified)
        .count() as u32;
    let bytes = serde_json::to_vec(&report)
        .map_err(|_| error(PublicationEvidenceErrorCodeV1::StorageReadFailed))?;
    if bytes.len() > MAX_REPORT_BYTES {
        return Err(error(PublicationEvidenceErrorCodeV1::LimitExceeded));
    }
    Ok(report)
}

fn current_projection(
    context: &PublicationReadContext,
    source: &WorkspaceSourceRef,
    value: Option<&Value>,
) -> Result<WorkspaceSourceSnapshot, SourceError> {
    let value = value.ok_or(SourceError::Unavailable)?;
    match source.kind {
        SourceKind::Message => message_snapshot(
            source,
            &serde_json::from_value(value.clone()).map_err(|_| SourceError::Unavailable)?,
        ),
        SourceKind::Tool => tool_snapshot(
            source,
            &serde_json::from_value(value.clone()).map_err(|_| SourceError::Unavailable)?,
        ),
        _ => {
            if source.start.is_some()
                || source.end.is_some()
                || source.content_sha256.is_some()
                || source.sequence.is_some()
                || source.event_hash.is_some()
            {
                return Err(SourceError::Unavailable);
            }
            let producer = value
                .get("producer_analysis_id")
                .and_then(Value::as_str)
                .and_then(|id| {
                    context.related_records.iter().find(|record| {
                        record.kind == SourceKind::Analysis && record.id.to_string() == id
                    })
                })
                .map(|record| &record.value);
            if source.kind == SourceKind::Artifact && producer.is_none() && context.truncated {
                return Err(SourceError::Unavailable);
            }
            let run = record_run_id(source.kind, value, producer);
            let conversation =
                run.and_then(|run| context.run_conversations.get(&run).copied().flatten());
            if run.is_some_and(|run| !context.run_conversations.contains_key(&run)) {
                return Err(SourceError::Unavailable);
            }
            record_snapshot(source, value.clone(), producer, conversation, false)
        }
    }
}

fn related<'a>(
    context: &'a PublicationReadContext,
    kind: SourceKind,
    id: &str,
) -> Option<&'a Value> {
    context
        .related_records
        .iter()
        .find(|record| record.kind == kind && record.id.to_string() == id)
        .map(|record| &record.value)
}
fn text<'a>(value: &'a Value, key: &str) -> &'a str {
    value.get(key).and_then(Value::as_str).unwrap_or("")
}

fn classify_record(
    report: &mut PublicationReadinessReportV1,
    context: &PublicationReadContext,
    record: &omicsops_store::workspace_navigation::PublicationRelatedRecord,
) {
    use ReadinessFindingCodeV1 as C;
    let value = &record.value;
    let id = Some(record.id);
    if record.run_owner_verified == Some(false) {
        finding(report, C::InvalidInput, None, id);
        finding(report, C::LineageNotRecorded, None, id);
    }
    match record.kind {
        SourceKind::Evidence => {
            if value["valid"] == false {
                finding(report, C::InvalidEvidence, None, id);
            }
            let sources = value["sources"].as_array();
            if sources.is_none_or(|sources| sources.is_empty()) {
                finding(report, C::LineageNotRecorded, None, id);
            }
            if let Some(sources) = sources {
                for source in sources {
                    if text(source, "kind") == "artifact"
                        && related(context, SourceKind::Artifact, text(source, "artifact_id"))
                            .is_none()
                        && !context.truncated
                    {
                        finding(report, C::LineageNotRecorded, None, id);
                    }
                }
            }
        }
        SourceKind::Artifact => {
            if value["valid"] == false {
                finding(report, C::InvalidArtifact, None, id);
            }
            finding(report, C::OriginalFileNotChecked, None, id);
            finding(report, C::HistoricalLocationUnverified, None, id);
            if related(
                context,
                SourceKind::Analysis,
                text(value, "producer_analysis_id"),
            )
            .is_none()
                && !context.truncated
            {
                finding(report, C::LineageNotRecorded, None, id);
            }
        }
        SourceKind::Analysis => {
            if text(value, "status") == "stale" {
                finding(report, C::StaleAnalysis, None, id);
            }
            if value.get("random_seed").is_none_or(Value::is_null) {
                finding(report, C::MissingRandomSeed, None, id);
            }
            let manifests: Vec<_> = context
                .related_records
                .iter()
                .filter(|m| {
                    m.kind == SourceKind::Provenance
                        && text(&m.value, "analysis_id") == record.id.to_string()
                        && text(&m.value, "run_id") == text(value, "run_id")
                        && m.run_owner_verified == Some(true)
                })
                .collect();
            if manifests.is_empty() && !context.truncated {
                finding(report, C::ProvenanceNotRecorded, None, id);
            }
            if let Some(requirements) = value["software_requirements"].as_array() {
                for requirement in requirements.iter().filter_map(Value::as_str) {
                    if !manifests.iter().any(|m| {
                        m.value["software_versions"]
                            .get(requirement)
                            .and_then(Value::as_str)
                            .is_some_and(|version| !version.trim().is_empty())
                    }) {
                        finding(report, C::MissingSoftwareVersion, None, id);
                    }
                }
            }
            if let Some(versions) = value["database_versions"].as_object() {
                if versions.values().any(|version| {
                    version
                        .as_str()
                        .is_none_or(|version| version.trim().is_empty())
                }) {
                    finding(report, C::MissingDatabaseVersion, None, id);
                }
            }
            if let Some(inputs) = value["input_dataset_ids"].as_array() {
                for input in inputs.iter().filter_map(Value::as_str) {
                    if let Some(dataset) = related(context, SourceKind::Dataset, input) {
                        if dataset["active"] == false {
                            finding(report, C::InvalidInput, None, id);
                        }
                        if let Some(samples) = dataset["sample_ids"].as_array() {
                            if samples.iter().any(|sample| {
                                !value["sample_ids"]
                                    .as_array()
                                    .is_some_and(|declared| declared.contains(sample))
                            }) {
                                finding(report, C::MissingSample, None, id);
                            }
                        }
                    } else if !context.truncated {
                        finding(report, C::InvalidInput, None, id);
                        finding(report, C::LineageNotRecorded, None, id);
                    }
                }
            }
            recorded_issues(report, &value["validation_issues"], record.id);
        }
        SourceKind::Provenance => {
            if value["complete"] == false {
                finding(report, C::IncompleteProvenance, None, id);
            }
            if related(context, SourceKind::Analysis, text(value, "analysis_id")).is_none()
                && !context.truncated
            {
                finding(report, C::LineageNotRecorded, None, id);
            }
            recorded_issues(report, &value["issues"], record.id);
        }
        _ => {}
    }
}

fn recorded_issues(report: &mut PublicationReadinessReportV1, issues: &Value, id: Uuid) {
    use ReadinessFindingCodeV1 as C;
    if let Some(issues) = issues.as_array() {
        for issue in issues {
            let code = match text(issue, "kind") {
                "missing_random_seed" => C::MissingRandomSeed,
                "missing_software_version" => C::MissingSoftwareVersion,
                "missing_database_version" => C::MissingDatabaseVersion,
                "missing_sample" => C::MissingSample,
                "invalid_input" => C::InvalidInput,
                _ => C::InvalidInput,
            };
            // Issue messages are arbitrary recorded text, never public findings.
            finding(report, code, None, Some(id));
        }
    }
}

fn finding(
    report: &mut PublicationReadinessReportV1,
    code: ReadinessFindingCodeV1,
    source: Option<&WorkspaceSourceRef>,
    related_record_id: Option<Uuid>,
) {
    use ReadinessFindingCodeV1 as C;
    use ReadinessFindingSeverityV1 as S;
    if report.findings.iter().any(|f| {
        f.code == code && f.source.as_ref() == source && f.related_record_id == related_record_id
    }) {
        return;
    }
    if report.findings.len() >= MAX_FINDINGS {
        report.truncated = true;
        return;
    }
    let (severity, message) = match code {
        C::SourceChanged => (
            S::Attention,
            "Current source differs from the saved public snapshot",
        ),
        C::SourceMissing => (
            S::Unverified,
            "Current source record was not found; saved snapshot is retained",
        ),
        C::SourceUnavailable => (
            S::Unverified,
            "Current source could not be verified; saved snapshot is retained",
        ),
        C::InvalidEvidence => (S::Attention, "Recorded evidence is invalid"),
        C::InvalidArtifact => (S::Attention, "Recorded artifact is invalid"),
        C::StaleAnalysis => (S::Attention, "Recorded analysis is stale"),
        C::IncompleteProvenance => (S::Attention, "Recorded provenance is incomplete"),
        C::MissingRandomSeed => (S::Attention, "Analysis random seed is not recorded"),
        C::MissingSoftwareVersion => (S::Attention, "A required software version is not recorded"),
        C::MissingDatabaseVersion => (S::Attention, "A required database version is not recorded"),
        C::MissingSample => (S::Attention, "Required sample information is missing"),
        C::InvalidInput => (
            S::Attention,
            "Recorded input or run association is invalid or unverified",
        ),
        C::ProvenanceNotRecorded => (S::Unverified, "No matching recorded provenance was found"),
        C::LineageNotRecorded => (
            S::Unverified,
            "Explicit recorded lineage could not be verified",
        ),
        C::OriginalFileNotChecked => (
            S::Unverified,
            "Original file was not opened or hashed; recorded hashes are historical",
        ),
        C::HistoricalLocationUnverified => (
            S::Unverified,
            "Historical file backend and root were not verified",
        ),
        C::LegacyRevision => (S::Info, "Legacy revision hash describes public Markdown"),
        C::InspectionLimit => (
            S::Unverified,
            "Inspection was truncated by a record, byte, depth or finding limit",
        ),
    };
    report.findings.push(ReadinessFindingV1 {
        code,
        severity,
        source: source.cloned(),
        related_record_id,
        message: message.into(),
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use omicsops_core::workspace::{Conversation, Project, ProjectTemplate};
    use omicsops_store::workspace_navigation::SavePublicationRecord;
    use serde_json::{Value, json};

    const OBSERVED: &str = "2026-10-12T00:00:00Z";

    async fn fixture() -> (Store, Uuid, Uuid, Uuid) {
        let store = Store::open_in_memory().await.unwrap();
        let p = Uuid::new_v4();
        let c = Uuid::new_v4();
        let r = Uuid::new_v4();
        store
            .save_project(&Project::new(
                p,
                "Synthetic",
                r"C:\does-not-exist",
                ProjectTemplate::Blank,
                Utc::now(),
            ))
            .await
            .unwrap();
        store
            .save_conversation(&Conversation::new(c, p, "Recorded", Utc::now()))
            .await
            .unwrap();
        store
            .save_agent_run_v4(r, p, c, "needs_attention", &json!({}))
            .await
            .unwrap();
        (store, p, c, r)
    }

    fn source(p: Uuid, kind: SourceKind, id: Uuid) -> WorkspaceSourceRef {
        WorkspaceSourceRef {
            project_id: p,
            kind,
            id: id.to_string(),
            conversation_id: None,
            run_id: None,
            sequence: None,
            event_hash: None,
            content_sha256: None,
            start: None,
            end: None,
        }
    }

    async fn save(
        store: &Store,
        p: Uuid,
        references: Vec<WorkspaceSourceSnapshot>,
    ) -> PublicationSelectionV1 {
        let detail = store
            .workspace_save_publication(&SavePublicationRecord {
                request: SavePublicationRequest {
                    request_id: Uuid::new_v4(),
                    project_id: p,
                    publication_id: None,
                    expected_revision: 0,
                    title: "Saved study".into(),
                    markdown: "Recorded observations".into(),
                    sources: references.iter().map(|s| s.source.clone()).collect(),
                },
                references,
            })
            .await
            .unwrap();
        PublicationSelectionV1 {
            project_id: p,
            publication_id: detail.publication.id,
            revision: 1,
            expected_revision_sha256: detail.revisions[0].sha256.clone(),
        }
    }

    fn saved(source: WorkspaceSourceRef) -> WorkspaceSourceSnapshot {
        WorkspaceSourceSnapshot {
            source,
            title: "Saved".into(),
            text: "Saved snapshot".into(),
            sha256: String::new(),
            status: "recorded".into(),
            metadata: Default::default(),
            availability: SourceAvailability::Available,
        }
    }

    async fn insert(store: &Store, kind: SourceKind, value: &Value) {
        let table = match kind {
            SourceKind::Evidence => "scientific_evidence_v4",
            SourceKind::Artifact => "scientific_artifacts_v4",
            SourceKind::Analysis => "scientific_analyses_v4",
            SourceKind::Dataset => "scientific_datasets_v4",
            SourceKind::Provenance => "scientific_provenance_v4",
            _ => panic!("fixture kind"),
        };
        let extra = match kind {
            SourceKind::Evidence => ",valid",
            SourceKind::Artifact => ",producer_analysis_id,valid",
            SourceKind::Analysis => ",status",
            SourceKind::Dataset => ",active",
            SourceKind::Provenance => ",run_id,analysis_id,complete",
            _ => unreachable!(),
        };
        let placeholders = match kind {
            SourceKind::Evidence | SourceKind::Analysis | SourceKind::Dataset => ",?",
            SourceKind::Artifact => ",?,?",
            SourceKind::Provenance => ",?,?,?",
            _ => unreachable!(),
        };
        let sql = format!(
            "INSERT INTO {table}(id,project_id,value_json{extra}) VALUES(?,?,?{placeholders})"
        );
        let mut q = sqlx::query(&sql)
            .bind(value["id"].as_str().unwrap())
            .bind(value["project_id"].as_str().unwrap())
            .bind(value.to_string());
        q = match kind {
            SourceKind::Evidence => q.bind(value["valid"].as_bool().unwrap_or(false)),
            SourceKind::Artifact => q
                .bind(value["producer_analysis_id"].as_str().unwrap())
                .bind(value["valid"].as_bool().unwrap_or(false)),
            SourceKind::Analysis => q.bind(value["status"].as_str().unwrap()),
            SourceKind::Dataset => q.bind(true),
            SourceKind::Provenance => q
                .bind(value["run_id"].as_str().unwrap())
                .bind(value["analysis_id"].as_str().unwrap())
                .bind(value["complete"].as_bool().unwrap_or(false)),
            _ => unreachable!(),
        };
        q.execute(store.pool()).await.unwrap();
    }

    fn has(report: &PublicationReadinessReportV1, code: ReadinessFindingCodeV1) -> bool {
        report.findings.iter().any(|f| f.code == code)
    }

    #[tokio::test]
    async fn readiness_independent_invalid_science_and_historical_files() {
        let (store, p, _, r) = fixture().await;
        let e = Uuid::new_v4();
        let a = Uuid::new_v4();
        let x = Uuid::new_v4();
        insert(&store,SourceKind::Evidence,&json!({"id":e,"project_id":p,"valid":false,"sources":[{"kind":"artifact","artifact_id":x}],"source_call_id":"same-call"})).await;
        insert(&store,SourceKind::Artifact,&json!({"id":x,"project_id":p,"valid":false,"relative_path":"results.tsv","sha256":"a".repeat(64),"producer_analysis_id":a})).await;
        insert(&store,SourceKind::Analysis,&json!({"id":a,"project_id":p,"run_id":r,"status":"stale","input_dataset_ids":[],"sample_ids":[],"random_seed":null,"software_requirements":[],"database_versions":{},"runtime":{"backend_id":"ssh:historical","process_identity":"must-not-leak"},"validation_issues":[]})).await;
        let selection = save(&store, p, vec![saved(source(p, SourceKind::Evidence, e))]).await;
        let before = store
            .workspace_get_publication(p, selection.publication_id)
            .await
            .unwrap();
        let changes_before: i64 = sqlx::query_scalar("SELECT total_changes()")
            .fetch_one(store.pool())
            .await
            .unwrap();
        let report = check_publication_readiness(&store, &selection, OBSERVED)
            .await
            .unwrap();
        for code in [
            ReadinessFindingCodeV1::InvalidEvidence,
            ReadinessFindingCodeV1::InvalidArtifact,
            ReadinessFindingCodeV1::StaleAnalysis,
            ReadinessFindingCodeV1::MissingRandomSeed,
            ReadinessFindingCodeV1::ProvenanceNotRecorded,
            ReadinessFindingCodeV1::OriginalFileNotChecked,
            ReadinessFindingCodeV1::HistoricalLocationUnverified,
        ] {
            assert!(has(&report, code), "{code:?}");
        }
        assert_eq!(
            report.reproduction_status,
            ReproductionStatusV1::NotExecuted
        );
        assert_eq!(report.related_records_checked, 3);
        assert!(
            !serde_json::to_string(&report)
                .unwrap()
                .contains("must-not-leak")
        );
        assert_eq!(
            before,
            store
                .workspace_get_publication(p, selection.publication_id)
                .await
                .unwrap()
        );
        let changes_after: i64 = sqlx::query_scalar("SELECT total_changes()")
            .fetch_one(store.pool())
            .await
            .unwrap();
        assert_eq!(
            changes_before, changes_after,
            "readiness must not write any Store table"
        );
    }

    #[tokio::test]
    async fn readiness_recorded_issues_are_fixed_safe_findings() {
        let (store, p, _, r) = fixture().await;
        let m = Uuid::new_v4();
        let a = Uuid::new_v4();
        let issues: Vec<_> = [
            "missing_random_seed",
            "missing_software_version",
            "missing_database_version",
            "missing_sample",
            "invalid_input",
        ]
        .iter()
        .map(|kind| json!({"kind":kind,"message":"password=must-not-leak"}))
        .collect();
        insert(&store,SourceKind::Provenance,&json!({"id":m,"project_id":p,"run_id":r,"analysis_id":a,"complete":false,"issues":issues})).await;
        let selection = save(&store, p, vec![saved(source(p, SourceKind::Provenance, m))]).await;
        let report = check_publication_readiness(&store, &selection, OBSERVED)
            .await
            .unwrap();
        for code in [
            ReadinessFindingCodeV1::IncompleteProvenance,
            ReadinessFindingCodeV1::MissingRandomSeed,
            ReadinessFindingCodeV1::MissingSoftwareVersion,
            ReadinessFindingCodeV1::MissingDatabaseVersion,
            ReadinessFindingCodeV1::MissingSample,
            ReadinessFindingCodeV1::InvalidInput,
            ReadinessFindingCodeV1::LineageNotRecorded,
        ] {
            assert!(has(&report, code), "{code:?}");
        }
        assert!(
            !serde_json::to_string(&report)
                .unwrap()
                .contains("must-not-leak")
        );
    }

    #[tokio::test]
    async fn readiness_missing_lineage_never_guesses_from_call_id() {
        let (store, p, _, r) = fixture().await;
        let e = Uuid::new_v4();
        let a = Uuid::new_v4();
        insert(
            &store,
            SourceKind::Evidence,
            &json!({"id":e,"project_id":p,"valid":true,"sources":[],"source_call_id":"same-call"}),
        )
        .await;
        insert(&store,SourceKind::Analysis,&json!({"id":a,"project_id":p,"run_id":r,"status":"succeeded","source_call_id":"same-call"})).await;
        let selection = save(&store, p, vec![saved(source(p, SourceKind::Evidence, e))]).await;
        let report = check_publication_readiness(&store, &selection, OBSERVED)
            .await
            .unwrap();
        assert_eq!(report.related_records_checked, 1);
        assert!(has(&report, ReadinessFindingCodeV1::LineageNotRecorded));
        assert!(!has(&report, ReadinessFindingCodeV1::MissingRandomSeed));
    }

    #[tokio::test]
    async fn readiness_budget_is_unverified_not_silent_success() {
        let (store, p, _, _) = fixture().await;
        let e = Uuid::new_v4();
        let mut links = vec![];
        for _ in 0..70 {
            let x = Uuid::new_v4();
            links.push(json!({"kind":"artifact","artifact_id":x}));
            insert(
                &store,
                SourceKind::Artifact,
                &json!({"id":x,"project_id":p,"valid":true,"producer_analysis_id":Uuid::new_v4()}),
            )
            .await;
        }
        insert(
            &store,
            SourceKind::Evidence,
            &json!({"id":e,"project_id":p,"valid":true,"sources":links}),
        )
        .await;
        let selection = save(&store, p, vec![saved(source(p, SourceKind::Evidence, e))]).await;
        let report = check_publication_readiness(&store, &selection, OBSERVED)
            .await
            .unwrap();
        assert!(report.truncated);
        assert!(has(&report, ReadinessFindingCodeV1::InspectionLimit));
        assert!(report.findings.len() <= 128);
        assert!(report.related_records_checked <= 64);
    }

    #[tokio::test]
    async fn readiness_exact_revision_identity_and_legacy_public_hash() {
        let (store, p, _, _) = fixture().await;
        let mut selection = save(&store, p, vec![]).await;
        let good = selection.clone();
        selection.expected_revision_sha256 = "b".repeat(64);
        assert_eq!(
            check_publication_readiness(&store, &selection, OBSERVED)
                .await
                .unwrap_err()
                .code,
            PublicationEvidenceErrorCodeV1::RevisionChanged
        );
        selection = good.clone();
        selection.project_id = Uuid::new_v4();
        assert_eq!(
            check_publication_readiness(&store, &selection, OBSERVED)
                .await
                .unwrap_err()
                .code,
            PublicationEvidenceErrorCodeV1::NotFound
        );
        let legacy_id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO publications(id,project_id,title,status) VALUES(?,?,'Legacy','draft')",
        )
        .bind(legacy_id.to_string())
        .bind(p.to_string())
        .execute(store.pool())
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO publication_revisions(id,publication_id,revision,content) VALUES(?,?,1,?)",
        )
        .bind(Uuid::new_v4().to_string())
        .bind(legacy_id.to_string())
        .bind("Legacy password=secret")
        .execute(store.pool())
        .await
        .unwrap();
        let mut legacy = good;
        legacy.publication_id = legacy_id;
        legacy.expected_revision_sha256 =
            hex::encode(sha2::Sha256::digest(b"Legacy password=[REDACTED]"));
        let report = check_publication_readiness(&store, &legacy, OBSERVED)
            .await
            .unwrap();
        assert_eq!(
            report.revision_hash_scope,
            PublicationRevisionHashScopeV1::PublicLegacyMarkdown
        );
        assert!(has(&report, ReadinessFindingCodeV1::LegacyRevision));
    }

    #[tokio::test]
    async fn readiness_same_observation_matches_host_science_projections_without_selectors() {
        let (store, p, c, r) = fixture().await;
        let a = Uuid::new_v4();
        let x = Uuid::new_v4();
        let m = Uuid::new_v4();
        insert(&store,SourceKind::Analysis,&json!({"id":a,"project_id":p,"run_id":r,"status":"succeeded","analysis_type":"QC","input_dataset_ids":[],"sample_ids":[],"random_seed":42,"software_requirements":[],"database_versions":{},"runtime":{"backend_id":"ssh:historical","process_identity":"private-process"},"validation_issues":[]})).await;
        insert(&store,SourceKind::Artifact,&json!({"id":x,"project_id":p,"valid":true,"relative_path":"results.tsv","sha256":"a".repeat(64),"producer_analysis_id":a})).await;
        insert(&store,SourceKind::Provenance,&json!({"id":m,"project_id":p,"run_id":r,"analysis_id":a,"complete":true,"issues":[],"backend_id":"ssh:historical","software_versions":{}})).await;
        let mut snapshots = vec![];
        for (kind, id) in [
            (SourceKind::Analysis, a),
            (SourceKind::Artifact, x),
            (SourceKind::Provenance, m),
        ] {
            let s = crate::workspace_navigation::resolve_source(&store, &source(p, kind, id))
                .await
                .unwrap();
            assert_eq!(s.metadata["conversation_id"], c.to_string());
            snapshots.push(s);
        }
        let selection = save(&store, p, snapshots).await;
        let report = check_publication_readiness(&store, &selection, OBSERVED)
            .await
            .unwrap();
        assert!(!has(&report, ReadinessFindingCodeV1::SourceChanged));
        assert!(!has(&report, ReadinessFindingCodeV1::SourceUnavailable));
        assert!(!has(&report, ReadinessFindingCodeV1::ProvenanceNotRecorded));
        sqlx::query("UPDATE scientific_analyses_v4 SET value_json=json_set(value_json,'$.status','stale') WHERE id=?").bind(a.to_string()).execute(store.pool()).await.unwrap();
        let changed = check_publication_readiness(&store, &selection, OBSERVED)
            .await
            .unwrap();
        assert!(has(&changed, ReadinessFindingCodeV1::SourceChanged));
        assert!(has(&changed, ReadinessFindingCodeV1::StaleAnalysis));
    }

    #[tokio::test]
    async fn readiness_explicit_requirements_samples_and_unowned_runs() {
        let (store, p, _, _) = fixture().await;
        let a = Uuid::new_v4();
        let d = Uuid::new_v4();
        insert(
            &store,
            SourceKind::Dataset,
            &json!({"id":d,"project_id":p,"active":false,"sample_ids":["sample-a","sample-b"]}),
        )
        .await;
        insert(&store,SourceKind::Analysis,&json!({"id":a,"project_id":p,"run_id":Uuid::new_v4(),"status":"succeeded","input_dataset_ids":[d],"sample_ids":["sample-a"],"random_seed":42,"software_requirements":["scanpy"],"database_versions":{"genome":""},"validation_issues":[]})).await;
        let selection = save(&store, p, vec![saved(source(p, SourceKind::Analysis, a))]).await;
        let report = check_publication_readiness(&store, &selection, OBSERVED)
            .await
            .unwrap();
        for code in [
            ReadinessFindingCodeV1::MissingSoftwareVersion,
            ReadinessFindingCodeV1::MissingDatabaseVersion,
            ReadinessFindingCodeV1::MissingSample,
            ReadinessFindingCodeV1::InvalidInput,
            ReadinessFindingCodeV1::LineageNotRecorded,
        ] {
            assert!(has(&report, code));
        }
        assert!(!has(&report, ReadinessFindingCodeV1::MissingRandomSeed));
    }

    #[tokio::test]
    async fn readiness_selected_old_revision_and_missing_snapshot_are_immutable() {
        let (store, p, _, _) = fixture().await;
        let e = Uuid::new_v4();
        let selection = save(&store, p, vec![saved(source(p, SourceKind::Evidence, e))]).await;
        store
            .workspace_save_publication(&SavePublicationRecord {
                request: SavePublicationRequest {
                    request_id: Uuid::new_v4(),
                    project_id: p,
                    publication_id: Some(selection.publication_id),
                    expected_revision: 1,
                    title: "Head".into(),
                    markdown: "Different head".into(),
                    sources: vec![],
                },
                references: vec![],
            })
            .await
            .unwrap();
        let before = store
            .workspace_get_publication(p, selection.publication_id)
            .await
            .unwrap();
        let report = check_publication_readiness(&store, &selection, OBSERVED)
            .await
            .unwrap();
        assert_eq!(report.selection, selection);
        assert_eq!(report.references_checked, 1);
        assert!(has(&report, ReadinessFindingCodeV1::SourceMissing));
        assert_eq!(
            before,
            store
                .workspace_get_publication(p, selection.publication_id)
                .await
                .unwrap()
        );
        assert_eq!(
            before
                .revisions
                .iter()
                .find(|r| r.revision == 1)
                .unwrap()
                .references[0]
                .text,
            "Saved snapshot"
        );
        let mut invalid = selection;
        invalid.revision = 0;
        assert_eq!(
            check_publication_readiness(&store, &invalid, OBSERVED)
                .await
                .unwrap_err()
                .code,
            PublicationEvidenceErrorCodeV1::InvalidSelection
        );
    }

    #[tokio::test]
    async fn readiness_rejects_unsafe_public_source_identifiers_without_echo() {
        let (store, p, _, _) = fixture().await;
        let mut s = source(p, SourceKind::Evidence, Uuid::new_v4());
        s.id = "password=do-not-echo".into();
        let selection = save(&store, p, vec![saved(s)]).await;
        let failure = check_publication_readiness(&store, &selection, OBSERVED)
            .await
            .unwrap_err();
        assert_eq!(
            failure.code,
            PublicationEvidenceErrorCodeV1::UnsafePublicContent
        );
        assert!(
            !serde_json::to_string(&failure)
                .unwrap()
                .contains("do-not-echo")
        );
    }

    #[tokio::test]
    async fn readiness_rejects_quoted_password_in_source_id_without_echo() {
        let (store, p, _, _) = fixture().await;
        let mut reference = source(p, SourceKind::Evidence, Uuid::new_v4());
        reference.id = r#""password": "synthetic-password-secret""#.into();
        let selection = save(&store, p, vec![saved(reference)]).await;
        let result = check_publication_readiness(&store, &selection, OBSERVED).await;
        assert!(
            result.is_err(),
            "quoted password source ID must be rejected"
        );
        let failure = result.unwrap_err();
        assert_eq!(
            failure.code,
            PublicationEvidenceErrorCodeV1::UnsafePublicContent
        );
        assert!(
            !serde_json::to_string(&failure)
                .unwrap()
                .contains("synthetic-password-secret")
        );
    }

    #[tokio::test]
    async fn readiness_rejects_quoted_token_in_source_id_without_echo() {
        let (store, p, _, _) = fixture().await;
        let mut reference = source(p, SourceKind::Tool, Uuid::new_v4());
        reference.id = r#""token": "synthetic-token-secret""#.into();
        let selection = save(&store, p, vec![saved(reference)]).await;
        let result = check_publication_readiness(&store, &selection, OBSERVED).await;
        assert!(result.is_err(), "quoted token source ID must be rejected");
        let failure = result.unwrap_err();
        assert_eq!(
            failure.code,
            PublicationEvidenceErrorCodeV1::UnsafePublicContent
        );
        assert!(
            !serde_json::to_string(&failure)
                .unwrap()
                .contains("synthetic-token-secret")
        );
    }

    #[tokio::test]
    async fn readiness_rejects_quoted_secret_in_historical_string_selectors() {
        let (store, p, _, _) = fixture().await;
        let mut selection = save(&store, p, vec![]).await;
        for (revision, field) in [(2, "event_hash"), (3, "content_sha256")] {
            let mut reference = saved(source(p, SourceKind::Tool, Uuid::new_v4()));
            let secret = Some(r#""password": "synthetic-selector-secret""#.to_owned());
            if field == "event_hash" {
                reference.source.event_hash = secret;
            } else {
                reference.source.content_sha256 = secret;
            }
            // Defensive read of a historical row that save-time hash validation
            // would reject. Keep the exact stored revision hash for selection.
            let content = json!({"schema_version":1,"title":"Historical","markdown":"Saved","references":[reference]}).to_string();
            sqlx::query("INSERT INTO publication_revisions(id,publication_id,revision,content) VALUES(?,?,?,?)")
                .bind(Uuid::new_v4().to_string()).bind(selection.publication_id.to_string()).bind(revision).bind(&content).execute(store.pool()).await.unwrap();
            selection.revision = revision as u64;
            selection.expected_revision_sha256 =
                hex::encode(sha2::Sha256::digest(content.as_bytes()));
            let result = check_publication_readiness(&store, &selection, OBSERVED).await;
            assert!(result.is_err(), "raw {field} secret must be rejected");
            let failure = result.unwrap_err();
            assert_eq!(
                failure.code,
                PublicationEvidenceErrorCodeV1::UnsafePublicContent
            );
            assert!(
                !serde_json::to_string(&failure)
                    .unwrap()
                    .contains("synthetic-selector-secret")
            );
        }
    }

    #[tokio::test]
    async fn readiness_rejects_authentication_uri_in_public_identifier() {
        let (store, p, _, _) = fixture().await;
        let mut s = source(p, SourceKind::Tool, Uuid::new_v4());
        s.id = "https://researcher:do-not-echo@example.invalid/tool".into();
        let selection = save(&store, p, vec![saved(s)]).await;
        let failure = check_publication_readiness(&store, &selection, OBSERVED)
            .await
            .unwrap_err();
        assert_eq!(
            failure.code,
            PublicationEvidenceErrorCodeV1::UnsafePublicContent
        );
        assert!(
            !serde_json::to_string(&failure)
                .unwrap()
                .contains("do-not-echo")
        );
    }
}
