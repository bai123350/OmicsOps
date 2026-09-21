import { beforeEach, describe, expect, it, vi } from "vitest";

import {
  settingsListSkillRemovals,
  settingsReadSkillFile,
  settingsRemoveSkill,
  settingsRetrySkillRemoval,
  settingsSkillDetail,
} from "./skill-settings-api";

const invoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({ invoke: (...args: unknown[]) => invoke(...args) }));

describe("skill settings API", () => {
  beforeEach(() => {
    invoke.mockReset();
    Object.defineProperty(window, "__TAURI_INTERNALS__", { configurable: true, value: {} });
  });

  it("uses UUID lookup for details and binds previews to the package hash", async () => {
    invoke.mockResolvedValueOnce({ skill: { id: "skill-1" } });
    await settingsSkillDetail("skill-1");
    expect(invoke).toHaveBeenCalledWith("settings_skill_detail", { skillId: "skill-1" });

    invoke.mockResolvedValueOnce({ relative_path: "SKILL.md", content: "# Safe", redacted: false, package_sha256: "sha" });
    await settingsReadSkillFile("skill-1", "SKILL.md", "sha");
    expect(invoke).toHaveBeenCalledWith("settings_read_skill_file", {
      skillId: "skill-1",
      relativePath: "SKILL.md",
      expectedPackageSha256: "sha",
    });
  });

  it("does not fall back to a web or network implementation", async () => {
    delete (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__;
    await expect(settingsSkillDetail("skill-1")).rejects.toThrow("desktop app");
    expect(invoke).not.toHaveBeenCalled();
  });

  it("binds removal to the skill hash and retries only a persisted operation ID", async () => {
    invoke.mockResolvedValueOnce({ removed_from_library: true, files_removed: false, preserved_files: true, status: "needs_attention", message: "Files preserved" });
    await settingsRemoveSkill("skill-1", "a".repeat(64), "owned_files");
    expect(invoke).toHaveBeenCalledWith("settings_remove_skill", {
      request: { skill_id: "skill-1", expected_package_sha256: "a".repeat(64), mode: "owned_files" },
    });

    invoke.mockResolvedValueOnce([{ operation_id: "operation-1" }]);
    await settingsListSkillRemovals();
    expect(invoke).toHaveBeenCalledWith("settings_list_skill_removals");

    invoke.mockResolvedValueOnce({ status: "removed" });
    await settingsRetrySkillRemoval("operation-1");
    expect(invoke).toHaveBeenCalledWith("settings_retry_skill_removal", { operationId: "operation-1" });
  });
});
