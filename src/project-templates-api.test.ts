import { beforeEach, describe, expect, it, vi } from "vitest";

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));

import {
  deleteQuickAction,
  deleteSpecialistTemplate,
  listQuickActions,
  listSpecialistTemplates,
  saveQuickAction,
  saveSpecialistTemplate,
} from "./project-templates-api";

beforeEach(() => {
  invoke.mockReset();
  (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = {};
});

describe("project templates API", () => {
  it("uses project-scoped quick action commands", async () => {
    invoke.mockResolvedValue(undefined);
    const request = {
      id: null,
      project_id: "project-1",
      name: "Review QC",
      description: "Insert the workflow",
      workflow_id: "workflow-1",
      enabled: true,
    };

    await listQuickActions("project-1");
    await saveQuickAction(request);
    await deleteQuickAction("project-1", "action-1");

    expect(invoke.mock.calls).toEqual([
      ["list_quick_actions", { projectId: "project-1" }],
      ["save_quick_action", { request }],
      ["delete_quick_action", { projectId: "project-1", id: "action-1" }],
    ]);
  });

  it("uses project-scoped specialist commands", async () => {
    invoke.mockResolvedValue(undefined);
    const request = {
      id: "specialist-1",
      project_id: "project-1",
      name: "Methods reviewer",
      description: "Review methods",
      instructions: "Identify unsupported claims.",
      enabled: false,
    };

    await listSpecialistTemplates("project-1");
    await saveSpecialistTemplate(request);
    await deleteSpecialistTemplate("project-1", "specialist-1");

    expect(invoke.mock.calls).toEqual([
      ["list_specialist_templates", { projectId: "project-1" }],
      ["save_specialist_template", { request }],
      ["delete_specialist_template", { projectId: "project-1", id: "specialist-1" }],
    ]);
  });

  it("does not pretend project template mutations work outside the desktop app", async () => {
    delete (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__;

    await expect(listQuickActions("project-1")).resolves.toEqual([]);
    await expect(listSpecialistTemplates("project-1")).resolves.toEqual([]);
    await expect(saveQuickAction({
      id: null,
      project_id: "project-1",
      name: "Review QC",
      description: "",
      workflow_id: "workflow-1",
      enabled: true,
    })).rejects.toThrow("desktop app");
    await expect(deleteSpecialistTemplate("project-1", "specialist-1")).rejects.toThrow("desktop app");
    expect(invoke).not.toHaveBeenCalled();
  });
});
