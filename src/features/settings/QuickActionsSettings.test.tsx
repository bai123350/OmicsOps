import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { useState } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import type { QuickAction } from "../../project-template-types";
import type { ComposerWorkflowTemplate, WorkspaceProject } from "../../types";
import { QuickActionsSettings } from "./QuickActionsSettings";
import { useWindowEscapeLayer } from "./BrowserSettings";

const api = vi.hoisted(() => ({
  listQuickActions: vi.fn(),
  saveQuickAction: vi.fn(),
  deleteQuickAction: vi.fn(),
  listComposerWorkflows: vi.fn(),
}));
vi.mock("../../project-templates-api", () => ({
  listQuickActions: api.listQuickActions,
  saveQuickAction: api.saveQuickAction,
  deleteQuickAction: api.deleteQuickAction,
}));
vi.mock("../../composer-workflow-api", () => ({ listComposerWorkflows: api.listComposerWorkflows }));

function project(id: string, name: string): WorkspaceProject {
  return {
    id,
    name,
    description: "",
    local_root: `E:/data/${id}`,
    remote_root: null,
    connection_id: null,
    template: "blank",
    status: "ready",
    ollama_only: false,
    created_at: "2026-09-15T00:00:00Z",
    updated_at: "2026-09-15T00:00:00Z",
  };
}

const workflow: ComposerWorkflowTemplate = {
  id: "workflow-1",
  project_id: "project-1",
  name: "QC workflow",
  description: "Inspect counts",
  steps: ["Inspect counts"],
  enabled: true,
};

const action: QuickAction = {
  id: "action-1",
  project_id: "project-1",
  name: "Review QC",
  description: "Insert the workflow",
  workflow_id: "workflow-1",
  enabled: true,
};

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((next) => { resolve = next; });
  return { promise, resolve };
}

beforeEach(() => {
  Object.values(api).forEach((mock) => mock.mockReset());
  api.listQuickActions.mockResolvedValue([]);
  api.listComposerWorkflows.mockResolvedValue([workflow]);
  api.saveQuickAction.mockResolvedValue(action);
  api.deleteQuickAction.mockResolvedValue(undefined);
});

