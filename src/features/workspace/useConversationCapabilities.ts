import { useCallback, useEffect, useState } from "react";
import { getConversationCapabilitiesV4 } from "../../tauri-api";
import type { ConversationCapabilitiesV4 } from "../../types";

export function useConversationCapabilities(projectId: string | null, conversationId: string | null, revision: string) {
  const [summary, setSummary] = useState<ConversationCapabilitiesV4 | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");
  const [retry, setRetry] = useState(0);
  const refresh = useCallback(() => setRetry((value) => value + 1), []);
  useEffect(() => {
    let disposed = false;
    setSummary(null);
    setError("");
    setLoading(Boolean(projectId && conversationId));
    if (!projectId || !conversationId) return;
    getConversationCapabilitiesV4(projectId, conversationId).then((value) => {
      if (disposed) return;
      if (value.project_id !== projectId || value.conversation_id !== conversationId) {
        throw new Error("Capability snapshot does not match the active conversation");
      }
      setSummary(value);
    }).catch((reason: unknown) => {
      if (!disposed) setError(reason instanceof Error ? reason.message : String(reason));
    }).finally(() => { if (!disposed) setLoading(false); });
    return () => { disposed = true; };
  }, [projectId, conversationId, revision, retry]);
  const matches = summary?.project_id === projectId && summary?.conversation_id === conversationId;
  return { summary: matches ? summary : null, loading, error, refresh };
}
