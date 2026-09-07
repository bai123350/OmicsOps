import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import DesktopApp from "./DesktopApp";
import * as api from "./tauri-api";
import type { AgentRunEventV4, ConversationAgentStateV4, ExecutionPlanV4, KernelEvent, ProposedPlanRevisionV4, RunSummaryV4, SyncEntry } from "./types";

beforeEach(() => vi.restoreAllMocks());

const stateProject = { id: "project-state", name: "状态测试项目", description: "", local_root: "E:/Science/state", remote_root: null, connection_id: null, template: "single_cell_rna_seq" as const, status: "running" as const, ollama_only: false, created_at: "2026-08-11T00:00:00Z", updated_at: "2026-08-11T00:00:00Z" };
const stateConversations = [
  { id: "conversation-agent", project_id: stateProject.id, title: "Agent 会话", status: "idle" as const, model_profile_id: "model-state", created_at: "2026-08-11T00:00:00Z", updated_at: "2026-08-11T00:00:00Z" },
  { id: "conversation-plan", project_id: stateProject.id, title: "Plan 会话", status: "idle" as const, model_profile_id: "model-state", created_at: "2026-08-12T00:00:00Z", updated_at: "2026-08-12T00:00:00Z" },
];
const stateModel = { id: "model-state", label: "State model", provider: "ollama" as const, base_url: "http://localhost:11434", model: "state", credential_reference: null, supports_tools: true, supports_vision: false };
const stateBackend = { descriptor: { schema_version: 4 as const, backend_id: "local", kind: "local" as const, isolation: "process" as const, available: true, supports_python: true, supports_r: false, supports_network_policy: false }, selectable: true, reason: null, python_status: "available" as const, r_status: "unavailable" as const, resolved_image_id: null };
const statePlan: ExecutionPlanV4 = { schema_version: 4, objective: "执行状态测试", steps: ["检查输入"], completion_criteria: ["报告完成"], requested_capabilities: [] };

function stateRevision(conversationId: string, runId = "run-plan", status: ProposedPlanRevisionV4["status"] = "pending", revision = 2): ProposedPlanRevisionV4 {
  return { id: `revision-${runId}-${revision}`, project_id: stateProject.id, conversation_id: conversationId, run_id: runId, revision, plan: statePlan, markdown: "状态测试计划", plan_hash: `hash-${runId}-${revision}`, status, feedback: null, created_at: "2026-08-20T00:00:00Z", updated_at: "2026-08-20T00:00:00Z" };
}

function stateRun(runId = "run-plan", status = "awaiting_approval", plan: ExecutionPlanV4 | null = statePlan, approvalHash: string | null = "approval-state", planRevision: number | null = 2): RunSummaryV4 {
  return { run_id: runId, status, plan, plan_hash: plan ? `hash-${runId}-${planRevision ?? 1}` : null, compute_selection: null, approval_hash: approvalHash, plan_revision: planRevision, session_mode: "plan" };
}

function stateSnapshot(conversationId: string, overrides: Partial<ConversationAgentStateV4> = {}): ConversationAgentStateV4 {
  return { project_id: stateProject.id, conversation_id: conversationId, mode: "agent", locked: false, latest_plan_revision: null, latest_run: null, ...overrides };
}

function deferred<T>() {
  let resolve!: (value: T | PromiseLike<T>) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((resolvePromise, rejectPromise) => { resolve = resolvePromise; reject = rejectPromise; });
  return { promise, resolve, reject };
}

function agentEvent(conversationId: string, runId: string, sequence: number, event: AgentRunEventV4["event"]): AgentRunEventV4 {
  return {
    schema_version: 4,
    run_id: runId,
    project_id: stateProject.id,
    conversation_id: conversationId,
    sequence,
    occurred_at: `2026-08-20T00:00:${String(sequence).padStart(2, "0")}Z`,
    previous_hash: "",
    event_hash: `${runId}-${sequence}`,
    event,
  };
}

function setupConversationStateHarness() {
  vi.spyOn(api, "listProjects").mockResolvedValue([stateProject]);
  vi.spyOn(api, "listConversations").mockResolvedValue(stateConversations);
  vi.spyOn(api, "listMessages").mockResolvedValue([]);
  vi.spyOn(api, "listModelProfiles").mockResolvedValue([stateModel]);
  vi.spyOn(api, "agentV4ComputeBackends").mockResolvedValue([stateBackend]);
  vi.spyOn(api, "agentV4EventsForConversation").mockResolvedValue([]);
  vi.spyOn(api, "agentV4Events").mockResolvedValue([]);
  return { stateSpy: vi.spyOn(api, "agentV4ConversationState") };
}

