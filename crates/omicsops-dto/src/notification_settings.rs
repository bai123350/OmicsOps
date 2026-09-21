use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotificationPermission {
    Granted,
    Denied,
    Prompt,
    Unsupported,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NotificationPreferences {
    #[serde(default)]
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NotificationFailure {
    pub message: String,
    pub occurred_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NotificationStatus {
    pub preference_enabled: bool,
    pub permission: NotificationPermission,
    pub platform: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_failure: Option<NotificationFailure>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunNotificationOutcome {
    Sent,
    SkippedDisabled,
    SkippedForeground,
    SkippedPermission,
    Duplicate,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunNotificationResult {
    pub outcome: RunNotificationOutcome,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn notification_status_contract_is_snake_case_and_default_is_off() {
        assert!(!NotificationPreferences::default().enabled);
        let status = NotificationStatus {
            preference_enabled: true,
            permission: NotificationPermission::Denied,
            platform: "windows".into(),
            last_failure: Some(NotificationFailure {
                message: "系统通知发送失败".into(),
                occurred_at: "2026-09-15T00:00:00Z".into(),
            }),
        };
        assert_eq!(
            serde_json::to_value(status).unwrap(),
            json!({
                "preference_enabled": true,
                "permission": "denied",
                "platform": "windows",
                "last_failure": {
                    "message": "系统通知发送失败",
                    "occurred_at": "2026-09-15T00:00:00Z"
                }
            })
        );
        assert_eq!(
            serde_json::to_value(RunNotificationResult {
                outcome: RunNotificationOutcome::SkippedForeground
            })
            .unwrap(),
            json!({ "outcome": "skipped_foreground" })
        );
    }
}
