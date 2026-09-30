import { afterEach, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import * as api from "./tauri-api";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
afterEach(() => { Reflect.deleteProperty(window, "__TAURI_INTERNALS__"); vi.clearAllMocks(); });
it("passes the subscription lifecycle through narrow native commands", async () => {
  Object.defineProperty(window, "__TAURI_INTERNALS__", { value: {}, configurable: true });
  vi.mocked(invoke).mockResolvedValue({});
  const profile = { label: "Codex", provider: "open_ai_codex" as const, base_url: "https://chatgpt.com/backend-api", model: "full-model-id" };
  await api.beginCodexLogin("profile"); await api.pollCodexLogin("login"); await api.cancelCodexLogin("login");
  await api.finishCodexLogin({ login_id: "login", profile }); await api.subscriptionModelStatus("profile"); await api.disconnectCodex("profile");
  await api.listModelProfileModelDiscovery("profile");
  expect(vi.mocked(invoke).mock.calls).toEqual([
    ["subscription_begin_codex_login", { profileId: "profile" }],
    ["subscription_poll_codex_login", { loginId: "login" }],
    ["subscription_cancel_codex_login", { loginId: "login" }],
    ["subscription_finish_codex_login", { request: { login_id: "login", profile } }],
    ["subscription_model_status", { profileId: "profile" }],
    ["subscription_disconnect_codex", { profileId: "profile" }],
    ["list_model_profile_model_discovery", { profileId: "profile" }],
  ]);
});
it("does not simulate successful subscription authentication in a browser", async () => {
  await expect(api.beginCodexLogin()).rejects.toThrow("desktop");
  await expect(api.subscriptionModelStatus("profile")).rejects.toThrow("desktop");
  expect(invoke).not.toHaveBeenCalled();
});
