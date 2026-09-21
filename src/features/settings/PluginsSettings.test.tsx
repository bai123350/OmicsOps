import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import * as api from "../../integration-packages-api";
import type { InstalledPlugin, PluginInspection } from "../../types";
import { useWindowEscapeLayer } from "./BrowserSettings";
import { PluginsSettings } from "./PluginsSettings";

vi.mock("../../integration-packages-api", () => ({
  choosePluginDirectory: vi.fn(), settingsInspectPlugin: vi.fn(), settingsInstallPlugin: vi.fn(),
  settingsListPlugins: vi.fn(), settingsSetPluginEnabled: vi.fn(), settingsRemovePlugin: vi.fn(),
}));

const inspection: PluginInspection = {
  manifest_digest: "a".repeat(64), source_path: "C:/plugins/qc", package_id: "lab.qc", version: "1.0.0", name: "QC helpers",
  files: [{ relative_path: "omicsops-plugin.json", size_bytes: 100, sha256: "b".repeat(64) }, { relative_path: "skills/qc/SKILL.md", size_bytes: 90, sha256: "c".repeat(64) }],
  bindings: [{ preset_id: "pubmed", server_id: "00000000-0000-0000-0000-000000000001", ownership: "shared_reference", configured: true, enabled: false }],
  existing_installation_id: null, changes: ["new_installation"],
};
const installed: InstalledPlugin = {
  installation_id: "00000000-0000-0000-0000-000000000002", package_id: "lab.qc", version: "1.0.0", name: "QC helpers",
  digest: inspection.manifest_digest, source_path: inspection.source_path, trust: "local_unverified", enabled: false, phase: "installed",
  cleanup_pending: false, predecessor_installation_id: null, files: inspection.files,
  skills: [{ skill_id: "00000000-0000-0000-0000-000000000003", name: "plugin-qc", relative_path: "skills/qc", package_sha256: "d".repeat(64) }],
  mcp_bindings: inspection.bindings, last_error: null, created_at: "2026-09-21T00:00:00Z",
};

describe("PluginsSettings", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(api.settingsListPlugins).mockResolvedValue([]);
    vi.mocked(api.choosePluginDirectory).mockResolvedValue(inspection.source_path);
    vi.mocked(api.settingsInspectPlugin).mockResolvedValue(inspection);
    vi.mocked(api.settingsInstallPlugin).mockResolvedValue(installed);
    vi.mocked(api.settingsSetPluginEnabled).mockResolvedValue({ ...installed, enabled: true });
    vi.mocked(api.settingsRemovePlugin).mockResolvedValue({ installation_id: installed.installation_id, status: "removed", removed_skills: 1, preserved_files: [], mcp_references_removed: 1, message: "removed" });
  });

  it("inspects before installing with the exact digest and refreshes", async () => {
    const changed = vi.fn();
    vi.mocked(api.settingsListPlugins).mockResolvedValueOnce([]).mockResolvedValue([installed]);
    render(<PluginsSettings locale="en-US" onOpenConnections={vi.fn()} onChanged={changed} />);
    await screen.findByText("No local plugins installed");
    fireEvent.click(screen.getByRole("button", { name: "Choose local package" }));
    expect(await screen.findByRole("dialog", { name: "Plugin package preview" })).toBeInTheDocument();
    expect(screen.getByText("skills/qc/SKILL.md")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Install package" }));
    await waitFor(() => expect(api.settingsInstallPlugin).toHaveBeenCalledWith(inspection.source_path, inspection.manifest_digest, null));
    expect(changed).toHaveBeenCalledTimes(1);
    expect(await screen.findByText("QC helpers")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Enable QC helpers" }));
    await waitFor(() => expect(api.settingsSetPluginEnabled).toHaveBeenCalledWith(installed.installation_id, true));
    fireEvent.click(screen.getByRole("button", { name: "Choose local package" }));
    expect(await screen.findByRole("dialog", { name: "Plugin package preview" })).toBeInTheDocument();
  });

  it("keeps a failed operation retryable and ignores late inspection after unmount", async () => {
    vi.mocked(api.settingsInstallPlugin).mockRejectedValueOnce(new Error("source changed"));
    const view = render(<PluginsSettings locale="en-US" onOpenConnections={vi.fn()} />);
    await screen.findByText("No local plugins installed");
    fireEvent.click(screen.getByRole("button", { name: "Choose local package" }));
    await screen.findByRole("dialog", { name: "Plugin package preview" });
    fireEvent.click(screen.getByRole("button", { name: "Install package" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("source changed");
    expect(screen.getByRole("button", { name: "Install package" })).toBeEnabled();
    view.unmount();
  });

  it("toggles and removes through real lifecycle calls without touching shared MCP", async () => {
    vi.mocked(api.settingsListPlugins).mockResolvedValue([installed]);
    render(<PluginsSettings locale="en-US" onOpenConnections={vi.fn()} />);
    await screen.findByText("QC helpers");
    fireEvent.click(screen.getByRole("button", { name: "Enable QC helpers" }));
    await waitFor(() => expect(api.settingsSetPluginEnabled).toHaveBeenCalledWith(installed.installation_id, true));
    fireEvent.click(screen.getByRole("button", { name: "Remove QC helpers" }));
    fireEvent.click(screen.getByRole("button", { name: "Confirm removal" }));
    await waitFor(() => expect(api.settingsRemovePlugin).toHaveBeenCalledWith(installed.installation_id, installed.digest));
  });

  it("retries a pending verified cleanup through removal instead of creating another installation", async () => {
    const cleanup = { ...installed, enabled: false, phase: "needs_attention" as const, cleanup_pending: true };
    vi.mocked(api.settingsListPlugins).mockResolvedValue([cleanup]);
    render(<PluginsSettings locale="en-US" onOpenConnections={vi.fn()} />);
    await screen.findByText("QC helpers");
    fireEvent.click(screen.getByRole("button", { name: "Retry cleanup" }));
    await waitFor(() => expect(api.settingsRemovePlugin).toHaveBeenCalledWith(cleanup.installation_id, cleanup.digest));
    expect(api.settingsInspectPlugin).not.toHaveBeenCalled();
    expect(api.settingsInstallPlugin).not.toHaveBeenCalled();
  });

  it("shows installed package provenance and contributions in details", async () => {
    vi.mocked(api.settingsListPlugins).mockResolvedValue([installed]);
    render(<PluginsSettings locale="en-US" onOpenConnections={vi.fn()} />);
    await screen.findByText("QC helpers");
    fireEvent.click(screen.getByRole("button", { name: "Details" }));
    const dialog = await screen.findByRole("dialog", { name: "Plugin details" });
    expect(dialog).toHaveTextContent(installed.source_path);
    expect(dialog).toHaveTextContent(installed.digest);
    expect(dialog).toHaveTextContent("plugin-qc");
    expect(dialog).toHaveTextContent("pubmed");
  });

  it("closes only the top package preview on immediate Escape", async () => {
    const parentClose = vi.fn();
    function Host() {
      useWindowEscapeLayer(true, parentClose);
      return <PluginsSettings locale="en-US" onOpenConnections={vi.fn()} />;
    }
    render(<Host />);
    await screen.findByText("No local plugins installed");
    fireEvent.click(screen.getByRole("button", { name: "Choose local package" }));
    await screen.findByRole("dialog", { name: "Plugin package preview" });
    fireEvent.keyDown(window, { key: "Escape" });
    expect(screen.queryByRole("dialog", { name: "Plugin package preview" })).not.toBeInTheDocument();
    expect(parentClose).not.toHaveBeenCalled();
  });
});
