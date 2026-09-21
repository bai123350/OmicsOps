import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";

import type { InstalledPlugin, PluginInspection, PluginRemovalResult } from "./types";

export async function choosePluginDirectory(): Promise<string | null> {
  const selected = await open({ directory: true, multiple: false });
  return typeof selected === "string" ? selected : null;
}

export function settingsInspectPlugin(sourcePath: string): Promise<PluginInspection> {
  return invoke("settings_inspect_plugin", { sourcePath });
}

export function settingsInstallPlugin(sourcePath: string, expectedDigest: string, expectedOldDigest?: string | null): Promise<InstalledPlugin> {
  return invoke("settings_install_plugin", {
    request: {
      source_path: sourcePath,
      expected_digest: expectedDigest,
      expected_old_digest: expectedOldDigest ?? null,
    },
  });
}

export function settingsListPlugins(): Promise<InstalledPlugin[]> {
  return invoke("settings_list_plugins");
}

export function settingsSetPluginEnabled(installationId: string, enabled: boolean): Promise<InstalledPlugin> {
  return invoke("settings_set_plugin_enabled", { request: { installation_id: installationId, enabled } });
}

export function settingsRemovePlugin(installationId: string, expectedDigest: string): Promise<PluginRemovalResult> {
  return invoke("settings_remove_plugin", { request: { installation_id: installationId, expected_digest: expectedDigest } });
}
