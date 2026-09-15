import * as queueApi from "./composer-queue-api";
import * as preferencesApi from "./conversation-preferences-api";
import { StrictMode } from "react";
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import DesktopApp, { samePendingSubmission } from "./DesktopApp";
import * as api from "./tauri-api";
import * as referenceApi from "./composer-reference-api";
import * as attachmentApi from "./composer-attachment-api";
import type { AgentRunEventV4, ComposerReference, ConversationAgentStateV4, ExecutionPlanV4, KernelEvent, ProposedPlanRevisionV4, RunSummaryV4, StopRunReceiptV4, SyncEntry, WorkspaceMessage } from "./types";

beforeEach(() => {
  vi.restoreAllMocks();
  window.localStorage.clear();
  vi.spyOn(api, "latestUsedConversation").mockResolvedValue(null);
});

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

function stopReceipt(projectId: string, conversationId: string, runId: string, status: StopRunReceiptV4["status"] = "observed"): StopRunReceiptV4 {
  return {
    request_id: `stop-request-${runId}`,
    project_id: projectId,
    conversation_id: conversationId,
    run_id: runId,
    status,
    created_at: "2026-08-20T00:00:00.000Z",
    updated_at: "2026-08-20T00:00:01.000Z",
  };
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
  vi.mocked(api.latestUsedConversation).mockResolvedValue(stateConversations[0]);
  vi.spyOn(api, "listMessages").mockResolvedValue([]);
  vi.spyOn(api, "listModelProfiles").mockResolvedValue([stateModel]);
  vi.spyOn(api, "agentV4ComputeBackends").mockResolvedValue([stateBackend]);
  vi.spyOn(api, "agentV4EventsForConversation").mockResolvedValue([]);
  vi.spyOn(api, "agentV4Events").mockResolvedValue([]);
  return { stateSpy: vi.spyOn(api, "agentV4ConversationState") };
}

