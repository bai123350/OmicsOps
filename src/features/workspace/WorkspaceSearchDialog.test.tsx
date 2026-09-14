import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import type { WorkspaceSearchEntry } from "../../workspace-search";
import { useWindowEscapeLayer } from "../settings/BrowserSettings";
import { WorkspaceSearchDialog } from "./WorkspaceSearchDialog";

const entries: WorkspaceSearchEntry[] = [
  { key: "project:p1", kind: "project", label: "Cells", description: "RNA analysis workspace", projectId: "p1" },
  { key: "session:p1:s1", kind: "session", label: "Earlier discussion", description: "Quality control notes", projectId: "p1" },
  { key: "artifact:p1:a1", kind: "artifact", label: "QC report", description: "Quality control results", projectId: "p1" },
  { key: "skill:qc", kind: "skill", label: "Quality checks", description: "Inspect sequencing quality" },
  { key: "action:files", kind: "action", label: "Browse files", description: "Open the project file browser", projectId: "p1" },
];

function deferred<T>() {
  let resolve!: (value: T | PromiseLike<T>) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise;
    reject = rejectPromise;
  });
  return { promise, resolve, reject };
}

function renderDialog(overrides: Partial<React.ComponentProps<typeof WorkspaceSearchDialog>> = {}) {
  const props: React.ComponentProps<typeof WorkspaceSearchDialog> = {
    entries,
    zh: false,
    onRetry: vi.fn(),
    onClose: vi.fn(),
    onOpen: vi.fn(),
    ...overrides,
  };
  return render(<WorkspaceSearchDialog {...props} />);
}

