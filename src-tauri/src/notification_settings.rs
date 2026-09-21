use chrono::Utc;
use omicsops_dto::{
    NotificationFailure, NotificationPermission, NotificationPreferences, NotificationStatus,
    RunNotificationOutcome, RunNotificationResult,
};
use omicsops_protocol::AgentEventKindV4;
use omicsops_store::Store;
use tauri::{AppHandle, Manager, State};
use tauri_plugin_notification::{NotificationExt, PermissionState};
use uuid::Uuid;

use crate::commands::AppState;

const NOTIFICATION_PREFERENCES_KIND: &str = "notification_preferences_v1";
const NOTIFICATION_STATE_ID: &str = "current";
const NOTIFICATION_FAILURE_KIND: &str = "notification_failure_v1";
const NOTIFICATION_TITLE: &str = "OmicsOps";
const DELIVERY_FAILURE_MESSAGE: &str = "系统通知发送失败";
const PERMISSION_FAILURE_MESSAGE: &str = "系统通知权限未授予";

trait NotificationSink: Sync {
    fn permission(&self) -> Result<NotificationPermission, String>;
    fn request_permission(&self) -> Result<NotificationPermission, String>;
    fn send(&self, title: &str, body: &str) -> Result<(), String>;
}

struct TauriNotificationSink<'a>(&'a AppHandle);

fn map_permission(permission: PermissionState) -> NotificationPermission {
    match permission {
        PermissionState::Granted => NotificationPermission::Granted,
        PermissionState::Denied => NotificationPermission::Denied,
        PermissionState::Prompt | PermissionState::PromptWithRationale => {
            NotificationPermission::Prompt
        }
    }
}

impl NotificationSink for TauriNotificationSink<'_> {
    fn permission(&self) -> Result<NotificationPermission, String> {
        self.0
            .notification()
            .permission_state()
            .map(map_permission)
            .map_err(|_| "系统通知状态读取失败".into())
    }

    fn request_permission(&self) -> Result<NotificationPermission, String> {
        self.0
            .notification()
            .request_permission()
            .map(map_permission)
            .map_err(|_| "系统通知权限请求失败".into())
    }

    fn send(&self, title: &str, body: &str) -> Result<(), String> {
        self.0
            .notification()
            .builder()
            .title(title)
            .body(body)
            .show()
            .map_err(|_| DELIVERY_FAILURE_MESSAGE.into())
    }
}

async fn load_preferences(repository: &Store) -> Result<NotificationPreferences, String> {
    repository
        .get_json(NOTIFICATION_PREFERENCES_KIND, NOTIFICATION_STATE_ID)
        .await
        .map(|value| value.unwrap_or_default())
        .map_err(|error| error.to_string())
}

async fn save_preferences(
    repository: &Store,
    preferences: NotificationPreferences,
) -> Result<NotificationPreferences, String> {
    repository
        .put_json(
            NOTIFICATION_PREFERENCES_KIND,
            NOTIFICATION_STATE_ID,
            &preferences,
        )
        .await
        .map_err(|error| error.to_string())?;
    Ok(preferences)
}

async fn load_failure(repository: &Store) -> Result<Option<NotificationFailure>, String> {
    repository
        .get_json(NOTIFICATION_FAILURE_KIND, NOTIFICATION_STATE_ID)
        .await
        .map_err(|error| error.to_string())
}

async fn record_failure(repository: &Store, message: &'static str) -> Result<(), String> {
    repository
        .put_json(
            NOTIFICATION_FAILURE_KIND,
            NOTIFICATION_STATE_ID,
            &NotificationFailure {
                message: message.into(),
                occurred_at: Utc::now().to_rfc3339(),
            },
        )
        .await
        .map_err(|error| error.to_string())
}

async fn clear_failure(repository: &Store) -> Result<(), String> {
    repository
        .delete_json(NOTIFICATION_FAILURE_KIND, NOTIFICATION_STATE_ID)
        .await
        .map(|_| ())
        .map_err(|error| error.to_string())
}

async fn notification_status_with(
    repository: &Store,
    sink: &impl NotificationSink,
    platform: &str,
) -> Result<NotificationStatus, String> {
    let preferences = load_preferences(repository).await?;
    Ok(NotificationStatus {
        preference_enabled: preferences.enabled,
        permission: sink.permission()?,
        platform: platform.to_owned(),
        last_failure: load_failure(repository).await?,
    })
}

