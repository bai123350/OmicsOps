import { act, renderHook, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import {
  APPEARANCE_STORAGE_KEY,
  AppearanceProvider,
  useAppearance,
} from "./use-appearance";

type MediaListener = (event: MediaQueryListEvent) => void;

function installMatchMedia(initialMatches: boolean) {
  let matches = initialMatches;
  const listeners = new Set<MediaListener>();
  const addEventListener = vi.fn((_type: string, listener: MediaListener) => listeners.add(listener));
  const removeEventListener = vi.fn((_type: string, listener: MediaListener) => listeners.delete(listener));

  Object.defineProperty(window, "matchMedia", {
    configurable: true,
    value: vi.fn(() => ({
      get matches() {
        return matches;
      },
      media: "(prefers-color-scheme: dark)",
      onchange: null,
      addEventListener,
      removeEventListener,
      addListener: vi.fn(),
      removeListener: vi.fn(),
      dispatchEvent: vi.fn(),
    })),
  });

  return {
    addEventListener,
    removeEventListener,
    emit(nextMatches: boolean) {
      matches = nextMatches;
      const event = { matches: nextMatches, media: "(prefers-color-scheme: dark)" } as MediaQueryListEvent;
      for (const listener of listeners) listener(event);
    },
  };
}

const wrapper = ({ children }: { children: ReactNode }) => (
  <AppearanceProvider>{children}</AppearanceProvider>
);

beforeEach(() => {
  window.localStorage.clear();
  installMatchMedia(false);
});

afterEach(() => {
  document.documentElement.removeAttribute("data-omicsops-theme");
  document.documentElement.removeAttribute("data-omicsops-ui-font");
  document.documentElement.removeAttribute("data-omicsops-code-font");
  document.documentElement.removeAttribute("data-omicsops-scale");
  document.documentElement.style.removeProperty("--omicsops-ui-font");
  document.documentElement.style.removeProperty("--omicsops-code-font");
});

describe("AppearanceProvider", () => {
  it("persists supported preferences and applies their DOM contract", async () => {
    const { result } = renderHook(() => useAppearance(), { wrapper });

    act(() => {
      result.current.setTheme("dark");
      result.current.setUiFont("sans");
      result.current.setCodeFont("mono");
      result.current.setUiScale(1.2);
    });

    expect(result.current.theme).toBe("dark");
    expect(result.current.uiFont).toBe("sans");
    expect(result.current.codeFont).toBe("mono");
    expect(result.current.uiScale).toBe(1.2);
    await waitFor(() => {
      expect(document.documentElement).toHaveAttribute("data-omicsops-theme", "dark");
      expect(document.documentElement).toHaveAttribute("data-omicsops-ui-font", "sans");
      expect(document.documentElement).toHaveAttribute("data-omicsops-code-font", "mono");
      expect(document.documentElement).toHaveAttribute("data-omicsops-scale", "1.2");
    });
    expect(document.documentElement.style.getPropertyValue("--omicsops-ui-font")).toContain("Inter");
    expect(document.documentElement.style.getPropertyValue("--omicsops-code-font")).toContain("Cascadia Code");
    expect(JSON.parse(window.localStorage.getItem(APPEARANCE_STORAGE_KEY)!)).toEqual({
      theme: "dark",
      uiFont: "sans",
      codeFont: "mono",
      uiScale: 1.2,
    });
  });

  it("restores supported values and falls back per field for malformed or unsupported values", () => {
    window.localStorage.setItem(APPEARANCE_STORAGE_KEY, JSON.stringify({
      theme: "midnight",
      uiFont: "https://example.com/font.woff2",
      codeFont: "mono",
      uiScale: 5,
    }));

    const first = renderHook(() => useAppearance(), { wrapper });
    expect(first.result.current.theme).toBe("system");
    expect(first.result.current.uiFont).toBe("system");
    expect(first.result.current.codeFont).toBe("mono");
    expect(first.result.current.uiScale).toBe(1);
    first.unmount();

    window.localStorage.setItem(APPEARANCE_STORAGE_KEY, "not-json");
    const second = renderHook(() => useAppearance(), { wrapper });
    expect(second.result.current).toMatchObject({
      theme: "system",
      uiFont: "system",
      codeFont: "system",
      uiScale: 1,
    });
  });

  it("tracks the system theme only while the system preference is selected and cleans up its listener", async () => {
    const media = installMatchMedia(true);
    const { result, unmount } = renderHook(() => useAppearance(), { wrapper });

    await waitFor(() => expect(document.documentElement).toHaveAttribute("data-omicsops-theme", "dark"));
    expect(media.addEventListener).toHaveBeenCalledTimes(1);

    act(() => media.emit(false));
    expect(document.documentElement).toHaveAttribute("data-omicsops-theme", "light");

    act(() => result.current.setTheme("dark"));
    expect(media.removeEventListener).toHaveBeenCalledTimes(1);
    act(() => media.emit(false));
    expect(document.documentElement).toHaveAttribute("data-omicsops-theme", "dark");

    act(() => result.current.setTheme("system"));
    expect(media.addEventListener).toHaveBeenCalledTimes(2);
    unmount();
    expect(media.removeEventListener).toHaveBeenCalledTimes(2);
  });

  it("keeps preferences and applies them immediately when localStorage is unavailable", async () => {
    const descriptor = Object.getOwnPropertyDescriptor(window, "localStorage");
    Object.defineProperty(window, "localStorage", {
      configurable: true,
      get() {
        throw new Error("storage denied");
      },
    });

    try {
      const { result } = renderHook(() => useAppearance(), { wrapper });
      act(() => {
        result.current.setTheme("dark");
        result.current.setUiScale(1.1);
      });

      expect(result.current.theme).toBe("dark");
      expect(result.current.uiScale).toBe(1.1);
      await waitFor(() => {
        expect(document.documentElement).toHaveAttribute("data-omicsops-theme", "dark");
        expect(document.documentElement).toHaveAttribute("data-omicsops-scale", "1.1");
      });
    } finally {
      if (descriptor) Object.defineProperty(window, "localStorage", descriptor);
    }
  });
});
