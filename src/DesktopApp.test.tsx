import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import DesktopApp from "./DesktopApp";
import * as api from "./tauri-api";

beforeEach(() => vi.restoreAllMocks());

describe("DesktopApp", () => {
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
    vi.spyOn(api, "agentV4Events").mockResolvedValue([]);

    render(<DesktopApp />);
    const composer = await screen.findByRole("textbox", { name: /描述研究目标/ });
    await screen.findByText("Agent 模式：发送后直接执行任务");
    fireEvent.change(composer, { target: { value: "先解释一下这个矩阵格式" } });
    fireEvent.click(screen.getByRole("button", { name: "发送" }));

    await waitFor(() => expect(startDirect).toHaveBeenCalledWith(expect.objectContaining({ objective: "先解释一下这个矩阵格式" })));
    expect(startPlanning).not.toHaveBeenCalled();
    expect(await screen.findByRole("button", { name: "添加上下文或选择模式" })).toBeInTheDocument();
  });

  it("starts planning from the plus-menu Plan mode and continues execution after approval", async () => {
    const project = { id: "project-1", name: "PBMC 图谱", description: "", local_root: "E:/Science/pbmc", remote_root: null, connection_id: null, template: "single_cell_rna_seq" as const, status: "running" as const, ollama_only: false, created_at: "2026-08-11T00:00:00Z", updated_at: "2026-08-11T00:00:00Z" };
    const conversation = { id: "conversation-1", project_id: project.id, title: "分析任务", status: "idle" as const, model_profile_id: "model-1", created_at: "2026-08-11T00:00:00Z", updated_at: "2026-08-11T00:00:00Z" };
    const model = { id: "model-1", label: "Test model", provider: "ollama" as const, base_url: "http://localhost:11434", model: "test", credential_reference: null, supports_tools: true, supports_vision: false };
    const selection = { schema_version: 4 as const, backend_id: "local", backend_kind: "local" as const, autonomy_mode: "supervised" as const, environment: "system", network_policy: "host_inherited" as const, container_image: null };
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

    render(<DesktopApp />);
    await screen.findByRole("main", { name: "科研对话" });
    fireEvent.click(screen.getByRole("button", { name: "添加上下文或选择模式" }));
    fireEvent.click(screen.getByRole("menuitem", { name: /Plan 模式/ }));
    await screen.findByRole("region", { name: "V4 计算后端" });
    await screen.findByRole("radio", { name: /LOCAL/ });
    fireEvent.change(screen.getByRole("textbox", { name: /描述研究目标/ }), { target: { value: "执行完整 QC" } });
    fireEvent.click(screen.getByRole("button", { name: "发送" }));

    await waitFor(() => expect(startPlanning).toHaveBeenCalledWith(expect.objectContaining({ objective: "执行完整 QC", compute_selection: selection })));
    fireEvent.click(await screen.findByRole("button", { name: "批准并运行" }));
    await waitFor(() => expect(approvePlan).toHaveBeenCalledWith("run-v4", "approval-hash"));
    expect(await screen.findByText("运行中")).toBeInTheDocument();
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

  it("centralizes model, remote compute, privacy, and legacy history settings", async () => {
    vi.spyOn(api, "listProjects").mockResolvedValue([]);
    render(<DesktopApp />);
    fireEvent.click(await screen.findByRole("button", { name: "设置" }));

    const dialog = screen.getByRole("dialog", { name: "工作台设置" });
    expect(dialog).toHaveTextContent("Anthropic");
    expect(dialog).toHaveTextContent("OpenAI-compatible");
    expect(dialog).toHaveTextContent("Ollama");
    expect(dialog).toHaveTextContent("历史运行只读");
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
    fireEvent.click(await screen.findByRole("button", { name: "新建会话" }));
    await waitFor(() => expect(createConversation).toHaveBeenCalledWith(project.id));
    expect(await screen.findByRole("heading", { name: "新会话" })).toBeInTheDocument();
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
    fireEvent.click(await screen.findByRole("button", { name: "删除会话：当前问题" }));
    await waitFor(() => expect(deleteConversation).toHaveBeenCalledWith(project.id, active.id));
    expect(await screen.findByRole("heading", { name: "保留的问题" })).toBeInTheDocument();
    confirm.mockRestore();
  });
});
