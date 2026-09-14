import { useCallback, useEffect, useRef, useState } from "react";

import * as api from "../../tauri-api";
import type { AgentStopScope } from "../../agent-stop-api";
import type { StopRunReceiptV4, StopRunRequestV4 } from "../../types";

export type { AgentStopScope } from "../../agent-stop-api";

const STOP_ERROR = "Could not update the run stop state. Please retry.";

export interface UseAgentStopResult {
  receipt: StopRunReceiptV4 | null;
  loading: boolean;
  requesting: boolean;
  stopping: boolean;
  error: string;
  requestStop: (scope?: AgentStopScope | null) => Promise<StopRunReceiptV4 | null>;
  refresh: () => void;
  markTerminal: (runId?: string) => void;
  clear: () => void;
}

function stopScopeKey(scope: AgentStopScope | null): string | null {
  return scope ? `${scope.projectId}\u0000${scope.conversationId}\u0000${scope.runId}` : null;
}

export function agentStopScopeKey(scope: AgentStopScope | null): string | null {
  return stopScopeKey(scope);
}

function newRequestId(): string {
  if (typeof crypto !== "undefined" && typeof crypto.randomUUID === "function") return crypto.randomUUID();
  const bytes = new Uint8Array(16);
  if (typeof crypto !== "undefined" && typeof crypto.getRandomValues === "function") crypto.getRandomValues(bytes);
  else for (let index = 0; index < bytes.length; index += 1) bytes[index] = Math.floor(Math.random() * 256);
  bytes[6] = (bytes[6] & 0x0f) | 0x40;
  bytes[8] = (bytes[8] & 0x3f) | 0x80;
  const hex = Array.from(bytes, (value) => value.toString(16).padStart(2, "0")).join("");
  return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`;
}

function errorMessage(error: unknown): string {
  return error instanceof Error && error.message ? error.message : STOP_ERROR;
}

function isReceipt(value: unknown): value is StopRunReceiptV4 {
  if (!value || typeof value !== "object") return false;
  const candidate = value as Record<string, unknown>;
  return typeof candidate.request_id === "string"
    && typeof candidate.project_id === "string"
    && typeof candidate.conversation_id === "string"
    && typeof candidate.run_id === "string"
    && (candidate.status === "requested" || candidate.status === "observed")
    && typeof candidate.created_at === "string"
    && typeof candidate.updated_at === "string";
}

function receiptMatchesScope(receipt: StopRunReceiptV4, scope: AgentStopScope): boolean {
  return receipt.project_id === scope.projectId
    && receipt.conversation_id === scope.conversationId
    && receipt.run_id === scope.runId;
}

function normalizeScope(
  projectOrScope: AgentStopScope | string | null | undefined,
  conversationId?: string | null,
  runId?: string | null,
): AgentStopScope | null {
  if (projectOrScope && typeof projectOrScope === "object") {
    if (!projectOrScope.projectId || !projectOrScope.conversationId || !projectOrScope.runId) return null;
    return projectOrScope;
  }
  if (typeof projectOrScope !== "string" || !conversationId || !runId) return null;
  return { projectId: projectOrScope, conversationId, runId };
}

export function useAgentStop(scope: AgentStopScope | null | undefined): UseAgentStopResult;
export function useAgentStop(
  projectId: string | null | undefined,
  conversationId: string | null | undefined,
  runId: string | null | undefined,
): UseAgentStopResult;
export function useAgentStop(
  projectOrScope: AgentStopScope | string | null | undefined,
  conversationId?: string | null,
  runId?: string | null,
): UseAgentStopResult {
  const scope = normalizeScope(projectOrScope, conversationId, runId);
  const scopeKey = stopScopeKey(scope);
  const scopeRef = useRef<AgentStopScope | null>(scope);
  scopeRef.current = scope;
  const scopeKeyRef = useRef<string | null>(scopeKey);
  scopeKeyRef.current = scopeKey;
  const mountedRef = useRef(false);
  const generationRef = useRef(0);
  const mutationVersionRef = useRef(0);
  const requestIdsRef = useRef(new Map<string, string>());
  const receiptRef = useRef<StopRunReceiptV4 | null>(null);
  const terminalScopeRef = useRef<string | null>(null);
  const inFlightRef = useRef<{ key: string; promise: Promise<StopRunReceiptV4> } | null>(null);
  const [receipt, setReceipt] = useState<StopRunReceiptV4 | null>(null);
  const [loading, setLoading] = useState(Boolean(scope));
  const [requesting, setRequesting] = useState(false);
  const [stopping, setStopping] = useState(false);
  const [error, setError] = useState("");
  const [reloadVersion, setReloadVersion] = useState(0);

  const isCurrent = useCallback((activeKey: string | null, generation: number) => (
    mountedRef.current
      && scopeKeyRef.current === activeKey
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
    const activeKey = scopeKey;
    const generation = ++generationRef.current;
    const lookupMutationVersion = mutationVersionRef.current;
    terminalScopeRef.current = null;
    receiptRef.current = null;
    setReceipt(null);
    setError("");
    setRequesting(false);
    setStopping(false);
    setLoading(Boolean(activeScope));

    if (!activeScope || !activeKey) {
      setLoading(false);
      return () => undefined;
    }

    let disposed = false;
    void api.agentV4GetStop(activeScope.projectId, activeScope.conversationId, activeScope.runId)
      .then((result) => {
        if (disposed || !isCurrent(activeKey, generation) || lookupMutationVersion !== mutationVersionRef.current) return;
        if (result === null) return;
        if (!isReceipt(result) || !receiptMatchesScope(result, activeScope)) throw new Error(STOP_ERROR);
        requestIdsRef.current.set(activeKey, result.request_id);
        receiptRef.current = result;
        setReceipt(result);
        if (result.status === "observed") terminalScopeRef.current = activeKey;
        setStopping(result.status === "requested" && terminalScopeRef.current !== activeKey);
      })
      .catch((failure) => {
        if (disposed || !isCurrent(activeKey, generation) || lookupMutationVersion !== mutationVersionRef.current) return;
        setError(errorMessage(failure));
      })
      .finally(() => {
        if (disposed || !isCurrent(activeKey, generation) || lookupMutationVersion !== mutationVersionRef.current) return;
        setLoading(false);
      });
    return () => {
      disposed = true;
    };
  }, [isCurrent, reloadVersion, scopeKey]);

  const requestStop = useCallback(async (requestedScope?: AgentStopScope | null): Promise<StopRunReceiptV4 | null> => {
    const activeScope = requestedScope ?? scopeRef.current;
    const activeKey = stopScopeKey(activeScope);
    if (!activeScope || !activeKey) return null;
    const visibleAtStart = mountedRef.current && scopeKeyRef.current === activeKey;
    const generation = generationRef.current;
    if (visibleAtStart && terminalScopeRef.current === activeKey && receiptRef.current?.status === "observed") return receiptRef.current;
    const inFlight = inFlightRef.current;
    if (inFlight?.key === activeKey) return inFlight.promise;

    const requestId = requestIdsRef.current.get(activeKey) ?? newRequestId();
    requestIdsRef.current.set(activeKey, requestId);
    const request: StopRunRequestV4 = {
      request_id: requestId,
      project_id: activeScope.projectId,
      conversation_id: activeScope.conversationId,
      run_id: activeScope.runId,
    };
    mutationVersionRef.current += 1;
    if (visibleAtStart) {
      setLoading(false);
      setRequesting(true);
      setStopping(true);
      setError("");
    }

    let operation!: Promise<StopRunReceiptV4>;
    operation = Promise.resolve()
      .then(() => api.agentV4RequestStop(request))
      .then((result) => {
        if (!isReceipt(result) || !receiptMatchesScope(result, activeScope)) throw new Error(STOP_ERROR);
        requestIdsRef.current.set(activeKey, result.request_id);
        const visible = mountedRef.current
          && scopeKeyRef.current === activeKey
          && generationRef.current === generation;
        if (visible) {
          receiptRef.current = result;
          setReceipt(result);
          setError("");
          if (result.status === "observed") terminalScopeRef.current = activeKey;
          setStopping(result.status === "requested" && terminalScopeRef.current !== activeKey);
        }
        return result;
      })
      .catch((failure) => {
        const visible = mountedRef.current
          && scopeKeyRef.current === activeKey
          && generationRef.current === generation;
        if (visible) {
          setStopping(false);
          setError(errorMessage(failure));
        }
        throw failure;
      })
      .finally(() => {
        if (inFlightRef.current?.promise === operation) inFlightRef.current = null;
        if (mountedRef.current && scopeKeyRef.current === activeKey && generationRef.current === generation) setRequesting(false);
      });
    inFlightRef.current = { key: activeKey, promise: operation };
    return operation;
  }, []);

  const refresh = useCallback(() => {
    if (!scopeRef.current || inFlightRef.current?.key === scopeKeyRef.current) return;
    setLoading(true);
    setError("");
    setReloadVersion((version) => version + 1);
  }, []);

  const markTerminal = useCallback((runIdToMark?: string) => {
    const activeScope = scopeRef.current;
    const activeKey = scopeKeyRef.current;
    if (!activeScope || !activeKey || (runIdToMark && runIdToMark !== activeScope.runId)) return;
    mutationVersionRef.current += 1;
    terminalScopeRef.current = activeKey;
    setStopping(false);
    const current = receiptRef.current;
    if (current && current.status !== "observed") {
      const observed = { ...current, status: "observed" as const };
      receiptRef.current = observed;
      setReceipt(observed);
    }
  }, []);

  const clear = useCallback(() => {
    mutationVersionRef.current += 1;
    terminalScopeRef.current = null;
    receiptRef.current = null;
    setReceipt(null);
    setRequesting(false);
    setStopping(false);
    setError("");
  }, []);

  return { receipt, loading, requesting, stopping, error, requestStop, refresh, markTerminal, clear };
}
