import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import * as api from "../../tauri-api";
import type { ModelProfile } from "../../types";
import { SubscriptionModelForm } from "./SubscriptionModelForm";
import { SettingsPanel } from "./SettingsPanel";

afterEach(() => vi.restoreAllMocks());
const profile: ModelProfile = { id: "saved", label: "Codex", provider: "open_ai_codex", base_url: "https://chatgpt.com/backend-api", model: "full-id", credential_reference: "model/saved", subscription_account_ref: "opaque-host-binding", supports_tools: true, supports_vision: false };
const challenge = { login_id: "login", user_code: "TEST-CODE", verification_uri: "https://auth.openai.com/codex/device", expires_at: "2099-01-01T00:00:00Z" };
it("subscription_form_saves_provider_specific_payloads_without_secrets", async () => {
  vi.spyOn(api, "beginCodexLogin").mockResolvedValue(challenge);
  vi.spyOn(api, "pollCodexLogin").mockResolvedValue({ login_id: "login", state: "authorized", expires_at: challenge.expires_at, error_code: null });
  vi.spyOn(api, "cancelCodexLogin").mockResolvedValue({ login_id: "login", state: "cancelled", expires_at: challenge.expires_at, error_code: null });
  const finish = vi.spyOn(api, "finishCodexLogin").mockResolvedValue(profile);
  const saved = vi.fn();
  render(<SubscriptionModelForm provider="open_ai_codex" profile={null} onSaved={saved} onCancel={vi.fn()} zh={false} />);
  fireEvent.change(screen.getByLabelText("Model"), { target: { value: "full-id" } });
  fireEvent.click(screen.getByRole("button", { name: "Sign in to Codex" }));
  expect(await screen.findByText("TEST-CODE")).toBeVisible();
  await waitFor(() => expect(screen.getByRole("button", { name: "Save profile" })).toBeEnabled());
  fireEvent.click(screen.getByRole("button", { name: "Save profile" }));
  await waitFor(() => expect(saved).toHaveBeenCalledWith(profile));
  const payload = finish.mock.calls[0][0].profile;
  expect(payload).toMatchObject({ provider: "open_ai_codex", model: "full-id", base_url: "https://chatgpt.com/backend-api" });
  expect(payload).not.toHaveProperty("credential"); expect(payload).not.toHaveProperty("subscription_account_ref"); expect(payload).not.toHaveProperty("refresh_token");
});
it("subscription_form_escape_cancels_only_top_layer_and_ignores_late_authorization", async () => {
  let resolve!: (v: typeof challenge) => void;
  vi.spyOn(api, "beginCodexLogin").mockImplementation(() => new Promise(r => { resolve = r; }));
  const cancel = vi.spyOn(api, "cancelCodexLogin").mockResolvedValue({ login_id: "login", state: "cancelled", expires_at: challenge.expires_at, error_code: null });
  const finish = vi.spyOn(api, "finishCodexLogin"); const close = vi.fn();
  render(<SettingsPanel locale="en-US" onClose={close} />);
  fireEvent.click(screen.getByRole("button", { name: "Configure Codex subscription" }));
  fireEvent.click(screen.getByRole("button", { name: "Sign in to Codex" }));
  fireEvent.keyDown(window, { key: "Escape" });
  expect(screen.getByRole("dialog", { name: "Workspace settings" })).toBeVisible();
  expect(close).not.toHaveBeenCalled();
  await act(async () => resolve(challenge));
  await waitFor(() => expect(cancel).toHaveBeenCalledTimes(1));
  expect(finish).not.toHaveBeenCalled(); expect(screen.queryByText("TEST-CODE")).not.toBeInTheDocument();
});
it("Claude uses a native executable and shows the policy gate without a key field", async () => {
  const claude = { ...profile, provider: "claude_code" as const, base_url: "claude-code://local", credential_reference: null, subscription_account_ref: undefined };
  vi.spyOn(api, "subscriptionModelStatus").mockResolvedValue({ provider: "claude_code", authenticated: true, masked_account_label: null, cli_version: "2.1.248", error_code: "claude_policy_unverifiable" });
  const save = vi.spyOn(api, "saveModelProfile").mockResolvedValue(claude);
  render(<SubscriptionModelForm provider="claude_code" profile={claude} onSaved={vi.fn()} onCancel={vi.fn()} zh={false} />);
  expect(screen.queryByLabelText("API key")).toBeNull();
  expect(await screen.findByText(/managed policy cannot be verified/i)).toBeVisible();
  fireEvent.change(screen.getByLabelText("Native claude.exe path"), { target: { value: "C:\\Tools\\claude.exe" } });
  fireEvent.click(screen.getByRole("button", { name: "Save profile" }));
  await waitFor(() => expect(save).toHaveBeenCalled());
  expect(save.mock.calls[0][0]).toMatchObject({ provider: "claude_code", cli_executable: "C:\\Tools\\claude.exe" });
  expect(save.mock.calls[0][0]).not.toHaveProperty("credential"); expect(save.mock.calls[0][0]).not.toHaveProperty("subscription_account_ref");
});
it("cancels an existing challenge once and ignores a late poll result", async () => {
  vi.spyOn(api, "beginCodexLogin").mockResolvedValue(challenge);
  let authorize!: (value: apiReturn) => void;
  type apiReturn = Awaited<ReturnType<typeof api.pollCodexLogin>>;
  vi.spyOn(api, "pollCodexLogin").mockImplementation(() => new Promise(r => { authorize = r; }));
  const cancel = vi.spyOn(api, "cancelCodexLogin").mockResolvedValue({ login_id: "login", state: "cancelled", expires_at: challenge.expires_at, error_code: null });
  const finish = vi.spyOn(api, "finishCodexLogin");
  render(<SettingsPanel locale="en-US" onClose={vi.fn()} />);
  fireEvent.click(screen.getByRole("button", { name: "Configure Codex subscription" }));
  fireEvent.click(screen.getByRole("button", { name: "Sign in to Codex" }));
  await screen.findByText("TEST-CODE");
  fireEvent.keyDown(window, { key: "Escape" });
  await act(async () => authorize({ login_id: "login", state: "authorized", expires_at: challenge.expires_at, error_code: null }));
  expect(cancel).toHaveBeenCalledTimes(1); expect(finish).not.toHaveBeenCalled();
  expect(screen.queryByRole("region", { name: "Subscription model configuration" })).not.toBeInTheDocument();
  expect(screen.getByRole("dialog", { name: "Workspace settings" })).toBeVisible();
});
it("disconnects only the selected Codex profile and preserves ordinary edit omission", async () => {
  vi.spyOn(api, "subscriptionModelStatus").mockResolvedValue({ provider: "open_ai_codex", authenticated: true, masked_account_label: "account-12345678", cli_version: null, error_code: null });
  const disconnected = { ...profile, subscription_account_ref: null };
  const disconnect = vi.spyOn(api, "disconnectCodex").mockResolvedValue(disconnected);
  const saved = vi.fn();
  render(<SubscriptionModelForm provider="open_ai_codex" profile={profile} onSaved={saved} onCancel={vi.fn()} zh={false} />);
  expect(await screen.findByText(/account-12345678/)).toBeVisible();
  fireEvent.click(screen.getByRole("button", { name: "Disconnect this subscription profile" }));
  await waitFor(() => expect(saved).toHaveBeenCalledWith(disconnected));
  expect(disconnect).toHaveBeenCalledWith("saved");
});
it("keeps a submitted profile mutation visible until the native result arrives", async () => {
  vi.spyOn(api, "subscriptionModelStatus").mockResolvedValue({ provider: "open_ai_codex", authenticated: true, masked_account_label: null, cli_version: null, error_code: null });
  let finish!: (value: ModelProfile) => void;
  const save = vi.spyOn(api, "saveModelProfile").mockImplementation(() => new Promise(r => { finish = r; }));
  const cancel = vi.fn(); const saved = vi.fn();
  render(<SubscriptionModelForm provider="open_ai_codex" profile={profile} onSaved={saved} onCancel={cancel} zh={false} />);
  fireEvent.click(screen.getByRole("button", { name: "Save profile" }));
  fireEvent.keyDown(window, { key: "Escape" });
  expect(cancel).not.toHaveBeenCalled(); expect(screen.getByRole("button", { name: "Cancel" })).toBeDisabled();
  const payload=save.mock.calls[0][0];
  expect(payload).not.toHaveProperty("context_window_tokens"); expect(payload).not.toHaveProperty("refresh_catalog"); expect(payload).not.toHaveProperty("reasoning_effort"); expect(payload).not.toHaveProperty("delegated_model_profile_id"); expect(payload).not.toHaveProperty("subscription_account_ref");
  await act(async () => finish(profile)); expect(saved).toHaveBeenCalledWith(profile);
});

