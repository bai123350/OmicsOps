import { invoke } from "@tauri-apps/api/core";

import type {
  QuickAction,
  SaveQuickActionRequest,
  SaveSpecialistTemplateRequest,
  SpecialistTemplate,
} from "./project-template-types";

function desktopAvailable() {
  return "__TAURI_INTERNALS__" in window;
}

export async function listQuickActions(projectId: string): Promise<QuickAction[]> {
  if (!desktopAvailable()) return [];
  return invoke("list_quick_actions", { projectId });
}

export async function saveQuickAction(request: SaveQuickActionRequest): Promise<QuickAction> {
  if (!desktopAvailable()) throw new Error("Quick action editing requires the desktop app.");
  return invoke("save_quick_action", { request });
}

export async function deleteQuickAction(projectId: string, id: string): Promise<void> {
  if (!desktopAvailable()) throw new Error("Quick action editing requires the desktop app.");
  return invoke("delete_quick_action", { projectId, id });
}

export async function listSpecialistTemplates(projectId: string): Promise<SpecialistTemplate[]> {
  if (!desktopAvailable()) return [];
  return invoke("list_specialist_templates", { projectId });
}

export async function saveSpecialistTemplate(
  request: SaveSpecialistTemplateRequest,
): Promise<SpecialistTemplate> {
  if (!desktopAvailable()) throw new Error("Specialist template editing requires the desktop app.");
  return invoke("save_specialist_template", { request });
}

export async function deleteSpecialistTemplate(projectId: string, id: string): Promise<void> {
  if (!desktopAvailable()) throw new Error("Specialist template editing requires the desktop app.");
  return invoke("delete_specialist_template", { projectId, id });
}
