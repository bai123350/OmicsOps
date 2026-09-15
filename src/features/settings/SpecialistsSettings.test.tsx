import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { useState } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import type { SpecialistTemplate } from "../../project-template-types";
import type { WorkspaceProject } from "../../types";
import { useWindowEscapeLayer } from "./BrowserSettings";
import { SpecialistsSettings } from "./SpecialistsSettings";

const api = vi.hoisted(() => ({
  listSpecialistTemplates: vi.fn(),
  saveSpecialistTemplate: vi.fn(),
  deleteSpecialistTemplate: vi.fn(),
}));
vi.mock("../../project-templates-api", () => api);

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

const specialist: SpecialistTemplate = {
  id: "specialist-1",
  project_id: "project-1",
  name: "Methods reviewer",
  description: "Review methods",
  instructions: "Identify unsupported claims in the visible draft.",
  enabled: true,
};

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((next) => { resolve = next; });
  return { promise, resolve };
}

beforeEach(() => {
  Object.values(api).forEach((mock) => mock.mockReset());
  api.listSpecialistTemplates.mockResolvedValue([]);
  api.saveSpecialistTemplate.mockResolvedValue(specialist);
  api.deleteSpecialistTemplate.mockResolvedValue(undefined);
});

describe("SpecialistsSettings", () => {
  it("creates a visible project role template without model or tool controls", async () => {
    const onChanged = vi.fn();
    render(<SpecialistsSettings selectedProject={project("project-1", "PBMC")} locale="en-US" onChanged={onChanged} />);
    await screen.findByText("No role templates yet.");
    expect(screen.getByText("Role templates insert visible text into your draft. Review it, edit it, and send it yourself.")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "New role template" }));
    fireEvent.change(screen.getByLabelText("Role template name"), { target: { value: "Methods reviewer" } });
    fireEvent.change(screen.getByLabelText("Role template description"), { target: { value: "Review methods" } });
    fireEvent.change(screen.getByLabelText("Instructions"), { target: { value: "Identify unsupported claims in the visible draft." } });
    fireEvent.click(screen.getByRole("button", { name: "Save role template" }));

    await waitFor(() => expect(api.saveSpecialistTemplate).toHaveBeenCalledWith({
      id: null,
      project_id: "project-1",
      name: "Methods reviewer",
      description: "Review methods",
      instructions: "Identify unsupported claims in the visible draft.",
      enabled: true,
    }));
    expect(onChanged).toHaveBeenCalledTimes(1);
    expect(screen.queryByLabelText(/model|tool/i)).not.toBeInTheDocument();
  });

  it("edits enabled state and confirms deletion inline", async () => {
    api.listSpecialistTemplates.mockResolvedValue([specialist]);
    api.saveSpecialistTemplate.mockResolvedValue({ ...specialist, enabled: false });
    const onChanged = vi.fn();
    render(<SpecialistsSettings selectedProject={project("project-1", "PBMC")} locale="en-US" onChanged={onChanged} />);
    await screen.findByText("Methods reviewer");
    fireEvent.click(screen.getByRole("button", { name: "Edit Methods reviewer" }));
    fireEvent.click(screen.getByRole("checkbox", { name: "Enabled" }));
    fireEvent.click(screen.getByRole("button", { name: "Save role template" }));
    await waitFor(() => expect(api.saveSpecialistTemplate).toHaveBeenCalledWith(expect.objectContaining({ id: "specialist-1", enabled: false })));

    fireEvent.click(screen.getByRole("button", { name: "Delete Methods reviewer" }));
    expect(screen.getByText("Delete Methods reviewer? This cannot be undone.")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Confirm delete Methods reviewer" }));
    await waitFor(() => expect(api.deleteSpecialistTemplate).toHaveBeenCalledWith("project-1", "specialist-1"));
    expect(onChanged).toHaveBeenCalledTimes(2);
  });

  it("retains all user text after a save failure", async () => {
    api.saveSpecialistTemplate.mockRejectedValue(new Error("private transport detail"));
    render(<SpecialistsSettings selectedProject={project("project-1", "PBMC")} locale="en-US" />);
    await screen.findByText("No role templates yet.");
    fireEvent.click(screen.getByRole("button", { name: "New role template" }));
    fireEvent.change(screen.getByLabelText("Role template name"), { target: { value: "Keep name" } });
    fireEvent.change(screen.getByLabelText("Role template description"), { target: { value: "Keep description" } });
    fireEvent.change(screen.getByLabelText("Instructions"), { target: { value: "Keep complete instructions" } });
    fireEvent.click(screen.getByRole("button", { name: "Save role template" }));

    expect(await screen.findByRole("alert")).toHaveTextContent("Could not save role template. Check the fields and try again.");
    expect(screen.queryByText("private transport detail")).not.toBeInTheDocument();
    expect(screen.getByLabelText("Role template name")).toHaveValue("Keep name");
    expect(screen.getByLabelText("Role template description")).toHaveValue("Keep description");
    expect(screen.getByLabelText("Instructions")).toHaveValue("Keep complete instructions");
  });

  it("locks the editor and submits a create only once while saving", async () => {
    const pending = deferred<SpecialistTemplate>();
    api.saveSpecialistTemplate.mockReturnValue(pending.promise);
    render(<SpecialistsSettings selectedProject={project("project-1", "PBMC")} locale="en-US" />);
    await screen.findByText("No role templates yet.");
    fireEvent.click(screen.getByRole("button", { name: "New role template" }));
    fireEvent.change(screen.getByLabelText("Role template name"), { target: { value: "Stable name" } });
    fireEvent.change(screen.getByLabelText("Instructions"), { target: { value: "Stable instructions" } });
    const save = screen.getByRole("button", { name: "Save role template" });
    fireEvent.click(save);
    fireEvent.submit(save.closest("form")!);

    expect(api.saveSpecialistTemplate).toHaveBeenCalledTimes(1);
    expect(screen.getByLabelText("Role template name")).toBeDisabled();
    expect(screen.getByLabelText("Role template description")).toBeDisabled();
    expect(screen.getByLabelText("Instructions")).toBeDisabled();
    pending.resolve(specialist);
    await waitFor(() => expect(screen.queryByLabelText("Role template name")).not.toBeInTheDocument());
  });

  it("does not start a create while the initial catalog snapshot is pending", async () => {
    const pending = deferred<SpecialistTemplate[]>();
    api.listSpecialistTemplates.mockReturnValue(pending.promise);
    render(<SpecialistsSettings selectedProject={project("project-1", "PBMC")} locale="en-US" />);

    expect(screen.getByRole("button", { name: "New role template" })).toBeDisabled();
    pending.resolve([]);
    await waitFor(() => expect(screen.getByRole("button", { name: "New role template" })).toBeEnabled());
  });

  it("ignores late list and save completions after changing projects", async () => {
    const oldList = deferred<SpecialistTemplate[]>();
    const oldSave = deferred<SpecialistTemplate>();
    const onChanged = vi.fn();
    api.listSpecialistTemplates
      .mockReturnValueOnce(oldList.promise)
      .mockResolvedValueOnce([]);
    const view = render(<SpecialistsSettings selectedProject={project("old", "Old")} locale="en-US" onChanged={onChanged} />);
    view.rerender(<SpecialistsSettings selectedProject={project("project-1", "New")} locale="en-US" onChanged={onChanged} />);
    await screen.findByText("No role templates yet.");
    oldList.resolve([{ ...specialist, project_id: "old", name: "Old specialist" }]);
    await Promise.resolve();
    expect(screen.queryByText("Old specialist")).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "New role template" }));
    fireEvent.change(screen.getByLabelText("Role template name"), { target: { value: "Pending specialist" } });
    fireEvent.change(screen.getByLabelText("Instructions"), { target: { value: "Pending instructions" } });
    api.saveSpecialistTemplate.mockReturnValueOnce(oldSave.promise);
    fireEvent.click(screen.getByRole("button", { name: "Save role template" }));
    await waitFor(() => expect(api.saveSpecialistTemplate).toHaveBeenCalledTimes(1));
    view.rerender(<SpecialistsSettings selectedProject={project("newer", "Newer")} locale="en-US" onChanged={onChanged} />);
    await screen.findByText("No role templates yet.");
    oldSave.resolve(specialist);
    await Promise.resolve();
    expect(screen.queryByText("Pending specialist")).not.toBeInTheDocument();
    expect(onChanged).not.toHaveBeenCalled();
  });

  it("closes its editor before a parent window Escape layer", async () => {
    const parentClose = vi.fn();
    function Host() {
      const [open] = useState(true);
      useWindowEscapeLayer(open, parentClose);
      return <SpecialistsSettings selectedProject={project("project-1", "PBMC")} locale="en-US" />;
    }
    render(<Host />);
    await screen.findByText("No role templates yet.");
    fireEvent.click(screen.getByRole("button", { name: "New role template" }));
    fireEvent.keyDown(window, { key: "Escape" });
    expect(screen.queryByLabelText("Role template name")).not.toBeInTheDocument();
    expect(parentClose).not.toHaveBeenCalled();
    fireEvent.keyDown(window, { key: "Escape" });
    expect(parentClose).toHaveBeenCalledTimes(1);
  });

  it("requires a project and explains the visible-draft boundary in Chinese", () => {
    render(<SpecialistsSettings selectedProject={null} locale="zh-CN" />);
    expect(screen.getByRole("heading", { name: "专家角色" })).toBeInTheDocument();
    expect(screen.getByText("角色模板只会把可见文本插入草稿。请检查、编辑并自行发送。" )).toBeInTheDocument();
    expect(screen.getByText("选择项目后才能管理该项目的角色模板。")).toBeInTheDocument();
    expect(api.listSpecialistTemplates).not.toHaveBeenCalled();
  });
});
