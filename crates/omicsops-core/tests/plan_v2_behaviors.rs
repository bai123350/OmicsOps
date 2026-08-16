use std::collections::BTreeMap;

use omicsops_core::{
    domain::{ResourceLimits, StepRisk},
    plan_v2::{
        AnalysisPlanV2, PlanEnvironment, PlanStageV2, PolicyEnvelope, StepAction, StepSpecV2,
        VerificationSpec, canonical_plan_hash,
    },
    tools::{ToolCatalog, builtin_tool_catalog},
    validation::validate_plan_v2,
};

#[test]
fn v2_checkpoint_tracks_verified_step_ids_instead_of_array_positions() {
    use omicsops_core::plan_v2::{RunCheckpointV2, RunStateV2};

    let mut checkpoint = RunCheckpointV2::new(
        Uuid::new_v4(),
        Uuid::new_v4(),
        Uuid::new_v4(),
        Uuid::new_v4(),
    );
    checkpoint.mark_verified("qc", "action-hash");

    assert!(checkpoint.completed_steps.contains("qc"));
    assert_eq!(checkpoint.action_hashes["qc"], "action-hash");
    checkpoint.needs_attention("remote manifest is missing");
    assert_eq!(checkpoint.state, RunStateV2::NeedsAttention);
    assert_eq!(
        checkpoint.attention_reason.as_deref(),
        Some("remote manifest is missing")
    );
}

#[test]
fn repair_assessment_rejects_changes_outside_approved_outputs_and_resources() {
    use omicsops_core::validation::assess_repair;
    let catalog = builtin_tool_catalog().unwrap();
    let candidate_plan = plan(tool_step("download", "core.download"));
    let approved = candidate_plan.stages[0].steps[0].clone();
    let mut proposed = approved.clone();
    proposed.resources.max_memory_gib += 1;
    proposed.expected_artifacts = vec!["results/different.fastq.gz".into()];
    let assessment = assess_repair(&candidate_plan, &approved, &proposed, &catalog);
    assert!(!assessment.within_envelope);
    assert!(
        assessment
            .diffs
            .iter()
            .any(|diff| diff.field == "resources")
    );
    assert!(
        assessment
            .diffs
            .iter()
            .any(|diff| diff.field == "expected_artifacts")
    );
    let mut changed_action = approved.clone();
    if let StepAction::Tool { arguments, .. } = &mut changed_action.action {
        arguments["output"] = json!("work/other.fastq.gz");
    }
    let assessment = assess_repair(&candidate_plan, &approved, &changed_action, &catalog);
    assert!(!assessment.within_envelope);
    assert!(
        assessment
            .diffs
            .iter()
            .any(|diff| diff.field == "action.output_paths")
    );
}
use serde_json::json;

#[test]
fn generated_steps_default_optional_execution_fields() {
    let value = json!({
        "id": "agent", "title": "Agent", "rationale": "Adaptive execution",
        "action": {"kind":"tool", "tool_id":"agent.remote_task", "version":"1.0.0", "arguments":{}}
    });
    let step: omicsops_core::plan_v2::StepSpecV2 = serde_json::from_value(value).unwrap();
    assert_eq!(
        step.resources,
        omicsops_core::domain::ResourceLimits::default()
    );
    assert!(step.verifications.is_empty());
    assert!(step.expected_artifacts.is_empty());
}
use uuid::Uuid;

fn tool_step(id: &str, tool_id: &str) -> StepSpecV2 {
    StepSpecV2 {
        id: id.into(),
        title: id.into(),
        rationale: "fixture".into(),
        dependencies: Vec::new(),
        action: StepAction::Tool {
            tool_id: tool_id.into(),
            version: "1.0.0".into(),
            arguments: json!({"url": "https://example.org/input.fastq.gz", "output": "work/input.fastq.gz"}),
        },
        working_directory: "work".into(),
        resources: ResourceLimits::default(),
        risk: StepRisk::Low,
        verifications: vec![VerificationSpec::File {
            path: "work/input.fastq.gz".into(),
            min_bytes: 1,
            sha256: None,
        }],
        expected_artifacts: vec!["work/input.fastq.gz".into()],
    }
}

fn plan(step: StepSpecV2) -> AnalysisPlanV2 {
    AnalysisPlanV2 {
        schema_version: 2,
        id: Uuid::new_v4(),
        title: "fixture".into(),
        summary: "fixture".into(),
        environment: PlanEnvironment::Micromamba {
            channels: vec!["conda-forge".into(), "bioconda".into()],
            dependencies: vec!["python=3.12".into()],
        },
        stages: vec![PlanStageV2 {
            id: "stage".into(),
            goal: "fixture".into(),
            dependencies: Vec::new(),
            steps: vec![step],
        }],
        resource_budget: ResourceLimits::default(),
        policy: PolicyEnvelope {
            allowed_tools: vec!["core.download".into()],
            allowed_domains: vec!["example.org".into()],
            max_risk: StepRisk::Low,
            allow_legacy_shell: false,
        },
        metadata: BTreeMap::new(),
    }
}

