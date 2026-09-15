import { invoke } from "@tauri-apps/api/core";

import type { SkillFilePreview, SkillSettingsDetail } from "./types";

function requireDesktop() {
  if (typeof window === "undefined" || !("__TAURI_INTERNALS__" in window)) {
    throw new Error("Skill settings require the desktop app.");
  }
}

export async function settingsSkillDetail(skillId: string): Promise<SkillSettingsDetail> {
  requireDesktop();
  return invoke("settings_skill_detail", { skillId });
}

export async function settingsReadSkillFile(
  skillId: string,
  relativePath: string,
  expectedPackageSha256: string,
): Promise<SkillFilePreview> {
  requireDesktop();
  return invoke("settings_read_skill_file", { skillId, relativePath, expectedPackageSha256 });
}
