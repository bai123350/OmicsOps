pub mod agent_commands;
pub mod agent_v4;
pub mod bio_mcp;
pub mod browser_commands;
pub mod bundled_mcp_commands;
pub mod commands;
pub mod conversation_mode;
#[cfg(test)]
mod conversation_mode_tests;
#[cfg(test)]
mod conversation_state_tests;
pub mod dto;
#[cfg(test)]
mod dto_contract_tests;
pub mod inspection;
pub mod kernel_commands;
mod model_catalog_shared;
pub mod model_commands;
pub mod p1_commands;
#[cfg(test)]
mod plan_revision_tests;
pub mod pubmed_mcp;
mod remote_jobs_v4;
pub mod research_commands;
mod runtime_jobs_v4;
pub mod skill_commands;
pub mod sync_commands;
pub mod workspace_commands;

use commands::AppState;
use omicsops_adapters::credentials::SystemCredentialVault;
use omicsops_store::Store;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};
use tauri::Manager;

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let data_dir = app
                .path()
                .app_data_dir()
                .map_err(|error| error.to_string())?;
            std::fs::create_dir_all(&data_dir)?;
            let skills_root = data_dir.join("skills");
            std::fs::create_dir_all(&skills_root)?;
            let packaged_skills_root = app
                .path()
                .resource_dir()
                .map_err(|error| error.to_string())?
                .join("skills")
                .join("single-cell");
            let development_skills_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("..")
                .join("skills")
                .join("single-cell");
            let bundled_skills_root = if packaged_skills_root.is_dir() {
                packaged_skills_root
            } else {
                development_skills_root
            };
            let packaged_browser_extension = app
                .path()
                .resource_dir()
                .map_err(|error| error.to_string())?
                .join("browser-extension");
            let development_browser_extension = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("..")
                .join("browser-extension");
            let browser_extension_root = if packaged_browser_extension.is_dir() {
                packaged_browser_extension
            } else {
                development_browser_extension
            };
            let browser =
                omicsops_browser::BrowserRuntime::new(data_dir.clone(), browser_extension_root);
            let repository = tauri::async_runtime::block_on(async {
                let repository = Store::open(data_dir.join("omicsops.db"))
                    .await
                    .map_err(|error| error.to_string())?;
                skill_commands::install_bundled_skills(
                    &repository,
                    &skills_root,
                    &bundled_skills_root,
                )
                .await?;
                let packaged_wisp = app
                    .path()
                    .resource_dir()
                    .map_err(|error| error.to_string())?
                    .join("skills")
                    .join("wisp-science");
                let development_wisp = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("..")
                    .join("skills")
                    .join("wisp-science");
                let wisp_root = if packaged_wisp.is_dir() {
                    packaged_wisp
                } else {
                    development_wisp
                };
                skill_commands::install_bundled_skills(&repository, &skills_root, &wisp_root)
                    .await?;
                if let Some(value) = repository
                    .browser_settings()
                    .await
                    .map_err(|error| error.to_string())?
                {
                    let config: omicsops_browser::BrowserConfig =
                        serde_json::from_value(value).map_err(|error| error.to_string())?;
                    browser
                        .set_config(config)
                        .await
                        .map_err(|error| error.to_string())?;
                }
                kernel_commands::mark_orphaned_kernels_interrupted(&repository).await?;
                sync_commands::mark_orphaned_sync_transfers_failed(&repository).await?;
                Ok::<_, String>(repository)
            })
            .map_err(std::io::Error::other)?;
            app.manage(AppState {
                repository,
                credentials: SystemCredentialVault,
                mcp_sessions: omicsops_mcp::McpSessionManager::new(),
                active_runs: Arc::new(Mutex::new(HashMap::new())),
                skills_root,
                research_last_request: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
                active_kernels: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
                project_kernel_queues: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
                sync_controls: Arc::new(Mutex::new(HashMap::new())),
                browser,
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::list_connections,
            commands::save_connection,
            commands::test_connection,
            commands::confirm_host_key,
            commands::inspect_project,
            commands::initialize_project,
            conversation_mode::get_conversation_agent_mode,
            conversation_mode::set_conversation_agent_mode,
            agent_v4::agent_v4_start_planning,
            agent_v4::agent_v4_start_direct,
            agent_v4::agent_v4_compute_backends,
            agent_v4::agent_v4_conversation_state,
            agent_v4::agent_v4_approve_plan,
            agent_v4::agent_v4_request_plan_revision,
            agent_v4::agent_v4_resume,
            agent_v4::agent_v4_cancel,
            agent_v4::agent_v4_cancel_runtime_recovery,
            agent_v4::agent_v4_answer,
            agent_v4::agent_v4_submit_guidance,
            agent_v4::agent_v4_list_guidance,
            agent_v4::agent_v4_decide_tool_approval,
            agent_v4::agent_v4_resolve_uncertain,
            agent_v4::agent_v4_events,
            agent_v4::agent_v4_events_for_conversation,
            browser_commands::browser_get_settings,
            browser_commands::browser_save_settings,
            browser_commands::browser_status,
            browser_commands::browser_setup,
            browser_commands::browser_close_run_tabs,
            browser_commands::browser_list_authorizations,
            browser_commands::browser_revoke_authorization,
            p1_commands::search_agent_memory,
            p1_commands::list_notebook_entries,
            p1_commands::list_project_artifacts,
            p1_commands::export_project_notebook,
            p1_commands::inspect_mcp_server,
            p1_commands::call_mcp_tool,
            p1_commands::list_mcp_servers,
            p1_commands::save_mcp_server,
            p1_commands::add_pubmed_mcp_server,
            bundled_mcp_commands::list_bundled_mcp_presets,
            bundled_mcp_commands::add_bundled_mcp_server,
            p1_commands::set_mcp_server_enabled,
            p1_commands::set_mcp_launch_approval,
            p1_commands::inspect_configured_mcp_server,
            p1_commands::set_mcp_tool_approval,
            p1_commands::call_configured_mcp_tool,
            workspace_commands::list_projects,
            workspace_commands::create_project,
            workspace_commands::delete_project,
            workspace_commands::update_project_remote,
            workspace_commands::list_conversations,
            workspace_commands::create_conversation,
            workspace_commands::delete_conversation,
            agent_commands::list_messages,
            agent_commands::submit_message,
            model_commands::list_model_profiles,
            model_commands::save_model_profile,
            model_commands::probe_model_profile,
            model_commands::list_model_profile_models,
            skill_commands::list_skill_packages,
            skill_commands::import_skill_directory,
            skill_commands::set_skill_enabled,
            research_commands::search_research,
            kernel_commands::start_kernel,
            kernel_commands::execute_kernel_cell,
            kernel_commands::interrupt_kernel,
            kernel_commands::stop_kernel,
            kernel_commands::list_kernel_sessions,
            kernel_commands::promote_kernel_cell,
            sync_commands::list_remote_files,
            sync_commands::upload_selected_files,
            sync_commands::download_project_file,
            sync_commands::preview_project_image,
            sync_commands::list_sync_entries,
            sync_commands::pause_sync_transfer,
            sync_commands::cancel_sync_transfer,
            sync_commands::retry_sync_transfer,
        ])
        .run(tauri::generate_context!())
        .expect("failed to run OmicsOps desktop");
}
