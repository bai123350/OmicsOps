import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import type { MemoryFileSummaryV4, MemoryFileV4, WorkspaceProject } from "../../types";
import { MemorySettings } from "./MemorySettings";

const api = vi.hoisted(() => ({
  listProjectMemoryFiles: vi.fn(),
  readProjectMemoryFile: vi.fn(),
  createProjectMemoryFile: vi.fn(),
  updateProjectMemoryFile: vi.fn(),
  deleteProjectMemoryFile: vi.fn(),
}));
vi.mock("../../memory-settings-api", () => api);

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((next) => { resolve = next; });
  return { promise, resolve };
}

const project = (id: string, name: string): WorkspaceProject => ({
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
});

const summary: MemoryFileSummaryV4 = {
  project_id: "p1",
  name: "study.md",
  size_bytes: 12,
  sha256: "hash-1",
};
const file: MemoryFileV4 = { ...summary, content: "first draft" };

beforeEach(() => {
  Object.values(api).forEach((mock) => mock.mockReset());
  api.listProjectMemoryFiles.mockResolvedValue([summary]);
  api.readProjectMemoryFile.mockResolvedValue(file);
  api.createProjectMemoryFile.mockResolvedValue(file);
  api.updateProjectMemoryFile.mockResolvedValue({ ...file, content: "saved", sha256: "hash-2" });
  api.deleteProjectMemoryFile.mockResolvedValue(true);
});

describe("MemorySettings", () => {
  it("reads the selected project's real file and saves with its loaded hash", async () => {
    const onChanged = vi.fn();
    render(<MemorySettings selectedProject={project("p1", "PBMC")} locale="en-US" onChanged={onChanged} />);

    expect(await screen.findByDisplayValue("first draft")).toBeInTheDocument();
    fireEvent.change(screen.getByLabelText("Memory content"), { target: { value: "saved" } });
    fireEvent.click(screen.getByRole("button", { name: "Save changes" }));

    await waitFor(() => expect(api.updateProjectMemoryFile).toHaveBeenCalledWith("p1", "study.md", "saved", "hash-1"));
    expect(onChanged).toHaveBeenCalledTimes(1);
    expect(screen.getByLabelText("Memory content")).toHaveValue("saved");
  });

  it("keeps failed drafts and requires a reload after a hash conflict", async () => {
    api.updateProjectMemoryFile.mockRejectedValue(new Error("memory_conflict: changed"));
    render(<MemorySettings selectedProject={project("p1", "PBMC")} locale="en-US" />);
    expect(await screen.findByDisplayValue("first draft")).toBeInTheDocument();

    fireEvent.change(screen.getByLabelText("Memory content"), { target: { value: "unsaved work" } });
    fireEvent.click(screen.getByRole("button", { name: "Save changes" }));
    expect(await screen.findByText("This file changed on disk. Reload it before saving again.")).toBeInTheDocument();
    expect(screen.getByLabelText("Memory content")).toHaveValue("unsaved work");
    expect(screen.getByRole("button", { name: "Save changes" })).toBeDisabled();

    api.readProjectMemoryFile.mockResolvedValue({ ...file, content: "disk version", sha256: "hash-new" });
    fireEvent.click(screen.getByRole("button", { name: "Reload file" }));
    expect(await screen.findByDisplayValue("disk version")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Save changes" })).toBeEnabled();
  });

  it("creates files and confirms deletion inline", async () => {
    api.listProjectMemoryFiles.mockResolvedValue([]);
    api.createProjectMemoryFile.mockResolvedValue({ ...file, content: "new note" });
    const onChanged = vi.fn();
    render(<MemorySettings selectedProject={project("p1", "PBMC")} locale="en-US" onChanged={onChanged} />);
    await screen.findByText("No memory files yet.");

    fireEvent.click(screen.getByRole("button", { name: "New memory file" }));
    fireEvent.change(screen.getByLabelText("Filename"), { target: { value: "study.md" } });
    fireEvent.change(screen.getByLabelText("Memory content"), { target: { value: "new note" } });
    fireEvent.click(screen.getByRole("button", { name: "Create file" }));
    await waitFor(() => expect(api.createProjectMemoryFile).toHaveBeenCalledWith("p1", "study.md", "new note"));

    fireEvent.click(screen.getByRole("button", { name: "Delete file" }));
    expect(screen.getByText("Delete study.md? This cannot be undone.")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Confirm delete" }));
    await waitFor(() => expect(api.deleteProjectMemoryFile).toHaveBeenCalledWith("p1", "study.md", "hash-1"));
    expect(onChanged).toHaveBeenCalledTimes(2);
  });

  it("discards late list responses when the selected project changes", async () => {
    const late = deferred<MemoryFileSummaryV4[]>();
    api.listProjectMemoryFiles
      .mockReturnValueOnce(late.promise)
      .mockResolvedValueOnce([{ ...summary, project_id: "p2", name: "new.md", sha256: "hash-2" }]);
    api.readProjectMemoryFile.mockResolvedValue({ ...file, project_id: "p2", name: "new.md", content: "new project", sha256: "hash-2" });
    const view = render(<MemorySettings selectedProject={project("p1", "Old")} locale="en-US" />);
    view.rerender(<MemorySettings selectedProject={project("p2", "New")} locale="en-US" />);

    expect(await screen.findByDisplayValue("new project")).toBeInTheDocument();
    late.resolve([summary]);
    await Promise.resolve();
    expect(screen.getByDisplayValue("new project")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "study.md" })).not.toBeInTheDocument();
  });

  it("can start a local draft while the initial file list is still loading", () => {
    const late = deferred<MemoryFileSummaryV4[]>();
    api.listProjectMemoryFiles.mockReturnValue(late.promise);
    render(<MemorySettings selectedProject={project("p1", "PBMC")} locale="en-US" />);

    fireEvent.click(screen.getByRole("button", { name: "New memory file" }));
    fireEvent.change(screen.getByLabelText("Filename"), { target: { value: "study.md" } });
    fireEvent.change(screen.getByLabelText("Memory content"), { target: { value: "local draft" } });

    expect(screen.getByRole("button", { name: "Create file" })).toBeEnabled();
  });

  it("shows the project requirement in Chinese without inventing global memory", async () => {
    render(<MemorySettings selectedProject={null} locale="zh-CN" />);
    expect(screen.getByRole("heading", { name: "记忆文件" })).toBeInTheDocument();
    expect(screen.getByText("选择一个项目以管理该项目 .omicsops/memory 目录中的 Markdown 文件。")).toBeInTheDocument();
    expect(api.listProjectMemoryFiles).not.toHaveBeenCalled();
  });
});
