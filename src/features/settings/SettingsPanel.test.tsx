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

  it("imports versioned skills and keeps them disabled until explicit enablement", async () => {
    const onImportSkill = vi.fn().mockResolvedValue(undefined);
    const onSetSkillEnabled = vi.fn().mockResolvedValue({});
    render(<SettingsPanel locale="zh-CN" onClose={() => undefined} skillPackages={[{ id: "skill-1", name: "scrna-qc", version: "1.2.0", source_path: "skills/scrna-qc/hash", sha256: "abcdef1234567890", enabled: false, capabilities: ["read_project_files"] }]} onImportSkill={onImportSkill} onSetSkillEnabled={onSetSkillEnabled} />);

    fireEvent.click(screen.getByRole("button", { name: "技能与 MCP" }));
    expect(screen.getByText("scrna-qc")).toBeInTheDocument();
    expect(screen.getByText("read_project_files")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "导入技能目录" }));
    expect(onImportSkill).toHaveBeenCalledOnce();
    await waitFor(() => expect(screen.getByRole("button", { name: "启用" })).not.toBeDisabled());
    fireEvent.click(screen.getByRole("button", { name: "启用" }));
    expect(onSetSkillEnabled).toHaveBeenCalledWith("skill-1", true);
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
