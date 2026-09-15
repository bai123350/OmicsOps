import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import type { StorageUsageSnapshotV4, WorkspaceProject } from "../../types";
import { StorageSettings } from "./StorageSettings";

const { settingsStorageUsage } = vi.hoisted(() => ({ settingsStorageUsage: vi.fn() }));
vi.mock("../../tauri-api", () => ({ settingsStorageUsage }));

function deferred<T>() {
  let resolve!: (value: T | PromiseLike<T>) => void;
  const promise = new Promise<T>((next) => {
    resolve = next;
  });
  return { promise, resolve };
}

const project: WorkspaceProject = {
  id: "project-1",
  name: "PBMC",
  description: "",
  local_root: "E:/data/PBMC",
  remote_root: null,
  connection_id: null,
  template: "single_cell_rna_seq",
  status: "ready",
  ollama_only: false,
  created_at: "2026-09-15T00:00:00Z",
  updated_at: "2026-09-15T00:00:00Z",
};

const managed: StorageUsageSnapshotV4 = {
  scope: "managed",
  project_id: null,
  entries: [{
    category: "database",
    project_id: null,
    path: "C:/AppData/OmicsOps/omicsops.db",
    known_logical_bytes: 2048,
    status: "complete",
    scanned_entries: 1,
    skipped_links: 0,
    issue: null,
  }],
  known_logical_bytes: 2048,
  status: "complete",
  scanned_entries: 1,
  skipped_links: 0,
  limits: { max_entries: 100_000, max_duration_ms: 2_000 },
};

beforeEach(() => {
  settingsStorageUsage.mockReset();
  settingsStorageUsage.mockResolvedValue(managed);
});

describe("StorageSettings", () => {
  it("loads bounded managed data and refreshes the same real scope", async () => {
    render(<StorageSettings selectedProject={project} locale="en-US" />);

    expect(await screen.findByRole("heading", { name: "Logical file bytes" })).toBeInTheDocument();
    expect(screen.getAllByText("2 KB")).toHaveLength(2);
    expect(screen.getByText("Managed app data and project metadata only")).toBeInTheDocument();
    expect(settingsStorageUsage).toHaveBeenCalledWith(undefined);

    fireEvent.click(screen.getByRole("button", { name: "Refresh storage usage" }));
    await waitFor(() => expect(settingsStorageUsage).toHaveBeenCalledTimes(2));
    expect(settingsStorageUsage).toHaveBeenLastCalledWith(undefined);
  });

  it("scans the selected full project only after the user chooses that scope", async () => {
    const projectReply = deferred<StorageUsageSnapshotV4>();
    settingsStorageUsage
      .mockResolvedValueOnce(managed)
      .mockReturnValueOnce(projectReply.promise);
    render(<StorageSettings selectedProject={project} locale="en-US" />);
    await screen.findByRole("heading", { name: "Logical file bytes" });

    fireEvent.change(screen.getByRole("combobox", { name: "Storage scope" }), {
      target: { value: "project" },
    });

    await waitFor(() => expect(settingsStorageUsage).toHaveBeenLastCalledWith(project.id));
    expect(screen.getByText("Scanning…")).toBeInTheDocument();
    expect(screen.queryByText("Application database")).not.toBeInTheDocument();
    projectReply.resolve({
      ...managed,
      scope: "project",
      project_id: project.id,
      status: "partial",
      known_logical_bytes: 4096,
      entries: [{
        category: "project_root",
        project_id: project.id,
        path: project.local_root,
        known_logical_bytes: null,
        status: "partial",
        scanned_entries: 0,
        skipped_links: 1,
        issue: "unreadable",
      }],
    } satisfies StorageUsageSnapshotV4);
    expect(await screen.findByText("Partial scan")).toBeInTheDocument();
    expect(screen.getByText("Unknown")).toBeInTheDocument();
    expect(screen.getByText("Managed app data plus full local project root: PBMC")).toBeInTheDocument();
    expect(screen.getByText(/100,000 entries or 2,000 ms/)).toBeInTheDocument();
    expect(screen.getByText(/symbolic links and Windows reparse points are not followed/i)).toBeInTheDocument();
  });

  it("does not offer a full-root scan without a selected project", async () => {
    render(<StorageSettings selectedProject={null} locale="zh-CN" />);

    await screen.findByRole("heading", { name: "逻辑文件字节数" });
    expect(screen.queryByRole("option", { name: /完整项目根目录/ })).not.toBeInTheDocument();
    expect(screen.getByText("逻辑文件字节数")).toBeInTheDocument();
  });
});
