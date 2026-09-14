import { invoke } from "@tauri-apps/api/core";
import type { ConversationBranchCheckpointKindV4, ConversationBranchCheckpointV4, ConversationBranchV4, CreateConversationBranchRequestV4 } from "./types";
function requireHost() {
  if (typeof window === "undefined" || !("__TAURI_INTERNALS__" in window)) throw new Error("Conversation branching requires the desktop app.");
}
export async function conversationBranchCheckpoint(projectId: string, sourceConversationId: string, sourceMessageId: string, checkpointKind: ConversationBranchCheckpointKindV4): Promise<ConversationBranchCheckpointV4> {
  requireHost(); return invoke("conversation_branch_checkpoint_v4", { projectId, sourceConversationId, sourceMessageId, checkpointKind });
}
export async function createConversationBranch(request: CreateConversationBranchRequestV4): Promise<ConversationBranchV4> {
  requireHost(); return invoke("conversation_branch_create_v4", { request });
}
export async function getConversationBranch(projectId: string, branchConversationId: string): Promise<ConversationBranchV4 | null> {
  requireHost(); return invoke("conversation_branch_get_v4", { projectId, branchConversationId });
}
export async function listConversationBranches(projectId: string, sourceConversationId: string): Promise<ConversationBranchV4[]> {
  requireHost(); return invoke("conversation_branch_list_v4", { projectId, sourceConversationId });
}

export async function createConversationBranchAndSend(request: import("./types").CreateConversationBranchAndSendRequestV4): Promise<import("./types").ConversationBranchSendReceiptV4> {
  if (typeof window === "undefined" || !("__TAURI_INTERNALS__" in window)) throw new Error("Branch sending requires the desktop app.");
  return invoke("conversation_branch_create_and_send_v4", { request });
}