describe("DesktopApp", () => {
  it("loads all models from the current API and saves the selected model before allowing sends", async () => {
    const { stateSpy } = setupConversationStateHarness();
    stateSpy.mockImplementation(async (_projectId, conversationId) => stateSnapshot(conversationId));
    const profile = { ...stateModel, label: "OpenAI-compatible", model: "research-model-a" };
    vi.mocked(api.listModelProfiles).mockResolvedValue([profile]);
    const listModels = vi.spyOn(api, "listModelProfileModels").mockResolvedValue(["research-model-a", "research-model-b"]);
    const saved = deferred<Awaited<ReturnType<typeof api.saveModelProfile>>>();
    const save = vi.spyOn(api, "saveModelProfile").mockReturnValue(saved.promise);
    render(<DesktopApp />);

    await screen.findByText(/Agent 模式：LOCAL/);
    const selector = await screen.findByRole("button", { name: "选择模型" });
    await waitFor(() => expect(selector).toBeEnabled());
    expect(selector).toHaveTextContent("research-model-a");
    fireEvent.click(selector);
    fireEvent.click(await screen.findByRole("menuitemradio", { name: "research-model-b" }));
    expect(listModels).toHaveBeenCalledWith(profile.id);
    expect(save).toHaveBeenCalledWith({ id: profile.id, label: profile.label, provider: profile.provider, base_url: profile.base_url, model: "research-model-b" });
    expect(selector).toHaveTextContent("research-model-a");
    expect(screen.getByRole("textbox", { name: /描述研究目标/ })).toBeDisabled();
    await act(async () => saved.resolve({ ...profile, model: "research-model-b" }));
    await waitFor(() => expect(selector).toHaveTextContent("research-model-b"));
    expect(screen.getByRole("textbox", { name: /描述研究目标/ })).toBeEnabled();
  });
  it("keeps the current model and unlocks the composer when saving an API model fails", async () => {
    const { stateSpy } = setupConversationStateHarness();
    stateSpy.mockImplementation(async (_projectId, conversationId) => stateSnapshot(conversationId));
    vi.spyOn(api, "listModelProfileModels").mockResolvedValue([stateModel.model, "other-api-model"]);
    vi.spyOn(api, "saveModelProfile").mockRejectedValue(new Error("sensitive transport detail"));
    render(<DesktopApp />);
    await screen.findByText(/Agent 模式：LOCAL/);
    const selector = screen.getByRole("button", { name: "选择模型" });
    fireEvent.click(selector);
    fireEvent.click(await screen.findByRole("menuitemradio", { name: "other-api-model" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("模型切换失败");
    expect(selector).toHaveTextContent(stateModel.model);
    expect(screen.getByRole("textbox", { name: /描述研究目标/ })).toBeEnabled();
    expect(screen.queryByText("sensitive transport detail")).not.toBeInTheDocument();
  });
  it("opens the local-first project library when no project exists", async () => {
    vi.spyOn(api, "listProjects").mockResolvedValue([]);
    render(<DesktopApp />);

    expect(await screen.findByRole("heading", { name: "生命科学项目" })).toBeInTheDocument();
    expect(screen.getByText("单细胞 RNA 测序")).toBeInTheDocument();
    expect(screen.getByText("文献综述")).toBeInTheDocument();
  });

  it("opens an existing project in the three-pane research workspace", async () => {
    vi.spyOn(api, "listProjects").mockResolvedValue([{ id: "project-1", name: "PBMC 图谱", description: "", local_root: "E:/Science/pbmc", remote_root: null, connection_id: null, template: "single_cell_rna_seq", status: "running", ollama_only: false, created_at: "2026-08-11T00:00:00Z", updated_at: "2026-08-11T00:00:00Z" }]);
    render(<DesktopApp />);

    expect(await screen.findByRole("main", { name: "科研对话" })).toBeInTheDocument();
    expect(screen.getByRole("complementary", { name: "项目上下文" })).toBeInTheDocument();
  });

  it("starts a direct V4 agent run instead of planning after an ordinary send", async () => {
    const project = { id: "project-1", name: "PBMC 图谱", description: "", local_root: "E:/Science/pbmc", remote_root: null, connection_id: null, template: "single_cell_rna_seq" as const, status: "running" as const, ollama_only: false, created_at: "2026-08-11T00:00:00Z", updated_at: "2026-08-11T00:00:00Z" };
    const conversation = { id: "conversation-1", project_id: project.id, title: "普通问答", status: "idle" as const, model_profile_id: "model-1", created_at: "2026-08-11T00:00:00Z", updated_at: "2026-08-11T00:00:00Z" };
    const model = { id: "model-1", label: "Test model", provider: "ollama" as const, base_url: "http://localhost:11434", model: "test", credential_reference: null, supports_tools: true, supports_vision: false };
    vi.spyOn(api, "listProjects").mockResolvedValue([project]);
    vi.spyOn(api, "listConversations").mockResolvedValue([conversation]);
    vi.spyOn(api, "listMessages").mockResolvedValue([]);
    vi.spyOn(api, "listModelProfiles").mockResolvedValue([model]);
    vi.spyOn(api, "agentV4ComputeBackends").mockResolvedValue([{ descriptor: { schema_version: 4, backend_id: "local", kind: "local", isolation: "process", available: true, supports_python: true, supports_r: false, supports_network_policy: false }, selectable: true, reason: null, python_status: "available", r_status: "unavailable", resolved_image_id: null }]);
    vi.spyOn(api, "submitMessage").mockResolvedValue({ id: "message-1", project_id: project.id, conversation_id: conversation.id, sequence: 1, role: "user", markdown: "先解释一下这个矩阵格式", created_at: "2026-08-21T00:00:00Z" });
    const startDirect = vi.spyOn(api, "agentV4StartDirect").mockResolvedValue({ run_id: "run-direct", status: "running", plan: null, plan_hash: null, compute_selection: { schema_version: 4, backend_id: "local", backend_kind: "local", autonomy_mode: "supervised", environment: "system", network_policy: "host_inherited", container_image: null }, approval_hash: null });
    const startPlanning = vi.spyOn(api, "agentV4StartPlanning");
    const cancelRun = vi.spyOn(api, "agentV4Cancel").mockResolvedValue();
    vi.spyOn(api, "agentV4Events").mockResolvedValue([]);

    render(<DesktopApp />);
    const composer = await screen.findByRole("textbox", { name: /描述研究目标/ });
    await screen.findByText(/Agent 模式：LOCAL/);
    fireEvent.change(composer, { target: { value: "先解释一下这个矩阵格式" } });
    fireEvent.click(screen.getByRole("button", { name: "发送" }));

    await waitFor(() => expect(startDirect).toHaveBeenCalledWith(expect.objectContaining({ objective: "先解释一下这个矩阵格式" })));
    expect(startPlanning).not.toHaveBeenCalled();
    expect(await screen.findByRole("button", { name: "添加上下文或选择模式" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "终止运行" }));
    await waitFor(() => expect(cancelRun).toHaveBeenCalledWith("run-direct"));
    expect(screen.getByRole("button", { name: "终止运行" })).toBeEnabled();
  });

  it("does not let a previous conversation send clear the new session busy state", async () => {
    const { stateSpy } = setupConversationStateHarness();
    stateSpy.mockImplementation(async (_projectId, conversationId) => stateSnapshot(conversationId));
    const oldSubmit = deferred<Awaited<ReturnType<typeof api.submitMessage>>>();
    const newSubmit = deferred<Awaited<ReturnType<typeof api.submitMessage>>>();
    const submit = vi.spyOn(api, "submitMessage").mockImplementation((request) => request.conversation_id === "conversation-agent" ? oldSubmit.promise : newSubmit.promise);
    vi.spyOn(api, "agentV4StartDirect").mockResolvedValue({ run_id: "new-run", status: "running", plan: null, plan_hash: null, compute_selection: { schema_version: 4, backend_id: "local", backend_kind: "local", autonomy_mode: "supervised", environment: "system", network_policy: "host_inherited", container_image: null }, approval_hash: null });
    vi.spyOn(api, "agentV4Events").mockResolvedValue([]);

    render(<DesktopApp />);
    const firstComposer = await screen.findByRole("textbox", { name: /描述研究目标/ });
    await screen.findByText(/Agent 模式：LOCAL/);
    fireEvent.change(firstComposer, { target: { value: "旧会话请求" } });
    fireEvent.click(screen.getByRole("button", { name: "发送" }));
    await waitFor(() => expect(submit).toHaveBeenCalledWith(expect.objectContaining({ conversation_id: "conversation-agent", markdown: "旧会话请求" })));

    fireEvent.click(screen.getByRole("button", { name: "Plan 会话" }));
    await screen.findByRole("heading", { name: "Plan 会话" });
    await screen.findByText(/Agent 模式：LOCAL/);
    const secondComposer = screen.getByRole("textbox", { name: /描述研究目标/ });
    fireEvent.change(secondComposer, { target: { value: "新会话请求" } });
    fireEvent.click(screen.getByRole("button", { name: "发送" }));
    await waitFor(() => expect(submit).toHaveBeenCalledWith(expect.objectContaining({ conversation_id: "conversation-plan", markdown: "新会话请求" })));

    oldSubmit.reject(new Error("旧会话发送失败"));
    await waitFor(() => expect(screen.getByRole("status")).toHaveTextContent("正在等待模型响应"));
    expect(screen.queryByText("旧会话发送失败")).not.toBeInTheDocument();

    newSubmit.resolve({ id: "new-message", project_id: stateProject.id, conversation_id: "conversation-plan", sequence: 1, role: "user", markdown: "新会话请求", created_at: "2026-08-20T00:00:00Z" });
    await waitFor(() => expect(api.agentV4StartDirect).toHaveBeenCalledWith(expect.objectContaining({ conversation_id: "conversation-plan" })));
  });

  it("surfaces an Agent event subscription failure instead of silently waiting", async () => {
    vi.spyOn(api, "listProjects").mockResolvedValue([{ id: "project-1", name: "PBMC 图谱", description: "", local_root: "E:/Science/pbmc", remote_root: null, connection_id: null, template: "single_cell_rna_seq", status: "running", ollama_only: false, created_at: "2026-08-11T00:00:00Z", updated_at: "2026-08-11T00:00:00Z" }]);
    vi.spyOn(api, "onAgentV4Event").mockRejectedValue(new Error("listen unavailable"));

    render(<DesktopApp />);

    expect(await screen.findByText("Agent V4 event subscription failed: listen unavailable")).toBeInTheDocument();
  });

  it("reconciles persisted Agent events every three seconds when live events are silent", async () => {
    vi.useFakeTimers();
    try {
      const project = { id: "project-1", name: "PBMC 图谱", description: "", local_root: "E:/Science/pbmc", remote_root: null, connection_id: null, template: "single_cell_rna_seq" as const, status: "running" as const, ollama_only: false, created_at: "2026-08-11T00:00:00Z", updated_at: "2026-08-11T00:00:00Z" };
      const conversation = { id: "conversation-1", project_id: project.id, title: "长期运行", status: "running" as const, model_profile_id: "model-1", created_at: "2026-08-11T00:00:00Z", updated_at: "2026-08-11T00:00:00Z" };
      const model = { id: "model-1", label: "Test model", provider: "ollama" as const, base_url: "http://localhost:11434", model: "test", credential_reference: null, supports_tools: true, supports_vision: false };
      const base = { schema_version: 4 as const, run_id: "run-poll", project_id: project.id, conversation_id: conversation.id, previous_hash: "", event_hash: "hash" };
      const started = { ...base, sequence: 1, occurred_at: "2026-08-24T00:00:01Z", event: { kind: "run_created" as const, mode: "execute" as const } };
      const progress = { ...base, sequence: 2, occurred_at: "2026-08-24T00:00:02Z", event: { kind: "model_text" as const, text: "轮询恢复了进度。" } };
      vi.spyOn(api, "listProjects").mockResolvedValue([project]);
      vi.spyOn(api, "listConversations").mockResolvedValue([conversation]);
      vi.spyOn(api, "listMessages").mockResolvedValue([]);
      vi.spyOn(api, "listModelProfiles").mockResolvedValue([model]);
      vi.spyOn(api, "agentV4ComputeBackends").mockResolvedValue([{ descriptor: { schema_version: 4, backend_id: "local", kind: "local", isolation: "process", available: true, supports_python: true, supports_r: false, supports_network_policy: false }, selectable: true, reason: null, python_status: "available", r_status: "unavailable", resolved_image_id: null }]);
      vi.spyOn(api, "agentV4EventsForConversation").mockResolvedValue([started]);
      let persistedReads = 0;
      const readEvents = vi.spyOn(api, "agentV4Events").mockImplementation(async () => {
        persistedReads += 1;
        if (persistedReads === 1) throw new Error("temporary event verification failure");
        return [started, progress];
      });

      render(<DesktopApp />);
      await act(async () => {
        await Promise.resolve();
        await Promise.resolve();
        await Promise.resolve();
        await vi.advanceTimersByTimeAsync(250);
        await Promise.resolve();
        await Promise.resolve();
      });
      expect(screen.getByRole("main", { name: "科研对话" })).toBeInTheDocument();
      expect(screen.getByText("temporary event verification failure")).toBeInTheDocument();
      const readsBeforeInterval = readEvents.mock.calls.length;
      await act(async () => { await vi.advanceTimersByTimeAsync(3_000); });
      expect(readEvents.mock.calls.length).toBeGreaterThan(readsBeforeInterval);
      expect(screen.getByText("轮询恢复了进度。")).toBeInTheDocument();
      expect(screen.queryByText("temporary event verification failure")).not.toBeInTheDocument();
    } finally {
      vi.useRealTimers();
    }
  });

  it("starts planning from the Agent controls Plan mode and continues execution after approval", async () => {
    const project = { id: "project-1", name: "PBMC 图谱", description: "", local_root: "E:/Science/pbmc", remote_root: null, connection_id: null, template: "single_cell_rna_seq" as const, status: "running" as const, ollama_only: false, created_at: "2026-08-11T00:00:00Z", updated_at: "2026-08-11T00:00:00Z" };
    const conversation = { id: "conversation-1", project_id: project.id, title: "分析任务", status: "idle" as const, model_profile_id: "model-1", created_at: "2026-08-11T00:00:00Z", updated_at: "2026-08-11T00:00:00Z" };
    const model = { id: "model-1", label: "Test model", provider: "ollama" as const, base_url: "http://localhost:11434", model: "test", credential_reference: null, supports_tools: true, supports_vision: false };
    const selection = { schema_version: 4 as const, backend_id: "local", backend_kind: "local" as const, autonomy_mode: "supervised" as const, approval_policy: "risk_based" as const, environment: "system", network_policy: "host_inherited" as const, container_image: null };
    const planned = { run_id: "run-v4", status: "awaiting_approval", plan: { schema_version: 4 as const, objective: "执行完整 QC", steps: ["检查输入", "执行 QC"], completion_criteria: ["报告已生成"], requested_capabilities: ["runtime.execute"] }, plan_hash: "plan-hash", compute_selection: selection, approval_hash: "approval-hash" };
    vi.spyOn(api, "listProjects").mockResolvedValue([project]);
    vi.spyOn(api, "listConversations").mockResolvedValue([conversation]);
    vi.spyOn(api, "listMessages").mockResolvedValue([]);
    vi.spyOn(api, "listModelProfiles").mockResolvedValue([model]);
    vi.spyOn(api, "agentV4ComputeBackends").mockResolvedValue([{ descriptor: { schema_version: 4, backend_id: "local", kind: "local", isolation: "process", available: true, supports_python: true, supports_r: false, supports_network_policy: false }, selectable: true, reason: null, python_status: "available", r_status: "unavailable", resolved_image_id: null }]);
    vi.spyOn(api, "submitMessage").mockResolvedValue({ id: "message-1", project_id: project.id, conversation_id: conversation.id, sequence: 1, role: "user", markdown: "执行完整 QC", created_at: "2026-08-21T00:00:00Z" });
    const startPlanning = vi.spyOn(api, "agentV4StartPlanning").mockResolvedValue(planned);
    const approvePlan = vi.spyOn(api, "agentV4ApprovePlan").mockResolvedValue({ ...planned, status: "running" });
    vi.spyOn(api, "agentV4Events").mockResolvedValue([]);
    const pendingRevision: ProposedPlanRevisionV4 = {
      id: "revision-v4-1", project_id: project.id, conversation_id: conversation.id, run_id: "run-v4", revision: 1,
      plan: planned.plan, markdown: "# 执行完整 QC", plan_hash: "plan-hash", status: "pending", feedback: null,
      created_at: "2026-08-21T00:00:00Z", updated_at: "2026-08-21T00:00:00Z",
    };
    const pendingSnapshot: ConversationAgentStateV4 = {
      project_id: project.id, conversation_id: conversation.id, mode: "plan", locked: true,
      latest_plan_revision: pendingRevision, latest_run: { ...planned, plan_revision: 1, session_mode: "plan" },
    };
    vi.spyOn(api, "agentV4ConversationState")
      .mockResolvedValueOnce({ project_id: project.id, conversation_id: conversation.id, mode: "agent", locked: false, latest_plan_revision: null, latest_run: null })
      .mockResolvedValueOnce({ project_id: project.id, conversation_id: conversation.id, mode: "plan", locked: false, latest_plan_revision: null, latest_run: null })
      .mockResolvedValue(pendingSnapshot);
    vi.spyOn(api, "setConversationAgentMode").mockResolvedValue({ project_id: project.id, conversation_id: conversation.id, mode: "plan" });

    render(<DesktopApp />);
    await screen.findByRole("main", { name: "科研对话" });
    await screen.findByText(/Agent 模式：LOCAL/);
    fireEvent.click(screen.getByRole("button", { name: "Agent 权限" }));
    fireEvent.click(screen.getByRole("menuitemcheckbox", { name: "先做计划" }));
    fireEvent.click(screen.getByRole("button", { name: "选择计算后端" }));
    await screen.findByRole("region", { name: "V4 计算后端" });
    await screen.findByRole("radio", { name: /LOCAL/ }, { timeout: 3000 });
    fireEvent.change(screen.getByRole("textbox", { name: /描述研究目标/ }), { target: { value: "执行完整 QC" } });
    fireEvent.click(screen.getByRole("button", { name: "发送" }));

    await waitFor(() => expect(startPlanning).toHaveBeenCalledWith(expect.objectContaining({ objective: "执行完整 QC", compute_selection: selection })));
    fireEvent.click(await screen.findByRole("button", { name: "批准并运行" }));
    await waitFor(() => expect(approvePlan).toHaveBeenCalledWith("run-v4", "approval-hash", 1));
    expect(await screen.findByText(/Agent 模式：LOCAL/)).toBeInTheDocument();
    expect(screen.getByRole("region", { name: "远程 Agent 运行控制" })).toHaveTextContent("远程 Agent 正在运行");
  });

  it("hydrates conversation-scoped Agent and Plan modes when switching sessions", async () => {
    const { stateSpy } = setupConversationStateHarness();
    stateSpy.mockImplementation(async (_projectId, conversationId) => conversationId === "conversation-plan"
      ? stateSnapshot(conversationId, { mode: "plan" })
      : stateSnapshot(conversationId));

    render(<DesktopApp />);
    await screen.findByRole("heading", { name: "Agent 会话" });
    expect(await screen.findByText(/Agent 模式：LOCAL/)).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Plan 会话" }));
    await screen.findByRole("heading", { name: "Plan 会话" });
    expect(await screen.findByText("Plan 模式配置")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Agent 会话" }));
    await screen.findByRole("heading", { name: "Agent 会话" });
    expect(await screen.findByText(/Agent 模式：LOCAL/)).toBeInTheDocument();
    expect(stateSpy).toHaveBeenCalledWith(stateProject.id, "conversation-plan");
  });

  it("keeps initial messages and locks every ordinary action during hydration", async () => {
    const { stateSpy } = setupConversationStateHarness();
    const storedMessage = { id: "message-before-mode", project_id: stateProject.id, conversation_id: "conversation-agent", sequence: 1, role: "user" as const, markdown: "hydrated before mode switch", created_at: "2026-08-20T00:00:00Z" };
    vi.spyOn(api, "listMessages").mockResolvedValue([storedMessage]);
    let releaseInitialState!: () => void;
    stateSpy
      .mockImplementationOnce(() => new Promise((resolve) => {
        releaseInitialState = () => resolve(stateSnapshot("conversation-agent", {
          mode: "plan",
          locked: true,
          latest_plan_revision: stateRevision("conversation-agent"),
          latest_run: stateRun(),
        }));
      }))
      .mockResolvedValue(stateSnapshot("conversation-agent", { mode: "plan" }));
    const setConversationAgentMode = vi.spyOn(api, "setConversationAgentMode").mockResolvedValue({ project_id: stateProject.id, conversation_id: "conversation-agent", mode: "plan" });
    const submitMessage = vi.spyOn(api, "submitMessage");

    render(<DesktopApp />);
    await screen.findByRole("heading", { name: "Agent 会话" });
    fireEvent.click(screen.getByRole("button", { name: "Agent 权限" }));
    const planMode = screen.getByRole("menuitemcheckbox", { name: "先做计划" });
    expect(planMode).toBeDisabled();
    fireEvent.click(planMode);
    expect(setConversationAgentMode).not.toHaveBeenCalled();
    expect(screen.getByRole("textbox", { name: /描述研究目标/ })).toBeDisabled();
    expect(screen.getByRole("button", { name: "删除会话：Agent 会话" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "新建会话" })).toBeDisabled();
    fireEvent.click(screen.getByRole("button", { name: "执行中…" }));
    expect(submitMessage).not.toHaveBeenCalled();

    releaseInitialState();
    expect(await screen.findByText("hydrated before mode switch")).toBeInTheDocument();
    expect(await screen.findByText(/Plan 模式：/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("tab", { name: "Plan" }));
    expect(await screen.findByRole("heading", { name: statePlan.objective })).toBeInTheDocument();
    expect(await screen.findByText("修订 2 · 待审批")).toBeInTheDocument();
  });

  it("merges live messages into a deferred hydration without rolling back sequence", async () => {
    const { stateSpy } = setupConversationStateHarness();
    stateSpy.mockResolvedValue(stateSnapshot("conversation-agent"));
    const storedRead = deferred<Awaited<ReturnType<typeof api.listMessages>>>();
    vi.spyOn(api, "listMessages").mockReturnValue(storedRead.promise);
    let emitConversation!: Parameters<typeof api.onConversationEvent>[0];
    vi.spyOn(api, "onConversationEvent").mockImplementation(async (callback) => {
      emitConversation = callback;
      return () => undefined;
    });
    const submitMessage = vi.spyOn(api, "submitMessage").mockResolvedValue({
      id: "message-after-hydration", project_id: stateProject.id, conversation_id: "conversation-agent",
      sequence: 6, role: "user", markdown: "after hydration", created_at: "2026-08-20T00:00:06Z",
    });
    vi.spyOn(api, "agentV4StartDirect").mockResolvedValue({ run_id: "run-after-hydration", status: "running", plan: null, plan_hash: null, compute_selection: null, approval_hash: null });

    render(<DesktopApp />);
    await screen.findByRole("heading", { name: "Agent 会话" });
    await waitFor(() => expect(emitConversation).toBeDefined());
    await act(async () => emitConversation({
      project_id: stateProject.id,
      conversation_id: "conversation-agent",
      message: { id: "live-message", project_id: stateProject.id, conversation_id: "conversation-agent", sequence: 5, role: "user", markdown: "live message wins", created_at: "2026-08-20T00:00:05Z" },
    }));
    storedRead.resolve([
      { id: "stored-message", project_id: stateProject.id, conversation_id: "conversation-agent", sequence: 1, role: "assistant", markdown: "stored history", created_at: "2026-08-20T00:00:01Z" },
      { id: "live-message", project_id: stateProject.id, conversation_id: "conversation-agent", sequence: 5, role: "user", markdown: "stale duplicate", created_at: "2026-08-20T00:00:04Z" },
    ]);
    expect(await screen.findByText("stored history")).toBeInTheDocument();
    expect(screen.getByText("live message wins")).toBeInTheDocument();
    expect(screen.queryByText("stale duplicate")).not.toBeInTheDocument();

    const composer = screen.getByRole("textbox", { name: /描述研究目标/ });
    fireEvent.change(composer, { target: { value: "after hydration" } });
    await waitFor(() => expect(screen.getByRole("button", { name: "发送" })).toBeEnabled());
    fireEvent.click(screen.getByRole("button", { name: "发送" }));
    await waitFor(() => expect(submitMessage).toHaveBeenCalledWith(expect.objectContaining({ sequence: 6 })));
  });

  it("does not let initial hydration roll back a newer Agent event snapshot", async () => {
    const { stateSpy } = setupConversationStateHarness();
    const revision = stateRevision("conversation-agent");
    const currentSnapshot = stateSnapshot("conversation-agent", {
      mode: "plan", locked: true, latest_plan_revision: revision, latest_run: stateRun(),
    });
    const initialSnapshot = deferred<ConversationAgentStateV4>();
    stateSpy.mockImplementationOnce(() => initialSnapshot.promise).mockResolvedValue(currentSnapshot);
    const proposed = agentEvent("conversation-agent", "run-plan", 1, { kind: "plan_proposed", plan: statePlan, plan_hash: revision.plan_hash });
    let emit!: (event: AgentRunEventV4) => void;
    vi.spyOn(api, "onAgentV4Event").mockImplementation(async (callback) => { emit = callback; return () => undefined; });

    render(<DesktopApp />);
    await screen.findByRole("heading", { name: "Agent 会话" });
    await waitFor(() => expect(emit).toBeDefined());
    await act(async () => emit(proposed));
    expect(await screen.findByText("修订 2 · 待审批")).toBeInTheDocument();

    initialSnapshot.resolve(stateSnapshot("conversation-agent"));
    await act(async () => { await initialSnapshot.promise; });
    expect(screen.getByText("修订 2 · 待审批")).toBeInTheDocument();
  });

  it("does not let historical events replace a newer live active run", async () => {
    const { stateSpy } = setupConversationStateHarness();
    stateSpy.mockResolvedValue(stateSnapshot("conversation-agent"));
    const historical = deferred<AgentRunEventV4[]>();
    vi.spyOn(api, "agentV4EventsForConversation").mockReturnValue(historical.promise);
    let emit!: Parameters<typeof api.onAgentV4Event>[0];
    vi.spyOn(api, "onAgentV4Event").mockImplementation(async (callback) => { emit = callback; return () => undefined; });
    const cancel = vi.spyOn(api, "agentV4Cancel").mockResolvedValue();

    render(<DesktopApp />);
    await screen.findByRole("heading", { name: "Agent 会话" });
    await waitFor(() => expect(emit).toBeDefined());
    await act(async () => emit(agentEvent("conversation-agent", "new-live-run", 1, { kind: "model_text", text: "new live run" })));
    historical.resolve([agentEvent("conversation-agent", "old-history-run", 59, { kind: "run_created", mode: "execute" })]);
    expect(await screen.findByText("new live run")).toBeInTheDocument();
    fireEvent.click(await screen.findByRole("button", { name: "终止运行" }));
    await waitFor(() => expect(cancel).toHaveBeenCalledWith("new-live-run"));
  });

  it("ignores events delivered to an old conversation listener after switching sessions", async () => {
    const { stateSpy } = setupConversationStateHarness();
    stateSpy.mockImplementation(async (_projectId, conversationId) => stateSnapshot(conversationId));
    const conversationListeners: Array<Parameters<typeof api.onConversationEvent>[0]> = [];
    const agentListeners: Array<Parameters<typeof api.onAgentV4Event>[0]> = [];
    vi.spyOn(api, "onConversationEvent").mockImplementation(async (callback) => { conversationListeners.push(callback); return () => undefined; });
    vi.spyOn(api, "onAgentV4Event").mockImplementation(async (callback) => { agentListeners.push(callback); return () => undefined; });

    render(<DesktopApp />);
    await screen.findByRole("heading", { name: "Agent 会话" });
    const oldConversationListener = conversationListeners.at(-1)!;
    const oldAgentListener = agentListeners.at(-1)!;
    fireEvent.click(screen.getByRole("button", { name: "Plan 会话" }));
    await screen.findByRole("heading", { name: "Plan 会话" });

    await act(async () => {
      oldConversationListener({
        project_id: stateProject.id,
        conversation_id: "conversation-agent",
        message: { id: "old-message", project_id: stateProject.id, conversation_id: "conversation-agent", sequence: 1, role: "user", markdown: "旧会话迟到的消息", created_at: "2026-08-20T00:00:00Z" },
      });
      oldAgentListener(agentEvent("conversation-agent", "old-run", 1, { kind: "model_text", text: "旧会话迟到的事件" }));
    });

    expect(screen.queryByText("旧会话迟到的消息")).not.toBeInTheDocument();
    expect(screen.queryByText("旧会话迟到的事件")).not.toBeInTheDocument();
    expect(screen.queryByRole("region", { name: "远程 Agent 运行控制" })).not.toBeInTheDocument();
  });

  it("ignores a late answer callback after switching sessions", async () => {
    const { stateSpy } = setupConversationStateHarness();
    stateSpy.mockImplementation(async (_projectId, conversationId) => stateSnapshot(conversationId));
    const requested = agentEvent("conversation-agent", "answer-run", 1, { kind: "input_requested", question_id: "question-old", question: "旧会话需要补充信息" });
    const staleResult = agentEvent("conversation-agent", "answer-run", 2, { kind: "model_text", text: "旧会话回答后的输出" });
    vi.spyOn(api, "agentV4EventsForConversation").mockImplementation(async (_projectId, conversationId) => conversationId === "conversation-agent" ? [requested] : []);
    const answer = deferred<void>();
    vi.spyOn(api, "agentV4Answer").mockReturnValue(answer.promise);
    const resume = vi.spyOn(api, "agentV4Resume").mockResolvedValue();
    let returnStaleEvents = false;
    const events = vi.spyOn(api, "agentV4Events").mockImplementation(async () => returnStaleEvents ? [staleResult] : []);

    render(<DesktopApp />);
    const answerInput = await screen.findByRole("textbox", { name: "回答 V4 问题" });
    fireEvent.change(answerInput, { target: { value: "旧会话回答" } });
    fireEvent.click(screen.getByRole("button", { name: "回答并恢复" }));
    fireEvent.click(screen.getByRole("button", { name: "Plan 会话" }));
    await screen.findByRole("heading", { name: "Plan 会话" });

    returnStaleEvents = true;
    answer.resolve();
    await waitFor(() => expect(resume).toHaveBeenCalledWith("answer-run"));
    await waitFor(() => expect(events).toHaveBeenCalledWith("answer-run"));
    expect(screen.queryByText("旧会话回答后的输出")).not.toBeInTheDocument();
    expect(screen.queryByRole("region", { name: "远程 Agent 运行控制" })).not.toBeInTheDocument();
  });

  it("ignores a late tool approval callback after switching sessions", async () => {
    const { stateSpy } = setupConversationStateHarness();
    stateSpy.mockImplementation(async (_projectId, conversationId) => stateSnapshot(conversationId));
    const request = { approval_id: "approval-old", call: { call_id: "call-old", tool_id: "runtime.execute", arguments: { code: "old" } }, effect: "runtime" as const, reason: "旧会话工具审批", call_hash: "call-hash-old" };
    const approval = agentEvent("conversation-agent", "approval-run", 1, { kind: "tool_approval_requested", request });
    const staleResult = agentEvent("conversation-agent", "approval-run", 2, { kind: "model_text", text: "旧会话审批后的输出" });
    vi.spyOn(api, "agentV4EventsForConversation").mockImplementation(async (_projectId, conversationId) => conversationId === "conversation-agent" ? [approval] : []);
    const decide = deferred<void>();
    vi.spyOn(api, "agentV4DecideToolApproval").mockReturnValue(decide.promise);
    const resume = vi.spyOn(api, "agentV4Resume").mockResolvedValue();
    let returnStaleEvents = false;
    const events = vi.spyOn(api, "agentV4Events").mockImplementation(async () => returnStaleEvents ? [staleResult] : []);

    render(<DesktopApp />);
    fireEvent.click(await screen.findByRole("button", { name: "批准并继续" }));
    fireEvent.click(screen.getByRole("button", { name: "Plan 会话" }));
    await screen.findByRole("heading", { name: "Plan 会话" });

    returnStaleEvents = true;
    decide.resolve();
    await waitFor(() => expect(resume).toHaveBeenCalledWith("approval-run"));
    await waitFor(() => expect(events).toHaveBeenCalledWith("approval-run"));
    expect(screen.queryByText("旧会话审批后的输出")).not.toBeInTheDocument();
    expect(screen.queryByRole("region", { name: "远程 Agent 运行控制" })).not.toBeInTheDocument();
  });

  it("ignores a late side-effect verification callback after switching sessions", async () => {
    const { stateSpy } = setupConversationStateHarness();
    stateSpy.mockImplementation(async (_projectId, conversationId) => stateSnapshot(conversationId));
    const uncertain = agentEvent("conversation-agent", "uncertain-run", 1, { kind: "tool_dispatch_uncertain", call_id: "call-old", tool_id: "runtime.execute" });
    const staleResult = agentEvent("conversation-agent", "uncertain-run", 2, { kind: "model_text", text: "旧会话核验后的输出" });
    vi.spyOn(api, "agentV4EventsForConversation").mockImplementation(async (_projectId, conversationId) => conversationId === "conversation-agent" ? [uncertain] : []);
    const resolveUncertain = deferred<void>();
    vi.spyOn(api, "agentV4ResolveUncertain").mockReturnValue(resolveUncertain.promise);
    const resume = vi.spyOn(api, "agentV4Resume").mockResolvedValue();
    let returnStaleEvents = false;
    const events = vi.spyOn(api, "agentV4Events").mockImplementation(async () => returnStaleEvents ? [staleResult] : []);

    render(<DesktopApp />);
    const evidence = await screen.findByRole("textbox", { name: "核验证据" });
    fireEvent.change(evidence, { target: { value: "旧会话核验记录" } });
    fireEvent.click(screen.getByRole("button", { name: "保存证据并继续" }));
    fireEvent.click(screen.getByRole("button", { name: "Plan 会话" }));
    await screen.findByRole("heading", { name: "Plan 会话" });

    returnStaleEvents = true;
    resolveUncertain.resolve();
    await waitFor(() => expect(resume).toHaveBeenCalledWith("uncertain-run"));
    await waitFor(() => expect(events).toHaveBeenCalledWith("uncertain-run"));
    expect(screen.queryByText("旧会话核验后的输出")).not.toBeInTheDocument();
    expect(screen.queryByRole("region", { name: "远程 Agent 运行控制" })).not.toBeInTheDocument();
  });

  it("ignores a late resume callback after switching sessions", async () => {
    const { stateSpy } = setupConversationStateHarness();
    stateSpy.mockImplementation(async (_projectId, conversationId) => stateSnapshot(conversationId));
    const failed = agentEvent("conversation-agent", "resume-run", 1, { kind: "run_failed", message: "旧会话运行失败" });
    const staleResult = agentEvent("conversation-agent", "resume-run", 2, { kind: "model_text", text: "旧会话恢复后的输出" });
    vi.spyOn(api, "agentV4EventsForConversation").mockImplementation(async (_projectId, conversationId) => conversationId === "conversation-agent" ? [failed] : []);
    const resumeDeferred = deferred<void>();
    const resume = vi.spyOn(api, "agentV4Resume").mockReturnValue(resumeDeferred.promise);
    let returnStaleEvents = false;
    const events = vi.spyOn(api, "agentV4Events").mockImplementation(async () => returnStaleEvents ? [staleResult] : []);

    render(<DesktopApp />);
    fireEvent.click(await screen.findByRole("button", { name: "继续运行" }));
    fireEvent.click(screen.getByRole("button", { name: "Plan 会话" }));
    await screen.findByRole("heading", { name: "Plan 会话" });

    returnStaleEvents = true;
    resumeDeferred.resolve();
    await waitFor(() => expect(resume).toHaveBeenCalledWith("resume-run"));
    expect(screen.queryByText("旧会话恢复后的输出")).not.toBeInTheDocument();
    expect(screen.queryByRole("region", { name: "远程 Agent 运行控制" })).not.toBeInTheDocument();
  });

  it("restores a pending revision and its lock after remount", async () => {
    const { stateSpy } = setupConversationStateHarness();
    const revision = stateRevision("conversation-agent");
    const snapshot = stateSnapshot("conversation-agent", {
      mode: "plan", locked: true, latest_plan_revision: revision, latest_run: stateRun(),
    });
    stateSpy.mockResolvedValue(snapshot);

    const first = render(<DesktopApp />);
    expect(await screen.findByText("修订 2 · 待审批")).toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: /描述研究目标/ })).toBeDisabled();
    expect(screen.getByRole("button", { name: "批准并运行" })).toBeEnabled();
    first.unmount();

    render(<DesktopApp />);
    expect(await screen.findByText("修订 2 · 待审批")).toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: /描述研究目标/ })).toBeDisabled();
    expect(stateSpy.mock.calls.filter(([, id]) => id === "conversation-agent").length).toBeGreaterThanOrEqual(2);
  });

  it("rolls back an optimistic mode change when persistence fails", async () => {
    const { stateSpy } = setupConversationStateHarness();
    stateSpy.mockResolvedValue(stateSnapshot("conversation-agent"));
    vi.spyOn(api, "setConversationAgentMode").mockRejectedValue(new Error("mode write failed"));

    render(<DesktopApp />);
    await screen.findByText(/Agent 模式：LOCAL/);
    fireEvent.click(screen.getByRole("button", { name: "Agent 权限" }));
    fireEvent.click(screen.getByRole("menuitemcheckbox", { name: "先做计划" }));

    expect(await screen.findByText("mode write failed")).toBeInTheDocument();
    expect(await screen.findByText(/Agent 模式：LOCAL/)).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Plan" })).not.toBeInTheDocument();
  });

  it("requests one revision resume and approves the latest revision before switching to Agent", async () => {
    const { stateSpy } = setupConversationStateHarness();
    const revision = stateRevision("conversation-agent");
    stateSpy.mockResolvedValue(stateSnapshot("conversation-agent", {
      mode: "plan", locked: true, latest_plan_revision: revision, latest_run: stateRun(),
    }));
    let releaseRequest!: () => void;
    const requestRevision = vi.spyOn(api, "agentV4RequestPlanRevision").mockImplementation(() => new Promise((resolve) => {
      releaseRequest = () => resolve({ run_id: revision.run_id, revision: revision.revision, plan_hash: revision.plan_hash, status: "revising", feedback: "增加批次检查" });
    }));
    const resume = vi.spyOn(api, "agentV4Resume").mockResolvedValue();
    const approve = vi.spyOn(api, "agentV4ApprovePlan").mockResolvedValue({ ...stateRun("run-plan", "running"), session_mode: "agent" });

    render(<DesktopApp />);
    const feedback = await screen.findByRole("textbox", { name: "计划修改意见" });
    const request = screen.getByRole("button", { name: "请求修改" });
    expect(request).toBeDisabled();
    fireEvent.change(feedback, { target: { value: "增加批次检查" } });
    fireEvent.click(request);
    fireEvent.click(request);
    expect(requestRevision).toHaveBeenCalledTimes(1);
    expect(screen.getByRole("button", { name: "批准并运行" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "取消计划" })).toBeDisabled();
    releaseRequest();
    await waitFor(() => expect(resume).toHaveBeenCalledTimes(1));

    fireEvent.click(screen.getByRole("button", { name: "批准并运行" }));
    await waitFor(() => expect(approve).toHaveBeenCalledWith("run-plan", "approval-state", 2));
    expect(await screen.findByText(/Agent 模式：LOCAL/)).toBeInTheDocument();
  });

  it("refreshes a stale approval without leaving Plan mode or unlocking the conversation", async () => {
    const { stateSpy } = setupConversationStateHarness();
    const revision2 = stateRevision("conversation-agent");
    const revision3 = stateRevision("conversation-agent", "run-plan", "pending", 3);
    stateSpy
      .mockResolvedValueOnce(stateSnapshot("conversation-agent", { mode: "plan", locked: true, latest_plan_revision: revision2, latest_run: stateRun() }))
      .mockResolvedValue(stateSnapshot("conversation-agent", { mode: "plan", locked: true, latest_plan_revision: revision3, latest_run: stateRun("run-plan", "awaiting_approval", statePlan, "approval-new", 3) }));
    vi.spyOn(api, "agentV4ApprovePlan").mockRejectedValue(new Error("stale plan revision"));

    render(<DesktopApp />);
    fireEvent.click(await screen.findByRole("button", { name: "批准并运行" }));

    expect(await screen.findByText("stale plan revision")).toBeInTheDocument();
    expect(await screen.findByText("修订 3 · 待审批")).toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: /描述研究目标/ })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Plan" })).toBeDisabled();
  });

  it("cancels a pending plan, stays in Plan mode, and unlocks the composer", async () => {
    const { stateSpy } = setupConversationStateHarness();
    const pending = stateRevision("conversation-agent");
    const cancelled = { ...pending, status: "cancelled" as const };
    stateSpy
      .mockResolvedValueOnce(stateSnapshot("conversation-agent", { mode: "plan", locked: true, latest_plan_revision: pending, latest_run: stateRun() }))
      .mockResolvedValue(stateSnapshot("conversation-agent", { mode: "plan", locked: false, latest_plan_revision: cancelled, latest_run: stateRun("run-plan", "cancelled", statePlan, "approval-state", 2) }));
    const cancel = vi.spyOn(api, "agentV4Cancel").mockResolvedValue();

    render(<DesktopApp />);
    fireEvent.click(await screen.findByRole("button", { name: "取消计划" }));
    await waitFor(() => expect(cancel).toHaveBeenCalledWith("run-plan"));

    expect(await screen.findByText("修订 2 · 已取消")).toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: /描述研究目标/ })).toBeEnabled();
    expect(screen.getByRole("button", { name: "Plan" })).toBeEnabled();
  });

  it("does not merge a historical cancelled plan into a later direct run", async () => {
    const { stateSpy } = setupConversationStateHarness();
    stateSpy.mockResolvedValue(stateSnapshot("conversation-agent", {
      mode: "agent",
      latest_plan_revision: stateRevision("conversation-agent", "old-plan", "cancelled", 1),
      latest_run: { ...stateRun("direct-run", "waiting_for_approval", null, null, null), session_mode: "agent" },
    }));

    render(<DesktopApp />);
    expect(await screen.findByText(/Agent 模式：LOCAL/)).toBeInTheDocument();
    expect(screen.queryByText(statePlan.objective)).not.toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: /描述研究目标/ })).toBeDisabled();
  });

  it("re-hydrates the latest pending revision when a plan lifecycle event arrives", async () => {
    const { stateSpy } = setupConversationStateHarness();
    const generating = stateRevision("conversation-agent", "run-plan", "generating", 2);
    const pending = { ...generating, status: "pending" as const };
    stateSpy
      .mockResolvedValueOnce(stateSnapshot("conversation-agent", { mode: "plan", locked: true, latest_plan_revision: generating, latest_run: stateRun("run-plan", "planning") }))
      .mockResolvedValue(stateSnapshot("conversation-agent", { mode: "plan", locked: true, latest_plan_revision: pending, latest_run: stateRun() }));
    let emit!: (event: Parameters<Parameters<typeof api.onAgentV4Event>[0]>[0]) => void;
    vi.spyOn(api, "onAgentV4Event").mockImplementation(async (callback) => { emit = callback; return () => undefined; });

    render(<DesktopApp />);
    expect(await screen.findByText("修订 2 · 生成中")).toBeInTheDocument();
    await act(async () => emit({
      schema_version: 4, run_id: "run-plan", project_id: stateProject.id, conversation_id: "conversation-agent",
      sequence: 2, occurred_at: "2026-08-20T00:00:01Z", previous_hash: "", event_hash: "hash",
      event: { kind: "plan_proposed", plan: statePlan, plan_hash: generating.plan_hash },
    }));

    expect(await screen.findByText("修订 2 · 待审批")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "批准并运行" })).toBeEnabled();
  });

  it("returns from an open workspace to the project library", async () => {
    vi.spyOn(api, "listProjects").mockResolvedValue([{ id: "project-1", name: "PBMC 项目", description: "", local_root: "E:/Science/pbmc", remote_root: null, connection_id: null, template: "single_cell_rna_seq", status: "running", ollama_only: false, created_at: "2026-08-11T00:00:00Z", updated_at: "2026-08-11T00:00:00Z" }]);
    render(<DesktopApp />);

    fireEvent.click(await screen.findByRole("button", { name: "返回项目主页" }));
    expect(await screen.findByRole("heading", { name: "生命科学项目" })).toBeInTheDocument();
    expect(screen.getByText("PBMC 项目")).toBeInTheDocument();
  });

  it("removes a deleted project from the recent project list", async () => {
    const project = { id: "project-1", name: "待删除项目", description: "", local_root: "E:/Science/delete-me", remote_root: null, connection_id: null, template: "single_cell_rna_seq" as const, status: "ready" as const, ollama_only: false, created_at: "2026-08-11T00:00:00Z", updated_at: "2026-08-11T00:00:00Z" };
    vi.spyOn(api, "listProjects").mockResolvedValue([project]);
    const deleteProject = vi.spyOn(api, "deleteProject").mockResolvedValue(undefined);
    vi.spyOn(window, "confirm").mockReturnValue(true);
    render(<DesktopApp />);

    fireEvent.click(await screen.findByRole("button", { name: "返回项目主页" }));
    fireEvent.click(screen.getByRole("button", { name: "删除项目：待删除项目" }));

    await waitFor(() => expect(deleteProject).toHaveBeenCalledWith(project.id));
    await waitFor(() => expect(screen.queryByText("待删除项目")).not.toBeInTheDocument());
  });

  it("centralizes model, remote compute, privacy, and permission settings", async () => {
    vi.spyOn(api, "listProjects").mockResolvedValue([]);
    render(<DesktopApp />);
    fireEvent.click(await screen.findByRole("button", { name: "设置" }));

    const dialog = screen.getByRole("dialog", { name: "工作台设置" });
    expect(dialog).toHaveTextContent("Anthropic");
    expect(dialog).toHaveTextContent("OpenAI-compatible");
    expect(dialog).toHaveTextContent("Ollama");
    expect(dialog).toHaveTextContent("隐私与权限");
    expect(dialog).not.toHaveTextContent("历史运行只读");
  });

  it("creates and switches to an empty conversation when New conversation is clicked", async () => {
    const project = { id: "project-1", name: "PBMC 项目", description: "", local_root: "E:/Science/pbmc", remote_root: null, connection_id: null, template: "single_cell_rna_seq" as const, status: "running" as const, ollama_only: false, created_at: "2026-08-11T00:00:00Z", updated_at: "2026-08-11T00:00:00Z" };
    const existing = { id: "conversation-1", project_id: project.id, title: "旧问题", status: "idle" as const, model_profile_id: null, created_at: "2026-08-11T00:00:00Z", updated_at: "2026-08-11T00:00:00Z" };
    const created = { ...existing, id: "conversation-2", title: "", created_at: "2026-08-12T00:00:00Z", updated_at: "2026-08-12T00:00:00Z" };
    vi.spyOn(api, "listProjects").mockResolvedValue([project]);
    vi.spyOn(api, "listConversations").mockResolvedValue([existing]);
    vi.spyOn(api, "listMessages").mockResolvedValue([]);
    const createConversation = vi.spyOn(api, "createConversation").mockResolvedValue(created);

    render(<DesktopApp />);
    await screen.findByRole("heading", { name: "旧问题" });
    await waitFor(() => expect(screen.getByRole("button", { name: "新建会话" })).toBeEnabled());
    fireEvent.click(await screen.findByRole("button", { name: "新建会话" }));
    await waitFor(() => expect(createConversation).toHaveBeenCalledWith(project.id));
    expect(await screen.findByRole("heading", { name: "新会话" })).toBeInTheDocument();
  });

  it("ignores a late new-conversation response after the user switches sessions", async () => {
    const { stateSpy } = setupConversationStateHarness();
    stateSpy.mockImplementation(async (_projectId, conversationId) => stateSnapshot(conversationId));
    const create = deferred<Awaited<ReturnType<typeof api.createConversation>>>();
    vi.spyOn(api, "createConversation").mockReturnValue(create.promise);

    render(<DesktopApp />);
    await screen.findByText(/Agent 模式：LOCAL/);
    fireEvent.click(screen.getByRole("button", { name: "新建会话" }));
    fireEvent.click(screen.getByRole("button", { name: "Plan 会话" }));
    await screen.findByRole("heading", { name: "Plan 会话" });
    create.resolve({ ...stateConversations[0], id: "late-created", title: "迟到新会话" });
    await act(async () => { await create.promise; });

    expect(screen.getByRole("heading", { name: "Plan 会话" })).toBeInTheDocument();
    expect(screen.queryByText("迟到新会话")).not.toBeInTheDocument();
  });

  it("deletes the active conversation and switches to the next one", async () => {
    const project = { id: "project-1", name: "PBMC 项目", description: "", local_root: "E:/Science/pbmc", remote_root: null, connection_id: null, template: "single_cell_rna_seq" as const, status: "running" as const, ollama_only: false, created_at: "2026-08-11T00:00:00Z", updated_at: "2026-08-11T00:00:00Z" };
    const active = { id: "conversation-2", project_id: project.id, title: "当前问题", status: "idle" as const, model_profile_id: null, created_at: "2026-08-12T00:00:00Z", updated_at: "2026-08-12T00:00:00Z" };
    const next = { ...active, id: "conversation-1", title: "保留的问题", created_at: "2026-08-11T00:00:00Z", updated_at: "2026-08-11T00:00:00Z" };
    vi.spyOn(api, "listProjects").mockResolvedValue([project]);
    vi.spyOn(api, "listConversations").mockResolvedValue([active, next]);
    vi.spyOn(api, "listMessages").mockResolvedValue([]);
    const deleteConversation = vi.spyOn(api, "deleteConversation").mockResolvedValue(undefined);
    const confirm = vi.spyOn(window, "confirm").mockReturnValue(true);

    render(<DesktopApp />);
    await screen.findByRole("heading", { name: "当前问题" });
    await waitFor(() => expect(screen.getByRole("button", { name: "删除会话：当前问题" })).toBeEnabled());
    fireEvent.click(await screen.findByRole("button", { name: "删除会话：当前问题" }));
    await waitFor(() => expect(deleteConversation).toHaveBeenCalledWith(project.id, active.id));
    expect(await screen.findByRole("heading", { name: "保留的问题" })).toBeInTheDocument();
    confirm.mockRestore();
  });

  it("ignores a late delete response after the user switches sessions", async () => {
    const { stateSpy } = setupConversationStateHarness();
    stateSpy.mockImplementation(async (_projectId, conversationId) => stateSnapshot(conversationId));
    const deletion = deferred<void>();
    vi.spyOn(api, "deleteConversation").mockReturnValue(deletion.promise);
    vi.spyOn(window, "confirm").mockReturnValue(true);

    render(<DesktopApp />);
    await screen.findByText(/Agent 模式：LOCAL/);
    fireEvent.click(screen.getByRole("button", { name: "删除会话：Agent 会话" }));
    fireEvent.click(screen.getByRole("button", { name: "Plan 会话" }));
    await screen.findByRole("heading", { name: "Plan 会话" });
    deletion.resolve();
    await act(async () => { await deletion.promise; });

    expect(screen.getByRole("heading", { name: "Plan 会话" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Agent 会话" })).toBeInTheDocument();
  });

  it("ignores kernel and sync callbacks retained by a previous project", async () => {
    const projectA = { ...stateProject, id: "project-a", name: "项目 A" };
    const projectB = { ...stateProject, id: "project-b", name: "项目 B" };
    const conversationA = { ...stateConversations[0], id: "conversation-a", project_id: projectA.id, title: "会话 A" };
    const conversationB = { ...stateConversations[0], id: "conversation-b", project_id: projectB.id, title: "会话 B" };
    vi.spyOn(api, "listProjects").mockResolvedValue([projectA, projectB]);
    vi.spyOn(api, "listConversations").mockImplementation(async (projectId) => projectId === projectA.id ? [conversationA] : [conversationB]);
    vi.spyOn(api, "listMessages").mockResolvedValue([]);
    vi.spyOn(api, "listModelProfiles").mockResolvedValue([stateModel]);
    vi.spyOn(api, "agentV4ComputeBackends").mockResolvedValue([stateBackend]);
    vi.spyOn(api, "agentV4ConversationState").mockImplementation(async (projectId, conversationId) => ({ ...stateSnapshot(conversationId), project_id: projectId }));
    vi.spyOn(api, "agentV4EventsForConversation").mockResolvedValue([]);
    vi.spyOn(api, "listKernelSessions").mockResolvedValue([
      { id: "shared-kernel", project_id: projectA.id, language: "python", state: "running" },
      { id: "shared-kernel", project_id: projectB.id, language: "python", state: "running" },
    ]);
    const kernelListeners: Array<Parameters<typeof api.onKernelEvent>[0]> = [];
    const syncListeners: Array<Parameters<typeof api.onSyncEvent>[0]> = [];
    vi.spyOn(api, "onKernelEvent").mockImplementation(async (callback) => { kernelListeners.push(callback); return () => undefined; });
    vi.spyOn(api, "onSyncEvent").mockImplementation(async (callback) => { syncListeners.push(callback); return () => undefined; });

    render(<DesktopApp />);
    await screen.findByRole("heading", { name: "会话 A" });
    await waitFor(() => expect(kernelListeners.length).toBeGreaterThan(0));
    const oldKernel = kernelListeners[0];
    const oldSync = syncListeners[0];
    fireEvent.click(screen.getByRole("button", { name: "返回项目主页" }));
    fireEvent.click(await screen.findByRole("button", { name: /^项目 B/ }));
    await screen.findByRole("heading", { name: "会话 B" });

    const kernelEvent: KernelEvent = { project_id: projectA.id, session_id: "shared-kernel", request_id: "old-request", sequence: 1, occurred_at: "2026-08-20T00:00:00Z", event: { kind: "stdout", payload: "旧项目 kernel 泄漏" } };
    const syncEntry: SyncEntry = { id: "old-sync", project_id: projectA.id, relative_path: "旧项目同步泄漏.txt", local_relative_path: null, remote_path: null, direction: "remote_to_local", size_bytes: 10, sha256: "old", state: "failed", transferred_bytes: 4, retry_count: 1, error: "old", updated_at: "2026-08-20T00:00:00Z" };
    await act(async () => { oldKernel(kernelEvent); oldSync(syncEntry); });
    fireEvent.click(screen.getByRole("tab", { name: "探索" }));
    expect(screen.queryByText("旧项目 kernel 泄漏")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("tab", { name: "文件" }));
    expect(screen.queryByText("旧项目同步泄漏.txt")).not.toBeInTheDocument();
  });
});
