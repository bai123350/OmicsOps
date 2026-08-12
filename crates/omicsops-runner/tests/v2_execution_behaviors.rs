use omicsops_core::{
    domain::{ResourceLimits, StepRisk},
    plan_v2::{StepAction, StepSpecV2, VerificationSpec},
    tools::builtin_tool_catalog,
};
use omicsops_runner::{
    compile_step_action, reconcile_manifest, render_cancel_command, render_remote_step_script_v2,
    render_verification_command,
};
use serde_json::json;

fn step(action: StepAction) -> StepSpecV2 {
    StepSpecV2 {
        id: "qc".into(),
        title: "QC".into(),
        rationale: "verify input".into(),
        dependencies: vec![],
        action,
        working_directory: "work".into(),
        resources: ResourceLimits {
            max_cpu_cores: 3,
            max_memory_gib: 5,
            max_disk_gib: 10,
            max_step_seconds: 600,
        },
        risk: StepRisk::Low,
        verifications: vec![VerificationSpec::File {
            path: "results/qc.html".into(),
            min_bytes: 100,
            sha256: None,
        }],
        expected_artifacts: vec!["results/qc.html".into()],
    }
}

#[test]
fn recovery_rejects_missing_tampered_or_mismatched_manifests() {
    assert!(reconcile_manifest("", "expected").is_err());
    assert!(reconcile_manifest("not-json", "expected").is_err());
    assert!(
        reconcile_manifest(
            r#"{"action_hash":"other","status":"exited","exit_code":0}"#,
            "expected"
        )
        .is_err()
    );
    assert!(
        reconcile_manifest(
            r#"{"action_hash":"expected","status":"exited","exit_code":1}"#,
            "expected"
        )
        .is_err()
    );
    assert!(
        reconcile_manifest(
            r#"{"action_hash":"expected","status":"exited","exit_code":0}"#,
            "expected"
        )
        .is_ok()
    );
}

#[test]
fn structured_arguments_are_quoted_as_individual_shell_arguments() {
    let catalog = builtin_tool_catalog().unwrap();
    let action = StepAction::Tool {
        tool_id: "bio.fastqc".into(),
        version: "1.0.0".into(),
        arguments: json!({"input": "reads/a.fastq; touch /tmp/pwned", "threads": 3}),
    };

    let command = compile_step_action(&action, &catalog).unwrap();

    assert!(command.starts_with("'fastqc'"));
    assert!(command.contains("'reads/a.fastq; touch /tmp/pwned'"));
    assert!(!command.contains("; touch /tmp/pwned' --"));
}

#[test]
fn scanpy_structured_plan_compiles_to_the_bundled_remote_workflow() {
    let catalog = builtin_tool_catalog().unwrap();
    let action = StepAction::Tool {
        tool_id: "bio.scanpy".into(),
        version: "1.0.0".into(),
        arguments: json!({
            "input_directory": "data/filtered_gene_bc_matrices/hg19",
            "qc": {"min_genes": 200, "max_mito_percent": 20},
            "outputs": {
                "h5ad": "results/scanpy/annotated_qc.h5ad",
                "qc_metrics": "results/scanpy/qc_metrics.tsv",
                "cluster_annotations": "results/scanpy/cluster_annotations.tsv",
                "umap": "results/scanpy/umap.png",
                "qc_plots": "results/scanpy/qc_plots.png",
                "marker_scores": "results/scanpy/marker_scores.tsv"
            }
        }),
    };

    let command = compile_step_action(&action, &catalog).unwrap();
    assert!(command.contains("micromamba run --prefix"));
    assert!(command.contains("sc.read_10x_mtx"));
    assert!(command.contains("data/filtered_gene_bc_matrices/hg19"));
    assert!(!command.contains("../"));
}

#[test]
fn structured_html_report_compiles_to_the_bundled_renderer() {
    let catalog = builtin_tool_catalog().unwrap();
    let action = StepAction::Tool {
        tool_id: "report.html".into(),
        version: "1.0.0".into(),
        arguments: json!({
            "inputs": ["results/qc.tsv", "results/umap.png"],
            "output_path": "results/report.html",
            "sections": ["QC", "clusters"],
            "title": "PBMC report"
        }),
    };
    let command = compile_step_action(&action, &catalog).unwrap();
    assert!(command.contains("Verified analysis outputs"));
    assert!(command.contains("results/report.html"));
}

#[test]
fn v2_script_starts_a_process_group_and_applies_resource_controls() {
    let catalog = builtin_tool_catalog().unwrap();
    let script = render_remote_step_script_v2(
        "/srv/project",
        &step(StepAction::Tool {
            tool_id: "bio.fastqc".into(),
            version: "1.0.0".into(),
            arguments: json!({"input": "reads/a.fastq"}),
        }),
        1,
        &catalog,
    )
    .unwrap();

    assert!(script.contains("setsid"));
    assert!(script.contains("prlimit --as="));
    assert!(script.contains("OMP_NUM_THREADS=3"));
    assert!(script.contains(".pgid"));
    assert!(script.contains("manifest.json"));
    assert!(script.contains("realpath -m"));
    assert!(script.contains("path escapes project root"));
    assert!(script.contains("du -sk"));
    assert!(script.contains("disk budget exceeded"));
}

#[test]
fn cancellation_targets_the_negative_process_group_id() {
    let command = render_cancel_command("/srv/project", "qc", 1, false);
    assert!(command.contains("kill -TERM -- -\"$(cat"));
    assert!(command.contains(".pgid"));
}

#[test]
fn verification_commands_cover_files_hash_json_tables_and_domains() {
    let specs = [
        VerificationSpec::File {
            path: "a.txt".into(),
            min_bytes: 2,
            sha256: Some("abc".into()),
        },
        VerificationSpec::JsonField {
            path: "a.json".into(),
            pointer: "/ok".into(),
            expected: "true".into(),
        },
        VerificationSpec::Table {
            path: "a.tsv".into(),
            delimiter: '\t',
            required_columns: vec!["gene".into()],
            min_rows: 2,
        },
        VerificationSpec::DomainReport {
            path: "a.h5ad".into(),
            validator: "h5ad".into(),
        },
    ];

    let commands = specs
        .iter()
        .map(render_verification_command)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert!(commands[0].contains("sha256sum"));
    assert!(commands[1].contains("json_pointer"));
    assert!(commands[2].contains("csv.reader"));
    assert!(commands[3].contains("anndata.read_h5ad"));
}
