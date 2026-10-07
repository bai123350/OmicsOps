import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import * as api from "../../remote-job-api";
import type { RemoteJobObservationV4, RemoteJobSummaryV4 } from "../../types";
import { useRemoteJobs } from "./useRemoteJobs";

vi.mock("../../remote-job-api", () => ({ listRemoteJobsV4: vi.fn(), refreshRemoteJobV4: vi.fn() }));
const job: RemoteJobSummaryV4 = { project_id: "p", conversation_id: "c", job_id: "j", run_id: "r", call_id: "call", backend_id: "ssh", language: "python", environment: "system", request_sha256: "hash", recorded_state: "running", query_eligibility: "needs_root_check", recorded_at: null };
const observed = (status: RemoteJobObservationV4["status"] = "running"): RemoteJobObservationV4 => ({ job, status, observed_at: "2026-10-07T00:00:00Z", stdout_excerpt: "", stderr_excerpt: "", captures: [], resubmitted: false });
const scope = { projectId: "p", conversationId: "c" as string | null, bindingVersion: "v1" };
function deferred<T>() { let resolve!: (v: T) => void; let reject!: (v: unknown) => void; const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; }); return { promise, resolve, reject }; }
async function flush() { await act(async () => { await Promise.resolve(); }); }
async function tick(ms: number) { await act(async () => { await vi.advanceTimersByTimeAsync(ms); }); }
async function setup() { const hook = renderHook(() => useRemoteJobs({ ...scope }, true)); await flush(); act(() => hook.result.current.selectJob("j")); return hook; }
beforeEach(() => { vi.useFakeTimers(); vi.resetAllMocks(); vi.mocked(api.listRemoteJobsV4).mockResolvedValue({ jobs: [job, { ...job, job_id: "j2" }], limit: 100, remote_liveness_checked: false }); vi.mocked(api.refreshRemoteJobV4).mockResolvedValue(observed()); });
afterEach(() => { cleanup(); expect(vi.getTimerCount()).toBe(0); vi.useRealTimers(); });

