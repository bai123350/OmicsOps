import { act, renderHook, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import * as api from "../../conversation-preferences-api";
import type { ConversationAgentPreferencesV4 } from "../../types";
import { useConversationAgentPreferences } from "./useConversationAgentPreferences";

const defaults: ConversationAgentPreferencesV4 = {
  delegation_enabled: true,
  auto_review: true,
  memory_enabled: true,
};

const disabledDelegation: ConversationAgentPreferencesV4 = {
  delegation_enabled: false,
  auto_review: true,
  memory_enabled: true,
};

function deferred<T>() {
  let resolve!: (value: T | PromiseLike<T>) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise;
    reject = rejectPromise;
  });
  return { promise, resolve, reject };
}

afterEach(() => { vi.restoreAllMocks(); vi.useRealTimers(); localStorage.clear(); });

describe("useConversationAgentPreferences", () => {
  it("ends a stalled preference load with a retryable error while keeping the scope locked", async () => {
    const stalled = deferred<ConversationAgentPreferencesV4>();
    vi.spyOn(api, "getConversationAgentPreferencesV4")
      .mockReturnValueOnce(stalled.promise)
      .mockResolvedValueOnce({ ...defaults, memory_enabled: false });
    vi.useFakeTimers();
    const { result } = renderHook(() => useConversationAgentPreferences("project-a", "conversation-a"));

    expect(result.current.loading).toBe(true);
    await act(async () => { await vi.advanceTimersByTimeAsync(120_000); });
    expect(result.current.loading).toBe(false);
    expect(result.current.loadError).toContain("Could not load conversation preferences");
    expect(result.current.busy).toBe(false);

    act(() => result.current.retry());
    await act(async () => { await vi.advanceTimersByTimeAsync(0); });
    expect(result.current.preferences.memory_enabled).toBe(false);
    expect(result.current.busy).toBe(false);
    await act(async () => stalled.resolve({ ...defaults, memory_enabled: true }));
    expect(result.current.preferences.memory_enabled).toBe(false);
  });

  it("loads defaults and persists a toggle with the exact conversation scope", async () => {
    const load = vi.spyOn(api, "getConversationAgentPreferencesV4").mockResolvedValue(defaults);
    const save = vi.spyOn(api, "saveConversationAgentPreferencesV4").mockResolvedValue(disabledDelegation);
    const { result } = renderHook(() => useConversationAgentPreferences("project-a", "conversation-a"));

    await waitFor(() => expect(result.current.loading).toBe(false));
    expect(result.current.preferences).toEqual(defaults);
    act(() => result.current.setPreference("delegation_enabled", false));

    expect(result.current.preferences.delegation_enabled).toBe(false);
    expect(result.current.saving).toBe(true);
    await waitFor(() => expect(result.current.saving).toBe(false));
    expect(save).toHaveBeenCalledWith("project-a", "conversation-a", disabledDelegation, expect.any(Number));
    expect(load).toHaveBeenCalledWith("project-a", "conversation-a");
    expect(result.current.error).toBe("");
  });

  it("persists Fast mode choices and reset to model inheritance without changing other preferences", async () => {
    const get = vi.spyOn(api, "getConversationAgentPreferencesV4").mockResolvedValue(defaults);
    const save = vi.spyOn(api, "saveConversationAgentPreferencesV4").mockImplementation(async (_projectId, _conversationId, preferences) => preferences);
    const { result } = renderHook(() => useConversationAgentPreferences("project-a", "conversation-a"));

    await waitFor(() => expect(result.current.loading).toBe(false));
    act(() => result.current.setFastMode(true));
    await waitFor(() => expect(result.current.saving).toBe(false));
    expect(save).toHaveBeenNthCalledWith(1, "project-a", "conversation-a", { ...defaults, fast_mode: true }, expect.any(Number));
    expect(result.current.preferences).toEqual({ ...defaults, fast_mode: true });

    act(() => result.current.setFastMode(null));
    await waitFor(() => expect(result.current.saving).toBe(false));
    expect(save).toHaveBeenNthCalledWith(2, "project-a", "conversation-a", { ...defaults, fast_mode: null }, expect.any(Number));
    expect(result.current.preferences).toEqual({ ...defaults, fast_mode: null });
    expect(get).toHaveBeenCalledWith("project-a", "conversation-a");
  });

  it("reconciles an unconfirmed Fast save before allowing another message", async () => {
    vi.useFakeTimers();
    const save = vi.spyOn(api, "saveConversationAgentPreferencesV4")
      .mockRejectedValueOnce(new Error("private Fast transport"));
    vi.spyOn(api, "getConversationAgentPreferencesV4").mockResolvedValue(defaults);
    const reconcile = vi.spyOn(api, "reconcileConversationAgentPreferencesV4").mockResolvedValue({ ...defaults, fast_mode: true });
    const { result } = renderHook(() => useConversationAgentPreferences("project-a", "conversation-a"));
    await act(async () => { await vi.advanceTimersByTimeAsync(0); });

    act(() => result.current.setFastMode(true));
    await act(async () => { await vi.advanceTimersByTimeAsync(0); });
    expect(result.current.unconfirmed).toBe(true);
    expect(result.current.blocked).toBe(true);
    expect(result.current.saveError).toContain("Could not confirm");
    expect(result.current.saveError).not.toContain("private Fast transport");

    await act(async () => { await vi.advanceTimersByTimeAsync(10_000); });
    act(() => result.current.retry());
    await act(async () => { await vi.advanceTimersByTimeAsync(0); });
    expect(reconcile).toHaveBeenCalledWith("project-a", "conversation-a", expect.any(Number));
    expect(save).toHaveBeenCalledTimes(1);
    expect(result.current.preferences).toEqual({ ...defaults, fast_mode: true });
    expect(result.current.blocked).toBe(false);
  });

  it("rejects malformed optional Fast values from the host", async () => {
    vi.spyOn(api, "getConversationAgentPreferencesV4").mockResolvedValue({ ...defaults, fast_mode: "fast" as unknown as boolean });
    const { result } = renderHook(() => useConversationAgentPreferences("project-a", "conversation-a"));

    await waitFor(() => expect(result.current.loadError).toContain("Could not load conversation preferences"));
    expect(result.current.preferences).toEqual(defaults);
  });

  it("holds an uncertain save and ignores duplicate toggles until a fenced read returns the durable value", async () => {
    vi.useFakeTimers();
    const request = deferred<ConversationAgentPreferencesV4>();
    const save = vi.spyOn(api, "saveConversationAgentPreferencesV4").mockReturnValue(request.promise);
    vi.spyOn(api, "getConversationAgentPreferencesV4").mockResolvedValue(defaults);
    vi.spyOn(api, "reconcileConversationAgentPreferencesV4").mockResolvedValue(defaults);
    const { result } = renderHook(() => useConversationAgentPreferences("project-a", "conversation-a"));
    await act(async () => { await vi.advanceTimersByTimeAsync(0); });

    act(() => result.current.toggle("delegation_enabled"));
    act(() => result.current.toggle("auto_review"));
    expect(save).toHaveBeenCalledTimes(1);
    expect(result.current.preferences).toEqual(disabledDelegation);
    expect(result.current.saving).toBe(true);

    await act(async () => { await vi.advanceTimersByTimeAsync(15_000); });
    expect(result.current.unconfirmed).toBe(true);
    expect(result.current.saving).toBe(false);
    expect(result.current.blocked).toBe(true);
    act(() => result.current.toggle("memory_enabled"));
    expect(save).toHaveBeenCalledTimes(1);

    act(() => result.current.retry());
    await act(async () => { await vi.advanceTimersByTimeAsync(0); });
    expect(result.current.preferences).toEqual(defaults);
    expect(result.current.blocked).toBe(false);
    await act(async () => request.resolve(disabledDelegation));
    expect(result.current.preferences).toEqual(defaults);
    expect(save).toHaveBeenCalledTimes(1);
  });

  it("keeps an unknown save fenced across hook remount before accepting its durable value", async () => {
    vi.useFakeTimers();
    const stalled = deferred<ConversationAgentPreferencesV4>();
    const get = vi.spyOn(api, "getConversationAgentPreferencesV4").mockResolvedValue(defaults);
    vi.spyOn(api, "saveConversationAgentPreferencesV4").mockReturnValue(stalled.promise);
    const reconcile = vi.spyOn(api, "reconcileConversationAgentPreferencesV4").mockResolvedValue(disabledDelegation);
    const first = renderHook(() => useConversationAgentPreferences("project-a", "conversation-a"));
    await act(async () => { await vi.advanceTimersByTimeAsync(0); });
    act(() => first.result.current.setPreference("delegation_enabled", false));
    first.unmount();

    const restored = renderHook(() => useConversationAgentPreferences("project-a", "conversation-a"));
    expect(restored.result.current.blocked).toBe(true);
    await act(async () => { await vi.advanceTimersByTimeAsync(10_000); });
    expect(reconcile).toHaveBeenCalledOnce();
    expect(get).toHaveBeenCalledOnce();
    expect(restored.result.current.preferences).toEqual(disabledDelegation);
    expect(restored.result.current.blocked).toBe(false);
    await act(async () => stalled.resolve(defaults));
    expect(restored.result.current.preferences).toEqual(disabledDelegation);
  });

  it("clears a save that the local API proves was never dispatched", async () => {
    vi.spyOn(api, "getConversationAgentPreferencesV4").mockResolvedValue(defaults);
    vi.spyOn(api, "saveConversationAgentPreferencesV4").mockRejectedValue(new api.PreferenceSaveNotDispatchedError("desktop required"));
    const { result } = renderHook(() => useConversationAgentPreferences("project-a", "conversation-a"));
    await waitFor(() => expect(result.current.loading).toBe(false));
    act(() => result.current.setPreference("delegation_enabled", false));
    await waitFor(() => expect(result.current.saving).toBe(false));
    expect(result.current.preferences).toEqual(defaults);
    expect(result.current.blocked).toBe(false);
    expect(Object.keys(localStorage).some((key) => key.startsWith("omicsops.preferences.pendingSave:project-a:conversation-a:"))).toBe(false);
  });

  it("does not accept a GET snapshot when another save starts before it returns", async () => {
    const get = deferred<ConversationAgentPreferencesV4>();
    vi.spyOn(api, "getConversationAgentPreferencesV4").mockReturnValue(get.promise);
    const { result } = renderHook(() => useConversationAgentPreferences("project-a", "conversation-a"));
    await act(async () => { await Promise.resolve(); });
    const key = "omicsops.preferences.pendingSave:project-a:conversation-a:new-write";
    localStorage.setItem(key, String(Date.now() - 1));
    await act(async () => get.resolve(defaults));
    expect(result.current.blocked).toBe(true);
    expect(result.current.loadError).toContain("Could not load conversation preferences");
    expect(localStorage.getItem(key)).not.toBeNull();
  });

  it("does not let an older fence clear a newer save marker with the same deadline", async () => {
    const deadline = Date.now() - 1;
    const firstKey = "omicsops.preferences.pendingSave:project-a:conversation-a:first-write";
    const secondKey = "omicsops.preferences.pendingSave:project-a:conversation-a:second-write";
    localStorage.setItem(firstKey, String(deadline));
    const read = deferred<ConversationAgentPreferencesV4>();
    vi.spyOn(api, "reconcileConversationAgentPreferencesV4").mockReturnValue(read.promise);
    const { result } = renderHook(() => useConversationAgentPreferences("project-a", "conversation-a"));
    await act(async () => { await Promise.resolve(); });
    localStorage.setItem(secondKey, String(deadline));
    await act(async () => read.resolve(defaults));
    expect(result.current.blocked).toBe(true);
    expect(localStorage.getItem(secondKey)).toBe(String(deadline));
  });

  it("does not let an old save acknowledgement clear a newer operation marker", async () => {
    const save = deferred<ConversationAgentPreferencesV4>();
    vi.spyOn(api, "getConversationAgentPreferencesV4").mockResolvedValue(defaults);
    vi.spyOn(api, "saveConversationAgentPreferencesV4").mockReturnValue(save.promise);
    const { result } = renderHook(() => useConversationAgentPreferences("project-a", "conversation-a"));
    await waitFor(() => expect(result.current.loading).toBe(false));
    act(() => result.current.setPreference("delegation_enabled", false));
    const prefix = "omicsops.preferences.pendingSave:project-a:conversation-a:";
    const originalKey = Object.keys(localStorage).find((key) => key.startsWith(prefix))!;
    const deadline = localStorage.getItem(originalKey)!;
    const newerKey = `${prefix}newer-write`;
    localStorage.setItem(newerKey, deadline);
    await act(async () => save.resolve(disabledDelegation));
    expect(result.current.blocked).toBe(true);
    expect(localStorage.getItem(newerKey)).toBe(deadline);
  });

  it("blocks a save when another pending marker cannot be read", async () => {
    vi.spyOn(api, "getConversationAgentPreferencesV4").mockResolvedValue(defaults);
    const save = vi.spyOn(api, "saveConversationAgentPreferencesV4");
    const { result } = renderHook(() => useConversationAgentPreferences("project-a", "conversation-a"));
    await waitFor(() => expect(result.current.loading).toBe(false));
    localStorage.setItem("omicsops.preferences.pendingSave:project-a:conversation-a:broken", "invalid");
    act(() => result.current.setPreference("delegation_enabled", false));
    expect(save).not.toHaveBeenCalled();
    expect(result.current.preferences).toEqual(defaults);
    expect(result.current.blocked).toBe(true);
  });

  it("does not dispatch a save when its recovery marker cannot be written", async () => {
    vi.spyOn(api, "getConversationAgentPreferencesV4").mockResolvedValue(defaults);
    const save = vi.spyOn(api, "saveConversationAgentPreferencesV4");
    const { result } = renderHook(() => useConversationAgentPreferences("project-a", "conversation-a"));
    await waitFor(() => expect(result.current.loading).toBe(false));
    vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => { throw new Error("storage unavailable"); });
    act(() => result.current.setPreference("delegation_enabled", false));
    expect(save).not.toHaveBeenCalled();
    expect(result.current.preferences).toEqual(defaults);
    expect(result.current.blocked).toBe(false);
    expect(result.current.saveError).toContain("Could not safely start");
  });

  it("clears the previous conversation and ignores stale load and save responses", async () => {
    const oldLoad = deferred<ConversationAgentPreferencesV4>();
    const newLoad = deferred<ConversationAgentPreferencesV4>();
    const oldSave = deferred<ConversationAgentPreferencesV4>();
    const get = vi.spyOn(api, "getConversationAgentPreferencesV4")
      .mockReturnValueOnce(oldLoad.promise)
      .mockReturnValueOnce(newLoad.promise);
    const save = vi.spyOn(api, "saveConversationAgentPreferencesV4").mockReturnValue(oldSave.promise);
    const { result, rerender } = renderHook(({ conversationId }) => useConversationAgentPreferences("project-a", conversationId), { initialProps: { conversationId: "conversation-a" } });

    rerender({ conversationId: "conversation-b" });
    expect(result.current.loading).toBe(true);
    expect(result.current.preferences).toEqual(defaults);
    await act(async () => newLoad.resolve({ ...defaults, memory_enabled: false }));
    await waitFor(() => expect(result.current.loading).toBe(false));
    expect(result.current.preferences.memory_enabled).toBe(false);
    expect(get).toHaveBeenLastCalledWith("project-a", "conversation-b");

    act(() => result.current.setPreference("delegation_enabled", false));
    expect(save).toHaveBeenCalledWith("project-a", "conversation-b", { delegation_enabled: false, auto_review: true, memory_enabled: false }, expect.any(Number));
    rerender({ conversationId: "conversation-c" });
    await act(async () => {
      oldLoad.resolve({ delegation_enabled: false, auto_review: false, memory_enabled: false });
      oldSave.resolve({ delegation_enabled: false, auto_review: false, memory_enabled: false });
    });
    expect(result.current.preferences).toEqual(defaults);
    expect(result.current.error).toBe("");
  });

  it("does not toggle while externally disabled or without an active conversation", async () => {
    const save = vi.spyOn(api, "saveConversationAgentPreferencesV4");
    vi.spyOn(api, "getConversationAgentPreferencesV4").mockResolvedValue(defaults);
    const { result: disabled } = renderHook(() => useConversationAgentPreferences("project-a", "conversation-a", true));
    await waitFor(() => expect(disabled.current.loading).toBe(false));
    act(() => disabled.current.toggle("memory_enabled"));
    expect(disabled.current.preferences).toEqual(defaults);
    expect(save).not.toHaveBeenCalled();

    const { result: noConversation } = renderHook(() => useConversationAgentPreferences("project-a", null));
    expect(noConversation.current.loading).toBe(false);
    act(() => noConversation.current.toggle("memory_enabled"));
    expect(noConversation.current.preferences).toEqual(defaults);
  });

  it("keeps a changed scope busy until its snapshot is loaded and offers a safe load retry", async () => {
    const first = deferred<ConversationAgentPreferencesV4>();
    const second = deferred<ConversationAgentPreferencesV4>();
    const retry = deferred<ConversationAgentPreferencesV4>();
    const get = vi.spyOn(api, "getConversationAgentPreferencesV4")
      .mockReturnValueOnce(first.promise)
      .mockReturnValueOnce(second.promise)
      .mockReturnValueOnce(retry.promise);
    const { result, rerender } = renderHook(({ conversationId }) => useConversationAgentPreferences("project-a", conversationId), { initialProps: { conversationId: "conversation-a" } });

    expect(result.current.busy).toBe(true);
    await act(async () => first.resolve(defaults));
    await waitFor(() => expect(result.current.busy).toBe(false));

    rerender({ conversationId: "conversation-b" });
    expect(result.current.busy).toBe(true);
    await act(async () => second.reject(new Error("private load details")));
    await waitFor(() => expect(result.current.loadError).toContain("Could not load conversation preferences"));
    expect(result.current.busy).toBe(false);
    expect(result.current.error).not.toContain("private load details");

    act(() => result.current.retry());
    expect(get).toHaveBeenLastCalledWith("project-a", "conversation-b");
    await act(async () => retry.resolve({ ...defaults, memory_enabled: false }));
    await waitFor(() => expect(result.current.busy).toBe(false));
    expect(result.current.preferences.memory_enabled).toBe(false);
  });
});
