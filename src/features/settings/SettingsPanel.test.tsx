import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { SettingsPanel } from "./SettingsPanel";

describe("SettingsPanel model providers", () => {
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
    expect(screen.getByText("Research Skills")).toBeInTheDocument();
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

  it("adds the native PubMed preset without skipping the approval workflow", async () => {
    const onAddPubMedMcp = vi.fn().mockResolvedValue({});
    render(<SettingsPanel locale="zh-CN" onClose={() => undefined} onAddPubMedMcp={onAddPubMedMcp} />);

    fireEvent.click(screen.getByRole("button", { name: "技能与 MCP" }));
    fireEvent.change(screen.getByLabelText("NCBI API key"), { target: { value: "ncbi-secret" } });
    fireEvent.change(screen.getByLabelText("NCBI admin email"), { target: { value: "scientist@example.org" } });
    fireEvent.click(screen.getByRole("button", { name: "一键添加 PubMed MCP" }));

    await waitFor(() => expect(onAddPubMedMcp).toHaveBeenCalledWith({ api_key: "ncbi-secret", admin_email: "scientist@example.org" }));
    expect(screen.getByRole("button", { name: "一键添加 PubMed MCP" })).toBeInTheDocument();
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
