import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const { invoke, open } = vi.hoisted(() => ({ invoke: vi.fn(), open: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open }));

import {
  chooseProjectDirectoryStartingAt,
  settingsGeneralPreferences,
  settingsProjectDirectoryStartAvailable,
  settingsGeneralSystemStatus,
  settingsProbeSystemInterpreters,
  settingsSaveGeneralPreferences,
} from "./general-settings-api";
import { chooseProjectDirectory } from "./tauri-api";

beforeEach(() => {
  invoke.mockReset();
  open.mockReset();
});

afterEach(() => {
  Reflect.deleteProperty(window, "__TAURI_INTERNALS__");
});

describe("General settings API", () => {
  it("uses the native preference, status, save, and fixed interpreter commands", async () => {
    invoke.mockResolvedValue({});
    await settingsGeneralPreferences();
    await settingsSaveGeneralPreferences({ project_directory_start: "E:/Science" });
    await settingsGeneralSystemStatus();
    await settingsProbeSystemInterpreters();
    await settingsProjectDirectoryStartAvailable("E:/Science");
    expect(invoke.mock.calls).toEqual([
      ["settings_general_preferences"],
      ["settings_save_general_preferences", { preferences: { project_directory_start: "E:/Science" } }],
      ["settings_general_system_status"],
      ["settings_probe_system_interpreters"],
      ["settings_project_directory_start_available", { path: "E:/Science" }],
    ]);
  });

  it("falls back to an ordinary picker when a saved start path is no longer usable", async () => {
    invoke.mockResolvedValueOnce(false);
    open.mockResolvedValueOnce("E:/Replacement");
    await expect(chooseProjectDirectoryStartingAt("E:/Missing")).resolves.toEqual({ path: "E:/Replacement", usedFallback: true });
    expect(open).toHaveBeenCalledOnce();
    expect(open).toHaveBeenCalledWith({ directory: true, multiple: false });
  });

  it("uses the persisted start for the project creation picker", async () => {
    Object.defineProperty(window, "__TAURI_INTERNALS__", { value: {}, configurable: true });
    invoke.mockResolvedValueOnce({ project_directory_start: "E:/Science" }).mockResolvedValueOnce(true);
    open.mockResolvedValueOnce("E:/Science/PBMC");
    await expect(chooseProjectDirectory()).resolves.toEqual({ path: "E:/Science/PBMC", usedFallback: false });
    expect(open).toHaveBeenCalledWith({ directory: true, multiple: false, defaultPath: "E:/Science" });
  });
});