async fn set_notifications_enabled_with(
    repository: &Store,
    sink: &impl NotificationSink,
    enabled: bool,
    platform: &str,
) -> Result<NotificationStatus, String> {
    save_preferences(repository, NotificationPreferences { enabled }).await?;
    if enabled && sink.permission()? != NotificationPermission::Granted {
        if sink.request_permission().is_err() {
            record_failure(repository, PERMISSION_FAILURE_MESSAGE).await?;
        }
    }
    notification_status_with(repository, sink, platform).await
}

async fn send_test_notification_with(
    repository: &Store,
    sink: &impl NotificationSink,
    platform: &str,
) -> Result<NotificationStatus, String> {
    let permission = if sink.permission()? == NotificationPermission::Granted {
        NotificationPermission::Granted
    } else {
        sink.request_permission()?
    };
    if permission != NotificationPermission::Granted {
        record_failure(repository, PERMISSION_FAILURE_MESSAGE).await?;
        return Err(PERMISSION_FAILURE_MESSAGE.into());
    }
    if sink
        .send(NOTIFICATION_TITLE, "这是一条 OmicsOps 测试通知")
        .is_err()
    {
        record_failure(repository, DELIVERY_FAILURE_MESSAGE).await?;
        return Err(DELIVERY_FAILURE_MESSAGE.into());
    }
    clear_failure(repository).await?;
    notification_status_with(repository, sink, platform).await
}

fn notification_body(event: &AgentEventKindV4) -> Option<&'static str> {
    match event {
        AgentEventKindV4::RunCompleted => Some("任务已完成"),
        AgentEventKindV4::RunFailed { .. } => Some("任务失败"),
        AgentEventKindV4::RunNeedsAttention { .. }
        | AgentEventKindV4::ToolApprovalRequested { .. } => Some("任务需要处理"),
        _ => None,
    }
}

async fn notify_run_event_with(
    repository: &Store,
    sink: &impl NotificationSink,
    foreground: bool,
    run_id: Uuid,
    event_hash: &str,
) -> Result<RunNotificationResult, String> {
    let event = repository
        .agent_event_v4_by_hash(run_id, event_hash)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "notification event does not match the persisted run".to_owned())?;
    let body = notification_body(&event.event)
        .ok_or_else(|| "notification event kind is not allowed".to_owned())?;

    // Claim before any skip or operating-system interaction. This prevents a
    // duplicate live subscription from turning a foreground/disabled event
    // into a delayed notification after state changes.
    if !repository
        .claim_notification_receipt(event_hash)
        .await
        .map_err(|error| error.to_string())?
    {
        return Ok(RunNotificationResult {
            outcome: RunNotificationOutcome::Duplicate,
        });
    }
    if !load_preferences(repository).await?.enabled {
        return Ok(RunNotificationResult {
            outcome: RunNotificationOutcome::SkippedDisabled,
        });
    }
    if foreground {
        return Ok(RunNotificationResult {
            outcome: RunNotificationOutcome::SkippedForeground,
        });
    }
    if sink.permission()? != NotificationPermission::Granted {
        return Ok(RunNotificationResult {
            outcome: RunNotificationOutcome::SkippedPermission,
        });
    }
    if sink.send(NOTIFICATION_TITLE, body).is_err() {
        record_failure(repository, DELIVERY_FAILURE_MESSAGE).await?;
        return Ok(RunNotificationResult {
            outcome: RunNotificationOutcome::Failed,
        });
    }
    Ok(RunNotificationResult {
        outcome: RunNotificationOutcome::Sent,
    })
}

fn main_window_is_focused(app: &AppHandle) -> bool {
    app.get_webview_window("main")
        .and_then(|window| window.is_focused().ok())
        .unwrap_or(false)
}

