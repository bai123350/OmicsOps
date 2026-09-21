use std::collections::{BTreeMap, BTreeSet};

use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use thiserror::Error;
use uuid::Uuid;

pub const SCIENTIFIC_STATE_V4: &str = "omicsops.scientific-state@4.0.0";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DatasetStageV4 {
    Raw,
    Processed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct DatasetRecordV4 {
    pub schema_version: u8,
    pub id: Uuid,
    pub project_id: Uuid,
    pub modality: String,
    pub species: String,
    pub sample_ids: BTreeSet<String>,
    pub matrix_shape: Vec<u64>,
    pub stage: DatasetStageV4,
    pub relative_path: String,
    pub size_bytes: u64,
    pub sha256: String,
    pub active: bool,
    pub supersedes: Option<Uuid>,
    pub registered_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AnalysisStatusV4 {
    Running,
    Succeeded,
    Failed,
    Cancelled,
    Stale,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct RuntimeIdentityV4 {
    pub backend_id: String,
    pub language: String,
    pub environment: String,
    pub session_id: Option<Uuid>,
    pub process_identity: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct AnalysisRecordV4 {
    pub schema_version: u8,
    pub id: Uuid,
    pub project_id: Uuid,
    pub run_id: Uuid,
    pub source_call_id: String,
    pub analysis_type: String,
    pub input_dataset_ids: BTreeSet<Uuid>,
    pub sample_ids: BTreeSet<String>,
    pub method: String,
    pub parameters: Value,
    pub software_requirements: BTreeSet<String>,
    pub database_versions: BTreeMap<String, String>,
    pub random_seed: Option<u64>,
    pub status: AnalysisStatusV4,
    pub runtime: RuntimeIdentityV4,
    pub started_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
    pub validation_issues: Vec<ProvenanceIssueV4>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ArtifactRecordV4 {
    pub schema_version: u8,
    pub id: Uuid,
    pub project_id: Uuid,
    pub artifact_type: String,
    pub relative_path: String,
    pub producer_analysis_id: Uuid,
    pub size_bytes: u64,
    pub sha256: String,
    pub preview: Option<String>,
    pub metadata: Value,
    pub valid: bool,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceStrengthV4 {
    Exploratory,
    Supporting,
    Strong,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EvidenceSourceV4 {
    Artifact { artifact_id: Uuid },
    Literature { source_id: String, citation: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct EvidenceRecordV4 {
    pub schema_version: u8,
    pub id: Uuid,
    pub project_id: Uuid,
    pub source_call_id: String,
    pub claim: String,
    pub sources: Vec<EvidenceSourceV4>,
    pub strength: EvidenceStrengthV4,
    pub conflicts_with: BTreeSet<Uuid>,
    pub valid: bool,
    pub invalid_reason: Option<String>,
    pub recorded_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ProvenanceIssueKindV4 {
    MissingRandomSeed,
    MissingSoftwareVersion,
    MissingDatabaseVersion,
    MissingSample,
    InvalidInput,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ProvenanceIssueV4 {
    pub kind: ProvenanceIssueKindV4,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ProvenanceManifestV4 {
    pub schema_version: u8,
    pub id: Uuid,
    pub project_id: Uuid,
    pub run_id: Uuid,
    pub analysis_id: Uuid,
    pub source_call_id: String,
    pub code: String,
    pub code_sha256: String,
    pub input_checksums: BTreeMap<Uuid, String>,
    pub output_artifact_ids: BTreeSet<Uuid>,
    pub software_versions: BTreeMap<String, String>,
    pub database_versions: BTreeMap<String, String>,
    pub environment: String,
    pub random_seed: Option<u64>,
    pub backend_id: String,
    pub runtime_session_id: Option<Uuid>,
    pub runtime_process_identity: Option<String>,
    pub issues: Vec<ProvenanceIssueV4>,
    pub complete: bool,
    pub recorded_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ScientificStateV4 {
    pub schema_version: u8,
    pub runtime_id: String,
    pub project_id: Uuid,
    pub revision: u64,
    pub datasets: BTreeMap<Uuid, DatasetRecordV4>,
    pub analyses: BTreeMap<Uuid, AnalysisRecordV4>,
    pub artifacts: BTreeMap<Uuid, ArtifactRecordV4>,
    pub evidence: BTreeMap<Uuid, EvidenceRecordV4>,
    pub provenance: BTreeMap<Uuid, ProvenanceManifestV4>,
}

impl ScientificStateV4 {
    pub fn new(project_id: Uuid) -> Self {
        Self {
            schema_version: 4,
            runtime_id: SCIENTIFIC_STATE_V4.into(),
            project_id,
            revision: 0,
            datasets: BTreeMap::new(),
            analyses: BTreeMap::new(),
            artifacts: BTreeMap::new(),
            evidence: BTreeMap::new(),
            provenance: BTreeMap::new(),
        }
    }

    pub fn digest(&self) -> String {
        hex::encode(Sha256::digest(
            serde_json::to_vec(self).expect("serializable scientific state"),
        ))
    }

    pub fn register_dataset(
        &mut self,
        fact: VerifiedDatasetFactV4,
        now: DateTime<Utc>,
    ) -> DatasetRecordV4 {
        let supersedes = self
            .datasets
            .values()
            .find(|dataset| {
                dataset.active
                    && dataset.relative_path == fact.relative_path
                    && dataset.sha256 != fact.sha256
            })
            .map(|dataset| dataset.id);
        if let Some(previous_id) = supersedes {
            if let Some(previous) = self.datasets.get_mut(&previous_id) {
                previous.active = false;
            }
            self.invalidate_dataset_dependents(previous_id);
        }
        let record = DatasetRecordV4 {
            schema_version: 4,
            id: Uuid::new_v4(),
            project_id: self.project_id,
            modality: fact.modality,
            species: fact.species,
            sample_ids: fact.sample_ids,
            matrix_shape: fact.matrix_shape,
            stage: fact.stage,
            relative_path: fact.relative_path,
            size_bytes: fact.size_bytes,
            sha256: fact.sha256,
            active: true,
            supersedes,
            registered_at: now,
        };
        self.datasets.insert(record.id, record.clone());
        self.revision += 1;
        record
    }

    pub fn start_analysis(
        &mut self,
        run_id: Uuid,
        source_call_id: String,
        declaration: AnalysisDeclarationV4,
        runtime: RuntimeIdentityV4,
        now: DateTime<Utc>,
    ) -> Result<AnalysisRecordV4, ScienceErrorV4> {
        let mut expected_samples = BTreeSet::new();
        for dataset_id in &declaration.input_dataset_ids {
            let dataset = self
                .datasets
                .get(dataset_id)
                .filter(|dataset| dataset.active)
                .ok_or(ScienceErrorV4::InvalidDataset(*dataset_id))?;
            expected_samples.extend(dataset.sample_ids.iter().cloned());
        }
        let missing = expected_samples
            .difference(&declaration.sample_ids)
            .cloned()
            .collect::<Vec<_>>();
        if !missing.is_empty() {
            return Err(ScienceErrorV4::MissingSamples(missing));
        }
        let record = AnalysisRecordV4 {
            schema_version: 4,
            id: Uuid::new_v4(),
            project_id: self.project_id,
            run_id,
            source_call_id,
            analysis_type: declaration.analysis_type,
            input_dataset_ids: declaration.input_dataset_ids,
            sample_ids: declaration.sample_ids,
            method: declaration.method,
            parameters: declaration.parameters,
            software_requirements: declaration.software_requirements,
            database_versions: declaration.database_versions,
            random_seed: declaration.random_seed,
            status: AnalysisStatusV4::Running,
            runtime,
            started_at: now,
            ended_at: None,
            validation_issues: vec![],
        };
        self.analyses.insert(record.id, record.clone());
        self.revision += 1;
        Ok(record)
    }

    pub fn finish_analysis(
        &mut self,
        source_call_id: &str,
        succeeded: bool,
        runtime_session_id: Option<Uuid>,
        runtime_process_identity: Option<String>,
        artifacts: Vec<VerifiedArtifactFactV4>,
        software_versions: BTreeMap<String, String>,
        code: String,
        now: DateTime<Utc>,
    ) -> Result<
        (
            AnalysisRecordV4,
            Vec<ArtifactRecordV4>,
            ProvenanceManifestV4,
        ),
        ScienceErrorV4,
    > {
        let analysis_id = self
            .analyses
            .values()
            .find(|analysis| {
                analysis.source_call_id == source_call_id
                    && analysis.status == AnalysisStatusV4::Running
            })
            .map(|analysis| analysis.id)
            .ok_or_else(|| ScienceErrorV4::AnalysisNotRunning(source_call_id.into()))?;
        let analysis = self
            .analyses
            .get_mut(&analysis_id)
            .expect("analysis selected above");
        analysis.status = if succeeded {
            AnalysisStatusV4::Succeeded
        } else {
            AnalysisStatusV4::Failed
        };
        analysis.ended_at = Some(now);
        analysis.runtime.session_id = runtime_session_id;
        analysis.runtime.process_identity = runtime_process_identity.clone();

        let mut issues = Vec::new();
        if analysis.random_seed.is_none() {
            issues.push(ProvenanceIssueV4 {
                kind: ProvenanceIssueKindV4::MissingRandomSeed,
                message: "analysis did not declare a random seed".into(),
            });
        }
        for requirement in &analysis.software_requirements {
            if !software_versions.contains_key(requirement) {
                issues.push(ProvenanceIssueV4 {
                    kind: ProvenanceIssueKindV4::MissingSoftwareVersion,
                    message: format!("missing version for {requirement}"),
                });
            }
        }
        analysis.validation_issues = issues.clone();
        let analysis_snapshot = analysis.clone();

        let created_artifacts = artifacts
            .into_iter()
            .map(|artifact| ArtifactRecordV4 {
                schema_version: 4,
                id: Uuid::new_v4(),
                project_id: self.project_id,
                artifact_type: artifact.artifact_type,
                relative_path: artifact.relative_path,
                producer_analysis_id: analysis_id,
                size_bytes: artifact.size_bytes,
                sha256: artifact.sha256,
                preview: artifact.preview,
                metadata: artifact.metadata,
                valid: succeeded,
                created_at: now,
            })
            .collect::<Vec<_>>();
        for artifact in &created_artifacts {
            self.artifacts.insert(artifact.id, artifact.clone());
        }
        let input_checksums = analysis_snapshot
            .input_dataset_ids
            .iter()
            .filter_map(|id| {
                self.datasets
                    .get(id)
                    .map(|dataset| (*id, dataset.sha256.clone()))
            })
            .collect();
        let manifest = ProvenanceManifestV4 {
            schema_version: 4,
            id: Uuid::new_v4(),
            project_id: self.project_id,
            run_id: analysis_snapshot.run_id,
            analysis_id,
            source_call_id: source_call_id.into(),
            code_sha256: hex::encode(Sha256::digest(code.as_bytes())),
            code,
            input_checksums,
            output_artifact_ids: created_artifacts
                .iter()
                .map(|artifact| artifact.id)
                .collect(),
            software_versions,
            database_versions: analysis_snapshot.database_versions.clone(),
            environment: analysis_snapshot.runtime.environment.clone(),
            random_seed: analysis_snapshot.random_seed,
            backend_id: analysis_snapshot.runtime.backend_id.clone(),
            runtime_session_id,
            runtime_process_identity,
            complete: succeeded && issues.is_empty(),
            issues,
            recorded_at: now,
        };
        self.provenance.insert(manifest.id, manifest.clone());
        self.revision += 1;
        Ok((analysis_snapshot, created_artifacts, manifest))
    }

    pub fn record_evidence(
        &mut self,
        source_call_id: String,
        declaration: EvidenceDeclarationV4,
        now: DateTime<Utc>,
    ) -> Result<EvidenceRecordV4, ScienceErrorV4> {
        if declaration.claim.trim().is_empty() || declaration.sources.is_empty() {
            return Err(ScienceErrorV4::InvalidEvidence);
        }
        for source in &declaration.sources {
            if let EvidenceSourceV4::Artifact { artifact_id } = source {
                if !self
                    .artifacts
                    .get(artifact_id)
                    .is_some_and(|artifact| artifact.valid)
                {
                    return Err(ScienceErrorV4::InvalidArtifact(*artifact_id));
                }
            }
        }
        if declaration
            .conflicts_with
            .iter()
            .any(|id| !self.evidence.contains_key(id))
        {
            return Err(ScienceErrorV4::InvalidEvidence);
        }
        let record = EvidenceRecordV4 {
            schema_version: 4,
            id: Uuid::new_v4(),
            project_id: self.project_id,
            source_call_id,
            claim: declaration.claim,
            sources: declaration.sources,
            strength: declaration.strength,
            conflicts_with: declaration.conflicts_with,
            valid: true,
            invalid_reason: None,
            recorded_at: now,
        };
        self.evidence.insert(record.id, record.clone());
        self.revision += 1;
        Ok(record)
    }

    fn invalidate_dataset_dependents(&mut self, dataset_id: Uuid) {
        let stale_analyses = self
            .analyses
            .values_mut()
            .filter(|analysis| analysis.input_dataset_ids.contains(&dataset_id))
            .map(|analysis| {
                analysis.status = AnalysisStatusV4::Stale;
                analysis.id
            })
            .collect::<BTreeSet<_>>();
        let invalid_artifacts = self
            .artifacts
            .values_mut()
            .filter(|artifact| stale_analyses.contains(&artifact.producer_analysis_id))
            .map(|artifact| {
                artifact.valid = false;
                artifact.id
            })
            .collect::<BTreeSet<_>>();
        for evidence in self.evidence.values_mut() {
            if evidence.sources.iter().any(|source| {
                matches!(source, EvidenceSourceV4::Artifact { artifact_id } if invalid_artifacts.contains(artifact_id))
            }) {
                evidence.valid = false;
                evidence.invalid_reason = Some("input dataset checksum changed".into());
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedDatasetFactV4 {
    pub modality: String,
    pub species: String,
    pub sample_ids: BTreeSet<String>,
    pub matrix_shape: Vec<u64>,
    pub stage: DatasetStageV4,
    pub relative_path: String,
    pub size_bytes: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct AnalysisDeclarationV4 {
    pub analysis_type: String,
    pub input_dataset_ids: BTreeSet<Uuid>,
    pub sample_ids: BTreeSet<String>,
    pub method: String,
    pub parameters: Value,
    #[serde(default)]
    pub software_requirements: BTreeSet<String>,
    #[serde(default)]
    pub database_versions: BTreeMap<String, String>,
    pub random_seed: Option<u64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct VerifiedArtifactFactV4 {
    pub artifact_type: String,
    pub relative_path: String,
    pub size_bytes: u64,
    pub sha256: String,
    pub preview: Option<String>,
    pub metadata: Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct EvidenceDeclarationV4 {
    pub claim: String,
    pub sources: Vec<EvidenceSourceV4>,
    pub strength: EvidenceStrengthV4,
    #[serde(default)]
    pub conflicts_with: BTreeSet<Uuid>,
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ScienceErrorV4 {
    #[error("dataset {0} is missing or inactive")]
    InvalidDataset(Uuid),
    #[error("analysis omitted required samples: {0:?}")]
    MissingSamples(Vec<String>),
    #[error("analysis for call {0} is not running")]
    AnalysisNotRunning(String),
    #[error("artifact {0} is missing or invalid")]
    InvalidArtifact(Uuid),
    #[error("evidence declaration is invalid")]
    InvalidEvidence,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn dataset(path: &str, hash: &str) -> VerifiedDatasetFactV4 {
        VerifiedDatasetFactV4 {
            modality: "single_cell_rna".into(),
            species: "human".into(),
            sample_ids: BTreeSet::from(["sample-a".into(), "sample-b".into()]),
            matrix_shape: vec![100, 20_000],
            stage: DatasetStageV4::Raw,
            relative_path: path.into(),
            size_bytes: 42,
            sha256: hash.into(),
        }
    }

    fn declaration(dataset_id: Uuid) -> AnalysisDeclarationV4 {
        AnalysisDeclarationV4 {
            analysis_type: "qc".into(),
            input_dataset_ids: BTreeSet::from([dataset_id]),
            sample_ids: BTreeSet::from(["sample-a".into(), "sample-b".into()]),
            method: "dynamic-python".into(),
            parameters: json!({"threshold":200}),
            software_requirements: BTreeSet::from(["scanpy".into()]),
            database_versions: BTreeMap::new(),
            random_seed: Some(7),
        }
    }

    fn runtime() -> RuntimeIdentityV4 {
        RuntimeIdentityV4 {
            backend_id: "ssh:test".into(),
            language: "python".into(),
            environment: "project".into(),
            session_id: None,
            process_identity: None,
        }
    }

    #[test]
    fn artifact_has_host_assigned_producer_and_complete_lineage() {
        let project = Uuid::new_v4();
        let mut state = ScientificStateV4::new(project);
        let data = state.register_dataset(dataset("data/raw.h5ad", "a"), Utc::now());
        let analysis = state
            .start_analysis(
                Uuid::new_v4(),
                "call".into(),
                declaration(data.id),
                runtime(),
                Utc::now(),
            )
            .unwrap();
        let (_, artifacts, manifest) = state
            .finish_analysis(
                "call",
                true,
                Some(Uuid::new_v4()),
                Some("process".into()),
                vec![VerifiedArtifactFactV4 {
                    artifact_type: "table".into(),
                    relative_path: "results/qc.csv".into(),
                    size_bytes: 10,
                    sha256: "b".into(),
                    preview: None,
                    metadata: json!({}),
                }],
                BTreeMap::from([("scanpy".into(), "1.11".into())]),
                "print('qc')".into(),
                Utc::now(),
            )
            .unwrap();
        assert_eq!(artifacts[0].producer_analysis_id, analysis.id);
        assert_eq!(manifest.input_checksums[&data.id], "a");
        assert!(manifest.output_artifact_ids.contains(&artifacts[0].id));
        assert!(manifest.complete);
    }

    #[test]
    fn changed_input_invalidates_analysis_artifact_and_evidence() {
        let mut state = ScientificStateV4::new(Uuid::new_v4());
        let data = state.register_dataset(dataset("data/raw.h5ad", "old"), Utc::now());
        let analysis = state
            .start_analysis(
                Uuid::new_v4(),
                "call".into(),
                declaration(data.id),
                runtime(),
                Utc::now(),
            )
            .unwrap();
        let (_, artifacts, _) = state
            .finish_analysis(
                "call",
                true,
                Some(Uuid::new_v4()),
                Some("process".into()),
                vec![VerifiedArtifactFactV4 {
                    artifact_type: "report".into(),
                    relative_path: "report.html".into(),
                    size_bytes: 1,
                    sha256: "artifact".into(),
                    preview: None,
                    metadata: json!({}),
                }],
                BTreeMap::from([("scanpy".into(), "1.11".into())]),
                "code".into(),
                Utc::now(),
            )
            .unwrap();
        let evidence = state
            .record_evidence(
                "evidence-call".into(),
                EvidenceDeclarationV4 {
                    claim: "QC passed".into(),
                    sources: vec![EvidenceSourceV4::Artifact {
                        artifact_id: artifacts[0].id,
                    }],
                    strength: EvidenceStrengthV4::Supporting,
                    conflicts_with: BTreeSet::new(),
                },
                Utc::now(),
            )
            .unwrap();
        state.register_dataset(dataset("data/raw.h5ad", "new"), Utc::now());
        assert_eq!(state.analyses[&analysis.id].status, AnalysisStatusV4::Stale);
        assert!(!state.artifacts[&artifacts[0].id].valid);
        assert!(!state.evidence[&evidence.id].valid);
    }

    #[test]
    fn omitted_samples_and_missing_reproducibility_fields_are_detected() {
        let mut state = ScientificStateV4::new(Uuid::new_v4());
        let data = state.register_dataset(dataset("data.h5ad", "a"), Utc::now());
        let mut incomplete = declaration(data.id);
        incomplete.sample_ids.remove("sample-b");
        assert!(matches!(
            state.start_analysis(
                Uuid::new_v4(),
                "bad".into(),
                incomplete,
                runtime(),
                Utc::now()
            ),
            Err(ScienceErrorV4::MissingSamples(_))
        ));

        let mut missing = declaration(data.id);
        missing.random_seed = None;
        state
            .start_analysis(
                Uuid::new_v4(),
                "missing".into(),
                missing,
                runtime(),
                Utc::now(),
            )
            .unwrap();
        let (_, _, manifest) = state
            .finish_analysis(
                "missing",
                true,
                None,
                None,
                vec![],
                BTreeMap::new(),
                "code".into(),
                Utc::now(),
            )
            .unwrap();
        assert!(!manifest.complete);
        assert!(
            manifest
                .issues
                .iter()
                .any(|issue| issue.kind == ProvenanceIssueKindV4::MissingRandomSeed)
        );
        assert!(
            manifest
                .issues
                .iter()
                .any(|issue| issue.kind == ProvenanceIssueKindV4::MissingSoftwareVersion)
        );
    }

    #[test]
    fn bare_or_empty_requirements_match_recorded_versions_but_combined_strings_do_not() {
        fn finish(requirements: BTreeSet<String>) -> ProvenanceManifestV4 {
            let mut state = ScientificStateV4::new(Uuid::new_v4());
            let data = state.register_dataset(dataset("data.h5ad", "a"), Utc::now());
            let mut analysis = declaration(data.id);
            analysis.software_requirements = requirements;
            state
                .start_analysis(
                    Uuid::new_v4(),
                    "call".into(),
                    analysis,
                    runtime(),
                    Utc::now(),
                )
                .unwrap();
            state
                .finish_analysis(
                    "call",
                    true,
                    None,
                    None,
                    vec![],
                    BTreeMap::from([
                        ("python".into(), "3.11".into()),
                        ("numpy".into(), "2.1".into()),
                    ]),
                    "code".into(),
                    Utc::now(),
                )
                .unwrap()
                .2
        }

        assert!(finish(BTreeSet::new()).complete);
        assert!(finish(BTreeSet::from(["numpy".into()])).complete);

        let malformed = finish(BTreeSet::from(["python=3.11 urllib".into()]));
        assert!(!malformed.complete);
        assert!(malformed.issues.iter().any(|issue| {
            issue.kind == ProvenanceIssueKindV4::MissingSoftwareVersion
                && issue.message.contains("python=3.11 urllib")
        }));
    }
}
