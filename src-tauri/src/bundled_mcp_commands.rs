//! Presets register declarations only. The existing MCP host owns all approvals.

use std::{collections::BTreeMap, path::Path};

use chrono::Utc;
use omicsops_store::Store;
use sha2::{Digest, Sha256};
use tauri::State;
use uuid::Uuid;

use crate::{
    commands::AppState,
    dto::{AddBundledMcpServerRequest, BundledMcpPreset},
    p1_commands::{McpServerProfile, SaveMcpServerRequest, mcp_profile_from_request},
};

#[tauri::command]
pub fn list_bundled_mcp_presets() -> Vec<BundledMcpPreset> {
    let mut counts = BTreeMap::<&str, usize>::new();
    for (domain, _) in omicsops_bio::catalog() {
        *counts.entry(domain).or_default() += 1;
    }
    counts
        .into_iter()
        .map(|(domain, tool_count)| {
            let metadata = omicsops_bio::domain_metadata(domain)
                .expect("compiled scientific domains must have metadata");
            BundledMcpPreset {
                id: domain.into(),
                name: format!("Science · {domain}"),
                description: metadata.description.clone(),
                description_zh: metadata.description_zh.clone(),
                tool_count,
            }
        })
        .collect()
}

fn preset_server_id(preset_id: &str) -> Uuid {
    let hash = Sha256::digest(format!("omicsops:bundled-science-mcp:{preset_id}"));
    let mut bytes = [0; 16];
    bytes.copy_from_slice(&hash[..16]);
    // A stable, application-defined UUIDv8; no user-controlled executable input.
    bytes[6] = (bytes[6] & 0x0f) | 0x80;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Uuid::from_bytes(bytes)
}

/// Register missing builtins before the Agent/UI reads the persisted MCP index.
/// Catalog discovery is local and does not grant process or tool execution.
pub async fn install_bundled_mcp_servers(
    repository: &Store,
    executable: &Path,
) -> Result<(), String> {
    for preset in list_bundled_mcp_presets() {
        register_preset(repository, &preset.id, executable).await?;
    }
    Ok(())
}

#[tauri::command]
pub async fn add_bundled_mcp_server(
    state: State<'_, AppState>,
    request: AddBundledMcpServerRequest,
) -> Result<McpServerProfile, String> {
    let id = preset_server_id(&request.preset_id);
    let _server_lock = state.mcp_sessions.lock_server(id).await;
    let executable = std::env::current_exe()
        .map_err(|error| format!("cannot locate OmicsOps executable: {error}"))?;
    let profile = register_preset(&state.repository, &request.preset_id, &executable).await?;
    state.mcp_sessions.invalidate_server(id).await;
    Ok(profile)
}

async fn register_preset(
    repository: &Store,
    preset_id: &str,
    executable: &Path,
) -> Result<McpServerProfile, String> {
    let preset = list_bundled_mcp_presets()
        .into_iter()
        .find(|preset| preset.id == preset_id)
        .ok_or_else(|| "unknown bundled scientific MCP preset".to_owned())?;
    let id = preset_server_id(preset_id);
    let existing = repository
        .get_json::<McpServerProfile>("mcp_server", &id.to_string())
        .await
        .map_err(|error| error.to_string())?;
    // Re-adding a configured preset must not reset credentials, edits or grants.
    if let Some(mut existing) = existing {
        // Upgrade old declarations without changing user enablement or custom commands.
        if existing.tools.is_empty()
            && existing.command == executable.to_string_lossy()
            && existing.args == ["--omicsops-bio-mcp", preset_id]
        {
            populate_compiled_catalog(&mut existing, preset_id)?;
            existing.approved_tools.clear();
            repository
                .put_json("mcp_server", &id.to_string(), &existing)
                .await
                .map_err(|error| error.to_string())?;
        }
        return Ok(existing);
    }
    let mut profile = mcp_profile_from_request(
        SaveMcpServerRequest {
            id: Some(id),
            name: preset.name,
            command: executable.to_string_lossy().into_owned(),
            args: vec!["--omicsops-bio-mcp".into(), preset.id],
            cwd: None,
            timeout_secs: 60,
            env_bindings: vec![],
        },
        None,
        Utc::now(),
    )?;
    profile.enabled = true;
    populate_compiled_catalog(&mut profile, preset_id)?;
    repository
        .put_json("mcp_server", &id.to_string(), &profile)
        .await
        .map_err(|error| error.to_string())?;
    Ok(profile)
}

