import { act, renderHook, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import * as api from "../../tauri-api";
import type { ComputeSelectionV4, RuntimeBoundaryRequestV4, RuntimeBoundaryViewV4 } from "../../types";
import { useRuntimeBoundary } from "./useRuntimeBoundary";

const selection: ComputeSelectionV4 = { schema_version: 4, backend_id: "local", backend_kind: "local", autonomy_mode: "supervised", environment: "system", network_policy: "host_inherited" };
const draft: RuntimeBoundaryRequestV4 = { source: "draft_selection", project_id: "p", compute_selection: selection };
const containerDraft: RuntimeBoundaryRequestV4 = { source: "draft_selection", project_id: "p", compute_selection: { ...selection, backend_id: "docker", backend_kind: "docker", network_policy: "none", container_image: { reference: "python:3.12", image_id: "sha256:one" } } };
const frozen = (run_id: string): RuntimeBoundaryRequestV4 => ({ source: "frozen_run", project_id: "p", run_id });
const view = (request: RuntimeBoundaryRequestV4): RuntimeBoundaryViewV4 => ({
  project_id: request.project_id,
  source: request.source === "draft_selection" ? { kind: "draft_selection" } : { kind: "frozen_run", run_id: request.run_id },
  compute_selection: request.source === "draft_selection" ? request.compute_selection : selection,
  execution_location: "local_host", isolation: "process", limits: ["same_user_permissions", "project_cwd_not_access_control"],
  interactive_lifecycle: "run_scoped_no_restart_reconnect", detached_job_lifecycle: "unsupported", verification: "not_checked_by_this_view",
});
function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: Error) => void;
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}
afterEach(() => vi.restoreAllMocks());

describe("runtime boundary view identity", () => {
  it("does not query an incomplete draft", () => {
    const query = vi.spyOn(api, "agentV4RuntimeBoundary");
    const { result } = renderHook(() => useRuntimeBoundary(null));
    expect(query).not.toHaveBeenCalled();
    expect(result.current.view).toBeNull();
  });

  it("rejects a mismatched project, source, run, or full draft selection", async () => {
    const responses = [
      { ...view(draft), project_id: "other" },
      { ...view(draft), source: { kind: "frozen_run" as const, run_id: "r" } },
      { ...view(frozen("r")), source: { kind: "frozen_run" as const, run_id: "other" } },
      { ...view(draft), compute_selection: { ...selection, environment: "other" } },
      { ...view(containerDraft), compute_selection: { ...containerDraft.compute_selection, container_image: { reference: "python:3.12", image_id: "sha256:other" } } },
    ];
    const requests = [draft, draft, frozen("r"), draft, containerDraft];
    const query = vi.spyOn(api, "agentV4RuntimeBoundary");
    for (let index = 0; index < responses.length; index++) {
      query.mockResolvedValueOnce(responses[index]);
      const { result, unmount } = renderHook(() => useRuntimeBoundary(requests[index]));
      await waitFor(() => expect(result.current.error).not.toBe(""));
      expect(result.current.view).toBeNull();
      unmount();
    }
  });

  it("accepts omitted default approval policy and retries after failure without retaining the old view", async () => {
    const query = vi.spyOn(api, "agentV4RuntimeBoundary").mockResolvedValueOnce({ ...view(draft), compute_selection: { ...selection, approval_policy: "risk_based" } })
      .mockRejectedValueOnce(new Error("unavailable")).mockResolvedValueOnce(view(draft));
    const { result } = renderHook(() => useRuntimeBoundary(draft));
    await waitFor(() => expect(result.current.view).not.toBeNull());
    act(() => result.current.refresh());
    await waitFor(() => expect(result.current.error).toBe("unavailable"));
    expect(result.current.view).toBeNull();
    act(() => result.current.refresh());
    await waitFor(() => expect(result.current.view).not.toBeNull());
    expect(query).toHaveBeenCalledTimes(3);
  });

  it("hides the old view immediately and ignores late success and late failure", async () => {
    const first = deferred<RuntimeBoundaryViewV4>();
    const second = deferred<RuntimeBoundaryViewV4>();
    vi.spyOn(api, "agentV4RuntimeBoundary").mockReturnValueOnce(first.promise).mockReturnValueOnce(second.promise).mockResolvedValueOnce(view(frozen("new")));
    const { result, rerender } = renderHook(({ request }) => useRuntimeBoundary(request), { initialProps: { request: frozen("old") as RuntimeBoundaryRequestV4 | null } });
    rerender({ request: frozen("middle") });
    rerender({ request: frozen("new") });
    await waitFor(() => expect(result.current.view?.source).toEqual({ kind: "frozen_run", run_id: "new" }));
    await act(async () => { first.resolve(view(frozen("old"))); second.reject(new Error("late error")); });
    expect(result.current.error).toBe("");
    expect(result.current.view?.source).toEqual({ kind: "frozen_run", run_id: "new" });
    rerender({ request: null });
    expect(result.current.view).toBeNull();
    expect(result.current.error).toBe("");
  });
});
