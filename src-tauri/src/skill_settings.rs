use std::{
    collections::BTreeMap,
    fs::{self, File},
    io::Read,
    path::{Component, Path, PathBuf},
    sync::OnceLock,
};

use omicsops_core::workspace::SkillPackage;
use omicsops_dto::{
    RemoveSkillRequest, SkillFilePreview, SkillInstallationReceipt, SkillOrigin, SkillRemovalMode,
    SkillRemovalOperation, SkillRemovalResult, SkillSettingsDetail, SkillSettingsFile,
    SkillSettingsPackage,
};
use omicsops_store::Store;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::State;
use uuid::Uuid;

use crate::{
    commands::AppState,
    composer_quotes::is_credential_filename,
    composer_references::public_text,
    skill_commands::{skill_dependencies, skill_name_matches_dependency},
};

pub(crate) const SKILL_INSTALLATION_KIND: &str = "skill_installation_v1";
const SKILL_REMOVAL_KIND: &str = "skill_removal_v1";
static RECEIPT_MUTATION: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
const MAX_INVENTORY_DEPTH: usize = 16;
const MAX_INVENTORY_FILES: usize = 4_096;
const MAX_INVENTORY_BYTES: u64 = 100 * 1024 * 1024;
const MAX_PREVIEW_BYTES: u64 = 256 * 1024;
const MAX_DEPENDENCY_MANIFEST_BYTES: u64 = 512 * 1024;
const MAX_PENDING_REMOVALS: usize = 100;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SkillRemovalJournal {
    operation_id: Uuid,
    skill_id: Uuid,
    name: String,
    package_sha256: String,
    original_root: String,
    quarantine_root: String,
    inventory_sha256: String,
    #[serde(default)]
    files: Vec<RemovalFile>,
    phase: String,
    preserved_files: bool,
    last_error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RemovalFile {
    relative_path: String,
    size_bytes: u64,
    sha256: String,
}

impl SkillRemovalJournal {
    fn public(&self) -> SkillRemovalOperation {
        SkillRemovalOperation {
            operation_id: self.operation_id,
            skill_id: self.skill_id,
            name: public_text(&self.name),
            package_sha256: self.package_sha256.clone(),
            phase: self.phase.clone(),
            preserved_files: self.preserved_files,
        }
    }

    fn unfinished(&self) -> bool {
        !matches!(self.phase.as_str(), "removed" | "restored")
    }
}

/// Persist the host's ownership evidence for one concrete catalog record.
/// Strong protected origins are never weakened by a later deduplicated import.
pub(crate) async fn record_installation_receipt(
    repository: &Store,
    skill: &SkillPackage,
    requested_origin: SkillOrigin,
    owns_files: bool,
) -> Result<SkillInstallationReceipt, String> {
    let _guard = RECEIPT_MUTATION
        .get_or_init(|| tokio::sync::Mutex::new(()))
        .lock()
        .await;
    ensure_skill_mutation_allowed(repository, skill.id).await?;
    let key = skill.id.to_string();
    let existing = repository
        .get_json::<SkillInstallationReceipt>(SKILL_INSTALLATION_KIND, &key)
        .await
        .map_err(|error| error.to_string())?;
    if let Some(existing) = &existing {
        if existing.skill_id != skill.id
            || existing.package_sha256 != skill.sha256
            || !same_path(&existing.installed_root, &skill.source_path)
        {
            return Err("skill installation receipt conflicts with the catalog record".into());
        }
        if existing.phase != "active" {
            return Err("skill package has an unfinished removal operation".into());
        }
    }

    let (origin, owns_files, plugin_installation_id) = match (existing.as_ref(), requested_origin) {
        (Some(receipt), _) if receipt.origin == SkillOrigin::PluginOwned => (
            SkillOrigin::PluginOwned,
            false,
            receipt.plugin_installation_id,
        ),
        (_, SkillOrigin::Bundled) => (SkillOrigin::Bundled, false, None),
        (Some(receipt), SkillOrigin::ManagedImport)
            if owns_files && receipt.origin == SkillOrigin::LegacyUnknown =>
        {
            (SkillOrigin::ManagedImport, true, None)
        }
        (Some(receipt), SkillOrigin::ManagedImport) => (
            receipt.origin,
            receipt.owns_files,
            receipt.plugin_installation_id,
        ),
        (Some(receipt), _) => (
            receipt.origin,
            receipt.owns_files,
            receipt.plugin_installation_id,
        ),
        (None, origin) => (origin, owns_files, None),
    };
    let receipt = SkillInstallationReceipt {
        skill_id: skill.id,
        package_sha256: skill.sha256.clone(),
        installed_root: skill.source_path.clone(),
        origin,
        owns_files,
        plugin_installation_id,
        phase: "active".into(),
    };
    repository
        .put_json(SKILL_INSTALLATION_KIND, &key, &receipt)
        .await
        .map_err(|error| error.to_string())?;
    Ok(receipt)
}

/// Record the ownership of one plugin-specific catalog row without allowing
/// a plugin install to claim a bundled or independently imported package.
pub(crate) async fn record_plugin_owned_receipt(
    repository: &Store,
    skill: &SkillPackage,
    installation_id: Uuid,
) -> Result<SkillInstallationReceipt, String> {
    let _guard = RECEIPT_MUTATION
        .get_or_init(|| tokio::sync::Mutex::new(()))
        .lock()
        .await;
    ensure_skill_mutation_allowed(repository, skill.id).await?;
    let key = skill.id.to_string();
    let existing = repository
        .get_json::<SkillInstallationReceipt>(SKILL_INSTALLATION_KIND, &key)
        .await
        .map_err(|error| error.to_string())?;
    if let Some(existing) = &existing {
        if existing.skill_id != skill.id
            || existing.package_sha256 != skill.sha256
            || !same_path(&existing.installed_root, &skill.source_path)
        {
            return Err("skill installation receipt conflicts with the catalog record".into());
        }
        if existing.origin != SkillOrigin::PluginOwned
            || existing.plugin_installation_id != Some(installation_id)
        {
            return Err("plugin installation cannot claim an existing skill package".into());
        }
        if existing.phase != "active" {
            return Err("skill package has an unfinished removal operation".into());
        }
    }
    let receipt = SkillInstallationReceipt {
        skill_id: skill.id,
        package_sha256: skill.sha256.clone(),
        installed_root: skill.source_path.clone(),
        origin: SkillOrigin::PluginOwned,
        owns_files: false,
        plugin_installation_id: Some(installation_id),
        phase: "active".into(),
    };
    repository
        .put_json(SKILL_INSTALLATION_KIND, &key, &receipt)
        .await
        .map_err(|error| error.to_string())?;
    Ok(receipt)
}

pub(crate) async fn installation_receipt(
    repository: &Store,
    skill: &SkillPackage,
) -> Result<Option<SkillInstallationReceipt>, String> {
    let receipt = repository
        .get_json::<SkillInstallationReceipt>(SKILL_INSTALLATION_KIND, &skill.id.to_string())
        .await
        .map_err(|error| error.to_string())?;
    if let Some(receipt) = &receipt {
        if receipt.skill_id != skill.id
            || receipt.package_sha256 != skill.sha256
            || !same_path(&receipt.installed_root, &skill.source_path)
        {
            return Err("skill installation receipt conflicts with the catalog record".into());
        }
    }
    Ok(receipt)
}

pub(crate) async fn ensure_skill_mutation_allowed(
    repository: &Store,
    skill_id: Uuid,
) -> Result<(), String> {
    let pending = repository
        .list_json::<SkillRemovalJournal>(SKILL_REMOVAL_KIND)
        .await
        .map_err(|error| error.to_string())?
        .into_iter()
        .any(|operation| operation.skill_id == skill_id && operation.unfinished());
    if pending {
        Err("skill package has an unfinished removal operation".into())
    } else {
        Ok(())
    }
}

#[tauri::command]
pub async fn settings_skill_detail(
    state: State<'_, AppState>,
    skill_id: Uuid,
) -> Result<SkillSettingsDetail, String> {
    let _guard = state.skills_gate.read().await;
    let mut detail =
        settings_skill_detail_for_repository(&state.repository, &state.skills_root, skill_id)
            .await?;
    let active_run_count = state
        .active_runs
        .lock()
        .map_err(|_| "active run registry is unavailable".to_owned())?
        .len();
    let durable_run_count = state
        .repository
        .skill_removal_blocking_run_ids()
        .await
        .map_err(|error| error.to_string())?
        .len();
    let blocking_run_count = active_run_count.max(durable_run_count);
    apply_run_blocker(&mut detail, blocking_run_count);
    Ok(detail)
}

fn apply_run_blocker(detail: &mut SkillSettingsDetail, blocking_run_count: usize) {
    if blocking_run_count == 0 {
        return;
    }
    detail.can_remove_from_library = false;
    detail.can_delete_files = false;
    detail.blocking_reasons.push(format!(
        "{blocking_run_count} active or recoverable run(s) still require live Skills"
    ));
}

pub(crate) async fn settings_skill_detail_for_repository(
    repository: &Store,
    skills_root: &Path,
    skill_id: Uuid,
) -> Result<SkillSettingsDetail, String> {
    let packages = repository
        .list_skill_packages()
        .await
        .map_err(|_| "skill library could not be loaded".to_owned())?;
    let skill = packages
        .iter()
        .find(|skill| skill.id == skill_id)
        .cloned()
        .ok_or_else(|| "skill package was not found".to_owned())?;
    let receipt = installation_receipt(repository, &skill).await?;
    let origin = receipt
        .as_ref()
        .map(|receipt| receipt.origin)
        .unwrap_or_else(|| {
            if path_is_lexically_within(Path::new(&skill.source_path), skills_root) {
                SkillOrigin::LegacyUnknown
            } else {
                SkillOrigin::External
            }
        });
    let managed_root = settings_package_root(repository, skills_root, &skill, receipt.as_ref())
        .await
        .ok();
    let mut blocking_reasons = Vec::new();
    let (files, inventory_complete, integrity) = if let Some(root) = managed_root.as_ref() {
        match inspect_inventory(root) {
            Ok(inventory) => {
                let package_hash = inventory
                    .complete
                    .then(|| hash_inventory(root, &inventory).ok())
                    .flatten();
                let integrity = if !inventory.complete {
                    blocking_reasons
                        .push("file inventory exceeds the safe inspection limit".into());
                    "incomplete"
                } else if package_hash.as_deref() == Some(skill.sha256.as_str()) {
                    "verified"
                } else {
                    blocking_reasons.push("installed package contents have changed".into());
                    "changed"
                };
                (inventory.files, inventory.complete, integrity)
            }
            Err(_) => {
                blocking_reasons
                    .push("installed package contains an unsafe filesystem entry".into());
                (Vec::new(), false, "unsafe")
            }
        }
    } else {
        blocking_reasons.push("files are outside the managed skill library".into());
        (Vec::new(), false, "unavailable")
    };
    let (dependent_skills, dependencies_complete) =
        dependent_skills(repository, &packages, &skill, skills_root).await;
    if !dependencies_complete {
        blocking_reasons.push("dependency inspection is incomplete".into());
    }
    if !dependent_skills.is_empty() {
        blocking_reasons.push("enabled skills still depend on this package".into());
    }
    let protected = matches!(origin, SkillOrigin::Bundled | SkillOrigin::PluginOwned);
    if origin == SkillOrigin::Bundled {
        blocking_reasons.push("bundled skills can be disabled but not removed".into());
    } else if origin == SkillOrigin::PluginOwned {
        blocking_reasons.push("this skill is managed by its plugin".into());
    }
    let owns_files = receipt
        .as_ref()
        .is_some_and(|receipt| receipt.origin == SkillOrigin::ManagedImport && receipt.owns_files);
    let dependencies_allow_removal = dependencies_complete && dependent_skills.is_empty();
    let can_remove_from_library = !protected && dependencies_allow_removal;
    let can_delete_files = owns_files
        && integrity == "verified"
        && inventory_complete
        && managed_root.is_some()
        && dependencies_allow_removal;

    Ok(SkillSettingsDetail {
        skill: sanitized_skill(&skill),
        origin,
        integrity: integrity.into(),
        files,
        inventory_complete,
        dependent_skills,
        can_remove_from_library,
        can_delete_files,
        blocking_reasons,
    })
}

#[tauri::command]
pub async fn settings_read_skill_file(
    state: State<'_, AppState>,
    skill_id: Uuid,
    relative_path: String,
    expected_package_sha256: String,
) -> Result<SkillFilePreview, String> {
    let _guard = state.skills_gate.read().await;
    settings_read_skill_file_for_repository(
        &state.repository,
        &state.skills_root,
        skill_id,
        &relative_path,
        &expected_package_sha256,
    )
    .await
}

pub(crate) async fn settings_read_skill_file_for_repository(
    repository: &Store,
    skills_root: &Path,
    skill_id: Uuid,
    relative_path: &str,
    expected_package_sha256: &str,
) -> Result<SkillFilePreview, String> {
    let skill = repository
        .list_skill_packages()
        .await
        .map_err(|_| "skill library could not be loaded".to_owned())?
        .into_iter()
        .find(|skill| skill.id == skill_id)
        .ok_or_else(|| "skill package was not found".to_owned())?;
    let receipt = installation_receipt(repository, &skill).await?;
    if expected_package_sha256 != skill.sha256 {
        return Err("skill package changed; reopen its details".into());
    }
    let root = settings_package_root(repository, skills_root, &skill, receipt.as_ref())
        .await
        .map_err(|_| "skill files are unavailable for preview".to_owned())?;
    let normalized = normalize_relative_path(relative_path)?;
    if normalized
        .iter()
        .any(|component| is_credential_filename(&component.to_string_lossy()))
    {
        return Err("credential-like skill files cannot be previewed".into());
    }
    if !is_previewable_extension(&normalized) {
        return Err("this skill file type cannot be previewed".into());
    }
    let inventory = inspect_inventory(&root)
        .map_err(|_| "skill package contains an unsafe filesystem entry".to_owned())?;
    if !inventory.complete {
        return Err("skill package exceeds the safe inspection limit".into());
    }
    let package_hash = hash_inventory(&root, &inventory)
        .map_err(|_| "skill package failed integrity validation".to_owned())?;
    if package_hash != skill.sha256 {
        return Err("skill package changed; reimport it before previewing files".into());
    }
    let normalized_text = normalized_path_text(&normalized)?;
    if !inventory
        .files
        .iter()
        .any(|file| file.relative_path == normalized_text && file.previewable)
    {
        return Err("skill file was not found in the verified inventory".into());
    }

    let path = root.join(&normalized);
    validate_path_components(&root, &normalized)?;
    let expected_identity = inventory
        .identities
        .get(&normalized_text)
        .ok_or_else(|| "skill file identity is unavailable".to_owned())?;
    let file = open_verified_regular_file(&path, expected_identity, || {})?;
    let metadata = file
        .metadata()
        .map_err(|_| "skill file could not be inspected".to_owned())?;
    if !metadata.is_file() || is_reparse(&metadata) || metadata.len() > MAX_PREVIEW_BYTES {
        return Err("skill file is unavailable for bounded text preview".into());
    }
    let mut bytes = Vec::new();
    file.take(MAX_PREVIEW_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "skill file could not be read".to_owned())?;
    if bytes.len() as u64 > MAX_PREVIEW_BYTES {
        return Err("skill file is too large to preview".into());
    }
    let raw =
        String::from_utf8(bytes).map_err(|_| "skill file is not valid UTF-8 text".to_owned())?;
    if raw.chars().any(|character| {
        character == '\0' || character.is_control() && !"\n\r\t".contains(character)
    }) {
        return Err("skill file contains unsupported control characters".into());
    }
    let content = public_text(&raw);
    Ok(SkillFilePreview {
        relative_path: public_text(&normalized_text),
        redacted: content != raw,
        content,
        package_sha256: skill.sha256,
    })
}

#[tauri::command]
pub async fn settings_remove_skill(
    state: State<'_, AppState>,
    request: RemoveSkillRequest,
) -> Result<SkillRemovalResult, String> {
    let _guard = state.skills_gate.write().await;
    let active_run_count = state
        .active_runs
        .lock()
        .map_err(|_| "active run registry is unavailable".to_owned())?
        .len();
    remove_skill_for_repository(
        &state.repository,
        &state.skills_root,
        active_run_count,
        request,
    )
    .await
}

#[tauri::command]
pub async fn settings_list_skill_removals(
    state: State<'_, AppState>,
) -> Result<Vec<SkillRemovalOperation>, String> {
    let _guard = state.skills_gate.read().await;
    list_skill_removals(&state.repository).await
}

#[tauri::command]
pub async fn settings_retry_skill_removal(
    state: State<'_, AppState>,
    operation_id: Uuid,
) -> Result<SkillRemovalResult, String> {
    let _guard = state.skills_gate.write().await;
    retry_skill_removal(&state.repository, &state.skills_root, operation_id).await
}

async fn list_skill_removals(repository: &Store) -> Result<Vec<SkillRemovalOperation>, String> {
    let mut operations = repository
        .list_json::<SkillRemovalJournal>(SKILL_REMOVAL_KIND)
        .await
        .map_err(|error| error.to_string())?
        .into_iter()
        .filter(SkillRemovalJournal::unfinished)
        .map(|operation| operation.public())
        .collect::<Vec<_>>();
    operations.sort_by(|left, right| left.operation_id.cmp(&right.operation_id));
    Ok(operations)
}

pub(crate) async fn reconcile_skill_removals(
    repository: &Store,
    skills_root: &Path,
) -> Result<(), String> {
    let catalog_ids = repository
        .list_skill_packages()
        .await
        .map_err(|error| error.to_string())?
        .into_iter()
        .map(|skill| skill.id)
        .collect::<std::collections::BTreeSet<_>>();
    for mut operation in repository
        .list_json::<SkillRemovalJournal>(SKILL_REMOVAL_KIND)
        .await
        .map_err(|error| error.to_string())?
        .into_iter()
        .filter(SkillRemovalJournal::unfinished)
    {
        let expected = quarantine_root(skills_root, operation.operation_id)?;
        if !same_path(&operation.quarantine_root, &expected.to_string_lossy()) {
            operation.phase = "needs_attention".into();
            operation.preserved_files = true;
            operation.last_error = Some("recorded quarantine path is invalid".into());
            save_removal_journal(repository, &operation).await?;
            continue;
        }
        if catalog_ids.contains(&operation.skill_id) {
            let _ = restore_precommit_removal(repository, skills_root, operation).await;
        } else if !filesystem_entry_exists(&expected)? {
            operation.phase = "removed".into();
            operation.preserved_files = false;
            operation.last_error = None;
            save_removal_journal(repository, &operation).await?;
        } else {
            operation.phase = "needs_attention".into();
            operation.preserved_files = true;
            operation.last_error = Some(
                "an earlier removal was interrupted; retry verified cleanup from Settings".into(),
            );
            save_removal_journal(repository, &operation).await?;
        }
    }
    Ok(())
}

async fn remove_skill_for_repository(
    repository: &Store,
    skills_root: &Path,
    active_run_count: usize,
    request: RemoveSkillRequest,
) -> Result<SkillRemovalResult, String> {
    let packages = repository
        .list_skill_packages()
        .await
        .map_err(|error| error.to_string())?;
    let Some(skill) = packages
        .iter()
        .find(|skill| skill.id == request.skill_id)
        .cloned()
    else {
        return removed_skill_result(repository, &request).await;
    };
    if skill.sha256 != request.expected_package_sha256 {
        return Err("skill package changed; reopen its details".into());
    }
    ensure_skill_mutation_allowed(repository, skill.id).await?;
    ensure_removal_not_in_use(repository, active_run_count).await?;
    let (dependent, complete) = dependent_skills(repository, &packages, &skill, skills_root).await;
    if !complete {
        return Err("skill dependency inspection is incomplete".into());
    }
    if !dependent.is_empty() {
        return Err(format!(
            "{} enabled skill(s) still depend on this package",
            dependent.len()
        ));
    }
    let existing_receipt = installation_receipt(repository, &skill).await?;
    let origin = existing_receipt
        .as_ref()
        .map(|receipt| receipt.origin)
        .unwrap_or_else(|| {
            if path_is_lexically_within(Path::new(&skill.source_path), skills_root) {
                SkillOrigin::LegacyUnknown
            } else {
                SkillOrigin::External
            }
        });
    match origin {
        SkillOrigin::Bundled => {
            return Err("bundled skills can be disabled but not removed".into());
        }
        SkillOrigin::PluginOwned => {
            return Err("this Skill is owned by a plugin; manage it from Plugins".into());
        }
        _ => {}
    }

    if request.mode == SkillRemovalMode::LibraryOnly {
        let mut receipt = existing_receipt.unwrap_or_else(|| SkillInstallationReceipt {
            skill_id: skill.id,
            package_sha256: skill.sha256.clone(),
            installed_root: skill.source_path.clone(),
            origin,
            owns_files: false,
            plugin_installation_id: None,
            phase: "active".into(),
        });
        receipt.phase = "removed_library".into();
        repository
            .commit_skill_library_removal(
                skill.id,
                &skill.sha256,
                SKILL_INSTALLATION_KIND,
                &receipt,
                None::<(&str, &str, &SkillRemovalJournal)>,
            )
            .await
            .map_err(|error| error.to_string())?;
        return Ok(SkillRemovalResult {
            removed_from_library: true,
            files_removed: false,
            preserved_files: true,
            status: "removed_library".into(),
            message: "Skill was removed from the library; installation files were preserved."
                .into(),
        });
    }

    let receipt = existing_receipt.ok_or_else(|| {
        "installation ownership is unknown; only library removal is allowed".to_owned()
    })?;
    if receipt.origin != SkillOrigin::ManagedImport || !receipt.owns_files {
        return Err("only independently managed imports can delete installation files".into());
    }
    if list_skill_removals(repository).await?.len() >= MAX_PENDING_REMOVALS {
        return Err(
            "too many unfinished Skill removals; retry them before starting another".into(),
        );
    }
    let original_root = exact_owned_root(skills_root, &skill)?;
    ensure_no_overlapping_skill_roots(&packages, &skill, &original_root)?;
    let inventory = inspect_inventory(&original_root)?;
    let package_hash = verified_inventory_hash(&original_root, &inventory, &skill.sha256)?;
    let files = removal_file_manifest(&original_root, &inventory)?;
    let operation_id = Uuid::new_v4();
    let quarantine = quarantine_root(skills_root, operation_id)?;
    let mut operation = SkillRemovalJournal {
        operation_id,
        skill_id: skill.id,
        name: skill.name.clone(),
        package_sha256: skill.sha256.clone(),
        original_root: original_root.to_string_lossy().into_owned(),
        quarantine_root: quarantine.to_string_lossy().into_owned(),
        inventory_sha256: package_hash,
        files,
        phase: "prepared".into(),
        preserved_files: true,
        last_error: None,
    };
    save_removal_journal(repository, &operation).await?;
    prepare_quarantine_parent(skills_root)?;
    if filesystem_entry_exists(&quarantine)? {
        operation.phase = "needs_attention".into();
        operation.last_error = Some("quarantine destination already exists".into());
        save_removal_journal(repository, &operation).await?;
        return Err("Skill removal quarantine already exists".into());
    }
    if let Err(error) = fs::rename(&original_root, &quarantine) {
        operation.phase = "restored".into();
        operation.last_error = Some(format!("could not quarantine installation: {error}"));
        save_removal_journal(repository, &operation).await?;
        return Err("Skill files could not be quarantined; the library was unchanged".into());
    }
    operation.phase = "quarantined".into();
    save_removal_journal(repository, &operation).await?;
    if let Err(error) = verify_full_quarantine(&operation, skills_root) {
        operation.phase = "needs_attention".into();
        operation.preserved_files = true;
        operation.last_error = Some(error);
        save_removal_journal(repository, &operation).await?;
        return Err("Skill quarantine verification failed; files were preserved".into());
    }
    let mut removed_receipt = receipt;
    removed_receipt.phase = "removed".into();
    operation.phase = "cleaning".into();
    repository
        .commit_skill_library_removal(
            skill.id,
            &skill.sha256,
            SKILL_INSTALLATION_KIND,
            &removed_receipt,
            Some((
                SKILL_REMOVAL_KIND,
                &operation.operation_id.to_string(),
                &operation,
            )),
        )
        .await
        .map_err(|error| error.to_string())?;
    finish_quarantine_cleanup(repository, skills_root, operation).await
}

async fn retry_skill_removal(
    repository: &Store,
    skills_root: &Path,
    operation_id: Uuid,
) -> Result<SkillRemovalResult, String> {
    let operation = repository
        .get_json::<SkillRemovalJournal>(SKILL_REMOVAL_KIND, &operation_id.to_string())
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "Skill removal operation was not found".to_owned())?;
    if operation.phase == "removed" {
        return Ok(SkillRemovalResult {
            removed_from_library: true,
            files_removed: true,
            preserved_files: false,
            status: "removed".into(),
            message: "Skill files were already removed.".into(),
        });
    }
    if operation.phase == "restored" {
        return Ok(restored_removal_result());
    }
    let catalog_present = repository
        .list_skill_packages()
        .await
        .map_err(|error| error.to_string())?
        .iter()
        .any(|skill| skill.id == operation.skill_id);
    if catalog_present {
        return restore_precommit_removal(repository, skills_root, operation).await;
    }
    finish_quarantine_cleanup(repository, skills_root, operation).await
}