it.each([
  ["codex_device_region_unsupported", /network region.*supported.*Windows system proxy/i],
  ["codex_auth_connection_failed", /internet connection.*Windows system proxy/i],
  ["codex_auth_network_uncertain", /internet connection.*Windows system proxy/i],
  ["codex_auth_timeout", /timed out.*retry/i],
  ["codex_device_web_verification_required", /web access or verification page.*Windows system proxy/i],
  ["codex_device_access_denied", /denied.*device.*sign-in/i],
  ["codex_device_rate_limited", /too many sign-in requests.*later/i],
  ["codex_device_login_unavailable", /sign-in.*unavailable.*later/i],
  ["codex_auth_response_invalid", /unexpected sign-in response.*retry/i],
  ["codex_auth_field_invalid", /unexpected sign-in response.*retry/i],
  ["codex_device_interval_invalid", /unexpected sign-in response.*retry/i],
  ["codex_token_exchange_failed", /finish authorization.*sign in again/i],
  ["codex_reauthentication_required", /sign in again/i],
])("shows actionable Codex begin guidance for exact safe code %s", async (code, message) => {
  vi.spyOn(api, "beginCodexLogin").mockRejectedValue(code);
  render(<SubscriptionModelForm provider="open_ai_codex" profile={null} onSaved={vi.fn()} onCancel={vi.fn()} zh={false} />);
  fireEvent.click(screen.getByRole("button", { name: "Sign in to Codex" }));
  expect(await screen.findByRole("alert")).toHaveTextContent(message);
  expect(screen.getByRole("button", { name: "Sign in to Codex" })).toBeEnabled();
  fireEvent.change(screen.getByLabelText("Model"), { target: { value: "full-id" } });
  expect(screen.getByRole("alert")).toHaveTextContent(message);
});

