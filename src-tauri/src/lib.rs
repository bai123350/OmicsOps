pub mod commands;
pub mod inspection;

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
            let repository = Repository::open(data_dir.join("omicsops.db"))
                .map_err(|error| error.to_string())?;
            app.manage(AppState {
                repository,
                credentials: SystemCredentialVault,
                active_runs: Arc::new(Mutex::new(HashMap::new())),
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
        ])
        .run(tauri::generate_context!())
        .expect("failed to run OmicsOps desktop");
}
