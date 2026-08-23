import type { AgentRunEventV4, RunSummaryV4 } from "../../types";
import type { Locale } from "./copy";

interface Props {
  locale: Locale;
  active: boolean;
  planLoading: boolean;
  v4Plan?: RunSummaryV4 | null;
  planApproved: boolean;
  runStarted: boolean;
  events: AgentRunEventV4[];
  onApprove: () => Promise<void> | void;
  onRegenerate?: () => Promise<void> | void;
}

export function V4PlanPanel({ locale, active, planLoading, v4Plan, planApproved, runStarted, events, onApprove, onRegenerate }: Props) {
  const zh = locale === "zh-CN";
  if (!active) return <div className="plan-panel-empty"><b>{zh ? "尚未进入 Plan 模式" : "Plan mode is not active"}</b><p>{zh ? "从输入框左侧的 + 菜单选择 Plan。默认 Agent 模式会直接执行任务。" : "Choose Plan from the + menu. Default Agent mode executes tasks directly."}</p></div>;
  if (!v4Plan?.plan) return <div className="plan-panel"><header><div><small>AGENT RUNTIME V4</small><h2>{planLoading ? (zh ? "正在生成计划" : "Generating plan") : (zh ? "Plan 模式配置" : "Plan mode setup")}</h2></div><span>{events.length ? `${events.length}` : "…"}</span></header><p className="plan-panel-lead">{zh ? "确认计算后端，然后在主输入框发送任务。规划阶段只读取和检查，不执行分析。" : "Confirm the compute backend, then send the task. Planning inspects without executing analysis."}</p></div>;
  const selection = v4Plan.compute_selection;
  return <div className="plan-panel"><header><div><small>AGENT RUNTIME V4 · PLAN</small><h2>{v4Plan.plan.objective}</h2></div><span>{runStarted ? (zh ? "运行中" : "Running") : planApproved ? (zh ? "已批准" : "Approved") : (zh ? "待审批" : "Review")}</span></header><section><h3>{zh ? "执行步骤" : "Execution steps"}</h3><ol>{v4Plan.plan.steps.map((step, index) => <li key={`${index}-${step}`}>{step}</li>)}</ol></section><section><h3>{zh ? "完成标准" : "Completion criteria"}</h3><ul>{v4Plan.plan.completion_criteria.map((criterion) => <li key={criterion}>{criterion}</li>)}</ul></section>{selection && <section className="plan-frozen"><h3>{zh ? "冻结配置" : "Frozen configuration"}</h3><code>{selection.backend_kind}:{selection.backend_id}</code><span>{selection.approval_policy ?? "risk_based"} · {selection.autonomy_mode} · {selection.environment} · network={selection.network_policy}</span>{selection.container_image && <small>{selection.container_image.reference}<br />{selection.container_image.image_id}</small>}</section>}<footer><small>{v4Plan.approval_hash ? `approval SHA-256 ${v4Plan.approval_hash}` : ""}</small><button disabled={planApproved || runStarted || !v4Plan.approval_hash} onClick={() => void onApprove()}>{zh ? "批准并运行" : "Approve and run"}</button><button disabled={planLoading || runStarted} onClick={() => void onRegenerate?.()}>{zh ? "重新生成" : "Regenerate"}</button></footer></div>;
}
