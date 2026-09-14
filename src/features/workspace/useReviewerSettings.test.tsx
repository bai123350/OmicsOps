import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { getReviewerSettingsV4, saveReviewerSettingsV4 } from "../../session-review-api";
import type { ReviewerSettingsV4 } from "../../types";
import { useReviewerSettings } from "./useReviewerSettings";

vi.mock("../../session-review-api", () => ({
  DEFAULT_REVIEWER_SETTINGS: { backend: { kind: "follow_session" }, default_http_profile_id: null },
  getReviewerSettingsV4: vi.fn(),
  saveReviewerSettingsV4: vi.fn(),
}));

const getSettings = vi.mocked(getReviewerSettingsV4);
const saveSettings = vi.mocked(saveReviewerSettingsV4);
const defaults: ReviewerSettingsV4 = { backend: { kind: "follow_session" }, default_http_profile_id: null };

function deferred<T>() {
  let resolve!: (value: T | PromiseLike<T>) => void;
  const promise = new Promise<T>((resolvePromise) => { resolve = resolvePromise; });
  return { promise, resolve };
}

beforeEach(() => {
  vi.clearAllMocks();
  getSettings.mockResolvedValue(defaults);
  saveSettings.mockImplementation(async (settings) => settings);
});

describe("useReviewerSettings", () => {
  it("loads global settings and saves a precise backend choice", async () => {
    const { result } = renderHook(() => useReviewerSettings());
    await waitFor(() => expect(result.current.loading).toBe(false));

    const next: ReviewerSettingsV4 = { backend: { kind: "http_profile", profile_id: "profile-ollama" }, default_http_profile_id: "profile-default" };
    act(() => result.current.setSettings(next));
    await act(async () => { await result.current.save(); });

    expect(saveSettings).toHaveBeenCalledWith(next);
    expect(result.current.settings).toEqual(next);
  });

  it("rolls back a rejected save and retries the same candidate without exposing transport details", async () => {
    saveSettings.mockRejectedValueOnce(new Error("private native detail"));
    const { result } = renderHook(() => useReviewerSettings());
    await waitFor(() => expect(result.current.loading).toBe(false));
    const next: ReviewerSettingsV4 = { backend: { kind: "default_http" }, default_http_profile_id: "profile-http" };
    act(() => result.current.setSettings(next));
    await act(async () => { await result.current.save(); });

    expect(result.current.settings).toEqual(defaults);
    expect(result.current.saveError).toContain("Could not save reviewer settings");
    expect(result.current.saveError).not.toContain("private native detail");
    await act(async () => { result.current.retrySave(); });
    await waitFor(() => expect(saveSettings).toHaveBeenCalledTimes(2));
    expect(saveSettings.mock.calls[1][0]).toEqual(next);
  });

  it("does not persist defaults after a failed initial load", async () => {
    getSettings.mockRejectedValueOnce(new Error("private load detail"));
    const { result } = renderHook(() => useReviewerSettings());
    await waitFor(() => expect(result.current.loadError).toContain("Could not load reviewer settings"));

    let accepted = true;
    await act(async () => { accepted = await result.current.save({ backend: { kind: "default_http" }, default_http_profile_id: "profile-a" }); });

    expect(accepted).toBe(false);
    expect(saveSettings).not.toHaveBeenCalled();
  });

  it("ignores a stale load after unmount", async () => {
    const pending = deferred<ReviewerSettingsV4>();
    getSettings.mockReturnValueOnce(pending.promise);
    const { result, unmount } = renderHook(() => useReviewerSettings());
    unmount();
    await act(async () => pending.resolve({ backend: { kind: "http_profile", profile_id: "stale" }, default_http_profile_id: null }));
    expect(result.current.settings).toEqual(defaults);
  });
});
