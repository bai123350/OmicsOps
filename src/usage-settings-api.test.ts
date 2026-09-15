import { beforeEach, expect, it, vi } from "vitest";

const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
import { settingsUsageConversations, settingsUsagePage } from "./usage-settings-api";

beforeEach(() => {
  invoke.mockReset().mockResolvedValue({});
  Object.assign(window, { __TAURI_INTERNALS__: {} });
});

it("passes the same scoped filter and opaque cursor to both host projections", async () => {
  const filter = { project_id: "project-1", from: "2026-09-01T00:00:00Z", until: "2026-10-01T00:00:00Z" };
  await settingsUsagePage(filter, "aggregate-cursor");
  await settingsUsageConversations(filter, "conversation-cursor");
  expect(invoke).toHaveBeenNthCalledWith(1, "settings_usage_page", { filter, cursor: "aggregate-cursor" });
  expect(invoke).toHaveBeenNthCalledWith(2, "settings_usage_conversations", { filter, cursor: "conversation-cursor" });
});

it("does not fabricate usage outside the desktop host", async () => {
  Reflect.deleteProperty(window, "__TAURI_INTERNALS__");
  await expect(settingsUsagePage({})).rejects.toThrow("require the desktop app");
  expect(invoke).not.toHaveBeenCalled();
});