describe("WorkspaceSearchDialog", () => {
  it("groups supplied entries and filters labels and descriptions case-insensitively", () => {
    renderDialog();

    const dialog = screen.getByRole("dialog", { name: "Search workspace" });
    expect(within(dialog).getByRole("heading", { name: "Projects" })).toBeInTheDocument();
    expect(within(dialog).getByRole("heading", { name: "Sessions" })).toBeInTheDocument();
    expect(within(dialog).getByRole("heading", { name: "Artifacts" })).toBeInTheDocument();
    expect(within(dialog).getByRole("heading", { name: "Skills" })).toBeInTheDocument();
    expect(within(dialog).getByRole("heading", { name: "Actions" })).toBeInTheDocument();
    expect(within(dialog).getByText("Cells")).toBeInTheDocument();
    expect(within(dialog).getByText("Browse files")).toBeInTheDocument();

    const search = within(dialog).getByRole("searchbox", { name: "Search workspace content" });
    expect(search).toHaveFocus();
    fireEvent.change(search, { target: { value: "SEQUENCING" } });

    expect(within(dialog).getByText("Quality checks")).toBeInTheDocument();
    expect(within(dialog).queryByText("Cells")).not.toBeInTheDocument();
    expect(within(dialog).queryByText("Browse files")).not.toBeInTheDocument();
  });

  it("moves the active result with arrows, scrolls it, and ignores composing keys", () => {
    const scrollIntoView = vi.fn();
    Object.defineProperty(HTMLElement.prototype, "scrollIntoView", { configurable: true, value: scrollIntoView });
    renderDialog({ entries: entries.slice(0, 2) });

    const dialog = screen.getByRole("dialog", { name: "Search workspace" });
    const search = within(dialog).getByRole("searchbox", { name: "Search workspace content" });
    const options = within(dialog).getAllByRole("option");
    expect(options[0]).toHaveAttribute("aria-selected", "true");

    fireEvent.keyDown(search, { key: "ArrowDown" });
    expect(within(dialog).getAllByRole("option")[1]).toHaveAttribute("aria-selected", "true");
    expect(scrollIntoView).toHaveBeenCalled();

    fireEvent.keyDown(search, { key: "ArrowUp", isComposing: true });
    expect(within(dialog).getAllByRole("option")[1]).toHaveAttribute("aria-selected", "true");
    fireEvent.keyDown(search, { key: "ArrowUp", keyCode: 229 });
    expect(within(dialog).getAllByRole("option")[1]).toHaveAttribute("aria-selected", "true");
  });

  it("opens the selected result with Enter after composition has ended", () => {
    const onOpen = vi.fn();
    renderDialog({ entries: entries.slice(0, 2), onOpen });

    const search = screen.getByRole("searchbox", { name: "Search workspace content" });
    fireEvent.keyDown(search, { key: "Enter", isComposing: true });
    expect(onOpen).not.toHaveBeenCalled();
    fireEvent.keyDown(search, { key: "Enter", keyCode: 229 });
    expect(onOpen).not.toHaveBeenCalled();

    fireEvent.keyDown(search, { key: "Enter" });
    expect(onOpen).toHaveBeenCalledWith(entries[0]);
  });

  it("keeps Open and Attach as separate actions and only offers Attach when allowed", async () => {
    const onOpen = vi.fn().mockResolvedValue(false);
    const onAttach = vi.fn().mockResolvedValue(true);
    renderDialog({ onOpen, onAttach, canAttach: (entry) => entry.kind === "artifact" });

    expect(screen.getByRole("button", { name: "Open QC report" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Attach QC report" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Attach Cells" })).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Attach QC report" }));
    await waitFor(() => expect(onAttach).toHaveBeenCalledWith(entries[2]));
    expect(screen.getByRole("dialog", { name: "Search workspace" })).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Open QC report" }));
    await waitFor(() => expect(onOpen).toHaveBeenCalledWith(entries[2]));
    expect(screen.getByRole("dialog", { name: "Search workspace" })).toBeInTheDocument();
  });

  it("prevents duplicate actions and preserves the query after a rejected action", async () => {
    const open = deferred<void>();
    const onOpen = vi.fn().mockReturnValue(open.promise);
    renderDialog({ entries: [entries[2]], onOpen });

    const search = screen.getByRole("searchbox", { name: "Search workspace content" });
    fireEvent.change(search, { target: { value: "qc" } });
    const openButton = screen.getByRole("button", { name: "Open QC report" });
    fireEvent.click(openButton);
    fireEvent.click(openButton);
    fireEvent.keyDown(search, { key: "Enter" });
    expect(onOpen).toHaveBeenCalledTimes(1);
    expect(openButton).toBeDisabled();

    await act(async () => {
      open.reject(new Error("private server response"));
    });
    await waitFor(() => expect(screen.getByRole("alert")).toHaveTextContent("Action failed. Please try again."));
    expect(screen.getByRole("searchbox", { name: "Search workspace content" })).toHaveValue("qc");
    expect(screen.queryByText("private server response")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Open QC report" })).toBeEnabled();
  });

  it("keeps current entries visible while loading or partially failed and exposes retry", () => {
    const onRetry = vi.fn();
    renderDialog({ entries: [entries[0], entries[4]], loading: true, failedProjects: 1, onRetry });

    expect(screen.getByText("Cells")).toBeInTheDocument();
    expect(screen.getByText("Browse files")).toBeInTheDocument();
    expect(screen.getByRole("status")).toHaveTextContent("Loading workspace results");
    expect(screen.getByRole("alert")).toHaveTextContent("Some project results could not be loaded.");
    fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    expect(onRetry).toHaveBeenCalledOnce();
  });

  it("closes on immediate Escape, leaves a parent layer open, and restores prior focus", () => {
    const parentClose = vi.fn();
    const onClose = vi.fn();
    function ParentLayer({ show }: { show: boolean }) {
      useWindowEscapeLayer(true, parentClose);
      return show ? <WorkspaceSearchDialog entries={entries.slice(0, 1)} zh={false} onRetry={vi.fn()} onClose={onClose} onOpen={vi.fn()} /> : null;
    }

    const trigger = document.createElement("button");
    trigger.type = "button";
    trigger.textContent = "Open search";
    document.body.appendChild(trigger);
    trigger.focus();
    const view = render(<ParentLayer show={false} />);
    view.rerender(<ParentLayer show />);

    fireEvent.keyDown(window, { key: "Escape" });
    expect(onClose).toHaveBeenCalledOnce();
    expect(parentClose).not.toHaveBeenCalled();
    view.unmount();
    expect(document.activeElement).toBe(trigger);
    trigger.remove();
  });

  it("does not update after an in-flight action resolves following unmount", async () => {
    const open = deferred<void>();
    const onOpen = vi.fn().mockReturnValue(open.promise);
    const onClose = vi.fn();
    const view = renderDialog({ entries: [entries[0]], onOpen, onClose });

    fireEvent.click(screen.getByRole("button", { name: "Open Cells" }));
    fireEvent.keyDown(window, { key: "Escape" });
    expect(onClose).toHaveBeenCalledOnce();
    view.unmount();

    await act(async () => {
      open.resolve();
    });
    expect(onClose).toHaveBeenCalledOnce();
  });

  it("renders the shared labels in Chinese", () => {
    renderDialog({ entries: [entries[0]], zh: true });

    expect(screen.getByRole("dialog", { name: "搜索工作区" })).toBeInTheDocument();
    expect(screen.getByRole("searchbox", { name: "搜索工作区内容" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "打开 Cells" })).toBeInTheDocument();
  });
});