describe("finite serial remote job observation", () => {
  it("lists metadata without refresh and ignores recreated scalar-equivalent scopes", async () => {
    const hook = await setup(); hook.rerender(); await tick(1000000);
    expect(api.listRemoteJobsV4).toHaveBeenCalledTimes(1);
    expect(api.listRemoteJobsV4).toHaveBeenCalledWith({ project_id: "p", conversation_id: "c" });
    expect(api.refreshRemoteJobV4).not.toHaveBeenCalled(); expect(hook.result.current.polling).toBe(false);
  });
  it("opts in to 2/4/8/16/30 seconds then 30 seconds up to ten attempts", async () => {
    const { result } = await setup(); act(() => result.current.startPolling());
    for (const [index, delay] of [2000, 4000, 8000, 16000, 30000, 30000, 30000, 30000, 30000, 30000].entries()) {
      await tick(delay - 1); expect(api.refreshRemoteJobV4).toHaveBeenCalledTimes(index);
      await tick(1); expect(api.refreshRemoteJobV4).toHaveBeenCalledTimes(index + 1);
    }
    expect(result.current.attempts).toBe(10); expect(result.current.polling).toBe(false); await tick(900000); expect(api.refreshRemoteJobV4).toHaveBeenCalledTimes(10);
  });
  it("starts delay after settlement and never overlaps automatic or manual refresh", async () => {
    const pending = deferred<RemoteJobObservationV4>(); let concurrent = 0; let maximum = 0;
    vi.mocked(api.refreshRemoteJobV4).mockImplementationOnce(async () => { maximum = Math.max(maximum, ++concurrent); try { return await pending.promise; } finally { concurrent--; } });
    const { result } = await setup(); act(() => result.current.startPolling()); await tick(2000); await tick(900000);
    act(() => { void result.current.refresh(); result.current.startPolling(); }); expect(api.refreshRemoteJobV4).toHaveBeenCalledTimes(1);
    await act(async () => pending.resolve(observed())); await tick(3999); expect(api.refreshRemoteJobV4).toHaveBeenCalledTimes(1);
    await tick(1); expect(api.refreshRemoteJobV4).toHaveBeenCalledTimes(2); expect(maximum).toBe(1);
  });
  it("manual refresh is one request by default and restarts an enabled finite window", async () => {
    const { result } = await setup(); await act(async () => result.current.refresh()); await tick(900000); expect(api.refreshRemoteJobV4).toHaveBeenCalledTimes(1);
    act(() => result.current.startPolling()); await tick(1000); await act(async () => result.current.refresh());
    expect(result.current.attempts).toBe(1); await tick(1999); expect(api.refreshRemoteJobV4).toHaveBeenCalledTimes(2);
    await tick(1); expect(api.refreshRemoteJobV4).toHaveBeenCalledTimes(3); expect(result.current.attempts).toBe(2);
  });
  it("continues a bounded window after failures without rendering arbitrary host errors", async () => {
    vi.mocked(api.refreshRemoteJobV4).mockRejectedValue("private /root key text"); const { result } = await setup(); act(() => result.current.startPolling()); await tick(2000);
    expect(result.current.error).toBe("remote_job_unavailable"); await tick(4000); expect(result.current.attempts).toBe(2);
    await tick(900000); expect(api.refreshRemoteJobV4).toHaveBeenCalledTimes(10); expect(result.current.polling).toBe(false);
  });
  it.each(["completed", "rejected"] as const)("stops on %s without further timers", async (status) => {
    vi.mocked(api.refreshRemoteJobV4).mockResolvedValue(observed(status)); const { result } = await setup(); act(() => result.current.startPolling()); await tick(2000);
    expect(result.current.observation?.status).toBe(status); expect(result.current.polling).toBe(false); await tick(900000); expect(api.refreshRemoteJobV4).toHaveBeenCalledTimes(1);
  });
  it("cannot automatically poll a recorded receipt or ineligible binding", async () => {
    for (const eligibility of ["recorded_receipt", "binding_changed", "unsupported", "integrity_mismatch"] as const) {
      vi.mocked(api.listRemoteJobsV4).mockResolvedValue({ jobs: [{ ...job, query_eligibility: eligibility }], limit: 100, remote_liveness_checked: false });
      const hook = await setup(); act(() => hook.result.current.startPolling()); await tick(900000); expect(api.refreshRemoteJobV4).not.toHaveBeenCalled();
      await act(async () => hook.result.current.refresh()); expect(api.refreshRemoteJobV4).toHaveBeenCalledTimes(eligibility === "recorded_receipt" ? 1 : 0); vi.mocked(api.refreshRemoteJobV4).mockClear(); hook.unmount();
    }
  });
  it.each(["close", "project", "conversation", "binding", "selected", "stop", "unmount"])("invalidates late promises and timers on %s while preserving the actual in-flight gate", async (change) => {
    const pending = deferred<RemoteJobObservationV4>(); vi.mocked(api.refreshRemoteJobV4).mockReturnValueOnce(pending.promise);
    const hook = renderHook(({ current, open }) => useRemoteJobs(current, open), { initialProps: { current: scope, open: true } }); await flush(); act(() => { hook.result.current.selectJob("j"); hook.result.current.startPolling(); }); await tick(2000);
    if (change === "unmount") hook.unmount();
    else if (change === "selected") act(() => hook.result.current.selectJob("j2"));
    else if (change === "stop") act(() => hook.result.current.stopPolling());
    else hook.rerender({ current: { ...scope, ...(change === "project" ? { projectId: "other" } : change === "conversation" ? { conversationId: null } : change === "binding" ? { bindingVersion: "v2" } : {}) }, open: change !== "close" });
    await flush(); if (change !== "unmount") act(() => { hook.result.current.selectJob("j2"); void hook.result.current.refresh(); });
    expect(api.refreshRemoteJobV4).toHaveBeenCalledTimes(1); await act(async () => pending.resolve(observed())); await tick(900000);
    if (change !== "unmount") { expect(hook.result.current.observation).toBeNull(); expect(hook.result.current.polling).toBe(false); }
    expect(api.refreshRemoteJobV4).toHaveBeenCalledTimes(1);
  });
  it("rejects mismatched observation and drops out-of-order list responses", async () => {
    const first = deferred<Awaited<ReturnType<typeof api.listRemoteJobsV4>>>(); vi.mocked(api.listRemoteJobsV4).mockReturnValueOnce(first.promise);
    const hook = renderHook(() => useRemoteJobs(scope, true)); await act(async () => hook.result.current.reload()); await act(async () => first.resolve({ jobs: [{ ...job, job_id: "late" }], limit: 100, remote_liveness_checked: false }));
    expect(hook.result.current.jobs[0].job_id).toBe("j"); act(() => hook.result.current.selectJob("j")); vi.mocked(api.refreshRemoteJobV4).mockResolvedValue({ ...observed(), job: { ...job, job_id: "other" } });
    await act(async () => hook.result.current.refresh()); expect(hook.result.current.observation).toBeNull(); expect(hook.result.current.error).toBe("remote_job_rejected");
  });
  it("stops queued timers before the first query on selection, binding and close changes", async () => {
    const hook = renderHook(({ current, open }) => useRemoteJobs(current, open), { initialProps: { current: scope, open: true } }); await flush();
    act(() => { hook.result.current.selectJob("j"); hook.result.current.startPolling(); hook.result.current.selectJob("j2"); }); await tick(2000); expect(api.refreshRemoteJobV4).not.toHaveBeenCalled();
    act(() => hook.result.current.startPolling()); hook.rerender({ current: { ...scope, bindingVersion: "changed" }, open: true }); await flush(); await tick(2000); expect(api.refreshRemoteJobV4).not.toHaveBeenCalled();
    act(() => { hook.result.current.selectJob("j"); hook.result.current.startPolling(); }); hook.rerender({ current: { ...scope, bindingVersion: "changed" }, open: false }); await tick(2000); expect(api.refreshRemoteJobV4).not.toHaveBeenCalled();
  });
  it("drops a late failure after stopping, and does not restore polling on reopen", async () => {
    const pending = deferred<RemoteJobObservationV4>(); vi.mocked(api.refreshRemoteJobV4).mockReturnValueOnce(pending.promise);
    const hook = renderHook(({ open }) => useRemoteJobs(scope, open), { initialProps: { open: true } }); await flush(); act(() => { hook.result.current.selectJob("j"); hook.result.current.startPolling(); }); await tick(2000);
    act(() => hook.result.current.stopPolling()); await act(async () => pending.reject("remote_job_timeout")); expect(hook.result.current.error).toBe("");
    hook.rerender({ open: false }); hook.rerender({ open: true }); await flush(); expect(hook.result.current.selectedJobId).toBeNull(); expect(hook.result.current.attempts).toBe(0); await tick(900000); expect(api.refreshRemoteJobV4).toHaveBeenCalledTimes(1);
  });
  it("retains the actual request gate across unmount/reopen and reactively releases it only after settlement", async () => {
    const pending = deferred<RemoteJobObservationV4>(); let concurrent = 0; let maximum = 0;
    vi.mocked(api.refreshRemoteJobV4).mockImplementation(async () => { maximum = Math.max(maximum, ++concurrent); try { return await pending.promise; } finally { concurrent--; } });
    const first = await setup(); act(() => { void first.result.current.refresh(); }); first.unmount();
    const second = await setup(); expect(second.result.current.loading).toBe(true); act(() => { void second.result.current.refresh(); second.result.current.startPolling(); }); await tick(900000); expect(api.refreshRemoteJobV4).toHaveBeenCalledTimes(1);
    await act(async () => pending.resolve(observed())); expect(second.result.current.loading).toBe(false); expect(second.result.current.observation).toBeNull(); expect(second.result.current.polling).toBe(false);
    await act(async () => second.result.current.refresh()); expect(api.refreshRemoteJobV4).toHaveBeenCalledTimes(2); expect(maximum).toBe(1);
  });
});
