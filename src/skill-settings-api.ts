import { invoke } from "@tauri-apps/api/core";

import type { SkillFilePreview, SkillRemovalMode, SkillRemovalOperation, SkillRemovalResult, SkillSettingsDetail } from "./types";

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

export async function settingsRemoveSkill(
  skillId: string,
  expectedPackageSha256: string,
  mode: SkillRemovalMode,
): Promise<SkillRemovalResult> {
  requireDesktop();
  return invoke("settings_remove_skill", {
    request: { skill_id: skillId, expected_package_sha256: expectedPackageSha256, mode },
  });
}

export async function settingsListSkillRemovals(): Promise<SkillRemovalOperation[]> {
  requireDesktop();
  return invoke("settings_list_skill_removals");
}

export async function settingsRetrySkillRemoval(operationId: string): Promise<SkillRemovalResult> {
  requireDesktop();
  return invoke("settings_retry_skill_removal", { operationId });
}
