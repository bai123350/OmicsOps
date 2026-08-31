use omicsops_browser::{
    BrowserConfig, BrowserRuntime, BrowserSessionKind, BrowserStatus, BrowserTabSummary,
    PROTOCOL_VERSION,
};
use omicsops_protocol::{
    AgentEventKindV4, BrowserApprovalBindingV4, BrowserAuthorizationV4, BrowserSessionKindV4,
};
use serde::Serialize;
use serde_json::Value;
use tauri::{AppHandle, Manager, State};
use uuid::Uuid;

use crate::commands::AppState;

#[derive(Debug, Clone, Serialize)]
pub struct BrowserSettingsResponse {
    pub config: BrowserConfig,
    pub shared: BrowserStatus,
    pub workspace: BrowserStatus,
    pub extension_path: String,
}

#[tauri::command]
pub async fn browser_get_settings(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<BrowserSettingsResponse, String> {
    if let Some(value) = state
        .repository
        .browser_settings()
        .await
        .map_err(|error| error.to_string())?
    {
        let config: BrowserConfig =
            serde_json::from_value(value).map_err(|error| error.to_string())?;
        state
            .browser
            .set_config(config)
            .await
            .map_err(|error| error.to_string())?;
    }
    browser_settings_response(&app, &state.browser).await
}

#[tauri::command]
pub async fn browser_save_settings(
    app: AppHandle,
    state: State<'_, AppState>,
    config: BrowserConfig,
) -> Result<BrowserSettingsResponse, String> {
    let previous = state.browser.config().await;
    state
        .browser
        .set_config(config)
        .await
        .map_err(|error| error.to_string())?;
    let config = state.browser.config().await;
    if let Err(error) = state
        .repository
        .save_browser_settings(&serde_json::to_value(&config).map_err(|error| error.to_string())?)
        .await
    {
        state
            .browser
            .set_config(previous)
            .await
            .map_err(|rollback| {
                format!(
                    "browser settings persistence failed ({error}); rollback failed: {rollback}"
                )
            })?;
        return Err(error.to_string());
    }
    browser_settings_response(&app, &state.browser).await
}

#[tauri::command]
pub async fn browser_status(
    state: State<'_, AppState>,
    session: String,
) -> Result<BrowserStatus, String> {
    Ok(state.browser.status(parse_session(&session)?).await)
}

#[tauri::command]
pub async fn browser_setup(
    state: State<'_, AppState>,
    session: String,
    launch_if_needed: Option<bool>,
) -> Result<BrowserStatus, String> {
    state
        .browser
        .setup(parse_session(&session)?, launch_if_needed.unwrap_or(true))
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn browser_close_run_tabs(
    state: State<'_, AppState>,
    session: String,
    run_id: Uuid,
) -> Result<(), String> {
    let session = parse_session(&session)?;
    let protocol_session = match session {
        BrowserSessionKind::Shared => BrowserSessionKindV4::Shared,
        BrowserSessionKind::Workspace => BrowserSessionKindV4::Workspace,
    };
    let events = state
        .repository
        .agent_events_v4(run_id)
        .await
        .map_err(|error| error.to_string())?;
    let persisted_targets = events.iter().rev().find_map(|event| match &event.event {
        AgentEventKindV4::BrowserTabCleanupRequired { tabs, .. } => Some(
            tabs.iter()
                .filter(|tab| tab.session == protocol_session)
                .map(|tab| BrowserTabSummary {
                    tab_id: tab.tab_id,
                    run_id: tab.run_id,
                    title: tab.title.clone(),
                    origin: tab.origin.clone(),
                    created_by_run: tab.created_by_run,
                })
                .collect::<Vec<_>>(),
        ),
        _ => None,
    });
    match persisted_targets {
        Some(targets) if !targets.is_empty() => {
            state
                .browser
                .force_close_run_tab_targets(session, run_id, targets)
                .await
        }
        _ => state.browser.force_close_run_tabs(session, run_id).await,
    }
    .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn browser_list_authorizations(
    state: State<'_, AppState>,
) -> Result<Vec<BrowserAuthorizationV4>, String> {
    state
        .repository
        .list_browser_authorizations_v4()
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn browser_revoke_authorization(
    state: State<'_, AppState>,
    id: String,
) -> Result<bool, String> {
    state
        .repository
        .revoke_browser_authorization_v4(&id)
        .await
        .map_err(|error| error.to_string())
}

async fn browser_settings_response(
    app: &AppHandle,
    runtime: &BrowserRuntime,
) -> Result<BrowserSettingsResponse, String> {
    let packaged = app
        .path()
        .resource_dir()
        .map_err(|error| error.to_string())?
        .join("browser-extension");
    let development = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("browser-extension");
    let extension = if packaged.is_dir() {
        packaged
    } else {
        development
    };
    Ok(BrowserSettingsResponse {
        config: runtime.config().await,
        shared: runtime.status(BrowserSessionKind::Shared).await,
        workspace: runtime.status(BrowserSessionKind::Workspace).await,
        extension_path: extension.to_string_lossy().into_owned(),
    })
}

fn parse_session(value: &str) -> Result<BrowserSessionKind, String> {
    match value {
        "shared" => Ok(BrowserSessionKind::Shared),
        "workspace" => Ok(BrowserSessionKind::Workspace),
        _ => Err("browser session must be shared or workspace".into()),
    }
}

fn normalize_capability(value: &str) -> Result<String, String> {
    let value = value.trim();
    if !matches!(
        value,
        "browser_setup"
            | "web_search"
            | "web_open_tab"
            | "web_scan"
            | "web_execute_js"
            | "web_screenshot"
            | "web_save_assets"
    ) {
        return Err("unknown browser capability".into());
    }
    Ok(value.into())
}

fn normalize_target_host(value: &str) -> Result<String, String> {
    let value = value.trim().trim_end_matches('.').to_ascii_lowercase();
    if value == "browser-session"
        || (!value.is_empty()
            && value.len() <= 253
            && value.chars().all(|character| {
                character.is_ascii_alphanumeric() || matches!(character, '.' | '-')
            }))
    {
        Ok(value)
    } else {
        Err("invalid browser target host".into())
    }
}

pub fn browser_binding_for_call(
    tool_id: &str,
    arguments: &Value,
) -> Result<BrowserApprovalBindingV4, String> {
    let session = match arguments
        .get("session")
        .and_then(Value::as_str)
        .unwrap_or("workspace")
    {
        "shared" => BrowserSessionKindV4::Shared,
        "workspace" => BrowserSessionKindV4::Workspace,
        _ => return Err("browser session must be shared or workspace".into()),
    };
    let target_host = match tool_id {
        "web_open_tab" => {
            let value = arguments
                .get("url")
                .and_then(Value::as_str)
                .ok_or("web_open_tab requires url")?;
            url::Url::parse(value)
                .map_err(|error| error.to_string())?
                .host_str()
                .ok_or("browser URL has no target host")?
                .to_ascii_lowercase()
        }
        "web_search" => match arguments
            .get("provider")
            .and_then(Value::as_str)
            .unwrap_or("default")
        {
            "google" => "www.google.com".into(),
            "bing" => "www.bing.com".into(),
            "duckduckgo" => "duckduckgo.com".into(),
            _ => "browser-session".into(),
        },
        "web_scan" | "web_execute_js" | "web_screenshot" | "web_save_assets" => {
            normalize_target_host(
                arguments
                    .get("target_host")
                    .and_then(Value::as_str)
                    .ok_or("browser tab command requires target_host")?,
            )?
        }
        _ => "browser-session".into(),
    };
    Ok(BrowserApprovalBindingV4 {
        capability: normalize_capability(tool_id)?,
        target_host,
        session,
        protocol_version: PROTOCOL_VERSION,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binding_is_exact_for_host_session_protocol_and_capability() {
        let binding = browser_binding_for_call(
            "web_open_tab",
            &serde_json::json!({"session":"shared","url":"https://EXAMPLE.org/paper"}),
        )
        .unwrap();
        assert_eq!(binding.capability, "web_open_tab");
        assert_eq!(binding.target_host, "example.org");
        assert_eq!(binding.session, BrowserSessionKindV4::Shared);
        assert_eq!(binding.protocol_version, PROTOCOL_VERSION);
    }

    #[test]
    fn rejects_wildcard_target_and_unknown_capability() {
        assert!(normalize_target_host("*").is_err());
        assert!(normalize_capability("runtime.execute").is_err());
    }
}
