import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { useState } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { listComposerWorkflows, saveComposerWorkflow } from "../../composer-workflow-api";
import type { ComposerWorkflowTemplate } from "../../types";
import { WorkflowLibraryDialog } from "./WorkflowLibraryDialog";

vi.mock("../../composer-workflow-api", () => ({
  listComposerWorkflows: vi.fn(),
  saveComposerWorkflow: vi.fn(),
}));

const listWorkflows = vi.mocked(listComposerWorkflows);
const saveWorkflow = vi.mocked(saveComposerWorkflow);

const qcWorkflow: ComposerWorkflowTemplate = {
  id: "workflow-qc",
  project_id: "project-a",
  name: "QC recipe",
  description: "Inspect counts before reporting.",
  steps: ["Inspect counts", "Write report"],
  enabled: true,
};

const savedWorkflow: ComposerWorkflowTemplate = {
  id: "workflow-new",
  project_id: "project-a",
  name: "New recipe",
  description: "A saved recipe.",
  steps: ["Inspect", "Report"],
  enabled: false,
};

function deferred<T>() {
  let resolve!: (value: T | PromiseLike<T>) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise;
    reject = rejectPromise;
  });
  return { promise, resolve, reject };
}

function renderDialog(overrides: Partial<React.ComponentProps<typeof WorkflowLibraryDialog>> = {}) {
  const props: React.ComponentProps<typeof WorkflowLibraryDialog> = {
    projectId: "project-a",
    zh: false,
    onClose: vi.fn(),
    onChanged: vi.fn(),
    ...overrides,
  };
  return render(<WorkflowLibraryDialog {...props} />);
}

beforeEach(() => {
  vi.clearAllMocks();
  listWorkflows.mockResolvedValue([]);
  saveWorkflow.mockResolvedValue(savedWorkflow);
});

