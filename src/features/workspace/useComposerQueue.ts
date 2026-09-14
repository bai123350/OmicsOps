import { useEffect, useRef, useState } from "react";
import * as api from "../../composer-queue-api";
import type { ComposerQueueActionRequestV4, ComposerQueueItemV4, EnqueueComposerTurnRequestV4, UpdateComposerQueueRequestV4 } from "../../types";

type QueueInput = Omit<EnqueueComposerTurnRequestV4, "request_id" | "message_id" | "run_id">;
export function useComposerQueue(projectId: string | null | undefined, conversationId: string | null | undefined, enabled: boolean, onRunActivity?: () => Promise<void>) {
  const [items, setItems] = useState<ComposerQueueItemV4[]>([]);
  const [error, setError] = useState(false);
  const [loading, setLoading] = useState(false);
  const scope = `${projectId ?? ""}:${conversationId ?? ""}`;
  const current = useRef(scope); current.current = scope;
  const generation = useRef(0);
  const pending = useRef(new Map<string, EnqueueComposerTurnRequestV4>());
  const submitting = useRef(new Map<string, Promise<boolean>>());
  const snapshot = useRef<ComposerQueueItemV4[]>([]);
  const refreshInFlight = useRef<Promise<void> | null>(null);
  const callback = useRef(onRunActivity); callback.current = onRunActivity;
  const activity = useRef("");
  const activityInFlight = useRef(false);
  function apply(incoming: ComposerQueueItemV4[], captured: number) {
    if (current.current !== scope || captured !== generation.current) return;
    if (!Array.isArray(incoming)) throw new Error("Invalid queue response");
    const scoped = incoming.filter((item) => item.project_id === projectId && item.conversation_id === conversationId);
    snapshot.current = scoped; setItems(scoped); setError(false);
    const signature = scoped.filter((item) => item.status !== "pending" && item.status !== "dispatching").map((item) => `${item.request_id}:${item.status}:${item.revision}`).join("|");
    if (signature && signature !== activity.current && !activityInFlight.current && callback.current) {
      activityInFlight.current = true;
      void callback.current().then(() => {
        if (current.current === scope && captured === generation.current) activity.current = signature;
      }).catch(() => {
        if (current.current === scope && captured === generation.current) setError(true);
      }).finally(() => {
        if (current.current === scope && captured === generation.current) activityInFlight.current = false;
      });
    }
  }
  async function refresh() {
    if (!enabled || !projectId || !conversationId) return;
    if (refreshInFlight.current) return refreshInFlight.current;
    const captured = generation.current;
    const work = (async () => {
      try { apply(await api.reconcileComposerQueue(projectId, conversationId), captured); }
      catch { if (current.current === scope && captured === generation.current) setError(true); }
    })();
    refreshInFlight.current = work;
    try { await work; } finally { if (refreshInFlight.current === work) refreshInFlight.current = null; }
  }
  useEffect(() => {
    generation.current += 1; activity.current = ""; activityInFlight.current = false; snapshot.current = []; setItems([]); setError(false); refreshInFlight.current = null;
    if (!enabled || !projectId || !conversationId) { setLoading(false); return; }
    let active = true; setLoading(true);
    void refresh().finally(() => { if (active) setLoading(false); });
    const timer = window.setInterval(() => void refresh(), 3000);
    return () => { active = false; generation.current += 1; window.clearInterval(timer); refreshInFlight.current = null; };
  }, [scope, enabled]);
  async function enqueue(input: QueueInput): Promise<boolean> {
    if (!enabled || input.project_id !== projectId || input.conversation_id !== conversationId) return false;
    const key = JSON.stringify(input);
    const existingWork = submitting.current.get(key);
    if (existingWork) return existingWork;
    const captured = generation.current;
    let request = pending.current.get(key);
    if (!request) { request = { ...input, request_id: crypto.randomUUID(), message_id: crypto.randomUUID(), run_id: crypto.randomUUID() }; pending.current.set(key, request); }
    const saved = request;
    const work = (async () => {
      try {
        const receipt = snapshot.current.find((item) => item.request_id === saved.request_id) ?? await api.enqueueComposerTurn(saved);
        if (receipt.project_id !== saved.project_id || receipt.conversation_id !== saved.conversation_id || receipt.request_id !== saved.request_id) throw new Error("Invalid queue receipt");
        pending.current.delete(key);
        if (current.current === scope && captured === generation.current) apply([...snapshot.current.filter((item) => item.request_id !== receipt.request_id), receipt].sort((a,b) => a.position-b.position), captured);
        if (receipt.status === "cancelled" && !receipt.cut_in_message_id) {
          if (current.current === scope && captured === generation.current) setError(true);
          return false;
        }
        return true;
      } catch { if (current.current === scope && captured === generation.current) setError(true); return false; }
    })();
    submitting.current.set(key, work);
    void work.finally(() => { if (submitting.current.get(key) === work) submitting.current.delete(key); });
    return work;
  }
  async function update(request: UpdateComposerQueueRequestV4) {
    if (request.project_id !== projectId || request.conversation_id !== conversationId) throw new Error("Queue scope changed");
    const captured = generation.current;
    const result = await api.updateComposerQueue(request);
    if (current.current === scope && captured === generation.current) apply(snapshot.current.map((item) => item.request_id === result.request_id ? result : item), captured);
  }
  async function action(request: ComposerQueueActionRequestV4) {
    if (request.project_id !== projectId || request.conversation_id !== conversationId) throw new Error("Queue scope changed");
    const captured = generation.current;
    const result = await api.actComposerQueue(request);
    if (request.action === "cancel") for (const [key, pendingRequest] of pending.current) if (pendingRequest.request_id === request.request_id) pending.current.delete(key);
    apply(result, captured);
  }
  return { items, error, loading, enqueue, update, action, refresh };
}
