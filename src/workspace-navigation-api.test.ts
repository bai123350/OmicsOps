import { beforeEach, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import * as api from "./workspace-navigation-api";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
beforeEach(() => { vi.clearAllMocks(); (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {}; });

it("keeps project ownership and stable write IDs in native requests", async () => {
  const request = { request_id: "r", project_id: "p", group_id: null, name: "RNA" };
  await api.saveConversationGroup(request);
  expect(invoke).toHaveBeenLastCalledWith("workspace_save_group", { request });
  await api.moveConversations({ project_id: "p", group_id: "g", conversation_ids: ["a", "b"] });
  expect(invoke).toHaveBeenLastCalledWith("workspace_move_conversations", { request: { project_id: "p", group_id: "g", conversation_ids: ["a", "b"] } });
  await api.exportWorkspacePublication("p", "paper", 2);
  expect(invoke).toHaveBeenLastCalledWith("workspace_export_publication", { projectId: "p", publicationId: "paper", revision: 2 });
});

it("does not pretend browser-only writes were saved", async () => {
  delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
  await expect(api.listConversationGroups("p")).resolves.toEqual({ groups: [], memberships: [] });
  await expect(api.saveConversationGroup({ request_id: "r", project_id: "p", group_id: null, name: "RNA" })).rejects.toThrow(/desktop/i);
  expect(invoke).not.toHaveBeenCalled();
});

it("rejects an incompatible host response without replacing the session list", async () => {
  vi.mocked(invoke).mockResolvedValueOnce([]);
  await expect(api.listConversationGroups("p")).rejects.toThrow("Invalid session groups");
});

it("sends only the saved selection to exact evidence commands and preserves cancellation", async () => {
  const savedSelection = { project_id: "p", publication_id: "paper", revision: 2, expected_revision_sha256: "a".repeat(64) };
  const report = { selection: savedSelection };
  vi.mocked(invoke).mockResolvedValueOnce(report);
  await expect(api.checkWorkspacePublicationReadiness(savedSelection)).resolves.toBe(report);
  expect(invoke).toHaveBeenLastCalledWith("workspace_check_publication_readiness", { request: savedSelection });
  vi.mocked(invoke).mockResolvedValueOnce(null);
  await expect(api.exportWorkspacePublicationReferencePackage(savedSelection)).resolves.toBeNull();
  expect(invoke).toHaveBeenLastCalledWith("workspace_export_publication_reference_package", { request: savedSelection });
  const receipt = { selection: savedSelection, path: "C:/synthetic study/package.zip", package_sha256: "b".repeat(64), manifest_sha256: "c".repeat(64), size_bytes: 1024 };
  vi.mocked(invoke).mockResolvedValueOnce(receipt);
  await expect(api.exportWorkspacePublicationReferencePackage(savedSelection)).resolves.toBe(receipt);
  delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
  await expect(api.checkWorkspacePublicationReadiness(savedSelection)).rejects.toThrow(/desktop/i);
  await expect(api.exportWorkspacePublicationReferencePackage(savedSelection)).rejects.toThrow(/desktop/i);
});