it("localizes safe Error.message sign-in failures and clears them on an explicit retry", async () => {
  const begin = vi.spyOn(api, "beginCodexLogin").mockRejectedValueOnce(new Error("codex_device_region_unsupported")).mockResolvedValueOnce(challenge);
  vi.spyOn(api, "pollCodexLogin").mockResolvedValue({ login_id: "login", state: "authorized", expires_at: challenge.expires_at, error_code: null });
  vi.spyOn(api, "cancelCodexLogin").mockResolvedValue({ login_id: "login", state: "cancelled", expires_at: challenge.expires_at, error_code: null });
  render(<SubscriptionModelForm provider="open_ai_codex" profile={null} onSaved={vi.fn()} onCancel={vi.fn()} zh />);
  fireEvent.click(screen.getByRole("button", { name: "登录 Codex 订阅" }));
  expect(await screen.findByRole("alert")).toHaveTextContent(/网络地区.*支持.*Windows 系统代理/);
  fireEvent.click(screen.getByRole("button", { name: "登录 Codex 订阅" }));
  expect(await screen.findByText("TEST-CODE")).toBeVisible();
  expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  expect(begin).toHaveBeenCalledTimes(2);
});

it.each([
  "codex_device_region_unsupported SECRET_AUTH_BODY_SENTINEL",
  new Error("SECRET_AUTH_BODY_SENTINEL"),
  { code: "codex_device_region_unsupported", body: "SECRET_AUTH_BODY_SENTINEL" },
])("never renders unknown or payload-bearing sign-in errors %#", async (failure) => {
  vi.spyOn(api, "beginCodexLogin").mockRejectedValue(failure);
  render(<SubscriptionModelForm provider="open_ai_codex" profile={null} onSaved={vi.fn()} onCancel={vi.fn()} zh={false} />);
  fireEvent.click(screen.getByRole("button", { name: "Sign in to Codex" }));
  expect(await screen.findByRole("alert")).toHaveTextContent("Operation failed. Check sign-in or configuration and retry.");
  expect(document.body).not.toHaveTextContent("SECRET_AUTH_BODY_SENTINEL");
  expect(document.body).not.toHaveTextContent("network region");
});

