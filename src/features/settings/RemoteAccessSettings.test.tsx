import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import type { SyncEntry, WorkspaceProject } from "../../types";
import { RemoteAccessSettings } from "./RemoteAccessSettings";

const api = vi.hoisted(() => ({
  cancelSyncTransfer: vi.fn(),
  chooseProjectFiles: vi.fn(),
  downloadProjectFile: vi.fn(),
  listSyncEntries: vi.fn(),
  pauseSyncTransfer: vi.fn(),
  retrySyncTransfer: vi.fn(),
  uploadSelectedFiles: vi.fn(),
}));

vi.mock("../../tauri-api", () => api);

function deferred<T>() {
  let resolve!: (value: T | PromiseLike<T>) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((next, fail) => {
    resolve = next;
    reject = fail;
  });
  return { promise, reject, resolve };
}

const project = (id: string, bound = true): WorkspaceProject => ({
  id,
  name: id === "project-1" ? "PBMC atlas" : "Spatial atlas",
  description: "",
  local_root: `E:/omics/${id}`,
  remote_root: bound ? `/srv/omics/${id}` : null,
  connection_id: bound ? `ssh-${id}` : null,
  template: "single_cell_rna_seq",
  status: "ready",
  ollama_only: false,
  created_at: "2026-09-15T00:00:00Z",
  updated_at: "2026-09-15T00:00:00Z",
});

const entry = (
  id: string,
  state: SyncEntry["state"],
  overrides: Partial<SyncEntry> = {},
): SyncEntry => ({
  id,
  project_id: "project-1",
  relative_path: `results/${id}.tsv`,
  local_relative_path: `results/${id}.tsv`,
  remote_path: `/srv/omics/project-1/results/${id}.tsv`,
  direction: "local_to_remote",
  size_bytes: 4096,
  sha256: "abc123",
  state,
  transferred_bytes: state === "synced" ? 4096 : 1024,
  retry_count: 0,
  error: state === "failed" ? "connection dropped" : null,
  updated_at: "2026-09-15T10:00:00Z",
  ...overrides,
});

beforeEach(() => {
  vi.clearAllMocks();
  api.listSyncEntries.mockResolvedValue([]);
  api.pauseSyncTransfer.mockResolvedValue(undefined);
  api.cancelSyncTransfer.mockResolvedValue(undefined);
  api.retrySyncTransfer.mockResolvedValue(entry("retry", "transferring"));
  api.chooseProjectFiles.mockResolvedValue([]);
  api.uploadSelectedFiles.mockResolvedValue([]);
});

