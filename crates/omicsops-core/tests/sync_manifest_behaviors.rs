use omicsops_core::sync::{SyncManifest, SyncSelectionError};

#[test]
fn only_explicit_relative_files_enter_an_upload_manifest() {
    let manifest =
        SyncManifest::for_uploads("project-1", ["data/counts.tsv", "analysis/qc.py"]).unwrap();
    assert_eq!(manifest.paths(), vec!["analysis/qc.py", "data/counts.tsv"]);
    assert!(!manifest.includes("notes/private.md"));
}

#[test]
fn upload_selection_rejects_absolute_traversal_and_metadata_paths() {
    for unsafe_path in [
        "../secret",
        "data/../../secret",
        "/etc/passwd",
        "C:\\secret.txt",
        ".omicsops/project.json",
    ] {
        assert_eq!(
            SyncManifest::for_uploads("project-1", [unsafe_path]).unwrap_err(),
            SyncSelectionError::UnsafePath(unsafe_path.into())
        );
    }
}

#[test]
fn duplicate_selections_are_normalized_without_widening_scope() {
    let manifest =
        SyncManifest::for_uploads("project-1", ["data/a.csv", "data\\a.csv", "./data/a.csv"])
            .unwrap();
    assert_eq!(manifest.paths(), vec!["data/a.csv"]);
}
