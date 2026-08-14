use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

use omicsops_adapters::{
    persistence::Repository,
    skills::{InstalledSkillPackage, install_skill_directory},
};
use omicsops_core::workspace::SkillPackage;
use serde::Deserialize;
use tauri::State;
use uuid::Uuid;

use crate::commands::AppState;

const LEGACY_PLACEHOLDER_SKILLS: &[&str] = &["scrna-qc", "bulk-rnaseq-de", "literature-review"];
const MAX_AGENT_SKILL_CONTEXT_BYTES: usize = 512 * 1024;

#[derive(Debug, Default, Deserialize)]
struct BundledSkillConfig {
    #[serde(default)]
    default_enabled: Vec<String>,
    #[serde(default)]
    replaces: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ImportSkillRequest {
    pub source_path: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SetSkillEnabledRequest {
    pub skill_id: Uuid,
    pub enabled: bool,
}

pub fn install_bundled_skills(
    repository: &Repository,
    skills_root: &Path,
    bundled_root: &Path,
) -> Result<(), String> {
    let sources = bundled_skill_directories(bundled_root)?;
    if sources.is_empty() {
        return Err(format!(
            "bundled skill directory contains no packages: {}",
            bundled_root.display()
        ));
    }

    let config = bundled_skill_config(bundled_root)?;
    let mut default_enabled_keys = config.default_enabled.into_iter().collect::<BTreeSet<_>>();
    for source in &sources {
        let markdown =
            std::fs::read_to_string(source.join("SKILL.md")).map_err(|error| error.to_string())?;
        if frontmatter_bool(&markdown, "workflow") {
            if let Some(key) = source.file_name().and_then(|value| value.to_str()) {
                default_enabled_keys.insert(key.to_owned());
            }
            default_enabled_keys.extend(skill_dependencies(&markdown));
        }
    }

    retire_replaced_bundled_skills(repository, &config.replaces)?;
    for source in sources {
        let key = source
            .file_name()
            .and_then(|value| value.to_str())
            .ok_or_else(|| format!("invalid bundled skill directory: {}", source.display()))?;
        let installed =
            install_skill_directory(&source, skills_root).map_err(|error| error.to_string())?;
        persist_installed(repository, installed, default_enabled_keys.contains(key))?;
    }
    Ok(())
}

fn bundled_skill_config(root: &Path) -> Result<BundledSkillConfig, String> {
    let path = root.join("BUNDLE.json");
    if !path.is_file() {
        return Ok(BundledSkillConfig::default());
    }
    let contents = std::fs::read_to_string(&path).map_err(|error| {
        format!(
            "cannot read bundled skill config {}: {error}",
            path.display()
        )
    })?;
    serde_json::from_str(&contents)
        .map_err(|error| format!("invalid bundled skill config {}: {error}", path.display()))
}

fn bundled_skill_directories(root: &Path) -> Result<Vec<PathBuf>, String> {
    let mut directories = std::fs::read_dir(root)
        .map_err(|error| format!("cannot read bundled skills {}: {error}", root.display()))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?
        .into_iter()
        .map(|entry| entry.path())
        .filter(|path| path.is_dir() && path.join("SKILL.md").is_file())
        .collect::<Vec<_>>();
    directories.sort();
    Ok(directories)
}

fn retire_replaced_bundled_skills(
    repository: &Repository,
    replacements: &[String],
) -> Result<(), String> {
    for skill in repository
        .list_skill_packages()
        .map_err(|error| error.to_string())?
    {
        let is_legacy_placeholder =
            skill.version == "1.0.0" && LEGACY_PLACEHOLDER_SKILLS.contains(&skill.name.as_str());
        if is_legacy_placeholder || replacements.contains(&skill.name) {
            repository
                .delete_skill_package(skill.id)
                .map_err(|error| error.to_string())?;
        }
    }
    Ok(())
}

#[tauri::command]
pub fn list_skill_packages(state: State<'_, AppState>) -> Result<Vec<SkillPackage>, String> {
    let mut skills = state
        .repository
        .list_skill_packages()
        .map_err(|error| error.to_string())?;
    skills.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(skills)
}

#[tauri::command]
pub fn import_skill_directory(
    state: State<'_, AppState>,
    request: ImportSkillRequest,
) -> Result<SkillPackage, String> {
    if request.source_path.trim().is_empty() {
        return Err("skill source directory is required".into());
    }
    let installed = install_skill_directory(Path::new(&request.source_path), &state.skills_root)
        .map_err(|error| error.to_string())?;
    persist_installed(&state.repository, installed, false)
}

fn persist_installed(
    repository: &Repository,
    installed: InstalledSkillPackage,
    enabled_by_default: bool,
) -> Result<SkillPackage, String> {
    if let Some(existing) = repository
        .list_skill_packages()
        .map_err(|error| error.to_string())?
        .into_iter()
        .find(|skill| skill.sha256 == installed.sha256)
    {
        return Ok(existing);
    }
    let mut package = SkillPackage {
        id: Uuid::new_v4(),
        name: installed.name,
        version: installed.version,
        source_path: installed.install_path.to_string_lossy().into_owned(),
        sha256: installed.sha256,
        enabled: false,
        capabilities: installed.capabilities,
    };
    repository
        .save_skill_package(&package)
        .map_err(|error| error.to_string())?;
    if enabled_by_default {
        package = set_skill_enabled_in_repository(repository, package.id, true)?;
    }
    Ok(package)
}

pub fn agent_skill_packages(repository: &Repository) -> Result<Vec<SkillPackage>, String> {
    let packages = repository
        .list_skill_packages()
        .map_err(|error| error.to_string())?;
    let mut selected = packages
        .iter()
        .filter(|skill| skill.enabled)
        .map(|skill| skill.id)
        .collect::<BTreeSet<_>>();

    loop {
        let mut discovered = BTreeSet::new();
        for skill in packages.iter().filter(|skill| selected.contains(&skill.id)) {
            let markdown = std::fs::read_to_string(Path::new(&skill.source_path).join("SKILL.md"))
                .map_err(|error| format!("cannot read enabled skill {}: {error}", skill.name))?;
            for dependency in skill_dependencies(&markdown) {
                if let Some(package) = packages.iter().find(|candidate| {
                    candidate.name == dependency
                        || candidate.name.ends_with(&format!("-{dependency}"))
                }) {
                    discovered.insert(package.id);
                }
            }
        }
        let previous = selected.len();
        selected.extend(discovered);
        if selected.len() == previous {
            break;
        }
    }

    let mut result = packages
        .into_iter()
        .filter(|skill| selected.contains(&skill.id))
        .collect::<Vec<_>>();
    result.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(result)
}

pub fn agent_skill_context(repository: &Repository) -> Result<String, String> {
    let packages = agent_skill_packages(repository)?;
    if packages.is_empty() {
        return Ok("No project skill package is currently enabled.".into());
    }
    let mut sections = Vec::new();
    let mut total_bytes = 0_usize;
    for skill in packages {
        let content = read_skill_package_text(&skill)?;
        total_bytes = total_bytes.saturating_add(content.len());
        if total_bytes > MAX_AGENT_SKILL_CONTEXT_BYTES {
            return Err(format!(
                "enabled Skill context exceeds {} KiB; disable unrelated packages",
                MAX_AGENT_SKILL_CONTEXT_BYTES / 1024
            ));
        }
        sections.push(format!(
            "## {} {}\nPackage SHA-256: {}\nCapabilities: {}\n{}",
            skill.name,
            skill.version,
            skill.sha256,
            skill.capabilities.join(", "),
            content
        ));
    }
    Ok(sections.join("\n\n"))
}

fn read_skill_package_text(skill: &SkillPackage) -> Result<String, String> {
    fn visit(root: &Path, directory: &Path, files: &mut Vec<PathBuf>) -> Result<(), String> {
        let mut entries = std::fs::read_dir(directory)
            .map_err(|error| error.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())?;
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            let path = entry.path();
            if path.is_dir() {
                visit(root, &path, files)?;
            } else if path.is_file() && is_agent_text_file(&path) {
                let canonical = path.canonicalize().map_err(|error| error.to_string())?;
                if !canonical.starts_with(root) {
                    return Err(format!(
                        "skill file escapes package root: {}",
                        path.display()
                    ));
                }
                files.push(canonical);
            }
        }
        Ok(())
    }

    let root = Path::new(&skill.source_path)
        .canonicalize()
        .map_err(|error| format!("cannot resolve skill {}: {error}", skill.name))?;
    let mut files = Vec::new();
    visit(&root, &root, &mut files)?;
    files.sort_by_key(|path| {
        let relative = path.strip_prefix(&root).unwrap_or(path);
        (relative != Path::new("SKILL.md"), relative.to_path_buf())
    });
    let mut documents = Vec::new();
    for path in files {
        let relative = path
            .strip_prefix(&root)
            .map_err(|error| error.to_string())?;
        let contents = std::fs::read_to_string(&path)
            .map_err(|error| format!("cannot read skill file {}: {error}", path.display()))?;
        documents.push(format!(
            "### Package file: {}\n{}",
            relative.display(),
            contents
        ));
    }
    Ok(documents.join("\n\n"))
}

fn is_agent_text_file(path: &Path) -> bool {
    path.extension()
        .and_then(|value| value.to_str())
        .is_some_and(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "md" | "py" | "r" | "sh" | "yml" | "yaml" | "toml" | "json" | "txt"
            )
        })
}

