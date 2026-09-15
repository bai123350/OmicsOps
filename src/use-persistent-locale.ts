import { useCallback, useState } from "react";

import type { Locale } from "./features/workspace/copy";

export const LOCALE_STORAGE_KEY = "omicsops.locale";

function isLocale(value: string | null): value is Locale {
  return value === "zh-CN" || value === "en-US";
}

function readStoredLocale(): Locale {
  if (typeof window === "undefined") return "zh-CN";

  try {
    const stored = window.localStorage.getItem(LOCALE_STORAGE_KEY);
    return isLocale(stored) ? stored : "zh-CN";
  } catch {
    return "zh-CN";
  }
}

export function usePersistentLocale(): [Locale, (locale: Locale) => void] {
  const [locale, setLocaleState] = useState<Locale>(readStoredLocale);

  const setLocale = useCallback((nextLocale: Locale) => {
    setLocaleState(nextLocale);

    if (typeof window === "undefined") return;
    try {
      window.localStorage.setItem(LOCALE_STORAGE_KEY, nextLocale);
    } catch {
      // Keep the in-memory preference when browser storage is unavailable.
    }
  }, []);

  return [locale, setLocale];
}
