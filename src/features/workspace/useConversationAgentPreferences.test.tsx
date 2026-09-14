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

afterEach(() => vi.restoreAllMocks());

describe("useConversationAgentPreferences", () => {
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
    expect(save).toHaveBeenCalledWith("project-a", "conversation-a", disabledDelegation);
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
    expect(save).toHaveBeenNthCalledWith(1, "project-a", "conversation-a", { ...defaults, fast_mode: true });
    expect(result.current.preferences).toEqual({ ...defaults, fast_mode: true });

    act(() => result.current.setFastMode(null));
    await waitFor(() => expect(result.current.saving).toBe(false));
    expect(save).toHaveBeenNthCalledWith(2, "project-a", "conversation-a", { ...defaults, fast_mode: null });
    expect(result.current.preferences).toEqual({ ...defaults, fast_mode: null });
    expect(get).toHaveBeenCalledWith("project-a", "conversation-a");
  });

  it("rolls back a failed Fast mode save and retries the same candidate once", async () => {
    const save = vi.spyOn(api, "saveConversationAgentPreferencesV4")
      .mockRejectedValueOnce(new Error("private Fast transport"))
      .mockImplementationOnce(async (_projectId, _conversationId, preferences) => preferences);
    vi.spyOn(api, "getConversationAgentPreferencesV4").mockResolvedValue(defaults);
    const { result } = renderHook(() => useConversationAgentPreferences("project-a", "conversation-a"));
    await waitFor(() => expect(result.current.loading).toBe(false));

    act(() => result.current.setFastMode(true));
    await waitFor(() => expect(result.current.saving).toBe(false));
    expect(result.current.preferences).toEqual(defaults);
    expect(result.current.saveError).toContain("Could not save conversation preferences");
    expect(result.current.saveError).not.toContain("private Fast transport");

    act(() => result.current.retry());
    await waitFor(() => expect(result.current.saving).toBe(false));
    expect(save).toHaveBeenNthCalledWith(2, "project-a", "conversation-a", { ...defaults, fast_mode: true });
    expect(result.current.preferences).toEqual({ ...defaults, fast_mode: true });
    expect(result.current.saveError).toBe("");
  });

  it("rejects malformed optional Fast values from the host", async () => {
    vi.spyOn(api, "getConversationAgentPreferencesV4").mockResolvedValue({ ...defaults, fast_mode: "fast" as unknown as boolean });
    const { result } = renderHook(() => useConversationAgentPreferences("project-a", "conversation-a"));

    await waitFor(() => expect(result.current.loadError).toContain("Could not load conversation preferences"));
    expect(result.current.preferences).toEqual(defaults);
  });

  it("rolls back a rejected save, exposes a safe retry, and ignores duplicate toggles while busy", async () => {
    const request = deferred<ConversationAgentPreferencesV4>();
    const save = vi.spyOn(api, "saveConversationAgentPreferencesV4").mockReturnValueOnce(request.promise).mockResolvedValueOnce(disabledDelegation);
    vi.spyOn(api, "getConversationAgentPreferencesV4").mockResolvedValue(defaults);
    const { result } = renderHook(() => useConversationAgentPreferences("project-a", "conversation-a"));
    await waitFor(() => expect(result.current.loading).toBe(false));

    act(() => result.current.toggle("delegation_enabled"));
    act(() => result.current.toggle("auto_review"));
    expect(save).toHaveBeenCalledTimes(1);
    expect(result.current.preferences).toEqual(disabledDelegation);
    expect(result.current.saving).toBe(true);

    await act(async () => request.reject(new Error("private transport details")));
    expect(result.current.preferences).toEqual(defaults);
    expect(result.current.error).toContain("Could not save conversation preferences");
    expect(result.current.error).not.toContain("private transport details");

    act(() => result.current.retry());
    expect(save).toHaveBeenCalledTimes(2);
    await waitFor(() => expect(result.current.saving).toBe(false));
    expect(result.current.preferences).toEqual(disabledDelegation);
    expect(result.current.error).toBe("");
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
    expect(save).toHaveBeenCalledWith("project-a", "conversation-b", { delegation_enabled: false, auto_review: true, memory_enabled: false });
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
    expect(result.current.busy).toBe(true);
    expect(result.current.error).not.toContain("private load details");

    act(() => result.current.retry());
    expect(get).toHaveBeenLastCalledWith("project-a", "conversation-b");
    await act(async () => retry.resolve({ ...defaults, memory_enabled: false }));
    await waitFor(() => expect(result.current.busy).toBe(false));
    expect(result.current.preferences.memory_enabled).toBe(false);
  });
});
