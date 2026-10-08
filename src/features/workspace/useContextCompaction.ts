import { useCallback, useLayoutEffect, useRef, useState } from "react";
import { compactContextV4 } from "../../context-usage-api";
import type { CompactContextRequestV4, ContextCompactionReceiptV4 } from "../../types";

type Operation = {
  request: CompactContextRequestV4;
  pending: Promise<void> | null;
  receipt: ContextCompactionReceiptV4 | null;
  error: boolean;
};
type WindowLedger = { operations: Map<string, Operation>; listeners: Set<() => void> };
// Back to projects destroys WorkspaceShell, but not this window. Keep only safe
// operation metadata here, never prompts, answers, raw exceptions or model state.
const windowLedgers = new WeakMap<Window, WindowLedger>();
function windowLedger(): WindowLedger {
  let ledger = windowLedgers.get(window);
  if (!ledger) {
    ledger = { operations: new Map(), listeners: new Set() };
    windowLedgers.set(window, ledger);
  }
  return ledger;
}
function notify(ledger: WindowLedger) {
  for (const listener of ledger.listeners) listener();
}

/** An uncertain retry keeps its identity; leaving a scope never cancels the host. */
export function useContextCompaction(scope: CompactContextRequestV4 | null, enabled: boolean) {
  // A remounted shell may supply a new seed ID. Existing window operations own
  // their identity; only a new explicit attempt after visible success rotates it.
  const key = scope ? JSON.stringify([scope.project_id, scope.conversation_id, scope.run_id]) : "";
  const ledger = windowLedger();
  const generation = useRef(0);
  const mounted = useRef(false);
  const [, redraw] = useState(0);
  const [saved, setSaved] = useState<{ key: string; receipt: ContextCompactionReceiptV4 | null; error: boolean } | null>(null);
  useLayoutEffect(() => {
    mounted.current = true; generation.current += 1;
    const operation = ledger.operations.get(key);
    setSaved(enabled && operation && !operation.pending
      ? { key, receipt: operation.receipt, error: operation.error } : null);
    const changed = () => redraw((n) => n + 1);
    ledger.listeners.add(changed);
    return () => {
      ledger.listeners.delete(changed); mounted.current = false; generation.current += 1;
    };
  }, [ledger, key, enabled]);
  const compact = useCallback(async () => {
    if (!enabled || !scope) return;
    let operation = ledger.operations.get(key);
    if (operation?.pending) return;
    if (operation?.receipt && saved?.receipt !== operation.receipt) {
      // A late offscreen success stays recoverable without dispatching again.
      // Revealing that original receipt is distinct from a new explicit attempt.
      setSaved({ key, receipt: operation.receipt, error: false });
      return;
    }
    if (!operation || operation.receipt) {
      operation = { request: { ...scope, request_id: operation ? crypto.randomUUID() : scope.request_id }, pending: null, receipt: null, error: false };
      ledger.operations.set(key, operation);
    }
    const request = operation.request;
    const captured = generation.current;
    let settled!: () => void;
    operation.pending = new Promise<void>((resolve) => { settled = resolve; });
    operation.error = false;
    setSaved(null); notify(ledger);
    try {
      const receipt = await compactContextV4(request);
      if (receipt.project_id !== request.project_id || receipt.conversation_id !== request.conversation_id
        || receipt.run_id !== request.run_id || receipt.request_id !== request.request_id
        || (receipt.status !== "completed" && receipt.status !== "not_needed")) throw new Error("Unconfirmed compaction");
      operation.receipt = receipt;
      if (mounted.current && generation.current === captured) setSaved({ key, receipt, error: false });
    } catch {
      operation.error = true;
      if (mounted.current && generation.current === captured) setSaved({ key, receipt: null, error: true });
    } finally {
      operation.pending = null; settled(); notify(ledger);
    }
  }, [ledger, key, enabled, scope?.project_id, scope?.conversation_id, scope?.run_id, scope?.request_id, saved?.receipt]);
  return { busy: Boolean(ledger.operations.get(key)?.pending), receipt: enabled && saved?.key === key ? saved.receipt : null,
    error: enabled && saved?.key === key ? saved.error : false, compact };
}
