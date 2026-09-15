import { useSyncExternalStore } from "react";

const LEGACY_STORAGE_KEY = "omicsops.composer.modifierSend";
const listeners = new Set<() => void>();
let sessionFallback = false;
let initialized = false;
let storageListenerInstalled = false;

function readPreference(): boolean {
  if (initialized || typeof window === "undefined") return sessionFallback;
  try {
    const stored = window.localStorage.getItem(LEGACY_STORAGE_KEY);
    sessionFallback = stored === "true";
  } catch {
    // Browser storage can be unavailable in hardened or embedded contexts.
  }
  initialized = true;
  return sessionFallback;
}

function notify(): void {
  listeners.forEach((listener) => listener());
}

function ensureStorageListener(): void {
  if (storageListenerInstalled || typeof window === "undefined") return;
  window.addEventListener("storage", (event) => {
    if (event.key !== LEGACY_STORAGE_KEY && event.key !== null) return;
    sessionFallback = event.key === null ? false : event.newValue === "true";
    initialized = true;
    notify();
  });
  storageListenerInstalled = true;
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  ensureStorageListener();
  return () => {
    listeners.delete(listener);
  };
}

export function setComposerSendPreference(modifierSend: boolean): void {
  sessionFallback = modifierSend;
  initialized = true;
  try {
    window.localStorage.setItem(LEGACY_STORAGE_KEY, String(modifierSend));
  } catch {
    // Retain the preference in memory for the current app session.
  }
  notify();
}

export function useComposerSendPreference(): [boolean, (modifierSend: boolean) => void] {
  const modifierSend = useSyncExternalStore(subscribe, readPreference, () => false);
  return [modifierSend, setComposerSendPreference];
}
