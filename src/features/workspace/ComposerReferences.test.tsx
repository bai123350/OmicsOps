import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import {
  ComposerReferenceChips,
  ComposerReferencePicker,
  parseComposerTrigger,
  referenceKey,
} from "./ComposerReferences";
import type { ComposerCatalogItem, ComposerPickerCommand, ComposerReference, ComposerReferenceTrigger } from "./ComposerReferences";

const artifact = (id: string, label: string, description: string): ComposerCatalogItem => ({
  reference: { kind: "artifact", project_id: "project-a", id },
  label,
  description,
});

const session = (id: string, label: string, description: string): ComposerCatalogItem => ({
  reference: { kind: "session", project_id: "project-a", id },
  label,
  description,
});

const skill = (id: string, label: string, description: string): ComposerCatalogItem => ({
  reference: { kind: "skill", id },
  label,
  description,
});

const project = (id: string, label: string, description: string): ComposerCatalogItem => ({
  reference: { kind: "project", project_id: id, id },
  label,
  description,
});

const executionContext = (backendId: string, label: string, description: string): ComposerCatalogItem => ({
  reference: { kind: "execution_context", project_id: "project-a", backend_id: backendId },
  label,
  description,
});

const runtime = (backendId: string, language: "python" | "r", label: string, description: string): ComposerCatalogItem => ({
  reference: { kind: "runtime", project_id: "project-a", backend_id: backendId, language },
  label,
  description,
});

const trigger = (kind: ComposerReferenceTrigger["kind"], query = ""): ComposerReferenceTrigger => ({
  start: 0,
  end: query.length + 1,
  kind,
  query,
});

describe("parseComposerTrigger", () => {
  it("finds an @, #, or / token immediately before the caret", () => {
    expect(parseComposerTrigger("Compare @batch", "Compare @batch".length)).toEqual({
      start: 8,
      end: 14,
      kind: "artifact",
      query: "batch",
    });
    expect(parseComposerTrigger("Review #session", "Review #session".length)).toEqual({
      start: 7,
      end: 15,
      kind: "session",
      query: "session",
    });
    expect(parseComposerTrigger("Use /literature", "Use /literature".length)).toEqual({
      start: 4,
      end: 15,
      kind: "skill",
      query: "literature",
    });
    expect(parseComposerTrigger("@", 1)).toEqual({ start: 0, end: 1, kind: "artifact", query: "" });
  });

  it("keeps project, execution context, and runtime rows in their Wisp trigger groups", () => {
    render(
      <>
        <textarea aria-label="Composer" />
        <ComposerReferencePicker
          items={[artifact("a", "Counts", "Artifact"), executionContext("local", "Local context", "unverified"), runtime("local", "python", "Python runtime", "unverified")]}
          trigger={trigger("artifact")}
          onSelect={vi.fn()}
          onClose={vi.fn()}
          zh={false}
        />
        <ComposerReferencePicker
          items={[project("project-a", "Cells", "Current project"), session("s", "Saved", "Transcript")]}
          trigger={trigger("session")}
          onSelect={vi.fn()}
          onClose={vi.fn()}
          zh={false}
        />
      </>,
    );

    const lists = screen.getAllByRole("listbox");
    expect(within(lists[0]).getAllByRole("option").map((option) => option.textContent)).toEqual([
      expect.stringContaining("Counts"),
      expect.stringContaining("Local context"),
      expect.stringContaining("Python runtime"),
    ]);
    expect(within(lists[1]).getAllByRole("option").map((option) => option.textContent)).toEqual([
      expect.stringContaining("Cells"),
      expect.stringContaining("Saved"),
    ]);
  });

  it("rejects embedded email markers and filesystem paths", () => {
    expect(parseComposerTrigger("email me@example.test", "email me@example.test".length)).toBeNull();
    expect(parseComposerTrigger("open /tmp/results", "open /tmp/results".length)).toBeNull();
    expect(parseComposerTrigger("open @C:\\data\\result", "open @C:\\data\\result".length)).toBeNull();
    expect(parseComposerTrigger("open ./result", "open ./result".length)).toBeNull();
  });
});

