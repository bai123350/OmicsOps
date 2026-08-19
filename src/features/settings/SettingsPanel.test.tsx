import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { SettingsPanel } from "./SettingsPanel";

describe("SettingsPanel model providers", () => {
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
