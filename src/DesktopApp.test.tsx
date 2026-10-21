import * as contextApi from "./context-usage-api";
import type { ContextCompactionReceiptV4 } from "./types";
import * as queueApi from "./composer-queue-api";
import * as preferencesApi from "./conversation-preferences-api";
import { StrictMode, type ReactNode } from "react";
import { act, fireEvent, render as testingRender, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import DesktopApp, { samePendingSubmission } from "./DesktopApp";
import { ComposerDraftStore } from "./features/workspace/composerDrafts";
import { AppearanceProvider } from "./use-appearance";
import * as api from "./tauri-api";
import * as offlineExampleApi from "./offline-example-api";
import fixture from "../examples/offline/synthetic-evidence-report-v1.json";
import type { OfflineExampleWorkspaceView } from "./types";
import * as referenceApi from "./composer-reference-api";
import * as attachmentApi from "./composer-attachment-api";
import * as usageApi from "./usage-settings-api";
import * as navigationApi from "./workspace-navigation-api";
import type { ConversationVisibilityRecordV1, SetConversationVisibilityRequestV1 } from "./workspace-navigation-types";
import * as remoteJobsPanel from "./features/workspace/RemoteJobsPanel";
import * as workspaceShell from "./features/workspace/WorkspaceShell";
import * as settingsPanel from "./features/settings/SettingsPanel";
import type { AgentRunEventV4, ComposerReference, ConversationAgentStateV4, ExecutionPlanV4, KernelEvent, ProposedPlanRevisionV4, RunSummaryV4, StopRunReceiptV4, SyncEntry, WorkspaceMessage } from "./types";

const render = (ui: ReactNode) => testingRender(ui, { wrapper: AppearanceProvider });

beforeEach(() => {
  vi.restoreAllMocks();
  window.localStorage.clear();
  vi.spyOn(api, "latestUsedConversation").mockResolvedValue(null);
  vi.spyOn(offlineExampleApi, "getOfflineExampleWorkspace").mockResolvedValue(null);
  vi.spyOn(api, "getConversationVisibility").mockImplementation(async (projectId, conversationId) => ({ project_id: projectId, conversation_id: conversationId, visibility: "visible", revision: 0, changed_at: null }));
  vi.spyOn(api, "getWorkspaceConversation").mockImplementation(async (projectId, conversationId) => {
    const item = (await api.listConversations(projectId)).find((value) => value.id === conversationId);
    if (!item) throw { code: "not_found" };
    return item;
  });
  vi.spyOn(navigationApi, "getConversationVisibility").mockImplementation((projectId, conversationId) => api.getConversationVisibility(projectId, conversationId));
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

const offlineProject = { ...stateProject, id: "example-project", name: "离线合成示例", template: "blank" as const, status: "ready" as const };
function offlineView(projectId = offlineProject.id): OfflineExampleWorkspaceView {
  return { schema_version: 1, project_id: projectId, example_id: fixture.example_id, source_kind: "synthetic", availability: "available", unavailable_reason: null, title: fixture.title, disclaimer: fixture.disclaimer, source_description: fixture.source_description, license_spdx: fixture.license_spdx, fixture_sha256: "fixture", entries: fixture.entries as OfflineExampleWorkspaceView["entries"], files: [] };
}

function forbidOfflineDispatch() {
  const spies = [
    vi.spyOn(api, "createConversation"), vi.spyOn(api, "agentV4StartDirect"), vi.spyOn(api, "agentV4StartPlanning"),
    vi.spyOn(api, "agentV4ComputeBackends"), vi.spyOn(api, "testConnection"), vi.spyOn(api, "listRemoteFiles"),
    vi.spyOn(api, "startKernel"), vi.spyOn(api, "probeModelProfile"), vi.spyOn(window, "fetch"),
  ];
  for (const spy of spies) spy.mockImplementation(() => { throw new Error("offline candidate initialization forbidden"); });
  return () => { for (const spy of spies) expect(spy).not.toHaveBeenCalled(); };
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

describe("project compute selection", () => {
  const remoteProject = { ...stateProject, connection_id: "remote-a", remote_root: "/work/project" };
  const remoteBackend = { ...stateBackend, descriptor: { ...stateBackend.descriptor, backend_id: "ssh:remote-a", kind: "ssh" as const }, python_status: "unverified" as const, r_status: "unverified" as const };

  function harness() {
    const { stateSpy } = setupConversationStateHarness();
    stateSpy.mockImplementation(async (projectId, id) => ({ ...stateSnapshot(id), project_id: projectId }));
    vi.mocked(api.listProjects).mockResolvedValue([remoteProject]);
    vi.mocked(api.agentV4ComputeBackends).mockResolvedValue([stateBackend, remoteBackend]);
    return vi.spyOn(workspaceShell, "WorkspaceShell");
  }

  it("defaults a newly opened SSH-bound project to SSH instead of the initial local backend", async () => {
    const shell = harness();
    render(<DesktopApp />);
    await screen.findByText(/Agent 模式：SSH/);
    expect(shell.mock.calls.at(-1)?.[0].runtimeDraftSelection).toMatchObject({ backend_id: "ssh:remote-a", backend_kind: "ssh" });
  });

  it("keeps an explicit Local choice when changing conversations and refreshing the same project's catalog", async () => {
    const shell = harness();
    render(<DesktopApp />);
    await screen.findByText(/Agent 模式：SSH/);
    fireEvent.click(screen.getByRole("button", { name: "选择计算后端" }));
    fireEvent.click(screen.getByRole("radio", { name: /LOCAL/ }));
    fireEvent.keyDown(window, { key: "Escape" });
    fireEvent.click(screen.getByRole("button", { name: stateConversations[1].title }));
    await screen.findByRole("heading", { name: stateConversations[1].title });
    const pending = deferred<Awaited<ReturnType<typeof api.agentV4ComputeBackends>>>();
    vi.mocked(api.agentV4ComputeBackends).mockReturnValueOnce(pending.promise);
    act(() => shell.mock.calls.at(-1)![0].onContainerImageChange?.("catalog-refresh"));
    const composer = screen.getByRole("textbox", { name: /描述研究目标/ });
    expect(composer).toBeEnabled();
    fireEvent.change(composer, { target: { value: "检查环境" } });
    expect(screen.getByRole("button", { name: "发送" })).toBeDisabled();
    expect(shell.mock.calls.at(-1)?.[0].runtimeDraftSelection).toBeNull();
    await waitFor(() => expect(api.agentV4ComputeBackends).toHaveBeenCalledWith(stateProject.id, "catalog-refresh"));
    await act(async () => pending.resolve([stateBackend, remoteBackend]));
    await waitFor(() => expect(shell.mock.calls.at(-1)?.[0].runtimeDraftSelection).toMatchObject({ backend_id: "local", backend_kind: "local" }));
  });

  it.each(["chat", "plan"] as const)("blocks %s submission until a switched project's compute catalog resolves", async (mode) => {
    const shell = harness();
    const otherProject = { ...remoteProject, id: "remote-project-b", name: "远端项目 B", connection_id: "remote-b" };
    const otherConversation = { ...stateConversations[0], id: "remote-conversation-b", project_id: otherProject.id, title: "远端会话 B" };
    const otherBackend = { ...remoteBackend, descriptor: { ...remoteBackend.descriptor, backend_id: "ssh:remote-b" } };
    const pending = deferred<Awaited<ReturnType<typeof api.agentV4ComputeBackends>>>();
    vi.mocked(api.listProjects).mockResolvedValue([stateProject, otherProject]);
    vi.mocked(api.listConversations).mockImplementation(async (id) => id === stateProject.id ? stateConversations : [otherConversation]);
    vi.mocked(api.latestUsedConversation).mockImplementation(async (id) => id === stateProject.id ? stateConversations[0] : otherConversation);
    vi.mocked(api.agentV4ComputeBackends).mockImplementation((id) => id === stateProject.id ? Promise.resolve([stateBackend]) : pending.promise);
    const submit = vi.spyOn(api, "submitMessage");
    const direct = vi.spyOn(api, "agentV4StartDirect");
    const plan = vi.spyOn(api, "agentV4StartPlanning");
    render(<DesktopApp />);
    await screen.findByText(/Agent 模式：LOCAL/);
    fireEvent.click(screen.getByRole("button", { name: "返回项目主页" }));
    fireEvent.click(await screen.findByRole("button", { name: "打开项目：远端项目 B" }));
    await screen.findByRole("heading", { name: "远端会话 B" });
    await waitFor(() => expect(shell.mock.calls.at(-1)?.[0].conversationHydrating).toBe(false));
    fireEvent.change(screen.getByRole("textbox", { name: /描述研究目标/ }), { target: { value: "检查远端环境" } });
    expect(screen.getByRole("button", { name: "发送" })).toBeDisabled();
    await act(async () => { expect(await shell.mock.calls.at(-1)![0].onSend?.("检查远端环境", mode)).toBe(false); });
    expect(submit).not.toHaveBeenCalled();
    expect(direct).not.toHaveBeenCalled();
    expect(plan).not.toHaveBeenCalled();
    await act(async () => pending.resolve([stateBackend, otherBackend]));
    await screen.findByText(/Agent 模式：SSH/);
    expect(shell.mock.calls.at(-1)?.[0].runtimeDraftSelection).toMatchObject({ backend_id: "ssh:remote-b" });
    expect(screen.getByRole("button", { name: "发送" })).toBeEnabled();
  });

  it("keeps the container image editable during debounce and freezes only the final verified image", async () => {
    const shell = harness();
    const docker = { ...stateBackend, descriptor: { ...stateBackend.descriptor, backend_id: "docker", kind: "docker" as const, isolation: "container" as const, supports_network_policy: true }, resolved_image_id: "sha256:previous" };
    const pending = deferred<Awaited<ReturnType<typeof api.agentV4ComputeBackends>>>();
    vi.mocked(api.agentV4ComputeBackends).mockImplementation((_id, image) => image ? pending.promise : Promise.resolve([stateBackend, docker]));
    render(<DesktopApp />);
    await screen.findByText(/Agent 模式：LOCAL/);
    fireEvent.change(screen.getByRole("textbox", { name: /描述研究目标/ }), { target: { value: "检查容器环境" } });
    fireEvent.click(screen.getByRole("button", { name: "选择计算后端" }));
    fireEvent.click(screen.getByRole("radio", { name: /DOCKER/ }));
    const image = screen.getByRole("textbox", { name: "容器镜像" });
    for (const value of ["p", "py", "python:verified"]) {
      fireEvent.change(image, { target: { value } });
      expect(image).toBeEnabled();
      expect(screen.getByRole("button", { name: "发送" })).toBeDisabled();
      expect(shell.mock.calls.at(-1)?.[0].runtimeDraftSelection).toBeNull();
    }
    await waitFor(() => expect(api.agentV4ComputeBackends).toHaveBeenCalledWith(stateProject.id, "python:verified"));
    expect(vi.mocked(api.agentV4ComputeBackends).mock.calls.map((call) => call[1])).toEqual(["", "python:verified"]);
    expect(screen.getByRole("button", { name: "发送" })).toBeDisabled();
    await act(async () => pending.resolve([{ ...docker, resolved_image_id: "sha256:verified" }]));
    await waitFor(() => expect(shell.mock.calls.at(-1)?.[0].runtimeDraftSelection).toMatchObject({ backend_id: "docker", container_image: { reference: "python:verified", image_id: "sha256:verified" } }));
    expect(screen.getByRole("button", { name: "发送" })).toBeEnabled();
  });
});

describe("project data refresh", () => {
  function harness() {
    const { stateSpy } = setupConversationStateHarness();
    stateSpy.mockImplementation(async (projectId, id) => ({ ...stateSnapshot(id), project_id: projectId }));
    const memory = vi.spyOn(api, "searchAgentMemory").mockResolvedValue([]);
    const notebook = vi.spyOn(api, "listNotebookEntries").mockResolvedValue([]);
    const artifacts = vi.spyOn(api, "listProjectArtifacts").mockResolvedValue([]);
    const transfers = vi.spyOn(api, "listSyncEntries").mockResolvedValue([]);
    const shell = vi.spyOn(workspaceShell, "WorkspaceShell");
    let listener!: (event: AgentRunEventV4) => void;
    vi.spyOn(api, "onAgentV4Event").mockImplementation(async (callback) => { listener = callback; return () => {}; });
    return { memory, notebook, artifacts, transfers, shell, emit: (sequence: number, event: AgentRunEventV4["event"]) => listener(agentEvent(stateConversations[0].id, "data-run", sequence, event)) };
  }

  const finished = { kind: "tool_finished" as const, outcome: { call_id: "save", tool_id: "artifact.record", succeeded: true, model_content: "saved", data: {}, provenance: [] } };
  const artifact = (projectId: string) => ({ id: `artifact-${projectId}`, project_id: projectId, run_id: null, relative_path: "result.tsv", remote_path: null, media_type: "text/tab-separated-values", size_bytes: 10, sha256: "hash", verified: true, created_at: "2026-10-21T00:00:00Z" });

  it("does not query all project data for model progress events", async () => {
    const test = harness();
    render(<DesktopApp />);
    await screen.findByRole("heading", { name: "Agent 会话" });
    const reads = [test.memory, test.notebook, test.artifacts, test.transfers];
    reads.forEach((read) => expect(read).toHaveBeenCalledTimes(1));
    for (let sequence = 1; sequence <= 12; sequence++) {
      await act(async () => test.emit(sequence, { kind: "model_text", text: `progress ${sequence}` }));
    }
    reads.forEach((read) => expect(read).toHaveBeenCalledTimes(1));
  });

  it("coalesces pending data refreshes and still reads changes arriving during the request", async () => {
    const test = harness();
    const pending = deferred<import("./types").ProjectArtifact[]>();
    const latest = artifact(stateProject.id);
    render(<DesktopApp />);
    await screen.findByRole("heading", { name: "Agent 会话" });
    test.artifacts.mockReturnValueOnce(pending.promise).mockResolvedValue([latest]);
    await act(async () => test.emit(1, finished));
    for (let sequence = 2; sequence <= 12; sequence++) await act(async () => test.emit(sequence, finished));
    const reads = [test.memory, test.notebook, test.artifacts, test.transfers];
    reads.forEach((read) => expect(read).toHaveBeenCalledTimes(2));
    await act(async () => pending.resolve([]));
    await waitFor(() => expect(test.shell.mock.calls.at(-1)?.[0].projectArtifacts).toEqual([latest]));
    reads.forEach((read) => expect(read).toHaveBeenCalledTimes(3));
  });

  it("waits for the whole read batch after one query fails before retrying pending changes", async () => {
    const test = harness();
    const pending = deferred<import("./types").ProjectArtifact[]>();
    render(<DesktopApp />);
    await screen.findByRole("heading", { name: "Agent 会话" });
    test.memory.mockRejectedValueOnce(new Error("memory read failed"));
    test.artifacts.mockReturnValueOnce(pending.promise);
    await act(async () => test.emit(1, finished));
    await act(async () => test.emit(2, finished));
    expect(test.artifacts).toHaveBeenCalledTimes(2);
    expect(test.memory).toHaveBeenCalledTimes(2);
    await act(async () => pending.resolve([]));
    await waitFor(() => expect(test.artifacts).toHaveBeenCalledTimes(3));
    expect(test.memory).toHaveBeenCalledTimes(3);
    expect(screen.queryByText("memory read failed")).not.toBeInTheDocument();
    expect(screen.queryByText("对话未完成")).not.toBeInTheDocument();
  });

  it.each(["success", "failure"] as const)("discards a project data %s after changing projects", async (result) => {
    const test = harness();
    const projectB = { ...stateProject, id: "project-data-b", name: "项目 B" };
    const conversationB = { ...stateConversations[0], id: "conversation-data-b", project_id: projectB.id, title: "项目 B 会话" };
    vi.mocked(api.listProjects).mockResolvedValue([stateProject, projectB]);
    vi.mocked(api.listConversations).mockImplementation(async (projectId) => projectId === projectB.id ? [conversationB] : stateConversations);
    vi.mocked(api.latestUsedConversation).mockImplementation(async (projectId) => projectId === projectB.id ? conversationB : stateConversations[0]);
    const old = deferred<import("./types").ProjectArtifact[]>();
    test.artifacts.mockImplementation((projectId) => projectId === stateProject.id ? old.promise : Promise.resolve([artifact(projectB.id)]));
    render(<DesktopApp />);
    await screen.findByRole("heading", { name: "Agent 会话" });
    fireEvent.click(screen.getByRole("button", { name: "返回项目主页" }));
    fireEvent.click(await screen.findByRole("button", { name: "打开项目：项目 B" }));
    await screen.findByRole("heading", { name: "项目 B 会话" });
    await waitFor(() => expect(test.shell.mock.calls.at(-1)?.[0].projectArtifacts).toEqual([artifact(projectB.id)]));
    await act(async () => result === "success" ? old.resolve([artifact(stateProject.id)]) : old.reject(new Error("old project failure")));
    expect(test.shell.mock.calls.at(-1)?.[0].projectArtifacts).toEqual([artifact(projectB.id)]);
    expect(screen.queryByText("old project failure")).not.toBeInTheDocument();
  });

  it("shows a recoverable project data error separately from a failed conversation", async () => {
    const test = harness();
    test.artifacts.mockRejectedValueOnce(new Error("project data pool timeout"));
    render(<DesktopApp />);
    await screen.findByText("project data pool timeout");
    expect(screen.getByText("项目资料暂时无法刷新")).toBeInTheDocument();
    expect(screen.queryByText("对话未完成")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "重试加载项目资料" }));
    await waitFor(() => expect(screen.queryByText("project data pool timeout")).not.toBeInTheDocument());
    expect(test.artifacts).toHaveBeenCalledTimes(2);
  });
});

describe("remote file listing state", () => {
  it("keeps listing failures out of transfer notices and clears the error after a successful retry", async () => {
    const { stateSpy } = setupConversationStateHarness();
    stateSpy.mockImplementation(async (_projectId, id) => stateSnapshot(id));
    vi.mocked(api.listProjects).mockResolvedValue([{ ...stateProject, connection_id: "ssh-a", remote_root: "/project" }]);
    const retry = deferred<import("./types").RemoteFileEntry[]>();
    vi.spyOn(api, "listRemoteFiles").mockRejectedValueOnce(new Error("remote_root_unavailable: PRIVATE_DETAILS")).mockReturnValueOnce(retry.promise);
    const shell = vi.spyOn(workspaceShell, "WorkspaceShell");
    render(<DesktopApp />);
    await waitFor(() => expect(shell.mock.calls.at(-1)?.[0].remoteFilesStatus).toBe("error"));
    expect(shell.mock.calls.at(-1)![0].remoteFilesError).toBe("remote_root_unavailable");
    expect(shell.mock.calls.at(-1)![0].remoteFiles).toEqual([]);
    expect(shell.mock.calls.at(-1)![0].fileNotice).toBe("");
    expect(screen.queryByText(/PRIVATE_DETAILS/)).not.toBeInTheDocument();

    act(() => { void shell.mock.calls.at(-1)![0].onRefreshFiles?.(); });
    await waitFor(() => expect(shell.mock.calls.at(-1)?.[0].remoteFilesStatus).toBe("loading"));
    expect(shell.mock.calls.at(-1)![0].remoteFilesError).toBeUndefined();
    const entry = { relative_path: "result.tsv", directory: false, size_bytes: 9, modified_unix_seconds: 0 };
    await act(async () => retry.resolve([entry]));
    await waitFor(() => expect(shell.mock.calls.at(-1)?.[0].remoteFilesStatus).toBe("ready"));
    expect(shell.mock.calls.at(-1)![0].remoteFiles).toEqual([entry]);
    expect(shell.mock.calls.at(-1)![0].remoteFilesError).toBeUndefined();
    expect(shell.mock.calls.at(-1)![0].fileNotice).toBe("");
  });

  it("preserves the upload success notice when its subsequent listing fails", async () => {
    const { stateSpy } = setupConversationStateHarness();
    stateSpy.mockImplementation(async (_projectId, id) => stateSnapshot(id));
    vi.mocked(api.listProjects).mockResolvedValue([{ ...stateProject, connection_id: "ssh-a", remote_root: "/project" }]);
    const uploaded: SyncEntry = { id: "uploaded", project_id: stateProject.id, relative_path: "result.tsv", local_relative_path: "result.tsv", remote_path: "/project/result.tsv", direction: "local_to_remote", size_bytes: 9, sha256: "verified", state: "synced", transferred_bytes: 9, retry_count: 0, error: null, updated_at: "2026-10-21T00:00:00Z" };
    vi.spyOn(api, "chooseProjectFiles").mockResolvedValue(["result.tsv"]);
    vi.spyOn(api, "uploadSelectedFiles").mockResolvedValue([uploaded]);
    vi.spyOn(api, "listSyncEntries").mockResolvedValue([uploaded]);
    vi.spyOn(api, "listRemoteFiles").mockResolvedValueOnce([]).mockRejectedValueOnce(new Error("PRIVATE_LISTING_ERROR"));
    const shell = vi.spyOn(workspaceShell, "WorkspaceShell");
    render(<DesktopApp />);
    await waitFor(() => expect(shell.mock.calls.at(-1)?.[0].remoteFilesStatus).toBe("ready"));
    await act(async () => shell.mock.calls.at(-1)![0].onUploadFiles?.());
    expect(shell.mock.calls.at(-1)![0].remoteFilesStatus).toBe("error");
    expect(shell.mock.calls.at(-1)![0].fileNotice).toMatch(/已校验上传 1 个文件|1 uploaded files verified/);
    expect(shell.mock.calls.at(-1)![0].filesBusy).toBe(false);
    expect(screen.queryByText(/PRIVATE_LISTING_ERROR/)).not.toBeInTheDocument();
  });
});

describe("DesktopApp", () => {
  function visibilityHarness() {
    const { stateSpy } = setupConversationStateHarness();
    stateSpy.mockImplementation(async (_id, id) => stateSnapshot(id));
    let current: ConversationVisibilityRecordV1 = { project_id: stateProject.id, conversation_id: stateConversations[0].id, visibility: "visible", revision: 0, changed_at: null };
    vi.mocked(api.getConversationVisibility).mockImplementation(async (projectId, id) => id === current.conversation_id ? { ...current } : { project_id: projectId, conversation_id: id, visibility: "visible", revision: 0, changed_at: null });
    vi.mocked(api.listConversations).mockImplementation(async () => stateConversations.filter((item) => item.id !== current.conversation_id || current.visibility === "visible"));
    vi.mocked(api.getWorkspaceConversation).mockImplementation(async (_projectId, id) => stateConversations.find((item) => item.id === id)!);
    vi.spyOn(navigationApi, "listConversationVisibility").mockImplementation(async () => ({ items: current.visibility === "shelved" ? [{ state: { ...current }, title: stateConversations[0].title, conversation_status: "running", conversation_created_at: stateConversations[0].created_at, conversation_updated_at: stateConversations[0].updated_at }] : [], next_offset: null }));
    const set = vi.spyOn(api, "setConversationVisibility").mockImplementation(async (request) => {
      current = { ...current, visibility: request.visibility, revision: current.revision + 1 };
      return { request_id: request.request_id, state: { ...current }, changed: true };
    });
    return { set, change: (value: ConversationVisibilityRecordV1) => { current = value; }, get: () => current };
  }

  it("visibility_running_stays_open and collection uses immediate window Escape", async () => {
    const { set } = visibilityHarness();
    const cancel = vi.spyOn(api, "agentV4Cancel");
    const kernel = vi.spyOn(api, "startKernel");
    vi.mocked(api.agentV4ConversationState).mockImplementation(async (_id, id) => stateSnapshot(id, { latest_run: stateRun("running", "running", null, null, null), locked: true }));
    vi.mocked(api.listMessages).mockResolvedValue([{ id: "running-message", project_id: stateProject.id, conversation_id: stateConversations[0].id, sequence: 1, role: "assistant", markdown: "running transcript", created_at: "now" }]);
    render(<DesktopApp />);
    await screen.findByText("running transcript");
    const draft = screen.getByRole("textbox", { name: /描述研究目标/ });
    fireEvent.change(draft, { target: { value: "retained draft" } });
    fireEvent.click(screen.getByRole("button", { name: `收起会话：${stateConversations[0].title}` }));
    await screen.findByText("此会话已收起。消息和正在运行的计算会继续保留。");
    expect(screen.getByText("running transcript")).toBeVisible();
    expect(draft).toHaveValue("retained draft");
    expect(within(screen.getByRole("navigation", { name: "项目与会话" })).queryByRole("button", { name: stateConversations[0].title })).toBeNull();
    expect(cancel).not.toHaveBeenCalled(); expect(kernel).not.toHaveBeenCalled(); expect(set).toHaveBeenCalledTimes(1);
    fireEvent.click(screen.getByRole("button", { name: "收起的会话" }));
    await screen.findByRole("dialog", { name: "收起的会话" });
    fireEvent.keyDown(window, { key: "Escape" });
    expect(screen.queryByRole("dialog", { name: "收起的会话" })).toBeNull();
  });

  it("visibility_lost_reply_requery keeps the immutable request and historical receipts cannot override current restore", async () => {
    const { set, change, get } = visibilityHarness();
    let first!: SetConversationVisibilityRequestV1;
    set.mockImplementationOnce(async (request) => { first = request; change({ ...get(), visibility: "shelved", revision: 1 }); throw new Error("reply lost"); });
    render(<DesktopApp />); await screen.findByRole("heading", { name: stateConversations[0].title });
    fireEvent.click(screen.getByRole("button", { name: `收起会话：${stateConversations[0].title}` }));
    await screen.findByText("可见性操作失败。请重试同一次操作。");
    change({ ...get(), visibility: "visible", revision: 2 });
    set.mockImplementationOnce(async (request) => ({ request_id: request.request_id, changed: true, state: { ...get(), visibility: "shelved", revision: 1 } }));
    fireEvent.click(screen.getByRole("button", { name: `收起会话：${stateConversations[0].title}` }));
    await waitFor(() => expect(set).toHaveBeenCalledTimes(2));
    expect(set.mock.calls[1][0]).toEqual(first);
    await waitFor(() => expect(screen.queryByText("此会话已收起。消息和正在运行的计算会继续保留。")).toBeNull());
    expect(api.getConversationVisibility).toHaveBeenCalledWith(stateProject.id, stateConversations[0].id);
  });

  it("visibility_direct_source_open loads a shelved exact ID outside the Visible list", async () => {
    const { change, get } = visibilityHarness();
    vi.spyOn(referenceApi, "composerReferenceCatalog").mockResolvedValue([{ reference: { kind: "session", project_id: stateProject.id, id: stateConversations[0].id }, label: "Saved hidden source", description: "historical source" }]);
    render(<DesktopApp />); await screen.findByRole("heading", { name: stateConversations[0].title });
    fireEvent.click(screen.getByRole("button", { name: stateConversations[1].title }));
    await screen.findByRole("heading", { name: stateConversations[1].title });
    change({ ...get(), visibility: "shelved", revision: 1 });
    fireEvent.keyDown(window, { key: "k", ctrlKey: true });
    fireEvent.click(await screen.findByRole("button", { name: "打开 Saved hidden source" }));
    await screen.findByRole("heading", { name: stateConversations[0].title });
    await screen.findByText("此会话已收起。消息和正在运行的计算会继续保留。");
    expect(api.getWorkspaceConversation).toHaveBeenCalledWith(stateProject.id, stateConversations[0].id);
  });

  it("keeps the newer restored state when an escaped same-active exact open completes last", async () => {
    const { get } = visibilityHarness();
    const shell = vi.spyOn(workspaceShell, "WorkspaceShell");
    render(<DesktopApp />); await screen.findByRole("heading", { name: stateConversations[0].title });
    fireEvent.click(screen.getByRole("button", { name: `收起会话：${stateConversations[0].title}` }));
    await screen.findByRole("button", { name: "恢复会话" });
    fireEvent.click(screen.getByRole("button", { name: "收起的会话" }));
    const dialog = await screen.findByRole("dialog", { name: "收起的会话" });
    const oldList = deferred<typeof stateConversations>();
    const reads = vi.mocked(api.listConversations).mock.calls.length;
    vi.mocked(api.listConversations).mockReturnValueOnce(oldList.promise);
    fireEvent.click(await within(dialog).findByRole("button", { name: "打开" }));
    await waitFor(() => expect(api.listConversations).toHaveBeenCalledTimes(reads + 1));
    fireEvent.keyDown(window, { key: "Escape" });
    expect(screen.queryByRole("dialog", { name: "收起的会话" })).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "恢复会话" }));
    await waitFor(() => expect(screen.queryByRole("button", { name: "恢复会话" })).toBeNull());
    expect(shell.mock.calls.at(-1)![0].activeConversationVisibility?.revision).toBe(2);
    await act(async () => oldList.resolve([stateConversations[1]]));
    expect(get().visibility).toBe("visible");
    expect(shell.mock.calls.at(-1)![0].activeConversationVisibility?.revision).toBe(2);
    expect(screen.queryByRole("button", { name: "恢复会话" })).toBeNull();
    expect(screen.getByRole("button", { name: stateConversations[0].title })).toBeVisible();
  });

  it("opens the exact branch while discarding its list captured before a completed Restore", async () => {
    const { get, set } = visibilityHarness();
    const shell = vi.spyOn(workspaceShell, "WorkspaceShell");
    render(<DesktopApp />); await screen.findByRole("heading", { name: stateConversations[0].title });
    fireEvent.click(screen.getByRole("button", { name: `收起会话：${stateConversations[0].title}` }));
    await screen.findByRole("button", { name: "恢复会话" });
    const oldList = deferred<typeof stateConversations>();
    const reads = vi.mocked(api.listConversations).mock.calls.length;
    vi.mocked(api.listConversations).mockReturnValueOnce(oldList.promise);
    const branch = { project_id: stateProject.id, source_conversation_id: stateConversations[0].id, branch_conversation_id: stateConversations[1].id } as Parameters<NonNullable<Parameters<typeof workspaceShell.WorkspaceShell>[0]["onOpenBranch"]>>[0];
    let opened!: Promise<boolean>;
    await act(async () => { opened = shell.mock.calls.at(-1)![0].onOpenBranch!(branch); });
    await waitFor(() => expect(api.listConversations).toHaveBeenCalledTimes(reads + 1));
    fireEvent.click(screen.getByRole("button", { name: "恢复会话" }));
    await waitFor(() => expect(screen.queryByRole("button", { name: "恢复会话" })).toBeNull());
    await act(async () => { oldList.resolve([stateConversations[1]]); expect(await opened).toBe(true); });
    expect(screen.getByRole("heading", { name: stateConversations[1].title })).toBeVisible();
    expect(screen.getByRole("button", { name: stateConversations[0].title })).toBeVisible();
    await waitFor(() => expect(shell.mock.calls.at(-1)![0].activeConversationVisibility?.conversation_id).toBe(stateConversations[1].id));
    expect(get().visibility).toBe("visible"); expect(get().revision).toBe(2);
    expect(set).toHaveBeenCalledTimes(2);
  });

  it("keeps the active exact visibility read through a different conversation mutation", async () => {
    const { set, get, change } = visibilityHarness();
    change({ ...get(), visibility: "shelved", revision: 1 });
    vi.mocked(api.latestUsedConversation).mockResolvedValue(stateConversations[1]);
    const activeRead = deferred<ConversationVisibilityRecordV1>();
    const activeState = { ...get() };
    let activeReads = 0; let otherShelved = false;
    vi.mocked(api.getConversationVisibility).mockImplementation(async (projectId, id) => {
      if (id === activeState.conversation_id) return ++activeReads === 2 ? activeRead.promise : { ...activeState };
      return { project_id: projectId, conversation_id: id, visibility: otherShelved ? "shelved" : "visible", revision: otherShelved ? 1 : 0, changed_at: null };
    });
    vi.mocked(api.listConversations).mockImplementation(async () => otherShelved ? [] : [stateConversations[1]]);
    set.mockImplementation(async (request) => {
      otherShelved = true;
      return { request_id: request.request_id, changed: true, state: { project_id: request.project_id, conversation_id: request.conversation_id, visibility: "shelved", revision: 1, changed_at: null } };
    });
    const shell = vi.spyOn(workspaceShell, "WorkspaceShell");
    render(<DesktopApp />); await screen.findByRole("heading", { name: stateConversations[1].title });
    fireEvent.click(screen.getByRole("button", { name: "收起的会话" }));
    const dialog = await screen.findByRole("dialog", { name: "收起的会话" });
    fireEvent.click(await within(dialog).findByRole("button", { name: "打开" }));
    await screen.findByRole("heading", { name: stateConversations[0].title });
    await waitFor(() => expect(activeReads).toBe(2));
    fireEvent.keyDown(window, { key: "Escape" });
    expect(shell.mock.calls.at(-1)![0].activeConversationVisibility).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: `收起会话：${stateConversations[1].title}` }));
    await waitFor(() => expect(screen.queryByRole("button", { name: stateConversations[1].title })).toBeNull());
    await act(async () => activeRead.resolve(activeState));
    expect(screen.getByRole("button", { name: "恢复会话" })).toBeEnabled();
    expect(shell.mock.calls.at(-1)![0].activeConversationVisibility).toEqual(activeState);
    expect(set).toHaveBeenCalledTimes(1);
    expect(set.mock.calls[0][0].conversation_id).toBe(stateConversations[1].id);
  });

  it("ignores an older active getter after same-tuple visibility reconciliation", async () => {
    const { get } = visibilityHarness();
    const before = { ...get() };
    const oldRead = deferred<ConversationVisibilityRecordV1>();
    vi.mocked(api.getConversationVisibility).mockReturnValueOnce(oldRead.promise);
    const shell = vi.spyOn(workspaceShell, "WorkspaceShell");
    render(<DesktopApp />); await screen.findByRole("heading", { name: stateConversations[0].title });
    fireEvent.click(screen.getByRole("button", { name: `收起会话：${stateConversations[0].title}` }));
    await screen.findByRole("button", { name: "恢复会话" });
    await act(async () => oldRead.resolve(before));
    expect(screen.getByRole("button", { name: "恢复会话" })).toBeEnabled();
    expect(shell.mock.calls.at(-1)![0].activeConversationVisibility?.revision).toBe(1);
    expect(shell.mock.calls.at(-1)![0].activeConversationVisibility?.visibility).toBe("shelved");
  });

  it("handles an exact selection rejection with a safe notice and lets the user retry", async () => {
    visibilityHarness();
    const shell = vi.spyOn(workspaceShell, "WorkspaceShell");
    vi.mocked(api.getWorkspaceConversation).mockRejectedValueOnce(new Error("private transport detail"));
    render(<DesktopApp />); await screen.findByRole("heading", { name: stateConversations[0].title });
    await act(async () => { await expect(shell.mock.calls.at(-1)![0].onSelectConversation!(stateConversations[1].id)).resolves.toBeUndefined(); });
    expect(screen.getByText("无法打开会话，请重试。")).toBeVisible();
    expect(screen.queryByText("private transport detail")).toBeNull();
    expect(screen.getByRole("heading", { name: stateConversations[0].title })).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: stateConversations[1].title }));
    await screen.findByRole("heading", { name: stateConversations[1].title });
  });

  it("drops a late failed exact selection without replacing a newer view notice", async () => {
    visibilityHarness();
    const shell = vi.spyOn(workspaceShell, "WorkspaceShell");
    const old = deferred<Awaited<ReturnType<typeof api.getWorkspaceConversation>>>();
    vi.mocked(api.getWorkspaceConversation).mockReturnValueOnce(old.promise);
    render(<DesktopApp />); await screen.findByRole("heading", { name: stateConversations[0].title });
    let settled!: Promise<void>;
    await act(async () => { settled = Promise.resolve(shell.mock.calls.at(-1)![0].onSelectConversation!(stateConversations[1].id)).catch(() => undefined); });
    fireEvent.click(screen.getByRole("button", { name: stateConversations[1].title }));
    await screen.findByRole("heading", { name: stateConversations[1].title });
    await act(async () => { await shell.mock.calls.at(-1)![0].onOpenSourceConversation!("missing-project", "missing-session"); });
    expect(screen.getByText("来源项目已不存在。")).toBeVisible();
    await act(async () => { old.reject(new Error("late private transport detail")); await settled; });
    expect(screen.getByRole("heading", { name: stateConversations[1].title })).toBeVisible();
    expect(screen.getByText("来源项目已不存在。")).toBeVisible();
    expect(screen.queryByText("无法打开会话，请重试。")).toBeNull();
    expect(screen.queryByText("late private transport detail")).toBeNull();
  });

  it("keeps an explicit retry reachable after a lost shelf reply and exact reopening, without chaining restore", async () => {
    const { set, change, get } = visibilityHarness();
    let first!: SetConversationVisibilityRequestV1;
    set.mockImplementationOnce(async (request) => { first = request; change({ ...get(), visibility: "shelved", revision: 1 }); throw new Error("reply lost"); });
    render(<DesktopApp />); await screen.findByRole("heading", { name: stateConversations[0].title });
    fireEvent.click(screen.getByRole("button", { name: `收起会话：${stateConversations[0].title}` }));
    await screen.findByText("可见性操作失败。请重试同一次操作。");
    expect(screen.getByRole("button", { name: "重试原可见性操作" })).toBeEnabled();
    fireEvent.click(screen.getByRole("button", { name: stateConversations[1].title }));
    await screen.findByRole("heading", { name: stateConversations[1].title });
    expect(screen.queryByRole("button", { name: "重试原可见性操作" })).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "收起的会话" }));
    const dialog = await screen.findByRole("dialog", { name: "收起的会话" });
    fireEvent.click(await within(dialog).findByRole("button", { name: "打开" }));
    await screen.findByRole("heading", { name: stateConversations[0].title });
    fireEvent.keyDown(window, { key: "Escape" });
    await screen.findByText("此会话已收起。消息和正在运行的计算会继续保留。");
    expect(screen.getByRole("button", { name: "恢复会话" })).toBeDisabled();
    set.mockRejectedValueOnce(new Error("retry reply lost"));
    fireEvent.click(screen.getByRole("button", { name: "重试原可见性操作" }));
    await screen.findByText("可见性操作失败。请重试同一次操作。");
    expect(set.mock.calls[1][0]).toEqual(first);
    // A receipt without a current read still leaves the original operation pending.
    const read = vi.mocked(api.getConversationVisibility).getMockImplementation()!;
    vi.mocked(api.getConversationVisibility).mockRejectedValueOnce(new Error("requery unavailable"));
    set.mockImplementationOnce(async (request) => ({ request_id: request.request_id, changed: true, state: { ...get() } }));
    fireEvent.click(screen.getByRole("button", { name: "重试原可见性操作" }));
    await waitFor(() => expect(set).toHaveBeenCalledTimes(3));
    await waitFor(() => expect(screen.getByRole("button", { name: "重试原可见性操作" })).toBeEnabled());
    expect(screen.getByRole("button", { name: "恢复会话" })).toBeDisabled();
    change({ ...get(), visibility: "visible", revision: 2 });
    vi.mocked(api.getConversationVisibility).mockImplementation(read);
    set.mockImplementationOnce(async (request) => ({ request_id: request.request_id, changed: true, state: { ...get(), visibility: "shelved", revision: 1 } }));
    fireEvent.click(screen.getByRole("button", { name: "重试原可见性操作" }));
    await waitFor(() => expect(screen.queryByRole("button", { name: "重试原可见性操作" })).toBeNull());
    expect(set).toHaveBeenCalledTimes(4);
    for (const [request] of set.mock.calls) expect(request).toEqual(first);
    expect(screen.queryByRole("button", { name: "恢复会话" })).toBeNull();
    expect(screen.getByRole("button", { name: stateConversations[0].title })).toBeVisible();
    // Only a later explicit action creates a new mutation at the displayed revision.
    fireEvent.click(screen.getByRole("button", { name: `收起会话：${stateConversations[0].title}` }));
    await waitFor(() => expect(set).toHaveBeenCalledTimes(5));
    expect(set.mock.calls[4][0]).toMatchObject({ visibility: "shelved", expected_revision: 2 });
    expect(set.mock.calls[4][0].request_id).not.toBe(first.request_id);
  });

  it("visibility_scope_and_escape closes a later search first and collection before the underlying sidebar", async () => {
    visibilityHarness();
    render(<DesktopApp />); await screen.findByRole("heading", { name: stateConversations[0].title });
    fireEvent.click(screen.getByRole("button", { name: "展开侧栏" }));
    fireEvent.click(screen.getByRole("button", { name: "收起的会话" }));
    await screen.findByRole("dialog", { name: "收起的会话" });
    fireEvent.keyDown(window, { key: "k", ctrlKey: true });
    await screen.findByRole("dialog", { name: "搜索工作区" });
    fireEvent.keyDown(window, { key: "Escape" });
    expect(screen.queryByRole("dialog", { name: "搜索工作区" })).toBeNull();
    expect(screen.getByRole("dialog", { name: "收起的会话" })).toBeVisible();
    fireEvent.keyDown(window, { key: "Escape" });
    expect(screen.queryByRole("dialog", { name: "收起的会话" })).toBeNull();
    expect(screen.getByRole("button", { name: "收起侧栏" })).toHaveAttribute("aria-expanded", "true");
    fireEvent.keyDown(window, { key: "Escape" });
    expect(screen.getByRole("button", { name: "展开侧栏" })).toHaveAttribute("aria-expanded", "false");
  });

  it("retains the active retry through project reentry and allows restore only after shelf reconciliation", async () => {
    const { set, change, get } = visibilityHarness();
    set.mockImplementationOnce(async () => { change({ ...get(), visibility: "shelved", revision: 1 }); throw new Error("lost shelf reply"); });
    render(<DesktopApp />); await screen.findByRole("heading", { name: stateConversations[0].title });
    fireEvent.click(screen.getByRole("button", { name: `收起会话：${stateConversations[0].title}` }));
    await screen.findByText("可见性操作失败。请重试同一次操作。");
    fireEvent.click(screen.getByRole("button", { name: "返回项目主页" }));
    expect(screen.queryByRole("button", { name: "重试原可见性操作" })).toBeNull();
    fireEvent.click(await screen.findByRole("button", { name: `打开项目：${stateProject.name}` }));
    await screen.findByRole("heading", { name: stateConversations[0].title });
    await screen.findByText("此会话已收起。消息和正在运行的计算会继续保留。");
    expect(screen.getByRole("button", { name: "恢复会话" })).toBeDisabled();
    set.mockImplementationOnce(async (request) => ({ request_id: request.request_id, changed: true, state: { ...get() } }));
    fireEvent.click(screen.getByRole("button", { name: "重试原可见性操作" }));
    await waitFor(() => expect(screen.queryByRole("button", { name: "重试原可见性操作" })).toBeNull());
    expect(set).toHaveBeenCalledTimes(2);
    expect(set.mock.calls[1][0]).toEqual(set.mock.calls[0][0]);
    expect(screen.getByRole("button", { name: "恢复会话" })).toBeEnabled();
    fireEvent.click(screen.getByRole("button", { name: "恢复会话" }));
    await waitFor(() => expect(set).toHaveBeenCalledTimes(3));
    expect(set.mock.calls[2][0]).toMatchObject({ visibility: "visible", expected_revision: 1 });
    expect(set.mock.calls[2][0].request_id).not.toBe(set.mock.calls[0][0].request_id);
    await waitFor(() => expect(screen.queryByRole("button", { name: "恢复会话" })).toBeNull());
  });

  it("keeps host unknown request through A to B to A and ignores old settlement while a newer retry is pending", async () => {
    const { set } = visibilityHarness();
    const old = deferred<Awaited<ReturnType<typeof api.setConversationVisibility>>>();
    const newer = deferred<Awaited<ReturnType<typeof api.setConversationVisibility>>>();
    set.mockReturnValueOnce(old.promise).mockReturnValueOnce(newer.promise);
    const shell = vi.spyOn(workspaceShell, "WorkspaceShell");
    render(<DesktopApp />); await screen.findByRole("heading", { name: stateConversations[0].title });
    let first!: Promise<void>;
    await act(async () => { first = shell.mock.calls.at(-1)![0].onSetVisibility!(stateConversations[0].id, "shelved", 0); });
    fireEvent.click(screen.getByRole("button", { name: stateConversations[1].title }));
    await screen.findByRole("heading", { name: stateConversations[1].title });
    fireEvent.click(screen.getByRole("button", { name: stateConversations[0].title }));
    await screen.findByRole("heading", { name: stateConversations[0].title });
    let retry!: Promise<void>;
    await act(async () => { retry = shell.mock.calls.at(-1)![0].onSetVisibility!(stateConversations[0].id, "shelved", 99); });
    expect(set.mock.calls[1][0]).toEqual(set.mock.calls[0][0]);
    const read = vi.mocked(api.getConversationVisibility).mock.calls.length;
    await act(async () => { old.resolve({ request_id: set.mock.calls[0][0].request_id, changed: true, state: { project_id: stateProject.id, conversation_id: stateConversations[0].id, visibility: "shelved", revision: 1, changed_at: null } }); await first; });
    expect(api.getConversationVisibility).toHaveBeenCalledTimes(read);
    await act(async () => { newer.reject({ code: "storage_failed" }); await retry.catch(() => undefined); });
    await act(async () => { await shell.mock.calls.at(-1)![0].onSetVisibility!(stateConversations[0].id, "shelved", 42); });
    expect(set.mock.calls[2][0]).toEqual(set.mock.calls[0][0]);
  });

  it("ignores a superseded same-view visibility retry completion and blocks the opposite operation while unknown", async () => {
    const { set } = visibilityHarness();
    const old = deferred<Awaited<ReturnType<typeof api.setConversationVisibility>>>();
    const newer = deferred<Awaited<ReturnType<typeof api.setConversationVisibility>>>();
    set.mockReturnValueOnce(old.promise).mockReturnValueOnce(newer.promise);
    const shell = vi.spyOn(workspaceShell, "WorkspaceShell");
    render(<DesktopApp />); await screen.findByRole("heading", { name: stateConversations[0].title });
    const callback = shell.mock.calls.at(-1)![0].onSetVisibility!;
    let first!: Promise<void>; let retry!: Promise<void>;
    await act(async () => { first = callback(stateConversations[0].id, "shelved", 0); retry = callback(stateConversations[0].id, "shelved", 8); });
    const reads = vi.mocked(api.getConversationVisibility).mock.calls.length;
    await act(async () => { old.resolve({ request_id: set.mock.calls[0][0].request_id, changed: true, state: { project_id: stateProject.id, conversation_id: stateConversations[0].id, visibility: "shelved", revision: 1, changed_at: null } }); await first; });
    expect(api.getConversationVisibility).toHaveBeenCalledTimes(reads);
    await expect(callback(stateConversations[0].id, "visible", 1)).rejects.toEqual({ code: "request_conflict" });
    expect(set).toHaveBeenCalledTimes(2);
    await act(async () => { newer.reject(new Error("lost reply")); await retry.catch(() => undefined); });
  });

  it("visibility_catalog_refresh retries global discovery independently from workflow catalogs", async () => {
    const { get } = visibilityHarness();
    const item = { reference: { kind: "session" as const, project_id: stateProject.id, id: stateConversations[0].id }, label: "Discoverable session", description: "Saved transcript" };
    const catalog = vi.spyOn(referenceApi, "composerReferenceCatalog").mockImplementation(async () => get().visibility === "visible" ? [item] : []);
    const shell = vi.spyOn(workspaceShell, "WorkspaceShell");
    render(<DesktopApp />); await screen.findByRole("heading", { name: stateConversations[0].title });
    fireEvent.keyDown(window, { key: "k", ctrlKey: true });
    await screen.findByRole("button", { name: "打开 Discoverable session" });
    await act(async () => { await shell.mock.calls.at(-1)![0].onSetVisibility!(stateConversations[0].id, "shelved", 0); });
    await waitFor(() => expect(screen.queryByRole("button", { name: "打开 Discoverable session" })).toBeNull());
    expect(catalog).toHaveBeenCalledTimes(2);
    expect(shell.mock.calls.at(-1)![0].referenceCatalogVersion).toBe(1);
    expect(shell.mock.calls.at(-1)![0].workflowCatalogVersion).toBe(0);
    await act(async () => { await shell.mock.calls.at(-1)![0].onSetVisibility!(stateConversations[0].id, "visible", 1); });
    await screen.findByRole("button", { name: "打开 Discoverable session" });
    expect(shell.mock.calls.at(-1)![0].referenceCatalogVersion).toBe(2);
  });

  it("opens a collection exact ID and restores using its current banner revision", async () => {
    const { change, get, set } = visibilityHarness();
    change({ ...get(), visibility: "shelved", revision: 5 });
    vi.mocked(api.latestUsedConversation).mockResolvedValue(stateConversations[1]);
    render(<DesktopApp />); await screen.findByRole("heading", { name: stateConversations[1].title });
    fireEvent.click(screen.getByRole("button", { name: "收起的会话" }));
    const dialog = await screen.findByRole("dialog", { name: "收起的会话" });
    fireEvent.click(await within(dialog).findByRole("button", { name: "打开" }));
    await screen.findByRole("heading", { name: stateConversations[0].title });
    expect(set).not.toHaveBeenCalled();
    fireEvent.keyDown(window, { key: "Escape" });
    fireEvent.click(screen.getByRole("button", { name: "恢复会话" }));
    await waitFor(() => expect(set).toHaveBeenCalledWith(expect.objectContaining({ visibility: "visible", expected_revision: 5 })));
    await waitFor(() => expect(screen.queryByRole("button", { name: "恢复会话" })).toBeNull());
    expect(screen.getByRole("button", { name: stateConversations[0].title })).toBeVisible();
  });

  it("opens a shelved exact branch result without relying on Visible membership", async () => {
    const { change, get, set } = visibilityHarness();
    change({ ...get(), visibility: "shelved", revision: 1 });
    vi.mocked(api.latestUsedConversation).mockResolvedValue(stateConversations[1]);
    const shell = vi.spyOn(workspaceShell, "WorkspaceShell");
    render(<DesktopApp />); await screen.findByRole("heading", { name: stateConversations[1].title });
    const branch = { project_id: stateProject.id, source_conversation_id: stateConversations[1].id, branch_conversation_id: stateConversations[0].id } as Parameters<NonNullable<Parameters<typeof workspaceShell.WorkspaceShell>[0]["onOpenBranch"]>>[0];
    await act(async () => { expect(await shell.mock.calls.at(-1)![0].onOpenBranch!(branch)).toBe(true); });
    await screen.findByRole("heading", { name: stateConversations[0].title });
    await screen.findByRole("button", { name: "恢复会话" });
    expect(screen.queryByRole("button", { name: stateConversations[0].title })).toBeNull();
    expect(set).not.toHaveBeenCalled();
  });

  it("new messages in a shelved session do not restore visibility or reinsert navigation", async () => {
    const { set } = visibilityHarness();
    const submit = vi.spyOn(api, "submitMessage").mockImplementation(async (request) => ({ id: "new-shelved-message", project_id: request.project_id, conversation_id: request.conversation_id, sequence: 1, role: "user", markdown: request.markdown, created_at: "" }));
    vi.spyOn(api, "agentV4StartDirect").mockResolvedValue({ ...stateRun("shelved-run", "running", null, null, null), session_mode: "agent" });
    render(<DesktopApp />); await screen.findByRole("heading", { name: stateConversations[0].title });
    fireEvent.click(screen.getByRole("button", { name: `收起会话：${stateConversations[0].title}` }));
    await screen.findByRole("button", { name: "恢复会话" });
    const input = screen.getByRole("textbox", { name: /描述研究目标/ });
    await waitFor(() => expect(input).toBeEnabled());
    fireEvent.change(input, { target: { value: "Continue shelved research" } });
    await waitFor(() => expect(screen.getByRole("button", { name: "发送" })).toBeEnabled());
    fireEvent.click(screen.getByRole("button", { name: "发送" }));
    await waitFor(() => expect(submit).toHaveBeenCalledWith(expect.objectContaining({ conversation_id: stateConversations[0].id, markdown: "Continue shelved research" })));
    await screen.findByText("Continue shelved research");
    expect(screen.getByRole("button", { name: "恢复会话" })).toBeVisible();
    expect(screen.queryByRole("button", { name: stateConversations[0].title })).toBeNull();
    expect(set).toHaveBeenCalledTimes(1);
  });

  it("delivered conversation updates retain the exact Shelved transcript without reinserting navigation", async () => {
    const { set, get } = visibilityHarness();
    const updated = vi.spyOn(api, "onConversationUpdated").mockResolvedValue(() => {});
    const messages = vi.spyOn(api, "onConversationEvent").mockResolvedValue(() => {});
    render(<DesktopApp />); await screen.findByRole("heading", { name: stateConversations[0].title });
    fireEvent.click(screen.getByRole("button", { name: `收起会话：${stateConversations[0].title}` }));
    await screen.findByRole("button", { name: "恢复会话" });
    await act(async () => {
      messages.mock.calls.at(-1)![0]({ project_id: stateProject.id, conversation_id: stateConversations[0].id, message: { id: "live-shelved", project_id: stateProject.id, conversation_id: stateConversations[0].id, sequence: 1, role: "user", markdown: "delivered shelved message", created_at: "now" } });
      updated.mock.calls.at(-1)![0]({ project_id: stateProject.id, conversation: { ...stateConversations[0], title: "Updated exact shelved title", status: "running" } });
    });
    expect(screen.getByText("delivered shelved message")).toBeVisible();
    expect(screen.getByRole("heading", { name: "Updated exact shelved title" })).toBeVisible();
    expect(screen.queryByRole("button", { name: "Updated exact shelved title" })).toBeNull();
    expect(get().visibility).toBe("shelved"); expect(set).toHaveBeenCalledTimes(1);
  });

  it("drops a late event list after a newer Shelf reconciliation", async () => {
    visibilityHarness();
    const updated = vi.spyOn(api, "onConversationUpdated").mockResolvedValue(() => {});
    render(<DesktopApp />); await screen.findByRole("heading", { name: stateConversations[0].title });
    const oldList = deferred<typeof stateConversations>();
    vi.mocked(api.listConversations).mockReturnValueOnce(oldList.promise);
    await act(async () => updated.mock.calls.at(-1)![0]({ project_id: stateProject.id, conversation: stateConversations[0] }));
    fireEvent.click(screen.getByRole("button", { name: `收起会话：${stateConversations[0].title}` }));
    await screen.findByRole("button", { name: "恢复会话" });
    await act(async () => oldList.resolve(stateConversations));
    expect(screen.queryByRole("button", { name: stateConversations[0].title })).toBeNull();
    expect(screen.getByRole("button", { name: "恢复会话" })).toBeEnabled();
  });

  it("drops a late event list through project A to B to A and preserves a newer event refresh", async () => {
    visibilityHarness();
    const otherProject = { ...stateProject, id: "event-project-b", name: "Event project B" };
    const otherConversation = { ...stateConversations[0], id: "event-conversation-b", project_id: otherProject.id, title: "Event conversation B" };
    vi.mocked(api.listProjects).mockResolvedValue([stateProject, otherProject]);
    const list = vi.mocked(api.listConversations).getMockImplementation()!;
    vi.mocked(api.listConversations).mockImplementation(async (id) => id === otherProject.id ? [otherConversation] : list(id));
    vi.mocked(api.latestUsedConversation).mockImplementation(async (id) => id === otherProject.id ? otherConversation : stateConversations[0]);
    vi.mocked(api.agentV4ConversationState).mockImplementation(async (id, conversationId) => ({ ...stateSnapshot(conversationId), project_id: id }));
    const updated = vi.spyOn(api, "onConversationUpdated").mockResolvedValue(() => {});
    render(<DesktopApp />); await screen.findByRole("heading", { name: stateConversations[0].title });
    const oldList = deferred<typeof stateConversations>(); const newList = deferred<typeof stateConversations>();
    const staleItem = { ...stateConversations[0], id: "stale-event", title: "Stale event discovery" };
    const newItem = { ...stateConversations[0], id: "new-event", title: "New visible discovery" };
    vi.mocked(api.listConversations).mockReturnValueOnce(oldList.promise);
    await act(async () => updated.mock.calls.at(-1)![0]({ project_id: stateProject.id, conversation: stateConversations[0] }));
    fireEvent.click(screen.getByRole("button", { name: "返回项目主页" }));
    fireEvent.click(await screen.findByRole("button", { name: `打开项目：${otherProject.name}` }));
    await screen.findByRole("heading", { name: otherConversation.title });
    fireEvent.click(screen.getByRole("button", { name: "返回项目主页" }));
    fireEvent.click(await screen.findByRole("button", { name: `打开项目：${stateProject.name}` }));
    await screen.findByRole("heading", { name: stateConversations[0].title });
    vi.mocked(api.listConversations).mockReturnValueOnce(newList.promise);
    await act(async () => updated.mock.calls.at(-1)![0]({ project_id: stateProject.id, conversation: stateConversations[0] }));
    await act(async () => oldList.resolve([staleItem]));
    expect(screen.queryByRole("button", { name: staleItem.title })).toBeNull();
    await act(async () => newList.resolve([...stateConversations, newItem]));
    expect(screen.getByRole("button", { name: newItem.title })).toBeVisible();
    expect(screen.getByRole("heading", { name: stateConversations[0].title })).toBeVisible();
  });

  it("discovers a newly Visible conversation from the authoritative event refresh", async () => {
    visibilityHarness();
    const updated = vi.spyOn(api, "onConversationUpdated").mockResolvedValue(() => {});
    render(<DesktopApp />); await screen.findByRole("heading", { name: stateConversations[0].title });
    const item = { ...stateConversations[0], id: "new-visible", title: "Created visible conversation" };
    vi.mocked(api.listConversations).mockResolvedValue([...stateConversations, item]);
    await act(async () => updated.mock.calls.at(-1)![0]({ project_id: stateProject.id, conversation: item }));
    expect(screen.getByRole("button", { name: item.title })).toBeVisible();
    expect(screen.getByRole("heading", { name: stateConversations[0].title })).toBeVisible();
  });

  it("keeps the newest same-view event list when an older refresh completes last", async () => {
    visibilityHarness();
    const updated = vi.spyOn(api, "onConversationUpdated").mockResolvedValue(() => {});
    render(<DesktopApp />); await screen.findByRole("heading", { name: stateConversations[0].title });
    const oldList = deferred<typeof stateConversations>(); const newList = deferred<typeof stateConversations>();
    const staleItem = { ...stateConversations[0], id: "stale-event", title: "Stale event discovery" };
    const newItem = { ...stateConversations[0], id: "new-event", title: "New visible discovery" };
    vi.mocked(api.listConversations).mockReturnValueOnce(oldList.promise).mockReturnValueOnce(newList.promise);
    await act(async () => {
      updated.mock.calls.at(-1)![0]({ project_id: stateProject.id, conversation: stateConversations[0] });
      updated.mock.calls.at(-1)![0]({ project_id: stateProject.id, conversation: stateConversations[0] });
    });
    await act(async () => newList.resolve([...stateConversations, newItem]));
    expect(screen.getByRole("button", { name: newItem.title })).toBeVisible();
    await act(async () => oldList.resolve([staleItem]));
    expect(screen.queryByRole("button", { name: staleItem.title })).toBeNull();
    expect(screen.getByRole("button", { name: newItem.title })).toBeVisible();
  });

  it("reaches the original unknown Shelf retry after exact reopening and only permits a later explicit Restore", async () => {
    const { set, change, get } = visibilityHarness();
    set.mockImplementationOnce(async () => { change({ ...get(), visibility: "shelved", revision: 1 }); throw new Error("lost committed reply"); });
    render(<DesktopApp />); await screen.findByRole("heading", { name: stateConversations[0].title });
    fireEvent.click(screen.getByRole("button", { name: `收起会话：${stateConversations[0].title}` }));
    await screen.findByText("可见性操作失败。请重试同一次操作。");
    fireEvent.click(screen.getByRole("button", { name: stateConversations[1].title }));
    await screen.findByRole("heading", { name: stateConversations[1].title });
    fireEvent.click(screen.getByRole("button", { name: "收起的会话" }));
    fireEvent.click(await within(await screen.findByRole("dialog", { name: "收起的会话" })).findByRole("button", { name: "打开" }));
    await screen.findByRole("heading", { name: stateConversations[0].title });
    fireEvent.keyDown(window, { key: "Escape" });
    expect(await screen.findByRole("button", { name: "恢复会话" })).toBeDisabled();
    set.mockImplementationOnce(async (request) => ({ request_id: request.request_id, changed: true, state: { ...get(), visibility: "shelved", revision: 1 } }));
    fireEvent.click(screen.getByRole("button", { name: "重试原可见性操作" }));
    await waitFor(() => expect(screen.getByRole("button", { name: "恢复会话" })).toBeEnabled());
    expect(set.mock.calls[1][0]).toEqual(set.mock.calls[0][0]);
    expect(set).toHaveBeenCalledTimes(2);
    fireEvent.click(screen.getByRole("button", { name: "恢复会话" }));
    await waitFor(() => expect(set).toHaveBeenCalledTimes(3));
    expect(set.mock.calls[2][0]).toMatchObject({ visibility: "visible", expected_revision: 1 });
    expect(set.mock.calls[2][0].request_id).not.toBe(set.mock.calls[0][0].request_id);
  });
  it("does not carry an example source request into the next ordinary workspace", async () => {
    const { stateSpy } = setupConversationStateHarness(); stateSpy.mockImplementation(async (_id, conversationId) => stateSnapshot(conversationId));
    vi.mocked(api.listProjects).mockResolvedValue([offlineProject, stateProject]);
    vi.mocked(offlineExampleApi.getOfflineExampleWorkspace).mockImplementation(async (id) => id === offlineProject.id ? offlineView() : null);
    vi.spyOn(referenceApi, "composerReferenceCatalog").mockResolvedValue([{ reference: { kind: "artifact", project_id: offlineProject.id, id: "example-source" }, label: "Example source", description: "Preset" }]);
    const shell = vi.spyOn(workspaceShell, "WorkspaceShell");
    render(<DesktopApp />); await screen.findByText(fixture.disclaimer.zh);
    fireEvent.keyDown(window, { key: "k", ctrlKey: true }); fireEvent.click(await screen.findByRole("button", { name: "打开 Example source" }));
    await waitFor(() => expect(screen.queryByRole("dialog", { name: "搜索工作区" })).toBeNull());
    fireEvent.click(screen.getByRole("button", { name: "返回项目库" }));
    fireEvent.click(screen.getByRole("button", { name: `打开项目：${stateProject.name}` }));
    await screen.findByRole("heading", { name: stateConversations[0].title });
    expect(shell.mock.calls.at(-1)?.[0].searchRequest).toBeNull();
  });
  it("identifies a normally created project before production initialization", async () => {
    vi.spyOn(api, "listProjects").mockResolvedValue([]);
    vi.spyOn(api, "chooseProjectDirectory").mockResolvedValue({ path: "E:/Created", usedFallback: false });
    vi.spyOn(api, "createProject").mockResolvedValue(offlineProject);
    const pending = deferred<OfflineExampleWorkspaceView | null>();
    vi.mocked(offlineExampleApi.getOfflineExampleWorkspace).mockReturnValue(pending.promise);
    const noDispatch = forbidOfflineDispatch(); const sessions = vi.spyOn(api, "listConversations");
    render(<DesktopApp />);
    fireEvent.click(await screen.findByRole("button", { name: "创建空白项目" }));
    fireEvent.click(screen.getByRole("button", { name: "选择目录" }));
    await screen.findAllByText("E:/Created");
    fireEvent.click(screen.getByRole("button", { name: "创建项目" }));
    await waitFor(() => expect(offlineExampleApi.getOfflineExampleWorkspace).toHaveBeenCalledWith(offlineProject.id));
    noDispatch(); expect(sessions).not.toHaveBeenCalled();
    await act(async () => pending.resolve(offlineView()));
    expect(screen.getByText(fixture.disclaimer.zh)).toBeInTheDocument(); noDispatch(); expect(sessions).not.toHaveBeenCalled();
  });

  it.each(["usage", "source"] as const)("preserves the identified current draft and receipt when deferred %s identity is cancelled", async (route) => {
    const { stateSpy } = setupConversationStateHarness(); stateSpy.mockImplementation(async (_id, conversationId) => stateSnapshot(conversationId));
    vi.mocked(api.listProjects).mockResolvedValue([stateProject, offlineProject]);
    vi.mocked(api.listMessages).mockResolvedValue([{ id: "receipt", project_id: stateProject.id, conversation_id: stateConversations[0].id, sequence: 1, role: "assistant", markdown: "Retained receipt fixture", created_at: "now" }]);
    const pending = deferred<OfflineExampleWorkspaceView | null>();
    vi.mocked(offlineExampleApi.getOfflineExampleWorkspace).mockImplementation((id) => id === offlineProject.id ? pending.promise : Promise.resolve(null));
    if (route === "usage") vi.spyOn(settingsPanel, "SettingsPanel").mockImplementation(({ onOpenUsageConversation, onClose }) => <div><button onClick={() => void onOpenUsageConversation?.(offlineProject.id, "example-session")}>Open deferred usage</button><button onClick={onClose}>Close deferred usage</button></div>);
    else {
      const OriginalShell = workspaceShell.WorkspaceShell;
      vi.spyOn(workspaceShell, "WorkspaceShell").mockImplementation((props) => <><button onClick={() => void props.onOpenSourceConversation?.(offlineProject.id, "example-session")}>Open deferred source</button><button onClick={() => void props.onOpenSourceConversation?.(stateProject.id, stateConversations[0].id)}>Keep current source</button><OriginalShell {...props} /></>);
    }
    render(<DesktopApp />);
    const input = await screen.findByRole("textbox", { name: /描述研究目标/ }); await waitFor(() => expect(input).toBeEnabled());
    fireEvent.change(input, { target: { value: "Keep identified project draft" } });
    if (route === "usage") fireEvent.click(screen.getByRole("button", { name: "设置" }));
    fireEvent.click(await screen.findByRole("button", { name: `Open deferred ${route}` }));
    await screen.findByText("正在识别项目来源…");
    expect(input).toHaveValue("Keep identified project draft"); expect(screen.getByText("Retained receipt fixture")).toBeInTheDocument();
    if (route === "usage") fireEvent.click(screen.getByRole("button", { name: "Close deferred usage" }));
    else fireEvent.click(screen.getByRole("button", { name: "Keep current source" }));
    await act(async () => pending.resolve(offlineView()));
    expect(screen.getByRole("heading", { name: stateConversations[0].title })).toBeInTheDocument();
    expect(input).toHaveValue("Keep identified project draft"); expect(screen.getByText("Retained receipt fixture")).toBeInTheDocument();
    expect(screen.queryByText(fixture.disclaimer.zh)).toBeNull();
    expect(api.listConversations).not.toHaveBeenCalledWith(offlineProject.id);
    expect(api.agentV4ComputeBackends).not.toHaveBeenCalledWith(offlineProject.id, expect.anything());
  });

  it("rechecks a current project's marker before committing refreshed metadata", async () => {
    const { stateSpy } = setupConversationStateHarness(); stateSpy.mockImplementation(async (_id, conversationId) => stateSnapshot(conversationId));
    let refresh!: (connectionId: string, remoteRoot: string) => Promise<void>;
    vi.spyOn(settingsPanel, "SettingsPanel").mockImplementation(({ onBindProjectRemote }) => { refresh = onBindProjectRemote!; return <span>Metadata settings</span>; });
    vi.spyOn(api, "updateProjectRemote").mockResolvedValue({ ...stateProject, connection_id: null, remote_root: null });
    render(<DesktopApp />); await screen.findByRole("heading", { name: stateConversations[0].title });
    fireEvent.click(screen.getByRole("button", { name: "设置" })); await screen.findByText("Metadata settings");
    vi.mocked(offlineExampleApi.getOfflineExampleWorkspace).mockResolvedValue(offlineView(stateProject.id));
    await act(async () => { await refresh("", ""); });
    expect(screen.getByText(fixture.disclaimer.zh)).toBeInTheDocument();
    expect(offlineExampleApi.getOfflineExampleWorkspace).toHaveBeenCalledTimes(2);
    expect(screen.queryByRole("textbox", { name: /描述研究目标/ })).toBeNull();
  });
  it("waits for marker identity without mounting Agent and fails closed on rejection", async () => {
    vi.spyOn(api, "listProjects").mockResolvedValue([offlineProject]);
    const pending = deferred<OfflineExampleWorkspaceView | null>();
    vi.mocked(offlineExampleApi.getOfflineExampleWorkspace).mockReturnValue(pending.promise);
    const noDispatch = forbidOfflineDispatch(); const sessions = vi.spyOn(api, "listConversations");
    render(<DesktopApp />);
    await waitFor(() => expect(offlineExampleApi.getOfflineExampleWorkspace).toHaveBeenCalledWith(offlineProject.id));
    expect(screen.queryByRole("textbox", { name: /描述研究目标/ })).toBeNull(); noDispatch(); expect(sessions).not.toHaveBeenCalled();
    await act(async () => pending.reject(new Error("unsafe raw path/API-key must not be shown")));
    expect(await screen.findByRole("alert")).toHaveTextContent("无法识别项目来源");
    expect(screen.queryByText(/unsafe raw path/)).toBeNull(); noDispatch(); expect(sessions).not.toHaveBeenCalled();
  });

  it("reopens a missing-receipt marker as unavailable and never restores production sessions", async () => {
    vi.spyOn(api, "listProjects").mockResolvedValue([offlineProject]);
    vi.mocked(offlineExampleApi.getOfflineExampleWorkspace).mockResolvedValue({ ...offlineView(), availability: "unavailable", unavailable_reason: "receipt_missing", entries: [], files: [] });
    const noDispatch = forbidOfflineDispatch(); const sessions = vi.spyOn(api, "listConversations");
    const view = render(<DesktopApp />);
    expect(await screen.findByRole("alert")).toHaveTextContent("收据缺失");
    view.unmount(); render(<DesktopApp />);
    expect(await screen.findByRole("alert")).toHaveTextContent("收据缺失");
    expect(screen.getByText(fixture.disclaimer.zh)).toBeInTheDocument();
    noDispatch(); expect(sessions).not.toHaveBeenCalled(); expect(api.latestUsedConversation).not.toHaveBeenCalled();
  });

  it("ignores a late startup identity after an explicit project-list navigation", async () => {
    vi.spyOn(api, "listProjects").mockResolvedValue([offlineProject]);
    const startup = deferred<OfflineExampleWorkspaceView | null>();
    vi.mocked(offlineExampleApi.getOfflineExampleWorkspace).mockReturnValueOnce(startup.promise).mockResolvedValue(offlineView());
    const noDispatch = forbidOfflineDispatch();
    render(<DesktopApp />);
    await waitFor(() => expect(offlineExampleApi.getOfflineExampleWorkspace).toHaveBeenCalledOnce());
    fireEvent.click(screen.getByRole("menuitem", { name: "File" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "项目列表" }));
    fireEvent.click(await screen.findByRole("button", { name: `打开项目：${offlineProject.name}` }));
    await screen.findByText(fixture.disclaimer.zh);
    await act(async () => startup.resolve(null));
    expect(screen.getByText(fixture.disclaimer.zh)).toBeInTheDocument(); noDispatch();
  });

  it("keeps one creation request through failure/retry and starts a new request after close/reopen", async () => {
    vi.spyOn(api, "listProjects").mockResolvedValue([]);
    vi.spyOn(api, "chooseProjectDirectory").mockResolvedValue({ path: "E:/Examples", usedFallback: false });
    const create = vi.spyOn(offlineExampleApi, "createOfflineExampleWorkspace").mockRejectedValue(new Error("example_registration_failed"));
    const ordinaryCreate = vi.spyOn(api, "createProject"); const noDispatch = forbidOfflineDispatch();
    render(<DesktopApp />);
    fireEvent.click(await screen.findByRole("button", { name: "离线示例" }));
    fireEvent.click(screen.getByRole("button", { name: "选择父目录" }));
    await screen.findByText("E:/Examples");
    fireEvent.click(screen.getByRole("button", { name: "创建示例" })); await screen.findByRole("alert");
    fireEvent.click(screen.getByRole("button", { name: "创建示例" })); await waitFor(() => expect(create).toHaveBeenCalledTimes(2));
    expect(create.mock.calls[1][0]).toEqual(create.mock.calls[0][0]);
    fireEvent.keyDown(window, { key: "Escape" }); fireEvent.click(screen.getByRole("button", { name: "离线示例" }));
    fireEvent.click(screen.getByRole("button", { name: "选择父目录" })); await screen.findByText("E:/Examples");
    vi.mocked(api.listProjects).mockResolvedValue([offlineProject]);
    create.mockResolvedValue({ project_id: offlineProject.id, workspace: offlineView() });
    vi.mocked(offlineExampleApi.getOfflineExampleWorkspace).mockResolvedValue(offlineView());
    fireEvent.click(screen.getByRole("button", { name: "创建示例" }));
    await screen.findByText(fixture.disclaimer.zh);
    expect(create.mock.calls[2][0].request_id).not.toBe(create.mock.calls[0][0].request_id);
    expect(create.mock.calls[2][0]).toMatchObject({ example_id: "synthetic-evidence-report-v1", parent_directory: "E:/Examples" });
    expect(offlineExampleApi.getOfflineExampleWorkspace).toHaveBeenCalledWith(offlineProject.id);
    expect(ordinaryCreate).not.toHaveBeenCalled(); noDispatch();
  });

  it.each(["session", "artifact", "project"] as const)("identifies an offline %s search target before any target conversation initialization", async (kind) => {
    vi.spyOn(api, "listProjects").mockResolvedValue([offlineProject]);
    vi.mocked(offlineExampleApi.getOfflineExampleWorkspace).mockResolvedValue(offlineView());
    vi.spyOn(referenceApi, "composerReferenceCatalog").mockResolvedValue(kind === "project" ? [] : [{ reference: { kind, project_id: offlineProject.id, id: "exact-source" }, label: "Exact offline source", description: "Preset source" }]);
    const sessions = vi.spyOn(api, "listConversations"); const noDispatch = forbidOfflineDispatch();
    render(<DesktopApp />); await screen.findByText(fixture.disclaimer.zh);
    fireEvent.keyDown(window, { key: "k", ctrlKey: true });
    fireEvent.click(await screen.findByRole("button", { name: `打开 ${kind === "project" ? offlineProject.name : "Exact offline source"}` }));
    await waitFor(() => expect(screen.queryByRole("dialog", { name: "搜索工作区" })).toBeNull());
    expect(screen.getByText(fixture.disclaimer.zh)).toBeInTheDocument();
    if (kind !== "project") expect(screen.getByRole("status")).toHaveTextContent("示例没有生产会话或运行来源");
    expect(sessions).not.toHaveBeenCalled(); noDispatch();
  });

  it.each(["usage", "source"] as const)("routes cross-project %s callbacks through the marker without reading example conversations", async (route) => {
    const { stateSpy } = setupConversationStateHarness(); stateSpy.mockImplementation(async (_id, conversationId) => stateSnapshot(conversationId));
    vi.mocked(api.listProjects).mockResolvedValue([stateProject, offlineProject]);
    vi.mocked(offlineExampleApi.getOfflineExampleWorkspace).mockImplementation(async (id) => id === offlineProject.id ? offlineView() : null);
    if (route === "usage") vi.spyOn(settingsPanel, "SettingsPanel").mockImplementation(({ onOpenUsageConversation }) => <button onClick={() => void onOpenUsageConversation?.(offlineProject.id, "example-session")}>Open example usage</button>);
    else vi.spyOn(workspaceShell, "WorkspaceShell").mockImplementation(({ onOpenSourceConversation }) => <button onClick={() => void onOpenSourceConversation?.(offlineProject.id, "example-session")}>Open example source</button>);
    render(<DesktopApp />);
    if (route === "usage") { await screen.findByRole("heading", { name: stateConversations[0].title }); fireEvent.click(screen.getByRole("button", { name: "设置" })); }
    fireEvent.click(await screen.findByRole("button", { name: `Open example ${route}` }));
    await screen.findByText(fixture.disclaimer.zh);
    expect(screen.getByRole("status")).toHaveTextContent("示例没有生产会话或运行来源");
    expect(api.listConversations).not.toHaveBeenCalledWith(offlineProject.id);
    expect(api.latestUsedConversation).not.toHaveBeenCalledWith(offlineProject.id);
    expect(api.agentV4ComputeBackends).not.toHaveBeenCalledWith(offlineProject.id, expect.anything());
  });
  it("does not initialize a production conversation when the first project is an offline example", async () => {
    const example = { ...stateProject, id: "example-project", name: "离线合成示例", template: "blank" as const, status: "ready" as const };
    vi.mocked(offlineExampleApi.getOfflineExampleWorkspace).mockResolvedValue(offlineView(example.id));
    vi.spyOn(api, "listProjects").mockResolvedValue([example]);
    const create = vi.spyOn(api, "createConversation").mockResolvedValue({ ...stateConversations[0], project_id: example.id });
    vi.spyOn(api, "listConversations").mockResolvedValue([]);
    const compute = vi.spyOn(api, "agentV4ComputeBackends").mockResolvedValue([]);
    const direct = vi.spyOn(api, "agentV4StartDirect"); const planning = vi.spyOn(api, "agentV4StartPlanning");
    render(<DesktopApp />);
    await waitFor(() => expect(screen.queryByText("OmicsOps", { selector: ".desktop-loading" })).not.toBeInTheDocument());
    expect(create).not.toHaveBeenCalled(); expect(compute).not.toHaveBeenCalled();
    expect(direct).not.toHaveBeenCalled(); expect(planning).not.toHaveBeenCalled();
    expect(screen.getByText(/合成数据，预置结果/)).toBeInTheDocument();
  });
  it("passes the current project binding to remote job observation on load and settings update", async () => {
    const { stateSpy } = setupConversationStateHarness();
    stateSpy.mockImplementation(async (_projectId, conversationId) => stateSnapshot(conversationId));
    const connection = { id: "connection-jobs", label: "Trusted cluster", host: "compute.example.org", port: 22, username: "scientist", authentication: "password" as const, authentication_reference: "ssh/connection-jobs", host_key_fingerprint: "SHA256:trusted" };
    const nextConnection = { ...connection, id: "connection-jobs-next", label: "Next trusted cluster" };
    const boundProject = { ...stateProject, connection_id: connection.id, remote_root: "/srv/original" };
    vi.mocked(api.listProjects).mockResolvedValue([boundProject]);
    vi.spyOn(api, "listConnections").mockResolvedValue([connection, nextConnection]);
    vi.spyOn(api, "updateProjectRemote").mockResolvedValue({ ...boundProject, connection_id: nextConnection.id, remote_root: "/srv/changed" });
    const panel = vi.spyOn(remoteJobsPanel, "RemoteJobsPanel").mockImplementation(({ bindingVersion }) => <output aria-label="Observation binding">{bindingVersion}</output>);
    render(<DesktopApp />);
    await screen.findByRole("heading", { name: stateConversations[0].title });
    await waitFor(() => expect(screen.getByRole("textbox", { name: /描述研究目标/ })).toBeEnabled());
    fireEvent.click(screen.getByRole("button", { name: "Python 环境" }));
    fireEvent.click(screen.getByRole("button", { name: "远端作业" }));
    expect(screen.getByLabelText("Observation binding")).toHaveTextContent(JSON.stringify([connection.id, "/srv/original"]));
    expect(panel.mock.calls.at(-1)?.[0]).toMatchObject({ projectId: stateProject.id, conversationId: stateConversations[0].id });
    fireEvent.click(screen.getByRole("button", { name: "设置" })); fireEvent.click(screen.getByRole("button", { name: "环境" }));
    fireEvent.change(screen.getByLabelText("Project SSH connection"), { target: { value: nextConnection.id } });
    fireEvent.change(screen.getByLabelText("Remote project root"), { target: { value: "/srv/changed" } });
    fireEvent.click(screen.getByRole("button", { name: "保存项目绑定" }));
    await waitFor(() => expect(screen.getByLabelText("Observation binding")).toHaveTextContent(JSON.stringify([nextConnection.id, "/srv/changed"])));
  });
  it("restores composer text after leaving and reopening a project", async () => {
    const { stateSpy } = setupConversationStateHarness();
    stateSpy.mockImplementation(async (_projectId, conversationId) => stateSnapshot(conversationId));
    render(<DesktopApp />);
    const input = await screen.findByRole("textbox", { name: /描述研究目标/ });
    await waitFor(() => expect(input).toBeEnabled());
    fireEvent.change(input, { target: { value: "Persistent window notes" } });
    fireEvent.click(screen.getByRole("button", { name: "返回项目主页" }));
    fireEvent.click(await screen.findByRole("button", { name: `打开项目：${stateProject.name}` }));
    await screen.findByRole("heading", { name: stateConversations[0].title });
    expect(screen.getByRole("textbox", { name: /描述研究目标/ })).toHaveValue("Persistent window notes");
  });

  it.each(["create", "restore"] as const)("adopts unassigned notes only after confirmed creation, not %s navigation", async (path) => {
    if (path === "create") window.localStorage.setItem("omicsops.sessions.resumeLast", "false");
    const { stateSpy } = setupConversationStateHarness();
    stateSpy.mockImplementation(async (_projectId, conversationId) => stateSnapshot(conversationId));
    vi.mocked(api.listConversations).mockRejectedValueOnce(new Error("Load failed"));
    vi.mocked(api.latestUsedConversation).mockResolvedValue(path === "create" ? null : stateConversations[0]);
    vi.spyOn(api, "createConversation").mockResolvedValue({ ...stateConversations[0], id: "created", title: "Created session" });
    render(<DesktopApp />);
    await screen.findByRole("button", { name: "重试会话恢复" });
    const input = screen.getByRole("textbox", { name: /描述研究目标/ });
    // Seed the unassigned draft state; this host disables editing without a session.
    fireEvent.change(input, { target: { value: "Unassigned goal" } });
    fireEvent.click(screen.getByRole("button", { name: "重试会话恢复" }));
    await screen.findByRole("heading", { name: path === "create" ? "Created session" : stateConversations[0].title });
    expect(input).toHaveValue(path === "create" ? "Unassigned goal" : "");
    if (path === "restore") {
      vi.mocked(api.listConversations).mockRejectedValueOnce(new Error("Load failed again"));
      fireEvent.click(screen.getByRole("button", { name: "返回项目主页" }));
      fireEvent.click(await screen.findByRole("button", { name: `打开项目：${stateProject.name}` }));
      await screen.findByRole("button", { name: "重试会话恢复" });
      expect(screen.getByRole("textbox", { name: /描述研究目标/ })).toHaveValue("Unassigned goal");
    }
  });

  it("creates an empty composer from an active conversation and restores the original", async () => {
    const { stateSpy } = setupConversationStateHarness();
    stateSpy.mockImplementation(async (_projectId, conversationId) => stateSnapshot(conversationId));
    vi.spyOn(api, "createConversation").mockResolvedValue({ ...stateConversations[0], id: "new", title: "New session" });
    render(<DesktopApp />);
    const input = await screen.findByRole("textbox", { name: /描述研究目标/ });
    await waitFor(() => expect(input).toBeEnabled());
    fireEvent.change(input, { target: { value: "Original goal" } });
    fireEvent.click(screen.getByRole("button", { name: "新建会话" }));
    await screen.findByRole("heading", { name: "New session" });
    expect(input).toHaveValue("");
    fireEvent.click(screen.getByRole("button", { name: stateConversations[0].title }));
    await screen.findByRole("heading", { name: stateConversations[0].title });
    expect(input).toHaveValue("Original goal");
  });

  it.each(["success", "failure"] as const)("purges only a confirmed deletion after switching away: %s", async (result) => {
    const { stateSpy } = setupConversationStateHarness();
    stateSpy.mockImplementation(async (_projectId, conversationId) => stateSnapshot(conversationId));
    const deletion = deferred<void>();
    vi.spyOn(api, "deleteConversation").mockReturnValue(deletion.promise);
    vi.spyOn(window, "confirm").mockReturnValue(true);
    render(<DesktopApp />);
    const input = await screen.findByRole("textbox", { name: /描述研究目标/ });
    await waitFor(() => expect(input).toBeEnabled());
    fireEvent.change(input, { target: { value: "Delete source notes" } });
    fireEvent.click(screen.getByRole("button", { name: "删除会话：Agent 会话" }));
    fireEvent.click(screen.getByRole("button", { name: stateConversations[1].title }));
    await screen.findByRole("heading", { name: stateConversations[1].title });
    await waitFor(() => expect(input).toBeEnabled());
    fireEvent.change(input, { target: { value: "Keep target notes" } });
    await act(async () => { if (result === "success") deletion.resolve(); else deletion.reject(new Error("Delete failed")); });
    expect(input).toHaveValue("Keep target notes");
    // The existing UI generation guard keeps the old list until refresh.
    fireEvent.click(screen.getByRole("button", { name: stateConversations[0].title }));
    await screen.findByRole("heading", { name: stateConversations[0].title });
    expect(input).toHaveValue(result === "success" ? "" : "Delete source notes");
  });

  it.each([false, true])("global Search owns focus above real Runtime and kernel=%s", async (kernel) => {
    const { stateSpy } = setupConversationStateHarness();
    stateSpy.mockImplementation(async (_projectId, conversationId) => stateSnapshot(conversationId));
    const event = agentEvent(stateConversations[0].id, "run-search", 1, { kind: "run_spec_frozen", approval_hash: "approval", spec_hash: "spec" });
    vi.mocked(api.agentV4EventsForConversation).mockResolvedValue([event, agentEvent(stateConversations[0].id, "run-search", 2, { kind: "run_completed" })]);
    vi.spyOn(api, "agentV4RuntimeBoundary").mockResolvedValue({ project_id: stateProject.id, source: { kind: "frozen_run", run_id: "run-search" }, compute_selection: { schema_version: 4, backend_id: "local", backend_kind: "local", autonomy_mode: "supervised", environment: "system", network_policy: "host_inherited" }, execution_location: "local_host", isolation: "process", limits: ["same_user_permissions"], interactive_lifecycle: "run_scoped_no_restart_reconnect", detached_job_lifecycle: "unsupported", verification: "not_checked_by_this_view" });
    vi.spyOn(referenceApi, "composerReferenceCatalog").mockResolvedValue([]);
    render(<DesktopApp />);
    const composer = await screen.findByRole("textbox", { name: /描述研究目标/ });
    await waitFor(() => expect(composer).toBeEnabled());
    fireEvent.change(composer, { target: { value: "/trajectory" } });
    fireEvent.click(screen.getByRole("option", { name: /\/trajectory/ }));
    const trajectory = screen.getByRole("dialog", { name: "运行轨迹" });
    fireEvent.click(await within(trajectory).findByRole("button", { name: "运行环境" }));
    const runtime = screen.getByRole("dialog", { name: "运行时" });
    if (kernel) fireEvent.click(within(runtime).getByRole("button", { name: "查看内核元数据" }));
    const owner = kernel ? screen.getByRole("dialog", { name: "内核元数据" }) : runtime;
    const previous = within(owner).getByRole("button", { name: kernel ? "关闭内核元数据" : "关闭运行时" });
    previous.focus();
    fireEvent.keyDown(window, { key: "k", ctrlKey: true });
    const search = screen.getByRole("dialog", { name: "搜索工作区" });
    const input = within(search).getByRole("searchbox", { name: "搜索工作区内容" });
    expect(input).toHaveFocus();
    // A lower window trap must remain suspended even outside DOM bubbling.
    for (const shiftKey of [false, true]) {
      const tab = new KeyboardEvent("keydown", { key: "Tab", shiftKey, bubbles: true, cancelable: true });
      window.dispatchEvent(tab);
      expect(tab.defaultPrevented).toBe(false);
      expect(input).toHaveFocus();
    }
    const searchClose = within(search).getByRole("button", { name: "关闭搜索" });
    fireEvent.keyDown(input, { key: "Tab", shiftKey: true });
    expect(searchClose).toHaveFocus();
    fireEvent.keyDown(searchClose, { key: "Tab", shiftKey: true });
    expect(searchClose).not.toHaveFocus();
    expect(search.contains(document.activeElement)).toBe(true);
    fireEvent.keyDown(document.activeElement!, { key: "Tab" });
    expect(searchClose).toHaveFocus();
    // Cycle past both ends: actual Search controls retain keyboard ownership.
    for (const shiftKey of [false, true]) for (let index = 0; index < within(search).getAllByRole("button").length + 2; index++) {
      fireEvent.keyDown(document.activeElement!, { key: "Tab", shiftKey });
      expect(search.contains(document.activeElement)).toBe(true);
    }
    fireEvent.keyDown(window, { key: "Escape" });
    expect(screen.queryByRole("dialog", { name: "搜索工作区" })).not.toBeInTheDocument();
    expect(runtime).toBeVisible(); expect(trajectory).toBeVisible(); expect(previous).toHaveFocus();
    fireEvent.keyDown(window, { key: "Tab", shiftKey: true });
    expect(owner.contains(document.activeElement)).toBe(true);
    fireEvent.keyDown(window, { key: "Escape" });
    if (kernel) { expect(screen.queryByRole("dialog", { name: "内核元数据" })).not.toBeInTheDocument(); expect(runtime).toBeVisible(); }
    else expect(screen.queryByRole("dialog", { name: "运行时" })).not.toBeInTheDocument();
    expect(trajectory).toBeVisible();
  });

  it("does not describe a container until its current project and image response resolves", async () => {
    const { stateSpy } = setupConversationStateHarness();
    stateSpy.mockImplementation(async (_projectId, conversationId) => stateSnapshot(conversationId));
    const docker = { ...stateBackend, descriptor: { ...stateBackend.descriptor, backend_id: "docker", kind: "docker" as const, isolation: "container" as const, supports_network_policy: true }, python_status: "unverified" as const, r_status: "unverified" as const, resolved_image_id: "sha256:old" };
    const pending = deferred<Awaited<ReturnType<typeof api.agentV4ComputeBackends>>>();
    vi.mocked(api.agentV4ComputeBackends).mockImplementation((_projectId, image) => image ? pending.promise : Promise.resolve([stateBackend, docker]));
    const boundary = vi.spyOn(api, "agentV4RuntimeBoundary").mockImplementation(async (request) => ({ project_id: request.project_id, source: { kind: "draft_selection" }, compute_selection: request.source === "draft_selection" ? request.compute_selection : { schema_version: 4, backend_id: "local", backend_kind: "local", autonomy_mode: "supervised", environment: "system", network_policy: "host_inherited" }, execution_location: "local_container", isolation: "container", limits: ["project_mount_read_write"], interactive_lifecycle: "run_scoped_no_restart_reconnect", detached_job_lifecycle: "unsupported", verification: "not_checked_by_this_view" }));
    render(<DesktopApp />);
    await screen.findByRole("heading", { name: stateConversations[0].title });
    fireEvent.click(screen.getByRole("button", { name: "选择计算后端" }));
    fireEvent.click(await screen.findByRole("radio", { name: /DOCKER/ }));
    fireEvent.click(screen.getByRole("button", { name: "Python 环境" }));
    expect(screen.getByRole("dialog", { name: "运行时" })).toHaveTextContent("配置未完成");
    expect(boundary).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "关闭运行时" }));
    fireEvent.click(screen.getByRole("button", { name: "选择计算后端" }));
    fireEvent.change(screen.getByRole("textbox", { name: "容器镜像" }), { target: { value: "python:new" } });
    fireEvent.click(screen.getByRole("button", { name: "Python 环境" }));
    expect(screen.getByRole("dialog", { name: "运行时" })).toHaveTextContent("配置未完成");
    expect(boundary).not.toHaveBeenCalled();
    await waitFor(() => expect(api.agentV4ComputeBackends).toHaveBeenCalledWith(stateProject.id, "python:new"));
    expect(boundary).not.toHaveBeenCalled();
    await act(async () => pending.resolve([{ ...docker, resolved_image_id: "sha256:new" }]));
    await waitFor(() => expect(boundary).toHaveBeenCalledWith(expect.objectContaining({ source: "draft_selection", compute_selection: expect.objectContaining({ container_image: { reference: "python:new", image_id: "sha256:new" } }) })));
  });

  it("clears a pending runtime boundary immediately when switching conversations", async () => {
    const { stateSpy } = setupConversationStateHarness();
    stateSpy.mockImplementation(async (_projectId, conversationId) => stateSnapshot(conversationId));
    const pending = deferred<Awaited<ReturnType<typeof api.agentV4RuntimeBoundary>>>();
    const query = vi.spyOn(api, "agentV4RuntimeBoundary").mockReturnValue(pending.promise);
    render(<DesktopApp />);
    await screen.findByRole("heading", { name: stateConversations[0].title });
    await waitFor(() => expect(api.agentV4ComputeBackends).toHaveBeenCalledWith(stateProject.id, ""));
    fireEvent.click(screen.getByRole("button", { name: "Python 环境" }));
    await waitFor(() => expect(query).toHaveBeenCalledWith(expect.objectContaining({ source: "draft_selection", project_id: stateProject.id })));
    expect(screen.getByRole("dialog", { name: "运行时" })).toHaveTextContent("正在加载运行边界");
    fireEvent.click(screen.getByRole("button", { name: stateConversations[1].title }));
    await waitFor(() => expect(screen.queryByRole("dialog", { name: "运行时" })).not.toBeInTheDocument());
    await act(async () => pending.resolve({ project_id: stateProject.id, source: { kind: "draft_selection" }, compute_selection: { schema_version: 4, backend_id: "local", backend_kind: "local", autonomy_mode: "supervised", environment: "system", network_policy: "host_inherited" }, execution_location: "local_host", isolation: "process", limits: ["same_user_permissions"], interactive_lifecycle: "run_scoped_no_restart_reconnect", detached_job_lifecycle: "unsupported", verification: "not_checked_by_this_view" }));
    expect(screen.queryByRole("dialog", { name: "运行时" })).not.toBeInTheDocument();
  });

  it("waits for asynchronous session search before closing the dialog", async () => {
    const { stateSpy } = setupConversationStateHarness();
    stateSpy.mockImplementation(async (_projectId, conversationId) => stateSnapshot(conversationId));
    vi.spyOn(referenceApi, "composerReferenceCatalog").mockResolvedValue([{ reference: { kind: "session", project_id: stateProject.id, id: stateConversations[1].id }, label: "Async session", description: "Saved transcript" }]);
    render(<DesktopApp />);
    await screen.findByRole("heading", { name: stateConversations[0].title });
    const pending = deferred<typeof stateConversations>();
    vi.mocked(api.listConversations).mockReturnValueOnce(pending.promise);
    fireEvent.keyDown(window, { key: "k", ctrlKey: true });
    fireEvent.click(await screen.findByRole("button", { name: "打开 Async session" }));
    expect(screen.getByRole("dialog", { name: "搜索工作区" })).toBeInTheDocument();
    await act(async () => pending.resolve(stateConversations));
    expect(await screen.findByRole("heading", { name: stateConversations[1].title })).toBeInTheDocument();
    await waitFor(() => expect(screen.queryByRole("dialog", { name: "搜索工作区" })).not.toBeInTheDocument());
  });

  it("keyboard navigation decision owns focus above Search and restores it on cancellation", async () => {
    const { stateSpy } = setupConversationStateHarness();
    stateSpy.mockImplementation(async (_projectId, conversationId) => stateSnapshot(conversationId));
    vi.spyOn(navigationApi, "isWorkspaceDesktopHost").mockReturnValue(true);
    vi.spyOn(navigationApi, "listWorkspacePublications").mockResolvedValue([]);
    const save = deferred<Awaited<ReturnType<typeof navigationApi.saveWorkspacePublication>>>();
    const saveRequest = vi.spyOn(navigationApi, "saveWorkspacePublication").mockReturnValue(save.promise);
    vi.spyOn(referenceApi, "composerReferenceCatalog").mockResolvedValue([{ reference: { kind: "session", project_id: stateProject.id, id: stateConversations[1].id }, label: "Guarded keyboard session", description: "Saved transcript" }]);
    render(<DesktopApp />);
    await screen.findByRole("heading", { name: stateConversations[0].title });
    fireEvent.click(screen.getByRole("button", { name: "发表工作区" }));
    fireEvent.click(await screen.findByRole("button", { name: "新建稿件" }));
    fireEvent.change(screen.getByRole("textbox", { name: "标题" }), { target: { value: "Keyboard draft" } });
    fireEvent.change(screen.getByRole("textbox", { name: "Markdown" }), { target: { value: "Keep keyboard draft" } });
    fireEvent.keyDown(window, { key: "k", ctrlKey: true });
    const search = screen.getByRole("dialog", { name: "搜索工作区" });
    const input = within(search).getByRole("searchbox", { name: "搜索工作区内容" });
    fireEvent.change(input, { target: { value: "Guarded keyboard session" } });
    const result = await within(search).findByRole("button", { name: "打开 Guarded keyboard session" });
    await waitFor(() => expect(result).toBeEnabled());
    expect(input).toHaveFocus();
    fireEvent.keyDown(input, { key: "Enter" });
    const decision = await screen.findByRole("dialog", { name: "未保存的稿件" });
    const close = within(decision).getByRole("button", { name: "关闭" });
    const initialTopFocus = document.activeElement === close;
    // Even a stray event from the covered input cannot reclaim the top decision's focus.
    close.focus();
    const lowerTab = new KeyboardEvent("keydown", { key: "Tab", bubbles: true, cancelable: true });
    fireEvent(input, lowerTab);
    expect({ initialTopFocus, lowerSearchConsumedTab: lowerTab.defaultPrevented, topFocusAfterLowerTab: document.activeElement === close }).toEqual({ initialTopFocus: true, lowerSearchConsumedTab: false, topFocusAfterLowerTab: true });
    const keepEditing = within(decision).getByRole("button", { name: "继续编辑" });
    fireEvent.keyDown(close, { key: "Tab", shiftKey: true }); expect(keepEditing).toHaveFocus();
    fireEvent.keyDown(keepEditing, { key: "Tab" }); expect(close).toHaveFocus();
    const saveAndLeave = within(decision).getByRole("button", { name: "保存并离开" });
    fireEvent.keyDown(close, { key: "Tab" }); expect(saveAndLeave).toHaveFocus();
    fireEvent.click(saveAndLeave);
    await waitFor(() => expect(saveRequest).toHaveBeenCalledTimes(1));
    expect(saveAndLeave).toBeDisabled();
    expect(within(decision).getByRole("button", { name: "放弃并离开" })).toBeDisabled();
    // Controls are recomputed after busy state changes, so disabled actions are skipped.
    close.focus(); fireEvent.keyDown(close, { key: "Tab" }); expect(keepEditing).toHaveFocus();
    fireEvent.keyDown(keepEditing, { key: "Tab", shiftKey: true }); expect(close).toHaveFocus();
    fireEvent.keyDown(window, { key: "Escape" });
    expect(screen.queryByRole("dialog", { name: "未保存的稿件" })).not.toBeInTheDocument();
    expect(search).toBeVisible(); expect(input).toHaveFocus();
    expect(screen.getByRole("textbox", { name: "Markdown" })).toHaveValue("Keep keyboard draft");
    await act(async () => save.reject(new Error("save unavailable")));
    expect(input).toHaveFocus();
    await waitFor(() => expect(result).toBeEnabled());
    fireEvent.keyDown(input, { key: "Tab", shiftKey: true });
    expect(within(search).getByRole("button", { name: "关闭搜索" })).toHaveFocus();
    fireEvent.keyDown(window, { key: "Escape" });
    expect(screen.queryByRole("dialog", { name: "搜索工作区" })).not.toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: "Markdown" })).toHaveValue("Keep keyboard draft");
  });

  it("keeps a dirty manuscript when cancelling search navigation and permits retry", async () => {
    const { stateSpy } = setupConversationStateHarness();
    stateSpy.mockImplementation(async (_projectId, conversationId) => stateSnapshot(conversationId));
    vi.spyOn(navigationApi, "isWorkspaceDesktopHost").mockReturnValue(true);
    vi.spyOn(navigationApi, "listWorkspacePublications").mockResolvedValue([]);
    vi.spyOn(referenceApi, "composerReferenceCatalog").mockResolvedValue([{ reference: { kind: "session", project_id: stateProject.id, id: stateConversations[1].id }, label: "Guarded session", description: "Saved transcript" }]);
    render(<DesktopApp />);
    await screen.findByRole("heading", { name: stateConversations[0].title });
    fireEvent.click(screen.getByRole("button", { name: "发表工作区" }));
    fireEvent.click(await screen.findByRole("button", { name: "新建稿件" }));
    fireEvent.change(screen.getByRole("textbox", { name: "Markdown" }), { target: { value: "Keep this draft" } });
    fireEvent.click(screen.getByRole("button", { name: "文件" }));
    await screen.findByRole("dialog", { name: "未保存的稿件" });
    fireEvent.keyDown(window, { key: "k", ctrlKey: true });
    expect(screen.queryByRole("dialog", { name: "搜索工作区" })).not.toBeInTheDocument();
    fireEvent.keyDown(window, { key: "Escape" });
    fireEvent.keyDown(window, { key: "k", ctrlKey: true });
    fireEvent.click(await screen.findByRole("button", { name: "打开 Guarded session" }));
    expect(await screen.findByRole("dialog", { name: "未保存的稿件" })).toBeInTheDocument();
    fireEvent.keyDown(window, { key: "Escape" });
    expect(screen.getByRole("textbox", { name: "Markdown" })).toHaveValue("Keep this draft");
    expect(screen.getByRole("dialog", { name: "搜索工作区" })).toBeInTheDocument();
    await waitFor(() => expect(screen.getByRole("button", { name: "打开 Guarded session" })).toBeEnabled());
    fireEvent.click(screen.getByRole("button", { name: "打开 Guarded session" }));
    fireEvent.click(await screen.findByRole("button", { name: "放弃并离开" }));
    expect(await screen.findByRole("heading", { name: stateConversations[1].title })).toBeInTheDocument();
  });

  it("creates one session with Ctrl+N and suppresses the shortcut while a dialog is open", async () => {
    const { stateSpy } = setupConversationStateHarness();
    stateSpy.mockImplementation(async (_projectId, conversationId) => stateSnapshot(conversationId));
    const create = vi.spyOn(api, "createConversation").mockResolvedValue({ ...stateConversations[0], id: "shortcut-session", title: "Shortcut session" });
    render(<DesktopApp />);
    const input = await screen.findByRole("textbox", { name: /描述研究目标/ });
    await waitFor(() => expect(input).toBeEnabled());
    fireEvent.keyDown(window, { key: "n", ctrlKey: true, repeat: true });
    expect(create).not.toHaveBeenCalled();
    fireEvent.keyDown(window, { key: "n", ctrlKey: true });
    await waitFor(() => expect(create).toHaveBeenCalledTimes(1));
    await screen.findByRole("heading", { name: "Shortcut session" });
    fireEvent.keyDown(window, { key: "k", ctrlKey: true });
    await screen.findByRole("dialog", { name: "搜索工作区" });
    fireEvent.keyDown(window, { key: "n", ctrlKey: true });
    expect(create).toHaveBeenCalledTimes(1);
  });

  it.each(["menu", "help", "help over search"])("closes chrome %s before the global search shortcut takes over", async (surface) => {
    vi.spyOn(api, "listProjects").mockResolvedValue([]);
    render(<DesktopApp />);
    await screen.findByRole("heading", { name: "最近项目" });
    if (surface === "help over search") fireEvent.keyDown(window, { key: "k", ctrlKey: true });
    fireEvent.click(screen.getByRole("menuitem", { name: "Help" }));
    if (surface !== "menu") fireEvent.click(screen.getByRole("menuitem", { name: "快速入门" }));
    fireEvent.keyDown(window, { key: "k", ctrlKey: true });
    expect(await screen.findByRole("dialog", { name: "搜索工作区" })).toBeInTheDocument();
    expect(screen.queryByRole("menu")).not.toBeInTheDocument();
    expect(screen.queryByRole("dialog", { name: "快速入门" })).not.toBeInTheDocument();
    fireEvent.keyDown(window, { key: "Escape" });
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("honors a new-project menu action while the initial project list is loading", async () => {
    const pendingProjects = deferred<typeof stateProject[]>();
    vi.spyOn(api, "listProjects").mockReturnValue(pendingProjects.promise);
    render(<DesktopApp />);
    fireEvent.click(screen.getByRole("menuitem", { name: "File" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "新建项目" }));
    await act(async () => pendingProjects.resolve([stateProject]));
    expect(await screen.findByRole("dialog", { name: "创建新项目" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "最近项目" })).toBeInTheDocument();
  });

  it("offers the four application menus and opens a new project from File", async () => {
    vi.spyOn(api, "listProjects").mockResolvedValue([]);
    render(<DesktopApp />);
    const bar = await screen.findByRole("menubar");
    for (const name of ["File", "Edit", "View", "Help"]) {
      expect(within(bar).getByRole("menuitem", { name })).toBeInTheDocument();
    }
    fireEvent.click(within(bar).getByRole("menuitem", { name: "File" }));
    expect(screen.getByRole("menuitem", { name: "新会话" })).toBeDisabled();
    fireEvent.click(screen.getByRole("menuitem", { name: "新建项目" }));
    expect(await screen.findByRole("dialog", { name: "创建新项目" })).toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: "项目名称" })).toHaveValue("空白研究项目");
    fireEvent.keyDown(window, { key: "Escape" });
    expect(screen.queryByRole("dialog", { name: "创建新项目" })).not.toBeInTheDocument();
    fireEvent.click(within(bar).getByRole("menuitem", { name: "File" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "新建项目" }));
    expect(await screen.findByRole("dialog", { name: "创建新项目" })).toBeInTheDocument();
  });

  it("routes menu actions in the workspace and dismisses only the top menu over settings", async () => {
    const { stateSpy } = setupConversationStateHarness();
    stateSpy.mockImplementation(async (_projectId, conversationId) => stateSnapshot(conversationId));
    render(<DesktopApp />);
    await screen.findByRole("heading", { name: stateConversations[0].title });
    fireEvent.click(screen.getByRole("menuitem", { name: "Edit" }));
    fireEvent.click(screen.getByRole("menuitem", { name: /搜索/ }));
    expect(await screen.findByRole("dialog", { name: "搜索工作区" })).toBeInTheDocument();
    fireEvent.keyDown(window, { key: "Escape" });
    fireEvent.click(screen.getByRole("menuitem", { name: "File" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "设置" }));
    const settings = await screen.findByRole("dialog", { name: /设置/ });
    fireEvent.click(screen.getByRole("menuitem", { name: "View" }));
    fireEvent.keyDown(window, { key: "Escape" });
    expect(screen.queryByRole("menu")).not.toBeInTheDocument();
    expect(settings).toBeInTheDocument();
    fireEvent.keyDown(window, { key: "Escape" });
    expect(settings).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("menuitem", { name: "File" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "项目列表" }));
    expect(await screen.findByRole("heading", { name: "最近项目" })).toBeInTheDocument();
    expect(screen.getByRole("menubar")).toBeInTheDocument();
  });

  it("opens project files from View over settings", async () => {
    const { stateSpy } = setupConversationStateHarness();
    stateSpy.mockImplementation(async (_projectId, conversationId) => stateSnapshot(conversationId));
    render(<DesktopApp />);
    await screen.findByRole("heading", { name: stateConversations[0].title });
    fireEvent.click(screen.getByRole("menuitem", { name: "File" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "设置" }));
    await screen.findByRole("dialog", { name: "工作台设置" });
    fireEvent.click(screen.getByRole("menuitem", { name: "View" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "项目文件" }));
    expect(await screen.findByRole("tab", { name: "Files" })).toHaveAttribute("aria-selected", "true");
    expect(screen.queryByRole("dialog", { name: "工作台设置" })).not.toBeInTheDocument();
  });

  it("refreshes model and conversation selections after deleting a profile", async () => {
    const { stateSpy } = setupConversationStateHarness();
    stateSpy.mockImplementation(async (_projectId, conversationId) => stateSnapshot(conversationId));
    const listProfiles = vi.mocked(api.listModelProfiles);
    const listConversations = vi.mocked(api.listConversations);
    const remove = vi.spyOn(api, "deleteModelProfile").mockResolvedValue(true);
    render(<DesktopApp />);
    await screen.findByText(/Agent 模式：LOCAL/);

    fireEvent.click(screen.getByRole("button", { name: "选择模型" }));
    fireEvent.click(screen.getByRole("button", { name: "设置" }));
    expect(await screen.findByRole("button", { name: "删除 State model" })).toBeInTheDocument();
    listProfiles.mockResolvedValue([]);
    listConversations.mockResolvedValue(stateConversations.map((item) => ({ ...item, model_profile_id: null })));
    fireEvent.click(screen.getByRole("button", { name: "删除 State model" }));
    fireEvent.click(within(screen.getByRole("alertdialog")).getByRole("button", { name: "删除" }));

    await waitFor(() => expect(remove).toHaveBeenCalledWith("model-state"));
    await waitFor(() => expect(listProfiles).toHaveBeenCalledTimes(2));
    await waitFor(() => expect(listConversations).toHaveBeenCalledTimes(2));
    expect(screen.queryByText("State model")).not.toBeInTheDocument();
  });

  it("keeps a completed deletion local when follow-up refreshes fail", async () => {
    const { stateSpy } = setupConversationStateHarness();
    stateSpy.mockImplementation(async (_projectId, conversationId) => stateSnapshot(conversationId));
    const listProfiles = vi.mocked(api.listModelProfiles);
    const listConversations = vi.mocked(api.listConversations);
    vi.spyOn(api, "deleteModelProfile").mockResolvedValue(true);
    render(<DesktopApp />);
    await screen.findByText(/Agent 模式：LOCAL/);

    fireEvent.click(screen.getByRole("button", { name: "选择模型" }));
    fireEvent.click(screen.getByRole("button", { name: "设置" }));
    await screen.findByRole("button", { name: "删除 State model" });
    listProfiles.mockRejectedValue(new Error("profile refresh failed"));
    listConversations.mockRejectedValue(new Error("conversation refresh failed"));
    fireEvent.click(screen.getByRole("button", { name: "删除 State model" }));
    fireEvent.click(within(screen.getByRole("alertdialog")).getByRole("button", { name: "删除" }));

    expect(await screen.findByText(/模型提供方已删除，但部分工作区状态无法刷新/)).toBeInTheDocument();
    expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument();
    expect(screen.queryByText("State model")).not.toBeInTheDocument();
  });

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
    vi.mocked(api.getWorkspaceConversation).mockImplementation(async (projectId, id) => projectId === other.id ? [first, requested].find((item) => item.id === id)! : stateConversations.find((item) => item.id === id)!);
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
    vi.spyOn(api, "listModelProfileModelDiscovery").mockResolvedValue({ models: [profile.model], source: "provider", can_refresh: true });
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
    vi.spyOn(api, "listModelProfileModelDiscovery").mockResolvedValue({ models: [profile.model], source: "provider", can_refresh: true });
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
    const listModels = vi.spyOn(api, "listModelProfileModelDiscovery").mockResolvedValue({ models: ["research-model-a", "research-model-b"], source: "provider", can_refresh: true });
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
    vi.spyOn(api, "listModelProfileModelDiscovery").mockResolvedValue({ models: [stateModel.model, "other-api-model"], source: "provider", can_refresh: true });
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

    expect(await screen.findByRole("heading", { name: "还没有项目" })).toBeInTheDocument();
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

    await waitFor(() => expect(startDirect).toHaveBeenCalledWith(expect.objectContaining({ objective: "先解释一下这个矩阵格式", compute_selection: expect.objectContaining({ approval_policy: "auto_approve_except_local_deletion" }) })));
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

  for (const mode of ["agent", "plan"] as const) {
    it.each(["accepted", "same text", "unknown", "null", "invalid"] as const)(`preserves durable legacy ${mode} start acceptance across navigation: %s`, async (outcome) => {
      const { stateSpy } = setupConversationStateHarness();
      stateSpy.mockImplementation(async (_projectId, conversationId) => stateSnapshot(conversationId, { mode }));
      vi.spyOn(api, "submitMessage").mockImplementation(async (request) => ({ id: "accepted-message", project_id: request.project_id, conversation_id: request.conversation_id, sequence: 1, role: "user", markdown: request.markdown, created_at: "" }));
      const pending = deferred<RunSummaryV4>();
      const start = vi.spyOn(api, mode === "agent" ? "agentV4StartDirect" : "agentV4StartPlanning").mockReturnValue(pending.promise);
      const subscribe = vi.spyOn(ComposerDraftStore.prototype, "subscribe");
      render(<DesktopApp />);
      const first = await screen.findByRole("textbox", { name: /描述研究目标/ });
      await waitFor(() => expect(first).toBeEnabled());
      const store = subscribe.mock.contexts[0] as ComposerDraftStore;
      const a = { projectId: stateProject.id, conversationId: "conversation-agent" };
      const b = { ...a, conversationId: "conversation-plan" };
      fireEvent.change(first, { target: { value: "Original notes" } });
      await waitFor(() => expect(screen.getByRole("button", { name: "发送" })).toBeEnabled());
      fireEvent.click(screen.getByRole("button", { name: "发送" }));
      await waitFor(() => expect(start).toHaveBeenCalledOnce());
      fireEvent.click(screen.getByRole("button", { name: "Plan 会话" }));
      await screen.findByRole("heading", { name: "Plan 会话" });
      const current = screen.getByRole("textbox", { name: /描述研究目标/ });
      act(() => store.update(b, "Destination notes"));
      expect(current).toHaveValue("Destination notes");
      if (outcome === "same text") act(() => { store.update(a, "Changed notes"); store.update(a, "Original notes"); });
      await act(async () => {
        if (outcome === "unknown") pending.reject(new Error("unknown start"));
        else if (outcome === "null") pending.resolve(null as unknown as RunSummaryV4);
        else pending.resolve({ ...stateRun("accepted-original", mode === "agent" ? "running" : "awaiting_approval"), run_id: outcome === "invalid" ? "" : "accepted-original", session_mode: mode });
      });
      expect(store.read(a).text).toBe(outcome === "accepted" ? "" : "Original notes");
      expect(store.read(b).text).toBe("Destination notes");
      expect(current).toHaveValue("Destination notes");
      expect(screen.queryByText("unknown start")).not.toBeInTheDocument();
      expect(api.agentV4Events).not.toHaveBeenCalledWith("accepted-original");
    });
  }

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
      expect(screen.getByText("运行状态暂时无法刷新")).toBeInTheDocument();
      expect(screen.queryByText("对话未完成")).not.toBeInTheDocument();
      expect(screen.getByRole("button", { name: "终止运行" })).toBeEnabled();
      const readsBeforeInterval = readEvents.mock.calls.length;
      await act(async () => { await vi.advanceTimersByTimeAsync(3_000); });
      expect(readEvents.mock.calls.length).toBeGreaterThan(readsBeforeInterval);
      expect(screen.getByText("轮询恢复了进度。")).toBeInTheDocument();
      expect(screen.queryByText("temporary event verification failure")).not.toBeInTheDocument();
      expect(screen.queryByText("运行状态暂时无法刷新")).not.toBeInTheDocument();
    } finally {
      vi.useRealTimers();
    }
  });

  it("clears a run polling failure when switching conversations", async () => {
    const { stateSpy } = setupConversationStateHarness();
    stateSpy.mockImplementation(async (_projectId, conversationId) => stateSnapshot(conversationId));
    const started = agentEvent(stateConversations[0].id, "run-poll-scope", 1, { kind: "run_created", mode: "execute" });
    vi.mocked(api.agentV4EventsForConversation).mockImplementation(async (_projectId, conversationId) => conversationId === stateConversations[0].id ? [started] : []);
    vi.mocked(api.agentV4Events).mockRejectedValue(new Error("old conversation pool timeout"));
    const shell = vi.spyOn(workspaceShell, "WorkspaceShell");
    render(<DesktopApp />);
    await screen.findByText("old conversation pool timeout");
    await act(async () => { await shell.mock.calls.at(-1)![0].onSelectConversation?.(stateConversations[1].id); });
    await waitFor(() => expect(shell.mock.calls.at(-1)?.[0].activeConversationId).toBe(stateConversations[1].id));
    expect(screen.queryByText("old conversation pool timeout")).not.toBeInTheDocument();
    expect(screen.queryByText("运行状态暂时无法刷新")).not.toBeInTheDocument();
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

    await waitFor(() => expect(startPlanning).toHaveBeenCalledWith(expect.objectContaining({ objective: "执行完整 QC", compute_selection: { ...selection, approval_policy: "auto_approve_except_local_deletion" } })));
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
    await waitFor(() => expect(stateSpy).toHaveBeenCalledWith(stateProject.id, "conversation-agent"));
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
    expect(await screen.findByRole("heading", { name: "最近项目" })).toBeInTheDocument();
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

  it.each(["success", "failure"] as const)("purges project drafts only after confirmed project deletion: %s", async (result) => {
    const { stateSpy } = setupConversationStateHarness();
    stateSpy.mockImplementation(async (_projectId, conversationId) => stateSnapshot(conversationId));
    const otherProject = { ...stateProject, id: "other-project", name: "保留项目" };
    vi.mocked(api.listProjects).mockResolvedValue([stateProject, otherProject]);
    const deletion = deferred<void>();
    vi.spyOn(api, "deleteProject").mockReturnValue(deletion.promise);
    vi.spyOn(window, "confirm").mockReturnValue(true);
    // Observe the real app-owned store through a call-through subscription spy.
    const subscribe = vi.spyOn(ComposerDraftStore.prototype, "subscribe");
    render(<DesktopApp />);
    const input = await screen.findByRole("textbox", { name: /描述研究目标/ });
    await waitFor(() => expect(input).toBeEnabled());
    const store = subscribe.mock.contexts[0] as ComposerDraftStore;
    const deletedScopes = [null, stateConversations[0].id, stateConversations[1].id].map((conversationId) => ({ projectId: stateProject.id, conversationId }));
    const retainedScopes = [null, stateConversations[0].id].map((conversationId) => ({ projectId: otherProject.id, conversationId }));
    act(() => {
      for (const scope of deletedScopes) store.update(scope, "Deleted project notes");
      for (const scope of retainedScopes) store.update(scope, "Retained project notes");
    });
    fireEvent.click(screen.getByRole("button", { name: "返回项目主页" }));
    fireEvent.click(await screen.findByRole("button", { name: `删除项目：${stateProject.name}` }));
    for (const scope of deletedScopes) expect(store.read(scope).text).toBe("Deleted project notes");
    await act(async () => {
      if (result === "success") deletion.resolve();
      else deletion.reject(new Error("Project deletion failed"));
    });
    if (result === "success") expect(screen.queryByRole("button", { name: `打开项目：${stateProject.name}` })).not.toBeInTheDocument();
    else expect(screen.getByRole("button", { name: `打开项目：${stateProject.name}` })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: `打开项目：${otherProject.name}` })).toBeInTheDocument();
    for (const scope of deletedScopes) expect(store.read(scope).text).toBe(result === "success" ? "" : "Deleted project notes");
    for (const scope of retainedScopes) expect(store.read(scope).text).toBe("Retained project notes");
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

  it("opens the exact persisted conversation selected from Usage settings", async () => {
    const { stateSpy } = setupConversationStateHarness();
    stateSpy.mockImplementation(async (_projectId, conversationId) => stateSnapshot(conversationId));
    vi.spyOn(navigationApi, "listWorkspacePublications").mockResolvedValue([]);
    const emptyCounter = { known: null, incomplete_attempts: 0 };
    const totals = { input_tokens: { known: 12, incomplete_attempts: 0 }, output_tokens: emptyCounter, reasoning_tokens: emptyCounter, cache_read_input_tokens: emptyCounter, cache_creation_input_tokens: emptyCounter, reported_total_tokens: emptyCounter, observed_attempts: 1, final_attempts: 1, partial_attempts: 0, interrupted_attempts: 0, unknown_attempts: 0 };
    vi.spyOn(usageApi, "settingsUsagePage").mockResolvedValue({ totals, projects: [], models: [], days: [], tools: [], next_cursor: null, scanned_runs: 1, omitted_runs: 0, unattributed_events: 0, snapshot_at: "2026-09-15T00:00:00Z", completeness: "complete" });
    vi.spyOn(usageApi, "settingsUsageConversations").mockResolvedValue({ items: [{ project_id: stateProject.id, conversation_id: stateConversations[1].id, label: stateConversations[1].title, latest_activity: "2026-09-15T00:00:00Z", totals, incomplete: false }], next_cursor: null, snapshot_at: "2026-09-15T00:00:00Z" });
    render(<DesktopApp />);
    await screen.findByRole("heading", { name: stateConversations[0].title });

    fireEvent.click(screen.getByRole("button", { name: "发表工作区" }));
    fireEvent.click(await screen.findByRole("button", { name: "新建稿件" }));
    fireEvent.change(screen.getByRole("textbox", { name: "Markdown" }), { target: { value: "Unsaved usage navigation draft" } });

    fireEvent.click(screen.getByRole("button", { name: "设置" }));
    fireEvent.click(screen.getByRole("button", { name: "用量" }));
    await screen.findByText(stateConversations[1].title);
    fireEvent.click(screen.getByRole("button", { name: "打开" }));
    expect(await screen.findByRole("dialog", { name: "未保存的稿件" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "继续编辑" }));
    expect(screen.getByRole("textbox", { name: "Markdown" })).toHaveValue("Unsaved usage navigation draft");
    await waitFor(() => expect(screen.getByRole("button", { name: "打开" })).toBeEnabled());
    fireEvent.click(screen.getByRole("button", { name: "打开" }));
    fireEvent.click(await screen.findByRole("button", { name: "放弃并离开" }));

    expect(await screen.findByRole("heading", { name: stateConversations[1].title })).toBeInTheDocument();
    expect(screen.queryByRole("dialog", { name: "工作台设置" })).not.toBeInTheDocument();
  });

  it("ignores a cross-project Usage navigation that resolves after Settings closes", async () => {
    const { stateSpy } = setupConversationStateHarness();
    stateSpy.mockImplementation(async (projectId, conversationId) => ({ ...stateSnapshot(conversationId), project_id: projectId }));
    const otherProject = { ...stateProject, id: "project-other-usage", name: "Other usage project" };
    const otherConversation = { ...stateConversations[0], id: "conversation-other-usage", project_id: otherProject.id, title: "Other usage conversation" };
    vi.mocked(api.listProjects).mockResolvedValue([stateProject, otherProject]);
    const pending = deferred<Awaited<ReturnType<typeof api.listConversations>>>();
    vi.mocked(api.listConversations).mockImplementation((projectId) => projectId === otherProject.id ? pending.promise : Promise.resolve(stateConversations));
    const emptyCounter = { known: null, incomplete_attempts: 0 };
    const totals = { input_tokens: { known: 12, incomplete_attempts: 0 }, output_tokens: emptyCounter, reasoning_tokens: emptyCounter, cache_read_input_tokens: emptyCounter, cache_creation_input_tokens: emptyCounter, reported_total_tokens: emptyCounter, observed_attempts: 1, final_attempts: 1, partial_attempts: 0, interrupted_attempts: 0, unknown_attempts: 0 };
    vi.spyOn(usageApi, "settingsUsagePage").mockResolvedValue({ totals, projects: [], models: [], days: [], tools: [], next_cursor: null, scanned_runs: 1, omitted_runs: 0, unattributed_events: 0, snapshot_at: "2026-09-15T00:00:00Z", completeness: "complete" });
    vi.spyOn(usageApi, "settingsUsageConversations").mockResolvedValue({ items: [{ project_id: otherProject.id, conversation_id: otherConversation.id, label: otherConversation.title, latest_activity: "2026-09-15T00:00:00Z", totals, incomplete: false }], next_cursor: null, snapshot_at: "2026-09-15T00:00:00Z" });
    render(<DesktopApp />);
    await screen.findByRole("heading", { name: stateConversations[0].title });
    fireEvent.click(screen.getByRole("button", { name: "设置" }));
    fireEvent.click(screen.getByRole("button", { name: "用量" }));
    await screen.findByText(otherConversation.title);
    fireEvent.click(screen.getByRole("button", { name: "打开" }));
    fireEvent.click(screen.getByRole("button", { name: "Close" }));

    await act(async () => pending.resolve([otherConversation]));
    expect(screen.getByRole("heading", { name: stateConversations[0].title })).toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: otherConversation.title })).not.toBeInTheDocument();
  });

  it("does not return to an old project when its remote binding finishes late", async () => {
    const { stateSpy } = setupConversationStateHarness();
    const otherProject = { ...stateProject, id: "project-other-binding", name: "Other binding project" };
    const otherConversation = { ...stateConversations[0], id: "conversation-other-binding", project_id: otherProject.id, title: "Other binding conversation" };
    const connection = { id: "connection-binding", label: "Trusted cluster", host: "compute.example.org", port: 22, username: "scientist", authentication: "password" as const, authentication_reference: "ssh/connection-binding", host_key_fingerprint: "SHA256:trusted" };
    const pending = deferred<Awaited<ReturnType<typeof api.updateProjectRemote>>>();
    vi.mocked(api.listProjects).mockResolvedValue([stateProject, otherProject]);
    vi.spyOn(api, "listConnections").mockResolvedValue([connection]);
    vi.spyOn(api, "updateProjectRemote").mockReturnValue(pending.promise);
    vi.mocked(api.listConversations).mockImplementation(async (projectId) => projectId === otherProject.id ? [otherConversation] : stateConversations);
    vi.mocked(api.latestUsedConversation).mockImplementation(async (projectId) => projectId === otherProject.id ? otherConversation : stateConversations[0]);
    stateSpy.mockImplementation(async (projectId, conversationId) => ({ ...stateSnapshot(conversationId), project_id: projectId }));
    render(<DesktopApp />);
    await screen.findByRole("heading", { name: stateConversations[0].title });

    fireEvent.click(screen.getByRole("button", { name: "设置" }));
    fireEvent.click(screen.getByRole("button", { name: "环境" }));
    fireEvent.change(screen.getByLabelText("Project SSH connection"), { target: { value: connection.id } });
    fireEvent.change(screen.getByLabelText("Remote project root"), { target: { value: "/srv/pbmc" } });
    fireEvent.click(screen.getByRole("button", { name: "保存项目绑定" }));
    await waitFor(() => expect(api.updateProjectRemote).toHaveBeenCalledWith(stateProject.id, connection.id, "/srv/pbmc"));

    fireEvent.click(screen.getByRole("button", { name: "Close" }));
    fireEvent.click(screen.getByRole("button", { name: "返回项目主页" }));
    fireEvent.click(screen.getByRole("button", { name: `打开项目：${otherProject.name}` }));
    expect(await screen.findByRole("heading", { name: otherConversation.title })).toBeInTheDocument();

    await act(async () => pending.resolve({ ...stateProject, connection_id: connection.id, remote_root: "/srv/pbmc" }));
    expect(screen.getByRole("heading", { name: otherConversation.title })).toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: stateConversations[0].title })).not.toBeInTheDocument();
  });

  it("persists the General language selection across remounts", async () => {
    vi.spyOn(api, "listProjects").mockResolvedValue([]);
    const first = render(<DesktopApp />);

    fireEvent.click(await screen.findByRole("button", { name: "设置" }));
    fireEvent.click(screen.getByRole("button", { name: "常规" }));
    fireEvent.change(screen.getByRole("combobox", { name: "界面语言" }), { target: { value: "en-US" } });
    expect(await screen.findByRole("button", { name: "Settings" })).toBeInTheDocument();
    expect(window.localStorage.getItem("omicsops.locale")).toBe("en-US");
    first.unmount();

    render(<DesktopApp />);
    expect(await screen.findByRole("button", { name: "Settings" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Settings" }));
    fireEvent.click(screen.getByRole("button", { name: "General" }));
    expect(screen.getByRole("combobox", { name: "Interface language" })).toHaveValue("en-US");
  });

  it("falls back to local deletion approval when leaving full access for a local backend", async () => {
    const { stateSpy } = setupConversationStateHarness();
    stateSpy.mockImplementation(async (_projectId, conversationId) => stateSnapshot(conversationId));
    const docker = { ...stateBackend, descriptor: { ...stateBackend.descriptor, backend_id: "docker", kind: "docker" as const, isolation: "container" as const, supports_network_policy: true }, python_status: "unverified" as const, resolved_image_id: "sha256:test" };
    vi.mocked(api.agentV4ComputeBackends).mockResolvedValue([stateBackend, docker]);
    render(<DesktopApp />);
    await screen.findByText(/Agent 模式：LOCAL/);
    fireEvent.click(screen.getByRole("button", { name: "选择计算后端" }));
    fireEvent.click(screen.getByRole("radio", { name: /DOCKER/ }));
    fireEvent.change(screen.getByRole("textbox", { name: "容器镜像" }), { target: { value: "omicsops:test" } });
    await screen.findByText(/Agent 模式：DOCKER/);
    fireEvent.click(screen.getByRole("button", { name: "Agent 权限" }));
    fireEvent.click(screen.getByRole("menuitemradio", { name: /^完全访问权限/ }));
    expect(screen.getByText(/Agent 模式：DOCKER · 完全访问/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "选择计算后端" }));
    fireEvent.click(screen.getByRole("radio", { name: /LOCAL/ }));
    expect(await screen.findByText(/Agent 模式：LOCAL · 仅本地删除询问/)).toBeInTheDocument();
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

  it("visibility_no_visible_latest shows an empty state when resume is enabled and no candidate exists", async () => {
    const { stateSpy } = setupConversationStateHarness();
    const blank = { ...stateConversations[0], id: "blank-no-candidate", title: "" };
    vi.mocked(api.latestUsedConversation).mockResolvedValue(null);
    const create = vi.spyOn(api, "createConversation").mockResolvedValue(blank);
    stateSpy.mockImplementation(async (_projectId, conversationId) => stateSnapshot(conversationId));

    render(<DesktopApp />);

    await screen.findByText("暂无可恢复的可见会话。可新建会话或打开收起的会话。");
    expect(create).not.toHaveBeenCalled();
    expect(api.listMessages).not.toHaveBeenCalled();
    expect(screen.getByRole("button", { name: "收起的会话" })).toBeEnabled();
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

    await waitFor(() => expect(create).toHaveBeenCalledWith(stateProject.id));
    await waitFor(() => expect(api.listMessages).toHaveBeenCalledWith(blank.id));
    expect(screen.getByRole("heading", { name: "新会话" })).toBeInTheDocument();
    expect(latest).not.toHaveBeenCalled();
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
    fireEvent.click(await screen.findByRole("button", { name: "打开项目：项目 B" }));
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
    fireEvent.click(await screen.findByRole("button", { name: "打开项目：项目 B" }));
    await screen.findByRole("button", { name: "重试会话恢复" });

    expect(screen.queryByRole("heading", { name: "Agent 会话" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Agent 会话" })).not.toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: /描述研究目标/ })).toBeDisabled();
  });

  it("deduplicates blank conversation creation during StrictMode effect replay", async () => {
    window.localStorage.setItem("omicsops.sessions.resumeLast", "false");
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
    fireEvent.click(await screen.findByRole("button", { name: "打开项目：项目 B" }));
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

it("keeps reasoning preview scoped to the current run and attempt across retry, terminal, and conversation changes", async () => {
  setupConversationStateHarness();
  let emit!: Parameters<typeof api.onAgentV4Event>[0];
  let preview!: Parameters<typeof api.onAgentV4ReasoningPreview>[0];
  vi.spyOn(api, "onAgentV4Event").mockImplementation(async (cb) => { emit = cb; return () => undefined; });
  vi.spyOn(api, "onAgentV4ReasoningPreview").mockImplementation(async (cb) => { preview = cb; return () => undefined; });
  render(<DesktopApp />);
  await screen.findByRole("heading", { name: "Agent 会话" });
  await waitFor(() => expect(preview).toBeDefined());
  const run = "reasoning-run";
  const request = (attemptId: string): AgentRunEventV4["event"] => ({ kind: "model_request_started", request: {
    logical_request_id: "logical", attempt_id: attemptId, model_profile_id: "profile", context_limit_source: { kind: "unknown" },
  } });
  await act(async () => emit(agentEvent("conversation-agent", run, 1, { kind: "run_created", mode: "execute" })));
  await act(async () => emit(agentEvent("conversation-agent", run, 2, request("attempt-1"))));
  await act(async () => preview({ run_id: "foreign-run", attempt_id: "attempt-1", text: "foreign run" }));
  await act(async () => preview({ run_id: run, attempt_id: "old-attempt", text: "foreign attempt" }));
  expect(screen.queryByRole("article", { name: "模型思考" })).not.toBeInTheDocument();
  await act(async () => preview({ run_id: run, attempt_id: "attempt-1", text: "第一段真实推理" }));
  expect(screen.getByRole("article", { name: "模型思考" })).toHaveTextContent("第一段真实推理");
  await act(async () => emit(agentEvent("conversation-agent", run, 3, { kind: "model_retrying", attempt: 2, class: "transient", message: "retry" })));
  expect(screen.queryByRole("article", { name: "模型思考" })).not.toBeInTheDocument();
  await act(async () => emit(agentEvent("conversation-agent", run, 4, request("attempt-2"))));
  await act(async () => preview({ run_id: run, attempt_id: "attempt-2", text: "第二次真实推理" }));
  await act(async () => preview({ run_id: run, attempt_id: "attempt-1", text: "late stale output" }));
  await act(async () => emit(agentEvent("conversation-agent", run, 3, { kind: "model_retrying", attempt: 2, class: "transient", message: "retry" })));
  expect(screen.getByRole("article", { name: "模型思考" })).toHaveTextContent("第二次真实推理");
  await act(async () => emit(agentEvent("conversation-agent", run, 5, { kind: "run_cancelled" })));
  await act(async () => preview({ run_id: run, attempt_id: "attempt-2", text: "late after cancel" }));
  expect(screen.queryByRole("article", { name: "模型思考" })).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Plan 会话" }));
  await screen.findByRole("heading", { name: "Plan 会话" });
  await act(async () => preview({ run_id: run, attempt_id: "attempt-2", text: "old conversation" }));
  expect(screen.queryByRole("article", { name: "模型思考" })).not.toBeInTheDocument();
});

it("accepts model activity after a same-run needs-attention resume", async () => {
  setupConversationStateHarness();
  let emit!: Parameters<typeof api.onAgentV4Event>[0];
  let activity!: Parameters<typeof api.onAgentV4ModelActivity>[0];
  vi.spyOn(api, "onAgentV4Event").mockImplementation(async (cb) => { emit = cb; return () => undefined; });
  vi.spyOn(api, "onAgentV4ModelActivity").mockImplementation(async (cb) => { activity = cb; return () => undefined; });
  render(<DesktopApp />);
  await screen.findByRole("heading", { name: "Agent 会话" });
  await waitFor(() => expect(activity).toBeDefined());
  const run = "resumed-run";
  const attempt = "resumed-attempt";
  await act(async () => emit(agentEvent("conversation-agent", run, 1, { kind: "run_created", mode: "execute" })));
  await act(async () => emit(agentEvent("conversation-agent", run, 2, { kind: "run_needs_attention", message: "resume required" })));
  await act(async () => emit(agentEvent("conversation-agent", run, 3, { kind: "model_request_started", request: {
    logical_request_id: "logical", attempt_id: attempt, model_profile_id: "profile", context_limit_source: { kind: "unknown" },
  } })));
  await act(async () => activity({ run_id: run, attempt_id: attempt, phase: "reasoning" }));
  expect(screen.getByRole("status", { name: "当前模型活动" })).toHaveTextContent("模型正在推理");
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
    expect(enqueue).toHaveBeenCalledWith(expect.objectContaining({ compute_selection: expect.objectContaining({ approval_policy: "auto_approve_except_local_deletion" }) }));
    expect(submit).not.toHaveBeenCalled(); expect(start).not.toHaveBeenCalled();
    await waitFor(() => expect(input).toHaveValue(""));
    await waitFor(() => expect(screen.getAllByText("原子提交的研究任务")).toHaveLength(1));
  } finally { view.unmount(); Reflect.deleteProperty(window, "__TAURI_INTERNALS__"); }
});

it("sends a new turn in the same conversation after the host settles an observed stop", async () => {
  const { stateSpy } = setupConversationStateHarness();
  const conversationId = stateConversations[0].id;
  const runId = "stopped-remote-dispatch";
  let stopped = false;
  let reconciled = false;
  const events: AgentRunEventV4[] = [
    agentEvent(conversationId, runId, 1, { kind: "run_created", mode: "execute" }),
    agentEvent(conversationId, runId, 2, { kind: "tool_requested", call: { call_id: "remote-cell", tool_id: "runtime.execute", arguments: { language: "python", code: "print('ready')" } } }),
  ];
  stateSpy.mockImplementation(async (_projectId, id) => stateSnapshot(id, {
    latest_run: { ...stateRun(runId, stopped ? "needs_attention" : "running", null, null, null), session_mode: "agent" },
    composer_blocking_run_ids: [],
  }));
  vi.mocked(api.agentV4EventsForConversation).mockImplementation(async () => [...events]);
  vi.mocked(api.agentV4Events).mockImplementation(async () => [...events]);
  for (const name of ["onConversationEvent", "onConversationUpdated", "onSyncEvent", "onKernelEvent", "onAgentV4Event", "onAgentV4TextPreview", "onAgentV4ReasoningPreview", "onAgentV4ModelActivity"] as const) vi.spyOn(api, name).mockResolvedValue(() => {});
  vi.spyOn(api, "agentV4GetStop").mockResolvedValue(null);
  vi.spyOn(api, "getConversationCapabilitiesV4").mockResolvedValue({ project_id: stateProject.id, conversation_id: conversationId, skills: [], mcp_servers: [], memory_count: 0 });
  vi.spyOn(preferencesApi, "getConversationAgentPreferencesV4").mockResolvedValue({ delegation_enabled: true, auto_review: true, memory_enabled: true });
  const computeSelection = { schema_version: 4 as const, backend_id: "local", backend_kind: "local" as const, autonomy_mode: "supervised" as const, approval_policy: "risk_based" as const, environment: "system", network_policy: "host_inherited" as const, container_image: null };
  const frozen = { model_profile_id: stateModel.id, model_configuration_hash: "hash", compute_selection: computeSelection, conversation_preferences: { delegation_enabled: true, auto_review: true, memory_enabled: true }, service_tier: {}, delegated_model: null, reviewer_model: null };
  vi.spyOn(queueApi, "reconcileComposerQueue").mockImplementation(async () => [{
    request_id: "old-request", message_id: "old-message", run_id: runId, project_id: stateProject.id, conversation_id: conversationId,
    mode: "agent", message_markdown: "Inspect the remote environment", references: [], attachments: [], attachment_receipts: [], frozen,
    position: 1, revision: reconciled ? 3 : stopped ? 2 : 1, status: reconciled ? "failed" : stopped ? "uncertain" : "running", created_at: "now", updated_at: "now",
  }]);
  const enqueue = vi.spyOn(queueApi, "enqueueComposerTurn").mockImplementation(async (request) => ({ ...request, frozen, attachment_receipts: [], position: 2, revision: 1, status: "pending", created_at: "now", updated_at: "now" }));
  const requestStop = vi.spyOn(api, "agentV4RequestStop").mockImplementation(async () => {
    stopped = true;
    events.push(
      agentEvent(conversationId, runId, 3, { kind: "tool_dispatch_uncertain", call_id: "remote-cell", tool_id: "runtime.execute" }),
      agentEvent(conversationId, runId, 4, { kind: "run_needs_attention", message: "Stopped with an unresolved remote dispatch" }),
    );
    return stopReceipt(stateProject.id, conversationId, runId);
  });
  const ActualWorkspaceShell = workspaceShell.WorkspaceShell;
  const shell = vi.spyOn(workspaceShell, "WorkspaceShell").mockImplementation((props) => <ActualWorkspaceShell {...props} />);
  Object.defineProperty(window, "__TAURI_INTERNALS__", { configurable: true, value: { invoke: vi.fn().mockResolvedValue([]) } });
  const view = render(<DesktopApp />);
  try {
    fireEvent.click(await screen.findByRole("button", { name: "停止当前运行" }));
    await waitFor(() => expect(requestStop).toHaveBeenCalledTimes(1));
    await waitFor(() => expect(shell.mock.calls.at(-1)![0]).toMatchObject({ activeRunId: null, runStarted: false, conversationLocked: false }));
    await act(async () => { await shell.mock.calls.at(-1)![0].onQueueRefresh!(); });
    expect(await screen.findByRole("button", { name: "待核实" })).toBeDisabled();
    expect(screen.queryByRole("button", { name: "停止当前运行" })).not.toBeInTheDocument();
    const input = screen.getByRole("textbox", { name: /描述研究目标/ });
    expect(input).toBeEnabled();
    fireEvent.change(input, { target: { value: "核实后继续下一项检查" } });
    fireEvent.keyDown(input, { key: "Enter" });
    expect(enqueue).not.toHaveBeenCalled();
    expect(input).toHaveValue("核实后继续下一项检查");
    for (const button of screen.getAllByRole("button", { name: "新建会话" })) expect(button).toBeEnabled();
    reconciled = true;
    await act(async () => { await shell.mock.calls.at(-1)![0].onQueueRefresh!(); });
    await waitFor(() => expect(screen.getByRole("button", { name: "发送" })).toBeEnabled());
    fireEvent.keyDown(input, { key: "Enter" });
    await waitFor(() => expect(enqueue).toHaveBeenCalledTimes(1));
    expect(enqueue).toHaveBeenCalledWith(expect.objectContaining({ conversation_id: conversationId, message_markdown: "核实后继续下一项检查" }));
    await waitFor(() => expect(input).toHaveValue(""));
    expect(events.at(-1)?.event.kind).toBe("run_needs_attention");
  } finally { view.unmount(); Reflect.deleteProperty(window, "__TAURI_INTERNALS__"); }
});

it("restores Send when a stale uncertain queue receipt settles for an already-resolved historical run", async () => {
  const { stateSpy } = setupConversationStateHarness();
  const conversationId = stateConversations[0].id;
  const runId = "resolved-historical-dispatch";
  let reconciled = false;
  const events = [
    agentEvent(conversationId, runId, 1, { kind: "run_created", mode: "execute" }),
    agentEvent(conversationId, runId, 2, { kind: "tool_dispatch_uncertain", call_id: "remote-cell", tool_id: "runtime.execute" }),
    agentEvent(conversationId, runId, 3, { kind: "tool_dispatch_resolved", call_id: "remote-cell", resolution: "side_effect_not_observed", evidence: "The host verified that the cell produced no side effect before the run ended." }),
    agentEvent(conversationId, runId, 4, { kind: "run_needs_attention", message: "The run ended after dispatch verification." }),
  ];
  stateSpy.mockImplementation(async (_projectId, id) => stateSnapshot(id, {
    latest_run: { ...stateRun(runId, "needs_attention", null, null, null), session_mode: "agent" },
  }));
  vi.mocked(api.agentV4EventsForConversation).mockResolvedValue(events);
  vi.mocked(api.agentV4Events).mockResolvedValue(events);
  for (const name of ["onConversationEvent", "onConversationUpdated", "onSyncEvent", "onKernelEvent", "onAgentV4Event", "onAgentV4TextPreview", "onAgentV4ReasoningPreview", "onAgentV4ModelActivity"] as const) vi.spyOn(api, name).mockResolvedValue(() => {});
  vi.spyOn(api, "getConversationCapabilitiesV4").mockResolvedValue({ project_id: stateProject.id, conversation_id: conversationId, skills: [], mcp_servers: [], memory_count: 0 });
  vi.spyOn(preferencesApi, "getConversationAgentPreferencesV4").mockResolvedValue({ delegation_enabled: true, auto_review: true, memory_enabled: true });
  const computeSelection = { schema_version: 4 as const, backend_id: "local", backend_kind: "local" as const, autonomy_mode: "supervised" as const, approval_policy: "risk_based" as const, environment: "system", network_policy: "host_inherited" as const, container_image: null };
  const frozen = { model_profile_id: stateModel.id, model_configuration_hash: "hash", compute_selection: computeSelection, conversation_preferences: { delegation_enabled: true, auto_review: true, memory_enabled: true }, service_tier: {}, delegated_model: null, reviewer_model: null };
  vi.spyOn(queueApi, "reconcileComposerQueue").mockImplementation(async () => [{
    request_id: "old-request", message_id: "old-message", run_id: runId, project_id: stateProject.id, conversation_id: conversationId,
    mode: "agent", message_markdown: "Inspect the environment", references: [], attachments: [], attachment_receipts: [], frozen,
    position: 1, revision: reconciled ? 2 : 1, status: reconciled ? "failed" : "uncertain", created_at: "now", updated_at: "now",
  }]);
  const enqueue = vi.spyOn(queueApi, "enqueueComposerTurn").mockImplementation(async (request) => ({ ...request, frozen, attachment_receipts: [], position: 2, revision: 1, status: "pending", created_at: "now", updated_at: "now" }));
  const ActualWorkspaceShell = workspaceShell.WorkspaceShell;
  const shell = vi.spyOn(workspaceShell, "WorkspaceShell").mockImplementation((props) => <ActualWorkspaceShell {...props} />);
  Object.defineProperty(window, "__TAURI_INTERNALS__", { configurable: true, value: { invoke: vi.fn().mockResolvedValue([]) } });
  const view = render(<DesktopApp />);
  try {
    expect(await screen.findByRole("button", { name: "待核实" })).toBeDisabled();
    await waitFor(() => expect(shell.mock.calls.at(-1)![0]).toMatchObject({ activeRunId: null, runStarted: false }));
    const input = screen.getByRole("textbox", { name: /描述研究目标/ });
    await waitFor(() => expect(input).toBeEnabled());
    fireEvent.change(input, { target: { value: "核实后继续下一项检查" } });
    reconciled = true;
    await act(async () => { await shell.mock.calls.at(-1)![0].onQueueRefresh!(); });
    await waitFor(() => expect(screen.getByRole("button", { name: "发送" })).toBeEnabled());
    expect(screen.queryByRole("button", { name: "待核实" })).not.toBeInTheDocument();
    expect(input).toHaveValue("核实后继续下一项检查");
    fireEvent.keyDown(input, { key: "Enter" });
    await waitFor(() => expect(enqueue).toHaveBeenCalledTimes(1));
    expect(enqueue).toHaveBeenCalledWith(expect.objectContaining({ conversation_id: conversationId, message_markdown: "核实后继续下一项检查" }));
    await waitFor(() => expect(input).toHaveValue(""));
  } finally { view.unmount(); Reflect.deleteProperty(window, "__TAURI_INTERNALS__"); }
});

describe("host composer dispatch state", () => {
  function setup(blockingRunIds: string[]) {
    const { stateSpy } = setupConversationStateHarness();
    const conversationId = stateConversations[0].id;
    const runId = "old-direct-run";
    const events = [
      agentEvent(conversationId, runId, 1, { kind: "tool_dispatch_uncertain", call_id: "old-call", tool_id: "runtime.execute" }),
      agentEvent(conversationId, runId, 2, { kind: "run_needs_attention", message: "An old stopped dispatch remains in history." }),
    ];
    const snapshot = (ids: string[]) => stateSnapshot(conversationId, { composer_blocking_run_ids: ids, latest_run: { ...stateRun(runId, "needs_attention", null, null, null), session_mode: "agent" } });
    stateSpy.mockResolvedValue(snapshot(blockingRunIds));
    vi.mocked(api.agentV4EventsForConversation).mockResolvedValue(events);
    vi.mocked(api.agentV4Events).mockResolvedValue(events);
    for (const name of ["onConversationEvent", "onConversationUpdated", "onSyncEvent", "onKernelEvent", "onAgentV4TextPreview", "onAgentV4ReasoningPreview", "onAgentV4ModelActivity"] as const) vi.spyOn(api, name).mockResolvedValue(() => {});
    let emit!: Parameters<typeof api.onAgentV4Event>[0];
    vi.spyOn(api, "onAgentV4Event").mockImplementation(async (listener) => { emit = listener; return () => {}; });
    vi.spyOn(api, "getConversationCapabilitiesV4").mockResolvedValue({ project_id: stateProject.id, conversation_id: conversationId, skills: [], mcp_servers: [], memory_count: 0 });
    vi.spyOn(preferencesApi, "getConversationAgentPreferencesV4").mockResolvedValue({ delegation_enabled: true, auto_review: true, memory_enabled: true });
    vi.spyOn(queueApi, "reconcileComposerQueue").mockResolvedValue([]);
    const enqueue = vi.spyOn(queueApi, "enqueueComposerTurn").mockImplementation(async (request) => ({ ...request,
      frozen: { model_profile_id: request.model_profile_id, model_configuration_hash: "hash", compute_selection: request.compute_selection, conversation_preferences: { delegation_enabled: true, auto_review: true, memory_enabled: true }, service_tier: {}, delegated_model: null, reviewer_model: null },
      attachment_receipts: [], position: 1, revision: 1, status: "pending", created_at: "now", updated_at: "now",
    }));
    const ActualWorkspaceShell = workspaceShell.WorkspaceShell;
    const shell = vi.spyOn(workspaceShell, "WorkspaceShell").mockImplementation((props) => <ActualWorkspaceShell {...props} />);
    Object.defineProperty(window, "__TAURI_INTERNALS__", { configurable: true, value: { invoke: vi.fn().mockResolvedValue([]) } });
    const view = render(<DesktopApp />);
    return { stateSpy, conversationId, runId, snapshot, shell, enqueue, emit: (event: AgentRunEventV4) => emit(event), close: () => { view.unmount(); Reflect.deleteProperty(window, "__TAURI_INTERNALS__"); } };
  }

  it("restores ordinary sending on reload for a stopped direct run with no queue row", async () => {
    const test = setup([]);
    try {
      const input = await screen.findByRole("textbox", { name: /描述研究目标/ });
      await waitFor(() => expect(input).toBeEnabled());
      fireEvent.change(input, { target: { value: "新的环境检查" } });
      await waitFor(() => expect(screen.getByRole("button", { name: "发送" })).toBeEnabled());
      fireEvent.keyDown(input, { key: "Enter" });
      await waitFor(() => expect(test.enqueue).toHaveBeenCalledTimes(1));
      expect(test.enqueue).toHaveBeenCalledWith(expect.objectContaining({ conversation_id: test.conversationId, message_markdown: "新的环境检查" }));
      expect(screen.getByText("An old stopped dispatch remains in history.")).toBeInTheDocument();
    } finally { test.close(); }
  });

  it("refreshes host clearance for an empty queue and retains the blocked draft after a failed state read", async () => {
    const test = setup(["old-direct-run"]);
    try {
      expect(await screen.findByRole("button", { name: "待核实" })).toBeDisabled();
      const input = screen.getByRole("textbox", { name: /描述研究目标/ });
      await waitFor(() => expect(input).toBeEnabled());
      fireEvent.change(input, { target: { value: "刷新后发送" } });
      test.stateSpy.mockRejectedValueOnce(new Error("state read unavailable"));
      fireEvent.click(screen.getByRole("button", { name: "刷新发送状态" }));
      await screen.findByRole("alert", { name: "发送状态刷新失败" });
      expect(screen.getByRole("button", { name: "待核实" })).toBeDisabled();
      expect(input).toHaveValue("刷新后发送");
      test.stateSpy.mockResolvedValue(test.snapshot([]));
      fireEvent.click(screen.getByRole("button", { name: "刷新发送状态" }));
      await waitFor(() => expect(screen.getByRole("button", { name: "发送" })).toBeEnabled());
      expect(screen.queryByRole("alert", { name: "发送状态刷新失败" })).not.toBeInTheDocument();
      fireEvent.keyDown(input, { key: "Enter" });
      await waitFor(() => expect(test.enqueue).toHaveBeenCalledTimes(1));
    } finally { test.close(); }
  });

  it("invalidates old host clearance on a new unresolved terminal event and rejects a delayed older snapshot", async () => {
    const test = setup([]);
    try {
      const input = await screen.findByRole("textbox", { name: /描述研究目标/ });
      await waitFor(() => expect(input).toBeEnabled());
      fireEvent.change(input, { target: { value: "等待新的宿主状态" } });
      await waitFor(() => expect(screen.getByRole("button", { name: "发送" })).toBeEnabled());
      const oldState = deferred<ConversationAgentStateV4>();
      const newState = deferred<ConversationAgentStateV4>();
      test.stateSpy.mockReturnValueOnce(oldState.promise).mockReturnValueOnce(newState.promise);
      const readsBefore = test.stateSpy.mock.calls.length;
      let refresh!: Promise<void>;
      await act(async () => { refresh = test.shell.mock.calls.at(-1)![0].onQueueRefresh!(); });
      await waitFor(() => expect(test.stateSpy).toHaveBeenCalledTimes(readsBefore + 1));
      await act(async () => {
        test.emit(agentEvent(test.conversationId, "new-unresolved-run", 1, { kind: "tool_dispatch_uncertain", call_id: "new-call", tool_id: "runtime.execute" }));
        test.emit(agentEvent(test.conversationId, "new-unresolved-run", 2, { kind: "run_needs_attention", message: "A new dispatch requires review." }));
      });
      expect(screen.getByRole("button", { name: "待核实" })).toBeDisabled();
      await act(async () => newState.resolve(test.snapshot(["new-unresolved-run"])));
      await act(async () => { oldState.resolve(test.snapshot([])); await refresh; });
      expect(screen.getByRole("button", { name: "待核实" })).toBeDisabled();
      fireEvent.keyDown(input, { key: "Enter" });
      expect(test.enqueue).not.toHaveBeenCalled();
      expect(input).toHaveValue("等待新的宿主状态");
    } finally { test.close(); }
  });

  it("retries the same settled queue snapshot after a live event supersedes its host clearance read", async () => {
    const intervals = vi.spyOn(window, "setInterval");
    const test = setup(["old-direct-run"]);
    try {
      expect(await screen.findByRole("button", { name: "待核实" })).toBeDisabled();
      await waitFor(() => expect(queueApi.reconcileComposerQueue).toHaveBeenCalled());
      const poll = intervals.mock.calls.find(([, delay]) => delay === 3000)![0] as () => void;
      const selection = { schema_version: 4 as const, backend_id: "local", backend_kind: "local" as const, autonomy_mode: "supervised" as const, approval_policy: "risk_based" as const, environment: "system", network_policy: "host_inherited" as const, container_image: null };
      vi.mocked(queueApi.reconcileComposerQueue).mockResolvedValue([{
        request_id: "settled-stop", message_id: "old-message", run_id: test.runId, project_id: stateProject.id, conversation_id: test.conversationId,
        mode: "agent", message_markdown: "Old request", references: [], attachments: [], attachment_receipts: [],
        frozen: { model_profile_id: stateModel.id, model_configuration_hash: "hash", compute_selection: selection, conversation_preferences: { delegation_enabled: true, auto_review: true, memory_enabled: true }, service_tier: {}, delegated_model: null, reviewer_model: null },
        position: 1, revision: 3, status: "failed", created_at: "now", updated_at: "now",
      }]);
      const delayed = deferred<ConversationAgentStateV4>();
      const clear = stateSnapshot(test.conversationId, { composer_blocking_run_ids: [], latest_run: { ...stateRun("next-running-turn", "running", null, null, null), session_mode: "agent" } });
      test.stateSpy.mockReturnValueOnce(delayed.promise).mockResolvedValue(clear);
      const readsBefore = test.stateSpy.mock.calls.length;
      await act(async () => poll());
      await waitFor(() => expect(test.stateSpy).toHaveBeenCalledTimes(readsBefore + 1));
      await act(async () => test.emit(agentEvent(test.conversationId, "next-running-turn", 2, { kind: "model_text", text: "A newly dispatched turn is making progress." })));
      await act(async () => delayed.resolve(clear));
      expect(test.shell.mock.calls.at(-1)![0].composerBlockingRunIds).toEqual([test.runId]);
      await act(async () => poll());
      await waitFor(() => expect(test.stateSpy).toHaveBeenCalledTimes(readsBefore + 2));
      await waitFor(() => expect(test.shell.mock.calls.at(-1)![0].composerBlockingRunIds).toEqual([]));
      expect(screen.queryByRole("button", { name: "待核实" })).not.toBeInTheDocument();
      const input = screen.getByRole("textbox", { name: /描述研究目标/ });
      fireEvent.change(input, { target: { value: "新的请求" } });
      await waitFor(() => expect(screen.getByRole("button", { name: "加入队列" })).toBeEnabled());
    } finally { test.close(); }
  });
});

it("retains pending compaction through Back to projects and reopening the same paused run", async () => {
  const { stateSpy } = setupConversationStateHarness();
  const runId = "compaction-library-remount";
  const question = agentEvent(stateConversations[0].id, runId, 1, { kind: "input_requested", question_id: "sample", question: "Choose the sample before continuing" });
  stateSpy.mockImplementation(async (_projectId, conversationId) => stateSnapshot(conversationId, conversationId === stateConversations[0].id ? {
    locked: true, latest_run: { ...stateRun(runId, "waiting_for_input", null, null, null), session_mode: "agent" },
  } : {}));
  vi.mocked(api.agentV4EventsForConversation).mockImplementation(async (_projectId, conversationId) => conversationId === stateConversations[0].id ? [question] : []);
  vi.mocked(api.agentV4Events).mockResolvedValue([question]);
  const pending = deferred<ContextCompactionReceiptV4>();
  const compact = vi.spyOn(contextApi, "compactContextV4").mockReturnValue(pending.promise);
  const answer = vi.spyOn(api, "agentV4Answer").mockResolvedValue();
  const resume = vi.spyOn(api, "agentV4Resume").mockResolvedValue();
  render(<DesktopApp />);
  await screen.findByText("Choose the sample before continuing");
  fireEvent.click(screen.getByRole("button", { name: "查看上下文用量" }));
  const start = screen.getByRole("button", { name: "压缩上下文" });
  await waitFor(() => expect(start).toBeEnabled()); fireEvent.click(start);
  expect(compact).toHaveBeenCalledTimes(1);
  const originalRequest = compact.mock.calls[0][0];
  fireEvent.click(screen.getByRole("button", { name: "返回项目主页" }));
  expect(screen.queryByRole("dialog", { name: "上下文用量" })).toBeNull();
  fireEvent.click(await screen.findByRole("button", { name: `打开项目：${stateProject.name}` }));
  await screen.findByRole("heading", { name: stateConversations[0].title });
  fireEvent.click(screen.getByRole("button", { name: "查看上下文用量" }));
  const returned = screen.getByRole("button", { name: "压缩中…" });
  expect(returned).toBeDisabled(); fireEvent.click(returned); expect(compact).toHaveBeenCalledTimes(1);
  await act(async () => pending.resolve({ ...originalRequest, status: "completed", source_through_sequence: 1, source_head_hash: "hash", before_bytes: 9000, after_bytes: 1000, created_at: "now", updated_at: "now" }));
  expect(screen.queryByText(/上下文已压缩/)).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "压缩上下文" }));
  expect(await screen.findByText(/上下文已压缩/)).toBeVisible();
  expect(compact).toHaveBeenCalledTimes(1);
  expect(screen.getByText("Choose the sample before continuing")).toBeVisible();
  expect(answer).not.toHaveBeenCalled(); expect(resume).not.toHaveBeenCalled();
});
