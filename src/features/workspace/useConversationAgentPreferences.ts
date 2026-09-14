import { useCallback, useEffect, useRef, useState } from "react";

import {
  DEFAULT_CONVERSATION_AGENT_PREFERENCES,
  getConversationAgentPreferencesV4,
  saveConversationAgentPreferencesV4,
} from "../../conversation-preferences-api";
import type { ConversationAgentPreferencesV4 } from "../../types";

export type ConversationAgentPreferenceKey = Exclude<keyof ConversationAgentPreferencesV4, "fast_mode">;

export interface UseConversationAgentPreferencesResult {
  preferences: ConversationAgentPreferencesV4;
  loading: boolean;
  saving: boolean;
  busy: boolean;
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

interface FailedSave {
  scope: string;
  candidate: ConversationAgentPreferencesV4;
}

const preferenceKeys: ConversationAgentPreferenceKey[] = ["delegation_enabled", "auto_review", "memory_enabled"];

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
  return "Could not save conversation preferences. Please retry.";
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
  const failedSaveRef = useRef<FailedSave | null>(null);
  const [preferences, setPreferences] = useState<ConversationAgentPreferencesV4>(() => ({ ...DEFAULT_CONVERSATION_AGENT_PREFERENCES }));
  const [loading, setLoading] = useState(Boolean(scope));
  const [saving, setSaving] = useState(false);
  const [loadError, setLoadError] = useState("");
  const [saveError, setSaveError] = useState("");
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
    failedSaveRef.current = null;
    setPreferences({ ...DEFAULT_CONVERSATION_AGENT_PREFERENCES });
    preferencesRef.current = { ...DEFAULT_CONVERSATION_AGENT_PREFERENCES };
    setLoading(Boolean(activeScope));
    setSaving(false);
    setLoadError("");
    setSaveError("");

    if (!activeScope || !projectId || !conversationId) {
      loadingRef.current = false;
      return;
    }

    let disposed = false;
    void getConversationAgentPreferencesV4(projectId, conversationId)
      .then((result) => {
        if (disposed || !isCurrent(activeScope, generation)) return;
        if (!isPreferences(result)) throw new Error("invalid conversation preferences");
        const next = { ...result };
        preferencesRef.current = next;
        setPreferences(next);
        loadedScopeRef.current = activeScope;
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
    if (savingRef.current || !projectId || !conversationId) return;
    const operation: FailedSave = { scope: activeScope, candidate: { ...candidate } };
    savingRef.current = true;
    setSaving(true);
    setSaveError("");
    failedSaveRef.current = null;
    void saveConversationAgentPreferencesV4(projectId, conversationId, candidate)
      .then((result) => {
        if (!isCurrent(activeScope, generation)) return;
        if (!isPreferences(result)) throw new Error("invalid conversation preferences");
        const next = { ...result };
        preferencesRef.current = next;
        setPreferences(next);
        setSaveError("");
      })
      .catch(() => {
        if (!isCurrent(activeScope, generation)) return;
        preferencesRef.current = { ...previous };
        setPreferences({ ...previous });
        failedSaveRef.current = operation;
        setSaveError(saveErrorMessage());
      })
      .finally(() => {
        if (!isCurrent(activeScope, generation)) return;
        savingRef.current = false;
        setSaving(false);
      });
  }, [conversationId, isCurrent, projectId]);

  const setPreference = useCallback((key: ConversationAgentPreferenceKey, value: boolean) => {
    if (!preferenceKeys.includes(key) || disabled || !scope || loadingRef.current || savingRef.current || loadedScopeRef.current !== scope) return;
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
    if ((value !== null && typeof value !== "boolean") || disabled || !scope || loadingRef.current || savingRef.current || loadedScopeRef.current !== scope) return;
    const previous = preferencesRef.current;
    if ((previous.fast_mode ?? null) === value) return;
    const candidate = { ...previous, fast_mode: value };
    preferencesRef.current = candidate;
    setPreferences(candidate);
    persist(scope, generationRef.current, candidate, previous);
  }, [disabled, persist, scope]);

  const retrySave = useCallback(() => {
    const failed = failedSaveRef.current;
    if (!failed || failed.scope !== scope || disabled || loadingRef.current || savingRef.current || loadedScopeRef.current !== scope) return;
    const previous = preferencesRef.current;
    preferencesRef.current = { ...failed.candidate };
    setPreferences({ ...failed.candidate });
    persist(scope, generationRef.current, failed.candidate, previous);
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

  // During a scope transition the effect that starts the new load runs after
  // render. Treat the old scope as busy in that interim render so a send or
  // click cannot race the new conversation's preference snapshot.
  const scopeBusy = scope !== null && loadedScopeRef.current !== scope;
  const error = saveError || loadError;
  return {
    preferences,
    loading,
    saving,
    busy: loading || saving || scopeBusy,
    loadError,
    saveError,
    error,
    setPreference,
    toggle,
    setFastMode,
    retry: saveError ? retrySave : refresh,
    retrySave,
    refresh,
  };
}
