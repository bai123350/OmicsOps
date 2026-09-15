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

export const PET_ENABLED_STORAGE_KEY = "omicsops.pet.enabled";
export const PET_APPEARANCE_STORAGE_KEY = "omicsops.pet.appearance.v1";

export type PetStyle = "orb" | "cell";
export type PetSize = "small" | "medium" | "large";

export interface PetPreferences {
  enabled: boolean;
  name: string;
  style: PetStyle;
  size: PetSize;
}

export interface PetPreferencesContextValue extends PetPreferences {
  setEnabled(enabled: boolean): void;
  setName(name: string): void;
  setStyle(style: PetStyle): void;
  setSize(size: PetSize): void;
}

const DEFAULT_PREFERENCES: PetPreferences = {
  enabled: false,
  name: "Pip",
  style: "orb",
  size: "medium",
};

const PetPreferencesContext = createContext<PetPreferencesContextValue | undefined>(undefined);

function isStyle(value: unknown): value is PetStyle {
  return value === "orb" || value === "cell";
}

function isSize(value: unknown): value is PetSize {
  return value === "small" || value === "medium" || value === "large";
}

function readPreferences(): PetPreferences {
  if (typeof window === "undefined") return DEFAULT_PREFERENCES;

  try {
    const enabledValue = window.localStorage.getItem(PET_ENABLED_STORAGE_KEY);
    const rawAppearance = window.localStorage.getItem(PET_APPEARANCE_STORAGE_KEY);
    const appearance = rawAppearance
      ? JSON.parse(rawAppearance) as { name?: unknown; style?: unknown; size?: unknown } | null
      : null;

    return {
      enabled: enabledValue === "true" ? true : enabledValue === "false" ? false : DEFAULT_PREFERENCES.enabled,
      name: typeof appearance?.name === "string" && appearance.name.trim()
        ? appearance.name.slice(0, 24)
        : DEFAULT_PREFERENCES.name,
      style: isStyle(appearance?.style) ? appearance.style : DEFAULT_PREFERENCES.style,
      size: isSize(appearance?.size) ? appearance.size : DEFAULT_PREFERENCES.size,
    };
  } catch {
    return DEFAULT_PREFERENCES;
  }
}

export function PetPreferencesProvider({ children }: { children: ReactNode }) {
  const [preferences, setPreferences] = useState<PetPreferences>(readPreferences);

  useEffect(() => {
    if (typeof window === "undefined") return;
    try {
      window.localStorage.setItem(PET_ENABLED_STORAGE_KEY, String(preferences.enabled));
      window.localStorage.setItem(PET_APPEARANCE_STORAGE_KEY, JSON.stringify({
        name: preferences.name,
        style: preferences.style,
        size: preferences.size,
      }));
    } catch {
      // The provider remains usable for this session when storage is unavailable.
    }
  }, [preferences]);

  const setEnabled = useCallback((enabled: boolean) => {
    setPreferences((current) => ({ ...current, enabled }));
  }, []);
  const setName = useCallback((name: string) => {
    setPreferences((current) => ({ ...current, name: name.slice(0, 24) }));
  }, []);
  const setStyle = useCallback((style: PetStyle) => {
    setPreferences((current) => ({ ...current, style }));
  }, []);
  const setSize = useCallback((size: PetSize) => {
    setPreferences((current) => ({ ...current, size }));
  }, []);

  const value = useMemo<PetPreferencesContextValue>(() => ({
    ...preferences,
    setEnabled,
    setName,
    setStyle,
    setSize,
  }), [preferences, setEnabled, setName, setSize, setStyle]);

  return createElement(PetPreferencesContext.Provider, { value }, children);
}

export function usePetPreferences(): PetPreferencesContextValue {
  const value = useContext(PetPreferencesContext);
  if (!value) throw new Error("usePetPreferences must be used within PetPreferencesProvider");
  return value;
}
