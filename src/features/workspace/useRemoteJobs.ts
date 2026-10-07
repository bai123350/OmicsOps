import { useCallback, useEffect, useLayoutEffect, useRef, useState, useSyncExternalStore } from "react";
import { listRemoteJobsV4, refreshRemoteJobV4 } from "../../remote-job-api";
import type { RemoteJobObservationV4, RemoteJobSummaryV4 } from "../../types";

export interface RemoteJobsScope { projectId: string; conversationId: string | null; bindingVersion: string }
interface View {
  jobs: RemoteJobSummaryV4[];
  selectedJobId: string | null;
  observation: RemoteJobObservationV4 | null;
  listLoading: boolean;
  refreshLoading: boolean;
  error: string;
  polling: boolean;
  attempts: number;
}
const emptyView = (): View => ({ jobs: [], selectedJobId: null, observation: null, listLoading: false, refreshLoading: false, error: "", polling: false, attempts: 0 });
const delays = [2000, 4000, 8000, 16000, 30000];
const safeError = (error: unknown): string => typeof error === "string" && ["remote_job_rejected", "remote_job_unavailable", "remote_job_timeout", "remote_job_not_found"].includes(error) ? error : "remote_job_unavailable";
const terminal = (job: RemoteJobSummaryV4) => job.recorded_state === "succeeded" || job.recorded_state === "failed";

// Window-lifetime busy ownership only; no responses or identities are cached.
// Closing a view cannot cancel the native Promise, so remounts share this gate.
let activeRefresh: symbol | null = null;
const busyListeners = new Set<() => void>();
function subscribeBusy(listener: () => void) { busyListeners.add(listener); return () => { busyListeners.delete(listener); }; }
function refreshIsBusy() { return activeRefresh !== null; }
function notifyBusy() { for (const listener of busyListeners) listener(); }

