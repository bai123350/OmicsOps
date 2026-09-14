import { invoke } from "@tauri-apps/api/core";
import type { SideChatSendRequestV4, SideChatTurnV4 } from "./types";
function requireHost() {
  if (typeof window === "undefined" || !("__TAURI_INTERNALS__" in window)) throw { kind: "rejected", message: "Side chat requires the desktop app." };
}
export async function sendSideChatTurn(request: SideChatSendRequestV4): Promise<SideChatTurnV4> {
  requireHost(); return invoke("side_chat_send_v4", { request });
}
export async function listSideChatTurns(projectId: string, conversationId: string, before?: { createdAt: string; requestId: string }): Promise<SideChatTurnV4[]> {
  requireHost(); return invoke("side_chat_list_v4", { projectId, conversationId, limit: 50, ...(before ? { beforeCreatedAt: before.createdAt, beforeRequestId: before.requestId } : {}) });
}
export async function getSideChatTurn(projectId: string, conversationId: string, requestId: string): Promise<SideChatTurnV4 | null> {
  requireHost(); return invoke("side_chat_get_v4", { projectId, conversationId, requestId });
}
