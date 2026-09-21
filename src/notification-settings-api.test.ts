import { beforeEach, expect, it, vi } from "vitest";

const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke }));

import {
  notifyRunEvent,
  settingsNotificationStatus,
  settingsSendTestNotification,
  settingsSetNotificationsEnabled,
} from "./notification-settings-api";

beforeEach(() => invoke.mockReset());

it("uses the fixed native notification commands and event identity only", async () => {
  invoke.mockResolvedValue({});
  await settingsNotificationStatus();
  await settingsSetNotificationsEnabled(true);
  await settingsSendTestNotification();
  await notifyRunEvent("run-1", "a".repeat(64));
  expect(invoke.mock.calls).toEqual([
    ["settings_notification_status"],
    ["settings_set_notifications_enabled", { enabled: true }],
    ["settings_send_test_notification"],
    ["notify_run_event", { runId: "run-1", eventHash: "a".repeat(64) }],
  ]);
});