async fn restore_precommit_removal(
    repository: &Store,
    skills_root: &Path,
    mut operation: SkillRemovalJournal,
) -> Result<SkillRemovalResult, String> {
    let canonical_skills = skills_root
        .canonicalize()
        .map_err(|_| "managed skill root is unavailable".to_owned())?;
    let name = normalize_relative_path(&operation.name)?;
    let digest = normalize_relative_path(&operation.package_sha256)?;
    if name.components().count() != 1
        || digest.components().count() != 1
        || operation.package_sha256.len() != 64
        || !operation
            .package_sha256
            .bytes()
            .all(|value| value.is_ascii_hexdigit())
    {
        return Err("recorded Skill identity is invalid".into());
    }
    let expected_original = canonical_skills.join(name).join(digest);
    if !same_path(
        &operation.original_root,
        &expected_original.to_string_lossy(),
    ) {
        operation.phase = "needs_attention".into();
        operation.last_error = Some("recorded original Skill path is invalid".into());
        save_removal_journal(repository, &operation).await?;
        return Err("Interrupted Skill removal paths require attention".into());
    }
    let quarantine = quarantine_root(skills_root, operation.operation_id)?;
    if filesystem_entry_exists(&quarantine)? {
        if let Err(error) = verify_full_quarantine(&operation, skills_root) {
            operation.phase = "needs_attention".into();
            operation.last_error = Some(error);
            save_removal_journal(repository, &operation).await?;
            return Err("Interrupted Skill quarantine changed; files were preserved".into());
        }
        if filesystem_entry_exists(&expected_original)? {
            operation.phase = "needs_attention".into();
            operation.last_error =
                Some("original path is occupied; quarantine was preserved".into());
            save_removal_journal(repository, &operation).await?;
            return Err("Original Skill path is occupied; quarantine was preserved".into());
        }
        let parent = expected_original
            .parent()
            .ok_or_else(|| "original Skill path is invalid".to_owned())?;
        if !filesystem_entry_exists(parent)? {
            fs::create_dir(parent)
                .map_err(|_| "original Skill parent could not be restored".to_owned())?;
        }
        validate_restore_parent(&canonical_skills, parent)?;
        fs::rename(&quarantine, &expected_original)
            .map_err(|_| "interrupted Skill quarantine could not be restored".to_owned())?;
    } else if filesystem_entry_exists(&expected_original)? {
        let relative = expected_original
            .strip_prefix(&canonical_skills)
            .map_err(|_| "restored Skill path escaped the managed root".to_owned())?;
        validate_path_components(&canonical_skills, relative)?;
        let resolved = expected_original
            .canonicalize()
            .map_err(|_| "restored Skill path is unavailable".to_owned())?;
        if !same_path(
            &resolved.to_string_lossy(),
            &expected_original.to_string_lossy(),
        ) {
            return Err("restored Skill path does not match its managed location".into());
        }
        let inventory = inspect_inventory(&expected_original)?;
        verified_inventory_hash(&expected_original, &inventory, &operation.package_sha256)?;
        if removal_file_manifest(&expected_original, &inventory)? != operation.files {
            operation.phase = "needs_attention".into();
            operation.last_error =
                Some("restored Skill inventory does not match its journal".into());
            save_removal_journal(repository, &operation).await?;
            return Err("Restored Skill files changed; operation requires attention".into());
        }
    } else {
        operation.phase = "needs_attention".into();
        operation.last_error = Some("both original and quarantine paths are unavailable".into());
        save_removal_journal(repository, &operation).await?;
        return Err("Interrupted Skill files are unavailable".into());
    }
    operation.phase = "restored".into();
    operation.preserved_files = true;
    operation.last_error = None;
    save_removal_journal(repository, &operation).await?;
    Ok(restored_removal_result())
}

