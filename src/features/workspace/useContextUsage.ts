import { useEffect, useState } from "react";
import { getContextUsage } from "../../context-usage-api";
import type { ContextUsageSnapshotV4 } from "../../types";
import type { ContextUsageView } from "./ContextUsagePanel";
export function contextUsageView(snapshot: ContextUsageSnapshotV4): ContextUsageView {
  const last = snapshot.last_request;
  return { breakdown: snapshot.breakdown, contextTokens: snapshot.current_context.used_tokens, contextLimit: snapshot.current_context.max_tokens,
    limitSource: snapshot.current_context.limit_source.kind, estimated: snapshot.current_context.estimated,
    inputTokens: last?.input_tokens, outputTokens: last?.output_tokens, reasoningTokens: last?.reasoning_tokens,
    cacheReadTokens: last?.cache_read_input_tokens, cacheCreationTokens: last?.cache_creation_input_tokens,
    observedInput: snapshot.observed_total.input_tokens.known, observedOutput: snapshot.observed_total.output_tokens.known,
    incompleteAttempts: Math.max(snapshot.observed_total.input_tokens.incomplete_attempts, snapshot.observed_total.output_tokens.incomplete_attempts, snapshot.observed_total.unknown_attempts),
    serializedRequestBytes: snapshot.conservative_budget.serialized_request_bytes, imageBoundTokens: snapshot.conservative_budget.image_bound_tokens };
}
export function useContextUsage(projectId: string | null | undefined, conversationId: string | null | undefined, enabled: boolean) {
  const scope = `${projectId ?? ""}:${conversationId ?? ""}`;
  const [saved, setSaved] = useState<{ scope: string; value: ContextUsageView } | null>(null);
  const [error, setError] = useState(false);
  useEffect(() => {
    let active = true; let pending = false; setSaved(null); setError(false);
    if (!enabled || !projectId || !conversationId) return;
    const refresh = async () => {
      if (pending) return; pending = true;
      try {
        const snapshot = await getContextUsage(projectId, conversationId);
        if (snapshot.project_id !== projectId || snapshot.conversation_id !== conversationId) throw new Error("Context usage scope mismatch");
        const next = contextUsageView(snapshot);
        if (active) { setSaved({ scope, value: next }); setError(false); }
      } catch { if (active) setError(true); }
      finally { pending = false; }
    };
    void refresh(); const timer = window.setInterval(() => void refresh(), 5000);
    return () => { active = false; window.clearInterval(timer); };
  }, [projectId, conversationId, enabled]);
  return { value: enabled && saved?.scope === scope ? saved.value : null, error };
}
