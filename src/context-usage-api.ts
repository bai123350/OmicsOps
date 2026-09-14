import { invoke } from "@tauri-apps/api/core";
import type { ContextUsageSnapshotV4 } from "./types";
export async function getContextUsage(projectId: string, conversationId: string): Promise<ContextUsageSnapshotV4> {
  if (typeof window === "undefined" || !("__TAURI_INTERNALS__" in window)) throw new Error("Context usage requires the desktop app.");
  return invoke("agent_v4_context_usage", { projectId, conversationId });
}
