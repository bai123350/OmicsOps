import { afterEach, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { getContextUsage } from "./context-usage-api";
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
afterEach(() => { Reflect.deleteProperty(window, "__TAURI_INTERNALS__"); vi.clearAllMocks(); });
it("sends scoped native arguments", async () => {
  Object.defineProperty(window, "__TAURI_INTERNALS__", { configurable: true, value: {} });
  await getContextUsage("p", "c");
  expect(invoke).toHaveBeenCalledWith("agent_v4_context_usage", { projectId: "p", conversationId: "c" });
});
it("does not fabricate usage in browser preview", async () => {
  await expect(getContextUsage("p", "c")).rejects.toThrow("requires the desktop app");
  expect(invoke).not.toHaveBeenCalled();
});
