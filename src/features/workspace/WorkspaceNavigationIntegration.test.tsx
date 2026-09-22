import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { expect, it, vi } from "vitest";
import { WorkspaceShell } from "./WorkspaceShell";

vi.mock("./WorkspaceResearchPages", () => ({ WorkspaceResearchPages: ({ page, onInsert }: { page: string; onInsert: (text: string) => void }) => <section aria-label="Research page"><h2>{page}</h2><button onClick={() => onInsert("saved snippet")}>Insert saved snippet</button></section> }));
const props = { project: { id: "p", name: "Research", status: "ready" as const, template: "blank" as const }, locale: "en-US" as const, onLocaleChange: () => undefined };

it("retains the composer draft across navigation and appends a saved snippet without sending", async () => {
  const send = vi.fn();
  render(<WorkspaceShell {...props} onSend={send} />);
  const input = screen.getByRole("textbox", { name: "Describe a research goal or @ mention a project file…" });
  fireEvent.change(input, { target: { value: "My draft" } });
  fireEvent.click(screen.getByRole("button", { name: "Library" }));
  expect(screen.getByRole("region", { name: "Research page" })).toBeVisible();
  expect(screen.queryByRole("textbox", { name: "Describe a research goal or @ mention a project file…" })).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Insert saved snippet" }));
  await waitFor(() => expect(screen.getByRole("textbox", { name: "Describe a research goal or @ mention a project file…" })).toHaveValue("My draft\n\nsaved snippet"));
  expect(send).not.toHaveBeenCalled();
});

it("opens the working Files tab and persists navigation collapse preference", () => {
  render(<WorkspaceShell {...props} />);
  fireEvent.click(screen.getByRole("button", { name: "Files" }));
  expect(screen.getByRole("tab", { name: "Files" })).toHaveAttribute("aria-selected", "true");
  expect(screen.getByRole("group", { name: "File source" })).toBeVisible();
  fireEvent.click(screen.getByRole("button", { name: "Collapse workspace navigation" }));
  expect(localStorage.getItem("omicsops.workspaceNavigationCollapsed")).toBe("true");
  fireEvent.click(screen.getByRole("button", { name: "Expand workspace navigation" }));
});

it("keeps the research editor mounted when a background plan becomes available", () => {
  const { rerender } = render(<WorkspaceShell {...props} planLoading={false} />);
  fireEvent.click(screen.getByRole("button", { name: "Publication" }));
  const editor = screen.getByRole("region", { name: "Research page" });
  rerender(<WorkspaceShell {...props} planLoading />);
  expect(screen.getByRole("region", { name: "Research page" })).toBe(editor);
  expect(screen.queryByRole("textbox", { name: "Describe a research goal or @ mention a project file…" })).not.toBeInTheDocument();
});
