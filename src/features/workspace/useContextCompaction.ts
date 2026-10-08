import { useCallback, useLayoutEffect, useRef, useState } from "react";
import { compactContextV4 } from "../../context-usage-api";
import type { CompactContextRequestV4, ContextCompactionReceiptV4 } from "../../types";

/** An uncertain retry keeps its identity; leaving a scope never cancels the host. */
export function useContextCompaction(scope: CompactContextRequestV4 | null, enabled: boolean) {
  const key = scope ? JSON.stringify([scope.project_id, scope.conversation_id, scope.run_id, scope.request_id]) : "";
  const operations = useRef(new Map<string, { request: CompactContextRequestV4; succeeded: boolean }>());
  const pending = useRef(new Set<string>());
  const generation = useRef(0);
  const mounted = useRef(false);
  const [, redraw] = useState(0);
  const [saved, setSaved] = useState<{ key: string; receipt: ContextCompactionReceiptV4 | null; error: boolean } | null>(null);
  useLayoutEffect(() => {
    mounted.current = true; generation.current += 1; setSaved(null);
    return () => { mounted.current = false; generation.current += 1; };
  }, [key, enabled]);
  const compact = useCallback(async () => {
    if (!enabled || !scope || pending.current.has(key)) return;
    let operation = operations.current.get(key);
    if (!operation || operation.succeeded) {
      operation = { request: { ...scope, request_id: operation ? crypto.randomUUID() : scope.request_id }, succeeded: false };
      operations.current.set(key, operation);
    }
    const request = operation.request;
    const captured = generation.current;
    pending.current.add(key); setSaved(null); redraw((n) => n + 1);
    try {
      const receipt = await compactContextV4(request);
      if (receipt.project_id !== request.project_id || receipt.conversation_id !== request.conversation_id
        || receipt.run_id !== request.run_id || receipt.request_id !== request.request_id
        || (receipt.status !== "completed" && receipt.status !== "not_needed")) throw new Error("Unconfirmed compaction");
      if (mounted.current && generation.current === captured) {
        operation.succeeded = true;
        setSaved({ key, receipt, error: false });
      }
    } catch {
      if (mounted.current && generation.current === captured) setSaved({ key, receipt: null, error: true });
    } finally {
      pending.current.delete(key);
      if (mounted.current) redraw((n) => n + 1);
    }
  }, [key, enabled, scope?.project_id, scope?.conversation_id, scope?.run_id, scope?.request_id]);
  return { busy: pending.current.has(key), receipt: enabled && saved?.key === key ? saved.receipt : null,
    error: enabled && saved?.key === key ? saved.error : false, compact };
}
