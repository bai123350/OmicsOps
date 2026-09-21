//! Local declarative plugin packages. A package contributes standalone Skills and
//! references compiled MCP presets; inspecting or installing it never executes code.

use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    fs,
    io::Read,
    path::{Component, Path, PathBuf},
};

use chrono::Utc;
use omicsops_core::workspace::SkillPackage;
use omicsops_dto::{
    InstallPluginRequest, InstalledPlugin, PluginFilePreview, PluginInspection, PluginOwnedSkill,
    PluginPhase, PluginPresetBinding, PluginRemovalResult, RemovePluginRequest,
    SetPluginEnabledRequest, SkillInstallationReceipt, SkillOrigin,
};
use omicsops_store::Store;
use regex::Regex;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use tauri::State;
use uuid::Uuid;

use crate::{
    bundled_mcp_commands::{list_bundled_mcp_presets, preset_server_id},
    commands::AppState,
    p1_commands::McpServerProfile,
};

const MANIFEST_NAME: &str = "omicsops-plugin.json";
const PLUGIN_RECORD_KIND: &str = "integration_package_v1";
const MAX_MANIFEST_BYTES: u64 = 64 * 1024;
const MAX_SKILLS: usize = 20;
const MAX_PRESETS: usize = 20;
const MAX_SKILL_BYTES: u64 = 256 * 1024;
const MAX_TOTAL_BYTES: u64 = 5 * 1024 * 1024;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PluginManifest {
    schema_version: u32,
    id: String,
    version: String,
    name: String,
    skills: Vec<ManifestSkill>,
    #[serde(default)]
    mcp_presets: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ManifestSkill {
    path: String,
    sha256: String,
}

#[derive(Debug)]
struct InspectedSource {
    manifest: PluginManifest,
    inspection: PluginInspection,
    verified_files: BTreeMap<PathBuf, Vec<u8>>,
}

fn plugins_root(state: &AppState) -> Result<PathBuf, String> {
    let app_data = state
        .skills_root
        .parent()
        .ok_or_else(|| "managed skills directory has no application-data parent".to_owned())?;
    Ok(app_data.join("plugins"))
}

#[tauri::command]
pub async fn settings_inspect_plugin(
    state: State<'_, AppState>,
    source_path: String,
) -> Result<PluginInspection, String> {
    inspect_plugin_source(&state.repository, Path::new(&source_path))
        .await
        .map(|source| source.inspection)
}

#[tauri::command]
pub async fn settings_install_plugin(
    state: State<'_, AppState>,
    request: InstallPluginRequest,
) -> Result<InstalledPlugin, String> {
    let _guard = state.skills_gate.write().await;
    install_plugin(&state.repository, &plugins_root(&state)?, request).await
}

#[tauri::command]
pub async fn settings_list_plugins(
    state: State<'_, AppState>,
) -> Result<Vec<InstalledPlugin>, String> {
    list_plugins(&state.repository, false).await
}

#[tauri::command]
pub async fn settings_set_plugin_enabled(
    state: State<'_, AppState>,
    request: SetPluginEnabledRequest,
) -> Result<InstalledPlugin, String> {
    let _guard = state.skills_gate.write().await;
    set_plugin_enabled(&state.repository, request).await
}

#[tauri::command]
pub async fn settings_remove_plugin(
    state: State<'_, AppState>,
    request: RemovePluginRequest,
) -> Result<PluginRemovalResult, String> {
    let _guard = state.skills_gate.write().await;
    remove_plugin(&state.repository, &plugins_root(&state)?, request).await
}

pub(crate) async fn reconcile_plugins(
    repository: &Store,
    plugins_root: &Path,
) -> Result<(), String> {
    fs::create_dir_all(plugins_root.join("staging")).map_err(|error| error.to_string())?;
    fs::create_dir_all(plugins_root.join("installs")).map_err(|error| error.to_string())?;
    for mut plugin in list_plugins(repository, true).await? {
        if matches!(
            plugin.phase,
            PluginPhase::Staging | PluginPhase::Updating | PluginPhase::Removing
        ) {
            plugin.phase = PluginPhase::NeedsAttention;
            plugin.enabled = false;
            plugin.last_error = Some(
                "an earlier lifecycle operation was interrupted; retry it from Plugins settings"
                    .into(),
            );
            save_plugin(repository, &plugin).await?;
            for owned in &plugin.skills {
                if let Some(skill) = skill_by_id(repository, owned.skill_id).await? {
                    let _ = repository.set_skill_enabled_atomic(skill.id, false).await;
                }
            }
        }
    }
    Ok(())
}

pub(crate) async fn plugin_allows_skill(
    repository: &Store,
    skill_id: Uuid,
) -> Result<bool, String> {
    let receipt = repository
        .get_json::<SkillInstallationReceipt>(
            crate::skill_settings::SKILL_INSTALLATION_KIND,
            &skill_id.to_string(),
        )
        .await
        .map_err(|error| error.to_string())?;
    let Some(receipt) = receipt else {
        let plugin_category = skill_by_id(repository, skill_id)
            .await?
            .and_then(|skill| skill.category)
            .is_some_and(|category| category.starts_with("plugin:"));
        return Ok(!plugin_category);
    };
    if receipt.origin != SkillOrigin::PluginOwned {
        return Ok(true);
    }
    let installation_id = receipt
        .plugin_installation_id
        .ok_or_else(|| "plugin-owned skill receipt has no plugin installation".to_owned())?;
    let plugin = plugin_by_id(repository, installation_id)
        .await?
        .ok_or_else(|| "plugin-owned skill has no parent plugin record".to_owned())?;
    Ok(plugin.phase == PluginPhase::Installed
        && plugin.enabled
        && plugin.skills.iter().any(|owned| {
            owned.skill_id == skill_id && owned.package_sha256 == receipt.package_sha256
        }))
}

/// Resolve a PluginOwned Skill only after its receipt and plugin-owned path agree.
/// The enabled flag is deliberately not required so Settings can inspect disabled packages.
pub(crate) async fn plugin_owned_skill_root(
    repository: &Store,
    app_data_root: &Path,
    receipt: &SkillInstallationReceipt,
    skill: &SkillPackage,
) -> Result<PathBuf, String> {
    if receipt.origin != SkillOrigin::PluginOwned
        || receipt.skill_id != skill.id
        || receipt.package_sha256 != skill.sha256
        || !same_path(
            Path::new(&receipt.installed_root),
            Path::new(&skill.source_path),
        )
    {
        return Err("plugin-owned skill receipt does not match the catalog record".into());
    }
    let installation_id = receipt
        .plugin_installation_id
        .ok_or_else(|| "plugin-owned skill receipt has no plugin installation".to_owned())?;
    let plugin = plugin_by_id(repository, installation_id)
        .await?
        .ok_or_else(|| "plugin installation record was not found".to_owned())?;
    if matches!(plugin.phase, PluginPhase::Removed) {
        return Err("plugin installation has been removed".into());
    }
    let owned = plugin
        .skills
        .iter()
        .find(|owned| owned.skill_id == skill.id && owned.package_sha256 == skill.sha256)
        .ok_or_else(|| "plugin record does not own this skill".to_owned())?;
    let install_root =
        checked_install_root(app_data_root.join("plugins").as_path(), installation_id)?;
    let expected = install_root.join(normalize_relative_path(&owned.relative_path)?);
    if !same_path(&expected, Path::new(&skill.source_path)) {
        return Err("plugin-owned skill path does not match its manifest binding".into());
    }
    checked_existing_directory(&expected, &install_root)
}

async fn inspect_plugin_source(
    repository: &Store,
    source: &Path,
) -> Result<InspectedSource, String> {
    let source = checked_source_root(source)?;
    let manifest_path = source.join(MANIFEST_NAME);
    let manifest_bytes = read_bounded_plain_file(&manifest_path, MAX_MANIFEST_BYTES)?;
    let manifest_size = manifest_bytes.len() as u64;
    let manifest: PluginManifest = serde_json::from_slice(&manifest_bytes)
        .map_err(|error| format!("invalid {MANIFEST_NAME}: {error}"))?;
    validate_manifest(&manifest)?;

    let mut declared_files = BTreeSet::from([MANIFEST_NAME.to_owned()]);
    let mut files = vec![PluginFilePreview {
        relative_path: MANIFEST_NAME.into(),
        size_bytes: manifest_size,
        sha256: sha256_bytes(&manifest_bytes),
    }];
    let mut digest = Sha256::new();
    digest.update(b"omicsops-plugin-v1\0");
    digest.update(MANIFEST_NAME.as_bytes());
    digest.update([0]);
    digest.update(&manifest_bytes);
    let mut total = manifest_size;
    let mut verified_files =
        BTreeMap::from([(PathBuf::from(MANIFEST_NAME), manifest_bytes.clone())]);
    for declared in &manifest.skills {
        let relative = normalize_relative_path(&declared.path)?;
        let normalized = slash_path(&relative);
        if !declared_files.insert(normalized.clone()) {
            return Err(format!("plugin declares duplicate file {normalized}"));
        }
        let path = source.join(&relative);
        let bytes = read_bounded_plain_file(&path, MAX_SKILL_BYTES)
            .map_err(|error| format!("plugin skill {normalized}: {error}"))?;
        total = total.saturating_add(bytes.len() as u64);
        if total > MAX_TOTAL_BYTES {
            return Err("plugin package exceeds 5 MiB".into());
        }
        let received = sha256_bytes(&bytes);
        if received != declared.sha256 {
            return Err(format!("SHA-256 mismatch for {normalized}"));
        }
        let markdown = std::str::from_utf8(&bytes)
            .map_err(|_| format!("plugin skill {normalized} is not UTF-8"))?;
        reject_relative_markdown_references(markdown, &normalized)?;
        let skill_root = path
            .parent()
            .ok_or_else(|| format!("plugin skill {normalized} has no root"))?;
        omicsops_adapters::skills::inspect_skill_directory(skill_root)
            .map_err(|error| format!("invalid plugin skill {normalized}: {error}"))?;
        digest.update(normalized.as_bytes());
        digest.update([0]);
        digest.update(&bytes);
        files.push(PluginFilePreview {
            relative_path: normalized,
            size_bytes: bytes.len() as u64,
            sha256: received,
        });
        verified_files.insert(relative, bytes);
    }
    validate_source_inventory(&source, &declared_files)?;

    let configured = repository
        .list_json::<McpServerProfile>("mcp_server")
        .await
        .map_err(|error| error.to_string())?
        .into_iter()
        .map(|profile| (profile.id, profile))
        .collect::<BTreeMap<_, _>>();
    let bindings = manifest
        .mcp_presets
        .iter()
        .map(|preset_id| {
            let server_id = preset_server_id(preset_id);
            let profile = configured.get(&server_id);
            PluginPresetBinding {
                preset_id: preset_id.clone(),
                server_id,
                ownership: "shared_reference".into(),
                configured: profile.is_some(),
                enabled: profile.is_some_and(|profile| profile.enabled),
            }
        })
        .collect::<Vec<_>>();
    let manifest_digest = format!("{:x}", digest.finalize());
    let existing = list_plugins(repository, true)
        .await?
        .into_iter()
        .filter(|plugin| plugin.package_id == manifest.id && plugin.phase != PluginPhase::Removed)
        .max_by_key(|plugin| plugin.created_at.clone());
    let mut changes = Vec::new();
    if let Some(existing) = &existing {
        if existing.digest == manifest_digest {
            changes.push("same_content".into());
        } else {
            changes.push(format!("version:{}→{}", existing.version, manifest.version));
            if existing.skills.len() != manifest.skills.len() {
                changes.push(format!(
                    "skills:{}→{}",
                    existing.skills.len(),
                    manifest.skills.len()
                ));
            }
        }
    } else {
        changes.push("new_installation".into());
    }
    Ok(InspectedSource {
        inspection: PluginInspection {
            manifest_digest,
            source_path: source.to_string_lossy().into_owned(),
            package_id: manifest.id.clone(),
            version: manifest.version.clone(),
            name: manifest.name.clone(),
            files,
            bindings,
            existing_installation_id: existing.map(|plugin| plugin.installation_id),
            changes,
        },
        manifest,
        verified_files,
    })
}

async fn install_plugin(
    repository: &Store,
    plugins_root: &Path,
    request: InstallPluginRequest,
) -> Result<InstalledPlugin, String> {
    validate_sha256(&request.expected_digest, "expected plugin digest")?;
    let inspected = inspect_plugin_source(repository, Path::new(&request.source_path)).await?;
    if inspected.inspection.manifest_digest != request.expected_digest {
        return Err("plugin source changed after inspection; inspect it again".into());
    }
    let existing = list_plugins(repository, true)
        .await?
        .into_iter()
        .filter(|plugin| {
            plugin.package_id == inspected.manifest.id && plugin.phase != PluginPhase::Removed
        })
        .max_by_key(|plugin| plugin.created_at.clone());
    if let Some(existing) = &existing {
        if existing.digest == request.expected_digest && existing.phase == PluginPhase::Installed {
            return Ok(existing.clone());
        }
        if existing.digest == request.expected_digest
            && existing.phase == PluginPhase::NeedsAttention
            && !existing.cleanup_pending
        {
            if request.expected_old_digest.as_deref() != Some(existing.digest.as_str()) {
                return Err("plugin recovery requires the interrupted installation digest".into());
            }
            return resume_plugin_registration(repository, plugins_root, existing.clone()).await;
        }
        if request.expected_old_digest.as_deref() != Some(existing.digest.as_str()) {
            return Err("plugin update requires the currently installed digest".into());
        }
    } else if request.expected_old_digest.is_some() {
        return Err("plugin update target was not found".into());
    }

    fs::create_dir_all(plugins_root.join("staging")).map_err(|error| error.to_string())?;
    fs::create_dir_all(plugins_root.join("installs")).map_err(|error| error.to_string())?;
    ensure_plain_directory(&plugins_root.join("staging"))?;
    ensure_plain_directory(&plugins_root.join("installs"))?;
    let installation_id = Uuid::new_v4();
    let stage = plugins_root
        .join("staging")
        .join(installation_id.to_string());
    let install = plugins_root
        .join("installs")
        .join(installation_id.to_string());
    fs::create_dir(&stage).map_err(|error| error.to_string())?;
    let copied = copy_declared_package(&inspected, &stage);
    if let Err(error) = copied {
        let _ = cleanup_partial_stage(&stage, &inspected.verified_files);
        return Err(error);
    }
    let staged = inspect_plugin_source(repository, &stage).await?;
    if staged.inspection.manifest_digest != request.expected_digest {
        let _ = cleanup_partial_stage(&stage, &inspected.verified_files);
        return Err("staged plugin digest does not match the inspected source".into());
    }
    fs::rename(&stage, &install).map_err(|error| error.to_string())?;
    let prepared = prepare_plugin_skills(&install, &inspected.manifest)?;
    let mut plugin = InstalledPlugin {
        installation_id,
        package_id: inspected.manifest.id.clone(),
        version: inspected.manifest.version.clone(),
        name: inspected.manifest.name.clone(),
        digest: request.expected_digest,
        source_path: inspected.inspection.source_path,
        trust: "local_unverified".into(),
        enabled: false,
        phase: if existing.is_some() {
            PluginPhase::Updating
        } else {
            PluginPhase::Staging
        },
        cleanup_pending: false,
        predecessor_installation_id: existing.as_ref().map(|plugin| plugin.installation_id),
        files: staged.inspection.files.clone(),
        skills: prepared.iter().map(|(_, owned)| owned.clone()).collect(),
        mcp_bindings: staged.inspection.bindings,
        last_error: None,
        created_at: Utc::now().to_rfc3339(),
    };
    save_plugin(repository, &plugin).await?;

    let registration = register_plugin_skills(repository, &prepared, installation_id).await;
    match registration {
        Ok(()) => {}
        Err(error) => {
            plugin.phase = PluginPhase::NeedsAttention;
            plugin.last_error = Some(error.clone());
            save_plugin(repository, &plugin).await?;
            return Err(error);
        }
    }
    if let Err(error) = retire_predecessor(repository, &plugin).await {
        plugin.phase = PluginPhase::NeedsAttention;
        plugin.last_error = Some(error.clone());
        save_plugin(repository, &plugin).await?;
        return Err(error);
    }
    plugin.phase = PluginPhase::Installed;
    save_plugin(repository, &plugin).await?;
    Ok(plugin)
}

async fn resume_plugin_registration(
    repository: &Store,
    plugins_root: &Path,
    mut plugin: InstalledPlugin,
) -> Result<InstalledPlugin, String> {
    let install_root = checked_install_root(plugins_root, plugin.installation_id)?;
    let verified = inspect_plugin_source(repository, &install_root).await?;
    if verified.inspection.manifest_digest != plugin.digest {
        return Err(
            "interrupted plugin files changed; remove or inspect the package before retrying"
                .into(),
        );
    }
    if plugin.skills.len() != verified.manifest.skills.len() {
        return Err("interrupted plugin ownership journal is incomplete".into());
    }
    for owned in &plugin.skills {
        let root = checked_existing_directory(
            &install_root.join(normalize_relative_path(&owned.relative_path)?),
            &install_root,
        )?;
        let inspection = omicsops_adapters::skills::inspect_skill_directory(&root)
            .map_err(|error| error.to_string())?;
        if inspection.sha256 != owned.package_sha256 || inspection.name != owned.name {
            return Err(format!("interrupted plugin Skill {} changed", owned.name));
        }
        let skill = SkillPackage {
            id: owned.skill_id,
            name: owned.name.clone(),
            version: inspection.version,
            source_path: root.to_string_lossy().into_owned(),
            sha256: inspection.sha256,
            enabled: false,
            capabilities: inspection.capabilities,
            category: Some(format!("plugin:{}", plugin.package_id)),
        };
        repository
            .save_skill_package(&skill)
            .await
            .map_err(|error| error.to_string())?;
        crate::skill_settings::record_plugin_owned_receipt(
            repository,
            &skill,
            plugin.installation_id,
        )
        .await?;
    }
    retire_predecessor(repository, &plugin).await?;
    plugin.phase = PluginPhase::Installed;
    plugin.enabled = false;
    plugin.last_error = None;
    save_plugin(repository, &plugin).await?;
    Ok(plugin)
}

async fn retire_predecessor(repository: &Store, plugin: &InstalledPlugin) -> Result<(), String> {
    let Some(predecessor_id) = plugin.predecessor_installation_id else {
        return Ok(());
    };
    let Some(mut predecessor) = plugin_by_id(repository, predecessor_id).await? else {
        return Err("plugin update predecessor record is missing".into());
    };
    if predecessor.package_id != plugin.package_id {
        return Err("plugin update predecessor does not match the package id".into());
    }
    if predecessor.phase == PluginPhase::Removed {
        return Ok(());
    }
    for owned in &predecessor.skills {
        if skill_by_id(repository, owned.skill_id).await?.is_some() {
            repository
                .set_skill_enabled_atomic(owned.skill_id, false)
                .await
                .map_err(|error| error.to_string())?;
        }
    }
    predecessor.enabled = false;
    predecessor.phase = PluginPhase::Removed;
    predecessor.cleanup_pending = false;
    predecessor.last_error =
        Some("superseded by a newer local package; files retained for audit".into());
    save_plugin(repository, &predecessor).await
}

fn prepare_plugin_skills(
    install_root: &Path,
    manifest: &PluginManifest,
) -> Result<Vec<(SkillPackage, PluginOwnedSkill)>, String> {
    let mut owned = Vec::new();
    for declared in &manifest.skills {
        let relative_file = normalize_relative_path(&declared.path)?;
        let relative_root = relative_file
            .parent()
            .ok_or_else(|| "plugin skill path has no parent".to_owned())?;
        let skill_root =
            checked_existing_directory(&install_root.join(relative_root), install_root)?;
        let inspection = omicsops_adapters::skills::inspect_skill_directory(&skill_root)
            .map_err(|error| error.to_string())?;
        let skill = SkillPackage {
            id: Uuid::new_v4(),
            name: inspection.name.clone(),
            version: inspection.version,
            source_path: skill_root.to_string_lossy().into_owned(),
            sha256: inspection.sha256.clone(),
            enabled: false,
            capabilities: inspection.capabilities,
            category: Some(format!("plugin:{}", manifest.id)),
        };
        let binding = PluginOwnedSkill {
            skill_id: skill.id,
            name: skill.name.clone(),
            relative_path: slash_path(relative_root),
            package_sha256: skill.sha256.clone(),
        };
        owned.push((skill, binding));
    }
    Ok(owned)
}

async fn register_plugin_skills(
    repository: &Store,
    prepared: &[(SkillPackage, PluginOwnedSkill)],
    installation_id: Uuid,
) -> Result<(), String> {
    for (skill, _) in prepared {
        repository
            .save_skill_package(skill)
            .await
            .map_err(|error| error.to_string())?;
        crate::skill_settings::record_plugin_owned_receipt(repository, skill, installation_id)
            .await?;
    }
    Ok(())
}

async fn set_plugin_enabled(
    repository: &Store,
    request: SetPluginEnabledRequest,
) -> Result<InstalledPlugin, String> {
    let mut plugin = plugin_by_id(repository, request.installation_id)
        .await?
        .ok_or_else(|| "plugin installation was not found".to_owned())?;
    if plugin.phase != PluginPhase::Installed {
        return Err("only a fully installed plugin can be enabled or disabled".into());
    }
    for owned in &plugin.skills {
        let skill = skill_by_id(repository, owned.skill_id)
            .await?
            .ok_or_else(|| format!("plugin-owned skill {} is missing", owned.name))?;
        if skill.sha256 != owned.package_sha256 {
            return Err(format!("plugin-owned skill {} changed", owned.name));
        }
    }
    for owned in &plugin.skills {
        repository
            .set_skill_enabled_atomic(owned.skill_id, request.enabled)
            .await
            .map_err(|error| error.to_string())?;
    }
    plugin.enabled = request.enabled;
    plugin.last_error = None;
    save_plugin(repository, &plugin).await?;
    Ok(plugin)
}

async fn remove_plugin(
    repository: &Store,
    plugins_root: &Path,
    request: RemovePluginRequest,
) -> Result<PluginRemovalResult, String> {
    let mut plugin = plugin_by_id(repository, request.installation_id)
        .await?
        .ok_or_else(|| "plugin installation was not found".to_owned())?;
    if plugin.phase == PluginPhase::Removed {
        return Ok(PluginRemovalResult {
            installation_id: plugin.installation_id,
            status: "removed".into(),
            removed_skills: 0,
            preserved_files: Vec::new(),
            mcp_references_removed: plugin.mcp_bindings.len(),
            message: "plugin was already removed".into(),
        });
    }
    if request.expected_digest != plugin.digest {
        return Err("plugin changed; reopen its details before removing it".into());
    }
    if !repository
        .skill_removal_blocking_run_ids()
        .await
        .map_err(|error| error.to_string())?
        .is_empty()
    {
        return Err(
            "plugin files are retained while an active or resumable run may still reference Skills"
                .into(),
        );
    }
    ensure_no_enabled_external_dependents(repository, &plugin).await?;
    if matches!(
        plugin.phase,
        PluginPhase::NeedsAttention | PluginPhase::Removing
    ) && plugin.cleanup_pending
    {
        return retry_plugin_cleanup(repository, plugins_root, plugin).await;
    }
    plugin.phase = PluginPhase::Removing;
    plugin.enabled = false;
    save_plugin(repository, &plugin).await?;
    let install_root = checked_install_root(plugins_root, plugin.installation_id)?;
    let manifest_path = install_root.join(MANIFEST_NAME);
    let mut expected_files = BTreeSet::from([manifest_path.clone()]);
    let mut preserved_files = Vec::new();
    let mut removable_skills = Vec::new();
    for owned in &plugin.skills {
        let root = install_root.join(normalize_relative_path(&owned.relative_path)?);
        let file = root.join("SKILL.md");
        expected_files.insert(file.clone());
        let receipt = repository
            .get_json::<SkillInstallationReceipt>(
                crate::skill_settings::SKILL_INSTALLATION_KIND,
                &owned.skill_id.to_string(),
            )
            .await
            .map_err(|error| error.to_string())?;
        match (skill_by_id(repository, owned.skill_id).await?, receipt) {
            (Some(skill), Some(receipt))
                if skill.sha256 == owned.package_sha256
                    && same_path(Path::new(&skill.source_path), &root)
                    && receipt.origin == SkillOrigin::PluginOwned
                    && receipt.plugin_installation_id == Some(plugin.installation_id)
                    && receipt.skill_id == skill.id
                    && receipt.package_sha256 == skill.sha256
                    && same_path(Path::new(&receipt.installed_root), &root)
                    && omicsops_adapters::skills::inspect_skill_directory(&root)
                        .is_ok_and(|inspection| inspection.sha256 == owned.package_sha256) =>
            {
                removable_skills.push(skill.id);
            }
            _ => preserved_files.push(slash_path(
                file.strip_prefix(&install_root).unwrap_or(file.as_path()),
            )),
        }
    }
    for file in inventory_files(&install_root)? {
        if !expected_files.contains(&file) {
            preserved_files.push(slash_path(
                file.strip_prefix(&install_root).unwrap_or(file.as_path()),
            ));
        }
    }
    preserved_files.sort();
    preserved_files.dedup();
    let mut verified_for_delete = None;
    if preserved_files.is_empty() {
        match inspect_plugin_source(repository, &install_root).await {
            Ok(verified) if verified.inspection.manifest_digest == plugin.digest => {
                verified_for_delete = Some(verified.verified_files);
            }
            _ => preserved_files.push(MANIFEST_NAME.into()),
        }
    }
    plugin.cleanup_pending = true;
    save_plugin(repository, &plugin).await?;
    for skill_id in &removable_skills {
        if let Err(error) = unregister_owned_skill(repository, &plugin, *skill_id).await {
            plugin.phase = PluginPhase::NeedsAttention;
            plugin.last_error = Some(format!("plugin catalog cleanup was interrupted: {error}"));
            save_plugin(repository, &plugin).await?;
            return Err(
                "plugin catalog cleanup was interrupted; retry from Plugins settings".into(),
            );
        }
    }
    for owned in &plugin.skills {
        if !removable_skills.contains(&owned.skill_id)
            && skill_by_id(repository, owned.skill_id).await?.is_some()
        {
            let _ = repository
                .set_skill_enabled_atomic(owned.skill_id, false)
                .await;
        }
    }
    if let Some(verified_files) = verified_for_delete {
        if let Err(error) = remove_known_plugin_tree(&install_root, &verified_files) {
            plugin.phase = PluginPhase::NeedsAttention;
            plugin.cleanup_pending = true;
            plugin.last_error = Some(format!("verified cleanup was interrupted: {error}"));
            save_plugin(repository, &plugin).await?;
            return Err("plugin cleanup was interrupted; retry from Plugins settings".into());
        }
    }
    let mcp_references_removed = plugin.mcp_bindings.len();
    plugin.mcp_bindings.clear();
    plugin.phase = PluginPhase::Removed;
    plugin.cleanup_pending = false;
    plugin.last_error =
        (!preserved_files.is_empty()).then(|| "modified or unknown files were preserved".into());
    save_plugin(repository, &plugin).await?;
    Ok(PluginRemovalResult {
        installation_id: plugin.installation_id,
        status: "removed".into(),
        removed_skills: removable_skills.len(),
        preserved_files: preserved_files.clone(),
        mcp_references_removed,
        message: if preserved_files.is_empty() {
            "plugin records and unchanged owned files were removed".into()
        } else {
            "plugin was disabled and unregistered; modified or unknown files were preserved".into()
        },
    })
}

async fn retry_plugin_cleanup(
    repository: &Store,
    plugins_root: &Path,
    mut plugin: InstalledPlugin,
) -> Result<PluginRemovalResult, String> {
    let candidate = plugins_root
        .join("installs")
        .join(plugin.installation_id.to_string());
    if !candidate.exists() {
        for owned in &plugin.skills {
            unregister_owned_skill(repository, &plugin, owned.skill_id).await?;
        }
        let removed_references = plugin.mcp_bindings.len();
        plugin.phase = PluginPhase::Removed;
        plugin.cleanup_pending = false;
        plugin.mcp_bindings.clear();
        plugin.last_error = None;
        save_plugin(repository, &plugin).await?;
        return Ok(PluginRemovalResult {
            installation_id: plugin.installation_id,
            status: "removed".into(),
            removed_skills: 0,
            preserved_files: Vec::new(),
            mcp_references_removed: removed_references,
            message: "verified plugin cleanup had already completed".into(),
        });
    }
    let install_root = checked_install_root(plugins_root, plugin.installation_id)?;
    let mut verified = BTreeMap::new();
    let mut preserved = Vec::new();
    for file in &plugin.files {
        let relative = normalize_relative_path(&file.relative_path)?;
        let path = install_root.join(&relative);
        if !path.exists() {
            continue;
        }
        let limit = if file.relative_path == MANIFEST_NAME {
            MAX_MANIFEST_BYTES
        } else {
            MAX_SKILL_BYTES
        };
        let bytes = read_bounded_plain_file(&path, limit)?;
        if sha256_bytes(&bytes) == file.sha256 {
            verified.insert(relative, bytes);
        } else {
            preserved.push(file.relative_path.clone());
        }
    }
    let declared = plugin
        .files
        .iter()
        .map(|file| file.relative_path.as_str())
        .collect::<BTreeSet<_>>();
    for path in inventory_files(&install_root)? {
        let relative = slash_path(
            path.strip_prefix(&install_root)
                .map_err(|error| error.to_string())?,
        );
        if !declared.contains(relative.as_str()) {
            preserved.push(relative);
        }
    }
    preserved.sort();
    preserved.dedup();
    for owned in &plugin.skills {
        let skill_file = normalize_relative_path(&format!("{}/SKILL.md", owned.relative_path))?;
        let skill_root = install_root.join(normalize_relative_path(&owned.relative_path)?);
        let unchanged = verified.contains_key(&skill_file)
            && omicsops_adapters::skills::inspect_skill_directory(&skill_root)
                .is_ok_and(|inspection| inspection.sha256 == owned.package_sha256);
        if unchanged {
            if let Err(error) = unregister_owned_skill(repository, &plugin, owned.skill_id).await {
                plugin.phase = PluginPhase::NeedsAttention;
                plugin.last_error =
                    Some(format!("plugin catalog cleanup was interrupted: {error}"));
                save_plugin(repository, &plugin).await?;
                return Err(
                    "plugin catalog cleanup is still blocked; retry from Plugins settings".into(),
                );
            }
        } else if skill_by_id(repository, owned.skill_id).await?.is_some() {
            let _ = repository
                .set_skill_enabled_atomic(owned.skill_id, false)
                .await;
        }
    }
    if preserved.is_empty() {
        if let Err(error) = remove_known_plugin_tree(&install_root, &verified) {
            plugin.last_error = Some(format!("verified cleanup was interrupted: {error}"));
            save_plugin(repository, &plugin).await?;
            return Err(
                "plugin cleanup is still blocked; close users of its files and retry".into(),
            );
        }
    }
    let removed_references = plugin.mcp_bindings.len();
    plugin.phase = PluginPhase::Removed;
    plugin.cleanup_pending = false;
    plugin.mcp_bindings.clear();
    plugin.last_error = (!preserved.is_empty()).then(|| "modified files were preserved".into());
    save_plugin(repository, &plugin).await?;
    Ok(PluginRemovalResult {
        installation_id: plugin.installation_id,
        status: "removed".into(),
        removed_skills: 0,
        preserved_files: preserved.clone(),
        mcp_references_removed: removed_references,
        message: if preserved.is_empty() {
            "verified plugin cleanup completed".into()
        } else {
            "modified plugin files were preserved".into()
        },
    })
}

async fn unregister_owned_skill(
    repository: &Store,
    plugin: &InstalledPlugin,
    skill_id: Uuid,
) -> Result<(), String> {
    let owned = plugin
        .skills
        .iter()
        .find(|owned| owned.skill_id == skill_id)
        .ok_or_else(|| "plugin cleanup journal does not own the Skill".to_owned())?;
    let skill = skill_by_id(repository, skill_id).await?;
    let receipt = repository
        .get_json::<SkillInstallationReceipt>(
            crate::skill_settings::SKILL_INSTALLATION_KIND,
            &skill_id.to_string(),
        )
        .await
        .map_err(|error| error.to_string())?;
    if skill.is_none() && receipt.is_none() {
        return Ok(());
    }
    let receipt = receipt.ok_or_else(|| {
        format!(
            "ownership receipt for plugin Skill {} is missing",
            owned.name
        )
    })?;
    if receipt.origin != SkillOrigin::PluginOwned
        || receipt.plugin_installation_id != Some(plugin.installation_id)
        || receipt.skill_id != skill_id
        || receipt.package_sha256 != owned.package_sha256
    {
        return Err(format!(
            "ownership receipt for plugin Skill {} changed",
            owned.name
        ));
    }
    if let Some(skill) = skill {
        if skill.id != skill_id
            || skill.sha256 != owned.package_sha256
            || !same_path(
                Path::new(&skill.source_path),
                Path::new(&receipt.installed_root),
            )
        {
            return Err(format!("plugin Skill {} changed", owned.name));
        }
        repository
            .delete_skill_package(skill_id)
            .await
            .map_err(|error| error.to_string())?;
    }
    repository
        .delete_json(
            crate::skill_settings::SKILL_INSTALLATION_KIND,
            &skill_id.to_string(),
        )
        .await
        .map(|_| ())
        .map_err(|error| error.to_string())
}

async fn ensure_no_enabled_external_dependents(
    repository: &Store,
    plugin: &InstalledPlugin,
) -> Result<(), String> {
    let owned_ids = plugin
        .skills
        .iter()
        .map(|owned| owned.skill_id)
        .collect::<BTreeSet<_>>();
    let packages = repository
        .list_skill_packages()
        .await
        .map_err(|error| error.to_string())?;
    let mut available = owned_ids.clone();
    for skill in &packages {
        if !owned_ids.contains(&skill.id) && plugin_allows_skill(repository, skill.id).await? {
            available.insert(skill.id);
        }
    }
    let roots = packages
        .iter()
        .filter(|skill| {
            skill.enabled && available.contains(&skill.id) && !owned_ids.contains(&skill.id)
        })
        .map(|skill| (skill.id, skill.name.clone()))
        .collect::<Vec<_>>();
    for (root_id, root_name) in roots {
        let mut queue = VecDeque::from([root_id]);
        let mut visited = BTreeSet::new();
        while let Some(skill_id) = queue.pop_front() {
            if !visited.insert(skill_id) {
                continue;
            }
            let skill = packages
                .iter()
                .find(|candidate| candidate.id == skill_id)
                .ok_or_else(|| {
                    "Skill dependency catalog changed during plugin removal".to_owned()
                })?;
            let dependencies = crate::skill_settings::read_skill_dependencies_bounded(Path::new(
                &skill.source_path,
            ))
            .map_err(|error| {
                format!("cannot verify Skill dependencies before plugin removal: {error}")
            })?;
            for dependency in dependencies {
                let Some(candidate) = packages.iter().find(|candidate| {
                    available.contains(&candidate.id)
                        && crate::skill_commands::skill_name_matches_dependency(
                            &candidate.name,
                            &dependency,
                        )
                }) else {
                    continue;
                };
                if owned_ids.contains(&candidate.id) {
                    return Err(format!(
                        "enabled Skill {root_name} still depends on this plugin"
                    ));
                }
                queue.push_back(candidate.id);
            }
        }
    }
    Ok(())
}

async fn list_plugins(
    repository: &Store,
    include_removed: bool,
) -> Result<Vec<InstalledPlugin>, String> {
    let mut plugins = repository
        .list_json::<InstalledPlugin>(PLUGIN_RECORD_KIND)
        .await
        .map_err(|error| error.to_string())?;
    if !include_removed {
        plugins.retain(|plugin| plugin.phase != PluginPhase::Removed);
    }
    plugins.sort_by(|left, right| {
        left.name
            .cmp(&right.name)
            .then(left.created_at.cmp(&right.created_at))
    });
    Ok(plugins)
}

async fn plugin_by_id(repository: &Store, id: Uuid) -> Result<Option<InstalledPlugin>, String> {
    repository
        .get_json(PLUGIN_RECORD_KIND, &id.to_string())
        .await
        .map_err(|error| error.to_string())
}

async fn save_plugin(repository: &Store, plugin: &InstalledPlugin) -> Result<(), String> {
    repository
        .put_json(
            PLUGIN_RECORD_KIND,
            &plugin.installation_id.to_string(),
            plugin,
        )
        .await
        .map_err(|error| error.to_string())
}

async fn skill_by_id(repository: &Store, id: Uuid) -> Result<Option<SkillPackage>, String> {
    Ok(repository
        .list_skill_packages()
        .await
        .map_err(|error| error.to_string())?
        .into_iter()
        .find(|skill| skill.id == id))
}

fn validate_manifest(manifest: &PluginManifest) -> Result<(), String> {
    if manifest.schema_version != 1 {
        return Err("plugin schema_version must be 1".into());
    }
    let id = Regex::new(r"^[a-z0-9][a-z0-9.-]{0,63}$").expect("static regex");
    if !id.is_match(&manifest.id)
        || manifest.id.ends_with('.')
        || manifest.id.ends_with("..")
        || is_windows_reserved(&manifest.id)
    {
        return Err("plugin id is invalid".into());
    }
    let semver = Regex::new(r"^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(?:-[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?(?:\+[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?$").expect("static regex");
    if !semver.is_match(&manifest.version) {
        return Err("plugin version must be a bounded semantic version".into());
    }
    if manifest.name.trim().is_empty() || manifest.name.len() > 120 {
        return Err("plugin name must contain 1 to 120 bytes".into());
    }
    if manifest.skills.is_empty() || manifest.skills.len() > MAX_SKILLS {
        return Err("plugin must declare 1 to 20 standalone Skills".into());
    }
    if manifest.mcp_presets.len() > MAX_PRESETS {
        return Err("plugin declares more than 20 MCP preset references".into());
    }
    let compiled = list_bundled_mcp_presets()
        .into_iter()
        .map(|preset| preset.id)
        .collect::<BTreeSet<_>>();
    let mut preset_ids = BTreeSet::new();
    for preset in &manifest.mcp_presets {
        if !compiled.contains(preset) {
            return Err(format!(
                "plugin references unknown compiled MCP preset {preset}"
            ));
        }
        if !preset_ids.insert(preset) {
            return Err(format!("plugin declares duplicate MCP preset {preset}"));
        }
    }
    let mut roots = Vec::new();
    for skill in &manifest.skills {
        validate_sha256(&skill.sha256, "skill SHA-256")?;
        let path = normalize_relative_path(&skill.path)?;
        if path.file_name().and_then(|value| value.to_str()) != Some("SKILL.md") {
            return Err("each plugin skill path must name SKILL.md".into());
        }
        let root = path.parent().unwrap_or_else(|| Path::new("")).to_path_buf();
        if root.as_os_str().is_empty() {
            return Err("each plugin Skill must have its own package directory".into());
        }
        if roots
            .iter()
            .any(|other: &PathBuf| root.starts_with(other) || other.starts_with(&root))
        {
            return Err("plugin skill roots must not overlap or nest".into());
        }
        roots.push(root);
    }
    Ok(())
}

fn normalize_relative_path(value: &str) -> Result<PathBuf, String> {
    if value.trim().is_empty() || value.contains(':') || value.starts_with(['/', '\\']) {
        return Err("plugin paths must be relative and cannot contain ADS/device prefixes".into());
    }
    let path = Path::new(value);
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => {
                let text = part
                    .to_str()
                    .ok_or_else(|| "plugin path must be valid Unicode".to_owned())?;
                if text.ends_with([' ', '.']) || is_windows_reserved(text) {
                    return Err(format!("unsafe Windows plugin path component {text}"));
                }
                normalized.push(part);
            }
            _ => return Err("plugin path contains traversal or a platform prefix".into()),
        }
    }
    if normalized.as_os_str().is_empty() {
        return Err("plugin path is empty".into());
    }
    Ok(normalized)
}

