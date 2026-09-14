import { useEffect, useState } from "react";
import { getConversationBranch } from "../../conversation-branch-api";
import type { ConversationBranchV4 } from "../../types";
import type { Locale } from "./copy";
export function ConversationBranchBanner({ projectId, conversationId, locale, onSelect }: { projectId: string; conversationId: string; locale: Locale; onSelect?: (id: string) => void | Promise<void> }) {
  const [branch, setBranch] = useState<ConversationBranchV4 | null>(null);
  useEffect(() => {
    let active = true; setBranch(null);
    void getConversationBranch(projectId, conversationId).then((value) => { if (active && value?.project_id === projectId && value.branch_conversation_id === conversationId) setBranch(value); }).catch(() => {});
    return () => { active = false; };
  }, [projectId, conversationId]);
  if (!branch) return null;
  const zh = locale === "zh-CN";
  return <div className="conversation-branch-banner"><span>{zh ? "分支会话" : "Branched conversation"} · {branch.checkpoint_kind === "before_user" ? (zh ? "用户消息之前" : "Before user message") : (zh ? "回复之后" : "After response")}</span><button type="button" disabled={!onSelect} onClick={() => void onSelect?.(branch.source_conversation_id)}>{zh ? "查看来源会话" : "Open source conversation"}</button></div>;
}