describe("WorkflowLibraryDialog", () => {
  it("lists persisted workflows and opens their ordered steps for editing", async () => {
    listWorkflows.mockResolvedValue([qcWorkflow]);
    renderDialog();

    const dialog = await screen.findByRole("dialog", { name: "Workflow library" });
    expect(within(dialog).getByText("QC recipe")).toBeInTheDocument();
    expect(within(dialog).getByText("Inspect counts before reporting.")).toBeInTheDocument();
    expect(within(dialog).getByText("2 steps")).toBeInTheDocument();
    expect(within(dialog).queryByRole("button", { name: /Run|Execute/i })).not.toBeInTheDocument();

    fireEvent.click(within(dialog).getByRole("button", { name: "Edit QC recipe" }));
    expect(screen.getByRole("textbox", { name: "Workflow name" })).toHaveValue("QC recipe");
    expect(screen.getByRole("textbox", { name: "Workflow description" })).toHaveValue("Inspect counts before reporting.");
    expect(screen.getByRole("textbox", { name: "Step 1" })).toHaveValue("Inspect counts");
    expect(screen.getByRole("textbox", { name: "Step 2" })).toHaveValue("Write report");
  });

  it("creates a workflow with ordered steps and saves its enabled state", async () => {
    const onChanged = vi.fn();
    renderDialog({ onChanged });
    await screen.findByText("No saved workflows");

    fireEvent.click(screen.getByRole("button", { name: "New workflow" }));
    fireEvent.change(screen.getByRole("textbox", { name: "Workflow name" }), { target: { value: "New recipe" } });
    fireEvent.change(screen.getByRole("textbox", { name: "Workflow description" }), { target: { value: "A saved recipe." } });
    fireEvent.change(screen.getByRole("textbox", { name: "Step 1" }), { target: { value: "Inspect" } });
    fireEvent.click(screen.getByRole("button", { name: "Add step" }));
    fireEvent.change(screen.getByRole("textbox", { name: "Step 2" }), { target: { value: "Report" } });
    fireEvent.click(screen.getByRole("checkbox", { name: "Enabled" }));
    fireEvent.click(screen.getByRole("button", { name: "Save workflow" }));

    await waitFor(() => expect(saveWorkflow).toHaveBeenCalledWith({
      id: null,
      project_id: "project-a",
      name: "New recipe",
      description: "A saved recipe.",
      steps: ["Inspect", "Report"],
      enabled: false,
    }));
    await waitFor(() => expect(onChanged).toHaveBeenCalledOnce());
    expect(screen.queryByRole("textbox", { name: "Workflow name" })).not.toBeInTheDocument();
    expect(screen.getByText("New recipe")).toBeInTheDocument();
  });

  it("supports removing steps and enforces the twelve step limit", async () => {
    renderDialog();
    await screen.findByText("No saved workflows");
    fireEvent.click(screen.getByRole("button", { name: "New workflow" }));

    for (let index = 1; index < 12; index += 1) fireEvent.click(screen.getByRole("button", { name: "Add step" }));
    expect(screen.getAllByRole("textbox", { name: /^Step \d+$/ })).toHaveLength(12);
    expect(screen.getByRole("button", { name: "Add step" })).toBeDisabled();
    fireEvent.click(screen.getByRole("button", { name: "Remove step 12" }));
    expect(screen.getAllByRole("textbox", { name: /^Step \d+$/ })).toHaveLength(11);
    expect(screen.getByRole("button", { name: "Add step" })).toBeEnabled();
  });

  it("preserves the complete draft after a save failure without showing transport details", async () => {
    saveWorkflow.mockRejectedValue(new Error("private transport detail"));
    renderDialog();
    await screen.findByText("No saved workflows");
    fireEvent.click(screen.getByRole("button", { name: "New workflow" }));
    fireEvent.change(screen.getByRole("textbox", { name: "Workflow name" }), { target: { value: "Keep this draft" } });
    fireEvent.change(screen.getByRole("textbox", { name: "Workflow description" }), { target: { value: "Draft description" } });
    fireEvent.change(screen.getByRole("textbox", { name: "Step 1" }), { target: { value: "Draft step" } });
    fireEvent.click(screen.getByRole("button", { name: "Save workflow" }));

    expect(await screen.findByRole("alert")).toHaveTextContent("Could not save workflow. Please try again.");
    expect(screen.queryByText("private transport detail")).not.toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: "Workflow name" })).toHaveValue("Keep this draft");
    expect(screen.getByRole("textbox", { name: "Workflow description" })).toHaveValue("Draft description");
    expect(screen.getByRole("textbox", { name: "Step 1" })).toHaveValue("Draft step");
    expect(screen.getByRole("button", { name: "Save workflow" })).toBeEnabled();
  });

  it("keeps a late save in the catalog after Escape and preserves a newer editor draft", async () => {
    const pending = deferred<ComposerWorkflowTemplate>();
    saveWorkflow.mockReturnValueOnce(pending.promise);
    const onChanged = vi.fn();
    renderDialog({ onChanged });
    await screen.findByText("No saved workflows");

    fireEvent.click(screen.getByRole("button", { name: "New workflow" }));
    fireEvent.change(screen.getByRole("textbox", { name: "Workflow name" }), { target: { value: "Saved after close" } });
    fireEvent.change(screen.getByRole("textbox", { name: "Step 1" }), { target: { value: "Inspect" } });
    fireEvent.click(screen.getByRole("button", { name: "Save workflow" }));
    await waitFor(() => expect(saveWorkflow).toHaveBeenCalledOnce());

    fireEvent.keyDown(window, { key: "Escape" });
    expect(screen.queryByRole("textbox", { name: "Workflow name" })).not.toBeInTheDocument();
    expect(screen.getByRole("dialog", { name: "Workflow library" })).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "New workflow" }));
    fireEvent.change(screen.getByRole("textbox", { name: "Workflow name" }), { target: { value: "Newer draft" } });
    const newerName = screen.getByRole("textbox", { name: "Workflow name" });
    expect(document.activeElement).toBe(newerName);

    await act(async () => pending.resolve(savedWorkflow));
    expect(await screen.findByText("New recipe")).toBeInTheDocument();
    expect(onChanged).toHaveBeenCalledOnce();
    expect(screen.getByRole("textbox", { name: "Workflow name" })).toHaveValue("Newer draft");
    expect(document.activeElement).toBe(screen.getByRole("textbox", { name: "Workflow name" }));
  });

  it("ignores a late save after the whole library closes", async () => {
    const pending = deferred<ComposerWorkflowTemplate>();
    saveWorkflow.mockReturnValueOnce(pending.promise);
    const onChanged = vi.fn();
    function Host() {
      const [open, setOpen] = useState(true);
      return open ? <WorkflowLibraryDialog projectId="project-a" zh={false} onClose={() => setOpen(false)} onChanged={onChanged} /> : null;
    }

    render(<Host />);
    await screen.findByText("No saved workflows");
    fireEvent.click(screen.getByRole("button", { name: "New workflow" }));
    fireEvent.change(screen.getByRole("textbox", { name: "Workflow name" }), { target: { value: "Closed library save" } });
    fireEvent.change(screen.getByRole("textbox", { name: "Step 1" }), { target: { value: "Inspect" } });
    fireEvent.click(screen.getByRole("button", { name: "Save workflow" }));
    await waitFor(() => expect(saveWorkflow).toHaveBeenCalledOnce());
    fireEvent.keyDown(window, { key: "Escape" });
    fireEvent.keyDown(window, { key: "Escape" });
    expect(screen.queryByRole("dialog", { name: "Workflow library" })).not.toBeInTheDocument();

    await act(async () => pending.resolve(savedWorkflow));
    expect(onChanged).not.toHaveBeenCalled();
  });

  it("ignores a late save after the library switches projects", async () => {
    const pending = deferred<ComposerWorkflowTemplate>();
    saveWorkflow.mockReturnValueOnce(pending.promise);
    const onChanged = vi.fn();
    function Host() {
      const [projectId, setProjectId] = useState("project-a");
      return <>
        <button type="button" onClick={() => setProjectId("project-b")}>Switch project</button>
        <WorkflowLibraryDialog projectId={projectId} zh={false} onClose={vi.fn()} onChanged={onChanged} />
      </>;
    }

    render(<Host />);
    await screen.findByText("No saved workflows");
    fireEvent.click(screen.getByRole("button", { name: "New workflow" }));
    fireEvent.change(screen.getByRole("textbox", { name: "Workflow name" }), { target: { value: "Stale project save" } });
    fireEvent.change(screen.getByRole("textbox", { name: "Step 1" }), { target: { value: "Inspect" } });
    fireEvent.click(screen.getByRole("button", { name: "Save workflow" }));
    await waitFor(() => expect(saveWorkflow).toHaveBeenCalledOnce());

    fireEvent.click(screen.getByRole("button", { name: "Switch project" }));
    await waitFor(() => expect(screen.queryByRole("textbox", { name: "Workflow name" })).not.toBeInTheDocument());
    await act(async () => pending.resolve(savedWorkflow));
    expect(onChanged).not.toHaveBeenCalled();
    expect(screen.queryByText("New recipe")).not.toBeInTheDocument();
  });

  it("shows loading, retryable failure, and the recovered catalog", async () => {
    const pending = deferred<ComposerWorkflowTemplate[]>();
    listWorkflows.mockReturnValueOnce(pending.promise).mockResolvedValueOnce([qcWorkflow]);
    renderDialog();
    expect(screen.getByRole("status", { name: "Loading workflows" })).toBeInTheDocument();
    await act(async () => pending.reject(new Error("private list detail")));
    expect(await screen.findByRole("alert")).toHaveTextContent("Could not load workflows. Please try again.");
    expect(screen.queryByText("private list detail")).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    expect(await screen.findByText("QC recipe")).toBeInTheDocument();
    expect(listWorkflows).toHaveBeenCalledTimes(2);
  });

  it("ignores stale catalog results after the project changes", async () => {
    const first = deferred<ComposerWorkflowTemplate[]>();
    const second = deferred<ComposerWorkflowTemplate[]>();
    listWorkflows.mockImplementation((projectId) => projectId === "project-a" ? first.promise : second.promise);
    const view = renderDialog();
    view.rerender(<WorkflowLibraryDialog projectId="project-b" zh={false} onClose={vi.fn()} onChanged={vi.fn()} />);

    await act(async () => second.resolve([{ ...qcWorkflow, id: "workflow-b", project_id: "project-b", name: "Project B recipe" }]));
    expect(await screen.findByText("Project B recipe")).toBeInTheDocument();
    await act(async () => first.resolve([{ ...qcWorkflow, name: "Stale project A recipe" }]));
    expect(screen.queryByText("Stale project A recipe")).not.toBeInTheDocument();
  });

  it("ignores an in-flight catalog result after the library closes", async () => {
    const pending = deferred<ComposerWorkflowTemplate[]>();
    listWorkflows.mockReturnValue(pending.promise);
    const onClose = vi.fn();
    function Host() {
      const [open, setOpen] = useState(true);
      return open ? <WorkflowLibraryDialog projectId="project-a" zh={false} onClose={() => { onClose(); setOpen(false); }} onChanged={vi.fn()} /> : null;
    }

    render(<Host />);
    fireEvent.click(screen.getByRole("button", { name: "Close workflow library" }));
    expect(onClose).toHaveBeenCalledOnce();
    await act(async () => pending.resolve([qcWorkflow]));
    expect(screen.queryByRole("dialog", { name: "Workflow library" })).not.toBeInTheDocument();
  });

  it("closes the editor before the library on immediate window Escape", async () => {
    listWorkflows.mockResolvedValue([qcWorkflow]);
    const onClose = vi.fn();
    renderDialog({ onClose });
    await screen.findByText("QC recipe");
    fireEvent.click(screen.getByRole("button", { name: "Edit QC recipe" }));

    fireEvent.keyDown(window, { key: "Escape" });
    expect(screen.queryByRole("textbox", { name: "Workflow name" })).not.toBeInTheDocument();
    expect(screen.getByRole("dialog", { name: "Workflow library" })).toBeInTheDocument();
    expect(onClose).not.toHaveBeenCalled();

    fireEvent.keyDown(window, { key: "Escape" });
    expect(onClose).toHaveBeenCalledOnce();
  });

  it("closes immediately from the window layer without requiring focus", () => {
    const onClose = vi.fn();
    renderDialog({ onClose });
    fireEvent.keyDown(window, { key: "Escape" });
    expect(onClose).toHaveBeenCalledOnce();
  });

  it("autofocuses the library and wraps Tab within its controls", async () => {
    renderDialog();
    const dialog = await screen.findByRole("dialog", { name: "Workflow library" });
    const closeButton = within(dialog).getByRole("button", { name: "Close workflow library" });
    const newButton = within(dialog).getByRole("button", { name: "New workflow" });

    expect(document.activeElement).toBe(closeButton);
    fireEvent.keyDown(closeButton, { key: "Tab" });
    expect(document.activeElement).toBe(newButton);
    fireEvent.keyDown(newButton, { key: "Tab", shiftKey: true });
    expect(document.activeElement).toBe(closeButton);
  });

  it("autofocuses the nested editor and wraps Tab within the editor", async () => {
    listWorkflows.mockResolvedValue([qcWorkflow]);
    renderDialog();
    const dialog = await screen.findByRole("dialog", { name: "Workflow library" });
    fireEvent.click(within(dialog).getByRole("button", { name: "Edit QC recipe" }));

    const name = screen.getByRole("textbox", { name: "Workflow name" });
    const editorClose = screen.getByRole("button", { name: "Close editor" });
    const save = screen.getByRole("button", { name: "Save workflow" });
    expect(document.activeElement).toBe(name);
    editorClose.focus();
    fireEvent.keyDown(editorClose, { key: "Tab", shiftKey: true });
    expect(document.activeElement).toBe(save);
    fireEvent.keyDown(save, { key: "Tab" });
    expect(document.activeElement).toBe(editorClose);
  });

  it("returns focus to the editor launch button when the editor closes", async () => {
    listWorkflows.mockResolvedValue([qcWorkflow]);
    renderDialog();
    const dialog = await screen.findByRole("dialog", { name: "Workflow library" });
    const editButton = within(dialog).getByRole("button", { name: "Edit QC recipe" });
    fireEvent.click(editButton);
    expect(document.activeElement).toBe(screen.getByRole("textbox", { name: "Workflow name" }));

    fireEvent.click(screen.getByRole("button", { name: "Close editor" }));
    expect(document.activeElement).toBe(editButton);
  });

  it("returns focus to the library launch control when the library unmounts", async () => {
    const onClose = vi.fn();
    function Host() {
      const [open, setOpen] = useState(false);
      return <>
        <button type="button" onClick={() => setOpen(true)}>Launch workflow library</button>
        {open && <WorkflowLibraryDialog projectId="project-a" zh={false} onClose={() => { onClose(); setOpen(false); }} onChanged={vi.fn()} />}
      </>;
    }

    render(<Host />);
    const launch = screen.getByRole("button", { name: "Launch workflow library" });
    launch.focus();
    fireEvent.click(launch);
    await screen.findByRole("dialog", { name: "Workflow library" });
    fireEvent.click(screen.getByRole("button", { name: "Close workflow library" }));

    await waitFor(() => expect(screen.queryByRole("dialog", { name: "Workflow library" })).not.toBeInTheDocument());
    expect(onClose).toHaveBeenCalledOnce();
    expect(document.activeElement).toBe(launch);
  });
});
