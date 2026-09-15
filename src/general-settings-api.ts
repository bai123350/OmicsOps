import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";

import type { GeneralNativePreferences, GeneralSystemStatus, SystemInterpreterDiagnostics } from "./types";

export interface ProjectDirectoryChoice {
  path: string | null;
  usedFallback: boolean;
}

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

export function settingsProjectDirectoryStartAvailable(path: string): Promise<boolean> {
  return invoke("settings_project_directory_start_available", { path });
}

export async function chooseProjectDirectoryStartingAt(defaultPath?: string | null): Promise<ProjectDirectoryChoice> {
  const options = { directory: true, multiple: false } as const;
  if (defaultPath && !(await settingsProjectDirectoryStartAvailable(defaultPath).catch(() => false))) {
    const selected = await open(options);
    return { path: typeof selected === "string" ? selected : null, usedFallback: true };
  }
  try {
    const selected = await open(defaultPath ? { ...options, defaultPath } : options);
    return { path: typeof selected === "string" ? selected : null, usedFallback: false };
  } catch (error) {
    if (!defaultPath) throw error;
    const selected = await open(options);
    return { path: typeof selected === "string" ? selected : null, usedFallback: true };
  }
}
