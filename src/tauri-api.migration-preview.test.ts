import { beforeEach, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { previewProjectMigration } from "./tauri-api";
import type { ProjectMigrationPreviewRequestV1 } from "./types";
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
beforeEach(() => { vi.clearAllMocks(); Object.defineProperty(window, "__TAURI_INTERNALS__", { configurable: true, value: {} }); });
it("invokes the exact native migration preview payload, preserving null expectation", async () => {
  const request: ProjectMigrationPreviewRequestV1 = { project_id: "project", categories: ["scientific_metadata"], files: [{ backend_id: "local", relative_path: "script.py", expected_full_sha256: null }] };
  const response = { preview_sha256: "fingerprint" };
  vi.mocked(invoke).mockResolvedValue(response);
  expect(await previewProjectMigration(request)).toBe(response);
  expect(invoke).toHaveBeenCalledTimes(1); expect(invoke).toHaveBeenCalledWith("workspace_preview_project_migration", { request });
});
it("rejects outside the desktop host without inventing a preview", async () => {
  delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
  await expect(previewProjectMigration({ project_id: "project", categories: [], files: [] })).rejects.toThrow();
  expect(invoke).not.toHaveBeenCalled();
});
