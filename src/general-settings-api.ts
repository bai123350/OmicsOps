import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";

import type { GeneralNativePreferences, GeneralSystemStatus, SystemInterpreterDiagnostics } from "./types";

export function settingsGeneralPreferences(): Promise<GeneralNativePreferences> {
  return invoke("settings_general_preferences");
}

export function settingsSaveGeneralPreferences(preferences: GeneralNativePreferences): Promise<GeneralNativePreferences> {
  return invoke("settings_save_general_preferences", { preferences });
}

export function settingsGeneralSystemStatus(): Promise<GeneralSystemStatus> {
  return invoke("settings_general_system_status");
}

export function settingsProbeSystemInterpreters(): Promise<SystemInterpreterDiagnostics> {
  return invoke("settings_probe_system_interpreters");
}

export async function chooseProjectDirectoryStartingAt(defaultPath?: string | null): Promise<{ path: string | null; usedFallback: boolean }> {
  const options = { directory: true, multiple: false } as const;
  try {
    const selected = await open(defaultPath ? { ...options, defaultPath } : options);
    return { path: typeof selected === "string" ? selected : null, usedFallback: false };
  } catch (error) {
    if (!defaultPath) throw error;
    const selected = await open(options);
    return { path: typeof selected === "string" ? selected : null, usedFallback: true };
  }
}