fn is_windows_reserved(value: &str) -> bool {
    let stem = value
        .split('.')
        .next()
        .unwrap_or(value)
        .to_ascii_uppercase();
    matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL" | "CLOCK$")
        || (stem.len() == 4
            && (stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.as_bytes()[3].is_ascii_digit()
            && stem.as_bytes()[3] != b'0')
}

fn checked_source_root(source: &Path) -> Result<PathBuf, String> {
    if !source.is_absolute() {
        return Err("plugin source directory must be absolute".into());
    }
    let metadata = fs::symlink_metadata(source).map_err(|error| error.to_string())?;
    if !metadata.is_dir() || is_link_or_reparse(&metadata) {
        return Err("plugin source must be a plain local directory".into());
    }
    source.canonicalize().map_err(|error| error.to_string())
}

fn safe_file_metadata(path: &Path) -> Result<fs::Metadata, String> {
    let metadata = fs::symlink_metadata(path).map_err(|error| error.to_string())?;
    if !metadata.is_file() || is_link_or_reparse(&metadata) {
        return Err(format!(
            "plugin entry must be a plain file: {}",
            path.display()
        ));
    }
    Ok(metadata)
}

fn validate_source_inventory(root: &Path, declared: &BTreeSet<String>) -> Result<(), String> {
    for file in inventory_files(root)? {
        let relative = slash_path(file.strip_prefix(root).map_err(|error| error.to_string())?);
        if !declared.contains(&relative) {
            return Err(format!("plugin contains undeclared file {relative}"));
        }
    }
    Ok(())
}

fn inventory_files(root: &Path) -> Result<Vec<PathBuf>, String> {
    fn visit(root: &Path, directory: &Path, files: &mut Vec<PathBuf>) -> Result<(), String> {
        let mut entries = fs::read_dir(directory)
            .map_err(|error| error.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())?;
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            let path = entry.path();
            let metadata = fs::symlink_metadata(&path).map_err(|error| error.to_string())?;
            if is_link_or_reparse(&metadata) {
                return Err(format!(
                    "plugin package contains a link or reparse point: {}",
                    path.display()
                ));
            }
            if metadata.is_dir() {
                let canonical = path.canonicalize().map_err(|error| error.to_string())?;
                if !canonical.starts_with(root) {
                    return Err("plugin directory escapes its source root".into());
                }
                visit(root, &canonical, files)?;
            } else if metadata.is_file() {
                files.push(path);
            } else {
                return Err(format!(
                    "plugin contains an unsupported filesystem entry: {}",
                    path.display()
                ));
            }
        }
        Ok(())
    }
    let canonical = root.canonicalize().map_err(|error| error.to_string())?;
    let mut files = Vec::new();
    visit(&canonical, &canonical, &mut files)?;
    Ok(files)
}

