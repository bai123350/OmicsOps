import { invoke } from "@tauri-apps/api/core";

import type { MemoryFileSummaryV4, MemoryFileV4 } from "./types";

export function listProjectMemoryFiles(projectId: string): Promise<MemoryFileSummaryV4[]> {
  return invoke("list_project_memory_files", { projectId });
}

export function readProjectMemoryFile(projectId: string, name: string): Promise<MemoryFileV4> {
  return invoke("read_project_memory_file", { projectId, name });
}

export function createProjectMemoryFile(projectId: string, name: string, content: string): Promise<MemoryFileV4> {
  return invoke("create_project_memory_file", {
    request: { project_id: projectId, name, content },
  });
}

export function updateProjectMemoryFile(
  projectId: string,
  name: string,
  content: string,
  expectedSha256: string,
): Promise<MemoryFileV4> {
  return invoke("update_project_memory_file", {
    request: { project_id: projectId, name, content, expected_sha256: expectedSha256 },
  });
}

export function deleteProjectMemoryFile(projectId: string, name: string, expectedSha256: string): Promise<boolean> {
  return invoke("delete_project_memory_file", {
    request: { project_id: projectId, name, expected_sha256: expectedSha256 },
  });
}
