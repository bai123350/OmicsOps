import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { useState } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import type { QuickAction, SpecialistTemplate } from "../../project-template-types";
import type { ComposerWorkflowTemplate, WorkspaceProject } from "../../types";
import { useWindowEscapeLayer } from "../settings/BrowserSettings";
import { ProjectTemplatePicker } from "./ProjectTemplatePicker";

const api = vi.hoisted(() => ({
  listQuickActions: vi.fn(),
  listSpecialistTemplates: vi.fn(),
  listComposerWorkflows: vi.fn(),
}));
vi.mock("../../project-templates-api", () => ({
  listQuickActions: api.listQuickActions,
  listSpecialistTemplates: api.listSpecialistTemplates,
}));
vi.mock("../../composer-workflow-api", () => ({ listComposerWorkflows: api.listComposerWorkflows }));

function project(id: string): WorkspaceProject {
  return {
    id,
    name: `Project ${id}`,
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
  description: "Insert the QC workflow",
  workflow_id: "workflow-1",
  enabled: true,
};
const specialist: SpecialistTemplate = {
  id: "specialist-1",
  project_id: "project-1",
  name: "Methods reviewer",
  description: "Review methods",
  instructions: "Identify unsupported claims.",
  enabled: true,
};

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((next, fail) => { resolve = next; reject = fail; });
  return { promise, resolve, reject };
}

function renderPicker(overrides: Partial<React.ComponentProps<typeof ProjectTemplatePicker>> = {}) {
  const props: React.ComponentProps<typeof ProjectTemplatePicker> = {
    selectedProject: project("project-1"),
    locale: "en-US",
    onSelectWorkflow: vi.fn(),
    onSelectSpecialist: vi.fn(),
    onClose: vi.fn(),
    ...overrides,
  };
  return { ...render(<ProjectTemplatePicker {...props} />), props };
}

beforeEach(() => {
  Object.values(api).forEach((mock) => mock.mockReset());
  api.listQuickActions.mockResolvedValue([action]);
  api.listSpecialistTemplates.mockResolvedValue([specialist]);
  api.listComposerWorkflows.mockResolvedValue([workflow]);
});

