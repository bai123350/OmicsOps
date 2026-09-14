import { useCallback, useEffect, useRef, useState } from "react";

import {
  DEFAULT_REVIEWER_SETTINGS,
  getReviewerSettingsV4,
  saveReviewerSettingsV4,
} from "../../session-review-api";
import type { ReviewerBackendChoiceV4, ReviewerSettingsV4 } from "../../types";

const LOAD_ERROR = "Could not load reviewer settings. Please retry.";
const SAVE_ERROR = "Could not save reviewer settings. Please retry.";

export interface UseReviewerSettingsResult {
  settings: ReviewerSettingsV4;
  loading: boolean;
  saving: boolean;
  busy: boolean;
  loadError: string;
  saveError: string;
  error: string;
  setSettings: (settings: ReviewerSettingsV4) => void;
  setBackend: (backend: ReviewerBackendChoiceV4) => void;
  setDefaultHttpProfileId: (profileId: string | null) => void;
  save: (settings?: ReviewerSettingsV4) => Promise<boolean>;
  retry: () => void;
  retrySave: () => void;
  refresh: () => void;
}

interface FailedSave {
  candidate: ReviewerSettingsV4;
}

function cloneSettings(settings: ReviewerSettingsV4): ReviewerSettingsV4 {
  return {
    backend: settings.backend.kind === "http_profile"
      ? { kind: "http_profile", profile_id: settings.backend.profile_id }
      : { kind: settings.backend.kind },
    default_http_profile_id: settings.default_http_profile_id ?? null,
  };
}

export function isReviewerSettings(value: unknown): value is ReviewerSettingsV4 {
  if (!value || typeof value !== "object" || Array.isArray(value)) return false;
  const candidate = value as Record<string, unknown>;
  if (candidate.default_http_profile_id !== undefined
    && candidate.default_http_profile_id !== null
    && typeof candidate.default_http_profile_id !== "string") return false;
  if (!candidate.backend || typeof candidate.backend !== "object" || Array.isArray(candidate.backend)) return false;
  const backend = candidate.backend as Record<string, unknown>;
  if (backend.kind === "follow_session" || backend.kind === "default_http") return Object.keys(backend).length === 1;
  return backend.kind === "http_profile"
    && typeof backend.profile_id === "string"
    && backend.profile_id.trim().length > 0
    && Object.keys(backend).length === 2;
}

export function useReviewerSettings(): UseReviewerSettingsResult {
  const mountedRef = useRef(false);
  const generationRef = useRef(0);
  const savingRef = useRef(false);
  const failedSaveRef = useRef<FailedSave | null>(null);
  const [settings, setSettingsState] = useState<ReviewerSettingsV4>(() => cloneSettings(DEFAULT_REVIEWER_SETTINGS));
  const settingsRef = useRef(settings);
  const persistedSettingsRef = useRef<ReviewerSettingsV4>(cloneSettings(DEFAULT_REVIEWER_SETTINGS));
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [loadError, setLoadError] = useState("");
  const [saveError, setSaveError] = useState("");
  const [reloadVersion, setReloadVersion] = useState(0);

  const isCurrent = useCallback((generation: number) => mountedRef.current && generationRef.current === generation, []);
  const setSettings = useCallback((next: ReviewerSettingsV4) => {
    if (!isReviewerSettings(next) || savingRef.current) return;
    const cloned = cloneSettings(next);
    settingsRef.current = cloned;
    setSettingsState(cloned);
    setSaveError("");
    failedSaveRef.current = null;
  }, []);

  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
      generationRef.current += 1;
    };
  }, []);

  useEffect(() => {
    const generation = ++generationRef.current;
    let disposed = false;
    setLoading(true);
    setLoadError("");
    setSaveError("");
    failedSaveRef.current = null;
    void getReviewerSettingsV4()
      .then((result) => {
        if (disposed || !isCurrent(generation)) return;
        if (!isReviewerSettings(result)) throw new Error("invalid reviewer settings");
        const next = cloneSettings(result);
        settingsRef.current = next;
        persistedSettingsRef.current = cloneSettings(next);
        setSettingsState(next);
      })
      .catch(() => {
        if (disposed || !isCurrent(generation)) return;
        setLoadError(LOAD_ERROR);
      })
      .finally(() => {
        if (disposed || !isCurrent(generation)) return;
        setLoading(false);
      });
    return () => {
      disposed = true;
    };
  }, [isCurrent, reloadVersion]);

  const save = useCallback(async (requested?: ReviewerSettingsV4): Promise<boolean> => {
    // A failed load leaves the in-memory defaults untrusted. Do not let a
    // draft overwrite the unknown persisted settings until the user retries
    // and we have a successful baseline.
    if (loading || loadError || savingRef.current) return false;
    const candidate = requested ?? settingsRef.current;
    if (!isReviewerSettings(candidate)) return false;
    const previous = persistedSettingsRef.current;
    const generation = generationRef.current;
    const next = cloneSettings(candidate);
    settingsRef.current = next;
    setSettingsState(next);
    savingRef.current = true;
    failedSaveRef.current = null;
    setSaving(true);
    setSaveError("");
    try {
      const result = await saveReviewerSettingsV4(next);
      if (!isCurrent(generation)) return false;
      if (!isReviewerSettings(result)) throw new Error("invalid reviewer settings");
      const accepted = cloneSettings(result);
      settingsRef.current = accepted;
      persistedSettingsRef.current = cloneSettings(accepted);
      setSettingsState(accepted);
      return true;
    } catch {
      if (!isCurrent(generation)) return false;
      settingsRef.current = cloneSettings(previous);
      setSettingsState(cloneSettings(previous));
      failedSaveRef.current = { candidate: next };
      setSaveError(SAVE_ERROR);
      return false;
    } finally {
      if (isCurrent(generation)) {
        savingRef.current = false;
        setSaving(false);
      }
    }
  }, [isCurrent, loadError, loading]);

  const setBackend = useCallback((backend: ReviewerBackendChoiceV4) => {
    if (!isReviewerSettings({ backend, default_http_profile_id: settingsRef.current.default_http_profile_id })) return;
    setSettings({ ...settingsRef.current, backend });
  }, [setSettings]);

  const setDefaultHttpProfileId = useCallback((profileId: string | null) => {
    setSettings({ ...settingsRef.current, default_http_profile_id: profileId || null });
  }, [setSettings]);

  const retrySave = useCallback(() => {
    const failed = failedSaveRef.current;
    if (!failed) return;
    void save(failed.candidate);
  }, [save]);

  const refresh = useCallback(() => {
    if (loading || savingRef.current) return;
    setReloadVersion((version) => version + 1);
  }, [loading]);

  const retry = useCallback(() => {
    if (saveError) retrySave();
    else refresh();
  }, [refresh, retrySave, saveError]);

  return {
    settings,
    loading,
    saving,
    busy: loading || saving,
    loadError,
    saveError,
    error: saveError || loadError,
    setSettings,
    setBackend,
    setDefaultHttpProfileId,
    save,
    retry,
    retrySave,
    refresh,
  };
}