fn reject_relative_markdown_references(markdown: &str, path: &str) -> Result<(), String> {
    let link = Regex::new(r"!?\[[^\]]*\]\(([^)\s]+)").expect("static regex");
    for capture in link.captures_iter(markdown) {
        let target = capture.get(1).map(|item| item.as_str()).unwrap_or_default();
        if !(target.starts_with("https://")
            || target.starts_with("http://")
            || target.starts_with('#')
            || target.starts_with("mailto:"))
        {
            return Err(format!(
                "plugin skill {path} contains relative reference {target}; schema 1 supports standalone SKILL.md only"
            ));
        }
    }
    Ok(())
}

fn copy_declared_package(inspected: &InspectedSource, stage: &Path) -> Result<(), String> {
    for (relative, bytes) in &inspected.verified_files {
        let destination = stage.join(&relative);
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        fs::write(destination, bytes).map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn checked_install_root(plugins_root: &Path, id: Uuid) -> Result<PathBuf, String> {
    let parent = plugins_root.join("installs");
    ensure_plain_directory(plugins_root)?;
    ensure_plain_directory(&parent)?;
    checked_existing_directory(&parent.join(id.to_string()), &parent)
}

fn checked_existing_directory(path: &Path, parent: &Path) -> Result<PathBuf, String> {
    let metadata = fs::symlink_metadata(path).map_err(|error| error.to_string())?;
    if !metadata.is_dir() || is_link_or_reparse(&metadata) {
        return Err(format!(
            "unsafe managed plugin directory: {}",
            path.display()
        ));
    }
    let canonical_parent = parent.canonicalize().map_err(|error| error.to_string())?;
    let canonical = path.canonicalize().map_err(|error| error.to_string())?;
    if !canonical.starts_with(&canonical_parent) {
        return Err("managed plugin directory escapes its owner root".into());
    }
    Ok(canonical)
}

fn ensure_plain_directory(path: &Path) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path).map_err(|error| error.to_string())?;
    if !metadata.is_dir() || is_link_or_reparse(&metadata) {
        return Err(format!(
            "unsafe plugin management directory: {}",
            path.display()
        ));
    }
    Ok(())
}

