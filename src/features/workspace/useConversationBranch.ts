import { useEffect, useRef, useState } from "react";
import { conversationBranchCheckpoint, createConversationBranch } from "../../conversation-branch-api";
import type { ConversationBranchCheckpointKindV4, ConversationBranchV4, CreateConversationBranchRequestV4 } from "../../types";
import type { Locale } from "./copy";

type Attempt = { request?: CreateConversationBranchRequestV4; result?: ConversationBranchV4; running: boolean };
export function useConversationBranch(projectId: string, conversationId: string | null | undefined, locale: Locale, onCreated?: (branch: ConversationBranchV4) => Promise<boolean>) {
  const attempts = useRef(new Map<string, Attempt>());
  const scope = `${projectId}:${conversationId ?? ""}`;
  const scopeRef = useRef(scope); scopeRef.current = scope;
  const mounted = useRef(true);
  const [, refresh] = useState(0);
  const [error, setError] = useState("");
  const onCreatedRef = useRef(onCreated); onCreatedRef.current = onCreated;
  useEffect(() => { setError(""); }, [scope]);
  useEffect(() => { mounted.current = true; return () => { mounted.current = false; }; }, []);
  const current = () => mounted.current && scopeRef.current === scope;
  const update = () => { if (mounted.current) refresh((value) => value + 1); };
  async function execute(sourceMessageId?: string, checkpointKind?: ConversationBranchCheckpointKindV4, title?: string) {
    if (!conversationId || !onCreatedRef.current) return false;
    let attempt = attempts.current.get(scope);
    if (attempt?.running) return false;
    if (!attempt) { attempt = { running: false }; attempts.current.set(scope, attempt); }
    const saved = attempt;
    saved.running = true; setError(""); update();
    try {
      if (!saved.result) {
        if (!saved.request) {
          if (!sourceMessageId || !checkpointKind || !title?.trim()) throw new Error("missing branch input");
          const checkpoint = await conversationBranchCheckpoint(projectId, conversationId, sourceMessageId, checkpointKind);
          saved.request = { request_id: crypto.randomUUID(), project_id: projectId, source_conversation_id: conversationId, source_message_id: checkpoint.source_message_id, checkpoint_kind: checkpoint.checkpoint_kind, expected_source_sequence: checkpoint.source_sequence, expected_head_sequence: checkpoint.source_head_sequence, expected_boundary_hash: checkpoint.boundary_hash, title: title.trim() };
        }
        saved.result = await createConversationBranch(saved.request);
      }
      if (!current()) return false;
      const opened = await onCreatedRef.current(saved.result);
      if (opened) attempts.current.delete(scope);
      return opened;
    } catch (failure) {
      const rejected = typeof failure === "object" && failure !== null && "kind" in failure && failure.kind === "rejected";
      if (!saved.request || rejected) attempts.current.delete(scope);
      if (current()) setError(locale === "zh-CN" ? (rejected ? "无法从此节点创建分支，请刷新会话后重试。" : "分支创建或打开尚未确认。重试会核对同一请求。") : (rejected ? "This checkpoint cannot be branched. Refresh the conversation and retry." : "Branch creation or opening was not confirmed. Retry will reconcile the same request."));
      return false;
    } finally { saved.running = false; update(); }
  }
  const active = attempts.current.get(scope);
  return { start: execute, retry: () => execute(), busy: Boolean(active?.running), retryAvailable: Boolean(active?.request || active?.result), error };
}
