import { invoke } from "@tauri-apps/api/core";

import type { ConversationAgentPreferencesV4 } from "./types";

export const DEFAULT_CONVERSATION_AGENT_PREFERENCES: ConversationAgentPreferencesV4 = {
  delegation_enabled: true,
  auto_review: true,
  memory_enabled: true,
};

function isTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

export async function getConversationAgentPreferencesV4(
  projectId: string,
  conversationId: string,
): Promise<ConversationAgentPreferencesV4> {
  if (!isTauri()) return { ...DEFAULT_CONVERSATION_AGENT_PREFERENCES };
  return invoke("conversation_get_agent_preferences_v4", { projectId, conversationId });
}

export async function saveConversationAgentPreferencesV4(
  projectId: string,
  conversationId: string,
  preferences: ConversationAgentPreferencesV4,
): Promise<ConversationAgentPreferencesV4> {
  if (!isTauri()) throw new Error("Conversation preferences require the desktop app.");
  return invoke("conversation_save_agent_preferences_v4", { projectId, conversationId, preferences });
}
