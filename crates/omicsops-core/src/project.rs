use std::path::{Component, Path};

use crate::{CoreError, CoreResult};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteProjectLayout {
    root: String,
}

impl RemoteProjectLayout {
    pub fn new(root: impl Into<String>) -> Self {
        let root = root.into();
        Self {
            root: root.trim_end_matches('/').to_owned(),
        }
    }

    pub fn root(&self) -> &str {
        &self.root
    }

    pub fn control_dir(&self) -> String {
        format!("{}/.omicsops", self.root)
    }

    pub fn state_dir(&self) -> String {
        format!("{}/.omicsops/state", self.root)
    }

    pub fn results_dir(&self) -> String {
        format!("{}/results", self.root)
    }

    pub fn required_directories(&self) -> Vec<String> {
        [
            self.control_dir(),
            self.state_dir(),
            format!("{}/scripts", self.root),
            format!("{}/logs", self.root),
            format!("{}/work", self.root),
            self.results_dir(),
        ]
        .into_iter()
        .collect()
    }

    pub fn initialization_command(&self) -> String {
        let directories = self
            .required_directories()
            .into_iter()
            .map(|path| shell_quote(&path))
            .collect::<Vec<_>>()
            .join(" ");
        format!("umask 077 && mkdir -p {directories}")
    }
}

pub fn validate_relative_remote_path(path: &Path) -> CoreResult<()> {
    let raw = path.to_string_lossy();
    if raw.is_empty()
        || raw.starts_with('/')
        || raw.starts_with('\\')
        || raw.contains('\\')
        || raw.contains('\0')
        || path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(CoreError::InvalidRemotePath(raw.into_owned()));
    }
    Ok(())
}

pub fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}
