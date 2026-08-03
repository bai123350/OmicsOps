import { useState } from "react";

import * as api from "../../tauri-api";
import type { AnalysisPlanV2, ApprovedPlan, PlanValidation } from "../../types";

export function V2PlanningView(props: {
  profileId: string;
  projectId: string;
  environmentSummary: string;
  onStarted: (runId: string) => void;
}) {
  const [goal, setGoal] = useState("");
  const [questions, setQuestions] = useState<string[]>([]);
  const [answers, setAnswers] = useState<Record<string, string>>({});
  const [plan, setPlan] = useState<AnalysisPlanV2 | null>(null);
  const [validation, setValidation] = useState<PlanValidation | null>(null);
  const [approved, setApproved] = useState<ApprovedPlan | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");

  const perform = async (operation: () => Promise<void>) => {
    setBusy(true);
    setError("");
    try { await operation(); } catch (caught) {
      setError(caught instanceof Error ? caught.message : String(caught));
    } finally { setBusy(false); }
  };

  const requestTurn = () => perform(async () => {
    const turn = await api.planningTurn({ goal, environment_summary: props.environmentSummary, answers });
    if (turn.kind === "clarification") {
      setQuestions(turn.questions);
      return;
    }
    setQuestions([]);
    setPlan(turn.plan);
    setValidation(null);
    setApproved(null);
  });

  const updateStepTitle = (stageIndex: number, stepIndex: number, title: string) => {
    if (!plan) return;
    const next = structuredClone(plan);
    next.stages[stageIndex].steps[stepIndex].title = title;
    setPlan(next);
    setValidation(null);
    setApproved(null);
  };

  return (
    <section className="panel" aria-label="V2 工具契约规划">
      <div className="section-heading">
        <div><span className="eyebrow">PLAN SCHEMA V2</span><h2>目标对话与可验证 DAG</h2></div>
      </div>
      <label>分析目标<textarea aria-label="分析目标" value={goal} onChange={(event) => setGoal(event.target.value)} /></label>
      <button className="primary" disabled={busy || !goal.trim()} onClick={requestTurn}>生成计划</button>

      {questions.length > 0 && <div className="approval-box">
        <strong>需要澄清</strong>
        {questions.map((question) => <label key={question}>{question}<input aria-label={question} value={answers[question] ?? ""} onChange={(event) => setAnswers((current) => ({ ...current, [question]: event.target.value }))} /></label>)}
        <button disabled={busy} onClick={requestTurn}>提交澄清</button>
      </div>}

      {plan && <>
        <div className="plan-summary"><strong>{plan.title}</strong><span>{plan.summary}</span></div>
        {plan.stages.map((stage, stageIndex) => <article className="stage-card" key={stage.id}>
          <header><strong>{stage.goal}</strong><span>依赖：{stage.dependencies.join(", ") || "无"}</span></header>
          {stage.steps.map((step, stepIndex) => <div className="step-row" key={step.id}>
            <input aria-label={`${step.id} 标题`} value={step.title} onChange={(event) => updateStepTitle(stageIndex, stepIndex, event.target.value)} />
            <code>{step.action.kind === "tool" ? `${step.action.tool_id}@${step.action.version}` : step.action.command}</code>
            <span>{step.resources.max_cpu_cores} CPU · {step.resources.max_memory_gib} GiB · {step.resources.max_step_seconds}s</span>
            <span>输出：{step.expected_artifacts.join(", ") || "无"}</span>
          </div>)}
        </article>)}
        <div className="approval-box">
          <strong>审批摘要</strong>
          <span>工具：{plan.policy.allowed_tools.join(", ")}</span>
          <span>域名：{plan.policy.allowed_domains.join(", ") || "无网络"}</span>
          <span>最高风险：{plan.policy.max_risk}</span>
          <span>Legacy Shell：{plan.policy.allow_legacy_shell ? "允许（展示精确命令）" : "禁止"}</span>
        </div>
        <button disabled={busy} onClick={() => perform(async () => setValidation(await api.validatePlanV2(plan)))}>校验计划</button>
        {validation && (validation.valid
          ? <p className="success">计划校验通过</p>
          : <ul className="validation-list">{validation.issues.map((issue) => <li key={`${issue.path}:${issue.code}`}><code>{issue.path}</code> {issue.message}</li>)}</ul>)}
        <button className="primary" disabled={busy || !validation?.valid || Boolean(approved)} onClick={() => perform(async () => setApproved(await api.approvePlanV2(plan, plan.policy)))}>批准冻结计划</button>
      </>}

      {approved && <div className="approval-box"><strong>已冻结</strong><code>plan_hash: {approved.plan_hash}</code><button className="primary" disabled={busy} onClick={() => perform(async () => props.onStarted(await api.startRunV2(props.profileId, props.projectId, approved.id)))}>按审批记录启动</button></div>}
      {error && <p role="alert">{error}</p>}
    </section>
  );
}
