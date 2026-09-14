import { afterEach, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { listComposerWorkflows, saveComposerWorkflow } from "./composer-workflow-api";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
afterEach(() => { Reflect.deleteProperty(window, "__TAURI_INTERNALS__"); vi.resetAllMocks(); });
const request = { project_id: "project-a", name: "Evidence review", description: "Collect and verify", steps: ["Collect evidence", "Verify citations"], enabled: true };

it("keeps the preview catalog empty and rejects nonpersistent preview saves", async () => {
  expect(await listComposerWorkflows("project-a")).toEqual([]);
  await expect(saveComposerWorkflow(request)).rejects.toThrow("desktop app");
  expect(invoke).not.toHaveBeenCalled();
});

it("uses the project-owned catalog and propagates save failures without a fake result", async () => {
  Object.defineProperty(window, "__TAURI_INTERNALS__", { value: {}, configurable: true });
  vi.mocked(invoke).mockResolvedValueOnce([]).mockRejectedValueOnce(new Error("Save failed"));
  await listComposerWorkflows("project-a");
  expect(invoke).toHaveBeenLastCalledWith("list_composer_workflows", { projectId: "project-a" });
  await expect(saveComposerWorkflow(request)).rejects.toThrow("Save failed");
  expect(invoke).toHaveBeenLastCalledWith("save_composer_workflow", { request });
});