#[tauri::command]
pub async fn settings_notification_status(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<NotificationStatus, String> {
    notification_status_with(
        &state.repository,
        &TauriNotificationSink(&app),
        std::env::consts::OS,
    )
    .await
}

#[tauri::command]
pub async fn settings_set_notifications_enabled(
    app: AppHandle,
    state: State<'_, AppState>,
    enabled: bool,
) -> Result<NotificationStatus, String> {
    set_notifications_enabled_with(
        &state.repository,
        &TauriNotificationSink(&app),
        enabled,
        std::env::consts::OS,
    )
    .await
}

#[tauri::command]
pub async fn settings_send_test_notification(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<NotificationStatus, String> {
    send_test_notification_with(
        &state.repository,
        &TauriNotificationSink(&app),
        std::env::consts::OS,
    )
    .await
}

#[tauri::command]
pub async fn notify_run_event(
    app: AppHandle,
    state: State<'_, AppState>,
    run_id: Uuid,
    event_hash: String,
) -> Result<RunNotificationResult, String> {
    notify_run_event_with(
        &state.repository,
        &TauriNotificationSink(&app),
        main_window_is_focused(&app),
        run_id,
        &event_hash,
    )
    .await
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use chrono::Utc;
    use omicsops_core::workspace::{Conversation, Project, ProjectTemplate};
    use omicsops_dto::{NotificationPermission, RunNotificationOutcome};
    use omicsops_protocol::{AgentEventKindV4, AgentEventV4};
    use omicsops_store::Store;
    use uuid::Uuid;

    use super::*;

    struct FakeSink {
        permission: Mutex<NotificationPermission>,
        request_count: Mutex<usize>,
        sends: Mutex<Vec<(String, String)>>,
        failure: Mutex<Option<String>>,
    }

    impl FakeSink {
        fn new(permission: NotificationPermission) -> Self {
            Self {
                permission: Mutex::new(permission),
                request_count: Mutex::new(0),
                sends: Mutex::new(vec![]),
                failure: Mutex::new(None),
            }
        }
    }

    impl NotificationSink for FakeSink {
        fn permission(&self) -> Result<NotificationPermission, String> {
            Ok(*self.permission.lock().unwrap())
        }

        fn request_permission(&self) -> Result<NotificationPermission, String> {
            *self.request_count.lock().unwrap() += 1;
            Ok(*self.permission.lock().unwrap())
        }

        fn send(&self, title: &str, body: &str) -> Result<(), String> {
            if let Some(error) = self.failure.lock().unwrap().clone() {
                return Err(error);
            }
            self.sends
                .lock()
                .unwrap()
                .push((title.to_owned(), body.to_owned()));
            Ok(())
        }
    }

    async fn stored_event(kind: AgentEventKindV4) -> (Store, AgentEventV4) {
        let store = Store::open_in_memory().await.unwrap();
        let now = Utc::now();
        let project_id = Uuid::new_v4();
        let conversation_id = Uuid::new_v4();
        let run_id = Uuid::new_v4();
        let root = tempfile::tempdir().unwrap().keep();
        store
            .save_project(&Project::new(
                project_id,
                "notifications",
                root.to_string_lossy(),
                ProjectTemplate::Blank,
                now,
            ))
            .await
            .unwrap();
        store
            .save_conversation(&Conversation::new(
                conversation_id,
                project_id,
                "notifications",
                now,
            ))
            .await
            .unwrap();
        store
            .save_agent_run_v4(
                run_id,
                project_id,
                conversation_id,
                "running",
                &serde_json::json!({}),
            )
            .await
            .unwrap();
        let event = AgentEventV4::first(run_id, project_id, conversation_id, now, kind);
        store.append_agent_event_v4(&event).await.unwrap();
        (store, event)
    }

    #[tokio::test]
    async fn disabled_foreground_and_denied_permission_never_send() {
        for (enabled, foreground, permission, expected) in [
            (
                false,
                false,
                NotificationPermission::Granted,
                RunNotificationOutcome::SkippedDisabled,
            ),
            (
                true,
                true,
                NotificationPermission::Granted,
                RunNotificationOutcome::SkippedForeground,
            ),
            (
                true,
                false,
                NotificationPermission::Denied,
                RunNotificationOutcome::SkippedPermission,
            ),
        ] {
            let (store, event) = stored_event(AgentEventKindV4::RunFailed {
                message: "private".into(),
            })
            .await;
            save_preferences(&store, NotificationPreferences { enabled })
                .await
                .unwrap();
            let sink = FakeSink::new(permission);
            let result =
                notify_run_event_with(&store, &sink, foreground, event.run_id, &event.event_hash)
                    .await
                    .unwrap();
            assert_eq!(result.outcome, expected);
            assert!(sink.sends.lock().unwrap().is_empty());
        }
    }

    #[tokio::test]
    async fn validates_persisted_run_hash_and_allowlisted_event_kind() {
        let (store, event) = stored_event(AgentEventKindV4::ModelText {
            text: "secret-sentinel".into(),
        })
        .await;
        save_preferences(&store, NotificationPreferences { enabled: true })
            .await
            .unwrap();
        let sink = FakeSink::new(NotificationPermission::Granted);
        assert!(
            notify_run_event_with(&store, &sink, false, event.run_id, &event.event_hash)
                .await
                .is_err()
        );
        assert!(
            notify_run_event_with(&store, &sink, false, event.run_id, &"f".repeat(64))
                .await
                .is_err()
        );
        assert!(
            notify_run_event_with(&store, &sink, false, Uuid::new_v4(), &event.event_hash)
                .await
                .is_err()
        );
        assert!(sink.sends.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn concurrent_duplicate_sends_once_with_private_generic_text() {
        let (store, event) = stored_event(AgentEventKindV4::RunFailed {
            message: "secret-sentinel".into(),
        })
        .await;
        save_preferences(&store, NotificationPreferences { enabled: true })
            .await
            .unwrap();
        let sink = FakeSink::new(NotificationPermission::Granted);
        let (first, second) = tokio::join!(
            notify_run_event_with(&store, &sink, false, event.run_id, &event.event_hash),
            notify_run_event_with(&store, &sink, false, event.run_id, &event.event_hash),
        );
        let outcomes = [first.unwrap().outcome, second.unwrap().outcome];
        assert!(outcomes.contains(&RunNotificationOutcome::Sent));
        assert!(outcomes.contains(&RunNotificationOutcome::Duplicate));
        let sends = sink.sends.lock().unwrap();
        assert_eq!(sends.len(), 1);
        assert_eq!(sends[0], ("OmicsOps".into(), "任务失败".into()));
        assert!(!format!("{:?}", sends[0]).contains("secret-sentinel"));
    }

    #[tokio::test]
    async fn send_failure_is_visible_and_is_not_retried() {
        let (store, event) = stored_event(AgentEventKindV4::RunNeedsAttention {
            message: "secret-sentinel".into(),
        })
        .await;
        save_preferences(&store, NotificationPreferences { enabled: true })
            .await
            .unwrap();
        let sink = FakeSink::new(NotificationPermission::Granted);
        *sink.failure.lock().unwrap() =
            Some("secret-sentinel native notification unavailable".into());
        let first = notify_run_event_with(&store, &sink, false, event.run_id, &event.event_hash)
            .await
            .unwrap();
        let second = notify_run_event_with(&store, &sink, false, event.run_id, &event.event_hash)
            .await
            .unwrap();
        assert_eq!(first.outcome, RunNotificationOutcome::Failed);
        assert_eq!(second.outcome, RunNotificationOutcome::Duplicate);
        let status = notification_status_with(&store, &sink, "windows")
            .await
            .unwrap();
        let failure = status.last_failure.unwrap();
        assert_eq!(failure.message, DELIVERY_FAILURE_MESSAGE);
        assert!(
            !serde_json::to_string(&failure)
                .unwrap()
                .contains("secret-sentinel")
        );
    }

    #[tokio::test]
    async fn permission_is_requested_only_by_enable_or_test_actions() {
        let store = Store::open_in_memory().await.unwrap();
        let sink = FakeSink::new(NotificationPermission::Prompt);
        let status = notification_status_with(&store, &sink, "windows")
            .await
            .unwrap();
        assert_eq!(status.permission, NotificationPermission::Prompt);
        assert_eq!(*sink.request_count.lock().unwrap(), 0);

        let enabled = set_notifications_enabled_with(&store, &sink, true, "windows")
            .await
            .unwrap();
        assert!(enabled.preference_enabled);
        assert_eq!(*sink.request_count.lock().unwrap(), 1);

        let disabled = set_notifications_enabled_with(&store, &sink, false, "windows")
            .await
            .unwrap();
        assert!(!disabled.preference_enabled);
        assert_eq!(*sink.request_count.lock().unwrap(), 1);

        let result = send_test_notification_with(&store, &sink, "windows").await;
        assert!(result.is_err());
        assert_eq!(*sink.request_count.lock().unwrap(), 2);
    }

    #[test]
    fn webview_has_no_permission_to_bypass_validated_native_commands() {
        let capability = include_str!("../capabilities/default.json");
        assert!(!capability.contains("notification:allow-notify"));
        assert!(!capability.contains("notification:default"));
    }
}
