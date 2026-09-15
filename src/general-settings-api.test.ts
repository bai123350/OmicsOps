import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const { invoke, open } = vi.hoisted(() => ({ invoke: vi.fn(), open: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open }));

import {
  chooseProjectDirectoryStartingAt,
  settingsGeneralPreferences,
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
    expect(invoke.mock.calls).toEqual([
      ["settings_general_preferences"],
      ["settings_save_general_preferences", { preferences: { project_directory_start: "E:/Science" } }],
      ["settings_general_system_status"],
      ["settings_probe_system_interpreters"],
    ]);
  });

  it("falls back to an ordinary picker when a saved start path is no longer usable", async () => {
    open.mockRejectedValueOnce(new Error("missing default path")).mockResolvedValueOnce("E:/Replacement");
    await expect(chooseProjectDirectoryStartingAt("E:/Missing")).resolves.toEqual({ path: "E:/Replacement", usedFallback: true });
    expect(open).toHaveBeenNthCalledWith(1, { directory: true, multiple: false, defaultPath: "E:/Missing" });
    expect(open).toHaveBeenNthCalledWith(2, { directory: true, multiple: false });
  });

  it("uses the persisted start for the project creation picker", async () => {
    Object.defineProperty(window, "__TAURI_INTERNALS__", { value: {}, configurable: true });
    invoke.mockResolvedValueOnce({ project_directory_start: "E:/Science" });
    open.mockResolvedValueOnce("E:/Science/PBMC");
    await expect(chooseProjectDirectory()).resolves.toBe("E:/Science/PBMC");
    expect(open).toHaveBeenCalledWith({ directory: true, multiple: false, defaultPath: "E:/Science" });
  });
});
