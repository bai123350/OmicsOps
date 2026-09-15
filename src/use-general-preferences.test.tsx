import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { setSelectionActionsEnabled, useGeneralPreferences } from "./use-general-preferences";

beforeEach(() => {
  window.localStorage.clear();
  setSelectionActionsEnabled(true);
});

afterEach(() => {
  vi.restoreAllMocks();
  setSelectionActionsEnabled(true);
});

describe("general preferences", () => {
  it("enables message selection actions by default and syncs every consumer", () => {
    const first = renderHook(() => useGeneralPreferences());
    const second = renderHook(() => useGeneralPreferences());
    expect(first.result.current.selectionActionsEnabled).toBe(true);

    act(() => first.result.current.setSelectionActionsEnabled(false));

    expect(first.result.current.selectionActionsEnabled).toBe(false);
    expect(second.result.current.selectionActionsEnabled).toBe(false);
    expect(window.localStorage.getItem("omicsops.general.selectionActionsEnabled")).toBe("false");
  });

  it("keeps a changed value in memory when localStorage rejects the write", () => {
    const hook = renderHook(() => useGeneralPreferences());
    vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => { throw new DOMException("quota", "QuotaExceededError"); });

    act(() => hook.result.current.setSelectionActionsEnabled(false));
    hook.rerender();

    expect(hook.result.current.selectionActionsEnabled).toBe(false);
  });
});
