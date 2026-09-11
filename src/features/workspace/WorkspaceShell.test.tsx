import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { WorkspaceShell } from "./WorkspaceShell";

const project = {
  id: "project-1",
  name: "PBMC 图谱",
  status: "running" as const,
  template: "single_cell_rna_seq" as const,
};

describe("WorkspaceShell", () => {
  it.each(["running", "waiting_for_approval", "needs_attention", "cancelled", "completed"])("does not present an ordinary %s contract as a Plan or hide its trace", (status) => {
    const plan = { schema_version: 4 as const, objective: "internal ordinary contract", steps: ["internal step"], completion_criteria: ["evidence"], requested_capabilities: [] };
    render(<WorkspaceShell project={project} locale="zh-CN" onLocaleChange={() => undefined} agentMode="agent"
      activeRunId="ordinary" runStarted={status === "running" || status === "waiting_for_approval"}
      v4Plan={{ run_id: "ordinary", status, plan, plan_hash: "internal", compute_selection: null, approval_hash: "internal", session_mode: "agent" }}
      agentRunEventsV4={[{ schema_version: 4, run_id: "ordinary", project_id: project.id, conversation_id: "c", sequence: 1, occurred_at: "2026-09-10T00:00:00Z", previous_hash: "", event_hash: "h", event: { kind: "model_text", text: "正在核对文献记录。" } }]} />);
    expect(screen.queryByText(/计划已生成/)).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "批准并运行" })).not.toBeInTheDocument();
    expect(screen.queryByText("internal ordinary contract")).not.toBeInTheDocument();
    expect(screen.getByText("正在核对文献记录。")).toBeVisible();
  });

  it("removes the run guidance card even when guidance is available", () => {
    render(<WorkspaceShell project={project} locale="zh-CN" onLocaleChange={() => undefined} guidanceAvailable activeConversationId="conversation" activeRunId="run" runStarted />);
    expect(screen.queryByRole("region", { name: "运行中指导" })).not.toBeInTheDocument();
    expect(screen.queryByText(/已接收的指导/)).not.toBeInTheDocument();
  });

  it("pretty prints JSON results and reports displayed line counts", () => {
    const base = { schema_version: 4 as const, run_id: "json", project_id: project.id, conversation_id: "c", previous_hash: "", event_hash: "h", occurred_at: "2026-09-10T00:00:00Z" };
    render(<WorkspaceShell project={project} locale="zh-CN" onLocaleChange={() => undefined} runStarted activeRunId="json" agentRunEventsV4={[
      { ...base, sequence: 1, event: { kind: "tool_requested", call: { call_id: "s", tool_id: "search_skills", arguments: { query: "literature" } } } },
      { ...base, sequence: 2, event: { kind: "tool_finished", outcome: { call_id: "s", tool_id: "search_skills", succeeded: true, model_content: '{"results":["literature-review"]}', data: null, provenance: [] } } },
    ]} />);
    const trace = screen.getByText("search_skills").closest("details")!;
    fireEvent.click(trace.querySelector("summary")!);
    expect(trace.querySelector("pre")?.textContent).toBe(JSON.stringify({ results: ["literature-review"] }, null, 2));
    expect(trace.querySelector("summary")).toHaveTextContent("5 行");
  });

  it("shows the ready card only for a pending approval plan", () => {
    const plan = { schema_version: 4 as const, objective: "用户请求的计划", steps: ["检查"], completion_criteria: ["核验"], requested_capabilities: [] };
    const props = { project, locale: "zh-CN" as const, onLocaleChange: () => undefined, agentMode: "plan" as const };
    const summary = { run_id: "plan-run", status: "awaiting_approval", plan, plan_hash: "h", compute_selection: null, approval_hash: "a", session_mode: "plan" as const };
    const { rerender } = render(<WorkspaceShell {...props} v4Plan={summary} />);
    expect(screen.getByText(/计划已生成/)).toBeVisible();
    rerender(<WorkspaceShell {...props} v4Plan={{ ...summary, status: "cancelled" }} />);
    expect(screen.queryByText(/计划已生成/)).not.toBeInTheDocument();
  });

  it.each(["zh-CN", "en-US"] as const)("keeps bookkeeping out of the conversation and explains attention in %s", (locale) => {
    const base = { schema_version: 4 as const, run_id: "run-attention", project_id: project.id, conversation_id: "conversation-1", occurred_at: "2026-09-10T00:00:00Z", previous_hash: "", event_hash: "hash" };
    render(<WorkspaceShell project={project} locale={locale} onLocaleChange={() => undefined} agentRunEventsV4={[
      { ...base, sequence: 1, event: { kind: "run_created", mode: "execute" } },
      { ...base, sequence: 2, event: { kind: "context_archived", archive: { archive_id: "private-archive", through_sequence: 1, size_bytes: 100, sha256: "hash" } } },
      { ...base, sequence: 3, event: { kind: "context_checkpointed", checkpoint: { schema_version: 4, through_sequence: 2, completion_criteria: [], unresolved_errors: [], recent_steps: [], scientific_state: {} } } },
      { ...base, sequence: 4, event: { kind: "run_needs_attention", message: "请配置可用的文献检索工具。" } },
    ]} />);
    const timeline = screen.getByRole("region", { name: locale === "zh-CN" ? "工具调用详情" : "Tool call details" });
    expect(screen.getByRole("region", { name: locale === "zh-CN" ? "分析对话" : "Analysis conversation" })).toHaveTextContent(locale === "zh-CN" ? "需要处理" : "Needs attention");
    expect(screen.getByText("请配置可用的文献检索工具。")).toBeVisible();
    expect(timeline).not.toHaveTextContent(/context_archived|context_checkpointed|run_needs_attention|run_created/);
    expect(screen.getByText("context_archived")).not.toBeVisible();
    expect(screen.queryByText("private-archive")).not.toBeInTheDocument();
    const diagnostics = screen.getByText(locale === "zh-CN" ? "诊断记录" : "Diagnostic records").closest("details")!;
    const recordFold = diagnostics.parentElement!.closest("details")!;
    recordFold.open = true;
    diagnostics.open = true;
    expect(screen.getByText("context_archived")).toBeVisible();
  });

  it("collapses completed progress while keeping the final answer visible", () => {
    const base = { schema_version: 4 as const, run_id: "run-visible", project_id: project.id, conversation_id: "conversation-1", previous_hash: "", event_hash: "hash" };
    render(<WorkspaceShell project={project} locale="zh-CN" onLocaleChange={() => undefined}
      messages={[
        { id: "request", role: "user", markdown: "检查数据", created_at: "2026-09-10T00:00:00Z" },
        { id: "answer", role: "assistant", markdown: "已检查 **3 个样本**。", created_at: "2026-09-10T00:00:05Z" },
      ]}
      agentRunEventsV4={[
        { ...base, sequence: 1, occurred_at: "2026-09-10T00:00:01Z", event: { kind: "run_created", mode: "execute" } },
        { ...base, sequence: 2, occurred_at: "2026-09-10T00:00:02Z", event: { kind: "model_text", text: "正在核对样本。" } },
        { ...base, sequence: 3, occurred_at: "2026-09-10T00:00:03Z", event: { kind: "run_completed" } },
      ]} />);
    expect(screen.getByText("执行过程").closest("details")).not.toHaveAttribute("open");
    expect(screen.getByText("正在核对样本。")).not.toBeVisible();
    expect(screen.getByText("3 个样本")).toBeVisible();
    expect(screen.getByText("正在核对样本。").compareDocumentPosition(screen.getByText("3 个样本")) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  });

  it("cancels saved recovery exclusively and hides actions after cancellation", async () => {
    let release: (() => void) | undefined;
    const cancel = vi.fn(() => new Promise<void>((resolve) => { release = resolve; }));
    const resume = vi.fn();
    const event = { schema_version: 4 as const, run_id: "receipt-run", project_id: project.id, conversation_id: "c", sequence: 1, occurred_at: "2026-09-09T00:00:00Z", previous_hash: "", event_hash: "hash", event: { kind: "runtime_recovery_available" as const, call_ids: ["cell"] } };
    const props = { project, locale: "zh-CN" as const, onLocaleChange: () => undefined, runStarted: true, activeRunId: event.run_id, onResumeAgentRunV4: resume, onCancelRuntimeRecoveryV4: cancel };
    const { rerender } = render(<WorkspaceShell {...props} agentRunEventsV4={[event]} />);
    fireEvent.click(screen.getByRole("button", { name: "取消此运行" }));
    fireEvent.click(screen.getByRole("button", { name: "取消中…" }));
    fireEvent.click(screen.getByRole("button", { name: "恢复已保存结果" }));
    expect(cancel).toHaveBeenCalledTimes(1);
    expect(cancel).toHaveBeenCalledWith(event.run_id);
    expect(resume).not.toHaveBeenCalled();
    release?.();
    await waitFor(() => expect(screen.getByRole("button", { name: "取消此运行" })).toBeEnabled());
    rerender(<WorkspaceShell {...props} agentRunEventsV4={[event, { ...event, sequence: 2, event: { kind: "run_cancelled" } }]} />);
    expect(screen.queryByRole("button", { name: "取消此运行" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "恢复已保存结果" })).not.toBeInTheDocument();
  });
  it("keeps a rejected recovery cancellation retryable without transport details", async () => {
    const cancel = vi.fn().mockRejectedValue(new Error("private transport"));
    const event = { schema_version: 4 as const, run_id: "receipt-run", project_id: project.id, conversation_id: "c", sequence: 1, occurred_at: "2026-09-09T00:00:00Z", previous_hash: "", event_hash: "hash", event: { kind: "runtime_recovery_available" as const, call_ids: ["cell"] } };
    render(<WorkspaceShell project={project} locale="zh-CN" onLocaleChange={() => undefined} agentRunEventsV4={[event]} onCancelRuntimeRecoveryV4={cancel} />);
    fireEvent.click(screen.getByRole("button", { name: "取消此运行" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("取消失败");
    expect(screen.getByRole("button", { name: "取消此运行" })).toBeEnabled();
    expect(screen.queryByText("private transport")).not.toBeInTheDocument();
  });
  it("resumes durable computation receipts once and hides the action after results are recorded", async () => {
    let finish: (() => void) | undefined;
    const resume = vi.fn(() => new Promise<void>((resolve) => { finish = resolve; }));
    const event = { schema_version: 4 as const, run_id: "receipt-run", project_id: project.id, conversation_id: "c", sequence: 1, occurred_at: "2026-09-09T00:00:00Z", previous_hash: "", event_hash: "hash", event: { kind: "runtime_recovery_available" as const, call_ids: ["cell"] } };
    const props = { project, locale: "zh-CN" as const, onLocaleChange: () => undefined, runStarted: true, activeRunId: event.run_id, onResumeAgentRunV4: resume, onCancelRuntimeRecoveryV4: vi.fn() };
    const { rerender } = render(<WorkspaceShell {...props} agentRunEventsV4={[event]} />);
    fireEvent.click(screen.getByRole("button", { name: "恢复已保存结果" }));
    expect(screen.getByRole("button", { name: "恢复中…" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "取消此运行" })).toBeDisabled();
    expect(resume).toHaveBeenCalledTimes(1);
    expect(resume).toHaveBeenCalledWith(event.run_id);
    finish?.();
    await waitFor(() => expect(screen.getByRole("button", { name: "恢复已保存结果" })).toBeEnabled());
    rerender(<WorkspaceShell {...props} agentRunEventsV4={[event, { ...event, sequence: 2, event: { kind: "tool_finished", outcome: { call_id: "cell", tool_id: "runtime.execute", succeeded: true, model_content: "result", data: {}, provenance: [] } } }]} />);
    expect(screen.queryByRole("button", { name: "恢复已保存结果" })).not.toBeInTheDocument();
  });
  it("switches the composer arrow to a stop square and restores it after cancellation", () => {
    const cancel = vi.fn();
    const props = { project, locale: "zh-CN" as const, onLocaleChange: () => undefined, onCancelRun: cancel };
    const { rerender, container } = render(<WorkspaceShell {...props} />);
    expect(container.querySelector(".send-button .lucide-arrow-up")).toBeInTheDocument();
    rerender(<WorkspaceShell {...props} runStarted activeRunId="run-stop" composerBusy />);
    const stop = screen.getByRole("button", { name: "终止运行" });
    expect(stop.closest(".composer")).toBeInTheDocument();
    expect(stop.querySelector(".lucide-square")).toBeInTheDocument();
    expect(stop).toBeEnabled();
    expect(screen.queryByRole("region", { name: "远程 Agent 运行控制" })).not.toBeInTheDocument();
    fireEvent.click(stop);
    expect(cancel).toHaveBeenCalledTimes(1);
    rerender(<WorkspaceShell {...props} runStarted activeRunId="run-stop" runStopping />);
    const stopping = screen.getByRole("button", { name: "终止中…" });
    expect(stopping).toBeDisabled();
    fireEvent.click(stopping);
    expect(cancel).toHaveBeenCalledTimes(1);
    rerender(<WorkspaceShell {...props} runStarted activeRunId="run-stop" agentRunEventsV4={[{
      schema_version: 4, run_id: "run-stop", project_id: project.id, conversation_id: "c", sequence: 1, occurred_at: "2026-09-08T00:00:00Z", previous_hash: "", event_hash: "hash", event: { kind: "run_cancelled" },
    }]} />);
    expect(screen.queryByRole("button", { name: "终止运行" })).not.toBeInTheDocument();
    expect(container.querySelector(".send-button .lucide-arrow-up")).toBeInTheDocument();
  });

  it("interleaves progress and individual read, write, edit calls even inside a batch", () => {
    const base = { schema_version: 4 as const, run_id: "run-compact", project_id: project.id, conversation_id: "conversation-1", previous_hash: "", event_hash: "hash" };
    const longOutput = `${"a".repeat(900)}\r\nlast line\r\n`;
    render(<WorkspaceShell project={project} locale="en-US" onLocaleChange={() => undefined} runStarted activeRunId={base.run_id} agentRunEventsV4={[
      { ...base, sequence: 1, occurred_at: "2026-08-17T00:00:00Z", event: { kind: "cycle_started", cycle_id: 1 } },
      { ...base, sequence: 2, occurred_at: "2026-08-17T00:00:01Z", event: { kind: "model_text", text: "Inspect the file first." } },
      { ...base, sequence: 3, occurred_at: "2026-08-17T00:00:02Z", event: { kind: "tool_batch_started", batch_id: 1, cycle_id: 1, phase: "executing", tool_names: ["project.read", "write", "edit"], call_ids: ["r", "w", "e"] } },
      { ...base, sequence: 4, occurred_at: "2026-08-17T00:00:03Z", event: { kind: "tool_requested", call: { call_id: "r", tool_id: "project.read", arguments: { path: "C:\\data\\input.txt" } } } },
      { ...base, sequence: 5, occurred_at: "2026-08-17T00:00:04Z", event: { kind: "tool_finished", outcome: { call_id: "r", tool_id: "project.read", succeeded: true, model_content: longOutput, data: null, provenance: [] } } },
      { ...base, sequence: 6, occurred_at: "2026-08-17T00:00:05Z", event: { kind: "model_text", text: "Now save and revise." } },
      { ...base, sequence: 7, occurred_at: "2026-08-17T00:00:06Z", event: { kind: "tool_requested", call: { call_id: "w", tool_id: "write", arguments: { file_path: "/Users/research/output.txt" } } } },
      { ...base, sequence: 8, occurred_at: "2026-08-17T00:00:07Z", event: { kind: "tool_requested", call: { call_id: "e", tool_id: "edit", arguments: { path: "C:\\data\\output.txt" } } } },
    ]} />);
    const timeline = screen.getByRole("region", { name: "Tool call details" });
    expect(Array.from(timeline.children).map((row) => row.querySelector(".markdown-content")?.textContent ?? row.querySelector("strong")?.textContent ?? row.textContent)).toEqual(["Inspect the file first.", "read", "Now save and revise.", "write", "edit"]);
    expect(timeline.closest("details")).toHaveClass("agent-run-fold");
    const read = within(timeline).getByText("read").closest("details")!;
    expect(read).not.toHaveAttribute("open");
    expect(read.querySelector("summary")).toHaveTextContent("1s · 2 lines");
    fireEvent.click(read.querySelector("summary")!);
    expect(read).toHaveAttribute("open");
    expect(read.querySelector("pre")?.textContent).toBe(longOutput);
    expect(within(timeline).getByText("/Users/research/output.txt")).toBeInTheDocument();
  });

  it("preserves reading position on new output and returns to the latest on demand", async () => {
    const props = { project, locale: "en-US" as const, onLocaleChange: () => undefined, onSend: vi.fn() };
    const { container, rerender } = render(<WorkspaceShell {...props} streamingAssistant="First" />);
    const stream = container.querySelector(".message-stream") as HTMLElement;
    Object.defineProperties(stream, { scrollHeight: { configurable: true, value: 1200 }, clientHeight: { configurable: true, value: 400 } });
    stream.scrollTop = 100;
    fireEvent.scroll(stream);
    rerender(<WorkspaceShell {...props} streamingAssistant="First and second" />);
    await new Promise((resolve) => setTimeout(resolve, 30));
    expect(stream.scrollTop).toBe(100);
    fireEvent.click(screen.getByRole("button", { name: /Back to latest/ }));
    expect(stream.scrollTop).toBe(1200);
    expect(screen.queryByRole("button", { name: /Back to latest/ })).not.toBeInTheDocument();
  });

  it("renders public progress without the obsolete phase dashboard or private reasoning", () => {
    const base = { schema_version: 4 as const, run_id: "run-guided", project_id: project.id, conversation_id: "conversation-1", previous_hash: "", event_hash: "hash" };
    const events = [
      { ...base, sequence: 1, occurred_at: "2026-08-17T00:00:00Z", event: Object.assign({ kind: "model_text" as const, text: "公开摘要：先检查输入。" }, { raw_reasoning: "private reasoning must never render" }) },
      { ...base, sequence: 2, occurred_at: "2026-08-17T00:00:01Z", event: { kind: "task_shape_selected" as const, task_shape: "multi_step" as const, source: "host" as const, reason: "needs verification" } },
      { ...base, sequence: 3, occurred_at: "2026-08-17T00:00:02Z", event: { kind: "phase_changed" as const, phase: "organizing" as const } },
      { ...base, sequence: 4, occurred_at: "2026-08-17T00:00:03Z", event: { kind: "task_list_updated" as const, revision: 2, change_summary: "分解为可验证步骤", tasks: [
        { id: "task-1", title: "检查输入矩阵", status: "completed" as const },
        { id: "task-2", title: "评估批次效应", status: "in_progress" as const },
        { id: "task-3", title: "等待参考注释", status: "blocked" as const, blocked_reason: "缺少注释文件" },
      ] } },
      { ...base, sequence: 5, occurred_at: "2026-08-17T00:00:04Z", event: { kind: "tool_requested" as const, call: { call_id: "update-1", tool_id: "agent.update_tasks", arguments: { tasks: ["private task payload"] } } } },
      { ...base, sequence: 6, occurred_at: "2026-08-17T00:00:05Z", event: { kind: "tool_requested" as const, call: { call_id: "route-1", tool_id: "agent.route_request", arguments: { route: "adaptive" } } } },
      { ...base, sequence: 7, occurred_at: "2026-08-17T00:00:06Z", event: { kind: "tool_batch_started" as const, batch_id: 2, cycle_id: 1, phase: "organizing" as const, tool_names: ["agent.update_tasks"], call_ids: ["update-1"] } },
      { ...base, sequence: 8, occurred_at: "2026-08-17T00:00:07Z", event: { kind: "tool_batch_finished" as const, batch_id: 2, cycle_id: 1, phase: "organizing" as const, tool_names: ["agent.update_tasks"], call_ids: ["update-1"], duration_ms: 0, succeeded: 1, failed: 0 } },
    ];
    render(<WorkspaceShell project={project} locale="zh-CN" onLocaleChange={() => undefined} runStarted activeRunId="run-guided" agentRunEventsV4={events} />);

    expect(screen.queryByRole("region", { name: "Agent 阶段轨迹" })).not.toBeInTheDocument();
    expect(screen.queryByRole("region", { name: "任务列表" })).not.toBeInTheDocument();
    expect(screen.getByText("公开摘要：先检查输入。")).toBeVisible();
    expect(screen.getByText("PROGRESS")).toBeVisible();
    expect(screen.queryByText("private reasoning must never render")).not.toBeInTheDocument();
    expect(screen.queryByText("agent.update_tasks")).not.toBeInTheDocument();
    expect(screen.getByText("task_list_updated")).not.toBeVisible();

  });

  it("summarizes tool batches by phase and model cycle while keeping input decisions in the same run", () => {
    const onAnswer = vi.fn();
    const base = { schema_version: 4 as const, run_id: "run-batch", project_id: project.id, conversation_id: "conversation-1", previous_hash: "", event_hash: "hash" };
    render(<WorkspaceShell project={project} locale="en-US" onLocaleChange={() => undefined} runStarted activeRunId="run-batch" onAnswerAgentQuestionV4={onAnswer} agentRunEventsV4={[
      { ...base, sequence: 1, occurred_at: "2026-08-17T00:00:00Z", event: { kind: "task_shape_selected" as const, task_shape: "multi_step" as const, source: "model" as const, reason: "requires verification" } },
      { ...base, sequence: 2, occurred_at: "2026-08-17T00:00:01Z", event: { kind: "cycle_started" as const, cycle_id: 3 } },
      { ...base, sequence: 3, occurred_at: "2026-08-17T00:00:02Z", event: { kind: "tool_batch_started" as const, batch_id: 8, cycle_id: 3, phase: "executing" as const, tool_names: ["project.read", "runtime.execute"], call_ids: ["call-1", "call-2"] } },
      { ...base, sequence: 4, occurred_at: "2026-08-17T00:00:03Z", event: { kind: "tool_batch_finished" as const, batch_id: 8, cycle_id: 3, phase: "executing" as const, tool_names: ["project.read", "runtime.execute"], call_ids: ["call-1", "call-2"], duration_ms: 1200, succeeded: 2, failed: 0 } },
      { ...base, sequence: 5, occurred_at: "2026-08-17T00:00:04Z", event: { kind: "input_requested" as const, question_id: "species", question: "Which species?" } },
    ]} />);

    const fold = screen.getByText("Processing").closest("details")!;
    fold.open = false;
    expect(screen.getByText("Which species?")).toBeVisible();
    expect(screen.getByText("Which species?").closest(".agent-run-fold")).toBeNull();
    expect(screen.getByText("Which species?")).toBeInTheDocument();
    fireEvent.change(screen.getByRole("textbox", { name: "Answer V4 question" }), { target: { value: "human" } });
    fireEvent.click(screen.getByRole("button", { name: "Answer and resume" }));
    expect(onAnswer).toHaveBeenCalledWith("run-batch", "species", "human");
  });

  it("keeps fast-path runs compact and omits an empty task panel", () => {
    const base = { schema_version: 4 as const, run_id: "run-fast", project_id: project.id, conversation_id: "conversation-1", previous_hash: "", event_hash: "hash" };
    render(<WorkspaceShell project={project} locale="en-US" onLocaleChange={() => undefined} runStarted activeRunId="run-fast" agentRunEventsV4={[
      { ...base, sequence: 1, occurred_at: "2026-08-17T00:00:00Z", event: { kind: "task_shape_selected" as const, task_shape: "fast" as const, source: "host" as const, reason: "one bounded operation" } },
      { ...base, sequence: 2, occurred_at: "2026-08-17T00:00:01Z", event: { kind: "task_list_updated" as const, revision: 1, change_summary: "no tasks needed", tasks: [] } },
    ]} />);

    expect(screen.queryByRole("region", { name: "Agent guided trajectory" })).not.toBeInTheDocument();
    expect(screen.queryByRole("region", { name: "Task list" })).not.toBeInTheDocument();
  });

  it("renders the V4 hash-chained trajectory", () => {
    render(<WorkspaceShell project={project} locale="zh-CN" onLocaleChange={() => undefined} runStarted activeRunId="run-v4" agentRunEventsV4={[{
      schema_version: 4, run_id: "run-v4", project_id: project.id, conversation_id: "conversation-1", sequence: 1,
      occurred_at: "2026-08-17T00:00:00Z", previous_hash: "", event_hash: "a".repeat(64), event: { kind: "run_created", mode: "plan" },
    }]} />);
    expect(screen.getByText("执行过程")).toBeInTheDocument();
    expect(screen.getByText("Agent 正在处理任务")).toBeInTheDocument();
    expect(screen.queryByText("规划启动")).not.toBeInTheDocument();
    expect(screen.getByText("run_created").closest("details")).not.toHaveAttribute("open");
  });
  it("warns when an active run has been silent for 90 seconds without hiding stop", () => {
    const onCancel = vi.fn();
    const lastActivity = new Date(Date.now() - 90_001).toISOString();
    render(<WorkspaceShell project={project} locale="zh-CN" onLocaleChange={() => undefined} runStarted activeRunId="run-stalled" activeRunLastActivityAt={lastActivity} onCancelRun={onCancel} agentRunEventsV4={[{
      schema_version: 4, run_id: "run-stalled", project_id: project.id, conversation_id: "conversation-1", sequence: 1,
      occurred_at: lastActivity, previous_hash: "", event_hash: "a".repeat(64), event: { kind: "run_created", mode: "execute" },
    }]} />);

    expect(screen.getByRole("status")).toHaveTextContent("超过 90 秒未收到新的 Agent 事件");
    expect(screen.getByRole("button", { name: "终止运行" })).toBeInTheDocument();
  });
  it("coalesces character-sized V4 model deltas into one completed response", () => {
    const deltas = ["我", "先", "检查", "输入", "目录", "。"];
    render(<WorkspaceShell project={project} locale="zh-CN" onLocaleChange={() => undefined} runStarted activeRunId="run-v4-text" agentRunEventsV4={deltas.map((text, index) => ({
      schema_version: 4 as const, run_id: "run-v4-text", project_id: project.id, conversation_id: "conversation-1", sequence: index + 1,
      occurred_at: "2026-08-17T00:00:00Z", previous_hash: String(index), event_hash: String(index + 1), event: { kind: "model_text" as const, text },
    }))} />);
    expect(screen.getAllByRole("article", { name: "模型输出" })).toHaveLength(1);
    expect(screen.getByRole("article", { name: "模型输出" })).toHaveTextContent("我先检查输入目录。");
  });
  it("shows the V4 input question and hides the answer form after it is answered", () => {
    const onAnswer = vi.fn();
    const base = { schema_version: 4 as const, run_id: "run-question", project_id: project.id, conversation_id: "conversation-1", previous_hash: "", event_hash: "hash" };
    const requested = { ...base, sequence: 1, occurred_at: "2026-08-17T00:00:01Z", event: { kind: "input_requested" as const, question_id: "species", question: "该数据来自人还是小鼠？" } };
    const { rerender } = render(<WorkspaceShell project={project} locale="zh-CN" onLocaleChange={() => undefined} runStarted activeRunId="run-question" agentRunEventsV4={[requested]} onAnswerAgentQuestionV4={onAnswer} />);

    expect(screen.getByText("需要补充信息")).toBeInTheDocument();
    expect(screen.getByText("该数据来自人还是小鼠？")).toBeInTheDocument();
    fireEvent.change(screen.getByRole("textbox", { name: "回答 V4 问题" }), { target: { value: "人" } });
    fireEvent.click(screen.getByRole("button", { name: "回答并恢复" }));
    expect(onAnswer).toHaveBeenCalledWith("run-question", "species", "人");

    rerender(<WorkspaceShell project={project} locale="zh-CN" onLocaleChange={() => undefined} runStarted activeRunId="run-question" agentRunEventsV4={[requested,
      { ...base, sequence: 2, occurred_at: "2026-08-17T00:00:02Z", event: { kind: "user_input_answered" as const, question_id: "species", answer: "人" } },
    ]} onAnswerAgentQuestionV4={onAnswer} />);
    expect(screen.getByText("已提交回答：人")).toBeInTheDocument();
    expect(screen.queryByRole("textbox", { name: "回答 V4 问题" })).not.toBeInTheDocument();
  });
  it("restores a completed V4 run after its persisted user message", () => {
    const base = { schema_version: 4 as const, run_id: "run-history", project_id: project.id, conversation_id: "conversation-1", previous_hash: "", event_hash: "hash" };
    render(<WorkspaceShell project={project} locale="zh-CN" onLocaleChange={() => undefined}
      messages={[{ id: "message-1", role: "user", markdown: "检查矩阵", created_at: "2026-08-17T00:00:00Z" }]}
      agentRunEventsV4={[
        { ...base, sequence: 1, occurred_at: "2026-08-17T00:00:01Z", event: { kind: "run_created", mode: "execute" } },
        { ...base, sequence: 2, occurred_at: "2026-08-17T00:00:02Z", event: { kind: "model_text", text: "矩阵检查完成。" } },
        { ...base, sequence: 3, occurred_at: "2026-08-17T00:00:03Z", event: { kind: "run_completed" } },
      ]} />);
    expect(screen.getByText("检查矩阵")).toBeInTheDocument();
    expect(screen.getByText(/已完成 · 0 个步骤/)).toBeInTheDocument();
    fireEvent.click(screen.getByText("执行过程"));
    expect(screen.getByRole("article", { name: "模型输出" })).toHaveTextContent("矩阵检查完成。");
  });
  it("offers to resume a failed V4 run and treats later events as running", () => {
    const onResume = vi.fn();
    const base = { schema_version: 4 as const, run_id: "run-recover", project_id: project.id, conversation_id: "conversation-1", previous_hash: "", event_hash: "hash" };
    const failed = [
      { ...base, sequence: 1, occurred_at: "2026-08-17T00:00:01Z", event: { kind: "run_created" as const, mode: "execute" as const } },
      { ...base, sequence: 2, occurred_at: "2026-08-17T00:00:02Z", event: { kind: "run_failed" as const, message: "system environment cannot be created or changed" } },
    ];
    const { rerender } = render(<WorkspaceShell project={project} locale="zh-CN" onLocaleChange={() => undefined} agentRunEventsV4={failed} onResumeAgentRunV4={onResume} />);

    fireEvent.click(screen.getByText("执行过程"));
    fireEvent.click(screen.getByRole("button", { name: "继续运行" }));
    expect(onResume).toHaveBeenCalledWith("run-recover");

    rerender(<WorkspaceShell project={project} locale="zh-CN" onLocaleChange={() => undefined} runStarted activeRunId="run-recover" agentRunEventsV4={[...failed,
      { ...base, sequence: 3, occurred_at: "2026-08-17T00:00:03Z", event: { kind: "tool_dispatch_resolved" as const, call_id: "call-1", resolution: "side_effect_not_observed" as const, evidence: "immutable system ensure" } },
    ]} onResumeAgentRunV4={onResume} />);
    expect(screen.getByText(/运行中 · 0 个步骤/)).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "继续运行" })).not.toBeInTheDocument();
  });
  it("shows all compute choices in chat and keeps full access container-only", () => {
    const onBackendChange = vi.fn();
    const onApprovalPolicyChange = vi.fn();
    render(<WorkspaceShell project={project} locale="zh-CN" onLocaleChange={() => undefined} onSend={() => true}
      computeBackendId="local" onComputeBackendChange={onBackendChange} onApprovalPolicyChange={onApprovalPolicyChange}
      computeBackends={[
        { descriptor: { schema_version: 4, backend_id: "local", kind: "local", isolation: "process", available: true, supports_python: true, supports_r: false, supports_network_policy: false }, selectable: true, reason: null, python_status: "available", r_status: "unavailable", resolved_image_id: null },
        { descriptor: { schema_version: 4, backend_id: "ssh:server", kind: "ssh", isolation: "process", available: true, supports_python: true, supports_r: false, supports_network_policy: false }, selectable: true, reason: null, python_status: "available", r_status: "unavailable", resolved_image_id: null },
        { descriptor: { schema_version: 4, backend_id: "docker", kind: "docker", isolation: "container", available: true, supports_python: true, supports_r: true, supports_network_policy: true }, selectable: true, reason: null, python_status: "unverified", r_status: "unverified", resolved_image_id: "sha256:abc" },
        { descriptor: { schema_version: 4, backend_id: "podman", kind: "podman", isolation: "container", available: false, supports_python: false, supports_r: false, supports_network_policy: true }, selectable: false, reason: "engine unavailable", python_status: "unavailable", r_status: "unavailable", resolved_image_id: null },
      ]} />);
    fireEvent.click(screen.getByRole("button", { name: "选择计算后端" }));
    expect(screen.getByRole("region", { name: "V4 计算后端" })).toHaveTextContent("本地与 SSH 按需连接；选择配置不代表依赖已安装");
    expect(screen.getByRole("radio", { name: /LOCAL/ })).toBeInTheDocument();
    expect(screen.getByRole("radio", { name: /SSH/ })).toBeInTheDocument();
    expect(screen.getByRole("radio", { name: /DOCKER/ })).toBeInTheDocument();
    expect(screen.getByRole("radio", { name: /PODMAN/ })).toBeDisabled();
    fireEvent.click(screen.getByRole("radio", { name: /DOCKER/ }));
    expect(onBackendChange).toHaveBeenCalledWith("docker");
    fireEvent.click(screen.getByRole("button", { name: "Agent 权限" }));
    expect(screen.getByRole("menuitemradio", { name: /^请求批准/ })).toBeInTheDocument();
    expect(screen.getByRole("menuitemradio", { name: /^帮我批准/ })).toBeInTheDocument();
    expect(screen.getByRole("menuitemradio", { name: /^完全访问权限/ })).toBeDisabled();
  });
  it("enables full access only for an available offline container", () => {
    const onApprovalPolicyChange = vi.fn();
    const onAutonomyModeChange = vi.fn();
    render(<WorkspaceShell project={project} locale="zh-CN" onLocaleChange={() => undefined} onSend={() => true}
      computeBackendId="docker" onApprovalPolicyChange={onApprovalPolicyChange} onAutonomyModeChange={onAutonomyModeChange}
      computeBackends={[{ descriptor: { schema_version: 4, backend_id: "docker", kind: "docker", isolation: "container", available: true, supports_python: true, supports_r: true, supports_network_policy: true }, selectable: true, reason: null, python_status: "unverified", r_status: "unverified", resolved_image_id: "sha256:abc" }]} />);
    fireEvent.click(screen.getByRole("button", { name: "Agent 权限" }));
    const fullAccess = screen.getByRole("menuitemradio", { name: /^完全访问权限/ });
    expect(fullAccess).toBeEnabled();
    fireEvent.click(fullAccess);
    expect(onApprovalPolicyChange).toHaveBeenCalledWith("full_access");
    expect(onAutonomyModeChange).toHaveBeenCalledWith("full_auto");
  });
  it("keeps ordinary questions in chat until the user explicitly selects Plan mode from Agent controls", async () => {
    const onSend = vi.fn().mockResolvedValue(true);
    render(<WorkspaceShell project={project} locale="zh-CN" onLocaleChange={() => undefined} onSend={onSend}
      computeBackendId="local" computeBackends={[{ descriptor: { schema_version: 4, backend_id: "local", kind: "local", isolation: "process", available: true, supports_python: true, supports_r: false, supports_network_policy: false }, selectable: true, reason: null, python_status: "available", r_status: "unavailable", resolved_image_id: null }]} />);

    fireEvent.change(screen.getByRole("textbox", { name: /描述研究目标/ }), { target: { value: "这个文件是什么格式？" } });
    fireEvent.click(screen.getByRole("button", { name: "发送" }));
    await waitFor(() => expect(onSend).toHaveBeenCalledWith("这个文件是什么格式？", "chat"));
    expect(screen.queryByRole("region", { name: "V4 计算后端" })).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Agent 权限" }));
    fireEvent.click(screen.getByRole("menuitemcheckbox", { name: "先做计划" }));
    fireEvent.click(screen.getByRole("button", { name: "选择计算后端" }));
    expect(screen.getByRole("region", { name: "V4 计算后端" })).toBeInTheDocument();
    fireEvent.change(screen.getByRole("textbox", { name: /描述研究目标/ }), { target: { value: "执行完整 QC" } });
    fireEvent.click(screen.getByRole("button", { name: "发送" }));
    await waitFor(() => expect(onSend).toHaveBeenLastCalledWith("执行完整 QC", "plan"));
  });
  it("keeps projects, scientific conversation, and context visible together", () => {
    render(<WorkspaceShell project={project} locale="zh-CN" onLocaleChange={() => undefined} />);

    expect(screen.getByRole("navigation", { name: "项目与会话" })).toBeInTheDocument();
    expect(screen.getByRole("main", { name: "科研对话" })).toBeInTheDocument();
    expect(screen.queryByRole("complementary", { name: "项目上下文" })).not.toBeInTheDocument();
    expect(screen.getAllByText("PBMC 图谱")).toHaveLength(2);
    expect(screen.getByText("Scanpy 质量控制")).toBeInTheDocument();
  });

  it("shows an honest empty artifact catalog", () => {
    render(<WorkspaceShell project={project} locale="zh-CN" onLocaleChange={() => undefined} />);

    if (!screen.queryByRole("complementary")) fireEvent.click(screen.getByRole("button", { name: "展开侧栏" }));
    if (!screen.queryByRole("menu", { name: "侧栏内容" })) fireEvent.click(screen.getByRole("button", { name: "添加侧栏标签" }));
    fireEvent.click(screen.getByRole("menuitemcheckbox", { name: "Artifacts (0)" }));
    expect(screen.getByText("暂无登记产物")).toBeVisible();
    expect(screen.queryByText("UMAP 聚类概览")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "展开预览" })).not.toBeInTheDocument();
  });

  it("loads the selected remote image into the preview", async () => {
    const onPreviewImage = vi.fn().mockResolvedValue({ relative_path: "results/umap.png", mime_type: "image/png", size_bytes: 1024, sha256: "a".repeat(64), data_url: "data:image/png;base64,iVBORw0KGgo=" });
    render(<WorkspaceShell project={project} locale="en-US" onLocaleChange={() => undefined} remoteFiles={[{ relative_path: "results/umap.png", directory: false, size_bytes: 1024, modified_unix_seconds: 0 }, { relative_path: "results/markers.csv", directory: false, size_bytes: 20, modified_unix_seconds: 0 }]} onPreviewImage={onPreviewImage} />);

    if (!screen.queryByRole("complementary")) fireEvent.click(screen.getByRole("button", { name: "Expand sidebar" }));
    if (!screen.queryByRole("menu", { name: "Sidebar sections" })) fireEvent.click(screen.getByRole("button", { name: "Add sidebar tab" }));
    fireEvent.click(screen.getByRole("menuitemcheckbox", { name: "Artifacts (0)" }));
    expect(screen.getByRole("combobox", { name: "Select project image" })).toHaveValue("results/umap.png");
    fireEvent.click(screen.getByRole("button", { name: "Show image" }));
    expect(await screen.findByRole("img", { name: "results/umap.png" })).toHaveAttribute("src", "data:image/png;base64,iVBORw0KGgo=");
    expect(onPreviewImage).toHaveBeenCalledWith("results/umap.png");
    expect(screen.queryByRole("option", { name: "results/markers.csv" })).not.toBeInTheDocument();
  });

  it("renders English copy from the shared locale resource", () => {
    render(<WorkspaceShell project={project} locale="en-US" onLocaleChange={() => undefined} />);

    expect(screen.getByRole("main", { name: "Research conversation" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Expand sidebar" }));
    fireEvent.click(screen.getByRole("button", { name: "Add sidebar tab" }));
    expect(screen.getByRole("menuitemcheckbox", { name: "Notebook (0)" })).toBeInTheDocument();
  });

  it("sends an ordinary message without exposing a Plan tab", () => {
    render(<WorkspaceShell project={project} locale="zh-CN" onLocaleChange={() => undefined} />);
    fireEvent.change(screen.getByRole("textbox", { name: /描述研究目标/ }), { target: { value: "先检查双细胞率" } });
    fireEvent.click(screen.getByRole("button", { name: "发送" }));
    expect(screen.getByText("先检查双细胞率")).toBeInTheDocument();
    expect(screen.queryByRole("tab", { name: "Plan" })).not.toBeInTheDocument();
  });

  it.each(["local", "ssh"] as const)("sends an ordinary research request with unverified %s compute", async (kind) => {
    const onSend = vi.fn(() => true);
    const backendId = kind === "ssh" ? "ssh:offline" : "local";
    render(<WorkspaceShell project={project} locale="zh-CN" onLocaleChange={() => undefined} agentMode="agent" onSend={onSend}
      computeBackendId={backendId} computeBackends={[{ descriptor: { schema_version: 4, backend_id: backendId, kind, isolation: "process", available: false, supports_python: true, supports_r: true, supports_network_policy: false }, selectable: true, reason: null, python_status: "unverified", r_status: "unverified", resolved_image_id: null }]} />);
    fireEvent.change(screen.getByRole("textbox", { name: /描述研究目标/ }), { target: { value: "寻找肝癌单细胞文献" } });
    expect(screen.getByRole("button", { name: "发送" })).toBeEnabled();
    fireEvent.click(screen.getByRole("button", { name: "发送" }));
    await waitFor(() => expect(onSend).toHaveBeenCalledWith("寻找肝癌单细胞文献", "chat"));
  });

  it("shows the real agent state instead of a fixed remote progress value", async () => {
    let finish!: (value: boolean) => void;
    const onSend = vi.fn(() => new Promise<boolean>((resolve) => { finish = resolve; }));
    const backend = { descriptor: { schema_version: 4 as const, backend_id: "local", kind: "local" as const, isolation: "process" as const, available: true, supports_python: true, supports_r: false, supports_network_policy: false }, selectable: true, reason: null, python_status: "available" as const, r_status: "unavailable" as const, resolved_image_id: null };
    const { rerender } = render(<WorkspaceShell project={project} locale="zh-CN" onLocaleChange={() => undefined} onSend={onSend} computeBackendId="local" computeBackends={[backend]} />);

    expect(screen.queryByText("65%")).not.toBeInTheDocument();
    fireEvent.change(screen.getByRole("textbox", { name: /描述研究目标/ }), { target: { value: "检查 hg19 数据" } });
    fireEvent.click(screen.getByRole("button", { name: "发送" }));
    expect(screen.getByRole("textbox", { name: /描述研究目标/ })).toHaveValue("");

    rerender(<WorkspaceShell project={project} locale="zh-CN" onLocaleChange={() => undefined} onSend={onSend} computeBackendId="local" computeBackends={[backend]} agentBusy agentNotice="503 model_not_found" />);
    expect(screen.getByRole("status")).toHaveTextContent("正在等待模型响应");
    expect(screen.getByRole("alert")).toHaveTextContent("503 model_not_found");
    finish(true);
    await waitFor(() => expect(onSend).toHaveBeenCalledWith("检查 hg19 数据", "chat"));
  });

  it("keeps the composer available with a long multiline assistant response", () => {
    const markdown = Array.from({ length: 80 }, (_, index) => `步骤 ${index + 1}\n\`gene_${index}\``).join("\n");
    render(<WorkspaceShell project={project} locale="zh-CN" onLocaleChange={() => undefined} onSend={() => true} messages={[{ id: "assistant-long", role: "assistant", markdown }]} />);

    expect(screen.getByText(/步骤 80/)).toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: /描述研究目标/ })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "发送" })).toBeInTheDocument();
  });

  it("requires an explicit decision for a pending V4 tool approval", () => {
    const decide = vi.fn();
    const request = { approval_id: "approval-1", call: { call_id: "call-1", tool_id: "runtime.execute", arguments: { code: "print(1)" } }, effect: "runtime" as const, reason: "首次代码执行需要批准", call_hash: "c".repeat(64) };
    render(<WorkspaceShell project={project} locale="zh-CN" onLocaleChange={() => undefined} runStarted activeRunId="run-approval" onCancelRun={vi.fn()} onDecideToolApprovalV4={decide} agentRunEventsV4={[{
      schema_version: 4, run_id: "run-approval", project_id: project.id, conversation_id: "conversation-1", sequence: 1, occurred_at: "2026-08-17T00:00:00Z", previous_hash: "", event_hash: "hash", event: { kind: "tool_approval_requested", request },
    }]} />);
    expect(screen.getByText(/等待工具审批 · 0 个步骤/)).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "终止运行" })).not.toBeInTheDocument();
    expect(screen.getByRole("region", { name: "工具审批" })).toHaveTextContent("runtime.execute");
    expect(screen.getByRole("region", { name: "工具审批" })).toHaveTextContent("首次代码执行需要批准");
    fireEvent.click(screen.getByRole("button", { name: "批准并继续" }));
    expect(decide).toHaveBeenCalledWith("run-approval", "approval-1", "c".repeat(64), "approved");
  });

  it("binds browser approval scope and resumes the same run after a successful browser call", () => {
    const decide = vi.fn();
    const resume = vi.fn();
    const base = { schema_version: 4 as const, run_id: "run-browser", project_id: project.id, conversation_id: "conversation-1", previous_hash: "", event_hash: "hash" };
    const request = { approval_id: "approval-browser", call: { call_id: "call-browser", tool_id: "web_open_tab", arguments: { session: "workspace", url: "https://example.org/paper" } }, effect: "network" as const, reason: "需要访问独立来源", call_hash: "d".repeat(64) };
    const { rerender } = render(<WorkspaceShell project={project} locale="zh-CN" onLocaleChange={() => undefined} runStarted activeRunId="run-browser" onDecideToolApprovalV4={decide} onResumeAgentRunV4={resume} agentRunEventsV4={[
      { ...base, sequence: 1, occurred_at: "2026-08-17T00:00:00Z", event: { kind: "tool_approval_requested" as const, request } },
    ]} />);
    fireEvent.change(screen.getByRole("combobox", { name: "浏览器授权范围" }), { target: { value: "project" } });
    fireEvent.click(screen.getByRole("button", { name: "批准并继续" }));
    expect(decide).toHaveBeenCalledWith("run-browser", "approval-browser", "d".repeat(64), "approved", "project");

    const disconnected = [
      { ...base, sequence: 2, occurred_at: "2026-08-17T00:00:01Z", event: { kind: "browser_connection_required" as const, session: "workspace" as const, protocol_version: 1, message: "connect the extension" } },
    ];
    rerender(<WorkspaceShell project={project} locale="zh-CN" onLocaleChange={() => undefined} runStarted activeRunId="run-browser" onResumeAgentRunV4={resume} agentRunEventsV4={disconnected} />);
    expect(screen.getByText(/等待连接浏览器 · 0 个步骤/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "已连接，继续" }));
    expect(resume).toHaveBeenCalledWith("run-browser");

    rerender(<WorkspaceShell project={project} locale="zh-CN" onLocaleChange={() => undefined} runStarted activeRunId="run-browser" onResumeAgentRunV4={resume} agentRunEventsV4={[...disconnected,
      { ...base, sequence: 3, occurred_at: "2026-08-17T00:00:02Z", event: { kind: "tool_finished" as const, outcome: { call_id: "call-search", tool_id: "web_search", succeeded: true, model_content: "searched", data: { tab_id: 10 }, provenance: [] } } },
    ]} />);
    expect(screen.getByText(/运行中 · 1 个步骤/)).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "已连接，继续" })).not.toBeInTheDocument();
  });

  it("keeps a terminal status when a tab-cleanup prompt follows it", () => {
    const closeTabs = vi.fn();
    const base = { schema_version: 4 as const, run_id: "run-cleanup", project_id: project.id, conversation_id: "conversation-1", previous_hash: "", event_hash: "hash" };
    render(<WorkspaceShell project={project} locale="zh-CN" onLocaleChange={() => undefined} runStarted activeRunId="run-cleanup" onCloseBrowserRunTabsV4={closeTabs} agentRunEventsV4={[
      { ...base, sequence: 1, occurred_at: "2026-08-17T00:00:00Z", event: { kind: "run_completed" as const } },
      { ...base, sequence: 2, occurred_at: "2026-08-17T00:00:01Z", event: { kind: "browser_tab_cleanup_required" as const, sessions: ["workspace" as const], tabs: [{ session: "workspace" as const, tab_id: 10, run_id: "run-cleanup", title: "Paper", origin: "https://example.org", created_by_run: true }], message: "close run tabs" } },
    ]} />);
    expect(screen.getByText(/已完成 · 0 个步骤/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "关闭本轮标签" }));
    expect(closeTabs).toHaveBeenCalledWith("run-cleanup", ["workspace"]);
  });

  it("pauses the same run for CAPTCHA intervention without claiming it was solved", () => {
    const resume = vi.fn();
    render(<WorkspaceShell project={project} locale="zh-CN" onLocaleChange={() => undefined} runStarted activeRunId="run-captcha" onResumeAgentRunV4={resume} agentRunEventsV4={[{
      schema_version: 4, run_id: "run-captcha", project_id: project.id, conversation_id: "conversation-1", sequence: 1, occurred_at: "2026-08-17T00:00:00Z", previous_hash: "", event_hash: "hash", event: { kind: "browser_human_intervention_required", session: "workspace", reason: "captcha_detected", message: "CAPTCHA detected" },
    }]} />);
    expect(screen.getByText(/等待人工处理浏览器 · 0 个步骤/)).toBeInTheDocument();
    expect(screen.getByText(/不会自动求解 CAPTCHA/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "已人工处理，继续" }));
    expect(resume).toHaveBeenCalledWith("run-captcha");
  });

  it("requires evidence before resolving an uncertain V4 dispatch", () => {
    const resolve = vi.fn();
    render(<WorkspaceShell project={project} locale="zh-CN" onLocaleChange={() => undefined} runStarted activeRunId="run-uncertain" onResolveUncertainV4={resolve} agentRunEventsV4={[{
      schema_version: 4, run_id: "run-uncertain", project_id: project.id, conversation_id: "conversation-1", sequence: 1, occurred_at: "2026-08-17T00:00:00Z", previous_hash: "", event_hash: "hash", event: { kind: "tool_dispatch_uncertain", call_id: "call-1", tool_id: "runtime.execute" },
    }]} />);
    const save = screen.getByRole("button", { name: "保存证据并继续" });
    expect(save).toBeDisabled();
    fireEvent.change(screen.getByRole("textbox", { name: "核验证据" }), { target: { value: "远端输出文件不存在" } });
    fireEvent.click(save);
    expect(resolve).toHaveBeenCalledWith("run-uncertain", "call-1", "side_effect_not_observed", "远端输出文件不存在");
  });

  it("offers resume for a context byte-budget pause", () => {
    const resume = vi.fn();
    render(<WorkspaceShell project={project} locale="zh-CN" onLocaleChange={() => undefined} runStarted activeRunId="run-context" onResumeAgentRunV4={resume} agentRunEventsV4={[{
      schema_version: 4, run_id: "run-context", project_id: project.id, conversation_id: "conversation-1", sequence: 1, occurred_at: "2026-08-17T00:00:00Z", previous_hash: "", event_hash: "hash", event: { kind: "run_needs_attention", message: "run needs attention: model context exceeds byte budget (268851 > 262144); original run and evidence retained" },
    }]} />);
    fireEvent.click(screen.getByRole("button", { name: "继续运行" }));
    expect(resume).toHaveBeenCalledWith("run-context");
  });

  it("shows legacy MCP uncertainty as failure without a side-effect form", () => {
    render(<WorkspaceShell project={project} locale="zh-CN" onLocaleChange={() => undefined} runStarted activeRunId="run-mcp-error" onResolveUncertainV4={vi.fn()} agentRunEventsV4={[{
      schema_version: 4, run_id: "run-mcp-error", project_id: project.id, conversation_id: "conversation-1", sequence: 1, occurred_at: "2026-08-17T00:00:00Z", previous_hash: "", event_hash: "hash", event: { kind: "tool_dispatch_uncertain", call_id: "call-1", tool_id: "use_mcp_tool" },
    }, { schema_version: 4, run_id: "run-mcp-error", project_id: project.id, conversation_id: "conversation-1", sequence: 2, occurred_at: "2026-08-17T00:00:01Z", previous_hash: "hash", event_hash: "hash2", event: { kind: "run_needs_attention", message: "MCP error: -32602: query exceeds limit" } }]} />);
    expect(screen.queryByRole("textbox", { name: "核验证据" })).not.toBeInTheDocument();
    expect(screen.queryByText("工具状态不确定")).not.toBeInTheDocument();
    expect(screen.getByText("工具调用失败")).toBeInTheDocument();
    expect(screen.getByText(/失败 · 1 个步骤/)).toBeInTheDocument();
  });

  it("renders user and assistant messages as safe GFM markdown", () => {
    render(<WorkspaceShell project={project} locale="en-US" onLocaleChange={() => undefined} messages={[
      { id: "user-md", role: "user", markdown: "**检查**矩阵" },
      { id: "assistant-md", role: "assistant", markdown: "## Results\n\n| gene | status |\n| --- | --- |\n| CD3D | pass |" },
    ]} />);
    expect(screen.getByText("检查")).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Results" })).toBeInTheDocument();
    expect(screen.getByRole("cell", { name: "CD3D" })).toBeInTheDocument();
  });

  it("opens the active trajectory while omitting persistence placeholder text", () => {
    const base = { schema_version: 4 as const, run_id: "run-technical", project_id: project.id, conversation_id: "conversation-1", previous_hash: "", event_hash: "hash" };
    render(<WorkspaceShell project={project} locale="zh-CN" runStarted activeRunId="run-technical" onLocaleChange={() => undefined} agentRunEventsV4={[
      { ...base, sequence: 1, occurred_at: "2026-08-17T00:00:00Z", event: { kind: "run_spec_frozen" as const, approval_hash: "a", spec_hash: "b" } },
      { ...base, sequence: 2, occurred_at: "2026-08-17T00:00:01Z", event: { kind: "tool_dispatch_started" as const, call_id: "call-1", tool_id: "runtime.execute", effect: "runtime", idempotency_key: "key" } },
    ]} />);
    expect(screen.getByText("执行过程").closest("details")).toHaveAttribute("open");
    expect(screen.queryByText(/状态已写入可验证事件链/)).not.toBeInTheDocument();
  });

  it("keeps agent.complete internal and shows a clean verification status", () => {
    const base = { schema_version: 4 as const, run_id: "run-completing", project_id: project.id, conversation_id: "conversation-1", previous_hash: "", event_hash: "hash" };
    render(<WorkspaceShell project={project} locale="zh-CN" runStarted activeRunId="run-completing" onLocaleChange={() => undefined} agentRunEventsV4={[
      { ...base, sequence: 1, occurred_at: "2026-08-17T00:00:00Z", event: { kind: "tool_requested" as const, call: { call_id: "complete-1", tool_id: "agent.complete", arguments: { schema_version: 4, summary: "done", answer_markdown: "## 不应显示在工具卡片中", criteria: [] } } } },
    ]} />);
    expect(screen.getByText("正在核验最终结果…")).toBeInTheDocument();
    expect(screen.getByText(/运行中 · 0 个步骤/)).toBeInTheDocument();
    expect(screen.queryByText("agent.complete")).not.toBeInTheDocument();
    expect(screen.queryByText("不应显示在工具卡片中")).not.toBeInTheDocument();
  });

  it("renders tool calls as human-readable collapsed steps", () => {
    const base = { schema_version: 4 as const, run_id: "run-readable-tool", project_id: project.id, conversation_id: "conversation-1", previous_hash: "", event_hash: "hash" };
    render(<WorkspaceShell project={project} locale="zh-CN" runStarted activeRunId="run-readable-tool" onLocaleChange={() => undefined} agentRunEventsV4={[
      { ...base, sequence: 1, occurred_at: "2026-08-17T00:00:00Z", event: { kind: "tool_requested" as const, call: { call_id: "read-1", tool_id: "project.read", arguments: { path: "results/audit.md" } } } },
      { ...base, sequence: 2, occurred_at: "2026-08-17T00:00:01Z", event: { kind: "tool_finished" as const, outcome: { call_id: "read-1", tool_id: "project.read", succeeded: true, model_content: "large internal report", data: null, provenance: [] } } },
    ]} />);
    fireEvent.click(screen.getByText("执行过程"));
    const step = screen.getByTitle("读取项目文件").closest("details");
    expect(step).not.toHaveAttribute("open");
    expect(step).toHaveTextContent("results/audit.md");
    expect(screen.getByText(/运行中 · 1 个步骤/)).toBeInTheDocument();
  });

  it("merges tool request, dispatch, finish, and reuse events into one detail", () => {
    const base = { schema_version: 4 as const, run_id: "run-tools", project_id: project.id, conversation_id: "conversation-1", previous_hash: "", event_hash: "hash" };
    render(<WorkspaceShell project={project} locale="zh-CN" runStarted activeRunId="run-tools" onLocaleChange={() => undefined} agentRunEventsV4={[
      { ...base, sequence: 1, occurred_at: "2026-08-17T00:00:00Z", event: { kind: "tool_requested" as const, call: { call_id: "call-1", tool_id: "runtime.execute", arguments: { code: "print(1)", secret: "hidden" } } } },
      { ...base, sequence: 2, occurred_at: "2026-08-17T00:00:01Z", event: { kind: "tool_dispatch_started" as const, call_id: "call-1", tool_id: "runtime.execute", effect: "runtime", idempotency_key: "key" } },
      { ...base, sequence: 3, occurred_at: "2026-08-17T00:00:02Z", event: { kind: "tool_finished" as const, outcome: { call_id: "call-1", tool_id: "runtime.execute", succeeded: true, model_content: "QC complete", data: null, provenance: [] } } },
      { ...base, sequence: 4, occurred_at: "2026-08-17T00:00:03Z", event: { kind: "tool_outcome_reused" as const, idempotency_key: "key", outcome: { call_id: "call-1", tool_id: "runtime.execute", succeeded: true, model_content: "QC complete", data: null, provenance: [] } } },
      { ...base, sequence: 5, occurred_at: "2026-08-17T00:00:04Z", event: { kind: "tool_requested" as const, call: { call_id: "call-2", tool_id: "runtime.execute", arguments: {} } } },
      { ...base, sequence: 6, occurred_at: "2026-08-17T00:00:05Z", event: { kind: "tool_finished" as const, outcome: { call_id: "call-2", tool_id: "runtime.execute", succeeded: true, model_content: "second result", data: null, provenance: [] } } },
      { ...base, sequence: 7, occurred_at: "2026-08-17T00:00:06Z", event: { kind: "tool_requested" as const, call: { call_id: "call-3", tool_id: "runtime.execute", arguments: {} } } },
      { ...base, sequence: 8, occurred_at: "2026-08-17T00:00:07Z", event: { kind: "tool_finished" as const, outcome: { call_id: "call-3", tool_id: "runtime.execute", succeeded: false, model_content: "failed result", data: null, provenance: [] } } },
    ]} />);
    fireEvent.click(screen.getByText("执行过程"));
    expect(screen.getAllByText(/runtime\.execute/)).toHaveLength(3);
    expect(document.querySelector(".v4-tool-status.reused")).toBeInTheDocument();
    expect(document.querySelector(".v4-tool-status.succeeded")).toBeInTheDocument();
    expect(document.querySelector(".v4-tool-status.failed")).toBeInTheDocument();
    expect(document.querySelector(".v4-tool-status.succeeded")).toBeInTheDocument();
    expect(screen.getByText("QC complete")).toBeInTheDocument();
    expect(screen.getByText(/print\(1\)/)).toBeInTheDocument();
    expect(screen.getByText(/\[REDACTED\]/)).toBeInTheDocument();
    expect(screen.queryByText(/hidden/)).not.toBeInTheDocument();
  });

  it("auto-expands blocked runs while keeping the interaction card available", () => {
    const base = { schema_version: 4 as const, run_id: "run-blocked", project_id: project.id, conversation_id: "conversation-1", previous_hash: "", event_hash: "hash" };
    const request = { approval_id: "approval-blocked", call: { call_id: "call-1", tool_id: "runtime.execute", arguments: {} }, effect: "runtime" as const, reason: "需要批准", call_hash: "c".repeat(64) };
    render(<WorkspaceShell project={project} locale="zh-CN" runStarted activeRunId="run-blocked" onLocaleChange={() => undefined} onDecideToolApprovalV4={vi.fn()} agentRunEventsV4={[{ ...base, sequence: 1, occurred_at: "2026-08-17T00:00:00Z", event: { kind: "tool_approval_requested" as const, request } }]} />);
    expect(screen.getByText("执行过程").closest("details")).toHaveAttribute("open");
    expect(screen.getByRole("button", { name: "批准并继续" })).toBeEnabled();
  });

  it("disables the composer for an active run but leaves approval controls enabled", () => {
    const base = { schema_version: 4 as const, run_id: "run-composer-lock", project_id: project.id, conversation_id: "conversation-1", previous_hash: "", event_hash: "hash" };
    const request = { approval_id: "approval-composer", call: { call_id: "call-1", tool_id: "runtime.execute", arguments: {} }, effect: "runtime" as const, reason: "需要批准", call_hash: "c".repeat(64) };
    render(<WorkspaceShell project={project} locale="zh-CN" onLocaleChange={() => undefined} onSend={vi.fn()} runStarted activeRunId="run-composer-lock" agentRunEventsV4={[{ ...base, sequence: 1, occurred_at: "2026-08-17T00:00:00Z", event: { kind: "tool_approval_requested" as const, request } }]} onDecideToolApprovalV4={vi.fn()} />);
    expect(screen.getByRole("textbox", { name: /描述研究目标/ })).toBeDisabled();
    expect(screen.getByRole("button", { name: "执行中…" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "批准并继续" })).toBeEnabled();
  });

  it("does not submit the composer twice while the first request is pending", async () => {
    let release!: (accepted: boolean) => void;
    const onSend = vi.fn(() => new Promise<boolean>((resolve) => { release = resolve; }));
    render(<WorkspaceShell project={project} locale="zh-CN" onLocaleChange={() => undefined} onSend={onSend}
      computeBackendId="local" computeBackends={[{ descriptor: { schema_version: 4, backend_id: "local", kind: "local", isolation: "process", available: true, supports_python: true, supports_r: false, supports_network_policy: false }, selectable: true, reason: null, python_status: "available", r_status: "unavailable", resolved_image_id: null }]} />);
    const composer = screen.getByRole("textbox", { name: /描述研究目标/ });
    fireEvent.change(composer, { target: { value: "检查矩阵" } });
    const send = screen.getByRole("button", { name: "发送" });
    fireEvent.click(send);
    fireEvent.click(send);
    expect(onSend).toHaveBeenCalledTimes(1);
    release(true);
    await waitFor(() => expect(composer).toBeEnabled());
  });

  it("guards plan approval and resume actions against double clicks", async () => {
    let releaseApproval!: () => void;
    const onApprovePlan = vi.fn(() => new Promise<void>((resolve) => { releaseApproval = resolve; }));
    const plan = { schema_version: 4 as const, objective: "执行 QC", steps: ["检查输入"], completion_criteria: ["报告完成"], requested_capabilities: [] };
    const { rerender } = render(<WorkspaceShell project={project} locale="zh-CN" onLocaleChange={() => undefined}
      v4Plan={{ run_id: "run-plan-guard", status: "awaiting_approval", plan, plan_hash: "plan", compute_selection: null, approval_hash: "approval" }} onApprovePlan={onApprovePlan} />);
    const approve = screen.getByRole("button", { name: "批准并运行" });
    fireEvent.click(approve);
    fireEvent.click(approve);
    expect(onApprovePlan).toHaveBeenCalledTimes(1);
    expect(approve).toBeDisabled();
    releaseApproval();
    await waitFor(() => expect(screen.getByRole("button", { name: "批准并运行" })).toBeEnabled());

    let releaseResume!: () => void;
    const onResume = vi.fn(() => new Promise<void>((resolve) => { releaseResume = resolve; }));
    const base = { schema_version: 4 as const, run_id: "run-resume-guard", project_id: project.id, conversation_id: "conversation-1", previous_hash: "", event_hash: "hash" };
    const failed = [{ ...base, sequence: 1, occurred_at: "2026-08-17T00:00:01Z", event: { kind: "run_created" as const, mode: "execute" as const } }, { ...base, sequence: 2, occurred_at: "2026-08-17T00:00:02Z", event: { kind: "run_failed" as const, message: "temporary failure" } }];
    rerender(<WorkspaceShell project={project} locale="zh-CN" onLocaleChange={() => undefined} agentRunEventsV4={failed} onResumeAgentRunV4={onResume} />);
    const resume = screen.getByRole("button", { name: "继续运行" });
    fireEvent.click(resume);
    fireEvent.click(resume);
    expect(onResume).toHaveBeenCalledTimes(1);
    expect(resume).toBeDisabled();
    releaseResume();
    await waitFor(() => expect(screen.getByRole("button", { name: "继续运行" })).toBeEnabled());
  });

  it("locks a pending Plan conversation even before execution starts", () => {
    const plan = { schema_version: 4 as const, objective: "审核 QC", steps: ["检查输入"], completion_criteria: ["报告完成"], requested_capabilities: [] };
    const revision = { id: "revision-pending", project_id: project.id, conversation_id: "conversation-1", run_id: "run-pending", revision: 4, plan, markdown: "# 审核 QC", plan_hash: "plan-hash", status: "pending" as const, feedback: null, created_at: "2026-08-20T00:00:00Z", updated_at: "2026-08-20T00:00:00Z" };
    render(<WorkspaceShell project={project} locale="zh-CN" onLocaleChange={() => undefined} onSend={vi.fn()}
      agentMode="plan" conversationLocked latestPlanRevision={revision}
      v4Plan={{ run_id: revision.run_id, status: "awaiting_approval", plan, plan_hash: revision.plan_hash, compute_selection: null, approval_hash: "approval", plan_revision: 4, session_mode: "plan" }}
      onApprovePlan={vi.fn()} onRequestPlanRevision={vi.fn()} onCancelRun={vi.fn()} />);

    expect(screen.getByRole("textbox", { name: /描述研究目标/ })).toBeDisabled();
    expect(screen.getByRole("button", { name: "批准并运行" })).toBeEnabled();
    expect(screen.getByRole("button", { name: "请求修改" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "取消计划" })).toBeEnabled();
  });

  it("submits revision feedback once and disables every competing plan action", async () => {
    let release!: () => void;
    const requestRevision = vi.fn(() => new Promise<void>((resolve) => { release = resolve; }));
    const plan = { schema_version: 4 as const, objective: "修改 QC", steps: ["检查输入"], completion_criteria: ["报告完成"], requested_capabilities: [] };
    const revision = { id: "revision-feedback", project_id: project.id, conversation_id: "conversation-1", run_id: "run-feedback", revision: 5, plan, markdown: "# 修改 QC", plan_hash: "plan-hash", status: "pending" as const, feedback: null, created_at: "2026-08-20T00:00:00Z", updated_at: "2026-08-20T00:00:00Z" };
    render(<WorkspaceShell project={project} locale="zh-CN" onLocaleChange={() => undefined}
      agentMode="plan" conversationLocked latestPlanRevision={revision}
      v4Plan={{ run_id: revision.run_id, status: "awaiting_approval", plan, plan_hash: revision.plan_hash, compute_selection: null, approval_hash: "approval", plan_revision: 5, session_mode: "plan" }}
      onApprovePlan={vi.fn()} onRequestPlanRevision={requestRevision} onCancelRun={vi.fn()} />);
    const feedback = screen.getByRole("textbox", { name: "计划修改意见" });
    fireEvent.change(feedback, { target: { value: "补充批次效应检查" } });
    const request = screen.getByRole("button", { name: "请求修改" });
    fireEvent.click(request);
    fireEvent.click(request);

    expect(requestRevision).toHaveBeenCalledTimes(1);
    expect(requestRevision).toHaveBeenCalledWith("补充批次效应检查");
    expect(screen.getByRole("button", { name: "批准并运行" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "取消计划" })).toBeDisabled();
    release();
    await waitFor(() => expect(feedback).toHaveValue(""));
    expect(screen.getByRole("button", { name: "批准并运行" })).toBeEnabled();
    expect(screen.getByRole("button", { name: "取消计划" })).toBeEnabled();
  });

  it("guards answer and uncertain-dispatch recovery actions while preserving their controls during a run", async () => {
    let releaseAnswer!: () => void;
    const onAnswer = vi.fn(() => new Promise<void>((resolve) => { releaseAnswer = resolve; }));
    const base = { schema_version: 4 as const, run_id: "run-input-guard", project_id: project.id, conversation_id: "conversation-1", previous_hash: "", event_hash: "hash" };
    const requested = { ...base, sequence: 1, occurred_at: "2026-08-17T00:00:01Z", event: { kind: "input_requested" as const, question_id: "species", question: "物种？" } };
    const { rerender } = render(<WorkspaceShell project={project} locale="zh-CN" onLocaleChange={() => undefined} runStarted activeRunId="run-input-guard" agentRunEventsV4={[requested]} onAnswerAgentQuestionV4={onAnswer} />);
    fireEvent.change(screen.getByRole("textbox", { name: "回答 V4 问题" }), { target: { value: "人" } });
    const answer = screen.getByRole("button", { name: "回答并恢复" });
    fireEvent.click(answer);
    fireEvent.click(answer);
    expect(onAnswer).toHaveBeenCalledTimes(1);
    expect(answer).toBeDisabled();
    releaseAnswer();
    await waitFor(() => expect(screen.getByRole("button", { name: "回答并恢复" })).toBeEnabled());

    let releaseResolve!: () => void;
    const onResolve = vi.fn(() => new Promise<void>((resolve) => { releaseResolve = resolve; }));
    const uncertain = { ...base, run_id: "run-uncertain-guard", sequence: 1, occurred_at: "2026-08-17T00:00:01Z", event: { kind: "tool_dispatch_uncertain" as const, call_id: "call-1", tool_id: "runtime.execute" } };
    rerender(<WorkspaceShell project={project} locale="zh-CN" onLocaleChange={() => undefined} runStarted activeRunId="run-uncertain-guard" agentRunEventsV4={[uncertain]} onResolveUncertainV4={onResolve} />);
    fireEvent.click(screen.getByText("执行过程"));
    fireEvent.change(screen.getByRole("textbox", { name: "核验证据" }), { target: { value: "已检查" } });
    const resolve = screen.getByRole("button", { name: "保存证据并继续" });
    fireEvent.click(resolve);
    fireEvent.click(resolve);
    expect(onResolve).toHaveBeenCalledTimes(1);
    expect(resolve).toBeDisabled();
    releaseResolve();
    await waitFor(() => expect(screen.getByRole("button", { name: "保存证据并继续" })).toBeEnabled());
  });
});

