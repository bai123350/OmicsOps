use std::path::Path;
use std::sync::OnceLock;

use omicsops_core::workspace::SkillPackage;
use omicsops_dto::{SkillInstallationReceipt, SkillOrigin};
use omicsops_store::Store;

pub(crate) const SKILL_INSTALLATION_KIND: &str = "skill_installation_v1";
static RECEIPT_MUTATION: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();

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
    use uuid::Uuid;

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
}