fn remove_known_plugin_tree(
    root: &Path,
    verified_files: &BTreeMap<PathBuf, Vec<u8>>,
) -> Result<(), String> {
    let mut directories = inventory_directories(root)?;
    for (relative, expected) in verified_files {
        let path = root.join(relative);
        let current = read_bounded_plain_file(&path, MAX_TOTAL_BYTES)?;
        if &current != expected {
            return Err(format!(
                "managed plugin file changed during removal: {}",
                slash_path(relative)
            ));
        }
    }
    for relative in verified_files.keys() {
        fs::remove_file(root.join(relative)).map_err(|error| error.to_string())?;
    }
    directories.sort_by_key(|path| std::cmp::Reverse(path.components().count()));
    for relative in directories {
        fs::remove_dir(&relative).map_err(|error| {
            format!("plugin directory gained an unknown entry and was preserved: {error}")
        })?;
    }
    fs::remove_dir(root).map_err(|error| {
        format!("plugin installation gained an unknown entry and was preserved: {error}")
    })
}

fn inventory_directories(root: &Path) -> Result<Vec<PathBuf>, String> {
    fn visit(directory: &Path, directories: &mut Vec<PathBuf>) -> Result<(), String> {
        let entries = fs::read_dir(directory)
            .map_err(|error| error.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())?;
        for entry in entries {
            let path = entry.path();
            let metadata = fs::symlink_metadata(&path).map_err(|error| error.to_string())?;
            if is_link_or_reparse(&metadata) {
                return Err("plugin cleanup encountered a link or reparse point".into());
            }
            if metadata.is_dir() {
                visit(&path, directories)?;
                directories.push(path);
            }
        }
        Ok(())
    }
    let mut directories = Vec::new();
    visit(root, &mut directories)?;
    Ok(directories)
}

