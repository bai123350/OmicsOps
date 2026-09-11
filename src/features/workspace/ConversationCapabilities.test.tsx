import { fireEvent, render, screen, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { WorkspaceShell } from "./WorkspaceShell";

const project = { id: "p", name: "Project", status: "ready" as const, template: "blank" as const };
const summary = {
  project_id: "p", conversation_id: "c", memory_count: 4,
  skills: [{ id: "s1", name: "Literature", enabled: true }, { id: "s2", name: "Analysis", enabled: false }],
  mcp_servers: [{ id: "m1", name: "PubMed", enabled: true, tool_count: 12 }, { id: "m2", name: "ChEMBL", enabled: false, tool_count: 8 }],
};
const props = { project, locale: "en-US" as const, onLocaleChange: vi.fn(), activeConversationId: "c", capabilitySummary: summary };

describe("conversation capabilities", () => {
  it("counts enabled skills and MCP servers and opens an actionable scoped overview", () => {
    const onOpenSettings = vi.fn();
    render(<WorkspaceShell {...props} onOpenSettings={onOpenSettings} />);
    expect(screen.getByText("1 skills · 1 MCP · 4 mem")).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: "Capabilities" }));
    const dialog = screen.getByRole("dialog", { name: "Capabilities" });
    expect(within(dialog).getByText(/Skills and MCP settings are shared across the app/)).toBeVisible();
    expect(within(dialog).getByText("Literature")).toBeVisible();
    expect(within(dialog).getByText("Analysis")).toBeVisible();
    expect(within(dialog).getByText("12 tools")).toBeVisible();
    expect(within(dialog).getByText("4 memories")).toBeVisible();
    fireEvent.click(within(dialog).getByRole("button", { name: "Manage Skills and MCP" }));
    expect(screen.queryByRole("dialog", { name: "Capabilities" })).not.toBeInTheDocument();
    expect(onOpenSettings).toHaveBeenCalledWith("skills");
    fireEvent.click(screen.getByRole("button", { name: "Settings" }));
    expect(onOpenSettings).toHaveBeenLastCalledWith("models");
  });

  it("hides stale counts for loading, a different conversation or project, and no conversation", () => {
    const { rerender } = render(<WorkspaceShell {...props} />);
    for (const extra of [{ capabilitiesLoading: true }, { activeConversationId: "other" }, { activeConversationId: null }, { project: { ...project, id: "other" } }]) {
      rerender(<WorkspaceShell {...props} {...extra} />);
      expect(screen.queryByText("1 skills · 1 MCP · 4 mem")).not.toBeInTheDocument();
      expect(screen.getByText("— skills · — MCP · — mem")).toBeVisible();
    }
    rerender(<WorkspaceShell {...props} capabilitySummary={{ ...summary, skills: [], mcp_servers: [], memory_count: 0 }} />);
    expect(screen.getByText("0 skills · 0 MCP · 0 mem")).toBeVisible();
  });

  it("reports errors without old counts and can retry", () => {
    const onRefreshCapabilities = vi.fn();
    render(<WorkspaceShell {...props} capabilitiesError="Unable to load capabilities" onRefreshCapabilities={onRefreshCapabilities} />);
    expect(screen.getByText("— skills · — MCP · — mem")).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: "Capabilities" }));
    expect(screen.getByRole("alert")).toHaveTextContent("Unable to load capabilities");
    fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    expect(onRefreshCapabilities).toHaveBeenCalledOnce();
  });

  it("closes only the top overlay with an immediate window Escape", () => {
    render(<WorkspaceShell {...props} />);
    fireEvent.click(screen.getByRole("button", { name: "Expand sidebar" }));
    fireEvent.click(screen.getByRole("button", { name: "Capabilities" }));
    fireEvent.keyDown(window, { key: "Escape" });
    expect(screen.queryByRole("dialog", { name: "Capabilities" })).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Collapse sidebar" })).toBeVisible();
    fireEvent.keyDown(window, { key: "Escape" });
    expect(screen.queryByRole("button", { name: "Collapse sidebar" })).not.toBeInTheDocument();
  });
});
