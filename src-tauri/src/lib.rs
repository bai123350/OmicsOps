pub mod agent_commands;
pub mod commands;
pub mod inspection;
pub mod kernel_commands;
pub mod model_commands;
pub mod p1_commands;
pub mod research_commands;
pub mod skill_commands;
pub mod sync_commands;
pub mod workspace_commands;

use commands::AppState;
use omicsops_adapters::{credentials::SystemCredentialVault, persistence::Repository};
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
            let repository = Repository::open(data_dir.join("omicsops.db"))
                .map_err(|error| error.to_string())?;
            commands::backfill_agent_run_conversation_ids(&repository)?;
            skill_commands::install_bundled_skills(
                &repository,
                &skills_root,
                &bundled_skills_root,
            )?;
            kernel_commands::mark_orphaned_kernels_interrupted(&repository)?;
            sync_commands::mark_orphaned_sync_transfers_failed(&repository)?;
            app.manage(AppState {
                repository,
                credentials: SystemCredentialVault,
                active_runs: Arc::new(Mutex::new(HashMap::new())),
                skills_root,
                research_last_request: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
                active_kernels: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
                project_kernel_queues: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
                sync_controls: Arc::new(Mutex::new(HashMap::new())),
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
            commands::extract_plan,
            commands::save_llm_config,
            commands::probe_llm,
            commands::list_tools,
            commands::planning_turn,
            commands::validate_plan,
            commands::approve_plan,
            commands::start_run,
            commands::export_run_bundle,
            commands::resume_run_v2,
            commands::list_runs_v2,
            commands::list_run_events_v2,
            commands::list_agent_run_events,
            commands::list_artifacts_v2,
            commands::list_step_attempts_v2,
            commands::get_environment_lock_v2,
            commands::generate_plan,
            commands::approve_legacy_plan,
            commands::legacy_start_run,
            commands::list_runs,
            commands::list_run_events,
            commands::resume_run,
            commands::approve_run,
            commands::cancel_run,
            commands::list_artifacts,
            commands::download_artifact,
            p1_commands::search_agent_memory,
            p1_commands::list_notebook_entries,
            p1_commands::list_project_artifacts,
            p1_commands::export_project_notebook,
            p1_commands::inspect_mcp_server,
            p1_commands::call_mcp_tool,
            p1_commands::list_mcp_servers,
            p1_commands::save_mcp_server,
            p1_commands::set_mcp_server_enabled,
            p1_commands::inspect_configured_mcp_server,
            p1_commands::set_mcp_tool_approval,
            p1_commands::call_configured_mcp_tool,
            workspace_commands::list_projects,
            workspace_commands::create_project,
            workspace_commands::update_project_remote,
            workspace_commands::list_conversations,
            workspace_commands::create_conversation,
            agent_commands::list_messages,
            agent_commands::submit_message,
            agent_commands::run_agent_turn,
            agent_commands::propose_analysis_plan,
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