fn cleanup_partial_stage(
    root: &Path,
    verified_files: &BTreeMap<PathBuf, Vec<u8>>,
) -> Result<(), String> {
    if !root.exists() {
        return Ok(());
    }
    ensure_plain_directory(root)?;
    let mut directories = BTreeSet::new();
    for (relative, expected) in verified_files {
        let path = root.join(relative);
        if !path.exists() {
            continue;
        }
        if read_bounded_plain_file(&path, MAX_TOTAL_BYTES)? != *expected {
            return Err("staging file changed during cleanup; it was preserved".into());
        }
        fs::remove_file(&path).map_err(|error| error.to_string())?;
        let mut parent = relative.parent();
        while let Some(directory) = parent {
            if !directory.as_os_str().is_empty() {
                directories.insert(directory.to_path_buf());
            }
            parent = directory.parent();
        }
    }
    let mut directories = directories.into_iter().collect::<Vec<_>>();
    directories.sort_by_key(|path| std::cmp::Reverse(path.components().count()));
    for directory in directories {
        let _ = fs::remove_dir(root.join(directory));
    }
    fs::remove_dir(root).map_err(|error| format!("staging directory was preserved: {error}"))
}

fn read_bounded_plain_file(path: &Path, limit: u64) -> Result<Vec<u8>, String> {
    safe_file_metadata(path)?;
    let file = fs::File::open(path).map_err(|error| error.to_string())?;
    let opened = file.metadata().map_err(|error| error.to_string())?;
    if !opened.is_file() || is_link_or_reparse(&opened) {
        return Err(format!(
            "plugin entry must be a plain file: {}",
            path.display()
        ));
    }
    if opened.len() > limit {
        return Err(format!("file exceeds {} bytes", limit));
    }
    let mut bytes = Vec::with_capacity(opened.len() as usize);
    file.take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() as u64 > limit {
        return Err(format!("file exceeds {} bytes", limit));
    }
    let after = safe_file_metadata(path)?;
    if after.len() != bytes.len() as u64 || opened.len() != after.len() {
        return Err("plugin source changed while it was being read".into());
    }
    Ok(bytes)
}