fn validate_restore_parent(skills_root: &Path, parent: &Path) -> Result<(), String> {
    let relative = parent
        .strip_prefix(skills_root)
        .map_err(|_| "original Skill parent escaped the managed root".to_owned())?;
    validate_path_components(skills_root, relative)?;
    let resolved = parent
        .canonicalize()
        .map_err(|_| "original Skill parent is unavailable".to_owned())?;
    if !same_path(&resolved.to_string_lossy(), &parent.to_string_lossy()) {
        return Err("original Skill parent does not match its managed location".into());
    }
    Ok(())
}

async fn finish_quarantine_cleanup(
    repository: &Store,
    skills_root: &Path,
    mut operation: SkillRemovalJournal,
) -> Result<SkillRemovalResult, String> {
    let quarantine = match verify_remaining_quarantine(&operation, skills_root) {
        Ok(Some(path)) => path,
        Ok(None) => {
            operation.phase = "removed".into();
            operation.preserved_files = false;
            operation.last_error = None;
            save_removal_journal(repository, &operation).await?;
            return Ok(completed_removal_result());
        }
        Err(error) => {
            operation.phase = "needs_attention".into();
            operation.preserved_files = true;
            operation.last_error = Some(error);
            save_removal_journal(repository, &operation).await?;
            return Ok(partial_cleanup_result());
        }
    };
    if let Err(error) = remove_verified_inventory(&quarantine, &operation.files) {
        operation.phase = "needs_attention".into();
        operation.preserved_files = true;
        operation.last_error = Some(error);
        save_removal_journal(repository, &operation).await?;
        return Ok(partial_cleanup_result());
    }
    operation.phase = "removed".into();
    operation.preserved_files = false;
    operation.last_error = None;
    save_removal_journal(repository, &operation).await?;
    if let Some(parent) = quarantine.parent() {
        let _ = fs::remove_dir(parent);
    }
    if let Some(parent) = Path::new(&operation.original_root).parent() {
        let _ = fs::remove_dir(parent);
    }
    Ok(completed_removal_result())
}

fn completed_removal_result() -> SkillRemovalResult {
    SkillRemovalResult {
        removed_from_library: true,
        files_removed: true,
        preserved_files: false,
        status: "removed".into(),
        message: "Skill and its verified installation files were removed.".into(),
    }
}

fn restored_removal_result() -> SkillRemovalResult {
    SkillRemovalResult {
        removed_from_library: false,
        files_removed: false,
        preserved_files: true,
        status: "restored".into(),
        message: "Interrupted removal was restored; the Skill remains in the library.".into(),
    }
}

fn partial_cleanup_result() -> SkillRemovalResult {
    SkillRemovalResult {
        removed_from_library: true,
        files_removed: false,
        preserved_files: true,
        status: "needs_attention".into(),
        message: "Skill was removed from the library, but verified cleanup could not finish; files were preserved for retry.".into(),
    }
}

async fn removed_skill_result(
    repository: &Store,
    request: &RemoveSkillRequest,
) -> Result<SkillRemovalResult, String> {
    let receipt = repository
        .get_json::<SkillInstallationReceipt>(
            SKILL_INSTALLATION_KIND,
            &request.skill_id.to_string(),
        )
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "skill package was not found".to_owned())?;
    if receipt.package_sha256 != request.expected_package_sha256 {
        return Err("skill package was not found".into());
    }
    match receipt.phase.as_str() {
        "removed_library" => Ok(SkillRemovalResult {
            removed_from_library: true,
            files_removed: false,
            preserved_files: true,
            status: "removed_library".into(),
            message: "Skill was already removed from the library; files remain preserved.".into(),
        }),
        "removed" => {
            let pending = repository
                .list_json::<SkillRemovalJournal>(SKILL_REMOVAL_KIND)
                .await
                .map_err(|error| error.to_string())?
                .into_iter()
                .find(|operation| operation.skill_id == request.skill_id && operation.unfinished());
            if pending.is_some() {
                Ok(partial_cleanup_result())
            } else {
                Ok(SkillRemovalResult {
                    removed_from_library: true,
                    files_removed: true,
                    preserved_files: false,
                    status: "removed".into(),
                    message: "Skill was already removed.".into(),
                })
            }
        }
        _ => Err("skill package was not found".into()),
    }
}

async fn ensure_removal_not_in_use(
    repository: &Store,
    active_run_count: usize,
) -> Result<(), String> {
    let durable = repository
        .skill_removal_blocking_run_ids()
        .await
        .map_err(|error| error.to_string())?;
    if active_run_count > 0 || !durable.is_empty() {
        return Err(format!(
            "Skill removal is blocked by {} active or recoverable run(s)",
            active_run_count.max(durable.len())
        ));
    }
    Ok(())
}

fn exact_owned_root(skills_root: &Path, skill: &SkillPackage) -> Result<PathBuf, String> {
    let root = managed_package_root(skills_root, skill)?;
    let canonical_skills = skills_root
        .canonicalize()
        .map_err(|_| "managed skill root is unavailable".to_owned())?;
    let expected = canonical_skills.join(&skill.name).join(&skill.sha256);
    if !same_path(&root.to_string_lossy(), &expected.to_string_lossy()) {
        return Err("owned Skill path does not match the managed installation layout".into());
    }
    Ok(root)
}

fn ensure_no_overlapping_skill_roots(
    packages: &[SkillPackage],
    target: &SkillPackage,
    target_root: &Path,
) -> Result<(), String> {
    for other in packages.iter().filter(|other| other.id != target.id) {
        let path = Path::new(&other.source_path);
        let resolved = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        if resolved == target_root
            || resolved.starts_with(target_root)
            || target_root.starts_with(&resolved)
        {
            return Err("another Skill record shares or nests this installation root".into());
        }
    }
    Ok(())
}

fn verified_inventory_hash(
    root: &Path,
    inventory: &Inventory,
    expected_sha256: &str,
) -> Result<String, String> {
    if !inventory.complete {
        return Err("Skill inventory exceeds the safe deletion limit".into());
    }
    let hash = hash_inventory(root, inventory)?;
    if hash != expected_sha256 {
        return Err("installed package contents changed; files were preserved".into());
    }
    Ok(hash)
}

fn prepare_quarantine_parent(skills_root: &Path) -> Result<PathBuf, String> {
    let parent = skills_root.join(".removing");
    if !filesystem_entry_exists(&parent)? {
        fs::create_dir(&parent).map_err(|_| "Skill quarantine could not be created".to_owned())?;
    }
    let metadata = fs::symlink_metadata(&parent)
        .map_err(|_| "Skill quarantine could not be inspected".to_owned())?;
    if !metadata.is_dir() || is_reparse(&metadata) {
        return Err("Skill quarantine root is unsafe".into());
    }
    Ok(parent)
}

fn quarantine_root(skills_root: &Path, operation_id: Uuid) -> Result<PathBuf, String> {
    let canonical = skills_root
        .canonicalize()
        .map_err(|_| "managed skill root is unavailable".to_owned())?;
    Ok(canonical.join(".removing").join(operation_id.to_string()))
}

fn checked_quarantine_path(
    operation: &SkillRemovalJournal,
    skills_root: &Path,
) -> Result<PathBuf, String> {
    let expected = quarantine_root(skills_root, operation.operation_id)?;
    if !same_path(&operation.quarantine_root, &expected.to_string_lossy()) {
        return Err("recorded Skill quarantine path is invalid".into());
    }
    let parent = expected
        .parent()
        .ok_or_else(|| "Skill quarantine path is invalid".to_owned())?;
    let parent_metadata = fs::symlink_metadata(parent)
        .map_err(|_| "Skill quarantine root is unavailable".to_owned())?;
    let metadata = fs::symlink_metadata(&expected)
        .map_err(|_| "Skill quarantine is unavailable".to_owned())?;
    if !parent_metadata.is_dir()
        || is_reparse(&parent_metadata)
        || !metadata.is_dir()
        || is_reparse(&metadata)
    {
        return Err("Skill quarantine contains an unsafe filesystem entry".into());
    }
    Ok(expected)
}

fn verify_full_quarantine(
    operation: &SkillRemovalJournal,
    skills_root: &Path,
) -> Result<PathBuf, String> {
    let expected = checked_quarantine_path(operation, skills_root)?;
    let inventory = inspect_inventory(&expected)?;
    let hash = verified_inventory_hash(&expected, &inventory, &operation.package_sha256)?;
    if hash != operation.inventory_sha256 {
        return Err("Skill quarantine inventory no longer matches its journal".into());
    }
    if operation.files.is_empty()
        || removal_file_manifest(&expected, &inventory)? != operation.files
    {
        return Err("Skill quarantine file proof no longer matches its journal".into());
    }
    Ok(expected)
}

fn verify_remaining_quarantine(
    operation: &SkillRemovalJournal,
    skills_root: &Path,
) -> Result<Option<PathBuf>, String> {
    let expected = quarantine_root(skills_root, operation.operation_id)?;
    if !same_path(&operation.quarantine_root, &expected.to_string_lossy()) {
        return Err("recorded Skill quarantine path is invalid".into());
    }
    if !filesystem_entry_exists(&expected)? {
        return Ok(None);
    }
    let expected = checked_quarantine_path(operation, skills_root)?;
    let inventory = inspect_inventory(&expected)?;
    validate_remaining_manifest(&expected, &inventory, &operation.files)?;
    Ok(Some(expected))
}

