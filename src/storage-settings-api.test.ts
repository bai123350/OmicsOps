import { beforeEach, describe, expect, it, vi } from "vitest";

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));

import { settingsStorageUsage } from "./tauri-api";

describe("settingsStorageUsage", () => {
  beforeEach(() => {
    invoke.mockReset();
    (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = {};
  });

  it("requests the bounded managed scope without sending a path", async () => {
    invoke.mockResolvedValue({ scope: "managed", project_id: null, entries: [] });

    await settingsStorageUsage();

    expect(invoke).toHaveBeenCalledWith("settings_storage_usage", { projectId: null });
  });

  it("selects a project by durable ID", async () => {
    invoke.mockResolvedValue({ scope: "project", project_id: "project-1", entries: [] });

    await settingsStorageUsage("project-1");

    expect(invoke).toHaveBeenCalledWith("settings_storage_usage", { projectId: "project-1" });
  });
});
