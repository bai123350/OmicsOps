import { useCallback, useLayoutEffect, useRef, useState, useSyncExternalStore } from "react";
import { inspectKernelV4 } from "../../tauri-api";
import type { KernelInspectionV4, KernelLanguage } from "../../types";

export interface KernelInspectionScope { projectId: string; conversationId: string; runId: string; language: KernelLanguage }
const empty = () => ({ value: null as KernelInspectionV4 | null, loading: false, error: false });

// A native Promise keeps running after its view closes. Share only its busy
// ownership across this window, never identities, metadata or results.
let activeInspection: symbol | null = null;
const busyListeners = new Set<() => void>();
function subscribeBusy(listener: () => void) { busyListeners.add(listener); return () => { busyListeners.delete(listener); }; }
function inspectionIsBusy() { return activeInspection !== null; }
function notifyBusy() { for (const listener of busyListeners) listener(); }

/** Manual UI observation only. Nothing is persisted or sent to the Agent. */
export function useKernelInspection(scope: KernelInspectionScope | null, enabled: boolean) {
  const key = JSON.stringify([scope?.projectId, scope?.conversationId, scope?.runId, scope?.language, enabled]);
  const config = useRef({ scope, enabled, key }); config.current = { scope, enabled, key };
  const [view, setView] = useState(empty);
  const session = useRef<string | null>(null);
  const mounted = useRef(false); const generation = useRef(0);
  const busy = useSyncExternalStore(subscribeBusy, inspectionIsBusy, inspectionIsBusy);
  useLayoutEffect(() => {
    mounted.current = true; generation.current++; session.current = null; setView(empty());
    return () => { mounted.current = false; generation.current++; session.current = null; };
  }, [key]);
  const refresh = useCallback(async () => {
    const snapshot = config.current;
    if (!mounted.current || !snapshot.enabled || !snapshot.scope || inspectionIsBusy()) return;
    const owner = snapshot.scope; const token = generation.current; const expected = session.current;
    const current = () => mounted.current && config.current.enabled && config.current.key === snapshot.key && generation.current === token;
    const flight = Symbol(); activeInspection = flight; notifyBusy();
    setView({ value: null, loading: true, error: false });
    try {
      const response = await inspectKernelV4({ project_id: owner.projectId, conversation_id: owner.conversationId, run_id: owner.runId, language: owner.language, expected_session_id: expected });
      if (!current()) return;
      if (response.project_id !== owner.projectId || response.conversation_id !== owner.conversationId || response.run_id !== owner.runId || response.language !== owner.language) throw new Error("scope");
      if (response.status === "ready" && response.session_id === null) throw new Error("session");
      const rebuilt = expected !== null && response.session_id !== null && response.session_id !== expected;
      if (rebuilt || response.status === "stale_session") {
        session.current = null;
        setView({ value: { ...response, session_id: null, status: "stale_session", objects: [], truncated: false }, loading: false, error: false });
      } else {
        session.current = response.status === "busy" ? (response.session_id ?? expected) : response.session_id;
        setView({ value: { ...response, objects: response.status === "ready" ? response.objects : [], truncated: response.status === "ready" && response.truncated }, loading: false, error: false });
      }
    } catch { if (current()) { session.current = null; setView({ value: null, loading: false, error: true }); } }
    finally { if (activeInspection === flight) { activeInspection = null; notifyBusy(); } }
  }, []);
  return { ...view, loading: Boolean(enabled && scope && (view.loading || busy)), refresh };
}