it.each([
  ["failed", "codex_device_access_denied", /denied.*device.*sign-in/i],
  ["expired", null, /sign-in expired.*sign in again/i],
  ["failed", "SECRET_AUTH_BODY_SENTINEL", /operation failed/i],
] as const)("renders terminal Codex polling state %s and safe guidance", async (state, code, message) => {
  vi.spyOn(api, "beginCodexLogin").mockResolvedValue(challenge);
  vi.spyOn(api, "pollCodexLogin").mockResolvedValue({ login_id: "login", state, expires_at: challenge.expires_at, error_code: code });
  vi.spyOn(api, "cancelCodexLogin").mockResolvedValue({ login_id: "login", state: "cancelled", expires_at: challenge.expires_at, error_code: null });
  render(<SubscriptionModelForm provider="open_ai_codex" profile={null} onSaved={vi.fn()} onCancel={vi.fn()} zh={false} />);
  fireEvent.click(screen.getByRole("button", { name: "Sign in to Codex" }));
  expect(await screen.findByRole("alert")).toHaveTextContent(message);
  expect(screen.getByRole("button", { name: "Save profile" })).toBeDisabled();
  expect(document.body).not.toHaveTextContent("SECRET_AUTH_BODY_SENTINEL");
});

it("handles safe poll rejections without losing sign-in retry", async () => {
  vi.spyOn(api, "beginCodexLogin").mockResolvedValue(challenge);
  vi.spyOn(api, "pollCodexLogin").mockRejectedValue(new Error("codex_auth_timeout"));
  vi.spyOn(api, "cancelCodexLogin").mockResolvedValue({ login_id: "login", state: "cancelled", expires_at: challenge.expires_at, error_code: null });
  render(<SubscriptionModelForm provider="open_ai_codex" profile={null} onSaved={vi.fn()} onCancel={vi.fn()} zh={false} />);
  fireEvent.click(screen.getByRole("button", { name: "Sign in to Codex" }));
  expect(await screen.findByRole("alert")).toHaveTextContent(/timed out.*retry/i);
  expect(screen.getByRole("button", { name: "Sign in to Codex" })).toBeEnabled();
});

it("keeps profile save failures generic even for a recognizable sign-in code", async () => {
  vi.spyOn(api, "subscriptionModelStatus").mockResolvedValue({ provider: "open_ai_codex", authenticated: true, masked_account_label: null, cli_version: null, error_code: null });
  vi.spyOn(api, "saveModelProfile").mockRejectedValue(new Error("codex_device_region_unsupported"));
  render(<SubscriptionModelForm provider="open_ai_codex" profile={profile} onSaved={vi.fn()} onCancel={vi.fn()} zh={false} />);
  fireEvent.click(screen.getByRole("button", { name: "Save profile" }));
  expect(await screen.findByRole("alert")).toHaveTextContent("Operation failed. Check sign-in or configuration and retry.");
});
