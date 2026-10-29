import { useCallback, useEffect, useState } from "react";
import { probeSshProjectEnvironments } from "../../tauri-api";
import type { SshEnvironmentReportV4 } from "../../types";

export interface SshEnvironmentScope {
  project_id: string;
  connection_id: string;
  remote_root: string;
  backend_id: string;
  connection_revision?: number;
}

export function useSshProjectEnvironments(scope: SshEnvironmentScope | null, environment: string, revision: number) {
  const scopeKey = scope && scope.backend_id === `ssh:${scope.connection_id}`
    ? JSON.stringify([scope.project_id, scope.connection_id, scope.remote_root, scope.backend_id, scope.connection_revision ?? 0]) : null;
  const [enabledScope, setEnabledScope] = useState<string | null>(null);
  const [retry, setRetry] = useState(0);
  const requestKey = scopeKey === null ? null : JSON.stringify([scopeKey, environment, revision, retry]);
  const [state, setState] = useState<{ scope: string | null; key: string | null; report: SshEnvironmentReportV4 | null; loading: boolean; error: boolean }>({ scope: null, key: null, report: null, loading: false, error: false });
  const enabled = scopeKey !== null && enabledScope === scopeKey;
  const refresh = useCallback(() => {
    if (scopeKey === null || (enabledScope === scopeKey && (state.key !== requestKey || state.loading))) return;
    setEnabledScope(scopeKey); setRetry((value) => value + 1);
  }, [scopeKey, enabledScope, state.key, state.loading, requestKey]);

  useEffect(() => { setEnabledScope(null); }, [scopeKey]);
  useEffect(() => {
    let disposed = false;
    if (!scope || scopeKey === null || requestKey === null || !enabled) return;
    setState((current) => ({ scope: scopeKey, key: requestKey, report: current.scope === scopeKey ? current.report : null, loading: true, error: false }));
    probeSshProjectEnvironments(scope.project_id, environment).then((report) => {
      if (disposed) return;
      if (report.project_id !== scope.project_id || report.connection_id !== scope.connection_id
        || report.remote_root !== scope.remote_root || report.requested_environment !== environment) throw new Error("SSH environment observation does not match its requested scope");
      setState({ scope: scopeKey, key: requestKey, report, loading: false, error: false });
    }).catch(() => { if (!disposed) setState({ scope: scopeKey, key: requestKey, report: null, loading: false, error: true }); });
    return () => { disposed = true; };
  }, [scopeKey, requestKey, enabled, retry, revision]);

  const report = enabled && state.scope === scopeKey ? state.report : null;
  return {
    report,
    selected: state.key === requestKey && !state.loading ? report?.environments.find((option) => option.environment === environment) ?? null : null,
    loading: enabled && (state.key !== requestKey || state.loading),
    error: enabled && state.key === requestKey && state.error,
    refresh,
  };
}
