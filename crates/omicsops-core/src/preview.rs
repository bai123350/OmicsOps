#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreviewKind {
    Pdf,
    Markdown,
    Code,
    Table,
    Image,
    Html,
    Json,
    Log,
    Notebook,
    H5adMetadata,
    SeuratMetadata,
    OnDemand,
    Unsupported,
}

pub fn classify_preview(path: &str, size_bytes: u64) -> PreviewKind {
    if size_bytes > 50 * 1024 * 1024 {
        return PreviewKind::OnDemand;
    }
    let extension = path
        .rsplit('.')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    match extension.as_str() {
        "pdf" => PreviewKind::Pdf,
        "md" | "markdown" => PreviewKind::Markdown,
        "py" | "r" | "rs" | "ts" | "tsx" | "js" | "sh" => PreviewKind::Code,
        "csv" | "tsv" => PreviewKind::Table,
        "png" | "jpg" | "jpeg" | "svg" | "webp" => PreviewKind::Image,
        "html" | "htm" => PreviewKind::Html,
        "json" => PreviewKind::Json,
        "log" | "txt" => PreviewKind::Log,
        "ipynb" => PreviewKind::Notebook,
        "h5ad" => PreviewKind::H5adMetadata,
        "rds" | "h5seurat" => PreviewKind::SeuratMetadata,
        _ => PreviewKind::Unsupported,
    }
}