fn validate_sha256(value: &str, label: &str) -> Result<(), String> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err(format!(
            "{label} must be 64 lowercase hexadecimal characters"
        ));
    }
    Ok(())
}

fn sha256_bytes(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn slash_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn same_path(left: &Path, right: &Path) -> bool {
    match (left.canonicalize(), right.canonicalize()) {
        (Ok(left), Ok(right)) => left == right,
        _ => left == right,
    }
}

#[cfg(windows)]
fn is_link_or_reparse(metadata: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    metadata.file_type().is_symlink() || metadata.file_attributes() & 0x400 != 0
}

#[cfg(not(windows))]
fn is_link_or_reparse(metadata: &fs::Metadata) -> bool {
    metadata.file_type().is_symlink()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn fixture(root: &Path, package_id: &str, version: &str) -> String {
        let skill_dir = root.join("skills/qc");
        fs::create_dir_all(&skill_dir).unwrap();
        let markdown = b"---\nname: plugin-qc\nversion: 1.0.0\ncapabilities:\n  - read_project_files\n---\n# QC\nUse only advertised tools.\n";
        fs::write(skill_dir.join("SKILL.md"), markdown).unwrap();
        let hash = sha256_bytes(markdown);
        fs::write(
            root.join(MANIFEST_NAME),
            serde_json::to_vec_pretty(&json!({
                "schema_version": 1,
                "id": package_id,
                "version": version,
                "name": "QC helpers",
                "skills": [{"path": "skills/qc/SKILL.md", "sha256": hash}],
                "mcp_presets": ["pubmed"]
            }))
            .unwrap(),
        )
        .unwrap();
        hash
    }

    fn catalog_skill(
        root: &Path,
        name: &str,
        enabled: bool,
        dependencies: &[&str],
    ) -> SkillPackage {
        let directory = root.join(name);
        fs::create_dir_all(&directory).unwrap();
        let depends_on = dependencies
            .iter()
            .map(|dependency| format!("  - {dependency}"))
            .collect::<Vec<_>>()
            .join("\n");
        let markdown = format!(
            "---\nname: {name}\nversion: 1.0.0\ndepends_on:\n{depends_on}\n---\n# {name}\n"
        );
        fs::write(directory.join("SKILL.md"), markdown).unwrap();
        let inspection = omicsops_adapters::skills::inspect_skill_directory(&directory).unwrap();
        SkillPackage {
            id: Uuid::new_v4(),
            name: inspection.name,
            version: inspection.version,
            source_path: directory.to_string_lossy().into_owned(),
            sha256: inspection.sha256,
            enabled,
            capabilities: inspection.capabilities,
            category: None,
        }
    }

    #[tokio::test]
    async fn inspection_is_strict_bounded_and_detects_source_changes() {
        let store = Store::open_in_memory().await.unwrap();
        let root = tempfile::tempdir().unwrap();
        fixture(root.path(), "lab.qc", "1.0.0");
        let inspection = inspect_plugin_source(&store, root.path()).await.unwrap();
        assert_eq!(inspection.inspection.files.len(), 2);
        assert_eq!(
            inspection.inspection.bindings[0].ownership,
            "shared_reference"
        );
        assert_eq!(inspection.inspection.changes, ["new_installation"]);

        let mut manifest: serde_json::Value =
            serde_json::from_slice(&fs::read(root.path().join(MANIFEST_NAME)).unwrap()).unwrap();
        manifest["script"] = json!("run.ps1");
        fs::write(
            root.path().join(MANIFEST_NAME),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();
        assert!(inspect_plugin_source(&store, root.path())
            .await
            .unwrap_err()
            .contains("unknown field"));
    }

    #[tokio::test]
    async fn inspection_rejects_unsafe_paths_hashes_attachments_and_relative_links() {
        let store = Store::open_in_memory().await.unwrap();
        for (path, expected) in [
            ("../SKILL.md", "traversal"),
            ("C:/SKILL.md", "relative"),
            ("skills/CON/SKILL.md", "unsafe Windows"),
        ] {
            let root = tempfile::tempdir().unwrap();
            fs::write(
                root.path().join(MANIFEST_NAME),
                serde_json::to_vec(&json!({
                    "schema_version": 1, "id": "lab.qc", "version": "1.0.0", "name": "QC",
                    "skills": [{"path": path, "sha256": "0".repeat(64)}], "mcp_presets": []
                }))
                .unwrap(),
            )
            .unwrap();
            assert!(inspect_plugin_source(&store, root.path())
                .await
                .unwrap_err()
                .contains(expected));
        }

        let root = tempfile::tempdir().unwrap();
        fixture(root.path(), "lab.qc", "1.0.0");
        fs::write(root.path().join("skills/qc/helper.md"), "attachment").unwrap();
        assert!(inspect_plugin_source(&store, root.path())
            .await
            .unwrap_err()
            .contains("undeclared file"));

        fs::remove_file(root.path().join("skills/qc/helper.md")).unwrap();
        let markdown = b"---\nname: plugin-qc\n---\n[helper](helper.md)\n";
        fs::write(root.path().join("skills/qc/SKILL.md"), markdown).unwrap();
        let mut manifest: serde_json::Value =
            serde_json::from_slice(&fs::read(root.path().join(MANIFEST_NAME)).unwrap()).unwrap();
        manifest["skills"][0]["sha256"] = json!(sha256_bytes(markdown));
        fs::write(
            root.path().join(MANIFEST_NAME),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();
        assert!(inspect_plugin_source(&store, root.path())
            .await
            .unwrap_err()
            .contains("standalone SKILL.md"));
    }

    #[tokio::test]
    async fn install_is_atomic_idempotent_gated_and_safe_to_remove() {
        let store = Store::open_in_memory().await.unwrap();
        let app = tempfile::tempdir().unwrap();
        let source = tempfile::tempdir().unwrap();
        fixture(source.path(), "lab.qc", "1.0.0");
        let inspected = inspect_plugin_source(&store, source.path()).await.unwrap();
        let request = InstallPluginRequest {
            source_path: source.path().to_string_lossy().into_owned(),
            expected_digest: inspected.inspection.manifest_digest.clone(),
            expected_old_digest: None,
        };
        fs::write(source.path().join("skills/qc/SKILL.md"), "changed").unwrap();
        let error = install_plugin(&store, &app.path().join("plugins"), request.clone())
            .await
            .unwrap_err();
        assert!(error.contains("mismatch") || error.contains("changed"));

        fixture(source.path(), "lab.qc", "1.0.0");
        let inspected = inspect_plugin_source(&store, source.path()).await.unwrap();
        let request = InstallPluginRequest {
            expected_digest: inspected.inspection.manifest_digest,
            ..request
        };
        let installed = install_plugin(&store, &app.path().join("plugins"), request.clone())
            .await
            .unwrap();
        assert!(!plugin_allows_skill(&store, installed.skills[0].skill_id)
            .await
            .unwrap());
        let same = install_plugin(&store, &app.path().join("plugins"), request)
            .await
            .unwrap();
        assert_eq!(same.installation_id, installed.installation_id);
        let enabled = set_plugin_enabled(
            &store,
            SetPluginEnabledRequest {
                installation_id: installed.installation_id,
                enabled: true,
            },
        )
        .await
        .unwrap();
        assert!(enabled.enabled);
        assert!(plugin_allows_skill(&store, installed.skills[0].skill_id)
            .await
            .unwrap());

        let result = remove_plugin(
            &store,
            &app.path().join("plugins"),
            RemovePluginRequest {
                installation_id: installed.installation_id,
                expected_digest: installed.digest,
            },
        )
        .await
        .unwrap();
        assert_eq!(result.removed_skills, 1);
        assert!(result.preserved_files.is_empty());
        assert!(store
            .list_json::<McpServerProfile>("mcp_server")
            .await
            .unwrap()
            .is_empty());
    }

    #[tokio::test]
    async fn remove_preserves_modified_owned_and_unknown_files() {
        let store = Store::open_in_memory().await.unwrap();
        let app = tempfile::tempdir().unwrap();
        let source = tempfile::tempdir().unwrap();
        fixture(source.path(), "lab.qc", "1.0.0");
        let inspected = inspect_plugin_source(&store, source.path()).await.unwrap();
        let installed = install_plugin(
            &store,
            &app.path().join("plugins"),
            InstallPluginRequest {
                source_path: source.path().to_string_lossy().into_owned(),
                expected_digest: inspected.inspection.manifest_digest,
                expected_old_digest: None,
            },
        )
        .await
        .unwrap();
        let root = app
            .path()
            .join("plugins/installs")
            .join(installed.installation_id.to_string());
        fs::write(root.join("skills/qc/SKILL.md"), "user edit").unwrap();
        fs::write(root.join("note.txt"), "keep").unwrap();
        let result = remove_plugin(
            &store,
            &app.path().join("plugins"),
            RemovePluginRequest {
                installation_id: installed.installation_id,
                expected_digest: installed.digest,
            },
        )
        .await
        .unwrap();
        assert!(result
            .preserved_files
            .iter()
            .any(|path| path == "skills/qc/SKILL.md"));
        assert!(result.preserved_files.iter().any(|path| path == "note.txt"));
        assert!(root.exists());
    }

    #[tokio::test]
    async fn interrupted_verified_cleanup_is_visible_and_retry_is_idempotent() {
        let store = Store::open_in_memory().await.unwrap();
        let app = tempfile::tempdir().unwrap();
        let source = tempfile::tempdir().unwrap();
        fixture(source.path(), "lab.retry", "1.0.0");
        let inspected = inspect_plugin_source(&store, source.path()).await.unwrap();
        let mut installed = install_plugin(
            &store,
            &app.path().join("plugins"),
            InstallPluginRequest {
                source_path: source.path().to_string_lossy().into_owned(),
                expected_digest: inspected.inspection.manifest_digest,
                expected_old_digest: None,
            },
        )
        .await
        .unwrap();
        let root = app
            .path()
            .join("plugins/installs")
            .join(installed.installation_id.to_string());
        fs::remove_file(root.join("skills/qc/SKILL.md")).unwrap();
        repository_cleanup_catalog_for_test(&store, &installed).await;
        installed.phase = PluginPhase::NeedsAttention;
        installed.cleanup_pending = true;
        save_plugin(&store, &installed).await.unwrap();

        let result = remove_plugin(
            &store,
            &app.path().join("plugins"),
            RemovePluginRequest {
                installation_id: installed.installation_id,
                expected_digest: installed.digest.clone(),
            },
        )
        .await
        .unwrap();
        assert_eq!(result.status, "removed");
        assert!(!root.exists());
        let repeated = remove_plugin(
            &store,
            &app.path().join("plugins"),
            RemovePluginRequest {
                installation_id: installed.installation_id,
                expected_digest: installed.digest,
            },
        )
        .await
        .unwrap();
        assert_eq!(repeated.status, "removed");
    }

    #[tokio::test]
    async fn interrupted_registration_reuses_owned_ids_without_duplicate_skills() {
        let store = Store::open_in_memory().await.unwrap();
        let app = tempfile::tempdir().unwrap();
        let source = tempfile::tempdir().unwrap();
        fixture(source.path(), "lab.resume", "1.0.0");
        let inspected = inspect_plugin_source(&store, source.path()).await.unwrap();
        let request = InstallPluginRequest {
            source_path: source.path().to_string_lossy().into_owned(),
            expected_digest: inspected.inspection.manifest_digest,
            expected_old_digest: None,
        };
        let mut installed = install_plugin(&store, &app.path().join("plugins"), request.clone())
            .await
            .unwrap();
        let owned_id = installed.skills[0].skill_id;
        store.delete_skill_package(owned_id).await.unwrap();
        store
            .delete_json(
                crate::skill_settings::SKILL_INSTALLATION_KIND,
                &owned_id.to_string(),
            )
            .await
            .unwrap();
        installed.phase = PluginPhase::NeedsAttention;
        installed.last_error = Some("simulated registration interruption".into());
        save_plugin(&store, &installed).await.unwrap();

        let resumed = install_plugin(
            &store,
            &app.path().join("plugins"),
            InstallPluginRequest {
                expected_old_digest: Some(installed.digest.clone()),
                ..request
            },
        )
        .await
        .unwrap();
        assert_eq!(resumed.installation_id, installed.installation_id);
        assert_eq!(resumed.skills[0].skill_id, owned_id);
        let packages = store.list_skill_packages().await.unwrap();
        assert_eq!(
            packages.iter().filter(|skill| skill.id == owned_id).count(),
            1
        );
    }

    #[tokio::test]
    async fn removal_blocks_suffix_and_transitive_dependencies_through_disabled_skills() {
        let store = Store::open_in_memory().await.unwrap();
        let app = tempfile::tempdir().unwrap();
        let source = tempfile::tempdir().unwrap();
        let external = tempfile::tempdir().unwrap();
        fixture(source.path(), "lab.dependencies", "1.0.0");
        let inspected = inspect_plugin_source(&store, source.path()).await.unwrap();
        let installed = install_plugin(
            &store,
            &app.path().join("plugins"),
            InstallPluginRequest {
                source_path: source.path().to_string_lossy().into_owned(),
                expected_digest: inspected.inspection.manifest_digest,
                expected_old_digest: None,
            },
        )
        .await
        .unwrap();

        let direct = catalog_skill(external.path(), "direct", true, &["qc"]);
        store.save_skill_package(&direct).await.unwrap();
        let request = RemovePluginRequest {
            installation_id: installed.installation_id,
            expected_digest: installed.digest.clone(),
        };
        assert!(
            remove_plugin(&store, &app.path().join("plugins"), request.clone())
                .await
                .unwrap_err()
                .contains("direct")
        );
        store
            .set_skill_enabled_atomic(direct.id, false)
            .await
            .unwrap();

        let middle = catalog_skill(external.path(), "middle", false, &["qc"]);
        let outer = catalog_skill(external.path(), "outer", true, &["middle"]);
        store.save_skill_package(&middle).await.unwrap();
        store.save_skill_package(&outer).await.unwrap();
        assert!(remove_plugin(&store, &app.path().join("plugins"), request)
            .await
            .unwrap_err()
            .contains("outer"));
    }

    #[tokio::test]
    async fn removing_cleanup_retry_unregisters_catalog_without_restart() {
        let store = Store::open_in_memory().await.unwrap();
        let app = tempfile::tempdir().unwrap();
        let source = tempfile::tempdir().unwrap();
        fixture(source.path(), "lab.catalog-retry", "1.0.0");
        let inspected = inspect_plugin_source(&store, source.path()).await.unwrap();
        let mut installed = install_plugin(
            &store,
            &app.path().join("plugins"),
            InstallPluginRequest {
                source_path: source.path().to_string_lossy().into_owned(),
                expected_digest: inspected.inspection.manifest_digest,
                expected_old_digest: None,
            },
        )
        .await
        .unwrap();
        installed.phase = PluginPhase::Removing;
        installed.cleanup_pending = true;
        save_plugin(&store, &installed).await.unwrap();

        let result = remove_plugin(
            &store,
            &app.path().join("plugins"),
            RemovePluginRequest {
                installation_id: installed.installation_id,
                expected_digest: installed.digest.clone(),
            },
        )
        .await
        .unwrap();
        assert_eq!(result.status, "removed");
        assert!(skill_by_id(&store, installed.skills[0].skill_id)
            .await
            .unwrap()
            .is_none());
        assert!(store
            .get_json::<SkillInstallationReceipt>(
                crate::skill_settings::SKILL_INSTALLATION_KIND,
                &installed.skills[0].skill_id.to_string(),
            )
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn interrupted_update_recovery_retires_predecessor_before_activation() {
        let store = Store::open_in_memory().await.unwrap();
        let app = tempfile::tempdir().unwrap();
        let old_source = tempfile::tempdir().unwrap();
        let new_source = tempfile::tempdir().unwrap();
        fixture(old_source.path(), "lab.update", "1.0.0");
        let old_inspection = inspect_plugin_source(&store, old_source.path())
            .await
            .unwrap();
        let mut old = install_plugin(
            &store,
            &app.path().join("plugins"),
            InstallPluginRequest {
                source_path: old_source.path().to_string_lossy().into_owned(),
                expected_digest: old_inspection.inspection.manifest_digest,
                expected_old_digest: None,
            },
        )
        .await
        .unwrap();
        old = set_plugin_enabled(
            &store,
            SetPluginEnabledRequest {
                installation_id: old.installation_id,
                enabled: true,
            },
        )
        .await
        .unwrap();

        fixture(new_source.path(), "lab.update", "2.0.0");
        let new_inspection = inspect_plugin_source(&store, new_source.path())
            .await
            .unwrap();
        let request = InstallPluginRequest {
            source_path: new_source.path().to_string_lossy().into_owned(),
            expected_digest: new_inspection.inspection.manifest_digest,
            expected_old_digest: Some(old.digest.clone()),
        };
        let mut current = install_plugin(&store, &app.path().join("plugins"), request.clone())
            .await
            .unwrap();

        old.phase = PluginPhase::Installed;
        old.enabled = true;
        save_plugin(&store, &old).await.unwrap();
        store
            .set_skill_enabled_atomic(old.skills[0].skill_id, true)
            .await
            .unwrap();
        current.phase = PluginPhase::NeedsAttention;
        current.last_error = Some("simulated crash before predecessor retirement".into());
        save_plugin(&store, &current).await.unwrap();

        let recovered = install_plugin(
            &store,
            &app.path().join("plugins"),
            InstallPluginRequest {
                expected_old_digest: Some(current.digest.clone()),
                ..request
            },
        )
        .await
        .unwrap();
        assert_eq!(recovered.phase, PluginPhase::Installed);
        let retired = plugin_by_id(&store, old.installation_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(retired.phase, PluginPhase::Removed);
        assert!(
            !skill_by_id(&store, old.skills[0].skill_id)
                .await
                .unwrap()
                .unwrap()
                .enabled
        );
    }

    async fn repository_cleanup_catalog_for_test(store: &Store, plugin: &InstalledPlugin) {
        for owned in &plugin.skills {
            store.delete_skill_package(owned.skill_id).await.unwrap();
            store
                .delete_json(
                    crate::skill_settings::SKILL_INSTALLATION_KIND,
                    &owned.skill_id.to_string(),
                )
                .await
                .unwrap();
        }
    }
}