describe("QuickActionsSettings", () => {
  it("creates a project action that points at an existing workflow", async () => {
    const onChanged = vi.fn();
    render(<QuickActionsSettings selectedProject={project("project-1", "PBMC")} locale="en-US" onChanged={onChanged} />);
    await screen.findByText("No quick actions yet.");

    fireEvent.click(screen.getByRole("button", { name: "New quick action" }));
    fireEvent.change(screen.getByLabelText("Quick action name"), { target: { value: "Review QC" } });
    fireEvent.change(screen.getByLabelText("Quick action description"), { target: { value: "Insert the workflow" } });
    fireEvent.change(screen.getByLabelText("Workflow"), { target: { value: "workflow-1" } });
    fireEvent.click(screen.getByRole("button", { name: "Save quick action" }));

    await waitFor(() => expect(api.saveQuickAction).toHaveBeenCalledWith({
      id: null,
      project_id: "project-1",
      name: "Review QC",
      description: "Insert the workflow",
      workflow_id: "workflow-1",
      enabled: true,
    }));
    expect(onChanged).toHaveBeenCalledTimes(1);
    expect(screen.getByText("Review QC")).toBeInTheDocument();
  });

  it("edits enabled state and confirms deletion inline", async () => {
    api.listQuickActions.mockResolvedValue([action]);
    api.saveQuickAction.mockResolvedValue({ ...action, enabled: false });
    const onChanged = vi.fn();
    render(<QuickActionsSettings selectedProject={project("project-1", "PBMC")} locale="en-US" onChanged={onChanged} />);
    await screen.findByText("Review QC");

    fireEvent.click(screen.getByRole("button", { name: "Edit Review QC" }));
    fireEvent.click(screen.getByRole("checkbox", { name: "Enabled" }));
    fireEvent.click(screen.getByRole("button", { name: "Save quick action" }));
    await waitFor(() => expect(api.saveQuickAction).toHaveBeenCalledWith(expect.objectContaining({
      id: "action-1",
      enabled: false,
    })));

    fireEvent.click(screen.getByRole("button", { name: "Delete Review QC" }));
    expect(screen.getByText("Delete Review QC? This cannot be undone.")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Confirm delete Review QC" }));
    await waitFor(() => expect(api.deleteQuickAction).toHaveBeenCalledWith("project-1", "action-1"));
    expect(onChanged).toHaveBeenCalledTimes(2);
    expect(screen.queryByText("Review QC")).not.toBeInTheDocument();
  });

  it("retains the complete editor draft when saving fails", async () => {
    api.saveQuickAction.mockRejectedValue(new Error("private transport detail"));
    render(<QuickActionsSettings selectedProject={project("project-1", "PBMC")} locale="en-US" />);
    await screen.findByText("No quick actions yet.");
    fireEvent.click(screen.getByRole("button", { name: "New quick action" }));
    fireEvent.change(screen.getByLabelText("Quick action name"), { target: { value: "Keep this draft" } });
    fireEvent.change(screen.getByLabelText("Quick action description"), { target: { value: "Unsaved details" } });
    fireEvent.change(screen.getByLabelText("Workflow"), { target: { value: "workflow-1" } });
    fireEvent.click(screen.getByRole("button", { name: "Save quick action" }));

    expect(await screen.findByRole("alert")).toHaveTextContent("Could not save quick action. Check the fields and try again.");
    expect(screen.queryByText("private transport detail")).not.toBeInTheDocument();
    expect(screen.getByLabelText("Quick action name")).toHaveValue("Keep this draft");
    expect(screen.getByLabelText("Quick action description")).toHaveValue("Unsaved details");
    expect(screen.getByLabelText("Workflow")).toHaveValue("workflow-1");
  });

  it("keeps disabled or missing workflow bindings visible but unavailable", async () => {
    api.listQuickActions.mockResolvedValue([
      action,
      { ...action, id: "action-2", name: "Missing workflow", workflow_id: "workflow-missing" },
    ]);
    api.listComposerWorkflows.mockResolvedValue([{ ...workflow, enabled: false }]);
    render(<QuickActionsSettings selectedProject={project("project-1", "PBMC")} locale="en-US" />);

    expect(await screen.findAllByText("Workflow unavailable or disabled")).toHaveLength(2);
    fireEvent.click(screen.getByRole("button", { name: "Edit Review QC" }));
    expect(screen.getByRole("button", { name: "Save quick action" })).toBeDisabled();
    expect(screen.getByLabelText("Workflow")).toHaveAccessibleDescription("The selected workflow is unavailable or disabled.");
  });

  it("ignores late catalogs after the selected project changes", async () => {
    const oldActions = deferred<QuickAction[]>();
    const oldWorkflows = deferred<ComposerWorkflowTemplate[]>();
    api.listQuickActions.mockImplementation((projectId: string) => projectId === "old" ? oldActions.promise : Promise.resolve([]));
    api.listComposerWorkflows.mockImplementation((projectId: string) => projectId === "old" ? oldWorkflows.promise : Promise.resolve([{ ...workflow, id: "workflow-new", project_id: "new", name: "New workflow" }]));
    const view = render(<QuickActionsSettings selectedProject={project("old", "Old")} locale="en-US" />);
    view.rerender(<QuickActionsSettings selectedProject={project("new", "New")} locale="en-US" />);

    expect(await screen.findByText("No quick actions yet.")).toBeInTheDocument();
    oldActions.resolve([action]);
    oldWorkflows.resolve([workflow]);
    await Promise.resolve();
    expect(screen.queryByText("Review QC")).not.toBeInTheDocument();
  });

  it("ignores a completed save after the selected project changes", async () => {
    const pending = deferred<QuickAction>();
    api.saveQuickAction.mockReturnValue(pending.promise);
    const onChanged = vi.fn();
    const view = render(<QuickActionsSettings selectedProject={project("project-1", "Old")} locale="en-US" onChanged={onChanged} />);
    await screen.findByText("No quick actions yet.");
    fireEvent.click(screen.getByRole("button", { name: "New quick action" }));
    fireEvent.change(screen.getByLabelText("Quick action name"), { target: { value: "Old project action" } });
    fireEvent.click(screen.getByRole("button", { name: "Save quick action" }));
    await waitFor(() => expect(api.saveQuickAction).toHaveBeenCalledTimes(1));

    view.rerender(<QuickActionsSettings selectedProject={project("project-2", "New")} locale="en-US" onChanged={onChanged} />);
    await screen.findByText("No quick actions yet.");
    pending.resolve(action);
    await Promise.resolve();
    expect(screen.queryByText("Old project action")).not.toBeInTheDocument();
    expect(onChanged).not.toHaveBeenCalled();
  });

  it("closes its editor before a parent window Escape layer", async () => {
    const parentClose = vi.fn();
    function Host() {
      const [open] = useState(true);
      useWindowEscapeLayer(open, parentClose);
      return <QuickActionsSettings selectedProject={project("project-1", "PBMC")} locale="en-US" />;
    }
    render(<Host />);
    await screen.findByText("No quick actions yet.");
    fireEvent.click(screen.getByRole("button", { name: "New quick action" }));

    fireEvent.keyDown(window, { key: "Escape" });
    expect(screen.queryByLabelText("Quick action name")).not.toBeInTheDocument();
    expect(parentClose).not.toHaveBeenCalled();
    fireEvent.keyDown(window, { key: "Escape" });
    expect(parentClose).toHaveBeenCalledTimes(1);
  });

  it("requires a selected project and explains the boundary in Chinese", () => {
    render(<QuickActionsSettings selectedProject={null} locale="zh-CN" />);
    expect(screen.getByRole("heading", { name: "快捷操作" })).toBeInTheDocument();
    expect(screen.getByText("选择项目后才能管理绑定到该项目工作流的快捷操作。")).toBeInTheDocument();
    expect(api.listQuickActions).not.toHaveBeenCalled();
  });
});
