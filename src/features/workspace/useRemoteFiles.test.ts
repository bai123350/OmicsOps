import { act, renderHook } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import * as api from "../../tauri-api";
import type { RemoteFileEntry, WorkspaceProject } from "../../types";
import { useRemoteFiles } from "./useRemoteFiles";

type Binding = Pick<WorkspaceProject, "id" | "connection_id" | "remote_root">;
const project: Binding = { id: "project-a", connection_id: "ssh-a", remote_root: "/project-a" };
const entry: RemoteFileEntry = { relative_path: "result.tsv", directory: false, size_bytes: 9, modified_unix_seconds: 0 };

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((resolvePromise, rejectPromise) => { resolve = resolvePromise; reject = rejectPromise; });
  return { promise, resolve, reject };
}

afterEach(() => vi.restoreAllMocks());

it("stays idle without a complete SSH binding and never requests a remote listing", async () => {
  const list = vi.spyOn(api, "listRemoteFiles");
  const { result, rerender } = renderHook(({ binding }) => useRemoteFiles(binding), { initialProps: { binding: null as Binding | null } });
  expect(result.current.status).toBe("idle");
  rerender({ binding: { ...project, connection_id: null } });
  await act(async () => result.current.refresh());
  rerender({ binding: { ...project, remote_root: null } });
  await act(async () => result.current.refresh());
  expect(result.current.status).toBe("idle");
  expect(result.current.files).toEqual([]);
  expect(list).not.toHaveBeenCalled();
});

it("only reports an empty directory as ready after a successful listing and recovers from errors", async () => {
  const first = deferred<RemoteFileEntry[]>();
  const retry = deferred<RemoteFileEntry[]>();
  vi.spyOn(api, "listRemoteFiles").mockReturnValueOnce(first.promise).mockReturnValueOnce(retry.promise);
  const { result } = renderHook(() => useRemoteFiles(project));
  expect(result.current.status).toBe("loading");
  await act(async () => first.reject(new Error("PRIVATE_HOST_DETAILS")));
  expect(result.current.status).toBe("error");
  act(() => { void result.current.refresh(); });
  expect(result.current.status).toBe("loading");
  await act(async () => retry.resolve([]));
  expect(result.current.status).toBe("ready");
  expect(result.current.files).toEqual([]);
});

it.each([
  [new Error("remote_root_resolution_failed: status 1"), "remote_root_resolution_failed"],
  ["remote_root_unavailable: status 1", "remote_root_unavailable"],
  ["remote_listing_failed", "remote_listing_failed"],
  ["remote_root_unavailable_extra", undefined],
  ["SSH error: remote_root_unavailable", undefined],
  ["PRIVATE_HOST_DETAILS", undefined],
  [{ message: "remote_root_unavailable" }, undefined],
] as const)("retains only a recognized listing error code from %s", async (failure, expected) => {
  const initial = deferred<RemoteFileEntry[]>();
  const retry = deferred<RemoteFileEntry[]>();
  vi.spyOn(api, "listRemoteFiles").mockReturnValueOnce(initial.promise).mockReturnValueOnce(retry.promise);
  const { result } = renderHook(() => useRemoteFiles(project));
  await act(async () => initial.reject(failure));
  expect(result.current.status).toBe("error");
  expect(result.current.error).toBe(expected);
  act(() => { void result.current.refresh(); });
  expect(result.current.status).toBe("loading");
  expect(result.current.error).toBeUndefined();
  await act(async () => retry.resolve([]));
  expect(result.current.status).toBe("ready");
  expect(result.current.error).toBeUndefined();
});

it.each(["success", "error"] as const)("ignores an older request's %s after a newer refresh succeeds", async (outcome) => {
  const older = deferred<RemoteFileEntry[]>();
  const latest = deferred<RemoteFileEntry[]>();
  vi.spyOn(api, "listRemoteFiles").mockReturnValueOnce(older.promise).mockReturnValueOnce(latest.promise);
  const { result } = renderHook(() => useRemoteFiles(project));
  act(() => { void result.current.refresh(); });
  await act(async () => latest.resolve([entry]));
  await act(async () => outcome === "success" ? older.resolve([]) : older.reject(new Error("old failure")));
  expect(result.current.status).toBe("ready");
  expect(result.current.files).toEqual([entry]);
});

