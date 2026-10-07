import { act, renderHook } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import * as api from "../../tauri-api";
import type { KernelInspectionV4 } from "../../types";
import { useKernelInspection } from "./useKernelInspection";

const scope = { projectId: "p", conversationId: "c", runId: "run", language: "python" as const };
const ready = (patch: Partial<KernelInspectionV4> = {}): KernelInspectionV4 => ({ project_id: "p", conversation_id: "c", run_id: "run", language: "python", session_id: "s1", status: "ready", objects: [{ name: "old_session_object", type_label: "opaque", dimensions: null, element_count: null, summary: "opaque" }], truncated: false, ...patch });
function deferred() { let resolve!: (v: KernelInspectionV4) => void; const promise = new Promise<KernelInspectionV4>((r) => { resolve = r; }); return { resolve, promise }; }
afterEach(() => vi.restoreAllMocks());
describe("useKernelInspection", () => {
  it.each(["reopen", "owner", "remount"])("keeps the uncancelled old native request visibly busy across %s without queueing", async (change) => {
    const old = deferred(); const next = deferred();
    const inspect = vi.spyOn(api, "inspectKernelV4").mockReturnValueOnce(old.promise).mockReturnValueOnce(next.promise);
    const hook = renderHook(({ owner, enabled }) => useKernelInspection(owner, enabled), { initialProps: { owner: scope, enabled: true } });
    let oldRequest!: Promise<void>; act(() => { oldRequest = hook.result.current.refresh(); });
    const owner = change === "owner" ? { ...scope, runId: "next" } : scope;
    if (change === "reopen") { hook.rerender({ owner, enabled: false }); hook.rerender({ owner, enabled: true }); }
    if (change === "owner") hook.rerender({ owner, enabled: true });
    if (change === "remount") hook.unmount();
    const fresh = change === "remount" ? renderHook(() => useKernelInspection(owner, true)) : hook;
    // Always settle the original Promise, including RED, so no gate leaks into later tests.
    const busyWhileOldPending = fresh.result.current.loading;
    await act(async () => { await fresh.result.current.refresh(); });
    const callsWhileOldPending = inspect.mock.calls.length;
    await act(async () => { old.resolve(ready()); await oldRequest; });
    expect(busyWhileOldPending).toBe(true);
    expect(callsWhileOldPending).toBe(1);
    expect(fresh.result.current.loading).toBe(false); expect(fresh.result.current.value).toBeNull(); expect(inspect).toHaveBeenCalledTimes(1);
    let nextRequest!: Promise<void>; act(() => { nextRequest = fresh.result.current.refresh(); });
    expect(fresh.result.current.loading).toBe(true); expect(inspect).toHaveBeenCalledTimes(2);
    await act(async () => { await fresh.result.current.refresh(); }); expect(inspect).toHaveBeenCalledTimes(2);
    await act(async () => { next.resolve(ready({ run_id: owner.runId, session_id: "new", objects: [] })); await nextRequest; });
    expect(fresh.result.current.value?.session_id).toBe("new"); expect(fresh.result.current.loading).toBe(false);
  });
  it("inspection_refresh_is_manual_and_scope_bound", async () => {
    const inspect = vi.spyOn(api, "inspectKernelV4").mockResolvedValue(ready());
    const hook = renderHook(() => useKernelInspection(scope, true));
    expect(inspect).not.toHaveBeenCalled();
    await act(async () => { await hook.result.current.refresh(); });
    expect(inspect).toHaveBeenCalledTimes(1);
    expect(inspect).toHaveBeenLastCalledWith({ project_id: "p", conversation_id: "c", run_id: "run", language: "python", expected_session_id: null });
    await act(async () => { await hook.result.current.refresh(); });
    expect(inspect).toHaveBeenLastCalledWith({ project_id: "p", conversation_id: "c", run_id: "run", language: "python", expected_session_id: "s1" });
    expect(hook.result.current.value?.objects).toHaveLength(1);
  });
  it.each(["close", "project", "conversation", "run", "language"])("discards late response after %s, with no automatic scope fetch", async (change) => {
    const pending = deferred(); const inspect = vi.spyOn(api, "inspectKernelV4").mockReturnValue(pending.promise);
    const hook = renderHook(({ owner, enabled }) => useKernelInspection(owner, enabled), { initialProps: { owner: scope, enabled: true } });
    let request!: Promise<void>; act(() => { request = hook.result.current.refresh(); });
    hook.rerender({ owner: { ...scope, ...(change === "project" ? { projectId: "other" } : change === "conversation" ? { conversationId: "other" } : change === "run" ? { runId: "other" } : change === "language" ? { language: "r" as typeof scope.language } : {}) }, enabled: change !== "close" });
    await act(async () => { pending.resolve(ready()); await request; });
    expect(hook.result.current.value).toBeNull(); expect(hook.result.current.loading).toBe(false); expect(inspect).toHaveBeenCalledTimes(1);
    hook.rerender({ owner: scope, enabled: true }); expect(hook.result.current.value).toBeNull();
  });
  it("does not queue duplicate manual refreshes or auto refresh Busy", async () => {
    const pending = deferred(); const inspect = vi.spyOn(api, "inspectKernelV4").mockReturnValue(pending.promise);
    const hook = renderHook(() => useKernelInspection(scope, true));
    let request!: Promise<void>; act(() => { request = hook.result.current.refresh(); });
    await act(async () => { await hook.result.current.refresh(); }); expect(inspect).toHaveBeenCalledTimes(1);
    await act(async () => { pending.resolve(ready({ status: "busy", objects: [] })); await request; });
    expect(hook.result.current.value?.status).toBe("busy"); expect(inspect).toHaveBeenCalledTimes(1);
  });
  it("inspection_discards_rebuilt_session_and_requires_an_explicit_new_snapshot", async () => {
    const inspect = vi.spyOn(api, "inspectKernelV4").mockResolvedValueOnce(ready()).mockResolvedValueOnce(ready({ session_id: "s2" })).mockResolvedValueOnce(ready({ session_id: "s2", objects: [] }));
    const hook = renderHook(() => useKernelInspection(scope, true));
    await act(async () => { await hook.result.current.refresh(); });
    await act(async () => { await hook.result.current.refresh(); });
    expect(hook.result.current.value?.status).toBe("stale_session"); expect(hook.result.current.value?.objects).toEqual([]);
    expect(inspect).toHaveBeenCalledTimes(2);
    await act(async () => { await hook.result.current.refresh(); });
    expect(inspect.mock.calls[2][0].expected_session_id).toBeNull(); expect(hook.result.current.value?.session_id).toBe("s2");
  });
  it("preserves Busy with a missing session and keeps the last expected session for the next manual query", async () => {
    const inspect = vi.spyOn(api, "inspectKernelV4").mockResolvedValueOnce(ready()).mockResolvedValueOnce(ready({ status: "busy", session_id: null, objects: [] })).mockResolvedValueOnce(ready());
    const hook = renderHook(() => useKernelInspection(scope, true));
    await act(async () => { await hook.result.current.refresh(); }); await act(async () => { await hook.result.current.refresh(); });
    expect(hook.result.current.value?.status).toBe("busy");
    await act(async () => { await hook.result.current.refresh(); }); expect(inspect.mock.calls[2][0].expected_session_id).toBe("s1");
  });
  it("rejects Ready without a session and clears metadata on host StaleSession", async () => {
    vi.spyOn(api, "inspectKernelV4").mockResolvedValueOnce(ready({ session_id: null })).mockResolvedValueOnce(ready()).mockResolvedValueOnce(ready({ status: "stale_session", session_id: "s2" }));
    const hook = renderHook(() => useKernelInspection(scope, true));
    await act(async () => { await hook.result.current.refresh(); }); expect(hook.result.current.value).toBeNull(); expect(hook.result.current.error).toBe(true);
    await act(async () => { await hook.result.current.refresh(); }); await act(async () => { await hook.result.current.refresh(); });
    expect(hook.result.current.value?.status).toBe("stale_session"); expect(hook.result.current.value?.objects).toEqual([]);
  });
  it.each(["project_id", "conversation_id", "run_id", "language"] as const)("rejects mismatched %s and hides raw errors", async (field) => {
    const inspect = vi.spyOn(api, "inspectKernelV4").mockResolvedValueOnce(ready({ [field]: "other" })).mockRejectedValueOnce("secret raw exception");
    const hook = renderHook(() => useKernelInspection(scope, true));
    await act(async () => { await hook.result.current.refresh(); });
    expect(hook.result.current.value).toBeNull(); expect(hook.result.current.error).toBe(true);
    await act(async () => { await hook.result.current.refresh(); }); expect(hook.result.current.error).toBe(true); expect(inspect).toHaveBeenCalledTimes(2);
  });
  it("disabled and absent owners cannot inspect; unmount rejects pending delivery", async () => {
    const inspect = vi.spyOn(api, "inspectKernelV4");
    const absent = renderHook(() => useKernelInspection(null, true)); const closed = renderHook(() => useKernelInspection(scope, false));
    await act(async () => { await absent.result.current.refresh(); await closed.result.current.refresh(); }); expect(inspect).not.toHaveBeenCalled();
    const pending = deferred(); inspect.mockReturnValue(pending.promise); const hook = renderHook(() => useKernelInspection(scope, true));
    let request!: Promise<void>; act(() => { request = hook.result.current.refresh(); }); hook.unmount(); await act(async () => { pending.resolve(ready()); await request; });
  });
});
