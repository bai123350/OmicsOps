import { useSyncExternalStore } from "react";

const STORAGE_KEY = "omicsops.sessions.resumeLast";
const listeners = new Set<() => void>();
let snapshot = true;
let memoryOnly = false;
let storageListenerInstalled = false;

function readPreference(): boolean {
  if (memoryOnly || typeof window === "undefined") return snapshot;
  try {
    const stored = window.localStorage.getItem(STORAGE_KEY);
    snapshot = stored === null ? true : stored === "true";
  } catch {
    memoryOnly = true;
  }
  return snapshot;
}

function notify(): void {
  listeners.forEach((listener) => listener());
}

function ensureStorageListener(): void {
  if (storageListenerInstalled || typeof window === "undefined") return;
  window.addEventListener("storage", (event) => {
    if (event.key !== STORAGE_KEY && event.key !== null) return;
    snapshot = event.key === null ? true : event.newValue === "true";
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

export function setResumeLastSessionPreference(enabled: boolean): void {
  snapshot = enabled;
  try {
    window.localStorage.setItem(STORAGE_KEY, String(enabled));
    memoryOnly = false;
  } catch {
    memoryOnly = true;
  }
  notify();
}

export function useResumeLastSessionPreference(): [boolean, (enabled: boolean) => void] {
  const enabled = useSyncExternalStore(subscribe, readPreference, () => true);
  return [enabled, setResumeLastSessionPreference];
}
