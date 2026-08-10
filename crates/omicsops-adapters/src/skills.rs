use std::path::{Component, Path, PathBuf};

use crate::{AdapterError, AdapterResult};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillEntry {
    pub path: String,
    pub symlink_target: Option<String>,
}

impl SkillEntry {
    pub fn file(path: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            symlink_target: None,
        }
    }

    pub fn symlink(path: impl Into<String>, target: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            symlink_target: Some(target.into()),
        }
    }
}

pub fn validate_skill_entries(entries: &[SkillEntry]) -> AdapterResult<()> {
    if !entries
        .iter()
        .any(|entry| entry.path.replace('\\', "/") == "SKILL.md")
    {
        return Err(AdapterError::InvalidInput(
            "skill package is missing SKILL.md".into(),
        ));
    }
    for entry in entries {
        let path = safe_relative_path(&entry.path)?;
        if let Some(target) = &entry.symlink_target {
            let target = target.replace('\\', "/");
            if target.starts_with('/') || target.contains(':') {
                return Err(AdapterError::InvalidInput(format!(
                    "unsafe skill symlink target: {target}"
                )));
            }
            let parent = path.parent().unwrap_or_else(|| Path::new(""));
            normalize_inside_root(&parent.join(target))?;
        }
    }
    Ok(())
}

fn safe_relative_path(value: &str) -> AdapterResult<PathBuf> {
    let value = value.replace('\\', "/");
    if value.starts_with('/') || value.contains(':') {
        return Err(AdapterError::InvalidInput(format!(
            "unsafe skill path: {value}"
        )));
    }
    normalize_inside_root(Path::new(&value))
}

fn normalize_inside_root(path: &Path) -> AdapterResult<PathBuf> {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => normalized.push(part),
            Component::CurDir => {}
            Component::ParentDir => {
                if !normalized.pop() {
                    return Err(AdapterError::InvalidInput(format!(
                        "path escapes skill package: {}",
                        path.display()
                    )));
                }
            }
            Component::RootDir | Component::Prefix(_) => {
                return Err(AdapterError::InvalidInput(format!(
                    "absolute skill path: {}",
                    path.display()
                )));
            }
        }
    }
    if normalized.as_os_str().is_empty() {
        return Err(AdapterError::InvalidInput("empty skill path".into()));
    }
    Ok(normalized)
}
