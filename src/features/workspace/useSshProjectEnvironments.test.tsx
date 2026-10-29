import { act, renderHook, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import * as api from "../../tauri-api";
import { useSshProjectEnvironments } from "./useSshProjectEnvironments";

const scope = { project_id: "p", connection_id: "lab", remote_root: "/work/project", backend_id: "ssh:lab" };
const report = (environment = "system", overrides = {}) => ({
  ...scope, requested_environment: environment, observed_at: "2026-10-29T00:00:00Z", suggested_environment: "envs:python311:r443", truncated: false,
  environments: [
    { environment: "system", label: "system", interpreters: [{ language: "python" as const, status: "not_found" as const }, { language: "r" as const, status: "not_found" as const }] },
    { environment: "envs:python311:r443", label: "Python .envs/python311 · R .envs/r443", interpreters: [{ language: "python" as const, status: "available" as const, executable: "/work/project/.envs/python311/bin/python", version: "Python 3.11.18" }, { language: "r" as const, status: "available" as const, executable: "/work/project/.envs/r443/bin/Rscript", version: "R 4.4.3" }] },
  ], ...overrides,
});
function deferred<T>() { let resolve!: (value: T) => void; let reject!: (error: Error) => void; const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; }); return { resolve, reject, promise }; }
afterEach(() => vi.restoreAllMocks());

describe("project SSH interpreter observations", () => {
  it("does not connect for catalog loading and refreshes only after the user requests detection", async () => {
    const query = vi.spyOn(api, "probeSshProjectEnvironments").mockResolvedValue(report());
    const { result, rerender } = renderHook(({ revision }) => useSshProjectEnvironments(scope, "system", revision), { initialProps: { revision: 0 } });
    expect(query).not.toHaveBeenCalled();
    rerender({ revision: 1 }); expect(query).not.toHaveBeenCalled();
    act(() => result.current.refresh());
    await waitFor(() => expect(result.current.report?.suggested_environment).toBe("envs:python311:r443"));
    expect(query).toHaveBeenCalledWith("p", "system");
    rerender({ revision: 2 });
    await waitFor(() => expect(query).toHaveBeenCalledTimes(2));
  });

  it("rechecks a changed environment and uses only its own observed interpreters", async () => {
    const query = vi.spyOn(api, "probeSshProjectEnvironments").mockImplementation(async (_id, environment) => report(environment));
    const { result, rerender } = renderHook(({ environment }) => useSshProjectEnvironments(scope, environment, 0), { initialProps: { environment: "system" } });
    act(() => result.current.refresh());
    await waitFor(() => expect(result.current.selected?.interpreters[0].status).toBe("not_found"));
    rerender({ environment: "envs:python311:r443" });
    expect(result.current.selected).toBeNull();
    await waitFor(() => expect(query).toHaveBeenLastCalledWith("p", "envs:python311:r443"));
    await waitFor(() => expect(result.current.selected?.interpreters[0].version).toBe("Python 3.11.18"));
  });

  it.each(["project_id", "connection_id", "remote_root", "backend_id", "connection_revision"] as const)("drops late responses and disables automatic detection after %s changes", async (field) => {
    const pending = deferred<ReturnType<typeof report>>();
    const query = vi.spyOn(api, "probeSshProjectEnvironments").mockReturnValueOnce(pending.promise);
    const { result, rerender } = renderHook(({ value }) => useSshProjectEnvironments(value, "system", 0), { initialProps: { value: scope } });
    act(() => result.current.refresh());
    await waitFor(() => expect(query).toHaveBeenCalledOnce());
    rerender({ value: { ...scope, [field]: "other" } });
    expect(result.current.report).toBeNull(); expect(result.current.loading).toBe(false);
    await act(async () => pending.resolve(report()));
    expect(result.current.report).toBeNull(); expect(query).toHaveBeenCalledOnce();
  });

  it("rejects mismatched responses, clears a failed observation and supports retry", async () => {
    const query = vi.spyOn(api, "probeSshProjectEnvironments").mockResolvedValueOnce(report("other"))
      .mockRejectedValueOnce(new Error("unreachable")).mockResolvedValueOnce(report());
    const { result } = renderHook(() => useSshProjectEnvironments(scope, "system", 0));
    act(() => result.current.refresh());
    await waitFor(() => expect(result.current.error).toBe(true)); expect(result.current.selected).toBeNull();
    act(() => result.current.refresh()); await waitFor(() => expect(query).toHaveBeenCalledTimes(2));
    await waitFor(() => expect(result.current.loading).toBe(false)); expect(result.current.selected).toBeNull();
    act(() => result.current.refresh()); await waitFor(() => expect(result.current.report).not.toBeNull());
  });

  it("reuses an in-flight observation when another view requests the same detection", async () => {
    const pending = deferred<ReturnType<typeof report>>();
    const query = vi.spyOn(api, "probeSshProjectEnvironments").mockReturnValue(pending.promise);
    const { result } = renderHook(() => useSshProjectEnvironments(scope, "system", 0));
    act(() => result.current.refresh());
    await waitFor(() => expect(result.current.loading).toBe(true));
    act(() => { result.current.refresh(); result.current.refresh(); });
    expect(query).toHaveBeenCalledOnce();
    await act(async () => pending.resolve(report()));
    expect(result.current.selected?.environment).toBe("system");
  });
});
