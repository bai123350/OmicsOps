use omicsops_core::domain::{
    AnalysisPlan, CompletionCondition, CompletionConditionKind, ResourceLimits, StageSpec,
    StepRisk, StepSpec,
};
use uuid::Uuid;

pub const PBMC_ENVIRONMENT_YAML: &str =
    include_str!("../../../workflows/scrna-pbmc/environment.yml");
pub const PBMC_SCANPY_SCRIPT: &str = include_str!("../../../workflows/scrna-pbmc/analyze_pbmc.py");
pub const SEURAT_CONVERTER_R: &str =
    include_str!("../../../workflows/scrna-pbmc/convert_h5ad_to_seurat.R");
pub const PBMC_DOWNLOAD_SCRIPT: &str =
    include_str!("../../../workflows/scrna-pbmc/download_pbmc.sh");
pub const PBMC_REPORT_SCRIPT: &str = include_str!("../../../workflows/scrna-pbmc/render_report.py");

pub fn pbmc_reference_plan() -> AnalysisPlan {
    let exists = |target: &str| CompletionCondition {
        kind: CompletionConditionKind::FileExists,
        target: target.into(),
        expected: None,
    };
    let step = |id: &str, title: &str, command: &str, expected_artifact: &str| -> StepSpec {
        StepSpec {
            id: id.into(),
            title: title.into(),
            rationale: "Reference scRNA-seq acceptance workflow".into(),
            command: command.into(),
            working_directory: ".".into(),
            timeout_seconds: 86_400,
            risk: StepRisk::Low,
            completion_conditions: vec![
                CompletionCondition {
                    kind: CompletionConditionKind::ExitCode,
                    target: "process".into(),
                    expected: Some("0".into()),
                },
                exists(expected_artifact),
            ],
            expected_artifacts: vec![expected_artifact.into()],
        }
    };

    AnalysisPlan {
        id: Uuid::new_v4(),
        title: "10x PBMC Scanpy acceptance workflow".into(),
        summary: "Download public PBMC data, run quality control and clustering, convert the h5ad to a validated Seurat RDS, and create a report.".into(),
        stages: vec![
            StageSpec {
                id: "download".into(),
                goal: "download and checksum the public 10x PBMC dataset".into(),
                dependencies: vec![],
                steps: vec![step(
                    "download-pbmc",
                    "Download PBMC data",
                    "bash scripts/download_pbmc.sh",
                    "work/pbmc3k/filtered_gene_bc_matrices/hg19/matrix.mtx",
                )],
                expected_artifacts: vec!["work/pbmc3k/filtered_gene_bc_matrices/hg19/matrix.mtx".into()],
                completion_conditions: vec![exists(
                    "work/pbmc3k/filtered_gene_bc_matrices/hg19/matrix.mtx",
                )],
            },
            StageSpec {
                id: "analysis".into(),
                goal: "run Scanpy quality control, dimensional reduction, cluster discovery, markers and annotation".into(),
                dependencies: vec!["download".into()],
                steps: vec![step(
                    "scanpy-analysis",
                    "Run Scanpy analysis",
                    "micromamba run -p .omicsops/env python scripts/analyze_pbmc.py",
                    "results/pbmc3k.h5ad",
                )],
                expected_artifacts: vec!["results/pbmc3k.h5ad".into()],
                completion_conditions: vec![exists("results/pbmc3k.h5ad")],
            },
            StageSpec {
                id: "seurat".into(),
                goal: "convert h5ad to a validated Seurat RDS with the self-authored R reader".into(),
                dependencies: vec!["analysis".into()],
                steps: vec![step(
                    "seurat-conversion",
                    "Build Seurat object",
                    "micromamba run -p .omicsops/env Rscript scripts/convert_h5ad_to_seurat.R results/pbmc3k.h5ad results/pbmc3k.seurat.rds results/seurat-validation.json",
                    "results/pbmc3k.seurat.rds",
                )],
                expected_artifacts: vec![
                    "results/pbmc3k.seurat.rds".into(),
                    "results/seurat-validation.json".into(),
                ],
                completion_conditions: vec![
                    exists("results/pbmc3k.seurat.rds"),
                    exists("results/seurat-validation.json"),
                ],
            },
            StageSpec {
                id: "report".into(),
                goal: "render the final quality and provenance report".into(),
                dependencies: vec!["seurat".into()],
                steps: vec![step(
                    "report",
                    "Render report",
                    "micromamba run -p .omicsops/env python scripts/render_report.py",
                    "results/report.html",
                )],
                expected_artifacts: vec!["results/report.html".into()],
                completion_conditions: vec![exists("results/report.html")],
            },
        ],
        resource_budget: ResourceLimits {
            max_cpu_cores: 8,
            max_memory_gib: 32,
            max_disk_gib: 50,
            max_step_seconds: 86_400,
        },
        highest_risk: StepRisk::Low,
        approved: false,
    }
}
