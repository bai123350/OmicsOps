import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import type { McpServerProfile } from "../../types";
import * as api from "../../tauri-api";
import { PermissionsSettings } from "./PermissionsSettings";

vi.mock("../../tauri-api", () => ({
  browserListAuthorizations: vi.fn(),
  browserRevokeAuthorization: vi.fn(),
}));

const authorization = {
  id: "browser-project-1",
  scope: "project" as const,
  binding: {
    capability: "web_open_tab",
    target_host: "pubmed.ncbi.nlm.nih.gov",
    session: "workspace" as const,
    protocol_version: 1,
  },
  project_id: "project-1",
  created_at_ms: 1_700_000_000_000,
};

const server = (approvedTools = ["search_papers", "fetch_paper"]): McpServerProfile => ({
  id: "mcp-pubmed",
  name: "PubMed",
  command: "omicsops-desktop",
  args: ["--mcp-server", "pubmed"],
  enabled: true,
  launch_approved: true,
  approved_tools: approvedTools,
  tools: approvedTools.map((name) => ({ name, description: `${name} description` })),
  capabilities: {},
  last_inspected_at: "2026-09-15T00:00:00Z",
  created_at: "2026-09-15T00:00:00Z",
  updated_at: "2026-09-15T00:00:00Z",
});

function mockedApi() {
  return api as unknown as {
    browserListAuthorizations: ReturnType<typeof vi.fn>;
    browserRevokeAuthorization: ReturnType<typeof vi.fn>;
  };
}

beforeEach(() => {
  vi.clearAllMocks();
  mockedApi().browserListAuthorizations.mockResolvedValue([authorization]);
  mockedApi().browserRevokeAuthorization.mockResolvedValue(true);
});

describe("PermissionsSettings", () => {
  it("lists only real MCP and browser authorization fields", async () => {
    render(<PermissionsSettings locale="en-US" mcpServers={[server()]} />);

    expect(screen.getByRole("heading", { name: "MCP permissions" })).toBeInTheDocument();
    expect(screen.getByText("Server launch")).toBeInTheDocument();
    expect(screen.getByText("search_papers")).toBeInTheDocument();
    expect(await screen.findByText("pubmed.ncbi.nlm.nih.gov")).toBeInTheDocument();
    expect(screen.getByText("Project")).toBeInTheDocument();
    expect(screen.getByText("web_open_tab")).toBeInTheDocument();
    expect(screen.getByText(/Workspace session · protocol v1/)).toBeInTheDocument();
    expect(screen.getByText(/project: project-1/)).toBeInTheDocument();
  });

  it("serializes tool revocations for one server and applies each returned profile", async () => {
    let resolveFirst!: (profile: McpServerProfile) => void;
    const first = new Promise<McpServerProfile>((resolve) => { resolveFirst = resolve; });
    const revokeTool = vi.fn()
      .mockReturnValueOnce(first)
      .mockResolvedValueOnce({ ...server([]), launch_approved: true });

    render(<PermissionsSettings locale="en-US" mcpServers={[server()]} onSetMcpToolApproval={revokeTool} />);

    const firstButton = screen.getByRole("button", { name: "Revoke search_papers permission for PubMed" });
    const secondButton = screen.getByRole("button", { name: "Revoke fetch_paper permission for PubMed" });
    fireEvent.click(firstButton);
    fireEvent.click(secondButton);
    expect(revokeTool).toHaveBeenCalledTimes(1);
    expect(secondButton).toBeDisabled();

    resolveFirst({ ...server(["fetch_paper"]), launch_approved: true });
    await waitFor(() => expect(screen.queryByText("search_papers")).not.toBeInTheDocument());
    fireEvent.click(screen.getByRole("button", { name: "Revoke fetch_paper permission for PubMed" }));
    await waitFor(() => expect(revokeTool).toHaveBeenNthCalledWith(2, "mcp-pubmed", "fetch_paper", false));
  });

  it("keeps MCP permissions visible when revocation fails", async () => {
    const revokeLaunch = vi.fn().mockRejectedValue(new Error("profile store unavailable"));
    render(<PermissionsSettings locale="en-US" mcpServers={[server()]} onSetMcpLaunchApproval={revokeLaunch} />);

    fireEvent.click(screen.getByRole("button", { name: "Revoke launch permission for PubMed" }));

    expect(await screen.findByRole("alert")).toHaveTextContent("profile store unavailable");
    expect(screen.getByText("Server launch")).toBeInTheDocument();
    expect(screen.getByText("search_papers")).toBeInTheDocument();
  });

  it("removes the returned MCP launch and tool approvals after a successful launch revocation", async () => {
    const revokeLaunch = vi.fn().mockResolvedValue({ ...server([]), enabled: false, launch_approved: false });
    render(<PermissionsSettings locale="en-US" mcpServers={[server()]} onSetMcpLaunchApproval={revokeLaunch} />);

    fireEvent.click(screen.getByRole("button", { name: "Revoke launch permission for PubMed" }));

    await waitFor(() => expect(screen.queryByText("Server launch")).not.toBeInTheDocument());
    expect(screen.queryByText("search_papers")).not.toBeInTheDocument();
    expect(screen.getByText("No durable MCP permissions.")).toBeInTheDocument();
  });

  it("does not hide a browser authorization when host revocation fails", async () => {
    mockedApi().browserRevokeAuthorization.mockResolvedValueOnce(false);
    render(<PermissionsSettings locale="en-US" mcpServers={[]} />);

    expect(await screen.findByText("pubmed.ncbi.nlm.nih.gov")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Revoke browser authorization for pubmed.ncbi.nlm.nih.gov" }));

    expect(await screen.findByRole("alert")).toHaveTextContent("was not found or was already revoked");
    expect(screen.getByText("pubmed.ncbi.nlm.nih.gov")).toBeInTheDocument();
  });

  it("keeps an authorization load error retryable", async () => {
    mockedApi().browserListAuthorizations
      .mockRejectedValueOnce(new Error("authorization store unavailable"))
      .mockResolvedValueOnce([authorization]);
    render(<PermissionsSettings locale="en-US" mcpServers={[]} />);

    expect(await screen.findByRole("alert")).toHaveTextContent("authorization store unavailable");
    fireEvent.click(screen.getByRole("button", { name: "Retry browser authorizations" }));

    expect(await screen.findByText("pubmed.ncbi.nlm.nih.gov")).toBeInTheDocument();
    expect(mockedApi().browserListAuthorizations).toHaveBeenCalledTimes(2);
  });
});