fn filesystem_entry_exists(path: &Path) -> Result<bool, String> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(_) => Err("Skill filesystem entry could not be inspected".into()),
    }
}

async fn save_removal_journal(
    repository: &Store,
    operation: &SkillRemovalJournal,
) -> Result<(), String> {
    repository
        .put_json(
            SKILL_REMOVAL_KIND,
            &operation.operation_id.to_string(),
            operation,
        )
        .await
        .map_err(|error| error.to_string())
}

fn remove_verified_inventory(root: &Path, original_files: &[RemovalFile]) -> Result<(), String> {
    let inventory = inspect_inventory(root)?;
    if !inventory.complete {
        return Err("Skill quarantine inventory is incomplete".into());
    }
    validate_remaining_manifest(root, &inventory, original_files)?;
    let mut directories = collect_safe_directories(root)?;
    for (relative, identity) in &inventory.identities {
        let path = root.join(normalize_relative_path(relative)?);
        remove_verified_file(&path, identity)?;
    }
    directories.sort_by_key(|path| std::cmp::Reverse(path.components().count()));
    for directory in directories {
        fs::remove_dir(&directory)
            .map_err(|_| "Skill cleanup stopped because a directory was not empty".to_owned())?;
    }
    fs::remove_dir(root)
        .map_err(|_| "Skill cleanup stopped because quarantine was not empty".to_owned())
}

fn removal_file_manifest(root: &Path, inventory: &Inventory) -> Result<Vec<RemovalFile>, String> {
    if !inventory.complete || inventory.identities.len() != inventory.sizes.len() {
        return Err("Skill inventory is incomplete".into());
    }
    let mut files = Vec::with_capacity(inventory.identities.len());
    for (relative, identity) in &inventory.identities {
        let size_bytes = inventory
            .sizes
            .get(relative)
            .copied()
            .ok_or_else(|| "Skill file size is unavailable".to_owned())?;
        let path = root.join(normalize_relative_path(relative)?);
        files.push(RemovalFile {
            relative_path: relative.clone(),
            size_bytes,
            sha256: hash_verified_file(&path, identity, size_bytes)?,
        });
    }
    Ok(files)
}

fn validate_remaining_manifest(
    root: &Path,
    inventory: &Inventory,
    original_files: &[RemovalFile],
) -> Result<(), String> {
    if original_files.is_empty() || !inventory.complete {
        return Err("Skill removal journal has no complete file proof".into());
    }
    let original = original_files
        .iter()
        .map(|file| (file.relative_path.as_str(), file))
        .collect::<BTreeMap<_, _>>();
    if original.len() != original_files.len() {
        return Err("Skill removal journal contains duplicate file proof".into());
    }
    for (relative, identity) in &inventory.identities {
        let proof = original
            .get(relative.as_str())
            .ok_or_else(|| "Skill quarantine contains a file absent from its journal".to_owned())?;
        let size = inventory
            .sizes
            .get(relative)
            .copied()
            .ok_or_else(|| "Skill quarantine file size is unavailable".to_owned())?;
        if size != proof.size_bytes {
            return Err("Skill quarantine file size changed after journaling".into());
        }
        let path = root.join(normalize_relative_path(relative)?);
        if hash_verified_file(&path, identity, size)? != proof.sha256 {
            return Err("Skill quarantine file changed after journaling".into());
        }
    }
    Ok(())
}

fn hash_verified_file(
    path: &Path,
    identity: &FileIdentity,
    expected_size: u64,
) -> Result<String, String> {
    let mut file = open_verified_regular_file(path, identity, || {})?;
    let metadata = file
        .metadata()
        .map_err(|_| "Skill file could not be inspected".to_owned())?;
    if metadata.len() != expected_size {
        return Err("Skill file size changed during validation".into());
    }
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; 1024 * 1024];
    let mut read_total = 0_u64;
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|_| "Skill file could not be read".to_owned())?;
        if read == 0 {
            break;
        }
        read_total = read_total.saturating_add(read as u64);
        if read_total > expected_size {
            return Err("Skill file grew during validation".into());
        }
        hasher.update(&buffer[..read]);
    }
    if read_total != expected_size {
        return Err("Skill file changed during validation".into());
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn collect_safe_directories(root: &Path) -> Result<Vec<PathBuf>, String> {
    fn visit(root: &Path, current: &Path, result: &mut Vec<PathBuf>) -> Result<(), String> {
        for entry in fs::read_dir(current)
            .map_err(|_| "Skill quarantine could not be inspected".to_owned())?
        {
            let path = entry
                .map_err(|_| "Skill quarantine could not be inspected".to_owned())?
                .path();
            let metadata = fs::symlink_metadata(&path)
                .map_err(|_| "Skill quarantine entry could not be inspected".to_owned())?;
            if is_reparse(&metadata) {
                return Err("Skill quarantine contains a link or reparse point".into());
            }
            if metadata.is_dir() {
                if !path.starts_with(root) {
                    return Err("Skill quarantine entry escaped its root".into());
                }
                visit(root, &path, result)?;
                result.push(path);
            } else if !metadata.is_file() {
                return Err("Skill quarantine contains an unsupported entry".into());
            }
        }
        Ok(())
    }
    let mut result = Vec::new();
    visit(root, root, &mut result)?;
    Ok(result)
}

#[cfg(windows)]
fn remove_verified_file(path: &Path, expected_identity: &FileIdentity) -> Result<(), String> {
    use std::os::windows::{fs::OpenOptionsExt, io::AsRawHandle};
    use windows_sys::Win32::Storage::FileSystem::{
        DELETE, FILE_DISPOSITION_INFO, FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ,
        FileDispositionInfo, SetFileInformationByHandle,
    };
    let file = fs::OpenOptions::new()
        .access_mode(0x80000000 | DELETE)
        .share_mode(FILE_SHARE_READ)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
        .map_err(|_| "Skill file could not be opened for verified deletion".to_owned())?;
    let metadata = file
        .metadata()
        .map_err(|_| "Skill file could not be inspected before deletion".to_owned())?;
    let (identity, links) = open_file_identity(&file)?;
    if !metadata.is_file() || is_reparse(&metadata) || links != 1 || identity != *expected_identity
    {
        return Err("Skill file identity changed before deletion".into());
    }
    let disposition = FILE_DISPOSITION_INFO { DeleteFile: true };
    let succeeded = unsafe {
        SetFileInformationByHandle(
            file.as_raw_handle(),
            FileDispositionInfo,
            (&disposition as *const FILE_DISPOSITION_INFO).cast(),
            std::mem::size_of::<FILE_DISPOSITION_INFO>() as u32,
        )
    };
    if succeeded == 0 {
        return Err("Skill file could not be deleted safely".into());
    }
    drop(file);
    Ok(())
}

#[cfg(not(windows))]
fn remove_verified_file(path: &Path, expected_identity: &FileIdentity) -> Result<(), String> {
    let file = open_verified_regular_file(path, expected_identity, || {})?;
    let current = inspect_file_identity(path)?;
    if current != *expected_identity {
        return Err("Skill file identity changed before deletion".into());
    }
    fs::remove_file(path).map_err(|_| "Skill file could not be deleted safely".to_owned())?;
    drop(file);
    Ok(())
}

async fn settings_package_root(
    repository: &Store,
    skills_root: &Path,
    skill: &SkillPackage,
    receipt: Option<&SkillInstallationReceipt>,
) -> Result<PathBuf, String> {
    if let Some(receipt) = receipt.filter(|receipt| receipt.origin == SkillOrigin::PluginOwned) {
        let app_data_root = skills_root
            .parent()
            .ok_or_else(|| "managed skill root has no application-data parent".to_owned())?;
        return crate::integration_packages::plugin_owned_skill_root(
            repository,
            app_data_root,
            receipt,
            skill,
        )
        .await;
    }
    managed_package_root(skills_root, skill)
}

fn sanitized_skill(skill: &SkillPackage) -> SkillSettingsPackage {
    SkillSettingsPackage {
        id: skill.id,
        name: public_text(&skill.name),
        version: public_text(&skill.version),
        source_path: public_text(&skill.source_path),
        sha256: public_text(&skill.sha256),
        enabled: skill.enabled,
        capabilities: skill
            .capabilities
            .iter()
            .map(|value| public_text(value))
            .collect(),
        category: skill.category.as_deref().map(public_text),
    }
}

async fn dependent_skills(
    repository: &Store,
    packages: &[SkillPackage],
    target: &SkillPackage,
    skills_root: &Path,
) -> (Vec<Uuid>, bool) {
    let mut available = std::collections::BTreeSet::new();
    let mut complete = true;
    for candidate in packages {
        match crate::integration_packages::plugin_allows_skill(repository, candidate.id).await {
            Ok(true) => {
                available.insert(candidate.id);
            }
            Ok(false) => {}
            Err(_) => {
                complete = false;
            }
        }
    }
    let mut effective = packages
        .iter()
        .filter(|candidate| candidate.enabled && available.contains(&candidate.id))
        .map(|candidate| candidate.id)
        .collect::<std::collections::BTreeSet<_>>();
    let mut inspected = std::collections::BTreeSet::new();
    let mut dependencies = BTreeMap::<Uuid, Vec<String>>::new();
    loop {
        let pending = effective
            .iter()
            .filter(|id| !inspected.contains(*id))
            .copied()
            .collect::<Vec<_>>();
        if pending.is_empty() {
            break;
        }
        for candidate_id in pending {
            inspected.insert(candidate_id);
            let Some(candidate) = packages.iter().find(|item| item.id == candidate_id) else {
                complete = false;
                continue;
            };
            let receipt = match installation_receipt(repository, candidate).await {
                Ok(receipt) => receipt,
                Err(_) => {
                    complete = false;
                    continue;
                }
            };
            let root =
                match settings_package_root(repository, skills_root, candidate, receipt.as_ref())
                    .await
                {
                    Ok(root) => root,
                    Err(_) => {
                        complete = false;
                        continue;
                    }
                };
            let declared = match read_skill_dependencies_bounded(&root) {
                Ok(declared) => declared,
                Err(_) => {
                    complete = false;
                    continue;
                }
            };
            for dependency in &declared {
                if let Some(package) = packages.iter().find(|package| {
                    available.contains(&package.id)
                        && skill_name_matches_dependency(&package.name, dependency)
                }) {
                    effective.insert(package.id);
                }
            }
            dependencies.insert(candidate.id, declared);
        }
    }
    let mut result = dependencies
        .into_iter()
        .filter(|(candidate_id, declared)| {
            *candidate_id != target.id
                && effective.contains(candidate_id)
                && declared
                    .iter()
                    .any(|dependency| skill_name_matches_dependency(&target.name, dependency))
        })
        .map(|(candidate_id, _)| candidate_id)
        .collect::<Vec<_>>();
    result.sort();
    result.dedup();
    (result, complete)
}

struct Inventory {
    files: Vec<SkillSettingsFile>,
    identities: BTreeMap<String, FileIdentity>,
    sizes: BTreeMap<String, u64>,
    complete: bool,
}

fn inspect_inventory(root: &Path) -> Result<Inventory, String> {
    let mut files = Vec::new();
    let mut identities = BTreeMap::new();
    let mut sizes = BTreeMap::new();
    let mut entry_count = 0_usize;
    let mut total_bytes = 0_u64;
    let mut complete = true;
    inspect_directory(
        root,
        root,
        0,
        &mut files,
        &mut entry_count,
        &mut total_bytes,
        &mut identities,
        &mut sizes,
        &mut complete,
    )?;
    files.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
    Ok(Inventory {
        files,
        identities,
        sizes,
        complete,
    })
}

fn inspect_directory(
    root: &Path,
    directory: &Path,
    depth: usize,
    files: &mut Vec<SkillSettingsFile>,
    entry_count: &mut usize,
    total_bytes: &mut u64,
    identities: &mut BTreeMap<String, FileIdentity>,
    sizes: &mut BTreeMap<String, u64>,
    complete: &mut bool,
) -> Result<(), String> {
    if depth > MAX_INVENTORY_DEPTH {
        *complete = false;
        return Ok(());
    }
    let entries =
        fs::read_dir(directory).map_err(|_| "skill directory could not be inspected".to_owned())?;
    for entry in entries {
        let entry = entry.map_err(|_| "skill directory could not be inspected".to_owned())?;
        *entry_count += 1;
        if *entry_count > MAX_INVENTORY_FILES {
            *complete = false;
            return Ok(());
        }
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path)
            .map_err(|_| "skill filesystem entry could not be inspected".to_owned())?;
        if is_reparse(&metadata) {
            return Err("skill package contains a link or reparse point".into());
        }
        if metadata.is_dir() {
            inspect_directory(
                root,
                &path,
                depth + 1,
                files,
                entry_count,
                total_bytes,
                identities,
                sizes,
                complete,
            )?;
            if !*complete {
                return Ok(());
            }
        } else if metadata.is_file() {
            *total_bytes = total_bytes.saturating_add(metadata.len());
            if *total_bytes > MAX_INVENTORY_BYTES {
                *complete = false;
                return Ok(());
            }
            let identity = inspect_file_identity(&path)?;
            let relative = path
                .strip_prefix(root)
                .map_err(|_| "skill file escaped its package".to_owned())?;
            let normalized = normalize_relative_path(&relative.to_string_lossy())?;
            let normalized_text = normalized_path_text(&normalized)?;
            identities.insert(normalized_text.clone(), identity);
            sizes.insert(normalized_text.clone(), metadata.len());
            if normalized
                .iter()
                .any(|part| is_credential_filename(&part.to_string_lossy()))
            {
                continue;
            }
            files.push(SkillSettingsFile {
                relative_path: public_text(&normalized_text),
                size_bytes: metadata.len(),
                previewable: metadata.len() <= MAX_PREVIEW_BYTES
                    && is_previewable_extension(&normalized),
            });
        } else {
            return Err("skill package contains an unsupported filesystem entry".into());
        }
    }
    Ok(())
}

