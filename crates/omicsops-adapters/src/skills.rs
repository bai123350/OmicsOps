use std::{
    collections::BTreeSet,
    fs,
    io::Read,
    path::{Component, Path, PathBuf},
};

use sha2::{Digest, Sha256};

use crate::{AdapterError, AdapterResult};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillEntry {
    pub path: String,
    pub symlink_target: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InspectedSkillPackage {
    pub name: String,
    pub version: String,
    pub sha256: String,
    pub capabilities: Vec<String>,
    pub files: usize,
    pub total_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledSkillPackage {
    pub name: String,
    pub version: String,
    pub sha256: String,
    pub capabilities: Vec<String>,
    pub install_path: PathBuf,
    /// True only when this invocation completed the final rename into the
    /// managed skill root. Callers must not infer ownership from `exists()`.
    pub created_new_directory: bool,
}

#[derive(Debug, Clone)]
struct PackageFile {
    relative: PathBuf,
    source: PathBuf,
    size_bytes: u64,
}

const MAX_SKILL_FILES: usize = 4_096;
const MAX_SKILL_BYTES: u64 = 100 * 1024 * 1024;
const ALLOWED_CAPABILITIES: &[&str] = &[
    "read_project_files",
    "write_project_files",
    "query_research_sources",
    "run_local_process",
    "submit_remote_job",
];

#[cfg(test)]
mod frontmatter_tests {
    use super::*;

    #[test]
    fn nested_service_metadata_does_not_override_skill_identity_or_capabilities() {
        let markdown = "---\nname: literature-review\nversion: 1.2.3\nmetadata:\n  name: OpenAlex\n  version: service-v1\n  capabilities: [not-a-host-grant]\ncapabilities:\n  - read_project_files\n---\n# Review\n";
        let (name, version, capabilities) = parse_skill_frontmatter(markdown).unwrap();
        assert_eq!(name, "literature-review");
        assert_eq!(version.as_deref(), Some("1.2.3"));
        assert_eq!(capabilities, ["read_project_files"]);
    }
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

pub fn inspect_skill_directory(source: &Path) -> AdapterResult<InspectedSkillPackage> {
    let root = source.canonicalize()?;
    if !root.is_dir() {
        return Err(AdapterError::InvalidInput(
            "skill source is not a directory".into(),
        ));
    }
    let files = collect_package_files(&root)?;
    let entries = files
        .iter()
        .map(|file| SkillEntry::file(file.relative.to_string_lossy()))
        .collect::<Vec<_>>();
    validate_skill_entries(&entries)?;
    let instruction = fs::read_to_string(root.join("SKILL.md"))?;
    let (name, explicit_version, capabilities) = parse_skill_frontmatter(&instruction)?;
    let sha256 = hash_package_files(&files)?;
    let version = explicit_version.unwrap_or_else(|| format!("0+{}", &sha256[..12]));
    Ok(InspectedSkillPackage {
        name,
        version,
        sha256,
        capabilities,
        files: files.len(),
        total_bytes: files.iter().map(|file| file.size_bytes).sum(),
    })
}

pub fn install_skill_directory(
    source: &Path,
    destination_root: &Path,
) -> AdapterResult<InstalledSkillPackage> {
    let inspection = inspect_skill_directory(source)?;
    let source = source.canonicalize()?;
    let files = collect_package_files(&source)?;
    fs::create_dir_all(destination_root)?;
    let destination_root = destination_root.canonicalize()?;
    let package_root = destination_root
        .join(&inspection.name)
        .join(&inspection.sha256);
    if package_root.exists() {
        let resolved = package_root.canonicalize()?;
        if !resolved.starts_with(&destination_root) || !resolved.is_dir() {
            return Err(AdapterError::InvalidInput(
                "installed skill path escapes the skill store".into(),
            ));
        }
        let stored = inspect_skill_directory(&resolved)?;
        if stored.sha256 != inspection.sha256 {
            return Err(AdapterError::Integrity {
                expected: inspection.sha256,
                received: stored.sha256,
            });
        }
        return Ok(installed_from_inspection(inspection, resolved, false));
    }
    let parent = package_root
        .parent()
        .ok_or_else(|| AdapterError::InvalidInput("skill destination has no parent".into()))?;
    fs::create_dir_all(parent)?;
    let temporary = parent.join(format!(".{}.importing", inspection.sha256));
    if temporary.exists() {
        fs::remove_dir_all(&temporary)?;
    }
    let copy_result = (|| -> AdapterResult<()> {
        fs::create_dir(&temporary)?;
        for file in &files {
            let destination = temporary.join(&file.relative);
            if let Some(parent) = destination.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(&file.source, destination)?;
        }
        let copied = inspect_skill_directory(&temporary)?;
        if copied.sha256 != inspection.sha256 {
            return Err(AdapterError::Integrity {
                expected: inspection.sha256.clone(),
                received: copied.sha256,
            });
        }
        fs::rename(&temporary, &package_root)?;
        Ok(())
    })();
    if copy_result.is_err() {
        let _ = fs::remove_dir_all(&temporary);
    }
    copy_result?;
    Ok(installed_from_inspection(inspection, package_root, true))
}

fn installed_from_inspection(
    inspection: InspectedSkillPackage,
    install_path: PathBuf,
    created_new_directory: bool,
) -> InstalledSkillPackage {
    InstalledSkillPackage {
        name: inspection.name,
        version: inspection.version,
        sha256: inspection.sha256,
        capabilities: inspection.capabilities,
        install_path,
        created_new_directory,
    }
}

fn collect_package_files(root: &Path) -> AdapterResult<Vec<PackageFile>> {
    fn visit(root: &Path, directory: &Path, files: &mut Vec<PackageFile>) -> AdapterResult<()> {
        let mut entries = fs::read_dir(directory)?.collect::<Result<Vec<_>, _>>()?;
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            let path = entry.path();
            let metadata = fs::symlink_metadata(&path)?;
            if metadata.file_type().is_symlink() {
                let resolved = path.canonicalize()?;
                if !resolved.starts_with(root) || !resolved.is_file() {
                    return Err(AdapterError::InvalidInput(format!(
                        "skill symlink escapes the package or targets a directory: {}",
                        path.display()
                    )));
                }
                push_package_file(root, &path, resolved, files)?;
            } else if metadata.is_dir() {
                visit(root, &path, files)?;
            } else if metadata.is_file() {
                push_package_file(root, &path, path.clone(), files)?;
            } else {
                return Err(AdapterError::InvalidInput(format!(
                    "unsupported skill filesystem entry: {}",
                    path.display()
                )));
            }
        }
        Ok(())
    }

    let mut files = Vec::new();
    visit(root, root, &mut files)?;
    files.sort_by(|left, right| left.relative.cmp(&right.relative));
    Ok(files)
}

fn push_package_file(
    root: &Path,
    display_path: &Path,
    source: PathBuf,
    files: &mut Vec<PackageFile>,
) -> AdapterResult<()> {
    if files.len() >= MAX_SKILL_FILES {
        return Err(AdapterError::InvalidInput(
            "skill package has too many files".into(),
        ));
    }
    let relative = display_path
        .strip_prefix(root)
        .map_err(|_| AdapterError::InvalidInput("skill file escaped package root".into()))?;
    let relative = safe_relative_path(&relative.to_string_lossy())?;
    let size_bytes = source.metadata()?.len();
    let accumulated: u64 = files.iter().map(|file| file.size_bytes).sum();
    if accumulated.saturating_add(size_bytes) > MAX_SKILL_BYTES {
        return Err(AdapterError::InvalidInput(
            "skill package exceeds 100 MiB".into(),
        ));
    }
    files.push(PackageFile {
        relative,
        source,
        size_bytes,
    });
    Ok(())
}

fn hash_package_files(files: &[PackageFile]) -> AdapterResult<String> {
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; 1024 * 1024];
    for file in files {
        let relative = file.relative.to_string_lossy().replace('\\', "/");
        hasher.update((relative.len() as u64).to_le_bytes());
        hasher.update(relative.as_bytes());
        hasher.update(file.size_bytes.to_le_bytes());
        let mut input = fs::File::open(&file.source)?;
        loop {
            let read = input.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            hasher.update(&buffer[..read]);
        }
    }
    Ok(hex::encode(hasher.finalize()))
}

fn parse_skill_frontmatter(contents: &str) -> AdapterResult<(String, Option<String>, Vec<String>)> {
    let mut lines = contents.lines();
    if lines.next().map(str::trim) != Some("---") {
        return Err(AdapterError::InvalidInput(
            "SKILL.md must start with YAML frontmatter".into(),
        ));
    }
    let mut name = None;
    let mut version = None;
    let mut capabilities = BTreeSet::new();
    let mut reading_capabilities = false;
    for raw in lines {
        let line = raw.trim();
        if line == "---" {
            break;
        }
        // Package identity and grants are top-level frontmatter fields. Nested
        // third-party metadata can also contain name/version/capabilities.
        let field = if raw.starts_with(char::is_whitespace) {
            ""
        } else {
            line
        };
        if let Some(value) = field.strip_prefix("name:") {
            name = Some(unquote(value.trim()).to_owned());
            reading_capabilities = false;
        } else if let Some(value) = field.strip_prefix("version:") {
            version = Some(unquote(value.trim()).to_owned());
            reading_capabilities = false;
        } else if let Some(value) = field.strip_prefix("capabilities:") {
            reading_capabilities = true;
            let value = value.trim().trim_start_matches('[').trim_end_matches(']');
            for capability in value
                .split(',')
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                capabilities.insert(unquote(capability).to_owned());
            }
        } else if reading_capabilities {
            if let Some(value) = line.strip_prefix('-') {
                capabilities.insert(unquote(value.trim()).to_owned());
            } else if !line.is_empty() {
                reading_capabilities = false;
            }
        }
    }
    let name = name.ok_or_else(|| AdapterError::InvalidInput("skill name is required".into()))?;
    validate_metadata_token("skill name", &name)?;
    if let Some(value) = &version {
        validate_metadata_token("skill version", value)?;
    }
    for capability in &capabilities {
        if !ALLOWED_CAPABILITIES.contains(&capability.as_str()) {
            return Err(AdapterError::InvalidInput(format!(
                "unsupported skill capability: {capability}"
            )));
        }
    }
    Ok((name, version, capabilities.into_iter().collect()))
}

fn unquote(value: &str) -> &str {
    value
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .or_else(|| {
            value
                .strip_prefix('\'')
                .and_then(|value| value.strip_suffix('\''))
        })
        .unwrap_or(value)
}

fn validate_metadata_token(label: &str, value: &str) -> AdapterResult<()> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "-_.+".contains(character))
    {
        return Err(AdapterError::InvalidInput(format!(
            "invalid {label}: {value}"
        )));
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

#[cfg(test)]
mod installation_tests {
    use super::*;

    #[test]
    fn installation_reports_only_the_rename_that_created_the_managed_directory() {
        let temporary = tempfile::tempdir().unwrap();
        let source = temporary.path().join("source");
        let destination = temporary.path().join("installed");
        fs::create_dir_all(&source).unwrap();
        fs::write(
            source.join("SKILL.md"),
            "---\nname: receipt-test\n---\n# Test\n",
        )
        .unwrap();

        let first = install_skill_directory(&source, &destination).unwrap();
        let repeated = install_skill_directory(&source, &destination).unwrap();
        assert!(first.created_new_directory);
        assert!(!repeated.created_new_directory);
        assert_eq!(first.install_path, repeated.install_path);
        assert_eq!(first.sha256, repeated.sha256);
    }
}
