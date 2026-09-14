import { useEffect, useRef, useState } from "react";
import { getSideChatTurn, listSideChatTurns, sendSideChatTurn } from "../../side-chat-api";
import type { SideChatSendRequestV4, SideChatTurnV4 } from "../../types";

export type SideChatInput = Pick<SideChatSendRequestV4, "question_markdown" | "references" | "attachments" | "parent_request_id">;
type Pending = { request: SideChatSendRequestV4; running: boolean };
type Entry = { records: SideChatTurnV4[]; loaded: boolean; loading: boolean; error: boolean; modelId?: string; pending?: Pending; version: number; draft: string; hasMore: boolean; loadingOlder: boolean; oldest?: { createdAt: string; requestId: string }; readInFlight?: Promise<void> };
const active = (turn: SideChatTurnV4) => turn.status === "queued" || turn.status === "running";
// SQLite cursors use milliseconds; RFC3339 strings may omit the fractional part.
function compareTime(left: string, right: string): number {
  const difference = Date.parse(left) - Date.parse(right);
  return Number.isNaN(difference) ? left.localeCompare(right) : difference;
}
function merge(records: SideChatTurnV4[], incoming: SideChatTurnV4[]) {
  const values = new Map(records.map((turn) => [turn.request_id, turn]));
  for (const turn of incoming) {
    const previous = values.get(turn.request_id);
    const recency = previous ? compareTime(turn.updated_at, previous.updated_at) : 1;
    if (!previous || recency > 0 || (recency === 0 && (active(previous) || !active(turn)))) values.set(turn.request_id, turn);
  }
  return [...values.values()].sort((a, b) => compareTime(a.created_at, b.created_at) || a.request_id.localeCompare(b.request_id));
}
function enqueueRead(entry: Entry, operation: () => Promise<void>): Promise<void> {
  const previous = entry.readInFlight ?? Promise.resolve();
  let current!: Promise<void>;
  current = previous
    .catch(() => undefined)
    .then(operation)
    .catch(() => undefined)
    .finally(() => {
      if (entry.readInFlight === current) entry.readInFlight = undefined;
    });
  entry.readInFlight = current;
  return current;
}
export type SideChatController = ReturnType<typeof useSideChat>;
export function useSideChat(projectId: string | undefined | null, conversationId: string | undefined | null, enabled: boolean, initialModelId?: string | null) {
  const cache = useRef(new Map<string, Entry>());
  const mounted = useRef(true);
  const [, render] = useState(0);
  const scope = `${projectId ?? ""}:${conversationId ?? ""}`;
  const current = useRef(scope); current.current = scope;
  if (!cache.current.has(scope)) cache.current.set(scope, { records: [], loaded: false, loading: enabled, error: false, modelId: initialModelId ?? undefined, version: 0, draft: "", hasMore: false, loadingOlder: false });
  const entry = cache.current.get(scope)!;
  if (!entry.modelId && initialModelId) entry.modelId = initialModelId;
  const update = () => { if (mounted.current && current.current === scope) render((value) => value + 1); };
  const belongs = (turn: SideChatTurnV4) => turn.project_id === projectId && turn.conversation_id === conversationId && turn.id === turn.request_id;
  useEffect(() => { mounted.current = true; return () => { mounted.current = false; }; }, []);
  function refresh(): Promise<void> {
    if (!enabled || !projectId || !conversationId) return Promise.resolve();
    if (!entry.loaded) { entry.loading = true; update(); }
    return enqueueRead(entry, async () => {
      const version = entry.version;
      try {
        const turns = await listSideChatTurns(projectId, conversationId);
        if (!turns.every(belongs)) throw new Error("Side chat scope mismatch");
        const pageSize = turns.length;
        const pageOldest = merge([], turns)[0];
        const headGap = pageSize === 50 && entry.records.length > 0 && !turns.some((turn) => entry.records.some((previous) => previous.request_id === turn.request_id));
        const missingActive = entry.records.filter((turn) => active(turn) && !turns.some((fresh) => fresh.request_id === turn.request_id));
        for (const previous of missingActive) {
          const fresh = await getSideChatTurn(projectId, conversationId, previous.request_id);
          if (fresh && belongs(fresh)) turns.push(fresh);
          else throw new Error("Side chat active receipt unavailable");
        }
        if (entry.version === version) entry.records = merge(entry.records, turns);
        if (!entry.oldest || headGap) {
          if (pageOldest) entry.oldest = { createdAt: pageOldest.created_at, requestId: pageOldest.request_id };
          entry.hasMore = pageSize === 50;
        }
        entry.loaded = true; entry.error = false;
      } catch { entry.error = true; }
      finally { entry.loading = false; update(); }
    });
  }
  useEffect(() => {
    if (!enabled || !projectId || !conversationId) return;
    let polling = false;
    const poll = async () => { if (polling) return; polling = true; try { await refresh(); } finally { polling = false; } };
    void poll();
    const timer = window.setInterval(() => void poll(), 2500);
    return () => window.clearInterval(timer);
  }, [scope, enabled]);
  function loadOlder(): Promise<void> {
    if (!enabled || !projectId || !conversationId || !entry.oldest || !entry.hasMore || entry.loadingOlder) return Promise.resolve();
    entry.loadingOlder = true; update();
    return enqueueRead(entry, async () => {
      try {
        const cursor = entry.oldest;
        if (!cursor || !entry.hasMore) return;
        const turns = await listSideChatTurns(projectId, conversationId, cursor);
        if (!turns.every(belongs)) throw new Error("Side chat history scope mismatch");
        entry.records = merge(entry.records, turns);
        const oldest = merge([], turns)[0];
        if (oldest) entry.oldest = { createdAt: oldest.created_at, requestId: oldest.request_id };
        entry.hasMore = turns.length === 50; entry.error = false;
      } catch { entry.error = true; }
      finally { entry.loadingOlder = false; update(); }
    });
  }
  async function execute(input?: SideChatInput): Promise<boolean> {
    if (!enabled || !projectId || !conversationId) return false;
    if (input) {
      if (entry.pending || !entry.loaded || entry.records.some(active) || !entry.modelId) return false;
      entry.pending = { request: { ...input, project_id: projectId, conversation_id: conversationId, request_id: crypto.randomUUID(), model_profile_id: entry.modelId }, running: false };
    }
    const pending = entry.pending;
    if (!pending || pending.running) return false;
    pending.running = true; entry.error = false; update();
    try {
      const turn = (!input ? await getSideChatTurn(projectId, conversationId, pending.request.request_id) : null)
        ?? await sendSideChatTurn(pending.request);
      if (!belongs(turn) || turn.request_id !== pending.request.request_id || turn.model_profile_id !== pending.request.model_profile_id) throw new Error("Side chat receipt mismatch");
      entry.records = merge(entry.records, [turn]); entry.version += 1; entry.pending = undefined;
      return true;
    } catch (error) {
      if (error && typeof error === "object" && "kind" in error && error.kind === "rejected") entry.pending = undefined;
      entry.error = true; return false;
    } finally { pending.running = false; update(); }
  }
  return {
    records: enabled ? entry.records : [], loading: enabled && entry.loading, ready: enabled && entry.loaded,
    busy: Boolean(entry.pending?.running) || entry.records.some(active), pending: Boolean(entry.pending), error: entry.error,
    originalQuestion: entry.pending?.request.question_markdown,
    draft: entry.draft, setDraft: (draft: string) => { entry.draft = draft; update(); },
    hasMore: entry.hasMore, loadingOlder: entry.loadingOlder, loadOlder,
    modelId: entry.modelId, setModelId: (id: string) => { entry.modelId = id; update(); },
    send: (input: SideChatInput) => execute(input), retry: () => execute(), refresh,
  };
}
