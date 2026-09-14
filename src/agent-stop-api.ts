import { invoke } from "@tauri-apps/api/core";

import type { StopRunReceiptV4, StopRunRequestV4 } from "./types";

/** A run identity used by the scoped stop commands. */
export interface AgentStopScope {
  projectId: string;
  conversationId: string;
  runId: string;
}

export function isDesktopStopHost(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

/**
 * Request a local Agent V4 run stop. A browser preview cannot make this
 * durable host request, so it rejects instead of pretending the run stopped.
 */
export async function agentV4RequestStop(request: StopRunRequestV4): Promise<StopRunReceiptV4> {
  if (!isDesktopStopHost()) throw new Error("Agent stop requires the desktop app.");
  return invoke<StopRunReceiptV4>("agent_v4_request_stop", { request });
}

/** Read the durable stop receipt for one project/conversation/run scope. */
export async function agentV4GetStop(
  projectId: string,
  conversationId: string,
  runId: string,
): Promise<StopRunReceiptV4 | null> {
  if (!isDesktopStopHost()) return null;
  return invoke<StopRunReceiptV4 | null>("agent_v4_get_stop", { projectId, conversationId, runId });
}
