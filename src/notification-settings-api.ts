import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

import { isTauri } from "./tauri-api";
import type { AgentRunEventV4, NotificationStatus, RunNotificationResult } from "./types";

export async function settingsNotificationStatus(): Promise<NotificationStatus> {
  return invoke("settings_notification_status");
}

export async function settingsSetNotificationsEnabled(enabled: boolean): Promise<NotificationStatus> {
  return invoke("settings_set_notifications_enabled", { enabled });
}

export async function settingsSendTestNotification(): Promise<NotificationStatus> {
  return invoke("settings_send_test_notification");
}

export async function notifyRunEvent(runId: string, eventHash: string): Promise<RunNotificationResult> {
  return invoke("notify_run_event", { runId, eventHash });
}

/** Independent app-wide live subscription. It never reads hydrated history. */
export async function onRunNotificationCandidate(
  callback: (event: AgentRunEventV4) => void,
): Promise<UnlistenFn> {
  if (!isTauri()) return () => undefined;
  return listen<AgentRunEventV4>("agent-v4-event", ({ payload }) => callback(payload));
}