describe("ProjectTemplatePicker", () => {
  it("selects the current enabled workflow object once without running or sending", async () => {
    const onSelectWorkflow = vi.fn();
    const onSelectSpecialist = vi.fn();
    const onClose = vi.fn();
    renderPicker({ onSelectWorkflow, onSelectSpecialist, onClose });

    fireEvent.click(await screen.findByRole("button", { name: "Use Review QC" }));
    expect(onSelectWorkflow).toHaveBeenCalledWith(workflow);
    expect(onSelectWorkflow).toHaveBeenCalledTimes(1);
    expect(onSelectSpecialist).not.toHaveBeenCalled();
    expect(onClose).toHaveBeenCalledTimes(1);
    expect(screen.queryByRole("button", { name: /Run|Send|Execute/ })).not.toBeInTheDocument();
  });

  it("returns the visible specialist template and leaves draft composition to its parent", async () => {
    const onSelectWorkflow = vi.fn();
    const onSelectSpecialist = vi.fn();
    const onClose = vi.fn();
    renderPicker({ onSelectWorkflow, onSelectSpecialist, onClose });

    fireEvent.click(await screen.findByRole("button", { name: "Use Methods reviewer" }));
    expect(onSelectSpecialist).toHaveBeenCalledWith(specialist);
    expect(onSelectSpecialist).toHaveBeenCalledTimes(1);
    expect(onSelectWorkflow).not.toHaveBeenCalled();
    expect(onClose).toHaveBeenCalledTimes(1);
    expect(screen.getByText("Adds visible text to your draft; it does not start a sub-agent.")).toBeInTheDocument();
  });

  it("marks disabled actions, disabled specialists, and missing workflows unavailable", async () => {
    api.listQuickActions.mockResolvedValue([
      { ...action, enabled: false },
      { ...action, id: "action-missing", name: "Missing workflow", workflow_id: "missing" },
    ]);
    api.listSpecialistTemplates.mockResolvedValue([{ ...specialist, enabled: false }]);
    renderPicker();

    expect(await screen.findByRole("button", { name: "Review QC unavailable" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Missing workflow unavailable" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Methods reviewer unavailable" })).toBeDisabled();
    expect(screen.getAllByText("Unavailable").length).toBeGreaterThanOrEqual(3);
  });

  it("discards a late project catalog after its project changes", async () => {
    const oldActions = deferred<QuickAction[]>();
    const oldSpecialists = deferred<SpecialistTemplate[]>();
    const oldWorkflows = deferred<ComposerWorkflowTemplate[]>();
    api.listQuickActions.mockReturnValueOnce(oldActions.promise).mockResolvedValueOnce([]);
    api.listSpecialistTemplates.mockReturnValueOnce(oldSpecialists.promise).mockResolvedValueOnce([]);
    api.listComposerWorkflows.mockReturnValueOnce(oldWorkflows.promise).mockResolvedValueOnce([]);
    const props = {
      locale: "en-US" as const,
      onSelectWorkflow: vi.fn(),
      onSelectSpecialist: vi.fn(),
      onClose: vi.fn(),
    };
    const view = render(<ProjectTemplatePicker selectedProject={project("old")} {...props} />);
    view.rerender(<ProjectTemplatePicker selectedProject={project("new")} {...props} />);
    expect(await screen.findByText("No quick actions in this project.")).toBeInTheDocument();

    oldActions.resolve([{ ...action, project_id: "old", name: "Old action" }]);
    oldSpecialists.resolve([{ ...specialist, project_id: "old", name: "Old specialist" }]);
    oldWorkflows.resolve([{ ...workflow, project_id: "old" }]);
    await Promise.resolve();
    expect(screen.queryByText("Old action")).not.toBeInTheDocument();
    expect(screen.queryByText("Old specialist")).not.toBeInTheDocument();
  });

  it("shows a retryable generic load error and recovers", async () => {
    const failed = deferred<QuickAction[]>();
    api.listQuickActions.mockReturnValueOnce(failed.promise).mockResolvedValueOnce([action]);
    renderPicker();
    failed.reject(new Error("private transport detail"));

    expect(await screen.findByRole("alert")).toHaveTextContent("Could not load project templates. Please try again.");
    expect(screen.queryByText("private transport detail")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    expect(await screen.findByRole("button", { name: "Use Review QC" })).toBeInTheDocument();
  });

  it("closes on the first window Escape while leaving a parent layer open", () => {
    const parentClose = vi.fn();
    const onClose = vi.fn();
    function Host() {
      const [open, setOpen] = useState(false);
      useWindowEscapeLayer(true, parentClose);
      return <>
        <button type="button" onClick={() => setOpen(true)}>Open templates</button>
        {open && <ProjectTemplatePicker selectedProject={project("project-1")} locale="en-US" onSelectWorkflow={vi.fn()} onSelectSpecialist={vi.fn()} onClose={() => { onClose(); setOpen(false); }} />}
      </>;
    }
    render(<Host />);
    fireEvent.click(screen.getByRole("button", { name: "Open templates" }));
    fireEvent.keyDown(window, { key: "Escape" });
    expect(onClose).toHaveBeenCalledTimes(1);
    expect(parentClose).not.toHaveBeenCalled();
    fireEvent.keyDown(window, { key: "Escape" });
    expect(parentClose).toHaveBeenCalledTimes(1);
  });

  it("does not load a global catalog when no project is selected", () => {
    renderPicker({ selectedProject: null, locale: "zh-CN" });
    expect(screen.getByText("选择项目后才能使用项目快捷操作和角色模板。")).toBeInTheDocument();
    expect(api.listQuickActions).not.toHaveBeenCalled();
    expect(api.listSpecialistTemplates).not.toHaveBeenCalled();
  });
});
