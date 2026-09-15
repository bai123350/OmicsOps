import { act, renderHook } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";

import type { Locale } from "./features/workspace/copy";
import { usePersistentLocale } from "./use-persistent-locale";

beforeEach(() => {
  window.localStorage.clear();
});

describe("usePersistentLocale", () => {
  it("defaults to Chinese when no locale has been saved", () => {
    const { result } = renderHook(() => usePersistentLocale());

    expect(result.current[0]).toBe("zh-CN");
  });

  it.each<Locale>(["zh-CN", "en-US"])("restores the supported locale %s", (locale) => {
    window.localStorage.setItem("omicsops.locale", locale);

    const { result } = renderHook(() => usePersistentLocale());

    expect(result.current[0]).toBe(locale);
  });

  it("ignores an unsupported saved locale", () => {
    window.localStorage.setItem("omicsops.locale", "fr-FR");

    const { result } = renderHook(() => usePersistentLocale());

    expect(result.current[0]).toBe("zh-CN");
  });

  it("saves a locale change so a later mount restores it", () => {
    const first = renderHook(() => usePersistentLocale());
    act(() => first.result.current[1]("en-US"));
    expect(window.localStorage.getItem("omicsops.locale")).toBe("en-US");
    first.unmount();

    const second = renderHook(() => usePersistentLocale());

    expect(second.result.current[0]).toBe("en-US");
  });

  it("keeps locale changes usable when browser storage cannot be read or written", () => {
    const descriptor = Object.getOwnPropertyDescriptor(window, "localStorage");
    Object.defineProperty(window, "localStorage", {
      configurable: true,
      get() {
        throw new Error("storage denied");
      },
    });

    try {
      const { result } = renderHook(() => usePersistentLocale());
      expect(result.current[0]).toBe("zh-CN");

      act(() => result.current[1]("en-US"));

      expect(result.current[0]).toBe("en-US");
    } finally {
      if (descriptor) Object.defineProperty(window, "localStorage", descriptor);
    }
  });
});
