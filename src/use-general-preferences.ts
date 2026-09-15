import { useSyncExternalStore } from "react";

const SELECTION_ACTIONS_KEY = "omicsops.general.selectionActionsEnabled";
const listeners = new Set<() => void>();
let selectionActionsEnabled = true;
let memoryOnly = false;
let storageListenerInstalled = false;

function readSelectionActionsEnabled(): boolean {
  if (memoryOnly || typeof window === "undefined") return selectionActionsEnabled;
  try {
    const stored = window.localStorage.getItem(SELECTION_ACTIONS_KEY);
    selectionActionsEnabled = stored === null ? true : stored === "true";
  } catch {
    memoryOnly = true;
  }
  return selectionActionsEnabled;
}

function notify(): void {
  listeners.forEach((listener) => listener());
}

function ensureStorageListener(): void {
  if (storageListenerInstalled || typeof window === "undefined") return;
  window.addEventListener("storage", (event) => {
    if (event.key !== SELECTION_ACTIONS_KEY && event.key !== null) return;
    selectionActionsEnabled = event.key === null ? true : event.newValue === "true";
    memoryOnly = false;
    notify();
  });
  storageListenerInstalled = true;
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  ensureStorageListener();
  return () => listeners.delete(listener);
}

export function setSelectionActionsEnabled(enabled: boolean): void {
  selectionActionsEnabled = enabled;
  try {
    window.localStorage.setItem(SELECTION_ACTIONS_KEY, String(enabled));
    memoryOnly = false;
  } catch {
    memoryOnly = true;
  }
  notify();
}

export function useGeneralPreferences() {
  const enabled = useSyncExternalStore(subscribe, readSelectionActionsEnabled, () => true);
  return { selectionActionsEnabled: enabled, setSelectionActionsEnabled };
}
