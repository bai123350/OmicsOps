import { useCallback, useEffect, useState } from "react";
import { agentV4RuntimeBoundary } from "../../tauri-api";
import type { ComputeSelectionV4, RuntimeBoundaryRequestV4, RuntimeBoundaryViewV4 } from "../../types";

function selectionKey(value: ComputeSelectionV4): string {
  return JSON.stringify({
    schema_version: value.schema_version,
    backend_id: value.backend_id,
    backend_kind: value.backend_kind,
    autonomy_mode: value.autonomy_mode,
    approval_policy: value.approval_policy ?? "risk_based",
    environment: value.environment,
    network_policy: value.network_policy,
    container_image: value.container_image ? { reference: value.container_image.reference, image_id: value.container_image.image_id } : null,
  });
}

function matches(request: RuntimeBoundaryRequestV4, response: RuntimeBoundaryViewV4): boolean {
  if (response.project_id !== request.project_id) return false;
  if (request.source === "frozen_run") return response.source.kind === "frozen_run" && response.source.run_id === request.run_id;
  return response.source.kind === "draft_selection" && selectionKey(response.compute_selection) === selectionKey(request.compute_selection);
}

export function useRuntimeBoundary(request: RuntimeBoundaryRequestV4 | null) {
  const requestKey = request === null ? null : request.source === "frozen_run"
    ? JSON.stringify([request.project_id, request.source, request.run_id])
    : JSON.stringify([request.project_id, request.source, selectionKey(request.compute_selection)]);
  const [state, setState] = useState<{ key: string | null; view: RuntimeBoundaryViewV4 | null; loading: boolean; error: string }>({ key: null, view: null, loading: false, error: "" });
  const [retry, setRetry] = useState(0);
  const refresh = useCallback(() => setRetry((value) => value + 1), []);

  useEffect(() => {
    let disposed = false;
    setState({ key: requestKey, view: null, loading: request !== null, error: "" });
    if (request === null || requestKey === null) return;
    agentV4RuntimeBoundary(request).then((response) => {
      if (disposed) return;
      if (!matches(request, response)) throw new Error("Runtime boundary does not match the requested selection or run");
      setState({ key: requestKey, view: response, loading: false, error: "" });
    }).catch((reason: unknown) => {
      if (!disposed) setState({ key: requestKey, view: null, loading: false, error: reason instanceof Error ? reason.message : String(reason) });
    });
    return () => { disposed = true; };
  }, [requestKey, retry]);

  return {
    view: state.key === requestKey ? state.view : null,
    loading: requestKey !== null && (state.key !== requestKey || state.loading),
    error: state.key === requestKey ? state.error : "",
    refresh,
  };
}
