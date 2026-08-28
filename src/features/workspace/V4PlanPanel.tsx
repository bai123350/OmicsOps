import { useRef, useState } from "react";

import type { AgentRunEventV4, ProposedPlanRevisionV4, RunSummaryV4 } from "../../types";
import type { Locale } from "./copy";

interface Props {
  locale: Locale;
  active: boolean;
  planLoading: boolean;
  v4Plan?: RunSummaryV4 | null;
  latestPlanRevision?: ProposedPlanRevisionV4 | null;
  /** Snapshot-derived lock. Plan actions remain available while locked. */
  conversationLocked?: boolean;
  /** Parent action lock shared by approve and cancel transitions. */
  planActionBusy?: boolean;
  planApproved: boolean;
  /** False while a plan is generating or waiting for approval. */
  runStarted: boolean;
  events: AgentRunEventV4[];
  onApprove: () => Promise<void> | void;
  onRequestPlanRevision?: (feedback: string) => Promise<void> | void;
  onCancel?: () => Promise<void> | void;
}

const activeRevisionStatuses = new Set<ProposedPlanRevisionV4["status"]>(["generating", "revising", "pending"]);

export function V4PlanPanel({ locale, active, planLoading, v4Plan, latestPlanRevision, conversationLocked = false, planActionBusy = false, planApproved, runStarted, events, onApprove, onRequestPlanRevision, onCancel }: Props) {
  const zh = locale === "zh-CN";
  const [feedback, setFeedback] = useState("");
  const [revisionBusy, setRevisionBusy] = useState(false);
  const revisionBusyRef = useRef(false);

  if (!active) return <div className="plan-panel-empty"><b>{zh ? "尚未进入 Plan 模式" : "Plan mode is not active"}</b><p>{zh ? "从输入框左侧的 + 菜单选择 Plan。默认 Agent 模式会直接执行任务。" : "Choose Plan from the + menu. Default Agent mode executes tasks directly."}</p></div>;
  if (!v4Plan?.plan && !latestPlanRevision?.plan) return <div className="plan-panel"><header><div><small>AGENT RUNTIME V4</small><h2>{planLoading ? (zh ? "正在生成计划" : "Generating plan") : (zh ? "Plan 模式配置" : "Plan mode setup")}</h2></div><span>{events.length ? `${events.length}` : "…"}</span></header><p className="plan-panel-lead">{zh ? "确认计算后端，然后在主输入框发送任务。规划阶段只读取和检查，不执行分析。" : "Confirm the compute backend, then send the task. Planning inspects without executing analysis."}</p></div>;

  const plan = v4Plan?.plan ?? latestPlanRevision!.plan;
  const selection = v4Plan?.compute_selection;
  const revisionStatus = latestPlanRevision?.status;
  const revisionIsActive = Boolean(revisionStatus && activeRevisionStatuses.has(revisionStatus));
  const pendingRevision = revisionStatus === "pending";
  const approvalHash = v4Plan?.approval_hash;
  const actionBusy = planActionBusy || revisionBusy;
  const canApprove = !planApproved
    && !runStarted
    && !actionBusy
    && Boolean(approvalHash)
    && (!latestPlanRevision || pendingRevision);
  const canRequestChanges = pendingRevision && Boolean(onRequestPlanRevision) && !actionBusy;
  const canCancel = Boolean(onCancel) && (conversationLocked || revisionIsActive || Boolean(v4Plan?.status && ["planning", "generating", "revising", "awaiting_approval", "waiting_for_approval"].includes(v4Plan.status)));

  function revisionStatusLabel(status: ProposedPlanRevisionV4["status"]) {
    const labels: Record<ProposedPlanRevisionV4["status"], [string, string]> = {
      generating: ["生成中", "Generating"],
      revising: ["修改中", "Revising"],
      pending: ["待审批", "Pending approval"],
      approved: ["已批准", "Approved"],
      superseded: ["已被替代", "Superseded"],
      cancelled: ["已取消", "Cancelled"],
    };
    return labels[status][zh ? 0 : 1];
  }

  async function requestChanges() {
    const trimmed = feedback.trim();
    if (!trimmed || !onRequestPlanRevision || revisionBusyRef.current) return;
    revisionBusyRef.current = true;
    setRevisionBusy(true);
    try {
      await onRequestPlanRevision(trimmed);
      setFeedback("");
    } finally {
      revisionBusyRef.current = false;
      setRevisionBusy(false);
    }
  }

  const headlineStatus = latestPlanRevision
    ? `${zh ? "修订" : "Revision"} ${latestPlanRevision.revision} · ${revisionStatusLabel(latestPlanRevision.status)}`
    : runStarted
      ? (zh ? "运行中" : "Running")
      : planApproved
        ? (zh ? "已批准" : "Approved")
        : (zh ? "待审批" : "Review");

  return <div className="plan-panel" aria-label={zh ? "计划审核" : "Plan review"}>
    <header><div><small>AGENT RUNTIME V4 · PLAN</small><h2>{plan.objective}</h2></div><span>{headlineStatus}</span></header>
    {latestPlanRevision && <div className="plan-revision-meta" aria-label={zh ? "计划修订状态" : "Plan revision status"}><b>{zh ? `修订 ${latestPlanRevision.revision}` : `Revision ${latestPlanRevision.revision}`}</b><span>{revisionStatusLabel(latestPlanRevision.status)}</span><code>SHA-256 {latestPlanRevision.plan_hash.slice(0, 12)}</code></div>}
    <section><h3>{zh ? "执行步骤" : "Execution steps"}</h3><ol>{plan.steps.map((step, index) => <li key={`${index}-${step}`}>{step}</li>)}</ol></section>
    <section><h3>{zh ? "完成标准" : "Completion criteria"}</h3><ul>{plan.completion_criteria.map((criterion) => <li key={criterion}>{criterion}</li>)}</ul></section>
    {selection && <section className="plan-frozen"><h3>{zh ? "冻结配置" : "Frozen configuration"}</h3><code>{selection.backend_kind}:{selection.backend_id}</code><span>{selection.approval_policy ?? "risk_based"} · {selection.autonomy_mode} · {selection.environment} · network={selection.network_policy}</span>{selection.container_image && <small>{selection.container_image.reference}<br />{selection.container_image.image_id}</small>}</section>}
    {pendingRevision && onRequestPlanRevision && <section className="plan-revision-form" aria-label={zh ? "请求修改计划" : "Request plan changes"}><label htmlFor="plan-revision-feedback">{zh ? "修改意见" : "Feedback for the next revision"}</label><textarea id="plan-revision-feedback" aria-label={zh ? "计划修改意见" : "Plan feedback"} value={feedback} disabled={actionBusy} onChange={(event) => setFeedback(event.target.value)} placeholder={zh ? "告诉 Agent 需要调整哪些步骤或标准" : "Tell Agent which steps or criteria to change"} /><button disabled={!canRequestChanges || !feedback.trim()} onClick={() => void requestChanges()}>{revisionBusy ? (zh ? "提交中…" : "Submitting…") : (zh ? "请求修改" : "Request changes")}</button></section>}
    <footer><small>{approvalHash ? `approval SHA-256 ${approvalHash}` : ""}{conversationLocked && <>{approvalHash ? " · " : ""}{zh ? "当前会话已锁定" : "This conversation is locked"}</>}</small><div className="plan-action-row"><button disabled={!canApprove} onClick={() => void onApprove()}>{zh ? "批准并运行" : "Approve and run"}</button>{canCancel && <button className="plan-cancel-button" disabled={actionBusy} onClick={() => void onCancel?.()}>{zh ? "取消计划" : "Cancel plan"}</button>}</div></footer>
  </div>;
}