describe("RemoteAccessSettings", () => {
  it("offers the real Environments route when no project or SSH binding is available", () => {
    const onOpenEnvironments = vi.fn();
    const view = render(
      <RemoteAccessSettings selectedProject={null} locale="en-US" onOpenEnvironments={onOpenEnvironments} />,
    );

    expect(screen.getByText(/open a project to manage its SSH file transfers/i)).toBeInTheDocument();
    expect(api.listSyncEntries).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Open Environments" }));
    expect(onOpenEnvironments).toHaveBeenCalledTimes(1);

    view.rerender(
      <RemoteAccessSettings selectedProject={project("project-1", false)} locale="en-US" onOpenEnvironments={onOpenEnvironments} />,
    );
    expect(screen.getByText(/does not have an SSH connection and remote root/i)).toBeInTheDocument();
    expect(api.listSyncEntries).not.toHaveBeenCalled();

    view.rerender(
      <RemoteAccessSettings selectedProject={null} locale="zh-CN" onOpenEnvironments={onOpenEnvironments} />,
    );
    expect(screen.getByRole("heading", { name: "远程访问" })).toBeInTheDocument();
    expect(screen.getByText("管理当前项目本地目录与已绑定 SSH 主机之间的文件传输、状态和恢复操作。")).toBeInTheDocument();
  });

  it("shows real project-bound transfer details and only actions allowed by each state", async () => {
    api.listSyncEntries.mockResolvedValue([
      entry("active", "transferring"),
      entry("broken", "failed", { direction: "remote_to_local" }),
      entry("done", "synced"),
    ]);

    render(<RemoteAccessSettings selectedProject={project("project-1")} locale="en-US" onOpenEnvironments={() => undefined} />);

    expect(await screen.findByText("results/active.tsv")).toBeInTheDocument();
    expect(screen.getAllByText("Local → SSH")).toHaveLength(2);
    expect(screen.getByText("SSH → local")).toBeInTheDocument();
    expect(screen.getByText(/Remote: \/srv\/omics\/project-1\/results\/broken\.tsv/)).toBeInTheDocument();
    expect(screen.getByText("connection dropped")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Pause results/active.tsv" })).toBeEnabled();
    expect(screen.getByRole("button", { name: "Cancel results/active.tsv" })).toBeEnabled();
    expect(screen.getByRole("button", { name: "Retry results/broken.tsv" })).toBeEnabled();
    expect(screen.queryByRole("button", { name: "Retry results/done.tsv" })).not.toBeInTheDocument();
  });

  it("discards a late snapshot after the selected project changes", async () => {
    const oldSnapshot = deferred<SyncEntry[]>();
    api.listSyncEntries
      .mockReturnValueOnce(oldSnapshot.promise)
      .mockResolvedValueOnce([
        entry("new-project", "synced", {
          project_id: "project-2",
          relative_path: "results/spatial.tsv",
          remote_path: "/srv/omics/project-2/results/spatial.tsv",
        }),
      ]);
    const view = render(
      <RemoteAccessSettings selectedProject={project("project-1")} locale="en-US" onOpenEnvironments={() => undefined} />,
    );

    view.rerender(
      <RemoteAccessSettings selectedProject={project("project-2")} locale="en-US" onOpenEnvironments={() => undefined} />,
    );
    expect(await screen.findByText("results/spatial.tsv")).toBeInTheDocument();
    oldSnapshot.resolve([entry("old-project", "failed")]);

    await waitFor(() => expect(screen.queryByText("results/old-project.tsv")).not.toBeInTheDocument());
    expect(screen.getByText("results/spatial.tsv")).toBeInTheDocument();
  });

  it("keeps records visible after pause, cancel, and retry failures and prevents duplicate retry", async () => {
    const retry = deferred<SyncEntry>();
    api.listSyncEntries.mockResolvedValue([
      entry("active", "transferring"),
      entry("broken", "failed"),
    ]);
    api.pauseSyncTransfer.mockRejectedValue(new Error("pause signal rejected"));
    api.cancelSyncTransfer.mockRejectedValue(new Error("cancel signal rejected"));
    api.retrySyncTransfer.mockReturnValue(retry.promise);
    render(<RemoteAccessSettings selectedProject={project("project-1")} locale="en-US" onOpenEnvironments={() => undefined} />);

    fireEvent.click(await screen.findByRole("button", { name: "Pause results/active.tsv" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("pause signal rejected");
    expect(screen.getByText("results/active.tsv")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Cancel results/active.tsv" }));
    await waitFor(() => expect(screen.getByRole("alert")).toHaveTextContent("cancel signal rejected"));
    expect(screen.getByText("results/active.tsv")).toBeInTheDocument();

    const retryButton = await screen.findByRole("button", { name: "Retry results/broken.tsv" });

    fireEvent.click(retryButton);
    fireEvent.click(retryButton);
    expect(api.retrySyncTransfer).toHaveBeenCalledTimes(1);
    retry.reject(new Error("resume dispatch uncertain"));

    expect(await screen.findByRole("alert")).toHaveTextContent("resume dispatch uncertain");
    expect(screen.getByText("results/broken.tsv")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Retry results/broken.tsv" })).toBeEnabled();
  });

  it("does not let an older refresh overwrite a successful retry", async () => {
    const refresh = deferred<SyncEntry[]>();
    const failed = entry("broken", "failed");
    const resumed = entry("broken", "transferring", { error: null, updated_at: "2026-09-15T10:01:00Z" });
    api.listSyncEntries.mockResolvedValueOnce([failed]).mockReturnValueOnce(refresh.promise);
    api.retrySyncTransfer.mockResolvedValue(resumed);
    render(<RemoteAccessSettings selectedProject={project("project-1")} locale="en-US" onOpenEnvironments={() => undefined} />);
    expect(await screen.findByText("Failed")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Refresh transfer records" }));
    fireEvent.click(screen.getByRole("button", { name: "Retry results/broken.tsv" }));
    expect(await screen.findByText("Transferring")).toBeInTheDocument();

    await act(async () => {
      refresh.resolve([failed]);
      await refresh.promise;
    });
    expect(screen.getByText("Transferring")).toBeInTheDocument();
    expect(screen.queryByText("Failed")).not.toBeInTheDocument();
  });

  it("passes only the selected project's root and relative paths to upload", async () => {
    const uploading = deferred<SyncEntry[]>();
    api.chooseProjectFiles.mockResolvedValue(["data/counts.tsv"]);
    api.uploadSelectedFiles.mockReturnValue(uploading.promise);
    render(<RemoteAccessSettings selectedProject={project("project-1")} locale="en-US" onOpenEnvironments={() => undefined} />);
    await waitFor(() => expect(api.listSyncEntries).toHaveBeenCalledWith("project-1"));

    const upload = screen.getByRole("button", { name: "Choose project files to upload" });
    fireEvent.click(upload);
    fireEvent.click(upload);
    await waitFor(() => expect(api.chooseProjectFiles).toHaveBeenCalledTimes(1));
    expect(api.chooseProjectFiles).toHaveBeenCalledWith("E:/omics/project-1");
    expect(api.uploadSelectedFiles).toHaveBeenCalledWith("project-1", ["data/counts.tsv"]);
    uploading.resolve([entry("uploaded", "synced", { relative_path: "data/counts.tsv" })]);
    expect(await screen.findByText(/1 file transfer finished/i)).toBeInTheDocument();
  });

  it("does not dispatch files returned by a picker after the project changes", async () => {
    const selection = deferred<string[]>();
    api.chooseProjectFiles.mockReturnValue(selection.promise);
    const view = render(
      <RemoteAccessSettings selectedProject={project("project-1")} locale="en-US" onOpenEnvironments={() => undefined} />,
    );
    await waitFor(() => expect(api.listSyncEntries).toHaveBeenCalledWith("project-1"));

    fireEvent.click(screen.getByRole("button", { name: "Choose project files to upload" }));
    view.rerender(
      <RemoteAccessSettings selectedProject={project("project-2")} locale="en-US" onOpenEnvironments={() => undefined} />,
    );
    selection.resolve(["data/old-project.tsv"]);

    await waitFor(() => expect(api.chooseProjectFiles).toHaveBeenCalledTimes(1));
    await waitFor(() => expect(api.uploadSelectedFiles).not.toHaveBeenCalled());
  });

  it("keeps the returned conflict entry and explains that cancel affects file transfer only", async () => {
    const conflict = entry("conflict-copy", "conflict", {
      direction: "remote_to_local",
      relative_path: "results/counts.conflict-1.tsv",
      local_relative_path: "results/counts.conflict-1.tsv",
    });
    api.downloadProjectFile.mockResolvedValue({ entry: conflict, conflict: true });
    render(<RemoteAccessSettings selectedProject={project("project-1")} locale="en-US" onOpenEnvironments={() => undefined} />);
    await waitFor(() => expect(api.listSyncEntries).toHaveBeenCalledWith("project-1"));

    fireEvent.change(screen.getByLabelText("Remote project-relative path"), {
      target: { value: "results/counts.tsv" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Download from SSH" }));

    expect(await screen.findByText("results/counts.conflict-1.tsv")).toBeInTheDocument();
    expect(screen.getByText(/local file differed.*conflict copy/i)).toBeInTheDocument();
    expect(screen.getByText(/canceling here stops only the selected file transfer/i)).toBeInTheDocument();
    expect(api.downloadProjectFile).toHaveBeenCalledWith("project-1", "results/counts.tsv");
  });
});