it("updates one live progress row then replaces it with the committed message", () => {
  const props = { project, locale: "en-US" as const, onLocaleChange: () => undefined, runStarted: true, activeRunId: "live" };
  const base = { schema_version: 4 as const, run_id: "live", project_id: project.id, conversation_id: "c", previous_hash: "", event_hash: "h", occurred_at: "2026-09-11T00:00:00Z" };
  const events: import("../../types").AgentRunEventV4[] = [{ ...base, sequence: 1, event: { kind: "run_created", mode: "execute" } }];
  const { rerender } = render(<WorkspaceShell {...props} agentRunEventsV4={events} agentTextPreview={{ run_id: "live", text: "Searching" }} />);
  const row = screen.getByRole("article", { name: "Live model output" });
  rerender(<WorkspaceShell {...props} agentRunEventsV4={events} agentTextPreview={{ run_id: "live", text: "Searching literature" }} />);
  expect(screen.getByRole("article", { name: "Live model output" })).toBe(row);
  expect(row).toHaveTextContent("Searching literature");
  rerender(<WorkspaceShell {...props} agentRunEventsV4={[...events, { ...base, sequence: 2, event: { kind: "model_text", text: "Searching literature" } }]} agentTextPreview={null} />);
  expect(screen.queryByRole("article", { name: "Live model output" })).not.toBeInTheDocument();
  expect(screen.getAllByRole("article", { name: "Model output" })).toHaveLength(1);
  rerender(<WorkspaceShell {...props} agentRunEventsV4={events} agentTextPreview={{ run_id: "other", text: "wrong run" }} />);
  expect(screen.queryByText("wrong run")).not.toBeInTheDocument();
});