fn populate_compiled_catalog(
    profile: &mut McpServerProfile,
    preset_id: &str,
) -> Result<(), String> {
    profile.tools = crate::bio_mcp::bundled_tools(preset_id)
        .into_iter()
        .map(serde_json::to_value)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    profile.tool_catalog_sha256 = Some(omicsops_mcp::catalog_digest(&profile.tools));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn startup_installs_all_domains_and_preserves_user_choices_on_restart() {
        let store = Store::open_in_memory().await.unwrap();
        let exe = Path::new(r"C:\OmicsOps\app.exe");
        install_bundled_mcp_servers(&store, exe).await.unwrap();
        let profiles = store
            .list_json::<McpServerProfile>("mcp_server")
            .await
            .unwrap();
        assert_eq!(profiles.len(), 23);
        for profile in &profiles {
            assert!(profile.enabled);
            assert!(!profile.tools.is_empty());
            assert_eq!(
                profile.tool_catalog_sha256.as_deref(),
                Some(omicsops_mcp::catalog_digest(&profile.tools).as_str())
            );
            assert!(!profile.launch_approved);
            assert!(profile.approved_tools.is_empty());
            assert!(profile.last_inspected_at.is_none());
        }
        let mut disabled = profiles[0].clone();
        disabled.enabled = false;
        store
            .put_json("mcp_server", &disabled.id.to_string(), &disabled)
            .await
            .unwrap();
        install_bundled_mcp_servers(&store, exe).await.unwrap();
        let again = store
            .list_json::<McpServerProfile>("mcp_server")
            .await
            .unwrap();
        assert_eq!(again.len(), 23);
        let saved = again.iter().find(|p| p.id == disabled.id).unwrap();
        assert_eq!(
            serde_json::to_value(saved).unwrap(),
            serde_json::to_value(disabled).unwrap()
        );
    }

    #[test]
    fn preset_catalog_covers_every_compiled_tool_exactly_once() {
        let presets = list_bundled_mcp_presets();
        assert_eq!(presets.len(), 23);
        assert_eq!(
            presets
                .iter()
                .map(|preset| preset.tool_count)
                .sum::<usize>(),
            omicsops_bio::catalog().len()
        );
        for preset in presets {
            assert!(preset.tool_count > 0);
            assert!(!preset.description.is_empty());
            assert!(!preset.description_zh.is_empty());
        }
    }

    #[tokio::test]
    async fn adding_a_preset_registers_discoverable_tools_without_execution_grants() {
        let store = Store::open_in_memory().await.unwrap();
        let profile = register_preset(&store, "omics-archives", Path::new(r"C:\OmicsOps\app.exe"))
            .await
            .unwrap();
        assert_eq!(profile.args, ["--omicsops-bio-mcp", "omics-archives"]);
        assert!(profile.enabled);
        assert!(!profile.launch_approved);
        assert!(profile.approved_tools.is_empty());
        assert_eq!(
            profile.tools.len(),
            list_bundled_mcp_presets()
                .into_iter()
                .find(|p| p.id == "omics-archives")
                .unwrap()
                .tool_count
        );
        assert!(
            profile
                .tools
                .iter()
                .all(|tool| tool["name"].is_string() && tool["inputSchema"].is_object())
        );
        assert!(profile.env_bindings.is_empty());
        assert!(profile.last_inspected_at.is_none());
    }

    #[tokio::test]
    async fn repeated_registration_preserves_configuration_and_grants() {
        let store = Store::open_in_memory().await.unwrap();
        let exe = Path::new(r"C:\OmicsOps\app.exe");
        let mut profile = register_preset(&store, "pubmed", exe).await.unwrap();
        profile.enabled = true;
        profile.launch_approved = true;
        profile.approved_tools = vec!["search_articles".into()];
        profile.name = "My literature tools".into();
        profile.env_bindings.push(omicsops_mcp::McpEnvBinding {
            name: "NCBI_API_KEY".into(),
            value: None,
            credential_reference: Some("keyring-only-reference".into()),
        });
        store
            .put_json("mcp_server", &profile.id.to_string(), &profile)
            .await
            .unwrap();
        let again = register_preset(&store, "pubmed", exe).await.unwrap();
        assert_eq!(
            serde_json::to_value(again).unwrap(),
            serde_json::to_value(profile).unwrap()
        );
        assert_eq!(
            store
                .list_json::<McpServerProfile>("mcp_server")
                .await
                .unwrap()
                .len(),
            1
        );
    }

    #[tokio::test]
    async fn legacy_empty_catalog_is_backfilled_without_reenabling_server() {
        let store = Store::open_in_memory().await.unwrap();
        let exe = Path::new(r"C:\OmicsOps\app.exe");
        let mut old = register_preset(&store, "pubmed", exe).await.unwrap();
        old.enabled = false;
        old.tools.clear();
        old.tool_catalog_sha256 = None;
        store
            .put_json("mcp_server", &old.id.to_string(), &old)
            .await
            .unwrap();
        install_bundled_mcp_servers(&store, exe).await.unwrap();
        let updated = register_preset(&store, "pubmed", exe).await.unwrap();
        assert!(!updated.enabled);
        assert!(!updated.launch_approved);
        assert!(!updated.tools.is_empty());
        assert!(updated.tool_catalog_sha256.is_some());
    }

    #[tokio::test]
    async fn unknown_preset_does_not_persist_anything() {
        let store = Store::open_in_memory().await.unwrap();
        for id in ["", "mcp_bio", "pubmed --extra", "../pubmed", "PUBMED"] {
            assert!(
                register_preset(&store, id, Path::new("app.exe"))
                    .await
                    .is_err()
            );
        }
        assert!(
            store
                .list_json::<McpServerProfile>("mcp_server")
                .await
                .unwrap()
                .is_empty()
        );
    }
}
