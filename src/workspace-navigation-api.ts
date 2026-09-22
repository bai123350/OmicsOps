import { invoke } from "@tauri-apps/api/core";
import type { ConversationGroup, ConversationGroupState, JourneyPage, LibraryDetail, LibraryPage, MoveConversationsRequest, PublicationDetail, PublicationSummary, RestoreWorkspacePublicationRequest, SaveConversationGroupRequest, SaveWorkspaceLibraryItemRequest, SaveWorkspacePublicationRequest, WorkspaceJourneyRequest, WorkspaceLibraryRequest, WorkspaceSourceRef, WorkspaceSourceSnapshot } from "./workspace-navigation-types";

export function isWorkspaceDesktopHost() { return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window; }
async function host<T>(command: string, args: Record<string, unknown>): Promise<T> {
  if (!isWorkspaceDesktopHost()) throw new Error("This action requires the desktop app.");
  return invoke<T>(command, args);
}
export async function listConversationGroups(projectId: string): Promise<ConversationGroupState> {
  if (!isWorkspaceDesktopHost()) return { groups: [], memberships: [] };
  const result = await host<ConversationGroupState>("workspace_list_groups", { projectId });
  if (!result || !Array.isArray(result.groups) || !Array.isArray(result.memberships)) throw new Error("Invalid session groups returned by desktop host.");
  return result;
}
export const saveConversationGroup = (request: SaveConversationGroupRequest) => host<ConversationGroup>("workspace_save_group", { request });
export const deleteConversationGroup = (projectId: string, groupId: string) => host<void>("workspace_delete_group", { projectId, groupId });
export const moveConversations = (request: MoveConversationsRequest) => host<void>("workspace_move_conversations", { request });
export const getWorkspaceJourney = (request: WorkspaceJourneyRequest) => host<JourneyPage>("workspace_journey", { request });
export const getWorkspaceSourceDetail = (source: WorkspaceSourceRef) => host<WorkspaceSourceSnapshot>("workspace_source_detail", { source });
export const listWorkspacePublications = (projectId: string) => host<PublicationSummary[]>("workspace_list_publications", { projectId });
export const getWorkspacePublication = (projectId: string, publicationId: string) => host<PublicationDetail>("workspace_get_publication", { projectId, publicationId });
export const saveWorkspacePublication = (request: SaveWorkspacePublicationRequest) => host<PublicationDetail>("workspace_save_publication", { request });
export const restoreWorkspacePublication = (request: RestoreWorkspacePublicationRequest) => host<PublicationDetail>("workspace_restore_publication", { request });
export const exportWorkspacePublication = (projectId: string, publicationId: string, revision: number) => host<string | null>("workspace_export_publication", { projectId, publicationId, revision });
export const listWorkspaceLibrary = (request: WorkspaceLibraryRequest) => host<LibraryPage>("workspace_list_library", { request });
export const getWorkspaceLibraryItem = (itemId: string) => host<LibraryDetail>("workspace_get_library_item", { itemId });
export const saveWorkspaceLibraryItem = (request: SaveWorkspaceLibraryItemRequest) => host<LibraryDetail>("workspace_save_library_item", { request });
export const deleteWorkspaceLibraryItem = (itemId: string) => host<void>("workspace_delete_library_item", { itemId });