/** Local scheduling only. The actual refresh gate survives scope invalidation. */
export function useRemoteJobs(scope: RemoteJobsScope, open: boolean) {
  const key = JSON.stringify([scope.projectId, scope.conversationId, scope.bindingVersion]);
  const config = useRef({ ...scope, open, key });
  config.current = { ...scope, open, key };
  const [view, setView] = useState<View>(emptyView);
  const state = useRef<View>(view);
  const mounted = useRef(false);
  const generation = useRef(0);
  const listSequence = useRef(0);
  const refreshBusy = useSyncExternalStore(subscribeBusy, refreshIsBusy, refreshIsBusy);
  const nextDelay = useRef(0);
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const update = useCallback((patch: Partial<View>) => {
    state.current = { ...state.current, ...patch };
    if (mounted.current) setView(state.current);
  }, []);
  const clearTimer = useCallback(() => { if (timer.current !== null) clearTimeout(timer.current); timer.current = null; }, []);
  const stopPolling = useCallback(() => {
    clearTimer(); generation.current++;
    update({ polling: false, refreshLoading: false });
  }, [clearTimer, update]);

  const reload = useCallback(async () => {
    const snapshot = config.current;
    if (!snapshot.open || !snapshot.projectId || !mounted.current) return;
    const sequence = ++listSequence.current;
    const current = () => mounted.current && config.current.open && config.current.key === snapshot.key && sequence === listSequence.current;
    update({ listLoading: true, error: "" });
    try {
      const page = await listRemoteJobsV4({ project_id: snapshot.projectId, conversation_id: snapshot.conversationId });
      if (!current()) return;
      if (page.remote_liveness_checked || page.jobs.some((job) => job.project_id !== snapshot.projectId || (snapshot.conversationId !== null && job.conversation_id !== snapshot.conversationId))) throw "remote_job_rejected";
      const selected = page.jobs.find((job) => job.job_id === state.current.selectedJobId);
      if (state.current.selectedJobId && (!selected || terminal(selected) || selected.query_eligibility !== "needs_root_check")) stopPolling();
      update({ jobs: page.jobs, ...(!selected ? { selectedJobId: null, observation: null } : {}) });
    } catch (error: unknown) { if (current()) update({ error: safeError(error) }); }
    finally { if (current()) update({ listLoading: false }); }
  }, [stopPolling, update]);

  const run = useCallback(async function query() {
    const snapshot = config.current;
    const job = state.current.jobs.find((item) => item.job_id === state.current.selectedJobId);
    if (refreshIsBusy()) { update({ polling: false }); return; }
    if (!mounted.current || !snapshot.open || !job || !["needs_root_check", "recorded_receipt"].includes(job.query_eligibility)) return;
    const token = generation.current;
    const current = () => mounted.current && config.current.open && config.current.key === snapshot.key && generation.current === token && state.current.selectedJobId === job.job_id;
    const owner = Symbol(); activeRefresh = owner; notifyBusy();
    update({ refreshLoading: true, error: "", attempts: state.current.attempts + 1 });
    let stop = false;
    try {
      const observation = await refreshRemoteJobV4({ project_id: snapshot.projectId, job_id: job.job_id });
      if (!current()) return;
      const actual = observation.job;
      if (observation.resubmitted || actual.project_id !== job.project_id || actual.conversation_id !== job.conversation_id || actual.job_id !== job.job_id || actual.run_id !== job.run_id || actual.call_id !== job.call_id || actual.request_sha256 !== job.request_sha256 || actual.backend_id !== job.backend_id || actual.language !== job.language || actual.environment !== job.environment) throw "remote_job_rejected";
      update({ observation });
      stop = observation.status === "completed" || observation.status === "rejected" || terminal(actual) || actual.query_eligibility !== "needs_root_check";
    } catch (error: unknown) {
      if (current()) { const code = safeError(error); update({ error: code }); stop = code === "remote_job_rejected" || code === "remote_job_not_found"; }
    } finally {
      // Clearing the gate only when the original Promise settles prevents a new
      // scope from overlapping an old request that cannot be cancelled locally.
      if (activeRefresh === owner) { activeRefresh = null; notifyBusy(); }
      if (current()) {
        update({ refreshLoading: false });
        if (stop || state.current.attempts >= 10) update({ polling: false });
        else if (state.current.polling) {
          timer.current = setTimeout(() => { timer.current = null; if (current()) { nextDelay.current++; void query(); } }, delays[Math.min(nextDelay.current, delays.length - 1)]);
        }
      }
    }
  }, [update]);

  const selectJob = useCallback((id: string | null) => {
    if (id === state.current.selectedJobId) return;
    stopPolling(); update({ selectedJobId: id, observation: null, attempts: 0, error: "" });
  }, [stopPolling, update]);
  const startPolling = useCallback(() => {
    const job = state.current.jobs.find((item) => item.job_id === state.current.selectedJobId);
    if (refreshIsBusy() || !mounted.current || !config.current.open || !job || terminal(job) || job.query_eligibility !== "needs_root_check" || state.current.observation?.status === "completed" || state.current.observation?.status === "rejected") return;
    clearTimer(); const token = ++generation.current; const snapshotKey = config.current.key;
    nextDelay.current = 1;
    update({ polling: true, attempts: 0, error: "" });
    timer.current = setTimeout(() => { timer.current = null; if (mounted.current && config.current.open && config.current.key === snapshotKey && generation.current === token) void run(); }, 2000);
  }, [clearTimer, run, update]);
  const refresh = useCallback(async () => {
    if (refreshIsBusy() || !mounted.current || !config.current.open) return;
    clearTimer(); generation.current++;
    nextDelay.current = 0;
    // An enabled window restarts with this manual query as attempt one.
    update({ attempts: 0 });
    await run();
  }, [clearTimer, run, update]);

  useLayoutEffect(() => {
    mounted.current = true;
    clearTimer(); generation.current++; listSequence.current++;
    state.current = emptyView(); setView(state.current);
    return () => { mounted.current = false; clearTimer(); generation.current++; listSequence.current++; };
  }, [key, open, clearTimer]);
  useEffect(() => { if (open) void reload(); }, [key, open, reload]);
  return { jobs: view.jobs, selectedJobId: view.selectedJobId, observation: view.observation, loading: view.listLoading || view.refreshLoading || (open && refreshBusy), error: view.error, polling: view.polling, attempts: view.attempts, selectJob, reload, refresh, startPolling, stopPolling };
}
