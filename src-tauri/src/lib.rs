pub mod agent_commands;
pub mod agent_settings;
pub mod agent_v4;
pub mod bio_mcp;
pub mod browser_commands;
pub mod bundled_mcp_commands;
pub mod capability_commands;
pub mod commands;
pub mod composer_attachments;
pub mod composer_files;
pub mod composer_queue;
mod composer_queue_driver;
pub mod composer_quotes;
pub mod composer_reference_validation;
pub mod composer_references;
pub mod composer_workflows;
pub mod context_compaction;
mod conversation_branch_material;
pub mod conversation_branches;
pub mod conversation_export;
pub mod conversation_mode;
#[cfg(test)]
mod conversation_mode_tests;
pub mod conversation_preferences;
#[cfg(test)]
mod conversation_state_tests;
pub mod credential_settings;
pub mod dto;
#[cfg(test)]
mod dto_contract_tests;
mod follow_up_questions;
pub mod general_settings;
pub mod inspection;
pub mod integration_packages;
pub mod kernel_commands;
mod model_catalog_shared;
pub mod model_commands;
pub mod model_deletion;
pub mod notification_settings;
pub mod p1_commands;
#[cfg(test)]
mod plan_revision_tests;
pub mod project_memory;
pub mod project_templates;
pub mod pubmed_mcp;
mod remote_jobs_v4;
pub mod research_commands;
mod run_ownership;
mod runtime_jobs_v4;
pub mod session_reviews;
pub mod side_chat;
pub mod skill_commands;
pub mod skill_settings;
pub mod storage_settings;
pub mod sync_commands;
pub mod usage_settings;
pub mod workspace_commands;
pub mod workspace_navigation;

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
        .plugin(tauri_plugin_notification::init())
        .setup(|app| {
            #[cfg(target_os = "windows")]
            if let Some(window) = app.get_webview_window("main") {
                window.set_decorations(false)?;
            }
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
                session_reviews::recover_interrupted_reviews(&repository, &data_dir).await?;
                side_chat::recover_interrupted_side_chats(&repository, &data_dir).await?;
                skill_settings::reconcile_skill_removals(&repository, &skills_root).await?;
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
                bundled_mcp_commands::install_bundled_mcp_servers(
                    &repository,
                    &std::env::current_exe().map_err(|error| error.to_string())?,
                )
                .await?;
                integration_packages::reconcile_plugins(&repository, &data_dir.join("plugins"))
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
                skills_gate: Arc::new(tokio::sync::RwLock::new(())),
                research_last_request: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
                active_kernels: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
                project_kernel_queues: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
                sync_controls: Arc::new(Mutex::new(HashMap::new())),
                browser,
            });
            app.manage(credential_settings::CredentialMutationState::default());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            conversation_export::save_conversation_export,
            composer_references::composer_reference_catalog,
            composer_files::list_local_composer_files,
            composer_files::resolve_composer_clipboard_paths,
            composer_workflows::list_composer_workflows,
            composer_workflows::save_composer_workflow,
            project_templates::list_quick_actions,
            project_templates::save_quick_action,
            project_templates::delete_quick_action,
            project_templates::list_specialist_templates,
            project_templates::save_specialist_template,
            project_templates::delete_specialist_template,
            usage_settings::settings_usage_page,
            usage_settings::settings_usage_conversations,
            composer_quotes::preview_composer_file_text,
            composer_quotes::create_composer_quote,
            conversation_preferences::conversation_get_agent_preferences_v4,
            conversation_preferences::conversation_save_agent_preferences_v4,
            composer_attachments::choose_composer_attachments,
            composer_attachments::stage_composer_attachment,
            composer_attachments::validate_composer_attachments,
            composer_reference_validation::validate_composer_references,
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
            agent_v4::agent_v4_request_stop,
            agent_v4::agent_v4_get_stop,
            agent_v4::agent_v4_cancel_runtime_recovery,
            agent_v4::agent_v4_answer,
            agent_v4::agent_v4_submit_guidance,
            agent_v4::agent_v4_list_guidance,
            agent_v4::agent_v4_decide_tool_approval,
            agent_v4::agent_v4_resolve_uncertain,
            agent_v4::agent_v4_events,
            agent_v4::agent_v4_events_for_conversation,
            agent_v4::agent_v4_context_usage,
            context_compaction::agent_v4_compact_context,
            browser_commands::browser_get_settings,
            follow_up_questions::agent_v4_suggest_follow_up_questions,
            agent_settings::agent_get_iteration_settings,
            agent_settings::agent_save_iteration_settings,
            general_settings::settings_general_preferences,
            general_settings::settings_save_general_preferences,
            general_settings::settings_general_system_status,
            general_settings::settings_probe_system_interpreters,
            general_settings::settings_project_directory_start_available,
            integration_packages::settings_inspect_plugin,
            integration_packages::settings_install_plugin,
            integration_packages::settings_list_plugins,
            integration_packages::settings_set_plugin_enabled,
            integration_packages::settings_remove_plugin,
            notification_settings::settings_notification_status,
            notification_settings::settings_set_notifications_enabled,
            notification_settings::settings_send_test_notification,
            notification_settings::notify_run_event,
            session_reviews::reviewer_get_settings_v4,
            session_reviews::reviewer_save_settings_v4,
            session_reviews::session_start_review_v4,
            side_chat::side_chat_send_v4,
            side_chat::side_chat_list_v4,
            side_chat::side_chat_get_v4,
            session_reviews::session_list_reviews_v4,
            session_reviews::session_get_review_v4,
            side_chat::side_chat_send_v4,
            side_chat::side_chat_list_v4,
            side_chat::side_chat_get_v4,
            browser_commands::browser_save_settings,
            browser_commands::browser_status,
            browser_commands::browser_setup,
            browser_commands::browser_close_run_tabs,
            browser_commands::browser_list_authorizations,
            browser_commands::browser_revoke_authorization,
            p1_commands::search_agent_memory,
            project_memory::list_project_memory_files,
            project_memory::read_project_memory_file,
            project_memory::create_project_memory_file,
            project_memory::update_project_memory_file,
            project_memory::delete_project_memory_file,
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
            capability_commands::get_conversation_capabilities_v4,
            p1_commands::set_mcp_server_enabled,
            p1_commands::set_mcp_launch_approval,
            p1_commands::inspect_configured_mcp_server,
            p1_commands::set_mcp_tool_approval,
            p1_commands::call_configured_mcp_tool,
            workspace_commands::list_projects,
            workspace_commands::create_project,
            workspace_commands::delete_project,
            workspace_commands::update_project_remote,
            composer_queue::composer_queue_list,
            composer_queue::composer_queue_enqueue,
            composer_queue::composer_queue_replace,
            composer_queue::composer_queue_update,
            composer_queue::composer_queue_action,
            composer_queue::composer_queue_reconcile,
            workspace_commands::list_conversations,
            workspace_commands::latest_used_conversation,
            workspace_commands::create_conversation,
            workspace_commands::delete_conversation,
            workspace_navigation::workspace_list_groups,
            workspace_navigation::workspace_save_group,
            workspace_navigation::workspace_delete_group,
            workspace_navigation::workspace_move_conversations,
            workspace_navigation::workspace_journey,
            workspace_navigation::workspace_source_detail,
            workspace_navigation::workspace_list_publications,
            workspace_navigation::workspace_get_publication,
            workspace_navigation::workspace_save_publication,
            workspace_navigation::workspace_restore_publication,
            workspace_navigation::workspace_export_publication,
            workspace_navigation::workspace_list_library,
            workspace_navigation::workspace_get_library_item,
            workspace_navigation::workspace_save_library_item,
            workspace_navigation::workspace_delete_library_item,
            storage_settings::settings_storage_usage,
            credential_settings::settings_list_credentials,
            credential_settings::settings_create_credential,
            credential_settings::settings_replace_credential,
            credential_settings::settings_delete_credential,
            conversation_branches::conversation_branch_checkpoint_v4,
            conversation_branches::conversation_branch_create_v4,
            conversation_branches::conversation_branch_create_and_send_v4,
            conversation_branches::conversation_branch_get_v4,
            conversation_branches::conversation_branch_list_v4,
            agent_commands::list_messages,
            agent_commands::submit_message,
            model_commands::list_model_profiles,
            model_commands::save_model_profile,
            model_deletion::delete_model_profile,
            model_commands::probe_model_profile,
            model_commands::list_model_profile_models,
            skill_commands::list_skill_packages,
            skill_commands::import_skill_directory,
            skill_commands::set_skill_enabled,
            skill_settings::settings_skill_detail,
            skill_settings::settings_read_skill_file,
            skill_settings::settings_remove_skill,
            skill_settings::settings_list_skill_removals,
            skill_settings::settings_retry_skill_removal,
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