#[test]
fn generated_steps_default_to_the_project_root_when_working_directory_is_omitted() {
    let mut value = serde_json::to_value(plan(tool_step("download", "core.download"))).unwrap();
    value["stages"][0]["steps"][0]
        .as_object_mut()
        .unwrap()
        .remove("working_directory");

    let decoded: AnalysisPlanV2 = serde_json::from_value(value).unwrap();
    assert_eq!(decoded.stages[0].steps[0].working_directory, ".");
}

#[test]
fn scanpy_tool_accepts_the_structured_analysis_contract() {
    let catalog = builtin_tool_catalog().unwrap();
    let mut candidate = plan(tool_step("scanpy", "bio.scanpy"));
    candidate.policy.allowed_tools = vec!["bio.scanpy".into()];
    candidate.stages[0].steps[0].action = StepAction::Tool {
        tool_id: "bio.scanpy".into(),
        version: "1.0.0".into(),
        arguments: json!({
            "input_directory": "data/filtered_gene_bc_matrices/hg19",
            "qc": {"min_genes": 200},
            "outputs": {
                "h5ad": "results/qc.h5ad",
                "qc_metrics": "results/qc.tsv",
                "cluster_annotations": "results/annotations.tsv",
                "umap": "results/umap.png",
                "qc_plots": "results/qc.png",
                "marker_scores": "results/markers.tsv"
            }
        }),
    };

    let validation = validate_plan_v2(&candidate, &catalog);
    assert!(validation.valid, "{:?}", validation.issues);
}

#[test]
fn remote_agent_tool_accepts_read_only_memory_context() {
    let catalog = builtin_tool_catalog().unwrap();
    let mut candidate = plan(tool_step("agent", "agent.remote_task"));
    candidate.policy.allowed_tools = vec!["agent.remote_task".into()];
    candidate.policy.max_risk = StepRisk::Medium;
    candidate.stages[0].steps[0].risk = StepRisk::Medium;
    candidate.stages[0].steps[0].expected_artifacts.clear();
    candidate.stages[0].steps[0].verifications = vec![VerificationSpec::ExitCode { expected: 0 }];
    candidate.stages[0].steps[0].action = StepAction::Tool {
        tool_id: "agent.remote_task".into(),
        version: "1.0.0".into(),
        arguments: json!({
            "goal": "continue the approved analysis",
            "remote_observation": "data/input.mtx exists",
            "completion_criteria": ["verify outputs"],
            "skill_context": "single-cell workflow",
            "conversation_history": "USER: continue from the previous step",
            "operational_memory": "scanpy was installed successfully",
            "max_iterations": 24
        }),
    };

    let validation = validate_plan_v2(&candidate, &catalog);
    assert!(validation.valid, "{:?}", validation.issues);
}

#[test]
fn harness_v3_is_versioned_without_removing_v2_recovery() {
    let catalog = builtin_tool_catalog().unwrap();
    assert!(catalog.get("agent.harness_v3", "3.0.0").is_some());
    assert!(catalog.get("agent.remote_task", "1.0.0").is_some());
}

#[test]
fn builtin_catalog_exposes_versioned_bioinformatics_tools() {
    let catalog = builtin_tool_catalog().unwrap();

    for id in [
        "core.download",
        "env.micromamba",
        "bio.fastqc",
        "bio.multiqc",
        "bio.salmon",
        "bio.deseq2",
        "bio.scanpy",
        "bio.h5ad_to_seurat",
        "report.html",
    ] {
        assert!(catalog.get(id, "1.0.0").is_some(), "missing {id}");
    }
}

#[test]
fn validator_rejects_unknown_tools_cycles_and_external_paths() {
    let catalog = builtin_tool_catalog().unwrap();
    let mut candidate = plan(tool_step("download", "missing.tool"));
    candidate.stages[0].steps[0].dependencies = vec!["download".into()];
    candidate.stages[0].steps[0].working_directory = "../outside".into();

    let result = validate_plan_v2(&candidate, &catalog);

    assert!(!result.valid);
    assert!(
        result
            .issues
            .iter()
            .any(|issue| issue.code == "unknown_tool")
    );
    assert!(
        result
            .issues
            .iter()
            .any(|issue| issue.code == "dependency_cycle")
    );
    assert!(
        result
            .issues
            .iter()
            .any(|issue| issue.code == "invalid_path")
    );
}

#[test]
fn validator_enforces_policy_domains_and_resource_budget() {
    let catalog = builtin_tool_catalog().unwrap();
    let mut candidate = plan(tool_step("download", "core.download"));
    candidate.stages[0].steps[0].resources.max_memory_gib = 64;
    if let StepAction::Tool { arguments, .. } = &mut candidate.stages[0].steps[0].action {
        arguments["url"] = json!("https://unapproved.example/input.fastq.gz");
    }

    let result = validate_plan_v2(&candidate, &catalog);

    assert!(!result.valid);
    assert!(
        result
            .issues
            .iter()
            .any(|issue| issue.code == "resource_budget_exceeded")
    );
    assert!(
        result
            .issues
            .iter()
            .any(|issue| issue.code == "domain_not_allowed")
    );
}

