import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { SettingsPanel } from "./SettingsPanel";
import { setComposerSendPreference } from "./useComposerSendPreference";

afterEach(() => {
  vi.restoreAllMocks();
  setComposerSendPreference(false);
});

describe("SettingsPanel model providers", () => {
  it("saves the DeepSeek preset through the compatible provider and supports custom model IDs", async () => {
    const save = vi.fn().mockResolvedValue(undefined);
    render(<SettingsPanel locale="zh-CN" onClose={() => undefined} onSaveModel={save} />);
    fireEvent.click(screen.getByRole("button", { name: "Configure DeepSeek" }));
    expect(screen.getByLabelText("Profile label")).toHaveValue("DeepSeek");
    expect(screen.getByLabelText("Base URL")).toHaveValue("https://api.deepseek.com/v1");
    expect(screen.getByLabelText("Model")).toHaveValue("deepseek-v4-flash");
    expect(screen.getByLabelText("Model")).toHaveAttribute("list", "deepseek-models");
    expect(document.querySelector('#deepseek-models option[value="deepseek-v4-pro"]')).toBeInTheDocument();
    fireEvent.change(screen.getByLabelText("Model"), { target: { value: "custom-deepseek-model" } });
    fireEvent.change(screen.getByLabelText("API key"), { target: { value: "test-only-key" } });
    fireEvent.click(screen.getByRole("button", { name: "保存提供方" }));
    expect(save).toHaveBeenCalledWith({ provider: "open_ai_compatible", label: "DeepSeek", base_url: "https://api.deepseek.com/v1", model: "custom-deepseek-model", credential: "test-only-key" });
    await waitFor(() => expect(screen.queryByLabelText("API key")).not.toBeInTheDocument());
  });

  it.each(["https://api.deepseek.com", "https://api.deepseek.com/v1/", "https://api.deepseek.com:443/v1"])("recognizes saved official DeepSeek profiles at %s and edits without exposing credentials", (base_url) => {
    const profile = { id: "deepseek", label: "Lab model", provider: "open_ai_compatible" as const, base_url, model: "deepseek-v4-pro", credential_reference: "model/deepseek", supports_tools: true, supports_vision: false };
    render(<SettingsPanel locale="en-US" onClose={() => undefined} modelProfiles={[profile]} />);
    const card = screen.getByRole("button", { name: "Configure DeepSeek" }).closest("article")!;
    expect(within(card).getByText(/configured/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Edit" }));
    expect(screen.getByLabelText("Model")).toHaveValue("deepseek-v4-pro");
    expect(screen.getByLabelText("API key")).toHaveValue("");
  });

  it.each(["https://api.openai.com/v1", "https://api.deepseek.com.example.org/v1", "http://api.deepseek.com/v1", "https://api.deepseek.com:8443/v1", "invalid"])("does not mark DeepSeek configured for %s", (base_url) => {
    const profile = { id: "other", label: "DeepSeek", provider: "open_ai_compatible" as const, base_url, model: "deepseek-v4-flash", credential_reference: null, supports_tools: true, supports_vision: false };
    render(<SettingsPanel locale="en-US" onClose={() => undefined} modelProfiles={[profile]} />);
    const card = screen.getByRole("button", { name: "Configure DeepSeek" }).closest("article")!;
    expect(within(card).queryByText(/configured/)).not.toBeInTheDocument();
  });

  it("closes only the DeepSeek form on immediate Escape and keeps generic defaults separate", () => {
    const close = vi.fn();
    render(<SettingsPanel locale="en-US" onClose={close} />);
    fireEvent.click(screen.getByRole("button", { name: "Configure DeepSeek" }));
    fireEvent.keyDown(window, { key: "Escape" });
    expect(screen.queryByLabelText("Model")).not.toBeInTheDocument();
    expect(screen.getByRole("dialog")).toBeInTheDocument();
    expect(close).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Configure OpenAI-compatible" }));
    expect(screen.getByLabelText("Base URL")).toHaveValue("https://api.openai.com/");
    expect(screen.getByLabelText("Model")).toHaveValue("");
    expect(screen.getByLabelText("Model")).not.toHaveAttribute("list");
  });

  it("refreshes only when explicitly selected and resets selection when editing again", async () => {
    const profile = { id: "known", label: "Known", provider: "open_ai_compatible" as const, base_url: "https://api.openai.com/v1", model: "gpt-4o", credential_reference: null, supports_tools: true, supports_vision: true };
    const save = vi.fn().mockResolvedValue(undefined);
    const close = vi.fn();
    render(<SettingsPanel locale="en-US" onClose={close} modelProfiles={[profile]} onSaveModel={save} />);
    fireEvent.click(screen.getByRole("button", { name: "Edit" }));
    expect(screen.getByLabelText("Refresh catalog capabilities")).not.toBeChecked();
    fireEvent.click(screen.getByLabelText("Refresh catalog capabilities"));
    expect(screen.getByText(/may prevent old runs from resuming/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Save provider" }));
    await waitFor(() => expect(save).toHaveBeenCalledWith(expect.objectContaining({ refresh_catalog: true })));
    await waitFor(() => expect(screen.queryByLabelText("Refresh catalog capabilities")).not.toBeInTheDocument());
    fireEvent.click(screen.getByRole("button", { name: "Edit" }));
    expect(screen.getByLabelText("Refresh catalog capabilities")).not.toBeChecked();
    fireEvent.keyDown(window, { key: "Escape" });
    expect(screen.queryByLabelText("Refresh catalog capabilities")).not.toBeInTheDocument();
    expect(close).not.toHaveBeenCalled();
    expect(screen.getByRole("dialog")).toBeInTheDocument();
  });

  it("shows a saved context budget but omits it until the user enters a non-empty replacement", async () => {
    const profile = { id: "budget", label: "Budget", provider: "open_ai_compatible" as const, base_url: "https://api.openai.com/v1", model: "gpt-5.6-sol", credential_reference: null, supports_tools: true, supports_vision: false, context_window_tokens: 32000 };
    const save = vi.fn().mockResolvedValue(undefined);
    render(<SettingsPanel locale="en-US" onClose={() => undefined} modelProfiles={[profile]} onSaveModel={save} />);

    fireEvent.click(screen.getByRole("button", { name: "Edit" }));
    expect(screen.getByLabelText("Configured context budget")).toHaveValue("32000");
    expect(screen.getByText(/leave it blank or unchanged to keep its budget/i)).toBeInTheDocument();
    fireEvent.click(screen.getByLabelText("Refresh catalog capabilities"));
    fireEvent.click(screen.getByRole("button", { name: "Save provider" }));
    await waitFor(() => expect(save).toHaveBeenCalledOnce());
    expect(save.mock.calls[0][0]).not.toHaveProperty("context_window_tokens");

    fireEvent.click(screen.getByRole("button", { name: "Edit" }));
    fireEvent.change(screen.getByLabelText("Configured context budget"), { target: { value: "" } });
    fireEvent.click(screen.getByRole("button", { name: "Save provider" }));
    await waitFor(() => expect(save).toHaveBeenCalledTimes(2));
    expect(save.mock.calls[1][0]).not.toHaveProperty("context_window_tokens");
  });

  it("submits only valid positive u32 context budgets", async () => {
    const save = vi.fn().mockResolvedValue(undefined);
    render(<SettingsPanel locale="en-US" onClose={() => undefined} onSaveModel={save} />);
    fireEvent.click(screen.getByRole("button", { name: "Configure OpenAI-compatible" }));
    fireEvent.change(screen.getByLabelText("Model"), { target: { value: "custom-model" } });
    const budget = screen.getByLabelText("Configured context budget");

    for (const invalid of ["0", "-1", "1.5", "4294967296", "abc"]) {
      fireEvent.change(budget, { target: { value: invalid } });
      expect(screen.getByRole("button", { name: "Save provider" })).toBeDisabled();
      expect(screen.getByRole("alert")).toHaveTextContent("whole number from 1 to 4294967295");
    }

    fireEvent.change(budget, { target: { value: "4294967295" } });
    expect(screen.getByRole("button", { name: "Save provider" })).toBeEnabled();
    fireEvent.click(screen.getByRole("button", { name: "Save provider" }));
    await waitFor(() => expect(save).toHaveBeenCalledWith(expect.objectContaining({ context_window_tokens: 4294967295 })));
  });

  it("clears the context budget draft when the model identity changes", () => {
    const profile = { id: "budget", label: "Budget", provider: "open_ai_compatible" as const, base_url: "https://api.openai.com/v1", model: "gpt-5.6-sol", credential_reference: null, supports_tools: true, supports_vision: false, context_window_tokens: 32000 };
    render(<SettingsPanel locale="en-US" onClose={() => undefined} modelProfiles={[profile]} />);
    fireEvent.click(screen.getByRole("button", { name: "Edit" }));
    const budget = screen.getByLabelText("Configured context budget");
    expect(budget).toHaveValue("32000");
    fireEvent.change(screen.getByLabelText("Base URL"), { target: { value: "https://gateway.example/v1" } });
    expect(budget).toHaveValue("");
    fireEvent.change(budget, { target: { value: "64000" } });
    fireEvent.change(screen.getByLabelText("Model"), { target: { value: "other-model" } });
    expect(budget).toHaveValue("");
  });

  it("round trips a profile Fast mode override and lets the user restore model inheritance", async () => {
    const profile = { id: "fast", label: "Fast profile", provider: "open_ai_compatible" as const, base_url: "https://api.openai.com/v1", model: "gpt-5.6-luna", credential_reference: "model/fast", supports_tools: true, supports_vision: false, fast_mode: true as const };
    const save = vi.fn().mockResolvedValue(undefined);
    render(<SettingsPanel locale="en-US" onClose={() => undefined} modelProfiles={[profile]} onSaveModel={save} />);

    fireEvent.click(screen.getByRole("button", { name: "Edit" }));
    const fastMode = screen.getByRole("combobox", { name: "Fast mode" });
    expect(fastMode).toHaveValue("fast");
    fireEvent.change(fastMode, { target: { value: "default" } });
    fireEvent.click(screen.getByRole("button", { name: "Save provider" }));
    await waitFor(() => expect(save).toHaveBeenCalledWith(expect.objectContaining({ id: "fast", fast_mode: null })));
  });

  it("keeps Fast unavailable for unreviewed profile endpoints while allowing standard and default", () => {
    render(<SettingsPanel locale="en-US" onClose={() => undefined} />);
    fireEvent.click(screen.getByRole("button", { name: "Configure OpenAI-compatible" }));
    const fastMode = screen.getByRole("combobox", { name: "Fast mode" });
    expect(within(fastMode).getByRole("option", { name: "Fast" })).toBeDisabled();
    expect(within(fastMode).getByRole("option", { name: "Standard" })).toBeEnabled();
    expect(within(fastMode).getByRole("option", { name: "Model default" })).toBeEnabled();
  });

  it("keeps refresh intent available for retry after a failed save", async () => {
    const profile = { id: "unknown", label: "Unknown", provider: "open_ai_compatible" as const, base_url: "https://gateway.example/v1", model: "exact", credential_reference: null, supports_tools: true, supports_vision: false };
    const save = vi.fn().mockRejectedValueOnce(new Error("private diagnostic")).mockResolvedValue(undefined);
    render(<SettingsPanel locale="en-US" onClose={() => undefined} modelProfiles={[profile]} onSaveModel={save} />);
    fireEvent.click(screen.getByRole("button", { name: "Edit" }));
    fireEvent.click(screen.getByLabelText("Refresh catalog capabilities"));
    fireEvent.click(screen.getByRole("button", { name: "Save provider" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Could not save");
    expect(screen.getByLabelText("Refresh catalog capabilities")).toBeChecked();
    expect(screen.queryByText("private diagnostic")).not.toBeInTheDocument();
    fireEvent.click(screen.getByLabelText("Refresh catalog capabilities"));
    fireEvent.click(screen.getByRole("button", { name: "Save provider" }));
    await waitFor(() => expect(save).toHaveBeenLastCalledWith(expect.objectContaining({ refresh_catalog: false })));
  });

  it("shows saved catalog limits without inventing metadata for legacy profiles", () => {
    const legacy = { id: "legacy", label: "Legacy", provider: "open_ai_compatible" as const, base_url: "https://gateway.example/v1", model: "exact-model", credential_reference: null, supports_tools: true, supports_vision: false };
    const known = { ...legacy, id: "known", label: "Known", catalog_capabilities: { source_provider: "openai", source_sha256: "a".repeat(64), context_limit: 128000, input_limit: null, output_limit: 16384, reasoning: false, reasoning_efforts: null } };
    render(<SettingsPanel locale="en-US" onClose={() => undefined} modelProfiles={[legacy, known]} />);
    expect(screen.getAllByText(/Catalog snapshot/)).toHaveLength(1);
    expect(screen.getByText(/Catalog snapshot/)).toHaveTextContent("Context limit 128000 · Output limit 16384");
  });

  it("edits and explicitly clears requested effort without changing the child binding", async () => {
    const profile = { id: "main", label: "Main", provider: "open_ai_compatible" as const, base_url: "https://gateway.example/v1", model: "exact-model", credential_reference: null, supports_tools: true, supports_vision: false, reasoning_effort: "max" as const, delegated_model_profile_id: "child" };
    const child = { ...profile, id: "child", label: "Reader", reasoning_effort: "low" as const, delegated_model_profile_id: null };
    const save = vi.fn().mockResolvedValue(undefined);
    render(<SettingsPanel locale="en-US" onClose={() => undefined} modelProfiles={[profile, child]} onSaveModel={save} />);
    fireEvent.click(screen.getAllByRole("button", { name: "Edit" })[0]);
    expect(screen.getByLabelText("Requested reasoning effort")).toHaveValue("max");
    fireEvent.change(screen.getByLabelText("Requested reasoning effort"), { target: { value: "high" } });
    fireEvent.click(screen.getByRole("button", { name: "Save provider" }));
    await waitFor(() => expect(screen.queryByLabelText("Requested reasoning effort")).not.toBeInTheDocument());
    expect(save).toHaveBeenLastCalledWith(expect.objectContaining({ id: "main", reasoning_effort: "high", delegated_model_profile_id: "child" }));
    fireEvent.click(screen.getAllByRole("button", { name: "Edit" })[0]);
    fireEvent.change(screen.getByLabelText("Requested reasoning effort"), { target: { value: "" } });
    fireEvent.click(screen.getByRole("button", { name: "Save provider" }));
    await waitFor(() => expect(screen.queryByLabelText("Requested reasoning effort")).not.toBeInTheDocument());
    expect(save).toHaveBeenLastCalledWith(expect.objectContaining({ reasoning_effort: null }));
    fireEvent.click(screen.getAllByRole("button", { name: "Edit" })[1]);
    expect(screen.getByLabelText("Requested reasoning effort")).toHaveValue("low");
  });

  it("saves a selected child profile, clears it explicitly, and closes only the form on Escape", async () => {
    const child = { id: "child", label: "Reader", provider: "ollama" as const, base_url: "http://127.0.0.1:11434", model: "reader-model", credential_reference: null, supports_tools: true, supports_vision: false };
    const main = { ...child, id: "main", label: "Main", delegated_model_profile_id: "child" };
    const save = vi.fn().mockResolvedValue(undefined);
    const close = vi.fn();
    render(<SettingsPanel locale="en-US" onClose={close} modelProfiles={[main, child]} onSaveModel={save} />);
    fireEvent.click(screen.getAllByRole("button", { name: "Edit" })[0]);
    fireEvent.keyDown(window, { key: "Escape" });
    expect(screen.queryByLabelText("Read-only subagent model")).not.toBeInTheDocument();
    expect(close).not.toHaveBeenCalled();
    expect(screen.getByRole("dialog")).toBeInTheDocument();
    fireEvent.click(screen.getAllByRole("button", { name: "Edit" })[0]);
    expect(screen.getByLabelText("Read-only subagent model")).toHaveValue("child");
    expect(screen.queryByRole("option", { name: /Main/ })).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Save provider" }));
    await waitFor(() => expect(screen.queryByLabelText("Read-only subagent model")).not.toBeInTheDocument());
    expect(save).toHaveBeenLastCalledWith(expect.objectContaining({ delegated_model_profile_id: "child" }));
    fireEvent.click(screen.getAllByRole("button", { name: "Edit" })[0]);
    fireEvent.change(screen.getByLabelText("Read-only subagent model"), { target: { value: "" } });
    fireEvent.click(screen.getByRole("button", { name: "Save provider" }));
    await waitFor(() => expect(save).toHaveBeenLastCalledWith(expect.objectContaining({ delegated_model_profile_id: null })));
    await waitFor(() => expect(screen.queryByLabelText("Read-only subagent model")).not.toBeInTheDocument());
    fireEvent.keyDown(window, { key: "Escape" });
    expect(close).toHaveBeenCalledOnce();
  });

  it("keeps a failed role selection editable without showing raw transport details", async () => {
    render(<SettingsPanel locale="en-US" onClose={() => undefined} onSaveModel={vi.fn().mockRejectedValue(new Error("secret transport"))} />);
    fireEvent.click(screen.getByRole("button", { name: "Configure Ollama" }));
    fireEvent.change(screen.getByLabelText("Model"), { target: { value: "reader" } });
    fireEvent.click(screen.getByRole("button", { name: "Save provider" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Could not save");
    expect(screen.queryByText("secret transport")).not.toBeInTheDocument();
    expect(screen.getByLabelText("Model")).toHaveValue("reader");
  });
  it("opens directly on Skills when launched from the composer", () => {
    render(<SettingsPanel locale="en-US" initialSection="skills" onClose={() => undefined} />);
    expect(screen.getByRole("button", { name: "Skills and MCP" })).toHaveClass("active");
    expect(screen.getByRole("button", { name: "Skills and MCP" })).toHaveAttribute("aria-current", "page");
    expect(screen.getByText("Research Skills")).toBeInTheDocument();
  });

  it("explains privacy boundaries and links to the existing capability settings without a project", () => {
    render(<SettingsPanel locale="zh-CN" onClose={() => undefined} selectedProject={null} />);

    fireEvent.click(screen.getByRole("button", { name: "隐私与权限" }));
    expect(screen.getByRole("button", { name: "隐私与权限" })).toHaveAttribute("aria-current", "page");
    expect(screen.getByRole("heading", { name: "隐私与权限" })).toBeInTheDocument();
    expect(screen.getByText(/模型、MCP 或外部服务可能接收提示词、结果或元数据/)).toBeInTheDocument();
    expect(screen.getByText(/Windows Credential Manager 或系统 keyring/)).toBeInTheDocument();
    expect(screen.getByText(/项目导出可能包含你选择导出的正文、记忆和产物/)).toBeInTheDocument();
    expect(screen.queryByText(/项目导出只保存引用或脱敏信息/)).not.toBeInTheDocument();
    expect(screen.getByText(/停止 Agent 不会取消已经派发的远端计算/)).toBeInTheDocument();
    expect(screen.getByText(/当前未打开项目/)).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "管理技能与 MCP" }));
    expect(screen.getByRole("button", { name: "技能与 MCP" })).toHaveAttribute("aria-current", "page");
    fireEvent.click(screen.getByRole("button", { name: "隐私与权限" }));
    fireEvent.click(screen.getByRole("button", { name: "管理浏览器" }));
    expect(screen.getByRole("button", { name: "浏览器" })).toHaveAttribute("aria-current", "page");
  });

  it("does not let a hidden model form consume Escape after navigating to privacy", () => {
    const close = vi.fn();
    render(<SettingsPanel locale="en-US" onClose={close} />);
    fireEvent.click(screen.getByRole("button", { name: "Configure DeepSeek" }));
    fireEvent.click(screen.getByRole("button", { name: "Privacy and permissions" }));

    expect(screen.queryByLabelText("Model configuration")).not.toBeInTheDocument();
    fireEvent.keyDown(window, { key: "Escape" });
    expect(close).toHaveBeenCalledOnce();
  });

  it("uses the localized Session navigation label and removes unsupported Ollama policy copy", () => {
    render(<SettingsPanel locale="zh-CN" onClose={() => undefined} />);
    expect(screen.getByRole("button", { name: "会话" })).toBeInTheDocument();
    expect(screen.queryByText(/项目强制策略/)).not.toBeInTheDocument();
    expect(screen.getByText(/模型与外部服务可能接收提示词、结果或元数据/)).toBeInTheDocument();
    expect(screen.queryByText(/诊断包仅在主动导出时生成/)).not.toBeInTheDocument();
  });

  it("changes the interface language immediately from General without a save action", () => {
    const onLocaleChange = vi.fn();
    render(<SettingsPanel locale="zh-CN" initialSection="general" onLocaleChange={onLocaleChange} onClose={() => undefined} />);

    expect(screen.getByRole("button", { name: "常规" })).toHaveAttribute("aria-current", "page");
    fireEvent.change(screen.getByRole("combobox", { name: "界面语言" }), { target: { value: "en-US" } });
    expect(onLocaleChange).toHaveBeenCalledWith("en-US");
    expect(screen.queryByRole("button", { name: /保存/ })).not.toBeInTheDocument();
  });

  it("presents the implemented settings in scrollable Wisp groups with a visible return action", () => {
    render(<SettingsPanel locale="en-US" onClose={() => undefined} />);

    expect(screen.getByRole("button", { name: "Close" })).toHaveTextContent("Back to workspace");
    const navigation = screen.getByRole("navigation", { name: "Settings pages" });
    expect(within(navigation).getByText("Workspace")).toBeInTheDocument();
    expect(within(navigation).getByText("Capabilities")).toBeInTheDocument();
    expect(within(navigation).getByRole("button", { name: "General" })).toBeInTheDocument();
    expect(within(navigation).getByRole("button", { name: "Skills and MCP" })).toBeInTheDocument();
    expect(within(navigation).queryByRole("button", { name: "Pet" })).not.toBeInTheDocument();
  });

  it("searches navigation labels in either language without changing or clearing the active page", () => {
    render(<SettingsPanel locale="en-US" onClose={() => undefined} />);
    fireEvent.click(screen.getByRole("button", { name: "Configure DeepSeek" }));
    fireEvent.change(screen.getByLabelText("Profile label"), { target: { value: "My DeepSeek" } });

    const search = screen.getByRole("searchbox", { name: "Search settings" });
    fireEvent.change(search, { target: { value: "技能" } });
    const navigation = screen.getByRole("navigation", { name: "Settings pages" });
    expect(within(navigation).getByRole("button", { name: "Skills and MCP" })).toBeInTheDocument();
    expect(within(navigation).queryByRole("button", { name: "General" })).not.toBeInTheDocument();
    expect(screen.getByLabelText("Profile label")).toHaveValue("My DeepSeek");

    fireEvent.change(search, { target: { value: "no-such-setting" } });
    expect(within(navigation).getByRole("status")).toHaveTextContent("No matching settings");
    expect(screen.getByLabelText("Profile label")).toHaveValue("My DeepSeek");
    expect(screen.getByRole("heading", { name: "Model providers" })).toBeInTheDocument();
  });

  it("changes the main composer send shortcut immediately and keeps the legacy storage key", () => {
    render(<SettingsPanel locale="en-US" initialSection="general" onClose={() => undefined} />);

    const shortcut = screen.getByRole("combobox", { name: "Send shortcut" });
    expect(shortcut).toHaveValue("enter");
    fireEvent.change(shortcut, { target: { value: "modifier" } });
    expect(shortcut).toHaveValue("modifier");
    expect(window.localStorage.getItem("omicsops.composer.modifierSend")).toBe("true");
    expect(screen.getByText(/main composer/i)).toBeInTheDocument();
  });

  it("keeps the send shortcut usable for this session when browser storage is unavailable", () => {
    vi.spyOn(Storage.prototype, "getItem").mockImplementation(() => { throw new Error("storage blocked"); });
    vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => { throw new Error("storage blocked"); });
    render(<SettingsPanel locale="en-US" initialSection="general" onClose={() => undefined} />);

    const shortcut = screen.getByRole("combobox", { name: "Send shortcut" });
    fireEvent.change(shortcut, { target: { value: "modifier" } });
    expect(shortcut).toHaveValue("modifier");
  });

  it("keeps the in-memory send shortcut when reading works but persistence fails", () => {
    window.localStorage.setItem("omicsops.composer.modifierSend", "false");
    vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => { throw new Error("quota exceeded"); });
    render(<SettingsPanel locale="en-US" initialSection="general" onClose={() => undefined} />);

    const shortcut = screen.getByRole("combobox", { name: "Send shortcut" });
    fireEvent.change(shortcut, { target: { value: "modifier" } });
    expect(shortcut).toHaveValue("modifier");
  });

  it("defaults to Chinese and disables General language controls without a change handler", () => {
    render(<SettingsPanel initialSection="general" onClose={() => undefined} />);

    expect(screen.getByRole("dialog", { name: "工作台设置" })).toBeInTheDocument();
    expect(screen.getByRole("combobox", { name: "界面语言" })).toBeDisabled();
  });
  it("collects provider configuration without rendering stored credentials", async () => {
    const onSaveModel = vi.fn().mockResolvedValue(undefined);
    render(<SettingsPanel locale="en-US" onClose={() => undefined} modelProfiles={[]} onSaveModel={onSaveModel} />);
    fireEvent.click(screen.getByRole("button", { name: "Configure Anthropic" }));
    fireEvent.change(screen.getByLabelText("Profile label"), { target: { value: "Lab Claude" } });
    fireEvent.change(screen.getByLabelText("Model"), { target: { value: "claude-science" } });
    fireEvent.change(screen.getByLabelText("API key"), { target: { value: "secret" } });
    fireEvent.click(screen.getByRole("button", { name: "Save provider" }));
    expect(onSaveModel).toHaveBeenCalledWith(expect.objectContaining({ provider: "anthropic", label: "Lab Claude", model: "claude-science", credential: "secret" }));
    await waitFor(() => expect(screen.queryByDisplayValue("secret")).not.toBeInTheDocument());
  });

  it("shows actionable model test failures and successful endpoint diagnostics", async () => {
    const profile = { id: "model-1", label: "Lab gateway", provider: "open_ai_compatible" as const, base_url: "https://models.example/v1", model: "science-model", credential_reference: "model/model-1", supports_tools: true, supports_vision: false };
    const onProbeModel = vi.fn()
      .mockRejectedValueOnce(new Error("401 Unauthorized: invalid API key"))
      .mockResolvedValueOnce({ endpoint: "https://models.example/v1/chat/completions", protocol: "OpenAiCompatible", model: "science-model", latency_ms: 42, response_preview: "OK" });
    render(<SettingsPanel locale="zh-CN" onClose={() => undefined} modelProfiles={[profile]} onProbeModel={onProbeModel} />);

    fireEvent.click(screen.getByRole("button", { name: "测试" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("401 Unauthorized");
    fireEvent.click(screen.getByRole("button", { name: "测试" }));
    expect(await screen.findByRole("status")).toHaveTextContent("连接成功");
    expect(screen.getByRole("status")).toHaveTextContent("/v1/chat/completions");
    expect(screen.getByRole("status")).toHaveTextContent("42 ms");
  });

  it("discovers gateway models and opens the selected model for editing", async () => {
    const profile = { id: "model-1", label: "Lab gateway", provider: "open_ai_compatible" as const, base_url: "https://models.example/v1", model: "missing-model", credential_reference: "model/model-1", supports_tools: true, supports_vision: false };
    const onListModels = vi.fn().mockResolvedValue(["gpt-5.6-sol", "gpt-5.6-terra"]);
    render(<SettingsPanel locale="en-US" onClose={() => undefined} modelProfiles={[profile]} onListModels={onListModels} />);

    fireEvent.click(screen.getByRole("button", { name: "Models" }));
    expect(await screen.findByRole("button", { name: "gpt-5.6-terra" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "gpt-5.6-terra" }));

    expect(onListModels).toHaveBeenCalledWith("model-1");
    expect(screen.getByLabelText("Model")).toHaveValue("gpt-5.6-terra");
    expect(screen.getByLabelText("Base URL")).toHaveValue("https://models.example/v1");
    expect(screen.getByLabelText("API key")).toHaveValue("");
  });

  it("keeps model discovery feedback independent from probe state and supports a sanitized retry", async () => {
    const profile = { id: "model-1", label: "Lab gateway", provider: "open_ai_compatible" as const, base_url: "https://models.example/v1", model: "science-model", credential_reference: "model/model-1", supports_tools: true, supports_vision: false };
    let rejectDiscovery!: (error: Error) => void;
    const firstDiscovery = new Promise<string[]>((_resolve, reject) => { rejectDiscovery = reject; });
    const onListModels = vi.fn().mockReturnValueOnce(firstDiscovery).mockResolvedValueOnce([]);
    const onProbeModel = vi.fn().mockResolvedValue({ endpoint: "https://models.example/v1/chat/completions", protocol: "OpenAiCompatible", model: "science-model", latency_ms: 42, response_preview: "OK" });
    render(<SettingsPanel locale="en-US" onClose={() => undefined} modelProfiles={[profile]} onListModels={onListModels} onProbeModel={onProbeModel} />);

    fireEvent.click(screen.getByRole("button", { name: "Models" }));
    expect(screen.getByRole("button", { name: "Loading models" })).toBeDisabled();
    fireEvent.click(screen.getByRole("button", { name: "Loading models" }));
    expect(onListModels).toHaveBeenCalledOnce();
    fireEvent.click(screen.getByRole("button", { name: "Test" }));
    expect(await screen.findByText("Connection succeeded")).toBeInTheDocument();

    rejectDiscovery(new Error("401 credential=secret-value"));
    expect(await screen.findByText("Could not list models. Retry the query.")).toBeInTheDocument();
    expect(screen.queryByText(/secret-value/)).not.toBeInTheDocument();
    expect(screen.getByText("Connection succeeded")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Retry models" }));
    expect(await screen.findByText("The provider returned no models.")).toBeInTheDocument();
    expect(onListModels).toHaveBeenCalledTimes(2);
    expect(screen.getByText("Connection succeeded")).toBeInTheDocument();
  });

  it("imports versioned skills and keeps them disabled until explicit enablement", async () => {
    const onImportSkill = vi.fn().mockResolvedValue(undefined);
    const onSetSkillEnabled = vi.fn().mockResolvedValue({});
    render(<SettingsPanel locale="zh-CN" onClose={() => undefined} skillPackages={[{ id: "skill-1", name: "scrna-qc", version: "1.2.0", source_path: "skills/scrna-qc/hash", sha256: "abcdef1234567890", enabled: false, capabilities: ["read_project_files"], category: "single_cell" }]} onImportSkill={onImportSkill} onSetSkillEnabled={onSetSkillEnabled} />);

    fireEvent.click(screen.getByRole("button", { name: "技能与 MCP" }));
    expect(screen.getByText(/固定 GitHub 快照/)).toBeInTheDocument();
    expect(screen.getByText("组学技能")).toBeInTheDocument();
    expect(screen.getByText("单细胞组学")).toBeInTheDocument();
    expect(screen.getByText("scrna-qc")).toBeInTheDocument();
    expect(screen.getByText("read_project_files")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "导入技能目录" }));
    expect(onImportSkill).toHaveBeenCalledOnce();
    await waitFor(() => expect(screen.getByRole("button", { name: "启用" })).not.toBeDisabled());
    fireEvent.click(screen.getByRole("button", { name: "启用" }));
    expect(onSetSkillEnabled).toHaveBeenCalledWith("skill-1", true);
  });

  it("configures MCP without launching it and requires inspection plus per-tool approval", async () => {
    const server = { id: "mcp-1", name: "paper-search", command: "npx", args: ["paper-mcp"], enabled: false, launch_approved: false, approved_tools: [], tools: [{ name: "search_papers", description: "Search papers" }], capabilities: { tools: {} }, last_inspected_at: "2026-08-13T12:00:00Z", created_at: "2026-08-13T12:00:00Z", updated_at: "2026-08-13T12:00:00Z" };
    const onSaveMcpServer = vi.fn().mockResolvedValue(server);
    const onInspectMcpServer = vi.fn().mockResolvedValue(undefined);
    const onSetMcpToolApproval = vi.fn().mockResolvedValue(server);
    render(<SettingsPanel locale="zh-CN" onClose={() => undefined} selectedProject={{ id: "project-1", name: "PBMC", description: "", local_root: "E:/PBMC", remote_root: null, connection_id: null, template: "single_cell_rna_seq", status: "ready", ollama_only: false, created_at: "", updated_at: "" }} mcpServers={[server]} onSaveMcpServer={onSaveMcpServer} onInspectMcpServer={onInspectMcpServer} onSetMcpToolApproval={onSetMcpToolApproval} />);

    fireEvent.click(screen.getByRole("button", { name: "技能与 MCP" }));
    fireEvent.change(screen.getByLabelText("MCP server name"), { target: { value: "local-files" } });
    fireEvent.change(screen.getByLabelText("MCP server command"), { target: { value: "npx" } });
    fireEvent.change(screen.getByLabelText("MCP server arguments"), { target: { value: "-y\nfilesystem-mcp" } });
    fireEvent.click(screen.getByRole("button", { name: "保存 MCP server" }));
    await waitFor(() => expect(onSaveMcpServer).toHaveBeenCalledWith({ name: "local-files", command: "npx", args: ["-y", "filesystem-mcp"] }));
    expect(onInspectMcpServer).not.toHaveBeenCalled();

    const inspect = screen.getByRole("button", { name: "检查并发现工具" });
    expect(inspect).toBeDisabled();
    fireEvent.click(screen.getByRole("checkbox"));
    expect(inspect).not.toBeDisabled();
    fireEvent.click(inspect);
    await waitFor(() => expect(onInspectMcpServer).toHaveBeenCalledWith("mcp-1"));
    fireEvent.click(screen.getByRole("button", { name: "批准调用" }));
    expect(onSetMcpToolApproval).toHaveBeenCalledWith("mcp-1", "search_papers", true);
  });

  it("adds PubMed only through the unified scientific catalog without bypassing approvals", async () => {
    const list = vi.fn().mockResolvedValue([{ id: "pubmed", name: "PubMed", description: "Literature", description_zh: "文献检索", tool_count: 3 }]);
    const add = vi.fn().mockResolvedValue({});
    const inspect = vi.fn();
    const approve = vi.fn();
    render(<SettingsPanel locale="zh-CN" initialSection="skills" onClose={() => undefined} onListBundledMcpPresets={list} onAddBundledMcp={add} onInspectMcpServer={inspect} onSetMcpToolApproval={approve} />);
    expect(screen.queryByRole("region", { name: "PubMed MCP 预设" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "一键添加 PubMed MCP" })).not.toBeInTheDocument();
    expect(screen.queryByLabelText("NCBI API key")).not.toBeInTheDocument();
    expect(screen.queryByLabelText("NCBI admin email")).not.toBeInTheDocument();
    await screen.findByRole("option", { name: "PubMed · 3 个工具" });
    fireEvent.change(screen.getByLabelText("选择内置 MCP"), { target: { value: "pubmed" } });
    fireEvent.click(screen.getByRole("button", { name: "添加内置 MCP" }));
    await waitFor(() => expect(add).toHaveBeenCalledWith({ preset_id: "pubmed" }));
    expect(inspect).not.toHaveBeenCalled();
    expect(approve).not.toHaveBeenCalled();
  });
  it("persists MCP working directory, timeout, and credential environment bindings", async () => {
    const onSaveMcpServer = vi.fn().mockResolvedValue({});
    render(<SettingsPanel locale="zh-CN" onClose={() => undefined} onSaveMcpServer={onSaveMcpServer} />);

    fireEvent.click(screen.getByRole("button", { name: "技能与 MCP" }));
    fireEvent.change(screen.getByLabelText("MCP server name"), { target: { value: "pubmed" } });
    fireEvent.change(screen.getByLabelText("MCP server command"), { target: { value: "omicsops-desktop" } });
    fireEvent.change(screen.getByLabelText("MCP working directory"), { target: { value: "E:/Science/project" } });
    fireEvent.change(screen.getByLabelText("MCP timeout seconds"), { target: { value: "120" } });
    fireEvent.click(screen.getByRole("button", { name: "添加变量" }));
    fireEvent.change(screen.getByLabelText("MCP env name 1"), { target: { value: "NCBI_API_KEY" } });
    fireEvent.change(screen.getByLabelText("MCP credential reference 1"), { target: { value: "ncbi/api-key" } });
    fireEvent.click(screen.getByRole("button", { name: "保存 MCP server" }));

    await waitFor(() => expect(onSaveMcpServer).toHaveBeenCalledWith(expect.objectContaining({
      name: "pubmed",
      command: "omicsops-desktop",
      cwd: "E:/Science/project",
      timeout_secs: 120,
      env_bindings: [{ name: "NCBI_API_KEY", credential_reference: "ncbi/api-key" }],
    })));
  });

  it("shows MCP runtime status and bounded diagnostics", async () => {
    const server = { id: "mcp-failed", name: "pubmed", command: "omicsops-desktop", args: ["--mcp-server", "pubmed"], enabled: false, launch_approved: false, approved_tools: [], tools: [], capabilities: {}, status: "failed", last_error: "server exited with code 1", stderr_tail: "invalid configuration", last_inspected_at: null, created_at: "", updated_at: "" };
    render(<SettingsPanel locale="en-US" onClose={() => undefined} mcpServers={[server]} />);
    fireEvent.click(screen.getByRole("button", { name: "Skills and MCP" }));
    expect(screen.getByText("Last run failed")).toBeInTheDocument();
    fireEvent.click(screen.getByText("Latest diagnostics"));
    expect(screen.getByText(/server exited with code 1/)).toBeInTheDocument();
  });

  it("requires host-key confirmation before authenticated remote diagnostics", async () => {
    const connection = { id: "connection-1", label: "Lab SSH", host: "compute.example.org", port: 20090, username: "scientist", authentication: "password" as const, authentication_reference: "ssh/connection-1", host_key_fingerprint: null };
    const onTestConnection = vi.fn()
      .mockResolvedValueOnce({ fingerprint: "SHA256:test", trusted: false, authenticated: false, latencyMs: 20, serverOs: null, remoteUsername: null, home: null, sftpAvailable: false, pythonAvailable: false, rAvailable: false })
      .mockResolvedValueOnce({ fingerprint: "SHA256:test", trusted: true, authenticated: true, latencyMs: 31, serverOs: "Linux 6.8", remoteUsername: "scientist", home: "/home/scientist", sftpAvailable: true, pythonAvailable: true, rAvailable: true });
    const onConfirmHostKey = vi.fn().mockResolvedValue(undefined);
    render(<SettingsPanel locale="zh-CN" onClose={() => undefined} connections={[connection]} onTestConnection={onTestConnection} onConfirmHostKey={onConfirmHostKey} />);

    fireEvent.click(screen.getByRole("button", { name: "远端计算" }));
    fireEvent.click(screen.getByRole("button", { name: "测试连接" }));
    expect(await screen.findByText("等待确认主机指纹")).toBeInTheDocument();
    expect(screen.queryByText(/Linux 6.8/)).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "确认此指纹" }));
    await waitFor(() => expect(onConfirmHostKey).toHaveBeenCalledWith("connection-1", "SHA256:test"));
    expect(await screen.findByText(/Linux 6.8/)).toBeInTheDocument();
  });
});