describe("ComposerReferencePicker", () => {
  it("filters by trigger kind and searches labels and descriptions case insensitively", () => {
    render(
      <ComposerReferencePicker
        items={[
          artifact("one", "Batch correction", "Corrected PBMC matrix"),
          artifact("two", "QC report", "Batch quality control"),
          session("session", "Batch discussion", "An earlier review"),
        ]}
        trigger={trigger("artifact", "BATCH")}
        onSelect={vi.fn()}
        onClose={vi.fn()}
        zh={false}
      />,
    );

    const listbox = screen.getByRole("listbox");
    expect(within(listbox).getAllByRole("option")).toHaveLength(2);
    expect(within(listbox).getByRole("option", { name: /Batch correction/ })).toBeInTheDocument();
    expect(within(listbox).getByRole("option", { name: /QC report/ })).toBeInTheDocument();
    expect(within(listbox).queryByRole("option", { name: /Batch discussion/ })).not.toBeInTheDocument();
  });

  it("renders loading, error, and no-match states without stale options", () => {
    const items = [artifact("one", "Batch correction", "Corrected matrix")];
    const { rerender } = render(
      <ComposerReferencePicker items={items} trigger={trigger("artifact")} onSelect={vi.fn()} onClose={vi.fn()} zh={false} loading />,
    );
    expect(screen.getByRole("status")).toHaveTextContent("Loading references");
    expect(screen.queryByRole("option")).not.toBeInTheDocument();

    rerender(<ComposerReferencePicker items={items} trigger={trigger("artifact")} onSelect={vi.fn()} onClose={vi.fn()} zh={false} error="Catalog unavailable" />);
    expect(screen.getByRole("alert")).toHaveTextContent("Catalog unavailable");
    expect(screen.queryByRole("option")).not.toBeInTheDocument();

    rerender(<ComposerReferencePicker items={items} trigger={trigger("artifact", "missing")} onSelect={vi.fn()} onClose={vi.fn()} zh={false} />);
    expect(screen.getByRole("status")).toHaveTextContent("No matching references");
    expect(screen.queryByRole("option")).not.toBeInTheDocument();
  });

  it("selects with arrows and Enter while preserving textarea focus", () => {
    const first = artifact("one", "One", "First");
    const second = artifact("two", "Two", "Second");
    const onSelect = vi.fn();
    const onClose = vi.fn();
    const { rerender } = render(
      <>
        <textarea aria-label="Composer" />
        <ComposerReferencePicker items={[first, second]} trigger={trigger("artifact")} onSelect={onSelect} onClose={onClose} zh={false} />
      </>,
    );
    const textarea = screen.getByRole("textbox", { name: "Composer" });
    textarea.focus();
    expect(document.activeElement).toBe(textarea);

    fireEvent.keyDown(textarea, { key: "ArrowDown" });
    fireEvent.keyDown(textarea, { key: "Enter" });
    expect(onSelect).toHaveBeenCalledWith(second);
    expect(onClose).toHaveBeenCalledTimes(1);
    expect(document.activeElement).toBe(textarea);

    onSelect.mockReset();
    onClose.mockReset();
    rerender(
      <>
        <textarea aria-label="Composer" />
        <ComposerReferencePicker items={[first, second]} trigger={trigger("artifact", "one")} onSelect={onSelect} onClose={onClose} zh={false} />
      </>,
    );
    const activeTextarea = screen.getByRole("textbox", { name: "Composer" });
    activeTextarea.focus();
    fireEvent.keyDown(activeTextarea, { key: "Enter" });
    expect(onSelect).toHaveBeenCalledWith(first);
    expect(onSelect).toHaveBeenCalledTimes(1);
  });

  it("does not act during IME composition or when Enter starts on an unrelated element", () => {
    const onSelect = vi.fn();
    const onClose = vi.fn();
    render(
      <>
        <button type="button">Unrelated</button>
        <textarea aria-label="Composer" />
        <ComposerReferencePicker items={[artifact("one", "One", "First")]} trigger={trigger("artifact")} onSelect={onSelect} onClose={onClose} zh={false} />
      </>,
    );
    fireEvent.keyDown(screen.getByRole("button", { name: "Unrelated" }), { key: "Enter" });
    expect(onSelect).not.toHaveBeenCalled();

    const textarea = screen.getByRole("textbox", { name: "Composer" });
    fireEvent.keyDown(textarea, { key: "Enter", isComposing: true, keyCode: 229 });
    expect(onSelect).not.toHaveBeenCalled();
    fireEvent.keyDown(textarea, { key: "Escape", isComposing: true, keyCode: 229 });
    expect(onClose).not.toHaveBeenCalled();
  });

  it("limits capture to the supplied composer textarea and keeps a rejected selection open", () => {
    const onSelect = vi.fn().mockReturnValue(false);
    const onClose = vi.fn();
    const composerRef = { current: null } as { current: HTMLTextAreaElement | null };
    const { rerender } = render(
      <>
        <textarea aria-label="Other textarea" />
        <textarea aria-label="Composer" ref={composerRef} />
        <ComposerReferencePicker items={[artifact("one", "One", "First")]} trigger={trigger("artifact")} onSelect={onSelect} onClose={onClose} zh={false} inputRef={composerRef} />
      </>,
    );
    fireEvent.keyDown(screen.getByRole("textbox", { name: "Other textarea" }), { key: "Enter" });
    expect(onSelect).not.toHaveBeenCalled();
    fireEvent.keyDown(screen.getByRole("textbox", { name: "Composer" }), { key: "Enter" });
    expect(onSelect).toHaveBeenCalledWith(expect.objectContaining({ label: "One" }));
    expect(onClose).not.toHaveBeenCalled();

    onSelect.mockReturnValue(undefined);
    rerender(
      <>
        <textarea aria-label="Other textarea" />
        <textarea aria-label="Composer" ref={composerRef} />
        <ComposerReferencePicker items={[artifact("one", "One", "First")]} trigger={trigger("artifact")} onSelect={onSelect} onClose={onClose} zh={false} inputRef={composerRef} />
      </>,
    );
    fireEvent.keyDown(screen.getByRole("textbox", { name: "Composer" }), { key: "Enter" });
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("groups commands before skills and uses one keyboard selection order", () => {
    const commandSelect = vi.fn();
    const referenceSelect = vi.fn();
    const command: ComposerPickerCommand = { id: "review", label: "Review session", description: "Check evidence", onSelect: commandSelect };
    const skillItem = skill("literature", "Literature helper", "Search primary sources");
    render(
      <>
        <textarea aria-label="Composer" />
        <ComposerReferencePicker items={[skillItem]} commands={[command]} trigger={trigger("skill")} onSelect={referenceSelect} onClose={vi.fn()} zh={false} />
      </>,
    );

    const listbox = screen.getByRole("listbox");
    expect(within(listbox).getByText("Commands")).toBeInTheDocument();
    expect(within(listbox).getByText("Skills")).toBeInTheDocument();
    expect(within(listbox).getAllByRole("option").map((option) => option.textContent)).toEqual([
      expect.stringContaining("Review session"),
      expect.stringContaining("Literature helper"),
    ]);

    const textarea = screen.getByRole("textbox", { name: "Composer" });
    fireEvent.keyDown(textarea, { key: "ArrowDown" });
    fireEvent.keyDown(textarea, { key: "Enter" });
    expect(referenceSelect).toHaveBeenCalledWith(skillItem);
    expect(commandSelect).not.toHaveBeenCalled();
    fireEvent.keyDown(textarea, { key: "ArrowDown" });
    fireEvent.keyDown(textarea, { key: "Enter" });
    expect(commandSelect).toHaveBeenCalledTimes(1);
  });

  it("scrolls the active result into view as keyboard selection moves", async () => {
    render(
      <>
        <textarea aria-label="Composer" />
        <ComposerReferencePicker items={[skill("one", "One", "First"), skill("two", "Two", "Second")]} trigger={trigger("skill")} onSelect={vi.fn()} onClose={vi.fn()} zh={false} />
      </>,
    );
    const options = screen.getAllByRole("option");
    const scrollIntoView = vi.fn();
    options.forEach((option) => {
      Object.defineProperty(option, "scrollIntoView", { configurable: true, value: scrollIntoView });
    });
    fireEvent.keyDown(screen.getByRole("textbox", { name: "Composer" }), { key: "ArrowDown" });
    await waitFor(() => expect(scrollIntoView).toHaveBeenCalledWith({ block: "nearest" }));
  });

  it("keeps local commands visible during skill loading or failure and honors command rejection", () => {
    const commandSelect = vi.fn().mockReturnValue(false);
    const onClose = vi.fn();
    const command: ComposerPickerCommand = { id: "review", label: "Review session", description: "Check evidence", onSelect: commandSelect };
    const { rerender } = render(
      <ComposerReferencePicker items={[]} commands={[command]} trigger={trigger("skill", "evidence")} onSelect={vi.fn()} onClose={onClose} zh={false} loading />,
    );
    expect(screen.getByRole("option", { name: /Review session/ })).toBeInTheDocument();
    expect(screen.getByRole("status")).toHaveTextContent("Loading references");
    fireEvent.click(screen.getByRole("option", { name: /Review session/ }));
    expect(commandSelect).toHaveBeenCalledTimes(1);
    expect(onClose).not.toHaveBeenCalled();

    rerender(<ComposerReferencePicker items={[]} commands={[command]} trigger={trigger("skill", "evidence")} onSelect={vi.fn()} onClose={onClose} zh={false} error="Skills unavailable" />);
    expect(screen.getByRole("option", { name: /Review session/ })).toBeInTheDocument();
    expect(screen.getByRole("alert")).toHaveTextContent("Skills unavailable");
  });

  it("does not invoke a command while the composer is composing IME text", () => {
    const commandSelect = vi.fn();
    const command: ComposerPickerCommand = { id: "review", label: "Review", description: "Check", onSelect: commandSelect };
    render(
      <>
        <textarea aria-label="Composer" />
        <ComposerReferencePicker items={[]} commands={[command]} trigger={trigger("skill")} onSelect={vi.fn()} onClose={vi.fn()} zh={false} />
      </>,
    );
    fireEvent.keyDown(screen.getByRole("textbox", { name: "Composer" }), { key: "Enter", isComposing: true, keyCode: 229 });
    expect(commandSelect).not.toHaveBeenCalled();
  });

  it("closes from the window Escape layer immediately without focus", () => {
    const onClose = vi.fn();
    render(<ComposerReferencePicker items={[artifact("one", "One", "First")]} trigger={trigger("artifact")} onSelect={vi.fn()} onClose={onClose} zh={false} />);
    fireEvent.keyDown(window, { key: "Escape" });
    expect(onClose).toHaveBeenCalledTimes(1);
  });
});

describe("ComposerReferenceChips", () => {
  it("visibly distinguishes local and SSH files with the same path", () => {
    const references: ComposerReference[] = [
      { kind: "workspace_file", project_id: "p", backend_id: "local", relative_path: "counts.csv" },
      { kind: "workspace_file", project_id: "p", backend_id: "ssh:host", relative_path: "counts.csv" },
    ];
    render(<ComposerReferenceChips references={references} items={references.map(reference => ({ reference, label: "counts.csv", description: "" }))} onRemove={vi.fn()} />);
    expect(screen.getByText("Local")).toBeInTheDocument();
    expect(screen.getByText("SSH")).toBeInTheDocument();
  });
  it("renders safe labels and removes the selected reference", () => {
    const reference: ComposerReference = { kind: "skill", id: "safe-skill" };
    const onRemove = vi.fn();
    render(
      <ComposerReferenceChips
        references={[reference]}
        items={[{ reference, label: "<script>alert(1)</script>", description: "unsafe label" }]}
        onRemove={onRemove}
        zh={false}
      />,
    );

    expect(screen.getByText("<script>alert(1)</script>")).toBeInTheDocument();
    expect(screen.queryByRole("heading")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: /Remove reference/ }));
    expect(onRemove).toHaveBeenCalledWith(reference, 0);
  });

  it("uses a stable key that includes project scope", () => {
    expect(referenceKey({ kind: "artifact", project_id: "a", id: "same" })).not.toBe(referenceKey({ kind: "artifact", project_id: "b", id: "same" }));
    expect(referenceKey({ kind: "skill", id: "same" })).not.toBe(referenceKey({ kind: "session", project_id: "a", id: "same" }));
    expect(referenceKey({ kind: "execution_context", project_id: "a", backend_id: "local" })).not.toBe(referenceKey({ kind: "execution_context", project_id: "b", backend_id: "local" }));
    expect(referenceKey({ kind: "runtime", project_id: "a", backend_id: "local", language: "python" })).not.toBe(referenceKey({ kind: "runtime", project_id: "a", backend_id: "local", language: "r" }));
  });

  it("disables chip removal when the composer is busy", () => {
    const reference: ComposerReference = { kind: "skill", id: "busy-skill" };
    render(<ComposerReferenceChips references={[reference]} onRemove={vi.fn()} disabled zh={false} />);
    expect(screen.getByRole("button", { name: /Remove reference/ })).toBeDisabled();
  });
});

it("groups persisted workflows between commands and skills and attaches a stable reference", () => {
  const onSelect = vi.fn();
  const workflow: ComposerCatalogItem = { reference: { kind: "workflow", project_id: "project-a", id: "workflow-a" }, label: "Evidence review", description: "Collect then verify" };
  render(<ComposerReferencePicker items={[skill("skill-a", "Research", ""), workflow]} trigger={trigger("skill")}
    onSelect={onSelect} onClose={vi.fn()} zh={false} commands={[{ id: "plan", label: "/plan", description: "Plan", onSelect: vi.fn() }]} />);
  expect(screen.getAllByRole("group").map((group) => group.getAttribute("aria-label"))).toEqual(["Commands", "Workflows", "Skills"]);
  fireEvent.click(within(screen.getByRole("group", { name: "Workflows" })).getByRole("option"));
  expect(onSelect).toHaveBeenCalledWith(workflow);
  expect(referenceKey(workflow.reference)).toBe("workflow:project-a:workflow-a");
});
