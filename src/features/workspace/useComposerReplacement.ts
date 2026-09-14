import { useEffect, useRef, useState } from "react";
import { replaceComposerTurn } from "../../composer-queue-api";
import type { EnqueueComposerTurnRequestV4, ReplaceComposerTurnRequestV4 } from "../../types";

type Input = Omit<EnqueueComposerTurnRequestV4, "request_id" | "message_id" | "run_id">;
export type ReplacementTarget = { run_id: string; sequence: number; event_hash: string };
type Entry = { request?: ReplaceComposerTurnRequestV4; busy: boolean; error: boolean; work?: Promise<boolean> };
export function useComposerReplacement(projectId: string | null | undefined, conversationId: string | null | undefined, enabled: boolean, onAccepted?: () => Promise<void>) {
  const entries = useRef(new Map<string, Entry>());
  const [, render] = useState(0);
  const scope = `${projectId ?? ""}:${conversationId ?? ""}`;
  const current = useRef(scope); current.current = scope;
  const mounted = useRef(true);
  useEffect(() => { mounted.current = true; return () => { mounted.current = false; }; }, []);
  let entry = entries.current.get(scope);
  if (!entry) { entry = { busy: false, error: false }; entries.current.set(scope, entry); }
  const captured = entry;
  function notify() { if (mounted.current && current.current === scope) render((value) => value + 1); }
  async function submit(request: ReplaceComposerTurnRequestV4): Promise<boolean> {
    if (captured.work) return captured.work;
    captured.busy = true; captured.error = false; notify();
    const work = (async () => {
      try {
        const receipt = await replaceComposerTurn(request);
        if (receipt.request_id !== request.turn.request_id || receipt.project_id !== request.turn.project_id || receipt.conversation_id !== request.turn.conversation_id || receipt.target_run_id !== request.target_run_id || receipt.source_event_sequence !== request.expected_event_sequence || receipt.source_event_hash !== request.expected_event_hash) throw new Error("Invalid replacement receipt");
        captured.request = undefined;
        // Refresh errors cannot undo durable acceptance or authorize a replay.
        if (current.current === scope) void onAccepted?.().catch(() => {});
        return true;
      } catch (error) {
        if (error && typeof error === "object" && "kind" in error && error.kind === "rejected") captured.request = undefined;
        captured.error = true;
        return false;
      } finally { captured.busy = false; captured.work = undefined; notify(); }
    })();
    captured.work = work;
    return work;
  }
  async function send(input: Input, target: ReplacementTarget): Promise<boolean> {
    if (!enabled || input.project_id !== projectId || input.conversation_id !== conversationId || captured.request || captured.busy) return false;
    const request: ReplaceComposerTurnRequestV4 = {
      turn: { ...input, references: [...input.references], attachments: [...input.attachments], request_id: crypto.randomUUID(), message_id: crypto.randomUUID(), run_id: crypto.randomUUID() },
      target_run_id: target.run_id, expected_event_sequence: target.sequence, expected_event_hash: target.event_hash, stop_request_id: crypto.randomUUID(),
    };
    captured.request = request;
    return submit(request);
  }
  async function retry(): Promise<boolean> {
    if (!enabled || !captured.request) return false;
    return submit(captured.request);
  }
  return { send, retry, busy: captured.busy, pending: Boolean(captured.request), error: captured.error, originalTurn: captured.request?.turn };
}