describe("DesktopApp", () => {
  it("opens shared search from workspace and project library, attaching without losing the draft", async () => {
    const { stateSpy } = setupConversationStateHarness();
    stateSpy.mockImplementation(async (_projectId, conversationId) => stateSnapshot(conversationId));
    const reference = { kind: "artifact" as const, project_id: stateProject.id, id: "artifact-search" };
    vi.spyOn(referenceApi, "composerReferenceCatalog").mockResolvedValue([{ reference, label: "Search QC", description: "Quality evidence" }]);
    render(<DesktopApp />);
    const input = await screen.findByRole("textbox", { name: /描述研究目标/ });
    await waitFor(() => expect(stateSpy).toHaveBeenCalled());
    await waitFor(() => expect(input).toBeEnabled());
    fireEvent.change(input, { target: { value: "Keep my question" } });
    fireEvent.keyDown(window, { key: "k", ctrlKey: true });
    expect(await screen.findByRole("dialog", { name: "搜索工作区" })).toBeInTheDocument();
    fireEvent.click(await screen.findByRole("button", { name: "附加 Search QC" }));
    await waitFor(() => expect(screen.queryByRole("dialog", { name: "搜索工作区" })).not.toBeInTheDocument());
    expect(input).toHaveValue("Keep my question");
    expect(screen.getByRole("button", { name: "移除引用：Search QC" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "返回项目主页" }));
    fireEvent.keyDown(window, { key: "k", metaKey: true });
    expect(await screen.findByRole("dialog", { name: "搜索工作区" })).toBeInTheDocument();
    fireEvent.keyDown(window, { key: "Escape" });
    expect(screen.queryByRole("dialog", { name: "搜索工作区" })).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: /搜索工作区/ })).toBeInTheDocument();
    fireEvent.keyDown(window, { key: "k", ctrlKey: true });
    fireEvent.click(await screen.findByRole("button", { name: `打开 ${stateProject.name}` }));
    await screen.findByRole("textbox", { name: /描述研究目标/ });
    expect(screen.queryByRole("button", { name: "移除引用：Search QC" })).not.toBeInTheDocument();
  });

  it("keeps an explicitly requested conversation through a failed project load and retry", async () => {
    const { stateSpy } = setupConversationStateHarness();
    const other = { ...stateProject, id: "project-other", name: "Other project" };
    const first = { ...stateConversations[0], project_id: other.id, id: "other-first", title: "Other first" };
    const requested = { ...first, id: "other-requested", title: "Requested saved session" };
    vi.mocked(api.listProjects).mockResolvedValue([stateProject, other]);
    let otherReads = 0;
    vi.mocked(api.listConversations).mockImplementation(async (projectId) => {
      if (projectId !== other.id) return stateConversations;
      otherReads += 1;
      if (otherReads === 2) throw new Error("temporary conversation list failure");
      return [first, requested];
    });
    vi.mocked(api.latestUsedConversation).mockImplementation(async (projectId) => projectId === other.id ? first : stateConversations[0]);
    stateSpy.mockImplementation(async (projectId, conversationId) => ({ ...stateSnapshot(conversationId), project_id: projectId }));
    vi.spyOn(referenceApi, "composerReferenceCatalog").mockImplementation(async (projectId) => projectId === other.id ? [{ reference: { kind: "session", project_id: other.id, id: requested.id }, label: requested.title, description: "Saved transcript" }] : []);
    render(<DesktopApp />);
    await screen.findByRole("textbox", { name: /描述研究目标/ });
    fireEvent.keyDown(window, { key: "k", ctrlKey: true });
    fireEvent.click(await screen.findByRole("button", { name: `打开 ${requested.title}` }));
    fireEvent.click(await screen.findByRole("button", { name: "重试会话恢复" }));
    await waitFor(() => expect(screen.getByRole("heading", { level: 1 })).toHaveTextContent(requested.title));
    await waitFor(() => expect(api.listMessages).toHaveBeenCalledWith(requested.id));
    expect(api.latestUsedConversation).not.toHaveBeenCalledWith(other.id);
  });

  it("ignores a pending search navigation after immediate Escape", async () => {
    const { stateSpy } = setupConversationStateHarness();
    stateSpy.mockImplementation(async (_projectId, conversationId) => stateSnapshot(conversationId));
    vi.spyOn(referenceApi, "composerReferenceCatalog").mockResolvedValue([{ reference: { kind: "session", project_id: stateProject.id, id: stateConversations[1].id }, label: "Pending search session", description: "Saved transcript" }]);
    render(<DesktopApp />);
    await screen.findByRole("heading", { name: stateConversations[0].title });
    const pending = deferred<typeof stateConversations>();
    vi.mocked(api.listConversations).mockReturnValueOnce(pending.promise);
    fireEvent.keyDown(window, { key: "k", ctrlKey: true });
    fireEvent.click(await screen.findByRole("button", { name: "打开 Pending search session" }));
    fireEvent.keyDown(window, { key: "Escape" });
    expect(screen.queryByRole("dialog", { name: "搜索工作区" })).not.toBeInTheDocument();
    await act(async () => pending.resolve(stateConversations));
    expect(screen.getByRole("heading", { name: stateConversations[0].title })).toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: stateConversations[1].title })).not.toBeInTheDocument();
  });

  it("hides search Attach actions while a plan revision is pending even when snapshot lock is false", async () => {
    const { stateSpy } = setupConversationStateHarness();
    stateSpy.mockImplementation(async (_projectId, conversationId) => stateSnapshot(conversationId, {
      mode: "plan",
      latest_plan_revision: stateRevision(conversationId),
      latest_run: stateRun(),
    }));
    vi.spyOn(referenceApi, "composerReferenceCatalog").mockResolvedValue([{
      reference: { kind: "artifact" as const, project_id: stateProject.id, id: "pending-plan-artifact" },
      label: "Pending plan report",
      description: "Quality evidence",
    }]);
    render(<DesktopApp />);
    await screen.findByRole("button", { name: "批准并运行" });
    fireEvent.keyDown(window, { key: "k", ctrlKey: true });
    await screen.findByRole("dialog", { name: "搜索工作区" });
    expect(screen.queryByRole("button", { name: "附加 Pending plan report" })).not.toBeInTheDocument();
  });

  it("keeps search above an active share dialog and Escape closes only search", async () => {
    const { stateSpy } = setupConversationStateHarness();
    stateSpy.mockImplementation(async (_projectId, conversationId) => stateSnapshot(conversationId));
    vi.mocked(api.listMessages).mockResolvedValue([{
      id: "share-message",
      project_id: stateProject.id,
      conversation_id: stateConversations[0].id,
      sequence: 1,
      role: "user",
      markdown: "A shareable research note",
      created_at: "2026-09-13T00:00:00Z",
    }]);
    vi.spyOn(referenceApi, "composerReferenceCatalog").mockResolvedValue([]);
    render(<DesktopApp />);
    const input = await screen.findByRole("textbox", { name: /描述研究目标/ });
    await waitFor(() => expect(input).toBeEnabled());
    fireEvent.click(screen.getByRole("button", { name: "添加上下文或选择模式" }));
    fireEvent.click(await screen.findByRole("menuitem", { name: /分享会话/ }));
    const share = await screen.findByRole("dialog", { name: "分享会话" });

    fireEvent.keyDown(window, { key: "k", ctrlKey: true });
    const search = await screen.findByRole("dialog", { name: "搜索工作区" });
    const searchBackdrop = search.parentElement;
    const shareBackdrop = share.parentElement;
    expect(searchBackdrop).not.toBeNull();
    expect(shareBackdrop).not.toBeNull();

    fireEvent.keyDown(window, { key: "Escape" });
    expect(screen.queryByRole("dialog", { name: "搜索工作区" })).not.toBeInTheDocument();
    expect(screen.getByRole("dialog", { name: "分享会话" })).toBe(share);
  });

  it("routes search settings actions when Settings is already on another section", async () => {
    const { stateSpy } = setupConversationStateHarness();
    stateSpy.mockImplementation(async (_projectId, conversationId) => stateSnapshot(conversationId));
    vi.spyOn(referenceApi, "composerReferenceCatalog").mockResolvedValue([]);
    render(<DesktopApp />);
    const input = await screen.findByRole("textbox", { name: /描述研究目标/ });
    await waitFor(() => expect(input).toBeEnabled());
    fireEvent.click(screen.getByRole("button", { name: "设置" }));
    await screen.findByRole("dialog", { name: "工作台设置" });
    fireEvent.click(screen.getByRole("button", { name: "环境" }));
    expect(await screen.findByRole("heading", { level: 3, name: "远端 Linux 计算" })).toBeInTheDocument();

    fireEvent.keyDown(window, { key: "k", ctrlKey: true });
    fireEvent.click(await screen.findByRole("button", { name: "打开 管理技能" }));
    await waitFor(() => expect(screen.getByRole("heading", { level: 3, name: "科研 Skills" })).toBeInTheDocument());
    expect(screen.getByRole("dialog", { name: "工作台设置" })).toBeInTheDocument();

    fireEvent.keyDown(window, { key: "k", ctrlKey: true });
    fireEvent.click(await screen.findByRole("button", { name: "打开 管理模型" }));
    await waitFor(() => expect(screen.getByRole("heading", { level: 3, name: "模型提供方" })).toBeInTheDocument());
    expect(screen.getByRole("dialog", { name: "工作台设置" })).toBeInTheDocument();

    fireEvent.keyDown(window, { key: "k", ctrlKey: true });
    fireEvent.click(await screen.findByRole("button", { name: "打开 管理 MCP" }));
    await waitFor(() => expect(screen.getByRole("heading", { level: 3, name: "MCP 连接" })).toBeInTheDocument());
  });

  it("closes Settings before search opens project files", async () => {
    const { stateSpy } = setupConversationStateHarness();
    stateSpy.mockImplementation(async (_projectId, conversationId) => stateSnapshot(conversationId));
    vi.spyOn(referenceApi, "composerReferenceCatalog").mockResolvedValue([]);
    render(<DesktopApp />);
    const input = await screen.findByRole("textbox", { name: /描述研究目标/ });
    await waitFor(() => expect(input).toBeEnabled());
    fireEvent.click(screen.getByRole("button", { name: "设置" }));
    await screen.findByRole("dialog", { name: "工作台设置" });
    fireEvent.keyDown(window, { key: "k", ctrlKey: true });
    fireEvent.click(await screen.findByRole("button", { name: "打开 项目文件" }));
    await waitFor(() => expect(screen.queryByRole("dialog", { name: "工作台设置" })).not.toBeInTheDocument());
    const filesTab = await screen.findByRole("tab", { name: "Files" });
    expect(filesTab).toHaveAttribute("aria-selected", "true");
  });

  it("closes Settings before search opens another project", async () => {
    const { stateSpy } = setupConversationStateHarness();
    const other = { ...stateProject, id: "project-other-search", name: "Other search project" };
    const otherConversation = { ...stateConversations[0], project_id: other.id, id: "other-search-conversation", title: "Other project conversation" };
    vi.mocked(api.listProjects).mockResolvedValue([stateProject, other]);
    vi.mocked(api.listConversations).mockImplementation(async (projectId) => projectId === other.id ? [otherConversation] : stateConversations);
    vi.mocked(api.listMessages).mockResolvedValue([]);
    stateSpy.mockImplementation(async (projectId, conversationId) => ({ ...stateSnapshot(conversationId), project_id: projectId }));
    vi.spyOn(referenceApi, "composerReferenceCatalog").mockResolvedValue([]);
    render(<DesktopApp />);
    const input = await screen.findByRole("textbox", { name: /描述研究目标/ });
    await waitFor(() => expect(input).toBeEnabled());
    fireEvent.click(screen.getByRole("button", { name: "设置" }));
    await screen.findByRole("dialog", { name: "工作台设置" });
    fireEvent.keyDown(window, { key: "k", ctrlKey: true });
    fireEvent.click(await screen.findByRole("button", { name: `打开 ${other.name}` }));
    await waitFor(() => expect(screen.queryByRole("dialog", { name: "工作台设置" })).not.toBeInTheDocument());
    await waitFor(() => expect(screen.getByRole("main", { name: "科研对话" })).toHaveTextContent(other.name));
  });

  it.each(["agent", "plan"] as const)("validates references before persisting a %s send and retains a rejected draft", async (mode) => {
    const { stateSpy } = setupConversationStateHarness();
    stateSpy.mockImplementation(async (_projectId, conversationId) => stateSnapshot(conversationId, { mode }));
    const reference = { kind: "artifact" as const, project_id: stateProject.id, id: "artifact-qc" };
    vi.spyOn(referenceApi, "composerReferenceCatalog").mockResolvedValue([{ reference, label: "QC report", description: "QC" }]);
    const validate = vi.spyOn(referenceApi, "validateComposerReferences").mockRejectedValue(new Error("Reference no longer available"));
    const submit = vi.spyOn(api, "submitMessage");
    const start = vi.spyOn(api, mode === "agent" ? "agentV4StartDirect" : "agentV4StartPlanning");
    render(<DesktopApp />);
    const input = await screen.findByRole("textbox", { name: /描述研究目标/ });
    await waitFor(() => expect(stateSpy).toHaveBeenCalled());
    await waitFor(() => expect(input).toBeEnabled());
    fireEvent.change(input, { target: { value: "Summarize @QC", selectionStart: 13 } });
    fireEvent.click(await screen.findByRole("option", { name: /QC report/ }));
    await waitFor(() => expect(screen.getByRole("button", { name: "发送" })).toBeEnabled());
    fireEvent.click(screen.getByRole("button", { name: "发送" }));
    await waitFor(() => expect(validate).toHaveBeenCalledWith(stateProject.id, stateConversations[0].id, [reference]));
    await waitFor(() => expect(screen.getByRole("button", { name: "发送" })).toBeEnabled());
    expect(submit).not.toHaveBeenCalled();
    expect(start).not.toHaveBeenCalled();
    expect(input).toHaveValue("Summarize ");
    expect(screen.getByRole("button", { name: /移除.*QC report/ })).toBeInTheDocument();
  });

  it.each(["agent", "plan"] as const)("preflights and sends stable attachment IDs through %s", async (mode) => {
    const { stateSpy } = setupConversationStateHarness();
    stateSpy.mockImplementation(async (_projectId, conversationId) => stateSnapshot(conversationId, { mode }));
    vi.spyOn(attachmentApi, "stageComposerAttachment").mockResolvedValue({ id: "plot-attachment", project_id: stateProject.id, conversation_id: stateConversations[0].id, name: "plot.png", relative_path: ".omicsops/attachments/plot/bytes.png", size_bytes: 4, sha256: "a".repeat(64), media_type: "image/png" });
    const validation = vi.spyOn(attachmentApi, "validateComposerAttachments").mockRejectedValueOnce(new Error("Attachment changed")).mockResolvedValue(undefined);
    const submit = vi.spyOn(api, "submitMessage").mockResolvedValue({ id: "message-attachment", project_id: stateProject.id, conversation_id: stateConversations[0].id, sequence: 1, role: "user", markdown: "Inspect plot", created_at: "2026-09-13T00:00:00Z" });
    const start = vi.spyOn(api, mode === "agent" ? "agentV4StartDirect" : "agentV4StartPlanning").mockResolvedValue(stateRun());
    render(<DesktopApp />);
    const input = await screen.findByRole("textbox", { name: /描述研究目标/ });
    await waitFor(() => expect(input).toBeEnabled());
    fireEvent.change(input, { target: { value: "Inspect plot" } });
    fireEvent.paste(input, { clipboardData: { files: [new File(["data"], "plot.png", { type: "image/png" })] } });
    await screen.findByText("plot.png");
    await waitFor(() => expect(screen.getByRole("button", { name: "发送" })).toBeEnabled());
    fireEvent.click(screen.getByRole("button", { name: "发送" }));
    await waitFor(() => expect(validation).toHaveBeenCalledWith(stateProject.id, stateConversations[0].id, ["plot-attachment"], expect.any(String)));
    await waitFor(() => expect(screen.getByRole("button", { name: "发送" })).toBeEnabled());
    expect(submit).not.toHaveBeenCalled();
    expect(input).toHaveValue("Inspect plot");
    expect(screen.getByText("plot.png")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "发送" }));
    await waitFor(() => expect(start).toHaveBeenCalledWith(expect.objectContaining({ objective: "Inspect plot", attachments: ["plot-attachment"] })));
    expect(submit).toHaveBeenCalledOnce();
    expect(JSON.stringify(start.mock.calls)).not.toContain("content_base64");
  });

  it.each(["agent", "plan"] as const)("transports selected reference IDs into a %s request", async (mode) => {
    const { stateSpy } = setupConversationStateHarness();
    stateSpy.mockImplementation(async (_projectId, conversationId) => stateSnapshot(conversationId, { mode }));
    const reference = { kind: "artifact" as const, project_id: stateProject.id, id: "artifact-qc" };
    vi.spyOn(referenceApi, "composerReferenceCatalog").mockResolvedValue([{ reference, label: "QC report", description: "QC" }]);
    vi.spyOn(api, "submitMessage").mockResolvedValue({ id: "message-reference", project_id: stateProject.id, conversation_id: stateConversations[0].id, sequence: 1, role: "user", markdown: "Summarize", created_at: "2026-09-13T00:00:00Z" });
    const start = vi.spyOn(api, mode === "agent" ? "agentV4StartDirect" : "agentV4StartPlanning").mockResolvedValue(stateRun());
    render(<DesktopApp />);
    const input = await screen.findByRole("textbox", { name: /描述研究目标/ });
    await waitFor(() => expect(stateSpy).toHaveBeenCalled());
    await waitFor(() => expect(input).toBeEnabled());
    fireEvent.change(input, { target: { value: "Summarize @QC", selectionStart: 13 } });
    fireEvent.click(await screen.findByRole("option", { name: /QC report/ }));
    await waitFor(() => expect(screen.getByRole("button", { name: "发送" })).toBeEnabled());
    fireEvent.click(screen.getByRole("button", { name: "发送" }));
    await waitFor(() => expect(start).toHaveBeenCalledWith(expect.objectContaining({ objective: "Summarize", references: [reference] })));
  });

  it("rejects a settings model save while reasoning effort persistence is in flight", async () => {
    const { stateSpy } = setupConversationStateHarness();
    stateSpy.mockImplementation(async (_projectId, conversationId) => stateSnapshot(conversationId));
    const profile = { ...stateModel, reasoning_effort: "high" as const };
    vi.mocked(api.listModelProfiles).mockResolvedValue([profile]);
    vi.spyOn(api, "listModelProfileModels").mockResolvedValue([profile.model]);
    const saved = deferred<Awaited<ReturnType<typeof api.saveModelProfile>>>();
    const save = vi.spyOn(api, "saveModelProfile").mockReturnValue(saved.promise);
    render(<DesktopApp />);
    await screen.findByText(/Agent 模式：LOCAL/);
    const selector = screen.getByRole("button", { name: "选择模型" });
    await waitFor(() => expect(selector).toBeEnabled());
    fireEvent.click(selector);
    fireEvent.click(screen.getByRole("button", { name: "推理强度: high" }));
    fireEvent.click(screen.getByRole("menuitemradio", { name: "默认" }));
    fireEvent.click(screen.getByRole("button", { name: "设置" }));
    fireEvent.click(screen.getByRole("button", { name: "编辑" }));
    fireEvent.change(screen.getByRole("textbox", { name: /^Model$/ }), { target: { value: "settings-model" } });
    fireEvent.click(screen.getByRole("button", { name: "保存提供方" }));
    expect(await screen.findByText("保存失败，请检查模型配置后重试。")).toBeInTheDocument();
    expect(save).toHaveBeenCalledTimes(1);
    await act(async () => saved.resolve({ ...profile, reasoning_effort: null }));
    save.mockResolvedValue({ ...profile, model: "settings-model" });
    fireEvent.click(screen.getByRole("button", { name: "保存提供方" }));
    await waitFor(() => expect(save).toHaveBeenCalledTimes(2));
  });
  it("persists an explicit default reasoning effort and locks sends until saved", async () => {
    const { stateSpy } = setupConversationStateHarness();
    stateSpy.mockImplementation(async (_projectId, conversationId) => stateSnapshot(conversationId));
    const profile = { ...stateModel, reasoning_effort: "high" as const };
    vi.mocked(api.listModelProfiles).mockResolvedValue([profile]);
    vi.spyOn(api, "listModelProfileModels").mockResolvedValue([profile.model]);
    const saved = deferred<Awaited<ReturnType<typeof api.saveModelProfile>>>();
    const save = vi.spyOn(api, "saveModelProfile").mockReturnValue(saved.promise);
    render(<DesktopApp />);
    await screen.findByText(/Agent 模式：LOCAL/);
    const selector = screen.getByRole("button", { name: "选择模型" });
    await waitFor(() => expect(selector).toBeEnabled());
    fireEvent.click(selector);
    fireEvent.click(screen.getByRole("button", { name: "推理强度: high" }));
    fireEvent.click(screen.getByRole("menuitemradio", { name: "默认" }));
    expect(save).toHaveBeenCalledWith({ id: profile.id, label: profile.label, provider: profile.provider, base_url: profile.base_url, model: profile.model, reasoning_effort: null });
    expect(screen.getByRole("textbox", { name: /描述研究目标/ })).toBeDisabled();
    await act(async () => saved.resolve({ ...profile, reasoning_effort: null }));
    expect(await screen.findByRole("button", { name: "推理强度: 默认" })).toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: /描述研究目标/ })).toBeEnabled();
  });
  it("refreshes shared memory counts after deleting another conversation", async () => {
    const { stateSpy } = setupConversationStateHarness();
    stateSpy.mockImplementation(async (_projectId, conversationId) => stateSnapshot(conversationId));
    let removed = false;
    vi.spyOn(api, "getConversationCapabilitiesV4").mockImplementation(async (project_id, conversation_id) => ({ project_id, conversation_id, skills: [], mcp_servers: [], memory_count: removed ? 1 : 3 }));
    vi.spyOn(api, "deleteConversation").mockImplementation(async () => { removed = true; });
    vi.spyOn(window, "confirm").mockReturnValue(true);
    render(<DesktopApp />);
    await screen.findByText("0 skills · 0 MCP · 3 mem");
    await waitFor(() => expect(screen.getByRole("button", { name: "删除会话：Plan 会话" })).toBeEnabled());
    fireEvent.click(screen.getByRole("button", { name: "删除会话：Plan 会话" }));
    await screen.findByText("0 skills · 0 MCP · 1 mem");
    expect(screen.getByRole("heading", { name: "Agent 会话" })).toBeVisible();
  });
  it("restores a locked ordinary tool approval as execution, not a Plan", async () => {
    const { stateSpy } = setupConversationStateHarness();
    const runId = "ordinary-tool-approval";
    stateSpy.mockImplementation(async (_projectId, conversationId) => stateSnapshot(conversationId, {
      locked: true,
      latest_run: { ...stateRun(runId, "waiting_for_approval", null, null, null), session_mode: "agent" },
    }));
    vi.mocked(api.agentV4EventsForConversation).mockResolvedValue([
      agentEvent("conversation-agent", runId, 1, { kind: "run_created", mode: "execute" }),
      agentEvent("conversation-agent", runId, 2, { kind: "model_text", text: "正在等待文献工具授权。" }),
    ]);
    render(<DesktopApp />);
    expect(await screen.findByText("正在等待文献工具授权。")).toBeVisible();
    expect(screen.queryByText(/计划已生成/)).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "批准并运行" })).not.toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: /描述研究目标/ })).toBeDisabled();
  });

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
    expect(screen.queryByRole("complementary", { name: "项目上下文" })).not.toBeInTheDocument();
  });

  it("starts a direct V4 agent run instead of planning after an ordinary send", async () => {
    const project = { id: "project-1", name: "PBMC 图谱", description: "", local_root: "E:/Science/pbmc", remote_root: null, connection_id: null, template: "single_cell_rna_seq" as const, status: "running" as const, ollama_only: false, created_at: "2026-08-11T00:00:00Z", updated_at: "2026-08-11T00:00:00Z" };
    const conversation = { id: "conversation-1", project_id: project.id, title: "普通问答", status: "idle" as const, model_profile_id: "model-1", created_at: "2026-08-11T00:00:00Z", updated_at: "2026-08-11T00:00:00Z" };
    const model = { id: "model-1", label: "Test model", provider: "ollama" as const, base_url: "http://localhost:11434", model: "test", credential_reference: null, supports_tools: true, supports_vision: false };
    vi.spyOn(api, "listProjects").mockResolvedValue([project]);
    vi.spyOn(api, "listConversations").mockResolvedValue([conversation]);
    vi.mocked(api.latestUsedConversation).mockResolvedValue(conversation);
    vi.spyOn(api, "listMessages").mockResolvedValue([]);
    vi.spyOn(api, "listModelProfiles").mockResolvedValue([model]);
    vi.spyOn(api, "agentV4ComputeBackends").mockResolvedValue([{ descriptor: { schema_version: 4, backend_id: "local", kind: "local", isolation: "process", available: true, supports_python: true, supports_r: false, supports_network_policy: false }, selectable: true, reason: null, python_status: "available", r_status: "unavailable", resolved_image_id: null }]);
    vi.spyOn(api, "submitMessage").mockResolvedValue({ id: "message-1", project_id: project.id, conversation_id: conversation.id, sequence: 1, role: "user", markdown: "先解释一下这个矩阵格式", created_at: "2026-08-21T00:00:00Z" });
    const startDirect = vi.spyOn(api, "agentV4StartDirect").mockResolvedValue({ run_id: "run-direct", status: "running", plan: null, plan_hash: null, compute_selection: { schema_version: 4, backend_id: "local", backend_kind: "local", autonomy_mode: "supervised", environment: "system", network_policy: "host_inherited", container_image: null }, approval_hash: null });
    const startPlanning = vi.spyOn(api, "agentV4StartPlanning");
    const requestStop = vi.spyOn(api, "agentV4RequestStop").mockResolvedValue(stopReceipt(project.id, conversation.id, "run-direct"));
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
    await waitFor(() => expect(requestStop).toHaveBeenCalledWith(expect.objectContaining({ project_id: project.id, conversation_id: conversation.id, run_id: "run-direct" })));
    await waitFor(() => expect(screen.queryByRole("button", { name: "终止运行" })).not.toBeInTheDocument());
  });

  it("keeps an accepted stop busy and reuses its request id after a retryable failure", async () => {
    const { stateSpy } = setupConversationStateHarness();
    const activeRun = { ...stateRun("run-direct", "running", null, null, null), session_mode: "agent" as const };
    stateSpy.mockResolvedValue(stateSnapshot("conversation-agent", { latest_run: activeRun }));
    const requestStop = vi.spyOn(api, "agentV4RequestStop")
      .mockRejectedValueOnce(new Error("temporary stop transport failure"))
      .mockResolvedValueOnce(stopReceipt(stateProject.id, "conversation-agent", "run-direct", "requested"));

    render(<DesktopApp />);
    const stop = await screen.findByRole("button", { name: "终止运行" });
    fireEvent.click(stop);
    await waitFor(() => expect(requestStop).toHaveBeenCalledTimes(1));
    expect(screen.getByRole("button", { name: "终止运行" })).toBeEnabled();

    fireEvent.click(screen.getByRole("button", { name: "终止运行" }));
    await waitFor(() => expect(requestStop).toHaveBeenCalledTimes(2));
    expect(requestStop.mock.calls[0][0].request_id).toBe(requestStop.mock.calls[1][0].request_id);
    expect(await screen.findByRole("button", { name: "终止中…" })).toBeDisabled();
  });

  it("shows a localized scoped stop lookup error while leaving retry available", async () => {
    const { stateSpy } = setupConversationStateHarness();
    const activeRun = { ...stateRun("run-stop", "running", null, null, null), session_mode: "agent" as const };
    stateSpy.mockResolvedValue(stateSnapshot("conversation-agent", { latest_run: activeRun }));
    vi.spyOn(api, "agentV4GetStop").mockRejectedValue(new Error("native stop details"));

    render(<DesktopApp />);
    expect(await screen.findByRole("button", { name: "终止运行" })).toBeEnabled();
    expect(await screen.findByText("停止状态更新失败，请重试。")).toBeInTheDocument();
    expect(screen.queryByText("native stop details")).not.toBeInTheDocument();
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

  it.each(["agent", "plan"] as const)("reuses the persisted %s message after a failed native start", async (mode) => {
    const { stateSpy } = setupConversationStateHarness();
    stateSpy.mockImplementation(async (_projectId, conversationId) => stateSnapshot(conversationId, { mode }));
    const submit = vi.spyOn(api, "submitMessage").mockResolvedValue({
      id: `message-${mode}-retry`, project_id: stateProject.id, conversation_id: stateConversations[0].id,
      sequence: 1, role: "user", markdown: "Retry this native run", created_at: "2026-09-13T00:00:00Z",
    });
    const startDirect = vi.spyOn(api, "agentV4StartDirect");
    const startPlanning = vi.spyOn(api, "agentV4StartPlanning");
    const start = mode === "agent" ? startDirect : startPlanning;
    start
      .mockRejectedValueOnce(new Error("native start rejected"))
      .mockResolvedValueOnce(mode === "plan"
        ? stateRun(`run-${mode}-retry`, "awaiting_approval")
        : { ...stateRun(`run-${mode}-retry`, "running", null, null, null), session_mode: "agent" });
    vi.spyOn(api, "agentV4Events").mockResolvedValue([]);

    render(<DesktopApp />);
    const input = await screen.findByRole("textbox", { name: /描述研究目标/ });
    await waitFor(() => expect(input).toBeEnabled());
    fireEvent.change(input, { target: { value: "Retry this native run" } });
    await waitFor(() => expect(screen.getByRole("button", { name: "发送" })).toBeEnabled());
    fireEvent.click(screen.getByRole("button", { name: "发送" }));

    await waitFor(() => expect(start).toHaveBeenCalledTimes(1));
    await waitFor(() => expect(screen.getByRole("button", { name: "发送" })).toBeEnabled());
    expect(submit).toHaveBeenCalledTimes(1);
    expect(input).toHaveValue("Retry this native run");

    fireEvent.click(screen.getByRole("button", { name: "发送" }));
    await waitFor(() => expect(start).toHaveBeenCalledTimes(2));
    await waitFor(() => expect(input).toHaveValue(""));
    expect(submit).toHaveBeenCalledTimes(1);
  });

  it("persists a new message when the failed-start draft changes before retry", async () => {
    const { stateSpy } = setupConversationStateHarness();
    stateSpy.mockImplementation(async (_projectId, conversationId) => stateSnapshot(conversationId));
    const submit = vi.spyOn(api, "submitMessage").mockImplementation(async (request) => ({
      id: `message-${request.markdown}`, project_id: request.project_id, conversation_id: request.conversation_id,
      sequence: request.sequence, role: "user", markdown: request.markdown, created_at: "2026-09-13T00:00:00Z",
    }));
    const start = vi.spyOn(api, "agentV4StartDirect")
      .mockRejectedValueOnce(new Error("first native start rejected"))
      .mockRejectedValueOnce(new Error("second native start rejected"));
    vi.spyOn(api, "agentV4Events").mockResolvedValue([]);

    render(<DesktopApp />);
    const input = await screen.findByRole("textbox", { name: /描述研究目标/ });
    await waitFor(() => expect(input).toBeEnabled());
    fireEvent.change(input, { target: { value: "First native run" } });
    await waitFor(() => expect(screen.getByRole("button", { name: "发送" })).toBeEnabled());
    fireEvent.click(screen.getByRole("button", { name: "发送" }));
    await waitFor(() => expect(start).toHaveBeenCalledTimes(1));
    await waitFor(() => expect(screen.getByRole("button", { name: "发送" })).toBeEnabled());

    fireEvent.change(input, { target: { value: "Changed native run" } });
    fireEvent.click(screen.getByRole("button", { name: "发送" }));
    await waitFor(() => expect(start).toHaveBeenCalledTimes(2));
    expect(submit).toHaveBeenCalledTimes(2);
    expect(submit.mock.calls.map(([request]) => request.markdown)).toEqual(["First native run", "Changed native run"]);
    expect(start.mock.calls.map(([request]) => request.objective)).toEqual(["First native run", "Changed native run"]);
  });

  it("matches a failed-start retry by stable reference IDs, while a changed ID starts a new submission", () => {
    const firstReference: ComposerReference = { kind: "artifact", project_id: stateProject.id, id: "artifact-1" };
    const equivalentReference = { id: "artifact-1", project_id: stateProject.id, kind: "artifact" as const };
    const pendingMessage: WorkspaceMessage = {
      id: "message-reference-retry", project_id: stateProject.id, conversation_id: stateConversations[0].id,
      sequence: 1, role: "user", markdown: "Retry with a reference", created_at: "2026-09-13T00:00:00Z",
    };
    const pending = {
      projectId: stateProject.id,
      conversationId: stateConversations[0].id,
      markdown: pendingMessage.markdown,
      mode: "chat" as const,
      references: [firstReference],
      attachments: [],
      message: pendingMessage,
    };
    expect(samePendingSubmission(pending, pending.projectId, pending.conversationId, pending.markdown, pending.mode, [equivalentReference], [])).toBe(true);
    expect(samePendingSubmission(pending, pending.projectId, pending.conversationId, pending.markdown, pending.mode, [{ ...equivalentReference, id: "artifact-2" }], [])).toBe(false);
  });

  it.each(["agent", "plan"] as const)("keeps an accepted %s run when event history reconciliation fails", async (mode) => {
    const { stateSpy } = setupConversationStateHarness();
    stateSpy.mockImplementation(async (_projectId, conversationId) => stateSnapshot(conversationId, { mode }));
    vi.spyOn(api, "submitMessage").mockResolvedValue({
      id: `message-${mode}-events`, project_id: stateProject.id, conversation_id: stateConversations[0].id,
      sequence: 1, role: "user", markdown: "Accept despite event read", created_at: "2026-09-13T00:00:00Z",
    });
    const accepted = mode === "plan"
      ? stateRun(`run-${mode}-events`, "awaiting_approval")
      : { ...stateRun(`run-${mode}-events`, "running", null, null, null), session_mode: "agent" as const };
    const startDirect = vi.spyOn(api, "agentV4StartDirect");
    const startPlanning = vi.spyOn(api, "agentV4StartPlanning");
    const start = mode === "agent" ? startDirect : startPlanning;
    start.mockResolvedValue(accepted);
    const events = vi.spyOn(api, "agentV4Events")
      .mockRejectedValueOnce(new Error("event history unavailable"))
      .mockResolvedValue([]);

    render(<DesktopApp />);
    const input = await screen.findByRole("textbox", { name: /描述研究目标/ });
    await waitFor(() => expect(input).toBeEnabled());
    fireEvent.change(input, { target: { value: "Accept despite event read" } });
    await waitFor(() => expect(screen.getByRole("button", { name: "发送" })).toBeEnabled());
    fireEvent.click(screen.getByRole("button", { name: "发送" }));

    await waitFor(() => expect(start).toHaveBeenCalledTimes(1));
    await waitFor(() => expect(input).toHaveValue(""));
    expect(events).toHaveBeenCalledWith(accepted.run_id);
    expect(screen.queryByText("消息未能发送")).not.toBeInTheDocument();
    expect(screen.queryByText("native start rejected")).not.toBeInTheDocument();
  });

  it("ignores a stale native-start failure after switching conversations", async () => {
    const { stateSpy } = setupConversationStateHarness();
    stateSpy.mockImplementation(async (projectId, conversationId) => ({ ...stateSnapshot(conversationId), project_id: projectId }));
    const oldStart = deferred<Awaited<ReturnType<typeof api.agentV4StartDirect>>>();
    const submit = vi.spyOn(api, "submitMessage").mockImplementation(async (request) => ({
      id: `message-${request.conversation_id}`, project_id: request.project_id, conversation_id: request.conversation_id,
      sequence: request.sequence, role: "user", markdown: request.markdown, created_at: "2026-09-13T00:00:00Z",
    }));
    const start = vi.spyOn(api, "agentV4StartDirect")
      .mockImplementationOnce(() => oldStart.promise)
      .mockResolvedValue({ ...stateRun("new-session-run", "running", null, null, null), session_mode: "agent" });
    vi.spyOn(api, "agentV4Events").mockResolvedValue([]);

    render(<DesktopApp />);
    const firstInput = await screen.findByRole("textbox", { name: /描述研究目标/ });
    await waitFor(() => expect(firstInput).toBeEnabled());
    fireEvent.change(firstInput, { target: { value: "Old session request" } });
    await waitFor(() => expect(screen.getByRole("button", { name: "发送" })).toBeEnabled());
    fireEvent.click(screen.getByRole("button", { name: "发送" }));
    await waitFor(() => expect(start).toHaveBeenCalledTimes(1));

    fireEvent.click(screen.getByRole("button", { name: "Plan 会话" }));
    await screen.findByRole("heading", { name: "Plan 会话" });
    await act(async () => oldStart.reject(new Error("old native start failed")));
    await waitFor(() => expect(screen.queryByText("old native start failed")).not.toBeInTheDocument());

    const secondInput = screen.getByRole("textbox", { name: /描述研究目标/ });
    fireEvent.change(secondInput, { target: { value: "New session request" } });
    await waitFor(() => expect(screen.getByRole("button", { name: "发送" })).toBeEnabled());
    fireEvent.click(screen.getByRole("button", { name: "发送" }));
    await waitFor(() => expect(start).toHaveBeenCalledTimes(2));
    expect(submit).toHaveBeenCalledWith(expect.objectContaining({ conversation_id: "conversation-plan", markdown: "New session request" }));
    expect(start).toHaveBeenLastCalledWith(expect.objectContaining({ conversation_id: "conversation-plan", objective: "New session request" }));
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
      vi.mocked(api.latestUsedConversation).mockResolvedValue(conversation);
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
    vi.mocked(api.latestUsedConversation).mockResolvedValue(conversation);
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
    expect(screen.getByRole("button", { name: "终止运行" })).toBeEnabled();
    expect(screen.queryByRole("region", { name: "远程 Agent 运行控制" })).not.toBeInTheDocument();
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
    if (!screen.queryByRole("complementary")) fireEvent.click(screen.getByRole("button", { name: "展开侧栏" }));
    fireEvent.click(screen.getByRole("tab", { name: "Plan" }));
    expect(await screen.findByRole("heading", { name: statePlan.objective })).toBeInTheDocument();
    expect(await screen.findByText("修订 2 · 待审批")).toBeInTheDocument();
  });

  it("reconciles Agent events before reading the durable state after a stop", async () => {
    const { stateSpy } = setupConversationStateHarness();
    const historical = deferred<AgentRunEventV4[]>();
    vi.spyOn(api, "agentV4EventsForConversation").mockReturnValue(historical.promise);
    stateSpy.mockResolvedValue(stateSnapshot("conversation-agent", {
      latest_run: stateRun("run-after-stop", "completed", null, null, null),
    }));

    render(<DesktopApp />);
    await screen.findByRole("heading", { name: "Agent 会话" });
    await waitFor(() => expect(api.agentV4EventsForConversation).toHaveBeenCalledWith(stateProject.id, "conversation-agent"));
    expect(stateSpy).not.toHaveBeenCalled();

    historical.resolve([agentEvent("conversation-agent", "run-after-stop", 1, { kind: "run_cancelled" })]);
    await waitFor(() => expect(stateSpy).toHaveBeenCalledWith(stateProject.id, "conversation-agent"));
    expect(screen.queryByRole("button", { name: "终止运行" })).not.toBeInTheDocument();
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
    const requestStop = vi.spyOn(api, "agentV4RequestStop").mockResolvedValue(stopReceipt(stateProject.id, "conversation-agent", "new-live-run"));

    render(<DesktopApp />);
    await screen.findByRole("heading", { name: "Agent 会话" });
    await waitFor(() => expect(emit).toBeDefined());
    await act(async () => emit(agentEvent("conversation-agent", "new-live-run", 1, { kind: "model_text", text: "new live run" })));
    historical.resolve([agentEvent("conversation-agent", "old-history-run", 59, { kind: "run_created", mode: "execute" })]);
    expect(await screen.findByText("new live run")).toBeInTheDocument();
    fireEvent.click(await screen.findByRole("button", { name: "终止运行" }));
    await waitFor(() => expect(requestStop).toHaveBeenCalledWith(expect.objectContaining({ project_id: stateProject.id, conversation_id: "conversation-agent", run_id: "new-live-run" })));
  });

  it("ignores delayed stop event reconciliation after switching conversations", async () => {
    const { stateSpy } = setupConversationStateHarness();
    stateSpy.mockImplementation(async (_projectId, conversationId) => stateSnapshot(conversationId));
    const delayedEvents = deferred<AgentRunEventV4[]>();
    const events = vi.spyOn(api, "agentV4Events").mockReturnValue(delayedEvents.promise);
    vi.spyOn(api, "agentV4RequestStop").mockResolvedValue(stopReceipt(stateProject.id, "conversation-agent", "stop-run", "requested"));
    let emit!: Parameters<typeof api.onAgentV4Event>[0];
    vi.spyOn(api, "onAgentV4Event").mockImplementation(async (callback) => { emit = callback; return () => undefined; });

    render(<DesktopApp />);
    await screen.findByRole("heading", { name: "Agent 会话" });
    await waitFor(() => expect(emit).toBeDefined());
    await act(async () => emit(agentEvent("conversation-agent", "stop-run", 1, { kind: "model_text", text: "run before stop" })));
    fireEvent.click(await screen.findByRole("button", { name: "终止运行" }));
    await waitFor(() => expect(events).toHaveBeenCalledWith("stop-run"));

    fireEvent.click(screen.getByRole("button", { name: "Plan 会话" }));
    await screen.findByRole("heading", { name: "Plan 会话" });
    await act(async () => delayedEvents.resolve([agentEvent("conversation-agent", "stop-run", 2, { kind: "run_failed", message: "stale stop event" })]));

    expect(screen.queryByText("stale stop event")).not.toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Plan 会话" })).toBeInTheDocument();
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

  it("shows an uncertain dispatch as failure without carrying it into another session", async () => {
    const { stateSpy } = setupConversationStateHarness();
    stateSpy.mockImplementation(async (_projectId, conversationId) => stateSnapshot(conversationId));
    const uncertain = agentEvent("conversation-agent", "uncertain-run", 1, { kind: "tool_dispatch_uncertain", call_id: "call-old", tool_id: "runtime.execute" });
    vi.spyOn(api, "agentV4EventsForConversation").mockImplementation(async (_projectId, conversationId) => conversationId === "conversation-agent" ? [uncertain] : []);

    render(<DesktopApp />);
    expect(await screen.findByText(/失败 · 1 个步骤/)).toBeInTheDocument();
    expect(screen.queryByRole("textbox", { name: "核验证据" })).not.toBeInTheDocument();
    expect(screen.getByText("工具调用失败")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Plan 会话" }));
    await screen.findByRole("heading", { name: "Plan 会话" });

    expect(screen.queryByText("工具调用失败")).not.toBeInTheDocument();
    expect(screen.queryByRole("region", { name: "远程 Agent 运行控制" })).not.toBeInTheDocument();
  });

  it("ignores a late resume callback after switching sessions", async () => {
    const { stateSpy } = setupConversationStateHarness();
    stateSpy.mockImplementation(async (_projectId, conversationId) => stateSnapshot(conversationId));
    const failed = agentEvent("conversation-agent", "resume-run", 1, { kind: "browser_connection_required", session: "workspace", protocol_version: 1, message: "connect the extension" });
    const staleResult = agentEvent("conversation-agent", "resume-run", 2, { kind: "model_text", text: "旧会话恢复后的输出" });
    vi.spyOn(api, "agentV4EventsForConversation").mockImplementation(async (_projectId, conversationId) => conversationId === "conversation-agent" ? [failed] : []);
    const resumeDeferred = deferred<void>();
    const resume = vi.spyOn(api, "agentV4Resume").mockReturnValue(resumeDeferred.promise);
    let returnStaleEvents = false;
    const events = vi.spyOn(api, "agentV4Events").mockImplementation(async () => returnStaleEvents ? [staleResult] : []);

    render(<DesktopApp />);
    fireEvent.click(await screen.findByRole("button", { name: "已连接，继续" }));
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
    const requestStop = vi.spyOn(api, "agentV4RequestStop").mockResolvedValue(stopReceipt(stateProject.id, "conversation-agent", "run-plan"));

    render(<DesktopApp />);
    fireEvent.click(await screen.findByRole("button", { name: "取消计划" }));
    await waitFor(() => expect(requestStop).toHaveBeenCalledWith(expect.objectContaining({ project_id: stateProject.id, conversation_id: "conversation-agent", run_id: "run-plan" })));

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

  it("opens ordinary Settings on General and resets there after a capability page", async () => {
    vi.spyOn(api, "listProjects").mockResolvedValue([]);
    render(<DesktopApp />);
    fireEvent.click(await screen.findByRole("button", { name: "设置" }));

    const dialog = screen.getByRole("dialog", { name: "工作台设置" });
    expect(screen.getByRole("button", { name: "常规" })).toHaveAttribute("aria-current", "page");
    expect(dialog).toHaveTextContent("发送快捷键");
    fireEvent.click(screen.getByRole("button", { name: "模型提供方" }));
    expect(dialog).toHaveTextContent("Anthropic");
    expect(dialog).toHaveTextContent("OpenAI-compatible");
    expect(dialog).toHaveTextContent("Ollama");
    expect(dialog).toHaveTextContent("权限");
    expect(dialog).toHaveTextContent("隐私");
    expect(dialog).not.toHaveTextContent("历史运行只读");
    fireEvent.click(screen.getByRole("button", { name: "Close" }));
    fireEvent.click(screen.getByRole("button", { name: "设置" }));
    expect(screen.getByRole("button", { name: "常规" })).toHaveAttribute("aria-current", "page");
  });

  it("persists the existing project-library language switch across remounts", async () => {
    vi.spyOn(api, "listProjects").mockResolvedValue([]);
    const first = render(<DesktopApp />);

    fireEvent.click(await screen.findByRole("button", { name: "English" }));
    expect(await screen.findByRole("button", { name: "Settings" })).toBeInTheDocument();
    expect(window.localStorage.getItem("omicsops.locale")).toBe("en-US");
    first.unmount();

    render(<DesktopApp />);
    expect(await screen.findByRole("button", { name: "Settings" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "简体中文" })).toBeInTheDocument();
  });

  it("applies General language changes immediately and keeps them after Settings closes and reopens", async () => {
    vi.spyOn(api, "listProjects").mockResolvedValue([]);
    render(<DesktopApp />);
    fireEvent.click(await screen.findByRole("button", { name: "设置" }));
    fireEvent.click(screen.getByRole("button", { name: "常规" }));
    fireEvent.change(screen.getByRole("combobox", { name: "界面语言" }), { target: { value: "en-US" } });

    expect(await screen.findByRole("dialog", { name: "Workspace settings" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Close" }));
    fireEvent.click(screen.getByRole("button", { name: "Settings" }));
    fireEvent.click(screen.getByRole("button", { name: "General" }));
    expect(screen.getByRole("combobox", { name: "Interface language" })).toHaveValue("en-US");
  });

  it("restores the backend-selected latest used conversation instead of list order", async () => {
    const { stateSpy } = setupConversationStateHarness();
    stateSpy.mockImplementation(async (_projectId, conversationId) => stateSnapshot(conversationId));
    vi.mocked(api.latestUsedConversation).mockResolvedValue(stateConversations[1]);

    render(<DesktopApp />);

    expect(await screen.findByRole("heading", { name: "Plan 会话" })).toBeInTheDocument();
    expect(api.listMessages).toHaveBeenCalledWith(stateConversations[1].id);
  });

  it("creates a blank conversation when resume is enabled but the backend has no candidate", async () => {
    const { stateSpy } = setupConversationStateHarness();
    const blank = { ...stateConversations[0], id: "blank-no-candidate", title: "" };
    vi.mocked(api.latestUsedConversation).mockResolvedValue(null);
    const create = vi.spyOn(api, "createConversation").mockResolvedValue(blank);
    stateSpy.mockImplementation(async (_projectId, conversationId) => stateSnapshot(conversationId));

    render(<DesktopApp />);

    expect(await screen.findByRole("heading", { name: "新会话" })).toBeInTheDocument();
    expect(create).toHaveBeenCalledOnce();
    expect(create).toHaveBeenCalledWith(stateProject.id);
  });

  it("starts a blank conversation without querying history when resume is disabled", async () => {
    window.localStorage.setItem("omicsops.sessions.resumeLast", "false");
    const { stateSpy } = setupConversationStateHarness();
    const blank = { ...stateConversations[0], id: "blank-disabled", title: "" };
    const latest = vi.mocked(api.latestUsedConversation);
    latest.mockClear();
    const create = vi.spyOn(api, "createConversation").mockResolvedValue(blank);
    stateSpy.mockImplementation(async (_projectId, conversationId) => stateSnapshot(conversationId));

    render(<DesktopApp />);

    expect(await screen.findByRole("heading", { name: "新会话" })).toBeInTheDocument();
    expect(latest).not.toHaveBeenCalled();
    expect(create).toHaveBeenCalledWith(stateProject.id);
  });

  it("keeps the current conversation open when the resume preference changes", async () => {
    const { stateSpy } = setupConversationStateHarness();
    stateSpy.mockImplementation(async (_projectId, conversationId) => stateSnapshot(conversationId));
    const create = vi.spyOn(api, "createConversation");

    render(<DesktopApp />);
    await screen.findByRole("heading", { name: "Agent 会话" });
    fireEvent.click(screen.getByRole("button", { name: "设置" }));
    fireEvent.click(screen.getByRole("button", { name: "常规" }));
    fireEvent.click(screen.getByRole("checkbox", { name: "恢复上次会话" }));
    fireEvent.click(screen.getByRole("button", { name: "Close" }));

    expect(screen.getByRole("heading", { name: "Agent 会话" })).toBeInTheDocument();
    expect(create).not.toHaveBeenCalled();
  });

  it("shows a retry action when the latest-session query fails", async () => {
    const { stateSpy } = setupConversationStateHarness();
    vi.mocked(api.latestUsedConversation)
      .mockRejectedValueOnce(new Error("latest session unavailable"))
      .mockResolvedValueOnce(stateConversations[1]);
    stateSpy.mockImplementation(async (_projectId, conversationId) => stateSnapshot(conversationId));

    render(<DesktopApp />);

    const retry = await screen.findByRole("button", { name: "重试会话恢复" });
    expect(screen.getByRole("alert")).toHaveTextContent("无法恢复此项目的会话");
    fireEvent.click(retry);
    expect(await screen.findByRole("heading", { name: "Plan 会话" })).toBeInTheDocument();
    expect(api.latestUsedConversation).toHaveBeenCalledTimes(2);
  });

  it("ignores a late latest-session response after switching projects", async () => {
    const { stateSpy } = setupConversationStateHarness();
    const projectB = { ...stateProject, id: "project-b", name: "项目 B" };
    const conversationB = { ...stateConversations[0], id: "conversation-b", project_id: projectB.id, title: "项目 B 会话" };
    const lateA = deferred<Awaited<ReturnType<typeof api.latestUsedConversation>>>();
    vi.mocked(api.listProjects).mockResolvedValue([stateProject, projectB]);
    vi.mocked(api.listConversations).mockImplementation(async (projectId) => projectId === projectB.id ? [conversationB] : stateConversations);
    vi.mocked(api.latestUsedConversation).mockImplementation((projectId) => projectId === projectB.id ? Promise.resolve(conversationB) : lateA.promise);
    stateSpy.mockImplementation(async (projectId, conversationId) => ({ ...stateSnapshot(conversationId), project_id: projectId }));

    render(<DesktopApp />);
    await screen.findByRole("main", { name: "科研对话" });
    fireEvent.click(screen.getByRole("button", { name: "返回项目主页" }));
    fireEvent.click(await screen.findByRole("button", { name: /^项目 B/ }));
    expect(await screen.findByRole("heading", { name: "项目 B 会话" })).toBeInTheDocument();
    await act(async () => lateA.resolve(stateConversations[1]));

    expect(screen.getByRole("heading", { name: "项目 B 会话" })).toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: "Plan 会话" })).not.toBeInTheDocument();
  });

  it("clears the old conversation and disables its composer when the next project fails to load", async () => {
    const { stateSpy } = setupConversationStateHarness();
    const projectB = { ...stateProject, id: "project-b", name: "项目 B" };
    const conversationB = { ...stateConversations[0], id: "conversation-b", project_id: projectB.id, title: "项目 B 会话" };
    vi.mocked(api.listProjects).mockResolvedValue([stateProject, projectB]);
    vi.mocked(api.listConversations).mockImplementation(async (projectId) => projectId === projectB.id ? [conversationB] : stateConversations);
    vi.mocked(api.latestUsedConversation).mockImplementation(async (projectId) => {
      if (projectId === projectB.id) throw new Error("restore lookup failed");
      return stateConversations[0];
    });
    stateSpy.mockImplementation(async (projectId, conversationId) => ({ ...stateSnapshot(conversationId), project_id: projectId }));

    render(<DesktopApp />);
    await screen.findByRole("heading", { name: "Agent 会话" });
    fireEvent.click(screen.getByRole("button", { name: "返回项目主页" }));
    fireEvent.click(await screen.findByRole("button", { name: /^项目 B/ }));
    await screen.findByRole("button", { name: "重试会话恢复" });

    expect(screen.queryByRole("heading", { name: "Agent 会话" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Agent 会话" })).not.toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: /描述研究目标/ })).toBeDisabled();
  });

  it("deduplicates blank conversation creation during StrictMode effect replay", async () => {
    const { stateSpy } = setupConversationStateHarness();
    const pending = deferred<Awaited<ReturnType<typeof api.createConversation>>>();
    vi.mocked(api.listConversations).mockResolvedValue([]);
    vi.mocked(api.latestUsedConversation).mockResolvedValue(null);
    const create = vi.spyOn(api, "createConversation").mockReturnValue(pending.promise);
    stateSpy.mockImplementation(async (_projectId, conversationId) => stateSnapshot(conversationId));

    render(<StrictMode><DesktopApp /></StrictMode>);
    await waitFor(() => expect(create).toHaveBeenCalledOnce());
    await act(async () => pending.resolve({ ...stateConversations[0], id: "strict-blank", title: "" }));

    expect(await screen.findByRole("heading", { name: "新会话" })).toBeInTheDocument();
    expect(create).toHaveBeenCalledOnce();
  });

  it("creates and switches to an empty conversation when New conversation is clicked", async () => {
    const project = { id: "project-1", name: "PBMC 项目", description: "", local_root: "E:/Science/pbmc", remote_root: null, connection_id: null, template: "single_cell_rna_seq" as const, status: "running" as const, ollama_only: false, created_at: "2026-08-11T00:00:00Z", updated_at: "2026-08-11T00:00:00Z" };
    const existing = { id: "conversation-1", project_id: project.id, title: "旧问题", status: "idle" as const, model_profile_id: null, created_at: "2026-08-11T00:00:00Z", updated_at: "2026-08-11T00:00:00Z" };
    const created = { ...existing, id: "conversation-2", title: "", created_at: "2026-08-12T00:00:00Z", updated_at: "2026-08-12T00:00:00Z" };
    vi.spyOn(api, "listProjects").mockResolvedValue([project]);
    vi.spyOn(api, "listConversations").mockResolvedValue([existing]);
    vi.mocked(api.latestUsedConversation).mockResolvedValue(existing);
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
    vi.mocked(api.latestUsedConversation).mockResolvedValue(active);
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
    vi.mocked(api.latestUsedConversation).mockImplementation(async (projectId) => projectId === projectA.id ? conversationA : conversationB);
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
    if (!screen.queryByRole("complementary")) fireEvent.click(screen.getByRole("button", { name: "展开侧栏" }));
    if (!screen.queryByRole("menu", { name: "侧栏内容" })) fireEvent.click(screen.getByRole("button", { name: "添加侧栏标签" }));
    fireEvent.click(screen.getByRole("menuitemcheckbox", { name: "Environment" }));
    expect(screen.queryByText("旧项目 kernel 泄漏")).not.toBeInTheDocument();
    if (!screen.queryByRole("complementary")) fireEvent.click(screen.getByRole("button", { name: "展开侧栏" }));
    if (!screen.queryByRole("menu", { name: "侧栏内容" })) fireEvent.click(screen.getByRole("button", { name: "添加侧栏标签" }));
    fireEvent.click(screen.getByRole("menuitemcheckbox", { name: "Files" }));
    expect(screen.queryByText("旧项目同步泄漏.txt")).not.toBeInTheDocument();
  });
});

it("streams public preview only for the current conversation and clears on commit", async () => {
  // Covered through the app subscription, not just the presentation component.
  const { stateSpy } = setupConversationStateHarness();
  stateSpy.mockResolvedValue(stateSnapshot("conversation-agent", { latest_run: { ...stateRun("live-run", "running", null, null, null), session_mode: "agent" } }));
  let emit!: Parameters<typeof api.onAgentV4Event>[0];
  let preview!: Parameters<typeof api.onAgentV4TextPreview>[0];
  vi.spyOn(api, "onAgentV4Event").mockImplementation(async (cb) => { emit = cb; return () => undefined; });
  vi.spyOn(api, "onAgentV4TextPreview").mockImplementation(async (cb) => { preview = cb; return () => undefined; });
  render(<DesktopApp />);
  await screen.findByRole("heading", { name: "Agent 会话" });
  await waitFor(() => expect(preview).toBeDefined());
  await act(async () => emit(agentEvent("conversation-agent", "live-run", 1, { kind: "run_created", mode: "execute" })));
  await act(async () => preview({ run_id: "wrong-run", text: "foreign preview" }));
  expect(screen.queryByText("foreign preview")).not.toBeInTheDocument();
  await act(async () => preview({ run_id: "live-run", text: "Live progress" }));
  expect(screen.getByRole("article", { name: "模型实时输出" })).toHaveTextContent("Live progress");
  await act(async () => emit(agentEvent("conversation-agent", "live-run", 2, { kind: "model_text", text: "Live progress" })));
  expect(screen.getAllByText("Live progress")).toHaveLength(1);
});

it("routes native idle sends through the durable queue and recovers the committed message", async () => {
  const { stateSpy } = setupConversationStateHarness();
  stateSpy.mockImplementation(async (_projectId, conversationId) => stateSnapshot(conversationId));
  for (const name of ["onConversationEvent", "onConversationUpdated", "onSyncEvent", "onKernelEvent", "onAgentV4Event", "onAgentV4TextPreview"] as const) vi.spyOn(api, name).mockResolvedValue(() => {});
  vi.spyOn(api, "getConversationCapabilitiesV4").mockResolvedValue({ project_id: stateProject.id, conversation_id: stateConversations[0].id, skills: [], mcp_servers: [], memory_count: 0 });
  vi.spyOn(preferencesApi, "getConversationAgentPreferencesV4").mockResolvedValue({ delegation_enabled: true, auto_review: true, memory_enabled: true });
  vi.spyOn(queueApi, "reconcileComposerQueue").mockResolvedValue([]);
  const submit = vi.spyOn(api, "submitMessage");
  const start = vi.spyOn(api, "agentV4StartDirect");
  const enqueue = vi.spyOn(queueApi, "enqueueComposerTurn").mockImplementation(async (request) => {
    vi.mocked(api.listMessages).mockResolvedValue([{ id: request.message_id, project_id: request.project_id, conversation_id: request.conversation_id, sequence: 1, role: "user", markdown: request.message_markdown, created_at: "2026-09-14T00:00:00Z" }]);
    return { ...request, position: 1, revision: 2, status: "running", frozen: { model_profile_id: request.model_profile_id, model_configuration_hash: "hash", compute_selection: request.compute_selection, conversation_preferences: { delegation_enabled: true, auto_review: true, memory_enabled: true }, service_tier: {}, delegated_model: null, reviewer_model: null }, attachment_receipts: [], created_at: "now", updated_at: "now" };
  });
  Object.defineProperty(window, "__TAURI_INTERNALS__", { configurable: true, value: { invoke: vi.fn().mockResolvedValue([]) } });
  const view = render(<DesktopApp />);
  try {
    const input = await screen.findByRole("textbox", { name: /描述研究目标/ });
    await waitFor(() => expect(input).toBeEnabled());
    fireEvent.change(input, { target: { value: "原子提交的研究任务" } });
    await waitFor(() => expect(screen.getByRole("button", { name: "发送" })).toBeEnabled());
    fireEvent.keyDown(input, { key: "Enter" });
    await waitFor(() => expect(enqueue).toHaveBeenCalledTimes(1));
    expect(submit).not.toHaveBeenCalled(); expect(start).not.toHaveBeenCalled();
    await waitFor(() => expect(input).toHaveValue(""));
    await waitFor(() => expect(screen.getAllByText("原子提交的研究任务")).toHaveLength(1));
  } finally { view.unmount(); Reflect.deleteProperty(window, "__TAURI_INTERNALS__"); }
});