fn hash_inventory(root: &Path, inventory: &Inventory) -> Result<String, String> {
    if !inventory.complete || inventory.identities.len() != inventory.sizes.len() {
        return Err("skill inventory is incomplete".into());
    }
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; 1024 * 1024];
    for (relative, identity) in &inventory.identities {
        let expected_size = inventory
            .sizes
            .get(relative)
            .copied()
            .ok_or_else(|| "skill file size is unavailable".to_owned())?;
        let normalized = normalize_relative_path(relative)?;
        let path = root.join(&normalized);
        validate_path_components(root, &normalized)?;
        let mut file = open_verified_regular_file(&path, identity, || {})?;
        let metadata = file
            .metadata()
            .map_err(|_| "skill file could not be inspected".to_owned())?;
        if metadata.len() != expected_size {
            return Err("skill file size changed during validation".into());
        }
        hasher.update((relative.len() as u64).to_le_bytes());
        hasher.update(relative.as_bytes());
        hasher.update(expected_size.to_le_bytes());
        let mut read_total = 0_u64;
        loop {
            let read = file
                .read(&mut buffer)
                .map_err(|_| "skill file could not be read".to_owned())?;
            if read == 0 {
                break;
            }
            read_total = read_total.saturating_add(read as u64);
            if read_total > expected_size {
                return Err("skill file grew during validation".into());
            }
            hasher.update(&buffer[..read]);
        }
        if read_total != expected_size {
            return Err("skill file changed during validation".into());
        }
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn managed_package_root(skills_root: &Path, skill: &SkillPackage) -> Result<PathBuf, String> {
    let root_metadata = fs::symlink_metadata(skills_root)
        .map_err(|_| "managed skill root is unavailable".to_owned())?;
    if !root_metadata.is_dir() || is_reparse(&root_metadata) {
        return Err("managed skill root is unsafe".into());
    }
    let canonical_root = skills_root
        .canonicalize()
        .map_err(|_| "managed skill root is unavailable".to_owned())?;
    let recorded = Path::new(&skill.source_path);
    let relative = recorded
        .strip_prefix(&canonical_root)
        .map_err(|_| "skill package is outside the managed root".to_owned())?;
    validate_path_components(&canonical_root, relative)?;
    let canonical_package = recorded
        .canonicalize()
        .map_err(|_| "skill package is unavailable".to_owned())?;
    if !canonical_package.starts_with(&canonical_root) || !canonical_package.is_dir() {
        return Err("skill package is outside the managed root".into());
    }
    Ok(canonical_package)
}

fn validate_path_components(root: &Path, relative: &Path) -> Result<(), String> {
    let mut current = root.to_path_buf();
    for component in relative.components() {
        let Component::Normal(part) = component else {
            return Err("skill path is unsafe".into());
        };
        current.push(part);
        let metadata =
            fs::symlink_metadata(&current).map_err(|_| "skill path is unavailable".to_owned())?;
        if is_reparse(&metadata) {
            return Err("skill path contains a link or reparse point".into());
        }
    }
    Ok(())
}

fn normalize_relative_path(value: &str) -> Result<PathBuf, String> {
    let normalized = value.replace('\\', "/");
    if normalized.is_empty()
        || normalized.starts_with('/')
        || normalized.contains(':')
        || normalized.chars().any(char::is_control)
    {
        return Err("skill file path is unsafe".into());
    }
    let mut result = PathBuf::new();
    for component in normalized.split('/') {
        if component.is_empty()
            || component == "."
            || component == ".."
            || component.ends_with(['.', ' '])
            || is_reserved_windows_name(component)
        {
            return Err("skill file path is unsafe".into());
        }
        result.push(component);
    }
    Ok(result)
}

fn is_reserved_windows_name(component: &str) -> bool {
    let stem = component
        .split('.')
        .next()
        .unwrap_or_default()
        .trim_end_matches(['.', ' '])
        .to_ascii_uppercase();
    matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || stem
            .strip_prefix("COM")
            .or_else(|| stem.strip_prefix("LPT"))
            .is_some_and(|number| {
                matches!(number, "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9")
            })
}

fn normalized_path_text(path: &Path) -> Result<String, String> {
    let parts = path
        .components()
        .map(|component| match component {
            Component::Normal(value) => value
                .to_str()
                .map(str::to_owned)
                .ok_or_else(|| "skill file name is not valid UTF-8".to_owned()),
            _ => Err("skill file path is unsafe".to_owned()),
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(parts.join("/"))
}

fn is_previewable_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|value| value.to_str())
        .is_some_and(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "md" | "py" | "r" | "sh" | "ps1" | "yml" | "yaml" | "toml" | "json" | "txt"
            )
        })
}

fn path_is_lexically_within(path: &Path, root: &Path) -> bool {
    match root.canonicalize() {
        Ok(root) => path.starts_with(root),
        Err(_) => path.starts_with(root),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FileIdentity {
    volume: u64,
    index: u64,
}

fn read_bounded_regular_text(root: &Path, relative: &Path, limit: u64) -> Result<String, String> {
    validate_path_components(root, relative)?;
    let path = root.join(relative);
    let metadata = fs::symlink_metadata(&path)
        .map_err(|_| "skill text file could not be inspected".to_owned())?;
    if !metadata.is_file() || is_reparse(&metadata) || metadata.len() > limit {
        return Err("skill text file is unavailable for bounded reading".into());
    }
    let identity = inspect_file_identity(&path)?;
    let file = open_verified_regular_file(&path, &identity, || {})?;
    let mut bytes = Vec::new();
    file.take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "skill text file could not be read".to_owned())?;
    if bytes.len() as u64 > limit {
        return Err("skill text file exceeds the safe reading limit".into());
    }
    String::from_utf8(bytes).map_err(|_| "skill text file is not valid UTF-8".into())
}

pub(crate) fn read_skill_dependencies_bounded(root: &Path) -> Result<Vec<String>, String> {
    read_bounded_regular_text(root, Path::new("SKILL.md"), MAX_DEPENDENCY_MANIFEST_BYTES)
        .map(|markdown| skill_dependencies(&markdown).into_iter().collect())
}

fn open_verified_regular_file(
    path: &Path,
    expected_identity: &FileIdentity,
    before_open: impl FnOnce(),
) -> Result<File, String> {
    before_open();
    let options = fs::OpenOptions::new();
    let file = open_regular_file_with_options(options, path)?;
    let metadata = file
        .metadata()
        .map_err(|_| "skill file could not be inspected".to_owned())?;
    let (identity, links) = open_file_identity(&file)?;
    if !metadata.is_file() || is_reparse(&metadata) || links != 1 || identity != *expected_identity
    {
        return Err("skill file identity changed before it could be opened".into());
    }
    Ok(file)
}

#[cfg(windows)]
fn open_regular_file_with_options(
    mut options: fs::OpenOptions,
    path: &Path,
) -> Result<File, String> {
    use std::os::windows::fs::OpenOptionsExt;
    const FILE_SHARE_READ: u32 = 0x00000001;
    const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x00200000;
    options
        .read(true)
        .share_mode(FILE_SHARE_READ)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
        .map_err(|_| "skill file could not be opened safely".to_owned())
}

#[cfg(not(windows))]
fn open_regular_file_with_options(
    mut options: fs::OpenOptions,
    path: &Path,
) -> Result<File, String> {
    options
        .read(true)
        .open(path)
        .map_err(|_| "skill file could not be opened safely".to_owned())
}

#[cfg(windows)]
fn is_reparse(metadata: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    metadata.file_attributes() & 0x400 != 0
}

fn inspect_file_identity(path: &Path) -> Result<FileIdentity, String> {
    let file = open_regular_file_with_options(fs::OpenOptions::new(), path)?;
    let metadata = file
        .metadata()
        .map_err(|_| "skill file could not be inspected".to_owned())?;
    let (identity, links) = open_file_identity(&file)?;
    if !metadata.is_file() || is_reparse(&metadata) || links != 1 {
        return Err("skill package contains a shared filesystem entry".into());
    }
    Ok(identity)
}

#[cfg(windows)]
fn open_file_identity(file: &File) -> Result<(FileIdentity, u64), String> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle,
    };
    let mut information = unsafe { std::mem::zeroed::<BY_HANDLE_FILE_INFORMATION>() };
    let succeeded = unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut information) };
    if succeeded == 0 {
        return Err("skill file identity could not be inspected".into());
    }
    Ok((
        FileIdentity {
            volume: u64::from(information.dwVolumeSerialNumber),
            index: (u64::from(information.nFileIndexHigh) << 32)
                | u64::from(information.nFileIndexLow),
        },
        u64::from(information.nNumberOfLinks),
    ))
}

#[cfg(unix)]
fn open_file_identity(file: &File) -> Result<(FileIdentity, u64), String> {
    use std::os::unix::fs::MetadataExt;
    let metadata = file
        .metadata()
        .map_err(|_| "skill file identity could not be inspected".to_owned())?;
    Ok((
        FileIdentity {
            volume: metadata.dev(),
            index: metadata.ino(),
        },
        metadata.nlink(),
    ))
}

#[cfg(not(windows))]
fn is_reparse(metadata: &fs::Metadata) -> bool {
    metadata.file_type().is_symlink()
}

