import { beforeEach, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import * as api from "./workspace-navigation-api";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
beforeEach(() => { vi.clearAllMocks(); (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {}; });

it("uses the five exact baseline commands with native nullable request fields", async () => {
  const create = { request_id: "r", project_id: "p", branch_conversation_id: null, title: "Before analysis", files: [{ backend_id: "local", relative_path: "my script.py", expected_full_sha256: null }] };
  await api.createWorkspaceFileBaseline(create);
  expect(invoke).toHaveBeenLastCalledWith("workspace_create_file_baseline", { request: create });
  const list = { project_id: "p", branch_conversation_id: null, offset: 0, limit: 20 };
  await api.listWorkspaceFileBaselines(list);
  expect(invoke).toHaveBeenLastCalledWith("workspace_list_file_baselines", { request: list });
  await api.getWorkspaceFileBaseline("p", "b");
  expect(invoke).toHaveBeenLastCalledWith("workspace_get_file_baseline", { projectId: "p", baselineId: "b" });
  const diff = { project_id: "p", baseline_id: "b", expected_manifest_sha256: "a".repeat(64) };
  await api.diffWorkspaceFileBaseline(diff);
  expect(invoke).toHaveBeenLastCalledWith("workspace_diff_file_baseline", { request: diff });
  const read = { ...diff, entry_index: 0 };
  await api.readWorkspaceBaselineText(read);
  expect(invoke).toHaveBeenLastCalledWith("workspace_read_baseline_text", { request: read });
  delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
  for (const call of [() => api.createWorkspaceFileBaseline(create), () => api.listWorkspaceFileBaselines(list), () => api.getWorkspaceFileBaseline("p", "b"), () => api.diffWorkspaceFileBaseline(diff), () => api.readWorkspaceBaselineText(read)]) await expect(call()).rejects.toThrow(/desktop/i);
});

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

it("visibility_api_exact_payloads", async () => {
  const list = { project_id: "p", visibility: "shelved" as const, query: "RNA", offset: 50, limit: 50 };
  const page = { items: [], next_offset: null };
  vi.mocked(invoke).mockResolvedValueOnce(page);
  await expect(api.listConversationVisibility(list)).resolves.toBe(page);
  expect(invoke).toHaveBeenLastCalledWith("workspace_list_conversation_visibility", { request: list });
  const set = { request_id: "r", project_id: "p", conversation_id: "hidden", expected_revision: 7, visibility: "visible" as const };
  const receipt = { request_id: "r", state: { project_id: "p", conversation_id: "hidden", visibility: "visible", revision: 8, changed_at: null }, changed: true };
  vi.mocked(invoke).mockResolvedValueOnce(receipt);
  await expect(api.setConversationVisibility(set)).resolves.toBe(receipt);
  expect(invoke).toHaveBeenLastCalledWith("workspace_set_conversation_visibility", { request: set });
  const current = { ...receipt.state, revision: 10 };
  vi.mocked(invoke).mockResolvedValueOnce(current);
  await expect(api.getConversationVisibility("p", "hidden")).resolves.toBe(current);
  expect(invoke).toHaveBeenLastCalledWith("workspace_get_conversation_visibility", { projectId: "p", conversationId: "hidden" });
  const conversation = { id: "hidden", project_id: "p", title: "RNA", status: "running", model_profile_id: null, created_at: "2026-10-04T00:00:00Z", updated_at: "2026-10-04T00:00:00Z" };
  vi.mocked(invoke).mockResolvedValueOnce(conversation);
  await expect(api.getWorkspaceConversation("p", "hidden")).resolves.toBe(conversation);
  expect(invoke).toHaveBeenLastCalledWith("workspace_get_conversation", { projectId: "p", conversationId: "hidden" });
  delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
  for (const call of [() => api.listConversationVisibility(list), () => api.setConversationVisibility(set), () => api.getConversationVisibility("p", "hidden"), () => api.getWorkspaceConversation("p", "hidden")]) await expect(call()).rejects.toThrow(/desktop/i);
});

it("visibility_safe_error preserves the typed native error for safe consumer rendering", async () => {
  const error = { code: "not_found", message: "Conversation unavailable." };
  vi.mocked(invoke).mockRejectedValueOnce(error);
  await expect(api.getConversationVisibility("p", "missing")).rejects.toBe(error);
});
