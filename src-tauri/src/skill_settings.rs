use std::{
    collections::BTreeMap,
    fs::{self, File},
    io::Read,
    path::{Component, Path, PathBuf},
    sync::OnceLock,
};

use omicsops_core::workspace::SkillPackage;
use omicsops_dto::{
    SkillFilePreview, SkillInstallationReceipt, SkillOrigin, SkillSettingsDetail,
    SkillSettingsFile, SkillSettingsPackage,
};
use omicsops_store::Store;
use sha2::{Digest, Sha256};
use tauri::State;
use uuid::Uuid;

use crate::{
    commands::AppState, composer_quotes::is_credential_filename, composer_references::public_text,
    skill_commands::skill_dependencies,
};

pub(crate) const SKILL_INSTALLATION_KIND: &str = "skill_installation_v1";
static RECEIPT_MUTATION: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
const MAX_INVENTORY_DEPTH: usize = 16;
const MAX_INVENTORY_FILES: usize = 4_096;
const MAX_INVENTORY_BYTES: u64 = 100 * 1024 * 1024;
const MAX_PREVIEW_BYTES: u64 = 256 * 1024;
const MAX_DEPENDENCY_MANIFEST_BYTES: u64 = 512 * 1024;

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

#[tauri::command]
pub async fn settings_skill_detail(
    state: State<'_, AppState>,
    skill_id: Uuid,
) -> Result<SkillSettingsDetail, String> {
    let _guard = state.skills_gate.read().await;
    settings_skill_detail_for_repository(&state.repository, &state.skills_root, skill_id).await
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
    let managed_root = managed_package_root(skills_root, &skill).ok();
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
        dependent_skills(&packages, &skill, skills_root);
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
    installation_receipt(repository, &skill).await?;
    if expected_package_sha256 != skill.sha256 {
        return Err("skill package changed; reopen its details".into());
    }
    let root = managed_package_root(skills_root, &skill)
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

fn dependent_skills(
    packages: &[SkillPackage],
    target: &SkillPackage,
    skills_root: &Path,
) -> (Vec<Uuid>, bool) {
    let mut result = Vec::new();
    let mut complete = true;
    for candidate in packages
        .iter()
        .filter(|candidate| candidate.id != target.id && candidate.enabled)
    {
        let markdown = managed_package_root(skills_root, candidate).and_then(|root| {
            read_bounded_regular_text(&root, Path::new("SKILL.md"), MAX_DEPENDENCY_MANIFEST_BYTES)
        });
        let markdown = match markdown {
            Ok(markdown) => markdown,
            Err(_) => {
                complete = false;
                continue;
            }
        };
        if skill_dependencies(&markdown).iter().any(|dependency| {
            target.name == *dependency || target.name.ends_with(&format!("-{dependency}"))
        }) {
            result.push(candidate.id);
        }
    }
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
        record_installation_receipt(
            &store,
            &independent,
            SkillOrigin::ManagedImport,
            true,
        )
        .await
        .unwrap();
        assert!(
            record_plugin_owned_receipt(&store, &independent, installation_id)
                .await
                .is_err()
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
}
