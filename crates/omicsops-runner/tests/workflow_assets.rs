use omicsops_runner::scrna::{
    PBMC_ENVIRONMENT_YAML, PBMC_SCANPY_SCRIPT, SEURAT_CONVERTER_R, pbmc_reference_plan,
};

#[test]
fn reference_plan_covers_the_scrna_acceptance_stages() {
    let plan = pbmc_reference_plan();
    let goals = plan
        .stages
        .iter()
        .map(|stage| stage.goal.as_str())
        .collect::<Vec<_>>()
        .join(" ");

    for expected in ["download", "quality", "cluster", "Seurat", "report"] {
        assert!(goals.to_lowercase().contains(&expected.to_lowercase()));
    }
}

#[test]
fn scanpy_script_preserves_counts_and_writes_h5ad() {
    assert!(PBMC_SCANPY_SCRIPT.contains("layers[\"counts\"]"));
    assert!(PBMC_SCANPY_SCRIPT.contains("Scrublet"));
    assert!(PBMC_SCANPY_SCRIPT.contains("qc-prefilter.png"));
    assert!(PBMC_SCANPY_SCRIPT.contains("qc_mask & doublet_mask"));
    assert!(PBMC_SCANPY_SCRIPT.contains("cluster_annotations.tsv"));
    assert!(PBMC_SCANPY_SCRIPT.contains("marker_specificity.tsv"));
    assert!(PBMC_SCANPY_SCRIPT.contains("\"Platelets\""));
    assert!(PBMC_SCANPY_SCRIPT.contains("\"FCGR3A monocytes\""));
    assert!(PBMC_SCANPY_SCRIPT.contains("not_applied_filtered_matrix_only"));
    assert!(PBMC_SCANPY_SCRIPT.contains("rank_genes_groups"));
    assert!(PBMC_SCANPY_SCRIPT.contains("write_h5ad"));
}

#[test]
fn r_converter_is_self_authored_and_does_not_use_seuratdisk() {
    assert!(SEURAT_CONVERTER_R.contains("hdf5r"));
    assert!(SEURAT_CONVERTER_R.contains("CreateSeuratObject"));
    assert!(SEURAT_CONVERTER_R.contains("read_embedding"));
    assert!(SEURAT_CONVERTER_R.contains("readRDS"));
    assert!(!SEURAT_CONVERTER_R.to_lowercase().contains("seuratdisk"));
}

#[test]
fn environment_contains_python_and_r_validation_dependencies() {
    for dependency in [
        "scanpy",
        "scrublet",
        "leidenalg",
        "r-seurat",
        "r-hdf5r",
        "r-matrix",
    ] {
        assert!(PBMC_ENVIRONMENT_YAML.contains(dependency));
    }
}