it("does not clear a newer listing error when an older request succeeds", async () => {
  const older = deferred<RemoteFileEntry[]>();
  const latest = deferred<RemoteFileEntry[]>();
  vi.spyOn(api, "listRemoteFiles").mockReturnValueOnce(older.promise).mockReturnValueOnce(latest.promise);
  const { result } = renderHook(() => useRemoteFiles(project));
  act(() => { void result.current.refresh(); });
  await act(async () => latest.reject(new Error("latest failure")));
  await act(async () => older.resolve([entry]));
  expect(result.current.status).toBe("error");
  expect(result.current.files).toEqual([]);
});

it.each([
  ["project", { ...project, id: "project-b" }],
  ["connection", { ...project, connection_id: "ssh-b" }],
  ["remote root", { ...project, remote_root: "/project-b" }],
] as const)("ignores late results and refresh callbacks after changing the %s", async (_name, replacement) => {
  const previous = deferred<RemoteFileEntry[]>();
  const current = deferred<RemoteFileEntry[]>();
  const list = vi.spyOn(api, "listRemoteFiles").mockReturnValueOnce(previous.promise).mockReturnValueOnce(current.promise);
  const { result, rerender } = renderHook(({ binding }) => useRemoteFiles(binding), { initialProps: { binding: project } });
  const obsoleteRefresh = result.current.refresh;
  rerender({ binding: replacement });
  expect(result.current.status).toBe("loading");
  expect(result.current.files).toEqual([]);
  await act(async () => obsoleteRefresh());
  expect(list).toHaveBeenCalledTimes(2);
  await act(async () => current.resolve([entry]));
  await act(async () => previous.resolve([{ ...entry, relative_path: "obsolete.tsv" }]));
  expect(result.current.status).toBe("ready");
  expect(result.current.files).toEqual([entry]);
});

it("invalidates a pending listing when a binding is removed", async () => {
  const pending = deferred<RemoteFileEntry[]>();
  vi.spyOn(api, "listRemoteFiles").mockReturnValue(pending.promise);
  const { result, rerender } = renderHook(({ binding }) => useRemoteFiles(binding), { initialProps: { binding: project as Binding | null } });
  rerender({ binding: null });
  await act(async () => pending.resolve([entry]));
  expect(result.current.status).toBe("idle");
  expect(result.current.files).toEqual([]);
});

it("does not revive an old response when returning to the same binding", async () => {
  const oldA = deferred<RemoteFileEntry[]>();
  const middleB = deferred<RemoteFileEntry[]>();
  const newA = deferred<RemoteFileEntry[]>();
  vi.spyOn(api, "listRemoteFiles").mockReturnValueOnce(oldA.promise).mockReturnValueOnce(middleB.promise).mockReturnValueOnce(newA.promise);
  const { result, rerender } = renderHook(({ binding }) => useRemoteFiles(binding), { initialProps: { binding: project } });
  rerender({ binding: { ...project, connection_id: "ssh-b" } });
  rerender({ binding: project });
  await act(async () => oldA.resolve([entry]));
  await act(async () => middleB.reject(new Error("obsolete")));
  expect(result.current.status).toBe("loading");
  expect(result.current.files).toEqual([]);
  await act(async () => newA.resolve([]));
  expect(result.current.status).toBe("ready");
  expect(result.current.files).toEqual([]);
});

it("does not dispatch from a retained refresh callback after unmount", async () => {
  const pending = deferred<RemoteFileEntry[]>();
  const list = vi.spyOn(api, "listRemoteFiles").mockReturnValue(pending.promise);
  const { result, unmount } = renderHook(() => useRemoteFiles(project));
  const refresh = result.current.refresh;
  unmount();
  await act(async () => pending.resolve([entry]));
  await refresh();
  expect(list).toHaveBeenCalledTimes(1);
});
