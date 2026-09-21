import { beforeEach, describe, expect, it, vi } from "vitest";

import {
  choosePluginDirectory,
  settingsInstallPlugin,
  settingsRemovePlugin,
  settingsSetPluginEnabled,
} from "./integration-packages-api";

const invoke = vi.fn();
const open = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({ invoke: (...args: unknown[]) => invoke(...args) }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: (...args: unknown[]) => open(...args) }));

describe("integration packages API", () => {
  beforeEach(() => {
    invoke.mockReset();
    open.mockReset();
  });

  it("chooses only one directory and binds installation to inspected digests", async () => {
    open.mockResolvedValue("C:/Lab/qc-plugin");
    expect(await choosePluginDirectory()).toBe("C:/Lab/qc-plugin");
    expect(open).toHaveBeenCalledWith({ directory: true, multiple: false });

    invoke.mockResolvedValue({ installation_id: "plugin-1" });
    await settingsInstallPlugin("C:/Lab/qc-plugin", "new-sha", "old-sha");
    expect(invoke).toHaveBeenCalledWith("settings_install_plugin", {
      request: {
        source_path: "C:/Lab/qc-plugin",
        expected_digest: "new-sha",
        expected_old_digest: "old-sha",
      },
    });
  });

  it("uses installation IDs for lifecycle changes and digest-binds removal", async () => {
    invoke.mockResolvedValue({ installation_id: "plugin-1" });
    await settingsSetPluginEnabled("plugin-1", true);
    expect(invoke).toHaveBeenCalledWith("settings_set_plugin_enabled", {
      request: { installation_id: "plugin-1", enabled: true },
    });

    await settingsRemovePlugin("plugin-1", "reviewed-sha");
    expect(invoke).toHaveBeenLastCalledWith("settings_remove_plugin", {
      request: { installation_id: "plugin-1", expected_digest: "reviewed-sha" },
    });
  });
});
