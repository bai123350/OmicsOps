import { useCallback, useEffect, useRef, useState } from "react";

import {
  DEFAULT_CONVERSATION_AGENT_PREFERENCES,
  getConversationAgentPreferencesV4,
  PreferenceSaveNotDispatchedError,
  reconcileConversationAgentPreferencesV4,
  saveConversationAgentPreferencesV4,
} from "../../conversation-preferences-api";
import type { ConversationAgentPreferencesV4 } from "../../types";

export type ConversationAgentPreferenceKey = Exclude<keyof ConversationAgentPreferencesV4, "fast_mode">;

export interface UseConversationAgentPreferencesResult {
  preferences: ConversationAgentPreferencesV4;
  loading: boolean;
  saving: boolean;
  busy: boolean;
  blocked: boolean;
  unconfirmed: boolean;
  loadError: string;
  saveError: string;
  error: string;
  setPreference: (key: ConversationAgentPreferenceKey, value: boolean) => void;
  toggle: (key: ConversationAgentPreferenceKey) => void;
  setFastMode: (value: boolean | null) => void;
  retry: () => void;
  retrySave: () => void;
  refresh: () => void;
}

const preferenceKeys: ConversationAgentPreferenceKey[] = ["delegation_enabled", "auto_review", "memory_enabled"];
const PREFERENCE_LOAD_TIMEOUT_MS = 20_000;
const PREFERENCE_SAVE_TIMEOUT_MS = 15_000;
const PREFERENCE_SAVE_DEADLINE_MS = 10_000;
const PENDING_SAVE_PREFIX = "omicsops.preferences.pendingSave:";

function pendingSavePrefix(projectId: string, conversationId: string): string {
  return `${PENDING_SAVE_PREFIX}${projectId}:${conversationId}:`;
}

interface PendingSaveMarker { key: string; expiresAtMs: number }

function pendingSaveMarkers(projectId: string, conversationId: string): PendingSaveMarker[] {
  const prefix = pendingSavePrefix(projectId, conversationId);
  return Object.keys(localStorage).filter((key) => key.startsWith(prefix)).map((key) => {
    const expiresAtMs = Number(localStorage.getItem(key));
    if (!Number.isSafeInteger(expiresAtMs) || expiresAtMs <= 0) throw new Error("invalid pending preference save");
    return { key, expiresAtMs };
  });
}

function clearObservedMarkers(projectId: string, conversationId: string, observed: PendingSaveMarker[]): boolean {
  for (const marker of observed) {
    if (localStorage.getItem(marker.key) !== String(marker.expiresAtMs)) return false;
  }
  for (const marker of observed) localStorage.removeItem(marker.key);
  return pendingSaveMarkers(projectId, conversationId).length === 0;
}

function limitRequest<T>(request: Promise<T>, milliseconds: number): Promise<T> {
  let timer: ReturnType<typeof setTimeout>;
  const timeout = new Promise<never>((_, reject) => {
    timer = setTimeout(() => reject(new Error("preference request timed out")), milliseconds);
  });
  return Promise.race([request, timeout]).finally(() => clearTimeout(timer));
}

function isPreferences(value: unknown): value is ConversationAgentPreferencesV4 {
  if (!value || typeof value !== "object") return false;
  const candidate = value as Record<string, unknown>;
  return preferenceKeys.every((key) => typeof candidate[key] === "boolean")
    && (candidate.fast_mode === undefined || candidate.fast_mode === null || typeof candidate.fast_mode === "boolean");
}

function loadErrorMessage(): string {
  return "Could not load conversation preferences. Please retry.";
}

function saveErrorMessage(): string {
  return "Could not confirm the preference save. Reconcile before sending another message.";
}