fn same_path(left: &str, right: &str) -> bool {
    if cfg!(windows) {
        left.replace('/', "\\")
            .eq_ignore_ascii_case(&right.replace('/', "\\"))
    } else {
        Path::new(left) == Path::new(right)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn owned_import(
        store: &Store,
        temporary: &tempfile::TempDir,
        skills_root: &Path,
        source_name: &str,
    ) -> (SkillPackage, PathBuf) {
        let source = temporary.path().join(format!("source-{source_name}"));
        fs::create_dir(&source).unwrap();
        fs::write(
            source.join("SKILL.md"),
            format!("---\nname: {source_name}\n---\n# {source_name}\n"),
        )
        .unwrap();
        fs::create_dir(source.join("scripts")).unwrap();
        fs::write(source.join("scripts/run.py"), "print('source stays')\n").unwrap();
        let installed =
            omicsops_adapters::skills::install_skill_directory(&source, skills_root).unwrap();
        assert!(installed.created_new_directory);
        let skill = SkillPackage {
            id: Uuid::new_v4(),
            name: installed.name,
            version: installed.version,
            source_path: installed.install_path.to_string_lossy().into_owned(),
            sha256: installed.sha256,
            enabled: true,
            capabilities: installed.capabilities,
            category: None,
        };
        store.save_skill_package(&skill).await.unwrap();
        record_installation_receipt(store, &skill, SkillOrigin::ManagedImport, true)
            .await
            .unwrap();
        (skill, source)
    }

    async fn managed_skill(
        store: &Store,
        skills_root: &Path,
        extra_files: &[(&str, &[u8])],
    ) -> SkillPackage {
        let package_root = skills_root.join("sample").join("package");
        fs::create_dir_all(&package_root).unwrap();
        fs::write(
            package_root.join("SKILL.md"),
            "---\nname: sample\n---\n# Safe skill\n",
        )
        .unwrap();
        for (name, content) in extra_files {
            let path = package_root.join(name);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).unwrap();
            }
            fs::write(path, content).unwrap();
        }
        let inspection = omicsops_adapters::skills::inspect_skill_directory(&package_root).unwrap();
        let skill = SkillPackage {
            id: Uuid::new_v4(),
            name: "sample".into(),
            version: inspection.version,
            source_path: package_root
                .canonicalize()
                .unwrap()
                .to_string_lossy()
                .into_owned(),
            sha256: inspection.sha256,
            enabled: true,
            capabilities: vec!["read_project_files".into()],
            category: None,
        };
        store.save_skill_package(&skill).await.unwrap();
        record_installation_receipt(store, &skill, SkillOrigin::ManagedImport, true)
            .await
            .unwrap();
        skill
    }

    fn skill() -> SkillPackage {
        SkillPackage {
            id: Uuid::new_v4(),
            name: "sample".into(),
            version: "1.0.0".into(),
            source_path: r"C:\OmicsOps\skills\sample\sha".into(),
            sha256: "sha".into(),
            enabled: false,
            capabilities: vec![],
            category: None,
        }
    }

    #[tokio::test]
    async fn bundled_origin_upgrades_and_cannot_be_reclaimed_by_manual_import() {
        let store = Store::open_in_memory().await.unwrap();
        let skill = skill();
        let manual = record_installation_receipt(&store, &skill, SkillOrigin::ManagedImport, true)
            .await
            .unwrap();
        assert!(manual.owns_files);
        let bundled = record_installation_receipt(&store, &skill, SkillOrigin::Bundled, false)
            .await
            .unwrap();
        assert_eq!(bundled.origin, SkillOrigin::Bundled);
        assert!(!bundled.owns_files);
        let repeated =
            record_installation_receipt(&store, &skill, SkillOrigin::ManagedImport, true)
                .await
                .unwrap();
        assert_eq!(repeated.origin, SkillOrigin::Bundled);
        assert!(!repeated.owns_files);
    }

    #[tokio::test]
    async fn receipt_must_match_catalog_identity_and_path() {
        let store = Store::open_in_memory().await.unwrap();
        let skill = skill();
        record_installation_receipt(&store, &skill, SkillOrigin::LegacyUnknown, false)
            .await
            .unwrap();
        let mut changed = skill.clone();
        changed.sha256 = "changed".into();
        assert!(installation_receipt(&store, &changed).await.is_err());
    }

    #[tokio::test]
    async fn plugin_owned_receipt_cannot_be_reclaimed_by_manual_import() {
        let store = Store::open_in_memory().await.unwrap();
        let skill = skill();
        let plugin_id = Uuid::new_v4();
        store
            .put_json(
                SKILL_INSTALLATION_KIND,
                &skill.id.to_string(),
                &SkillInstallationReceipt {
                    skill_id: skill.id,
                    package_sha256: skill.sha256.clone(),
                    installed_root: skill.source_path.clone(),
                    origin: SkillOrigin::PluginOwned,
                    owns_files: false,
                    plugin_installation_id: Some(plugin_id),
                    phase: "active".into(),
                },
            )
            .await
            .unwrap();

        let receipt = record_installation_receipt(&store, &skill, SkillOrigin::ManagedImport, true)
            .await
            .unwrap();
        assert_eq!(receipt.origin, SkillOrigin::PluginOwned);
        assert_eq!(receipt.plugin_installation_id, Some(plugin_id));
        assert!(!receipt.owns_files);
    }

    #[tokio::test]
    async fn plugin_receipt_requires_an_independent_catalog_record_and_is_idempotent() {
        let store = Store::open_in_memory().await.unwrap();
        let skill = skill();
        let installation_id = Uuid::new_v4();
        let receipt = record_plugin_owned_receipt(&store, &skill, installation_id)
            .await
            .unwrap();
        assert_eq!(receipt.origin, SkillOrigin::PluginOwned);
        assert_eq!(receipt.plugin_installation_id, Some(installation_id));
        assert!(!receipt.owns_files);
        assert_eq!(
            record_plugin_owned_receipt(&store, &skill, installation_id)
                .await
                .unwrap(),
            receipt
        );
        assert!(
            record_plugin_owned_receipt(&store, &skill, Uuid::new_v4())
                .await
                .is_err()
        );

        let independent = SkillPackage {
            id: Uuid::new_v4(),
            ..skill.clone()
        };
        record_installation_receipt(&store, &independent, SkillOrigin::ManagedImport, true)
            .await
            .unwrap();
        assert!(
            record_plugin_owned_receipt(&store, &independent, installation_id)
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn plugin_owned_detail_and_preview_use_only_the_bound_plugin_install_root() {
        use omicsops_dto::{InstalledPlugin, PluginOwnedSkill, PluginPhase};

        let temporary = tempfile::tempdir().unwrap();
        let skills_root = temporary.path().join("skills");
        fs::create_dir(&skills_root).unwrap();
        let installation_id = Uuid::new_v4();
        let plugin_root = temporary
            .path()
            .join("plugins")
            .join("installs")
            .join(installation_id.to_string());
        let skill_root = plugin_root.join("skills").join("plugin-skill");
        fs::create_dir_all(&skill_root).unwrap();
        fs::write(
            skill_root.join("SKILL.md"),
            "---\nname: plugin-skill\n---\n# Plugin Skill\n",
        )
        .unwrap();
        let inspection = omicsops_adapters::skills::inspect_skill_directory(&skill_root).unwrap();
        let skill = SkillPackage {
            id: Uuid::new_v4(),
            name: "plugin-skill".into(),
            version: inspection.version,
            source_path: skill_root
                .canonicalize()
                .unwrap()
                .to_string_lossy()
                .into_owned(),
            sha256: inspection.sha256,
            enabled: false,
            capabilities: vec![],
            category: Some("plugin:example".into()),
        };
        let store = Store::open_in_memory().await.unwrap();
        store.save_skill_package(&skill).await.unwrap();
        record_plugin_owned_receipt(&store, &skill, installation_id)
            .await
            .unwrap();
        store
            .put_json(
                "integration_package_v1",
                &installation_id.to_string(),
                &InstalledPlugin {
                    installation_id,
                    package_id: "example.plugin".into(),
                    version: "1".into(),
                    name: "Example".into(),
                    digest: "d".repeat(64),
                    source_path: "redacted".into(),
                    trust: "local_declarative".into(),
                    enabled: true,
                    phase: PluginPhase::Installed,
                    predecessor_installation_id: None,
                    cleanup_pending: false,
                    files: vec![],
                    skills: vec![PluginOwnedSkill {
                        skill_id: skill.id,
                        name: skill.name.clone(),
                        relative_path: "skills/plugin-skill".into(),
                        package_sha256: skill.sha256.clone(),
                    }],
                    mcp_bindings: vec![],
                    last_error: None,
                    created_at: "2026-09-21T00:00:00Z".into(),
                },
            )
            .await
            .unwrap();

        let detail = settings_skill_detail_for_repository(&store, &skills_root, skill.id)
            .await
            .unwrap();
        assert_eq!(detail.origin, SkillOrigin::PluginOwned);
        assert_eq!(detail.integrity, "verified");
        assert!(!detail.can_remove_from_library && !detail.can_delete_files);
        let preview = settings_read_skill_file_for_repository(
            &store,
            &skills_root,
            skill.id,
            "SKILL.md",
            &skill.sha256,
        )
        .await
        .unwrap();
        assert!(preview.content.contains("Plugin Skill"));
        let (independent, _) =
            owned_import(&store, &temporary, &skills_root, "independent-target").await;
        let independent_detail =
            settings_skill_detail_for_repository(&store, &skills_root, independent.id)
                .await
                .unwrap();
        assert!(
            !independent_detail
                .blocking_reasons
                .iter()
                .any(|reason| reason.contains("dependency inspection"))
        );
        assert!(
            remove_skill_for_repository(
                &store,
                &skills_root,
                0,
                RemoveSkillRequest {
                    skill_id: skill.id,
                    expected_package_sha256: skill.sha256.clone(),
                    mode: SkillRemovalMode::LibraryOnly,
                },
            )
            .await
            .unwrap_err()
            .contains("Plugins")
        );
    }

    #[tokio::test]
    async fn pending_removal_cannot_be_reset_by_import_or_enable() {
        let store = Store::open_in_memory().await.unwrap();
        let skill = skill();
        store.save_skill_package(&skill).await.unwrap();
        record_installation_receipt(&store, &skill, SkillOrigin::ManagedImport, true)
            .await
            .unwrap();
        let operation = SkillRemovalJournal {
            operation_id: Uuid::new_v4(),
            skill_id: skill.id,
            name: skill.name.clone(),
            package_sha256: skill.sha256.clone(),
            original_root: skill.source_path.clone(),
            quarantine_root: "quarantine".into(),
            inventory_sha256: skill.sha256.clone(),
            files: vec![],
            phase: "needs_attention".into(),
            preserved_files: true,
            last_error: None,
        };
        save_removal_journal(&store, &operation).await.unwrap();

        assert!(
            record_installation_receipt(&store, &skill, SkillOrigin::ManagedImport, true)
                .await
                .unwrap_err()
                .contains("unfinished removal")
        );
        assert!(
            crate::skill_commands::set_skill_enabled_in_repository(&store, skill.id, true)
                .await
                .unwrap_err()
                .contains("unfinished removal")
        );
        let receipt = installation_receipt(&store, &skill).await.unwrap().unwrap();
        assert_eq!(receipt.phase, "active");
    }

    #[tokio::test]
    async fn library_removal_preserves_files_and_is_idempotent() {
        let temporary = tempfile::tempdir().unwrap();
        let skills_root = temporary.path().join("skills");
        fs::create_dir(&skills_root).unwrap();
        let store = Store::open_in_memory().await.unwrap();
        let (skill, source) = owned_import(&store, &temporary, &skills_root, "library-only").await;
        let installed = PathBuf::from(&skill.source_path);
        let request = RemoveSkillRequest {
            skill_id: skill.id,
            expected_package_sha256: skill.sha256.clone(),
            mode: SkillRemovalMode::LibraryOnly,
        };

        let removed = remove_skill_for_repository(&store, &skills_root, 0, request.clone())
            .await
            .unwrap();
        assert!(removed.removed_from_library && removed.preserved_files);
        assert!(!removed.files_removed);
        assert!(installed.join("SKILL.md").is_file());
        assert!(source.join("scripts/run.py").is_file());
        assert!(store.list_skill_packages().await.unwrap().is_empty());

        let repeated = remove_skill_for_repository(&store, &skills_root, 0, request)
            .await
            .unwrap();
        assert_eq!(repeated.status, "removed_library");
        assert!(installed.is_dir());
    }

    #[tokio::test]
    async fn owned_removal_deletes_only_verified_install_and_keeps_import_source() {
        let temporary = tempfile::tempdir().unwrap();
        let skills_root = temporary.path().join("skills");
        fs::create_dir(&skills_root).unwrap();
        let store = Store::open_in_memory().await.unwrap();
        let (skill, source) = owned_import(&store, &temporary, &skills_root, "delete-owned").await;
        let installed = PathBuf::from(&skill.source_path);
        let request = RemoveSkillRequest {
            skill_id: skill.id,
            expected_package_sha256: skill.sha256.clone(),
            mode: SkillRemovalMode::OwnedFiles,
        };

        let removed = remove_skill_for_repository(&store, &skills_root, 0, request.clone())
            .await
            .unwrap();
        assert!(removed.removed_from_library && removed.files_removed);
        assert!(!removed.preserved_files);
        assert!(!installed.exists());
        assert!(source.join("SKILL.md").is_file());
        assert!(source.join("scripts/run.py").is_file());
        assert!(list_skill_removals(&store).await.unwrap().is_empty());

        let repeated = remove_skill_for_repository(&store, &skills_root, 0, request)
            .await
            .unwrap();
        assert_eq!(repeated.status, "removed");
    }

    #[tokio::test]
    async fn owned_removal_rejects_shared_or_nested_catalog_roots() {
        let temporary = tempfile::tempdir().unwrap();
        let skills_root = temporary.path().join("skills");
        fs::create_dir(&skills_root).unwrap();
        let store = Store::open_in_memory().await.unwrap();
        let (skill, _) = owned_import(&store, &temporary, &skills_root, "shared-root").await;
        store
            .save_skill_package(&SkillPackage {
                id: Uuid::new_v4(),
                name: "alias".into(),
                source_path: skill.source_path.clone(),
                sha256: skill.sha256.clone(),
                ..skill.clone()
            })
            .await
            .unwrap();

        let error = remove_skill_for_repository(
            &store,
            &skills_root,
            0,
            RemoveSkillRequest {
                skill_id: skill.id,
                expected_package_sha256: skill.sha256.clone(),
                mode: SkillRemovalMode::OwnedFiles,
            },
        )
        .await
        .unwrap_err();
        assert!(error.contains("shares or nests"));
        assert!(Path::new(&skill.source_path).join("SKILL.md").is_file());
    }

    #[tokio::test]
    async fn removal_rejects_protected_unknown_changed_dependent_and_active_cases() {
        let temporary = tempfile::tempdir().unwrap();
        let skills_root = temporary.path().join("skills");
        fs::create_dir(&skills_root).unwrap();
        let store = Store::open_in_memory().await.unwrap();
        let (skill, _) = owned_import(&store, &temporary, &skills_root, "protected-target").await;
        let request = |mode| RemoveSkillRequest {
            skill_id: skill.id,
            expected_package_sha256: skill.sha256.clone(),
            mode,
        };

        assert!(
            remove_skill_for_repository(
                &store,
                &skills_root,
                1,
                request(SkillRemovalMode::LibraryOnly)
            )
            .await
            .unwrap_err()
            .contains("run")
        );
        let dependent_root = skills_root.join("dependent").join("record");
        fs::create_dir_all(&dependent_root).unwrap();
        fs::write(
            dependent_root.join("SKILL.md"),
            "---\ndepends_on:\n  - protected-target\n---\n# workflow\n",
        )
        .unwrap();
        store
            .save_skill_package(&SkillPackage {
                id: Uuid::new_v4(),
                name: "dependent".into(),
                version: "1".into(),
                source_path: dependent_root
                    .canonicalize()
                    .unwrap()
                    .to_string_lossy()
                    .into_owned(),
                sha256: "dependent".into(),
                enabled: true,
                capabilities: vec![],
                category: None,
            })
            .await
            .unwrap();
        assert!(
            remove_skill_for_repository(
                &store,
                &skills_root,
                0,
                request(SkillRemovalMode::LibraryOnly)
            )
            .await
            .unwrap_err()
            .contains("depend")
        );
        store
            .delete_skill_package(
                store
                    .list_skill_packages()
                    .await
                    .unwrap()
                    .into_iter()
                    .find(|item| item.name == "dependent")
                    .unwrap()
                    .id,
            )
            .await
            .unwrap();

        let mut receipt = installation_receipt(&store, &skill).await.unwrap().unwrap();
        receipt.origin = SkillOrigin::Bundled;
        receipt.owns_files = false;
        store
            .put_json(SKILL_INSTALLATION_KIND, &skill.id.to_string(), &receipt)
            .await
            .unwrap();
        assert!(
            remove_skill_for_repository(
                &store,
                &skills_root,
                0,
                request(SkillRemovalMode::OwnedFiles)
            )
            .await
            .unwrap_err()
            .contains("bundled")
        );
        receipt.origin = SkillOrigin::LegacyUnknown;
        store
            .put_json(SKILL_INSTALLATION_KIND, &skill.id.to_string(), &receipt)
            .await
            .unwrap();
        assert!(
            remove_skill_for_repository(
                &store,
                &skills_root,
                0,
                request(SkillRemovalMode::OwnedFiles)
            )
            .await
            .unwrap_err()
            .contains("managed imports")
        );
        receipt.origin = SkillOrigin::ManagedImport;
        receipt.owns_files = true;
        store
            .put_json(SKILL_INSTALLATION_KIND, &skill.id.to_string(), &receipt)
            .await
            .unwrap();
        fs::write(Path::new(&skill.source_path).join("new.txt"), "changed").unwrap();
        assert!(
            remove_skill_for_repository(
                &store,
                &skills_root,
                0,
                request(SkillRemovalMode::OwnedFiles)
            )
            .await
            .unwrap_err()
            .contains("changed")
        );
        assert!(Path::new(&skill.source_path).join("new.txt").is_file());
        assert!(
            store
                .list_skill_packages()
                .await
                .unwrap()
                .iter()
                .any(|item| item.id == skill.id)
        );
    }

    #[tokio::test]
    async fn retry_preserves_a_quarantine_whose_inventory_changed() {
        let temporary = tempfile::tempdir().unwrap();
        let skills_root = temporary.path().join("skills");
        fs::create_dir(&skills_root).unwrap();
        let store = Store::open_in_memory().await.unwrap();
        let (skill, _) = owned_import(&store, &temporary, &skills_root, "retry-safe").await;
        let original = exact_owned_root(&skills_root, &skill).unwrap();
        let inventory = inspect_inventory(&original).unwrap();
        let operation_id = Uuid::new_v4();
        let quarantine = quarantine_root(&skills_root, operation_id).unwrap();
        prepare_quarantine_parent(&skills_root).unwrap();
        fs::rename(&original, &quarantine).unwrap();
        let operation = SkillRemovalJournal {
            operation_id,
            skill_id: skill.id,
            name: skill.name.clone(),
            package_sha256: skill.sha256.clone(),
            original_root: original.to_string_lossy().into_owned(),
            quarantine_root: quarantine.to_string_lossy().into_owned(),
            inventory_sha256: hash_inventory(&quarantine, &inventory).unwrap(),
            files: removal_file_manifest(&quarantine, &inventory).unwrap(),
            phase: "cleaning".into(),
            preserved_files: true,
            last_error: None,
        };
        let mut receipt = installation_receipt(&store, &skill).await.unwrap().unwrap();
        receipt.phase = "removed".into();
        store
            .commit_skill_library_removal(
                skill.id,
                &skill.sha256,
                SKILL_INSTALLATION_KIND,
                &receipt,
                Some((SKILL_REMOVAL_KIND, &operation_id.to_string(), &operation)),
            )
            .await
            .unwrap();
        fs::write(quarantine.join("unexpected.txt"), "do not delete").unwrap();

        let result = retry_skill_removal(&store, &skills_root, operation_id)
            .await
            .unwrap();
        assert_eq!(result.status, "needs_attention");
        assert!(result.preserved_files);
        assert!(quarantine.join("unexpected.txt").is_file());
        assert_eq!(list_skill_removals(&store).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn interrupted_precommit_removal_restores_catalog_files_and_unlocks_mutations() {
        let temporary = tempfile::tempdir().unwrap();
        let skills_root = temporary.path().join("skills");
        fs::create_dir(&skills_root).unwrap();
        let store = Store::open_in_memory().await.unwrap();
        let (skill, _) = owned_import(&store, &temporary, &skills_root, "restore-precommit").await;
        let original = exact_owned_root(&skills_root, &skill).unwrap();
        let inventory = inspect_inventory(&original).unwrap();
        let operation_id = Uuid::new_v4();
        let quarantine = quarantine_root(&skills_root, operation_id).unwrap();
        prepare_quarantine_parent(&skills_root).unwrap();
        let operation = SkillRemovalJournal {
            operation_id,
            skill_id: skill.id,
            name: skill.name.clone(),
            package_sha256: skill.sha256.clone(),
            original_root: original.to_string_lossy().into_owned(),
            quarantine_root: quarantine.to_string_lossy().into_owned(),
            inventory_sha256: hash_inventory(&original, &inventory).unwrap(),
            files: removal_file_manifest(&original, &inventory).unwrap(),
            phase: "prepared".into(),
            preserved_files: true,
            last_error: None,
        };
        save_removal_journal(&store, &operation).await.unwrap();
        fs::rename(&original, &quarantine).unwrap();

        reconcile_skill_removals(&store, &skills_root)
            .await
            .unwrap();
        let restored = retry_skill_removal(&store, &skills_root, operation_id)
            .await
            .unwrap();
        assert_eq!(restored.status, "restored");
        assert!(original.join("SKILL.md").is_file());
        assert!(!quarantine.exists());
        ensure_skill_mutation_allowed(&store, skill.id)
            .await
            .unwrap();
        assert!(
            store
                .list_skill_packages()
                .await
                .unwrap()
                .iter()
                .any(|item| item.id == skill.id)
        );
    }

    #[tokio::test]
    async fn precommit_restore_never_follows_a_replaced_parent_link() {
        let temporary = tempfile::tempdir().unwrap();
        let skills_root = temporary.path().join("skills");
        fs::create_dir(&skills_root).unwrap();
        let store = Store::open_in_memory().await.unwrap();
        let (skill, _) = owned_import(&store, &temporary, &skills_root, "restore-link").await;
        let original = exact_owned_root(&skills_root, &skill).unwrap();
        let inventory = inspect_inventory(&original).unwrap();
        let operation_id = Uuid::new_v4();
        let quarantine = quarantine_root(&skills_root, operation_id).unwrap();
        prepare_quarantine_parent(&skills_root).unwrap();
        let operation = SkillRemovalJournal {
            operation_id,
            skill_id: skill.id,
            name: skill.name.clone(),
            package_sha256: skill.sha256.clone(),
            original_root: original.to_string_lossy().into_owned(),
            quarantine_root: quarantine.to_string_lossy().into_owned(),
            inventory_sha256: hash_inventory(&original, &inventory).unwrap(),
            files: removal_file_manifest(&original, &inventory).unwrap(),
            phase: "quarantined".into(),
            preserved_files: true,
            last_error: None,
        };
        save_removal_journal(&store, &operation).await.unwrap();
        fs::rename(&original, &quarantine).unwrap();
        let original_parent = original.parent().unwrap();
        fs::remove_dir(original_parent).unwrap();
        let outside = temporary.path().join("outside");
        fs::create_dir(&outside).unwrap();
        if create_directory_link(&outside, original_parent).is_ok() {
            assert!(
                retry_skill_removal(&store, &skills_root, operation_id)
                    .await
                    .is_err()
            );
            assert!(quarantine.join("SKILL.md").is_file());
            assert!(!outside.join(&skill.sha256).exists());
        }
    }

    #[tokio::test]
    async fn cleanup_retry_accepts_only_verified_remaining_original_files() {
        let temporary = tempfile::tempdir().unwrap();
        let skills_root = temporary.path().join("skills");
        fs::create_dir(&skills_root).unwrap();
        let store = Store::open_in_memory().await.unwrap();
        let (skill, _) = owned_import(&store, &temporary, &skills_root, "partial-retry").await;
        let original = exact_owned_root(&skills_root, &skill).unwrap();
        let inventory = inspect_inventory(&original).unwrap();
        let operation_id = Uuid::new_v4();
        let quarantine = quarantine_root(&skills_root, operation_id).unwrap();
        prepare_quarantine_parent(&skills_root).unwrap();
        let files = removal_file_manifest(&original, &inventory).unwrap();
        let operation = SkillRemovalJournal {
            operation_id,
            skill_id: skill.id,
            name: skill.name.clone(),
            package_sha256: skill.sha256.clone(),
            original_root: original.to_string_lossy().into_owned(),
            quarantine_root: quarantine.to_string_lossy().into_owned(),
            inventory_sha256: hash_inventory(&original, &inventory).unwrap(),
            files: files.clone(),
            phase: "cleaning".into(),
            preserved_files: true,
            last_error: None,
        };
        fs::rename(&original, &quarantine).unwrap();
        let mut receipt = installation_receipt(&store, &skill).await.unwrap().unwrap();
        receipt.phase = "removed".into();
        store
            .commit_skill_library_removal(
                skill.id,
                &skill.sha256,
                SKILL_INSTALLATION_KIND,
                &receipt,
                Some((SKILL_REMOVAL_KIND, &operation_id.to_string(), &operation)),
            )
            .await
            .unwrap();
        let already_removed = files
            .iter()
            .find(|file| file.relative_path.ends_with("run.py"))
            .unwrap();
        fs::remove_file(quarantine.join(&already_removed.relative_path)).unwrap();

        let result = retry_skill_removal(&store, &skills_root, operation_id)
            .await
            .unwrap();
        assert_eq!(result.status, "removed");
        assert!(!quarantine.exists());

        let mut operation = store
            .get_json::<SkillRemovalJournal>(SKILL_REMOVAL_KIND, &operation_id.to_string())
            .await
            .unwrap()
            .unwrap();
        operation.phase = "cleaning".into();
        operation.preserved_files = true;
        save_removal_journal(&store, &operation).await.unwrap();
        let absent = retry_skill_removal(&store, &skills_root, operation_id)
            .await
            .unwrap();
        assert_eq!(absent.status, "removed");
    }

    #[tokio::test]
    async fn transitive_disabled_dependency_still_blocks_target_removal() {
        let temporary = tempfile::tempdir().unwrap();
        let skills_root = temporary.path().join("skills");
        fs::create_dir(&skills_root).unwrap();
        let store = Store::open_in_memory().await.unwrap();
        let (target, _) = owned_import(&store, &temporary, &skills_root, "transitive-target").await;
        for (name, enabled, dependency) in [
            ("middle", false, "transitive-target"),
            ("workflow", true, "middle"),
        ] {
            let root = skills_root.join(name).join("record");
            fs::create_dir_all(&root).unwrap();
            fs::write(
                root.join("SKILL.md"),
                format!("---\nname: {name}\ndepends_on:\n  - {dependency}\n---\n# {name}\n"),
            )
            .unwrap();
            store
                .save_skill_package(&SkillPackage {
                    id: Uuid::new_v4(),
                    name: name.into(),
                    version: "1".into(),
                    source_path: root.canonicalize().unwrap().to_string_lossy().into_owned(),
                    sha256: name.into(),
                    enabled,
                    capabilities: vec![],
                    category: None,
                })
                .await
                .unwrap();
        }

        let detail = settings_skill_detail_for_repository(&store, &skills_root, target.id)
            .await
            .unwrap();
        assert_eq!(detail.dependent_skills.len(), 1);
        assert!(
            remove_skill_for_repository(
                &store,
                &skills_root,
                0,
                RemoveSkillRequest {
                    skill_id: target.id,
                    expected_package_sha256: target.sha256.clone(),
                    mode: SkillRemovalMode::LibraryOnly,
                },
            )
            .await
            .unwrap_err()
            .contains("depend")
        );
    }

    #[tokio::test]
    async fn shared_skill_gate_keeps_freeze_and_catalog_removal_mutually_exclusive() {
        let temporary = tempfile::tempdir().unwrap();
        let skills_root = temporary.path().join("skills");
        fs::create_dir(&skills_root).unwrap();
        let store = Store::open_in_memory().await.unwrap();
        let skill = managed_skill(&store, &skills_root, &[]).await;
        let gate = std::sync::Arc::new(tokio::sync::RwLock::new(()));

        let read = gate.read().await;
        let frozen = crate::skill_commands::freeze_skill_package(&skill, &[]).unwrap();
        assert_eq!(frozen.package_sha256, skill.sha256);
        assert!(!frozen.sections.is_empty());

        let writer_store = store.clone();
        let writer_gate = gate.clone();
        let (finished_tx, mut finished_rx) = tokio::sync::oneshot::channel();
        tokio::spawn(async move {
            let _write = writer_gate.write().await;
            writer_store.delete_skill_package(skill.id).await.unwrap();
            let _ = finished_tx.send(());
        });
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(25), &mut finished_rx)
                .await
                .is_err()
        );
        drop(read);
        tokio::time::timeout(std::time::Duration::from_secs(1), finished_rx)
            .await
            .unwrap()
            .unwrap();
        assert!(store.list_skill_packages().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn detail_lists_only_safe_managed_files_and_reports_verified_ownership() {
        let temporary = tempfile::tempdir().unwrap();
        let skills_root = temporary.path().join("skills");
        fs::create_dir(&skills_root).unwrap();
        let store = Store::open_in_memory().await.unwrap();
        let skill = managed_skill(
            &store,
            &skills_root,
            &[
                ("scripts/run.py", b"print('safe')\n"),
                ("asset.bin", &[0, 159, 146, 150]),
                ("token.txt", b"must-not-be-listed"),
            ],
        )
        .await;

        let detail = settings_skill_detail_for_repository(&store, &skills_root, skill.id)
            .await
            .unwrap();
        assert_eq!(detail.origin, SkillOrigin::ManagedImport);
        assert_eq!(detail.integrity, "verified");
        assert!(detail.inventory_complete);
        assert!(detail.can_delete_files);
        assert!(detail.can_remove_from_library);
        assert!(
            detail
                .files
                .iter()
                .any(|file| { file.relative_path == "scripts/run.py" && file.previewable })
        );
        assert!(
            detail
                .files
                .iter()
                .any(|file| { file.relative_path == "asset.bin" && !file.previewable })
        );
        assert!(
            !serde_json::to_string(&detail)
                .unwrap()
                .contains("token.txt")
        );
        assert!(
            !serde_json::to_string(&detail)
                .unwrap()
                .contains("must-not-be-listed")
        );
        let mut blocked = detail;
        apply_run_blocker(&mut blocked, 2);
        assert!(!blocked.can_remove_from_library && !blocked.can_delete_files);
        assert!(blocked.blocking_reasons.iter().any(|reason| reason.contains('2')));
    }

    #[tokio::test]
    async fn preview_redacts_assignments_authorization_and_private_keys() {
        let temporary = tempfile::tempdir().unwrap();
        let skills_root = temporary.path().join("skills");
        fs::create_dir(&skills_root).unwrap();
        let store = Store::open_in_memory().await.unwrap();
        let content = b"password: swordfish\ntoken=abc123\nauthorization: Bearer bearer-secret\n-----BEGIN PRIVATE KEY-----\nprivate-material\n-----END PRIVATE KEY-----\n";
        let skill = managed_skill(&store, &skills_root, &[("config.json", content)]).await;

        let preview = settings_read_skill_file_for_repository(
            &store,
            &skills_root,
            skill.id,
            "config.json",
            &skill.sha256,
        )
        .await
        .unwrap();
        assert!(preview.redacted);
        let encoded = serde_json::to_string(&preview).unwrap();
        for secret in ["swordfish", "abc123", "bearer-secret", "private-material"] {
            assert!(!encoded.contains(secret));
        }
        assert!(preview.content.contains("[REDACTED]"));
    }

    #[tokio::test]
    async fn preview_rejects_unsafe_paths_external_roots_and_changed_packages() {
        let temporary = tempfile::tempdir().unwrap();
        let skills_root = temporary.path().join("skills");
        fs::create_dir(&skills_root).unwrap();
        let store = Store::open_in_memory().await.unwrap();
        let skill = managed_skill(&store, &skills_root, &[("script.py", b"print('safe')")]).await;

        for path in [
            "../secret.txt",
            "C:/secret.txt",
            "script.py:stream",
            "CON.txt",
        ] {
            let error = settings_read_skill_file_for_repository(
                &store,
                &skills_root,
                skill.id,
                path,
                &skill.sha256,
            )
            .await
            .unwrap_err();
            assert!(!error.contains(path));
        }
        fs::write(
            Path::new(&skill.source_path).join("script.py"),
            "password=changed-secret",
        )
        .unwrap();
        let error = settings_read_skill_file_for_repository(
            &store,
            &skills_root,
            skill.id,
            "script.py",
            &skill.sha256,
        )
        .await
        .unwrap_err();
        assert!(error.contains("changed"));
        assert!(!error.contains("changed-secret"));

        let outside_root = temporary.path().join("outside");
        fs::create_dir(&outside_root).unwrap();
        fs::write(outside_root.join("SKILL.md"), "# external").unwrap();
        let external = SkillPackage {
            id: Uuid::new_v4(),
            name: "external".into(),
            version: "1".into(),
            source_path: outside_root.to_string_lossy().into_owned(),
            sha256: "external".into(),
            enabled: false,
            capabilities: vec![],
            category: None,
        };
        store.save_skill_package(&external).await.unwrap();
        let detail = settings_skill_detail_for_repository(&store, &skills_root, external.id)
            .await
            .unwrap();
        assert_eq!(detail.origin, SkillOrigin::External);
        assert!(detail.files.is_empty());
        assert_eq!(detail.integrity, "unavailable");
        assert!(
            settings_read_skill_file_for_repository(
                &store,
                &skills_root,
                external.id,
                "SKILL.md",
                &external.sha256,
            )
            .await
            .is_err()
        );
    }

    #[tokio::test]
    async fn detail_stops_at_the_shared_entry_and_depth_budget_without_hashing_again() {
        let temporary = tempfile::tempdir().unwrap();
        let skills_root = temporary.path().join("skills");
        fs::create_dir(&skills_root).unwrap();
        let store = Store::open_in_memory().await.unwrap();
        let skill = managed_skill(&store, &skills_root, &[]).await;
        let package_root = Path::new(&skill.source_path);
        let mut deep = package_root.to_path_buf();
        for index in 0..=MAX_INVENTORY_DEPTH {
            deep.push(format!("d{index}"));
            fs::create_dir(&deep).unwrap();
        }
        let detail = settings_skill_detail_for_repository(&store, &skills_root, skill.id)
            .await
            .unwrap();
        assert!(!detail.inventory_complete);
        assert_eq!(detail.integrity, "incomplete");

        fs::remove_dir_all(package_root.join("d0")).unwrap();
        for index in 0..=MAX_INVENTORY_FILES {
            fs::create_dir(package_root.join(format!("empty-{index}"))).unwrap();
        }
        let detail = settings_skill_detail_for_repository(&store, &skills_root, skill.id)
            .await
            .unwrap();
        assert!(!detail.inventory_complete);
        assert_eq!(detail.integrity, "incomplete");
    }

    #[tokio::test]
    async fn dependency_manifest_is_bounded_and_links_are_never_followed() {
        let temporary = tempfile::tempdir().unwrap();
        let skills_root = temporary.path().join("skills");
        fs::create_dir(&skills_root).unwrap();
        let store = Store::open_in_memory().await.unwrap();
        let target = managed_skill(&store, &skills_root, &[]).await;
        let candidate_root = skills_root.join("candidate").join("package");
        fs::create_dir_all(&candidate_root).unwrap();
        fs::write(
            candidate_root.join("SKILL.md"),
            "x".repeat(MAX_DEPENDENCY_MANIFEST_BYTES as usize + 1),
        )
        .unwrap();
        let candidate = SkillPackage {
            id: Uuid::new_v4(),
            name: "candidate".into(),
            version: "1".into(),
            source_path: candidate_root
                .canonicalize()
                .unwrap()
                .to_string_lossy()
                .into_owned(),
            sha256: "candidate".into(),
            enabled: true,
            capabilities: vec![],
            category: None,
        };
        store.save_skill_package(&candidate).await.unwrap();
        let detail = settings_skill_detail_for_repository(&store, &skills_root, target.id)
            .await
            .unwrap();
        assert!(
            detail
                .blocking_reasons
                .iter()
                .any(|reason| reason.contains("dependency inspection"))
        );
        assert!(detail.dependent_skills.is_empty());

        fs::remove_file(candidate_root.join("SKILL.md")).unwrap();
        let outside = temporary.path().join("outside-secret.md");
        fs::write(&outside, "depends_on: sample\nsecret=must-not-be-read").unwrap();
        if create_file_link(&outside, &candidate_root.join("SKILL.md")).is_ok() {
            let linked = settings_skill_detail_for_repository(&store, &skills_root, target.id)
                .await
                .unwrap();
            assert!(linked.dependent_skills.is_empty());
            assert!(
                linked
                    .blocking_reasons
                    .iter()
                    .any(|reason| reason.contains("dependency inspection"))
            );
            assert!(
                !serde_json::to_string(&linked)
                    .unwrap()
                    .contains("must-not-be-read")
            );
        }
    }

    #[test]
    fn opened_file_identity_rejects_replacement_and_hardlinks() {
        let temporary = tempfile::tempdir().unwrap();
        let path = temporary.path().join("script.py");
        fs::write(&path, "first").unwrap();
        let expected = inspect_file_identity(&path).unwrap();
        let backup = temporary.path().join("original.py");
        let error = open_verified_regular_file(&path, &expected, || {
            fs::rename(&path, &backup).unwrap();
            fs::write(&path, "replacement").unwrap();
        })
        .unwrap_err();
        assert!(error.contains("identity changed"));

        let hardlink = temporary.path().join("shared.py");
        fs::hard_link(&path, &hardlink).unwrap();
        assert!(inspect_file_identity(&path).is_err());
    }

    #[tokio::test]
    async fn preview_rejects_large_binary_control_and_unknown_skill_inputs() {
        let temporary = tempfile::tempdir().unwrap();
        let skills_root = temporary.path().join("skills");
        fs::create_dir(&skills_root).unwrap();
        let store = Store::open_in_memory().await.unwrap();
        let skill = managed_skill(
            &store,
            &skills_root,
            &[
                ("large.txt", &vec![b'x'; MAX_PREVIEW_BYTES as usize + 1]),
                ("control.txt", b"safe\0secret"),
            ],
        )
        .await;
        assert!(
            settings_read_skill_file_for_repository(
                &store,
                &skills_root,
                skill.id,
                "large.txt",
                &skill.sha256,
            )
            .await
            .is_err()
        );
        assert!(
            settings_read_skill_file_for_repository(
                &store,
                &skills_root,
                skill.id,
                "control.txt",
                &skill.sha256,
            )
            .await
            .is_err()
        );
        assert!(
            settings_skill_detail_for_repository(&store, &skills_root, Uuid::new_v4())
                .await
                .is_err()
        );
    }

    #[cfg(windows)]
    fn create_file_link(target: &Path, link: &Path) -> std::io::Result<()> {
        std::os::windows::fs::symlink_file(target, link)
    }

    #[cfg(unix)]
    fn create_file_link(target: &Path, link: &Path) -> std::io::Result<()> {
        std::os::unix::fs::symlink(target, link)
    }

    #[cfg(windows)]
    fn create_directory_link(target: &Path, link: &Path) -> std::io::Result<()> {
        std::os::windows::fs::symlink_dir(target, link)
    }

    #[cfg(unix)]
    fn create_directory_link(target: &Path, link: &Path) -> std::io::Result<()> {
        std::os::unix::fs::symlink(target, link)
    }
}