fn frontmatter_bool(markdown: &str, key: &str) -> bool {
    frontmatter_lines(markdown).any(|line| {
        line.split_once(':')
            .is_some_and(|(candidate, value)| candidate.trim() == key && value.trim() == "true")
    })
}

fn skill_dependencies(markdown: &str) -> BTreeSet<String> {
    let mut dependencies = BTreeSet::new();
    let mut reading = false;
    for line in frontmatter_lines(markdown) {
        let trimmed = line.trim();
        if trimmed == "depends_on:" {
            reading = true;
            continue;
        }
        if reading {
            if let Some(value) = trimmed.strip_prefix('-') {
                if let Some(key) = value.trim().rsplit('/').next() {
                    if !key.is_empty() {
                        dependencies.insert(key.to_owned());
                    }
                }
            } else if !trimmed.is_empty() && !line.starts_with(char::is_whitespace) {
                reading = false;
            }
        }
    }
    dependencies
}

fn frontmatter_lines(markdown: &str) -> impl Iterator<Item = &str> {
    let mut lines = markdown.lines();
    let valid = lines.next().is_some_and(|line| line.trim() == "---");
    lines.take_while(move |line| valid && line.trim() != "---")
}

#[tauri::command]
pub fn set_skill_enabled(
    state: State<'_, AppState>,
    request: SetSkillEnabledRequest,
) -> Result<SkillPackage, String> {
    set_skill_enabled_in_repository(&state.repository, request.skill_id, request.enabled)
}

pub fn set_skill_enabled_in_repository(
    repository: &Repository,
    skill_id: Uuid,
    enabled: bool,
) -> Result<SkillPackage, String> {
    let mut packages = repository
        .list_skill_packages()
        .map_err(|error| error.to_string())?;
    let index = packages
        .iter()
        .position(|skill| skill.id == skill_id)
        .ok_or_else(|| "skill package was not found".to_string())?;
    let target_name = packages[index].name.clone();
    if enabled {
        for skill in packages
            .iter_mut()
            .filter(|skill| skill.name == target_name)
        {
            if skill.enabled {
                skill.enabled = false;
                repository
                    .save_skill_package(skill)
                    .map_err(|error| error.to_string())?;
            }
        }
    }
    packages[index].enabled = enabled;
    repository
        .save_skill_package(&packages[index])
        .map_err(|error| error.to_string())?;
    Ok(packages[index].clone())
}
