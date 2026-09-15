import { beforeEach, describe, expect, it, vi } from "vitest";

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));

import {
  createProjectMemoryFile,
  deleteProjectMemoryFile,
  listProjectMemoryFiles,
  readProjectMemoryFile,
  updateProjectMemoryFile,
} from "./memory-settings-api";

beforeEach(() => invoke.mockReset());

describe("memory settings API", () => {
  it("uses project-scoped commands and explicit expected hashes", async () => {
    invoke.mockResolvedValue(undefined);
    await listProjectMemoryFiles("p1");
    await readProjectMemoryFile("p1", "study.md");
    await createProjectMemoryFile("p1", "study.md", "first");
    await updateProjectMemoryFile("p1", "study.md", "second", "hash-1");
    await deleteProjectMemoryFile("p1", "study.md", "hash-2");

    expect(invoke.mock.calls).toEqual([
      ["list_project_memory_files", { projectId: "p1" }],
      ["read_project_memory_file", { projectId: "p1", name: "study.md" }],
      ["create_project_memory_file", { request: { project_id: "p1", name: "study.md", content: "first" } }],
      ["update_project_memory_file", { request: { project_id: "p1", name: "study.md", content: "second", expected_sha256: "hash-1" } }],
      ["delete_project_memory_file", { request: { project_id: "p1", name: "study.md", expected_sha256: "hash-2" } }],
    ]);
  });
});
