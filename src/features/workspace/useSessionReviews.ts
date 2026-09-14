import { useCallback, useEffect, useRef, useState } from "react";

import {
  getSessionReviewV4,
  isRejectedReviewStart,
  listSessionReviewsV4,
  startSessionReviewV4,
} from "../../session-review-api";
import type {
  ModelProfile,
  SessionReviewRecordV4,
  SessionReviewRequestV4,
} from "../../types";

const REVIEW_POLL_MS = 1_500;

const LOAD_ERROR = "Could not load review history. Please retry.";
const START_ERROR = "Could not start the review. Please retry.";
const POLL_ERROR = "Could not refresh the review status. Please retry.";
const MODEL_ERROR = "Choose a model before requesting a review.";

export interface UseSessionReviewsResult {
  records: SessionReviewRecordV4[];
  latestReview: SessionReviewRecordV4 | null;
  loading: boolean;
  starting: boolean;
  busy: boolean;
  error: string;
  refresh: () => void;
  retry: () => void;
  startReview: () => Promise<void>;
}

function scopeKey(projectId: string, conversationId: string): string {
  return `${projectId}\u0000${conversationId}`;
}

function newRequestId(): string {
  if (typeof crypto !== "undefined" && typeof crypto.randomUUID === "function") return crypto.randomUUID();
  // The native request contract still receives a UUID-shaped value in older
  // browsers that do not expose crypto.randomUUID.
  const bytes = new Uint8Array(16);
  if (typeof crypto !== "undefined" && typeof crypto.getRandomValues === "function") crypto.getRandomValues(bytes);
  else for (let index = 0; index < bytes.length; index += 1) bytes[index] = Math.floor(Math.random() * 256);
  bytes[6] = (bytes[6] & 0x0f) | 0x40;
  bytes[8] = (bytes[8] & 0x3f) | 0x80;
  const hex = Array.from(bytes, (value) => value.toString(16).padStart(2, "0")).join("");
  return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`;
}

function reviewBelongsToScope(record: SessionReviewRecordV4, projectId: string, conversationId: string): boolean {
  return record.project_id === projectId && record.conversation_id === conversationId;
}

function latestRecord(records: SessionReviewRecordV4[]): SessionReviewRecordV4 | null {
  return records.reduce<SessionReviewRecordV4 | null>((latest, record) => {
    if (!latest) return record;
    const latestTime = Date.parse(latest.updated_at || latest.created_at);
    const recordTime = Date.parse(record.updated_at || record.created_at);
    if (Number.isFinite(recordTime) && (!Number.isFinite(latestTime) || recordTime > latestTime)) return record;
    return latest;
  }, null);
}

export function useSessionReviews(
  projectId: string | null | undefined,
  conversationId: string | null | undefined,
  activeModelProfile?: Pick<ModelProfile, "id"> | null,
): UseSessionReviewsResult {
  const scope = projectId && conversationId ? scopeKey(projectId, conversationId) : null;
  const scopeRef = useRef(scope);
  scopeRef.current = scope;
  const mountedRef = useRef(false);
  const generationRef = useRef(0);
  const recordsRef = useRef<SessionReviewRecordV4[]>([]);
  const startInFlightRef = useRef(false);
  const pollTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const failureKindRef = useRef<"load" | "start" | null>(null);
  const pendingRequestRef = useRef<SessionReviewRequestV4 | null>(null);
  const ambiguousRequestRef = useRef(false);
  const [records, setRecords] = useState<SessionReviewRecordV4[]>([]);
  const [loading, setLoading] = useState(Boolean(scope));
  const [starting, setStarting] = useState(false);
  const [error, setError] = useState("");
  const [reloadVersion, setReloadVersion] = useState(0);

  const isCurrent = useCallback((activeScope: string, generation: number) => (
    mountedRef.current
      && scopeRef.current === activeScope
      && generationRef.current === generation
  ), []);

  const replaceRecords = useCallback((next: SessionReviewRecordV4[]) => {
    recordsRef.current = next;
    setRecords(next);
  }, []);

  const mergeRecord = useCallback((record: SessionReviewRecordV4, activeScope: string) => {
    if (!projectId || !conversationId || !reviewBelongsToScope(record, projectId, conversationId)) return;
    const shown = record;
    replaceRecords([
      ...recordsRef.current.filter((item) => item.id !== shown.id),
      shown,
    ]);
  }, [conversationId, projectId, replaceRecords]);

  const pollOwnedRef = useRef<(activeScope: string, generation: number) => Promise<void>>(async () => undefined);
  const schedulePoll = useCallback((activeScope: string, generation: number) => {
    if (pollTimerRef.current !== null) return;
    if (!recordsRef.current.some((record) => record.status === "running")) return;
    pollTimerRef.current = setTimeout(() => {
      pollTimerRef.current = null;
      void pollOwnedRef.current(activeScope, generation);
    }, REVIEW_POLL_MS);
  }, []);

  // Assigned after declaration so schedulePoll can keep a stable callback and
  // the timer always calls the current polling implementation.
  const pollOwned = useCallback(async (activeScope: string, generation: number) => {
    if (!isCurrent(activeScope, generation) || !projectId || !conversationId) return;
    const runningRecords = recordsRef.current.filter((record) => record.status === "running");
    if (!runningRecords.length) return;

    let failed = false;
    for (const record of runningRecords) {
      if (!isCurrent(activeScope, generation)) return;
      try {
        const refreshed = await getSessionReviewV4(projectId, conversationId, record.id);
        if (!isCurrent(activeScope, generation)) return;
        if (refreshed && reviewBelongsToScope(refreshed, projectId, conversationId)) {
          mergeRecord(refreshed, activeScope);
        }
      } catch {
        failed = true;
      }
    }
    if (!isCurrent(activeScope, generation)) return;
    if (failed) {
      failureKindRef.current = null;
      setError(POLL_ERROR);
    }
    schedulePoll(activeScope, generation);
  }, [conversationId, isCurrent, mergeRecord, projectId, schedulePoll]);
  pollOwnedRef.current = pollOwned;

  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
      generationRef.current += 1;
      if (pollTimerRef.current !== null) {
        clearTimeout(pollTimerRef.current);
        pollTimerRef.current = null;
      }
    };
  }, []);

  useEffect(() => {
    const activeScope = scope;
    const generation = ++generationRef.current;
    if (pollTimerRef.current !== null) {
      clearTimeout(pollTimerRef.current);
      pollTimerRef.current = null;
    }
    startInFlightRef.current = false;
    pendingRequestRef.current = null;
    ambiguousRequestRef.current = false;
    setStarting(false);
    failureKindRef.current = null;
    replaceRecords([]);
    setError("");
    setLoading(Boolean(activeScope));

    if (!activeScope || !projectId || !conversationId) {
      setLoading(false);
      return () => undefined;
    }

    let disposed = false;
    void listSessionReviewsV4(projectId, conversationId)
      .then((result) => {
        if (disposed || !isCurrent(activeScope, generation)) return;
        const scoped = result.filter((record) => reviewBelongsToScope(record, projectId, conversationId));
        replaceRecords(scoped);
        schedulePoll(activeScope, generation);
      })
      .catch(() => {
        if (disposed || !isCurrent(activeScope, generation)) return;
        failureKindRef.current = "load";
        setError(LOAD_ERROR);
      })
      .finally(() => {
        if (disposed || !isCurrent(activeScope, generation)) return;
        setLoading(false);
      });
    return () => {
      disposed = true;
    };
  }, [conversationId, isCurrent, projectId, reloadVersion, replaceRecords, schedulePoll, scope]);

  const refresh = useCallback(() => {
    if (!scope || loading || startInFlightRef.current) return;
    setLoading(true);
    setError("");
    failureKindRef.current = null;
    setReloadVersion((version) => version + 1);
  }, [loading, scope]);

  const startReview = useCallback(async () => {
    const activeScope = scope;
    if (!activeScope || !projectId || !conversationId || !activeModelProfile?.id || startInFlightRef.current || loading) {
      if (activeScope && !activeModelProfile?.id && !startInFlightRef.current) {
        failureKindRef.current = "start";
        setError(MODEL_ERROR);
      }
      return;
    }
    const generation = generationRef.current;
    // When a start and its same-id lookup both failed, keep the request ID
    // locked until a later lookup resolves it. A user retry probes that ID
    // and reuses it for native start even if the lookup finds no record yet.
    if (ambiguousRequestRef.current && pendingRequestRef.current) {
      startInFlightRef.current = true;
      setStarting(true);
      setError("");
      try {
        const existing = await getSessionReviewV4(projectId, conversationId, pendingRequestRef.current.request_id);
        if (!isCurrent(activeScope, generation)) return;
        if (existing) {
          ambiguousRequestRef.current = false;
          pendingRequestRef.current = null;
          mergeRecord(existing, activeScope);
          schedulePoll(activeScope, generation);
          return;
        }
        // A null lookup does not prove that the original host request never
        // crossed its persistence boundary. Keep the same request ID and let
        // the idempotent native start resolve it on the next attempt.
        ambiguousRequestRef.current = false;
      } catch {
        if (isCurrent(activeScope, generation)) {
          failureKindRef.current = "start";
          setError(START_ERROR);
        }
        return;
      } finally {
        if (isCurrent(activeScope, generation)) {
          startInFlightRef.current = false;
          setStarting(false);
        }
      }
      if (!isCurrent(activeScope, generation)) return;
    }
    const request: SessionReviewRequestV4 = pendingRequestRef.current ?? {
      request_id: newRequestId(),
      project_id: projectId,
      conversation_id: conversationId,
      model_profile_id: activeModelProfile.id,
    };
    pendingRequestRef.current = request;
    startInFlightRef.current = true;
    setStarting(true);
    setError("");
    failureKindRef.current = null;

    let result: SessionReviewRecordV4 | null = null;
    try {
      try {
        result = await startSessionReviewV4(request);
      } catch (failure) {
        if (isRejectedReviewStart(failure)) {
          if (isCurrent(activeScope, generation)) {
            pendingRequestRef.current = null;
            ambiguousRequestRef.current = false;
            failureKindRef.current = "start";
            setError("Review was not started. Check the conversation and reviewer model settings, then retry.");
          }
          return;
        }
        // A transport error is ambiguous: the host may have persisted and
        // begun the request before the webview observed the failure. Resolve
        // the same request id before allowing a fresh dispatch.
        try {
          result = await getSessionReviewV4(projectId, conversationId, request.request_id);
        } catch {
          ambiguousRequestRef.current = true;
          result = null;
        }
        if (!result) {
          if (isCurrent(activeScope, generation)) {
            failureKindRef.current = "start";
            pendingRequestRef.current = request;
            ambiguousRequestRef.current = true;
            setError(START_ERROR);
          }
          return;
        }
      }
      if (!result || !isCurrent(activeScope, generation)) return;
      if (!reviewBelongsToScope(result, projectId, conversationId)) {
        failureKindRef.current = "start";
        pendingRequestRef.current = null;
        ambiguousRequestRef.current = false;
        setError(START_ERROR);
        return;
      }
      pendingRequestRef.current = null;
      ambiguousRequestRef.current = false;
      mergeRecord(result, activeScope);
      schedulePoll(activeScope, generation);
    } finally {
      if (isCurrent(activeScope, generation)) {
        startInFlightRef.current = false;
        setStarting(false);
      }
    }
  }, [activeModelProfile?.id, conversationId, isCurrent, loading, mergeRecord, projectId, schedulePoll, scope]);

  const retry = useCallback(() => {
    if (failureKindRef.current === "start") {
      void startReview();
      return;
    }
    refresh();
  }, [refresh, startReview]);

  const running = scope ? records.some((record) => record.status === "running") : false;
  const latestReview = latestRecord(records);

  return {
    records,
    latestReview,
    loading,
    starting,
    busy: starting || running || ambiguousRequestRef.current,
    error,
    refresh,
    retry,
    startReview,
  };
}
