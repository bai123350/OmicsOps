use omicsops_core::preview::{PreviewKind, classify_preview};

#[test]
fn scientific_artifacts_choose_safe_desktop_previews() {
    assert_eq!(classify_preview("paper.pdf", 10), PreviewKind::Pdf);
    assert_eq!(classify_preview("markers.csv", 10), PreviewKind::Table);
    assert_eq!(
        classify_preview("analysis.ipynb", 10),
        PreviewKind::Notebook
    );
    assert_eq!(
        classify_preview("cells.h5ad", 10),
        PreviewKind::H5adMetadata
    );
    assert_eq!(
        classify_preview("object.rds", 10),
        PreviewKind::SeuratMetadata
    );
}

#[test]
fn large_files_remain_metadata_only_until_explicit_download() {
    assert_eq!(
        classify_preview("report.html", 60 * 1024 * 1024),
        PreviewKind::OnDemand
    );
    assert_eq!(
        classify_preview("unknown.bin", 12),
        PreviewKind::Unsupported
    );
}
