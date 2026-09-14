import { invoke } from "@tauri-apps/api/core";
import type { ReplaceComposerTurnRequestV4, ComposerReplacementReceiptV4 } from "./types";
import type { ComposerQueueActionRequestV4, ComposerQueueItemV4, EnqueueComposerTurnRequestV4, UpdateComposerQueueRequestV4 } from "./types";

function requireQueueHost() {
  if (typeof window === "undefined" || !("__TAURI_INTERNALS__" in window)) throw new Error("The send queue requires the desktop app.");
}
export async function replaceComposerTurn(request: ReplaceComposerTurnRequestV4): Promise<ComposerReplacementReceiptV4> {
  requireQueueHost();
  return invoke("composer_queue_replace", { request });
}
export async function listComposerQueue(projectId: string, conversationId: string): Promise<ComposerQueueItemV4[]> {
  requireQueueHost();
  return invoke("composer_queue_list", { projectId, conversationId });
}
export async function enqueueComposerTurn(request: EnqueueComposerTurnRequestV4): Promise<ComposerQueueItemV4> {
  requireQueueHost();
  return invoke("composer_queue_enqueue", { request });
}
export async function updateComposerQueue(request: UpdateComposerQueueRequestV4): Promise<ComposerQueueItemV4> {
  requireQueueHost();
  return invoke("composer_queue_update", { request });
}
export async function actComposerQueue(request: ComposerQueueActionRequestV4): Promise<ComposerQueueItemV4[]> {
  requireQueueHost();
  return invoke("composer_queue_action", { request });
}

export async function reconcileComposerQueue(projectId: string, conversationId: string): Promise<ComposerQueueItemV4[]> {
  requireQueueHost();
  return invoke("composer_queue_reconcile", { projectId, conversationId });
}
