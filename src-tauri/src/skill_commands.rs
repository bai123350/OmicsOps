use std::path::Path;

use omicsops_adapters::{
    persistence::Repository,
    skills::{InstalledSkillPackage, install_skill_directory},
};
use omicsops_core::workspace::SkillPackage;
use serde::Deserialize;
use tauri::State;
use uuid::Uuid;

use crate::commands::AppState;

const BUILTIN_SKILLS: &[(&str, &str)] = &[
    (
        "scrna-qc",
        "---\nname: scrna-qc\nversion: 1.0.0\ncapabilities:\n  - read_project_files\n  - submit_remote_job\n---\n# Single-cell RNA-seq QC\n\nReview study design, preserve raw counts, quantify cell and gene QC, and require an approved versioned plan before remote execution.\n",
    ),
    (
        "bulk-rnaseq-de",
        "---\nname: bulk-rnaseq-de\nversion: 1.0.0\ncapabilities:\n  - read_project_files\n  - submit_remote_job\n---\n# Bulk RNA-seq differential expression\n\nValidate sample metadata and contrasts, retain count-scale provenance, and report effect sizes with multiple-testing correction.\n",
    ),
    (
        "literature-review",
        "---\nname: literature-review\nversion: 1.0.0\ncapabilities:\n  - query_research_sources\n  - write_project_files\n---\n# Traceable literature review\n\nRecord every query, source identifier, retrieval time, inclusion decision, and citation in the project notebook.\n",
    ),
];

#[derive(Debug, Clone, Deserialize)]
pub struct ImportSkillRequest {
    pub source_path: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SetSkillEnabledRequest {
    pub skill_id: Uuid,
    pub enabled: bool,
}

pub fn install_builtin_skills(repository: &Repository, skills_root: &Path) -> Result<(), String> {
    let sources = skills_root.join(".builtin-sources");
    std::fs::create_dir_all(&sources).map_err(|error| error.to_string())?;
    for (directory, contents) in BUILTIN_SKILLS {
        let source = sources.join(directory);
        std::fs::create_dir_all(&source).map_err(|error| error.to_string())?;
        std::fs::write(source.join("SKILL.md"), contents).map_err(|error| error.to_string())?;
        let installed =
            install_skill_directory(&source, skills_root).map_err(|error| error.to_string())?;
        persist_installed(repository, installed, true)?;
    }
    Ok(())
}

#[tauri::command]
pub fn list_skill_packages(state: State<'_, AppState>) -> Result<Vec<SkillPackage>, String> {
    state
        .repository
        .list_skill_packages()
        .map_err(|error| error.to_string())
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
    let package = SkillPackage {
        id: Uuid::new_v4(),
        name: installed.name,
        version: installed.version,
        source_path: installed.install_path.to_string_lossy().into_owned(),
        sha256: installed.sha256,
        enabled: enabled_by_default,
        capabilities: installed.capabilities,
    };
    repository
        .save_skill_package(&package)
        .map_err(|error| error.to_string())?;
    Ok(package)
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
