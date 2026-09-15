import { act, renderHook } from "@testing-library/react";
import type { ReactNode } from "react";
import { beforeEach, describe, expect, it } from "vitest";

import {
  PET_APPEARANCE_STORAGE_KEY,
  PET_ENABLED_STORAGE_KEY,
  PetPreferencesProvider,
  usePetPreferences,
} from "./use-pet-preferences";

const wrapper = ({ children }: { children: ReactNode }) => (
  <PetPreferencesProvider>{children}</PetPreferencesProvider>
);

beforeEach(() => window.localStorage.clear());

describe("PetPreferencesProvider", () => {
  it("restores supported preferences and persists changes immediately", () => {
    window.localStorage.setItem(PET_ENABLED_STORAGE_KEY, "true");
    window.localStorage.setItem(PET_APPEARANCE_STORAGE_KEY, JSON.stringify({
      name: "Nova",
      style: "cell",
      size: "large",
    }));

    const { result } = renderHook(() => usePetPreferences(), { wrapper });
    expect(result.current).toMatchObject({ enabled: true, name: "Nova", style: "cell", size: "large" });

    act(() => {
      result.current.setEnabled(false);
      result.current.setName("Pico");
      result.current.setStyle("orb");
      result.current.setSize("small");
    });

    expect(result.current).toMatchObject({ enabled: false, name: "Pico", style: "orb", size: "small" });
    expect(window.localStorage.getItem(PET_ENABLED_STORAGE_KEY)).toBe("false");
    expect(JSON.parse(window.localStorage.getItem(PET_APPEARANCE_STORAGE_KEY)!)).toEqual({
      name: "Pico",
      style: "orb",
      size: "small",
    });
  });

  it("falls back from malformed storage without losing later in-memory changes", () => {
    window.localStorage.setItem(PET_ENABLED_STORAGE_KEY, "sometimes");
    window.localStorage.setItem(PET_APPEARANCE_STORAGE_KEY, JSON.stringify({
      name: 42,
      style: "downloaded-cat",
      size: "huge",
    }));

    const descriptor = Object.getOwnPropertyDescriptor(window, "localStorage");
    const originalStorage = window.localStorage;
    let denyWrites = false;
    Object.defineProperty(window, "localStorage", {
      configurable: true,
      get() {
        if (denyWrites) throw new Error("storage denied");
        return originalStorage;
      },
    });

    try {
      const { result } = renderHook(() => usePetPreferences(), { wrapper });
      expect(result.current).toMatchObject({ enabled: false, name: "Pip", style: "orb", size: "medium" });

      denyWrites = true;
      act(() => {
        result.current.setEnabled(true);
        result.current.setName("Mica");
      });

      expect(result.current.enabled).toBe(true);
      expect(result.current.name).toBe("Mica");
    } finally {
      if (descriptor) Object.defineProperty(window, "localStorage", descriptor);
    }
  });

  it("fails clearly when the hook is used without its provider", () => {
    expect(() => renderHook(() => usePetPreferences())).toThrow(
      "usePetPreferences must be used within PetPreferencesProvider",
    );
  });
});
