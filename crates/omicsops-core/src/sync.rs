use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum SyncSelectionError {
    #[error("unsafe sync path: {0}")]
    UnsafePath(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncManifest {
    pub project_id: String,
    selected_uploads: BTreeSet<String>,
}

impl SyncManifest {
    pub fn for_uploads<'a>(
        project_id: impl Into<String>,
        paths: impl IntoIterator<Item = &'a str>,
    ) -> Result<Self, SyncSelectionError> {
        let mut selected_uploads = BTreeSet::new();
        for original in paths {
            let normalized = original.replace('\\', "/");
            let normalized = normalized.strip_prefix("./").unwrap_or(&normalized);
            let segments: Vec<_> = normalized.split('/').collect();
            let unsafe_path = normalized.is_empty()
                || normalized.starts_with('/')
                || normalized.contains(':')
                || segments.iter().any(|segment| *segment == "..")
                || segments.first() == Some(&".omicsops");
            if unsafe_path {
                return Err(SyncSelectionError::UnsafePath(original.into()));
            }
            let clean = segments
                .into_iter()
                .filter(|segment| !segment.is_empty() && *segment != ".")
                .collect::<Vec<_>>()
                .join("/");
            if clean.is_empty() {
                return Err(SyncSelectionError::UnsafePath(original.into()));
            }
            selected_uploads.insert(clean);
        }
        Ok(Self {
            project_id: project_id.into(),
            selected_uploads,
        })
    }

    pub fn paths(&self) -> Vec<&str> {
        self.selected_uploads.iter().map(String::as_str).collect()
    }

    pub fn includes(&self, relative_path: &str) -> bool {
        self.selected_uploads.contains(relative_path)
    }
}
