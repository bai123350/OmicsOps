import {
  createContext,
  createElement,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
  type ReactNode,
} from "react";

export const APPEARANCE_STORAGE_KEY = "omicsops.appearance.v1";

export type AppearanceTheme = "system" | "light" | "dark";
export type AppearanceUiFont = "system" | "sans";
export type AppearanceCodeFont = "system" | "mono";
export type AppearanceUiScale = 0.9 | 1 | 1.1 | 1.2;

export interface AppearancePreferences {
  theme: AppearanceTheme;
  uiFont: AppearanceUiFont;
  codeFont: AppearanceCodeFont;
  uiScale: AppearanceUiScale;
}

export interface AppearanceContextValue extends AppearancePreferences {
  setTheme(theme: AppearanceTheme): void;
  setUiFont(font: AppearanceUiFont): void;
  setCodeFont(font: AppearanceCodeFont): void;
  setUiScale(scale: AppearanceUiScale): void;
}

const DEFAULT_PREFERENCES: AppearancePreferences = {
  theme: "system",
  uiFont: "system",
  codeFont: "system",
  uiScale: 1,
};

const UI_FONT_STACKS: Record<AppearanceUiFont, string> = {
  system: 'system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", "Microsoft YaHei UI", sans-serif',
  sans: 'Inter, "Noto Sans SC", "Segoe UI", "Microsoft YaHei UI", Arial, sans-serif',
};

const CODE_FONT_STACKS: Record<AppearanceCodeFont, string> = {
  system: 'ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, "Liberation Mono", monospace',
  mono: '"Cascadia Code", "Cascadia Mono", Consolas, "Microsoft YaHei UI", monospace',
};

const AppearanceContext = createContext<AppearanceContextValue | undefined>(undefined);

function isTheme(value: unknown): value is AppearanceTheme {
  return value === "system" || value === "light" || value === "dark";
}

function isUiFont(value: unknown): value is AppearanceUiFont {
  return value === "system" || value === "sans";
}

function isCodeFont(value: unknown): value is AppearanceCodeFont {
  return value === "system" || value === "mono";
}

function isUiScale(value: unknown): value is AppearanceUiScale {
  return value === 0.9 || value === 1 || value === 1.1 || value === 1.2;
}

function readPreferences(): AppearancePreferences {
  if (typeof window === "undefined") return DEFAULT_PREFERENCES;

  try {
    const raw = window.localStorage.getItem(APPEARANCE_STORAGE_KEY);
    if (!raw) return DEFAULT_PREFERENCES;
    const parsed = JSON.parse(raw) as Partial<AppearancePreferences> | null;
    if (!parsed || typeof parsed !== "object") return DEFAULT_PREFERENCES;

    return {
      theme: isTheme(parsed.theme) ? parsed.theme : DEFAULT_PREFERENCES.theme,
      uiFont: isUiFont(parsed.uiFont) ? parsed.uiFont : DEFAULT_PREFERENCES.uiFont,
      codeFont: isCodeFont(parsed.codeFont) ? parsed.codeFont : DEFAULT_PREFERENCES.codeFont,
      uiScale: isUiScale(parsed.uiScale) ? parsed.uiScale : DEFAULT_PREFERENCES.uiScale,
    };
  } catch {
    return DEFAULT_PREFERENCES;
  }
}

function systemTheme(): "light" | "dark" {
  if (typeof window === "undefined" || typeof window.matchMedia !== "function") return "light";
  return window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";
}

export function AppearanceProvider({ children }: { children: ReactNode }) {
  const [preferences, setPreferences] = useState<AppearancePreferences>(readPreferences);
  const [resolvedSystemTheme, setResolvedSystemTheme] = useState<"light" | "dark">(systemTheme);

  useEffect(() => {
    if (preferences.theme !== "system" || typeof window.matchMedia !== "function") return;

    const media = window.matchMedia("(prefers-color-scheme: dark)");
    setResolvedSystemTheme(media.matches ? "dark" : "light");
    const handleChange = (event: MediaQueryListEvent) => {
      setResolvedSystemTheme(event.matches ? "dark" : "light");
    };
    media.addEventListener("change", handleChange);
    return () => media.removeEventListener("change", handleChange);
  }, [preferences.theme]);

  useEffect(() => {
    if (typeof window === "undefined") return;
    try {
      window.localStorage.setItem(APPEARANCE_STORAGE_KEY, JSON.stringify(preferences));
    } catch {
      // The in-memory preferences remain authoritative when storage is unavailable.
    }
  }, [preferences]);

  useEffect(() => {
    if (typeof document === "undefined") return;
    const root = document.documentElement;
    const resolvedTheme = preferences.theme === "system" ? resolvedSystemTheme : preferences.theme;
    root.dataset.omicsopsTheme = resolvedTheme;
    root.dataset.omicsopsUiFont = preferences.uiFont;
    root.dataset.omicsopsCodeFont = preferences.codeFont;
    root.dataset.omicsopsScale = String(preferences.uiScale);
    root.style.setProperty("--omicsops-ui-font", UI_FONT_STACKS[preferences.uiFont]);
    root.style.setProperty("--omicsops-code-font", CODE_FONT_STACKS[preferences.codeFont]);

    return () => {
      delete root.dataset.omicsopsTheme;
      delete root.dataset.omicsopsUiFont;
      delete root.dataset.omicsopsCodeFont;
      delete root.dataset.omicsopsScale;
      root.style.removeProperty("--omicsops-ui-font");
      root.style.removeProperty("--omicsops-code-font");
    };
  }, [preferences, resolvedSystemTheme]);

  const setTheme = useCallback((theme: AppearanceTheme) => {
    setPreferences((current) => ({ ...current, theme }));
  }, []);
  const setUiFont = useCallback((uiFont: AppearanceUiFont) => {
    setPreferences((current) => ({ ...current, uiFont }));
  }, []);
  const setCodeFont = useCallback((codeFont: AppearanceCodeFont) => {
    setPreferences((current) => ({ ...current, codeFont }));
  }, []);
  const setUiScale = useCallback((uiScale: AppearanceUiScale) => {
    setPreferences((current) => ({ ...current, uiScale }));
  }, []);

  const value = useMemo<AppearanceContextValue>(() => ({
    ...preferences,
    setTheme,
    setUiFont,
    setCodeFont,
    setUiScale,
  }), [preferences, setCodeFont, setTheme, setUiFont, setUiScale]);

  return createElement(AppearanceContext.Provider, { value }, children);
}

export function useAppearance(): AppearanceContextValue {
  const value = useContext(AppearanceContext);
  if (!value) throw new Error("useAppearance must be used within AppearanceProvider");
  return value;
}
