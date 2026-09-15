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

  it("filters the existing memory list by filename without searching file contents", async () => {
    const other = { ...summary, name: "analysis-notes.md", size_bytes: 20, sha256: "hash-other" };
    api.listProjectMemoryFiles.mockResolvedValue([summary, other]);
    render(<MemorySettings selectedProject={project("p1", "PBMC")} locale="en-US" />);
    expect(await screen.findByRole("button", { name: /analysis-notes\.md/ })).toBeInTheDocument();

    fireEvent.change(screen.getByRole("searchbox", { name: "Filter memory files" }), { target: { value: "STUDY" } });
    expect(screen.getByRole("button", { name: /study\.md/ })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /analysis-notes\.md/ })).not.toBeInTheDocument();

    fireEvent.change(screen.getByRole("searchbox", { name: "Filter memory files" }), { target: { value: "first draft" } });
    expect(screen.getByRole("status")).toHaveTextContent("No matching filenames.");
    expect(screen.getByLabelText("Memory content")).toHaveValue("first draft");
  });

  it("locks new-file and file navigation while a save is pending", async () => {
    const other = { ...summary, name: "other.md", size_bytes: 20, sha256: "hash-other" };
    const saving = deferred<MemoryFileV4>();
    api.listProjectMemoryFiles.mockResolvedValue([summary, other]);
    api.updateProjectMemoryFile.mockReturnValue(saving.promise);
    render(<MemorySettings selectedProject={project("p1", "PBMC")} locale="en-US" />);
    expect(await screen.findByDisplayValue("first draft")).toBeInTheDocument();

    fireEvent.change(screen.getByLabelText("Memory content"), { target: { value: "saved draft" } });
    fireEvent.click(screen.getByRole("button", { name: "Save changes" }));

    expect(screen.getByRole("button", { name: "New memory file" })).toBeDisabled();
    expect(screen.getByRole("button", { name: /other\.md/ })).toBeDisabled();
    saving.resolve({ ...file, content: "saved draft", sha256: "hash-2" });
    await waitFor(() => expect(screen.getByRole("button", { name: "New memory file" })).toBeEnabled());
  });

  it("locks the filename and content while creating so a late receipt cannot overwrite new input", async () => {
    const saving = deferred<MemoryFileV4>();
    api.listProjectMemoryFiles.mockResolvedValue([]);
    api.createProjectMemoryFile.mockReturnValue(saving.promise);
    render(<MemorySettings selectedProject={project("p1", "PBMC")} locale="en-US" />);
    await screen.findByText("No memory files yet.");

    fireEvent.click(screen.getByRole("button", { name: "New memory file" }));
    fireEvent.change(screen.getByLabelText("Filename"), { target: { value: "pending.md" } });
    fireEvent.change(screen.getByLabelText("Memory content"), { target: { value: "pending draft" } });
    fireEvent.click(screen.getByRole("button", { name: "Create file" }));

    expect(screen.getByLabelText("Filename")).toBeDisabled();
    expect(screen.getByLabelText("Memory content")).toBeDisabled();
    saving.resolve({ ...file, name: "pending.md", content: "pending draft" });
    await waitFor(() => expect(screen.queryByLabelText("Filename")).not.toBeInTheDocument());
    expect(screen.getByLabelText("Memory content")).toBeEnabled();
  });

  it("locks file navigation while deletion is pending", async () => {
    const other = { ...summary, name: "other.md", size_bytes: 20, sha256: "hash-other" };
    const deleting = deferred<boolean>();
    api.listProjectMemoryFiles.mockResolvedValue([summary, other]);
    api.deleteProjectMemoryFile.mockReturnValue(deleting.promise);
    render(<MemorySettings selectedProject={project("p1", "PBMC")} locale="en-US" />);
    expect(await screen.findByDisplayValue("first draft")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Delete file" }));
    fireEvent.click(screen.getByRole("button", { name: "Confirm delete" }));

    expect(screen.getByRole("button", { name: "New memory file" })).toBeDisabled();
    expect(screen.getByRole("button", { name: /other\.md/ })).toBeDisabled();
    deleting.resolve(true);
    await waitFor(() => expect(screen.getByRole("button", { name: "New memory file" })).toBeEnabled());
  });

  it("resets mutation state on project change and ignores the old completion", async () => {
    const saving = deferred<MemoryFileV4>();
    const onChanged = vi.fn();
    api.listProjectMemoryFiles.mockResolvedValueOnce([summary]).mockResolvedValueOnce([]);
    api.updateProjectMemoryFile.mockReturnValue(saving.promise);
    const view = render(<MemorySettings selectedProject={project("p1", "Old")} locale="en-US" onChanged={onChanged} />);
    expect(await screen.findByDisplayValue("first draft")).toBeInTheDocument();

    fireEvent.change(screen.getByLabelText("Memory content"), { target: { value: "old save" } });
    fireEvent.click(screen.getByRole("button", { name: "Save changes" }));
    view.rerender(<MemorySettings selectedProject={project("p2", "New")} locale="en-US" onChanged={onChanged} />);

    expect(await screen.findByText("No memory files yet.")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "New memory file" })).toBeEnabled();
    fireEvent.click(screen.getByRole("button", { name: "New memory file" }));
    fireEvent.change(screen.getByLabelText("Filename"), { target: { value: "new.md" } });
    fireEvent.change(screen.getByLabelText("Memory content"), { target: { value: "new project draft" } });
    expect(screen.getByRole("button", { name: "Create file" })).toBeEnabled();

    saving.resolve({ ...file, content: "old save", sha256: "hash-old" });
    await Promise.resolve();
    expect(screen.getByLabelText("Memory content")).toHaveValue("new project draft");
    expect(screen.getByRole("button", { name: "Create file" })).toBeEnabled();
    expect(onChanged).not.toHaveBeenCalled();
  });

  it("shows the project requirement in Chinese without inventing global memory", async () => {
    render(<MemorySettings selectedProject={null} locale="zh-CN" />);
    expect(screen.getByRole("heading", { name: "记忆文件" })).toBeInTheDocument();
    expect(screen.getByText("选择一个项目以管理该项目 .omicsops/memory 目录中的 Markdown 文件。")).toBeInTheDocument();
    expect(api.listProjectMemoryFiles).not.toHaveBeenCalled();
  });
});
