import { useEffect, useRef, useState } from "react";
import { conversationBranchCheckpoint, createConversationBranchAndSend } from "../../conversation-branch-api";
import type { ConversationBranchV4, ConversationBranchSendReceiptV4, CreateConversationBranchAndSendRequestV4 } from "../../types";
type Input = Omit<CreateConversationBranchAndSendRequestV4, "branch" | "queue_request_id" | "queue_message_id" | "queue_run_id"> & { sourceMessageId: string };
type Pending = { input: Input; request?: CreateConversationBranchAndSendRequestV4; receipt?: ConversationBranchSendReceiptV4; running: boolean };
function titleFor(text: string) {
  let title = ""; const encoder = new TextEncoder();
  for (const character of text.trim().replace(/\s+/g, " ")) { if (encoder.encode(title + character).length > 240) break; title += character; }
  return title || "Conversation branch";
}
export function useConversationBranchSend(projectId: string | undefined, conversationId: string | undefined, onCreated: (branch: ConversationBranchV4) => Promise<boolean>) {
  const pending = useRef(new Map<string, Pending>());
  const mounted = useRef(true);
  useEffect(() => { mounted.current = true; return () => { mounted.current = false; }; }, []);
  const [errorScope, setErrorScope] = useState<string | null>(null);
  const [, render] = useState(0);
  const scope = `${projectId ?? ""}:${conversationId ?? ""}`;
  const current = useRef(scope); current.current = scope;
  const open = useRef(onCreated); open.current = onCreated;
  async function submit(input?: Input): Promise<boolean> {
    if (!projectId || !conversationId) return false;
    let attempt = pending.current.get(scope);
    if (attempt?.running || (attempt && input && JSON.stringify(attempt.input) !== JSON.stringify(input))) { setErrorScope(scope); return false; }
    if (!attempt) { if (!input) return false; attempt = { input, running: false }; pending.current.set(scope, attempt); }
    const saved = attempt; saved.running = true; setErrorScope(null); render((n) => n + 1);
    try {
      if (!saved.receipt) {
        if (!saved.request) {
          const checkpoint = await conversationBranchCheckpoint(projectId, conversationId, saved.input.sourceMessageId, "after_response");
          const { sourceMessageId: _, ...payload } = saved.input;
          saved.request = { ...payload, queue_request_id: crypto.randomUUID(), queue_message_id: crypto.randomUUID(), queue_run_id: crypto.randomUUID(), branch: { request_id: crypto.randomUUID(), project_id: projectId, source_conversation_id: conversationId, source_message_id: checkpoint.source_message_id, checkpoint_kind: checkpoint.checkpoint_kind, expected_source_sequence: checkpoint.source_sequence, expected_head_sequence: checkpoint.source_head_sequence, expected_boundary_hash: checkpoint.boundary_hash, title: titleFor(payload.message_markdown) } };
        }
        saved.receipt = await createConversationBranchAndSend(saved.request);
        if (saved.receipt.branch.project_id !== projectId || saved.receipt.branch.source_conversation_id !== conversationId || saved.receipt.branch.request_id !== saved.request.branch.request_id || saved.receipt.queue.project_id !== projectId || saved.receipt.queue.message_id !== saved.request.queue_message_id || saved.receipt.queue.run_id !== saved.request.queue_run_id || saved.receipt.queue.request_id !== saved.request.queue_request_id || saved.receipt.queue.conversation_id !== saved.receipt.branch.branch_conversation_id) { saved.receipt = undefined; throw new Error("Invalid branch send receipt"); }
      }
      if (!mounted.current || current.current !== scope) return false;
      if (!await open.current(saved.receipt.branch)) throw new Error("Branch navigation not confirmed");
      pending.current.delete(scope); return true;
    } catch {
      // Even a rejected material read may follow committed branch creation.
      // Preserve the complete original request and reconcile it on explicit retry.
      if (!saved.request) pending.current.delete(scope);
      if (mounted.current && current.current === scope) setErrorScope(scope);
      return false;
    } finally { saved.running = false; if (mounted.current) render((n) => n + 1); }
  }
  return { originalMarkdown: pending.current.get(scope)?.input.message_markdown, submit, retry: () => submit(), busy: Boolean(pending.current.get(scope)?.running), pending: pending.current.has(scope), error: errorScope === scope };
}
