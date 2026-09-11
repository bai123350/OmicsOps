import { fireEvent, render, screen, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { WorkspaceShell } from "./WorkspaceShell";

const props = { project: { id: "p", name: "Research", status: "ready" as const, template: "blank" as const }, locale: "en-US" as const, onLocaleChange: () => undefined };
function openMenu() { fireEvent.click(screen.getByRole("button", { name: "Add sidebar tab" })); }
describe("reference workspace sidebar", () => {
  it("marks all open tabs, opens without duplicates, and selects a neighbor on close", () => {
    render(<WorkspaceShell {...props} />);
    expect(screen.queryByRole("complementary")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Expand sidebar" }));
    expect(screen.queryByRole("menu")).not.toBeInTheDocument();
    openMenu();
    expect(screen.getAllByRole("menuitemcheckbox").map((item) => item.textContent)).toEqual(["Artifacts (0)", "Agents", "Notebook (0)", "Highlights", "Files", "Provenance (0)", "Environment", "Side chat"]);
    for (const name of ["Artifacts (0)", "Agents", "Files", "Environment"]) expect(screen.getByRole("menuitemcheckbox", { name })).toHaveAttribute("aria-checked", "true");
    expect(screen.getByRole("menuitemcheckbox", { name: "Notebook (0)" })).toHaveAttribute("aria-checked", "false");
    expect(screen.getByRole("menuitemcheckbox", { name: "Highlights" })).toBeDisabled();
    expect(screen.getByRole("menuitemcheckbox", { name: "Side chat" })).toBeDisabled();
    fireEvent.click(screen.getByRole("menuitemcheckbox", { name: "Notebook (0)" }));
    expect(screen.getByRole("tab", { name: "Notebook (0)" })).toHaveAttribute("aria-selected", "true");
    openMenu();
    fireEvent.click(screen.getByRole("menuitemcheckbox", { name: "Notebook (0)" }));
    expect(screen.getAllByRole("tab", { name: "Notebook (0)" })).toHaveLength(1);
    fireEvent.click(screen.getByRole("button", { name: "Close tab: Notebook (0)" }));
    expect(screen.getByRole("tab", { name: "Environment" })).toHaveAttribute("aria-selected", "true");
  });
  it("closes menu, preview, and sidebar one layer at a time without moving focus", () => {
    render(<WorkspaceShell {...props} remoteFiles={[{ relative_path: "result.png", directory: false, size_bytes: 42, modified_unix_seconds: 0 }]} />);
    fireEvent.click(screen.getByRole("button", { name: "Expand sidebar" }));
    openMenu();
    fireEvent.keyDown(window, { key: "Escape" });
    expect(screen.queryByRole("menu")).not.toBeInTheDocument();
    expect(screen.getByRole("complementary")).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: "Expand preview" }));
    fireEvent.keyDown(window, { key: "Escape" });
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(screen.getByRole("complementary")).toBeVisible();
    fireEvent.keyDown(window, { key: "Escape" });
    expect(screen.queryByRole("complementary")).not.toBeInTheDocument();
  });
  it("shows delegated tasks separately from Plan and code cells separately from formal records", () => {
    render(<WorkspaceShell {...props} messages={[{ id: "m", role: "assistant", markdown: "```python\nprint(42)\n```" }]} />);
    fireEvent.click(screen.getByRole("button", { name: "Expand sidebar" }));
    fireEvent.click(screen.getByRole("tab", { name: "Agents" }));
    expect(screen.getByText("No delegated tasks")).toBeVisible();
    expect(screen.queryByText("Plan mode is not active")).not.toBeInTheDocument();
    openMenu();
    fireEvent.click(screen.getByRole("menuitemcheckbox", { name: "Notebook (1)" }));
    const panel = screen.getByRole("tabpanel");
    expect(within(panel).getByText("print(42)")).toBeVisible();
    expect(within(panel).getByText("Generated source · not executed")).toBeVisible();
  });
  it("hides the pane after the last tab closes and can reopen it", () => {
    render(<WorkspaceShell {...props} />);
    fireEvent.click(screen.getByRole("button", { name: "Expand sidebar" }));
    for (const name of ["Artifacts (0)", "Agents", "Files", "Environment"]) fireEvent.click(screen.getByRole("button", { name: `Close tab: ${name}` }));
    expect(screen.queryByRole("complementary")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Expand sidebar" }));
    expect(screen.getByRole("tab", { name: "Artifacts (0)" })).toBeVisible();
  });
  it("can reopen a closed Plan without confusing it with Agents", () => {
    render(<WorkspaceShell {...props} agentMode="plan" />);
    expect(screen.getByRole("tab", { name: "Plan" })).toHaveAttribute("aria-selected", "true");
    fireEvent.click(screen.getByRole("button", { name: "Close tab: Plan" }));
    fireEvent.click(screen.getByRole("button", { name: "Review Plan" }));
    expect(screen.getByRole("tab", { name: "Plan" })).toHaveAttribute("aria-selected", "true");
  });
});