#[test]
fn validator_enforces_tool_argument_contracts() {
    let catalog = builtin_tool_catalog().unwrap();
    let mut candidate = plan(tool_step("download", "core.download"));
    if let StepAction::Tool { arguments, .. } = &mut candidate.stages[0].steps[0].action {
        *arguments = json!({"url": 42, "unexpected": "value"});
    }
    let result = validate_plan_v2(&candidate, &catalog);
    assert!(
        result
            .issues
            .iter()
            .any(|issue| issue.code == "missing_argument")
    );
    assert!(
        result
            .issues
            .iter()
            .any(|issue| issue.code == "invalid_argument_type")
    );
    assert!(
        result
            .issues
            .iter()
            .any(|issue| issue.code == "unknown_argument")
    );
}

#[test]
fn every_declared_artifact_requires_a_path_verification() {
    let catalog = builtin_tool_catalog().unwrap();
    let mut candidate = plan(tool_step("download", "core.download"));
    candidate.stages[0].steps[0]
        .expected_artifacts
        .push("results/unverified.tsv".into());
    let result = validate_plan_v2(&candidate, &catalog);
    assert!(
        result
            .issues
            .iter()
            .any(|issue| issue.code == "artifact_unverified")
    );
}

#[test]
fn tool_path_arguments_cannot_traverse_outside_the_project() {
    let catalog = builtin_tool_catalog().unwrap();
    let mut candidate = plan(tool_step("download", "core.download"));
    if let StepAction::Tool { arguments, .. } = &mut candidate.stages[0].steps[0].action {
        arguments["output"] = json!("../../outside.fastq.gz");
    }
    let result = validate_plan_v2(&candidate, &catalog);
    assert!(
        result
            .issues
            .iter()
            .any(|issue| issue.code == "invalid_argument_path")
    );
}

#[test]
fn canonical_hash_is_stable_for_equivalent_json_object_order() {
    let mut first = plan(tool_step("download", "core.download"));
    let mut second = first.clone();
    if let StepAction::Tool { arguments, .. } = &mut first.stages[0].steps[0].action {
        *arguments =
            json!({"url": "https://example.org/input.fastq.gz", "output": "work/input.fastq.gz"});
    }
    if let StepAction::Tool { arguments, .. } = &mut second.stages[0].steps[0].action {
        *arguments = serde_json::from_str(
            r#"{"output":"work/input.fastq.gz","url":"https://example.org/input.fastq.gz"}"#,
        )
        .unwrap();
    }

    assert_eq!(
        canonical_plan_hash(&first).unwrap(),
        canonical_plan_hash(&second).unwrap()
    );
}

#[test]
fn legacy_shell_requires_an_explicit_policy_envelope() {
    let catalog = ToolCatalog::new(Vec::new()).unwrap();
    let mut step = tool_step("legacy", "core.download");
    step.action = StepAction::LegacyShell {
        command: "python scripts/custom.py".into(),
    };
    let candidate = plan(step);

    let result = validate_plan_v2(&candidate, &catalog);

    assert!(!result.valid);
    assert!(
        result
            .issues
            .iter()
            .any(|issue| issue.code == "legacy_shell_not_allowed")
    );
}

#[test]
fn v1_plan_migrates_to_exact_legacy_shell_actions_requiring_reapproval() {
    use omicsops_core::{
        domain::{AnalysisPlan, StageSpec, StepSpec},
        plan_v2::migrate_v1_plan,
    };
    let command = "python scripts/analyze.py --input 'data/a file.h5ad'";
    let legacy = AnalysisPlan {
        id: Uuid::new_v4(),
        title: "legacy".into(),
        summary: "legacy".into(),
        stages: vec![StageSpec {
            id: "analysis".into(),
            goal: "analysis".into(),
            dependencies: vec![],
            steps: vec![StepSpec {
                id: "run".into(),
                title: "run".into(),
                rationale: "legacy".into(),
                command: command.into(),
                working_directory: "work".into(),
                timeout_seconds: 60,
                risk: StepRisk::Low,
                completion_conditions: vec![],
                expected_artifacts: vec![],
            }],
            expected_artifacts: vec![],
            completion_conditions: vec![],
        }],
        resource_budget: ResourceLimits::default(),
        highest_risk: StepRisk::Low,
        approved: true,
    };

    let migrated = migrate_v1_plan(&legacy);
    assert!(
        matches!(&migrated.stages[0].steps[0].action, StepAction::LegacyShell { command: actual } if actual == command)
    );
    assert!(migrated.policy.allow_legacy_shell);
    assert_eq!(migrated.metadata["approval_status"], "reapproval_required");
}