export function useConversationAgentPreferences(
  projectId: string | null | undefined,
  conversationId: string | null | undefined,
  disabled = false,
): UseConversationAgentPreferencesResult {
  const scope = projectId && conversationId ? `${projectId}\u0000${conversationId}` : null;
  const scopeRef = useRef(scope);
  scopeRef.current = scope;
  const mountedRef = useRef(false);
  const generationRef = useRef(0);
  const loadedScopeRef = useRef<string | null>(null);
  const preferencesRef = useRef<ConversationAgentPreferencesV4>({ ...DEFAULT_CONVERSATION_AGENT_PREFERENCES });
  const loadingRef = useRef(Boolean(scope));
  const savingRef = useRef(false);
  const unconfirmedRef = useRef(false);
  const [preferences, setPreferences] = useState<ConversationAgentPreferencesV4>(() => ({ ...DEFAULT_CONVERSATION_AGENT_PREFERENCES }));
  const [loading, setLoading] = useState(Boolean(scope));
  const [saving, setSaving] = useState(false);
  const [loadError, setLoadError] = useState("");
  const [saveError, setSaveError] = useState("");
  const [unconfirmed, setUnconfirmed] = useState(false);
  const [retryVersion, setRetryVersion] = useState(0);

  const isCurrent = useCallback((activeScope: string, generation: number) => (
    mountedRef.current
      && scopeRef.current === activeScope
      && generationRef.current === generation
  ), []);

  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
      generationRef.current += 1;
    };
  }, []);

  useEffect(() => {
    const activeScope = scope;
    const generation = ++generationRef.current;
    loadedScopeRef.current = null;
    loadingRef.current = Boolean(activeScope);
    savingRef.current = false;
    unconfirmedRef.current = false;
    setPreferences({ ...DEFAULT_CONVERSATION_AGENT_PREFERENCES });
    preferencesRef.current = { ...DEFAULT_CONVERSATION_AGENT_PREFERENCES };
    setLoading(Boolean(activeScope));
    setSaving(false);
    setLoadError("");
    setSaveError("");
    setUnconfirmed(false);

    if (!activeScope || !projectId || !conversationId) {
      loadingRef.current = false;
      return;
    }

    let disposed = false;
    let observedMarkers: PendingSaveMarker[] = [];
    const request = Promise.resolve().then(async () => {
      observedMarkers = pendingSaveMarkers(projectId, conversationId);
      if (!observedMarkers.length) return getConversationAgentPreferencesV4(projectId, conversationId);
      const deadline = Math.max(...observedMarkers.map((marker) => marker.expiresAtMs));
      if (deadline > Date.now() + 30_000) throw new Error("invalid pending preference deadline");
      if (!disposed && isCurrent(activeScope, generation)) {
        unconfirmedRef.current = true;
        setUnconfirmed(true);
      }
      if (Date.now() < deadline) {
        await new Promise<void>((resolve) => setTimeout(resolve, deadline - Date.now()));
      }
      return reconcileConversationAgentPreferencesV4(projectId, conversationId, deadline);
    });
    void limitRequest(request, PREFERENCE_LOAD_TIMEOUT_MS)
      .then((result) => {
        if (disposed || !isCurrent(activeScope, generation)) return;
        if (!isPreferences(result)) throw new Error("invalid conversation preferences");
        if (!clearObservedMarkers(projectId, conversationId, observedMarkers)) {
          unconfirmedRef.current = true;
          setUnconfirmed(true);
          throw new Error("preference marker changed while loading");
        }
        const next = { ...result };
        preferencesRef.current = next;
        setPreferences(next);
        loadedScopeRef.current = activeScope;
        unconfirmedRef.current = false;
        setUnconfirmed(false);
      })
      .catch(() => {
        if (disposed || !isCurrent(activeScope, generation)) return;
        loadedScopeRef.current = null;
        setLoadError(loadErrorMessage());
      })
      .finally(() => {
        if (disposed || !isCurrent(activeScope, generation)) return;
        loadingRef.current = false;
        setLoading(false);
      });
    return () => {
      disposed = true;
    };
  }, [conversationId, isCurrent, projectId, retryVersion, scope]);

  const persist = useCallback((
    activeScope: string,
    generation: number,
    candidate: ConversationAgentPreferencesV4,
    previous: ConversationAgentPreferencesV4,
  ) => {
    if (savingRef.current || unconfirmedRef.current || !projectId || !conversationId) return;
    const expiresAtMs = Date.now() + PREFERENCE_SAVE_DEADLINE_MS;
    const operationId = globalThis.crypto?.randomUUID?.() ?? `${Date.now()}-${Math.random()}`;
    const markerKey = `${pendingSavePrefix(projectId, conversationId)}${operationId}`;
    let existingMarkers: PendingSaveMarker[];
    try {
      existingMarkers = pendingSaveMarkers(projectId, conversationId);
    } catch {
      preferencesRef.current = { ...previous };
      setPreferences({ ...previous });
      unconfirmedRef.current = true;
      setUnconfirmed(true);
      setSaveError(saveErrorMessage());
      return;
    }
    if (existingMarkers.length) {
      preferencesRef.current = { ...previous };
      setPreferences({ ...previous });
      unconfirmedRef.current = true;
      setUnconfirmed(true);
      setSaveError(saveErrorMessage());
      return;
    }
    try {
      localStorage.setItem(markerKey, String(expiresAtMs));
    } catch {
      preferencesRef.current = { ...previous };
      setPreferences({ ...previous });
      setSaveError("Could not safely start the preference save.");
      return;
    }
    savingRef.current = true;
    setSaving(true);
    setSaveError("");
    void limitRequest(
      saveConversationAgentPreferencesV4(projectId, conversationId, candidate, expiresAtMs),
      PREFERENCE_SAVE_TIMEOUT_MS,
    )
      .then((result) => {
        if (!isCurrent(activeScope, generation)) return;
        if (!isPreferences(result)) throw new Error("invalid conversation preferences");
        localStorage.removeItem(markerKey);
        if (pendingSaveMarkers(projectId, conversationId).length) {
          unconfirmedRef.current = true;
          setUnconfirmed(true);
          setSaveError(saveErrorMessage());
          return;
        }
        const next = { ...result };
        preferencesRef.current = next;
        setPreferences(next);
        setSaveError("");
      })
      .catch((error) => {
        if (!isCurrent(activeScope, generation)) return;
        if (error instanceof PreferenceSaveNotDispatchedError) {
          preferencesRef.current = { ...previous };
          setPreferences({ ...previous });
          try {
            localStorage.removeItem(markerKey);
            if (pendingSaveMarkers(projectId, conversationId).length) {
              unconfirmedRef.current = true;
              setUnconfirmed(true);
              setSaveError(saveErrorMessage());
            } else {
              setSaveError("Conversation preferences require the desktop app.");
            }
          } catch {
            unconfirmedRef.current = true;
            setUnconfirmed(true);
            setSaveError(saveErrorMessage());
          }
          return;
        }
        unconfirmedRef.current = true;
        setUnconfirmed(true);
        setSaveError(saveErrorMessage());
      })
      .finally(() => {
        if (!isCurrent(activeScope, generation)) return;
        savingRef.current = false;
        setSaving(false);
      });
  }, [conversationId, isCurrent, projectId]);

  const setPreference = useCallback((key: ConversationAgentPreferenceKey, value: boolean) => {
    if (!preferenceKeys.includes(key) || disabled || !scope || loadingRef.current || savingRef.current || unconfirmedRef.current || loadedScopeRef.current !== scope) return;
    const previous = preferencesRef.current;
    if (previous[key] === value) return;
    const candidate = { ...previous, [key]: value };
    preferencesRef.current = candidate;
    setPreferences(candidate);
    persist(scope, generationRef.current, candidate, previous);
  }, [disabled, persist, scope]);

  const toggle = useCallback((key: ConversationAgentPreferenceKey) => {
    if (!preferenceKeys.includes(key)) return;
    setPreference(key, !preferencesRef.current[key]);
  }, [setPreference]);

  const setFastMode = useCallback((value: boolean | null) => {
    if ((value !== null && typeof value !== "boolean") || disabled || !scope || loadingRef.current || savingRef.current || unconfirmedRef.current || loadedScopeRef.current !== scope) return;
    const previous = preferencesRef.current;
    if ((previous.fast_mode ?? null) === value) return;
    const candidate = { ...previous, fast_mode: value };
    preferencesRef.current = candidate;
    setPreferences(candidate);
    persist(scope, generationRef.current, candidate, previous);
  }, [disabled, persist, scope]);

  const refresh = useCallback(() => {
    if (!scope || loadingRef.current || savingRef.current) return;
    // Mark the retry as loading synchronously so repeated clicks cannot queue
    // multiple loads before React runs the effect below.
    loadingRef.current = true;
    setLoading(true);
    setLoadError("");
    setRetryVersion((version) => version + 1);
  }, [scope]);
  const retrySave = refresh;

  // During a scope transition the effect that starts the new load runs after
  // render. Treat the old scope as busy in that interim render so a send or
  // click cannot race the new conversation's preference snapshot.
  const scopeBusy = scope !== null && loadedScopeRef.current !== scope && !loadError;
  const error = saveError || loadError;
  return {
    preferences,
    loading,
    saving,
    busy: loading || saving || scopeBusy,
    blocked: loading || saving || scopeBusy || Boolean(loadError) || unconfirmed,
    unconfirmed,
    loadError,
    saveError,
    error,
    setPreference,
    toggle,
    setFastMode,
    retry: refresh,
    retrySave,
    refresh,
  };
}
