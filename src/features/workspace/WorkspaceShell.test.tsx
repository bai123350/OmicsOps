import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { WorkspaceShell } from "./WorkspaceShell";

const project = {
  id: "project-1",
  name: "PBMC 图谱",
  status: "running" as const,
  template: "single_cell_rna_seq" as const,
};

describe("WorkspaceShell", () => {
  it("renders the V4 hash-chained trajectory separately from legacy runs", () => {
    render(<WorkspaceShell project={project} locale="zh-CN" onLocaleChange={() => undefined} runStarted activeRunId="run-v4" agentRunEventsV4={[{
      schema_version: 4, run_id: "run-v4", project_id: project.id, conversation_id: "conversation-1", sequence: 1,
      occurred_at: "2026-08-17T00:00:00Z", previous_hash: "", event_hash: "a".repeat(64), event: { kind: "run_created", mode: "plan" },
    }]} />);
    expect(screen.getByText("Agent Runtime V4")).toBeInTheDocument();
    expect(screen.getByText(/Plan\/Execute 硬隔离/)).toBeInTheDocument();
    expect(screen.getByText("规划启动")).toBeInTheDocument();
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
    expect(screen.getByText(/已完成 · 3 条哈希事件/)).toBeInTheDocument();
    fireEvent.click(screen.getByText("Agent Runtime V4"));
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

    fireEvent.click(screen.getByText("Agent Runtime V4"));
    fireEvent.click(screen.getByRole("button", { name: "继续运行" }));
    expect(onResume).toHaveBeenCalledWith("run-recover");

    rerender(<WorkspaceShell project={project} locale="zh-CN" onLocaleChange={() => undefined} runStarted activeRunId="run-recover" agentRunEventsV4={[...failed,
      { ...base, sequence: 3, occurred_at: "2026-08-17T00:00:03Z", event: { kind: "tool_dispatch_resolved" as const, call_id: "call-1", resolution: "side_effect_not_observed" as const, evidence: "legacy immutable system ensure" } },
    ]} onResumeAgentRunV4={onResume} />);
    expect(screen.getByText(/运行中 · 3 条哈希事件/)).toBeInTheDocument();
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
    expect(screen.getByRole("region", { name: "V4 计算后端" })).toHaveTextContent("探测不会拉取镜像或启动容器");
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
  it("keeps ordinary questions in chat until the user explicitly selects Plan mode from the plus menu", async () => {
    const onSend = vi.fn().mockResolvedValue(true);
    render(<WorkspaceShell project={project} locale="zh-CN" onLocaleChange={() => undefined} onSend={onSend}
      computeBackendId="local" computeBackends={[{ descriptor: { schema_version: 4, backend_id: "local", kind: "local", isolation: "process", available: true, supports_python: true, supports_r: false, supports_network_policy: false }, selectable: true, reason: null, python_status: "available", r_status: "unavailable", resolved_image_id: null }]} />);

    fireEvent.change(screen.getByRole("textbox", { name: /描述研究目标/ }), { target: { value: "这个文件是什么格式？" } });
    fireEvent.click(screen.getByRole("button", { name: "发送" }));
    await waitFor(() => expect(onSend).toHaveBeenCalledWith("这个文件是什么格式？", "chat"));
    expect(screen.queryByRole("region", { name: "V4 计算后端" })).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "添加上下文或选择模式" }));
    fireEvent.click(screen.getByRole("menuitem", { name: /Plan 模式/ }));
    expect(screen.getByRole("region", { name: "V4 计算后端" })).toBeInTheDocument();
    fireEvent.change(screen.getByRole("textbox", { name: /描述研究目标/ }), { target: { value: "执行完整 QC" } });
    fireEvent.click(screen.getByRole("button", { name: "发送" }));
    await waitFor(() => expect(onSend).toHaveBeenLastCalledWith("执行完整 QC", "plan"));
  });
  it("keeps projects, scientific conversation, and context visible together", () => {
    render(<WorkspaceShell project={project} locale="zh-CN" onLocaleChange={() => undefined} />);

    expect(screen.getByRole("navigation", { name: "项目与会话" })).toBeInTheDocument();
    expect(screen.getByRole("main", { name: "科研对话" })).toBeInTheDocument();
    expect(screen.getByRole("complementary", { name: "项目上下文" })).toBeInTheDocument();
    expect(screen.getAllByText("PBMC 图谱")).toHaveLength(2);
    expect(screen.getByText("Scanpy 质量控制")).toBeInTheDocument();
  });

  it("switches context tabs and expands artifact preview", () => {
    render(<WorkspaceShell project={project} locale="zh-CN" onLocaleChange={() => undefined} />);

    fireEvent.click(screen.getByRole("tab", { name: "预览" }));
    expect(screen.getAllByText("UMAP 聚类概览")).toHaveLength(2);
    fireEvent.click(screen.getByRole("button", { name: "展开预览" }));
    expect(screen.getByRole("dialog", { name: "产物预览" })).toBeInTheDocument();
  });

  it("loads the selected remote image into the preview", async () => {
    const onPreviewImage = vi.fn().mockResolvedValue({ relative_path: "results/umap.png", mime_type: "image/png", size_bytes: 1024, sha256: "a".repeat(64), data_url: "data:image/png;base64,iVBORw0KGgo=" });
    render(<WorkspaceShell project={project} locale="en-US" onLocaleChange={() => undefined} remoteFiles={[{ relative_path: "results/umap.png", directory: false, size_bytes: 1024, modified_unix_seconds: 0 }, { relative_path: "results/markers.csv", directory: false, size_bytes: 20, modified_unix_seconds: 0 }]} onPreviewImage={onPreviewImage} />);

    fireEvent.click(screen.getByRole("tab", { name: "Preview" }));
    expect(screen.getByRole("combobox", { name: "Select project image" })).toHaveValue("results/umap.png");
    fireEvent.click(screen.getByRole("button", { name: "Show image" }));
    expect(await screen.findByRole("img", { name: "results/umap.png" })).toHaveAttribute("src", "data:image/png;base64,iVBORw0KGgo=");
    expect(onPreviewImage).toHaveBeenCalledWith("results/umap.png");
    expect(screen.queryByRole("option", { name: "results/markers.csv" })).not.toBeInTheDocument();
  });

  it("renders English copy from the shared locale resource", () => {
    render(<WorkspaceShell project={project} locale="en-US" onLocaleChange={() => undefined} />);

    expect(screen.getByRole("main", { name: "Research conversation" })).toBeInTheDocument();
    expect(screen.getByRole("tab", { name: "Lab notebook" })).toBeInTheDocument();
  });

  it("sends a demo research message and exposes the dedicated Plan tab", () => {
    render(<WorkspaceShell project={project} locale="zh-CN" onLocaleChange={() => undefined} />);
    fireEvent.change(screen.getByRole("textbox", { name: /描述研究目标/ }), { target: { value: "先检查双细胞率" } });
    fireEvent.click(screen.getByRole("button", { name: "发送" }));
    expect(screen.getByText("先检查双细胞率")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("tab", { name: "Plan" }));
    expect(screen.getByText("尚未进入 Plan 模式")).toBeInTheDocument();
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

  it("shows streamed agent decisions, SSH commands, and remote output", () => {
    render(<WorkspaceShell project={project} locale="zh-CN" onLocaleChange={() => undefined} runStarted agentRunEvents={[
      { run_id: "run-1", project_id: "project-1", sequence: 1, timestamp: "2026-08-12T08:00:00Z", kind: "model_action", title: "Agent action selected", content: "先核对矩阵维度", iteration: 1 },
      { run_id: "run-1", project_id: "project-1", sequence: 2, timestamp: "2026-08-12T08:00:01Z", kind: "tool_started", title: "SSH command", content: "python scripts/inspect.py", iteration: 1 },
      { run_id: "run-1", project_id: "project-1", sequence: 3, timestamp: "2026-08-12T08:00:02Z", kind: "stdout", title: "stdout", content: "genes=32738 cells=12000", iteration: 1 },
    ]} />);

    expect(screen.getAllByRole("article", { name: "远程 Agent 工作消息" })[0]).toHaveTextContent("先核对矩阵维度");
    expect(screen.getByText("python scripts/inspect.py")).toBeInTheDocument();
    expect(screen.getByText("genes=32738 cells=12000")).toBeInTheDocument();
  });

  it("preserves every streamed evaluation and waiting event inside the fold", () => {
    render(<WorkspaceShell project={project} locale="en-US" onLocaleChange={() => undefined} runStarted agentRunEvents={[
      { run_id: "run-1", project_id: "project-1", sequence: 1, timestamp: "2026-08-12T08:00:00Z", kind: "model_waiting", title: "Evaluating remote evidence", content: "10s elapsed", iteration: 1 },
      { run_id: "run-1", project_id: "project-1", sequence: 2, timestamp: "2026-08-12T08:00:10Z", kind: "model_waiting", title: "Evaluating remote evidence", content: "20s elapsed", iteration: 1 },
      { run_id: "run-1", project_id: "project-1", sequence: 3, timestamp: "2026-08-12T08:00:11Z", kind: "model_progress", title: "Agent evaluation", content: "Verified the matrix and barcode files exist.", iteration: 1 },
      { run_id: "run-1", project_id: "project-1", sequence: 4, timestamp: "2026-08-12T08:00:12Z", kind: "model_progress", title: "Agent evaluation", content: "Next I will inspect their headers without modifying data.", iteration: 1 },
    ]} />);

    expect(screen.getByText("10s elapsed")).toBeInTheDocument();
    expect(screen.getByText("20s elapsed")).toBeInTheDocument();
    expect(screen.getByText("Verified the matrix and barcode files exist.")).toBeInTheDocument();
    expect(screen.getByText("Next I will inspect their headers without modifying data.")).toBeInTheDocument();
    expect(screen.getAllByText("Agent stream")).toHaveLength(4);
  });

  it("retains completed runs as collapsed history while keeping the active run open", () => {
    render(<WorkspaceShell project={project} locale="en-US" onLocaleChange={() => undefined} runStarted activeRunId="run-2" agentRunEvents={[
      { run_id: "run-1", project_id: "project-1", conversation_id: "conversation-1", sequence: 1, timestamp: "2026-08-12T08:00:00Z", kind: "stdout", title: "stdout", content: "historical output", iteration: 1 },
      { run_id: "run-1", project_id: "project-1", conversation_id: "conversation-1", sequence: 2, timestamp: "2026-08-12T08:00:05Z", kind: "agent_completed", title: "Completed", content: "Done", iteration: null },
      { run_id: "run-2", project_id: "project-1", conversation_id: "conversation-1", sequence: 1, timestamp: "2026-08-12T08:01:00Z", kind: "model_waiting", title: "Evaluating", content: "Working", iteration: 1 },
    ]} />);

    const historicalRun = screen.getByText("Processed 5s").closest("details");
    const activeRun = screen.getByText("Processing 0s").closest("details");
    expect(historicalRun).not.toHaveAttribute("open");
    expect(activeRun).toHaveAttribute("open");
    expect(screen.getByText("historical output")).toBeInTheDocument();
    expect(screen.getByText("Completed · 2 stream events")).toBeInTheDocument();
    expect(screen.getAllByText("Agent and server interaction")).toHaveLength(2);
  });

  it("places each persisted run directly after the conversation message that triggered it", () => {
    render(<WorkspaceShell project={project} locale="en-US" onLocaleChange={() => undefined} messages={[
      { id: "message-1", role: "assistant", markdown: "First analysis proposal", created_at: "2026-08-12T08:00:00Z" },
      { id: "message-2", role: "user", markdown: "Please make the second plot", created_at: "2026-08-12T08:02:00Z" },
      { id: "message-3", role: "assistant", markdown: "Second analysis finished", created_at: "2026-08-12T08:04:00Z" },
    ]} agentRunEvents={[
      { run_id: "run-1", project_id: "project-1", sequence: 1, timestamp: "2026-08-12T08:01:00Z", kind: "stdout", title: "stdout", content: "first run", iteration: 1 },
      { run_id: "run-1", project_id: "project-1", sequence: 2, timestamp: "2026-08-12T08:01:05Z", kind: "agent_completed", title: "Completed", content: "Done", iteration: null },
      { run_id: "run-2", project_id: "project-1", sequence: 1, timestamp: "2026-08-12T08:03:00Z", kind: "stdout", title: "stdout", content: "second run", iteration: 1 },
      { run_id: "run-2", project_id: "project-1", sequence: 2, timestamp: "2026-08-12T08:03:07Z", kind: "agent_completed", title: "Completed", content: "Done", iteration: null },
    ]} />);

    const firstMessage = screen.getByText("First analysis proposal");
    const firstRun = screen.getByText("Processed 5s");
    const secondMessage = screen.getByText("Please make the second plot");
    const secondRun = screen.getByText("Processed 7s");
    const finalMessage = screen.getByText("Second analysis finished");
    expect(firstMessage.compareDocumentPosition(firstRun) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    expect(firstRun.compareDocumentPosition(secondMessage) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    expect(secondMessage.compareDocumentPosition(secondRun) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    expect(secondRun.compareDocumentPosition(finalMessage) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  });

  it("keeps the active stream at the current conversation bottom instead of anchoring it to an older message", () => {
    render(<WorkspaceShell project={project} locale="en-US" onLocaleChange={() => undefined} runStarted activeRunId="run-live" messages={[
      { id: "message-1", role: "assistant", markdown: "Plan approved", created_at: "2026-08-12T08:00:00Z" },
      { id: "message-2", role: "user", markdown: "Use the Seurat R package", created_at: "2026-08-12T09:00:00Z" },
    ]} agentRunEvents={[
      { run_id: "run-live", project_id: "project-1", sequence: 1, timestamp: "2026-08-12T08:01:00Z", kind: "tool_started", title: "SSH command", content: "micromamba install r-seurat", iteration: 1 },
    ]} />);

    const latestMessage = screen.getByText("Use the Seurat R package");
    const activeFold = screen.getByText("Processing 0s").closest("details");
    expect(activeFold).toHaveAttribute("open");
    expect(latestMessage.compareDocumentPosition(activeFold!) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    expect(screen.getByText("micromamba install r-seurat")).toBeInTheDocument();
  });

  it("anchors a completed Harness v3 run before a later conversation turn", () => {
    render(<WorkspaceShell project={project} locale="zh-CN" onLocaleChange={() => undefined} agentBusy messages={[
      { id: "message-1", role: "assistant", markdown: "首轮分析计划", created_at: "2026-08-12T08:00:00Z" },
      { id: "message-2", role: "user", markdown: "用 Seurat R 包跑", created_at: "2026-08-12T09:00:00Z" },
    ]} agentRunEventsV3={[
      { run_id: "run-old", project_id: "project-1", conversation_id: "conversation-1", sequence: 1, previous_hash: "0", event_hash: "a", occurred_at: "2026-08-12T08:01:00Z", event: { kind: "run_started" } },
      { run_id: "run-old", project_id: "project-1", conversation_id: "conversation-1", sequence: 2, previous_hash: "a", event_hash: "b", occurred_at: "2026-08-12T08:02:00Z", event: { kind: "run_failed", payload: { message: "old run failed" } } },
    ]} />);

    const oldRun = screen.getByText("失败 · 2 条事件").closest("details")!;
    const newTurn = screen.getByText("用 Seurat R 包跑");
    const pending = screen.getByRole("status");
    expect(oldRun.compareDocumentPosition(newTurn) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    expect(newTurn.compareDocumentPosition(pending) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  });

  it("keeps the streamed interaction inline and expands it from the processed row", () => {
    render(<WorkspaceShell project={project} locale="en-US" onLocaleChange={() => undefined} messages={[
      { id: "message-1", role: "assistant", markdown: "Approved plan", created_at: "2026-08-12T08:00:00Z" },
      { id: "message-2", role: "user", markdown: "Later message", created_at: "2026-08-12T09:00:00Z" },
    ]} agentRunEvents={[
      { run_id: "run-1", project_id: "project-1", sequence: 1, timestamp: "2026-08-12T08:01:00Z", kind: "stdout", title: "stdout", content: "full streamed output", iteration: 1 },
      { run_id: "run-1", project_id: "project-1", sequence: 2, timestamp: "2026-08-12T08:01:05Z", kind: "agent_completed", title: "Completed", content: "Done", iteration: null },
    ]} />);

    expect(screen.queryByRole("navigation", { name: "Agent and server interaction navigation" })).not.toBeInTheDocument();
    const fold = screen.getByRole("group", { name: "Agent run" });
    expect(fold).not.toHaveAttribute("open");
    fireEvent.click(screen.getByText("Processed 5s"));
    expect(fold).toHaveAttribute("open");
    expect(screen.getByText("full streamed output")).toBeInTheDocument();
  });

  it("keeps a stop control visible while the remote agent is active", async () => {
    const onCancelRun = vi.fn();
    const { rerender } = render(<WorkspaceShell project={project} locale="zh-CN" onLocaleChange={() => undefined} runStarted onCancelRun={onCancelRun} agentRunEvents={[
      { run_id: "run-1", project_id: "project-1", sequence: 1, timestamp: "2026-08-12T08:00:00Z", kind: "model_waiting", title: "Evaluating", content: "Working", iteration: 1 },
    ]} />);

    fireEvent.click(screen.getByRole("button", { name: "终止运行" }));
    expect(onCancelRun).toHaveBeenCalledOnce();

    rerender(<WorkspaceShell project={project} locale="zh-CN" onLocaleChange={() => undefined} runStarted runStopping onCancelRun={onCancelRun} agentRunEvents={[
      { run_id: "run-1", project_id: "project-1", sequence: 2, timestamp: "2026-08-12T08:00:01Z", kind: "cancel_requested", title: "Stopping", content: "Cancellation requested", iteration: null },
    ]} />);
    expect(screen.getByRole("button", { name: "终止中…" })).toBeDisabled();
  });

  it("hides the stop control after cancellation is confirmed", () => {
    render(<WorkspaceShell project={project} locale="zh-CN" onLocaleChange={() => undefined} runStarted onCancelRun={() => undefined} agentRunEvents={[
      { run_id: "run-1", project_id: "project-1", sequence: 3, timestamp: "2026-08-12T08:00:02Z", kind: "agent_canceled", title: "Stopped", content: "Canceled", iteration: null },
    ]} />);

    expect(screen.queryByRole("button", { name: "终止运行" })).not.toBeInTheDocument();
  });

  it("replays Harness v3 tool, artifact, reviewer, and user-input cards", () => {
    const onAnswerAgentQuestionV3 = vi.fn();
    render(<WorkspaceShell project={project} locale="en-US" onLocaleChange={() => undefined}
      runStarted activeRunId="run-v3" onAnswerAgentQuestionV3={onAnswerAgentQuestionV3}
      agentRunEventsV3={[
        { run_id: "run-v3", project_id: "project-1", conversation_id: "conversation-1", sequence: 1, previous_hash: "0", event_hash: "a", occurred_at: "2026-08-16T08:00:00Z", event: { kind: "run_started" } },
        { run_id: "run-v3", project_id: "project-1", conversation_id: "conversation-1", sequence: 2, previous_hash: "a", event_hash: "b", occurred_at: "2026-08-16T08:00:01Z", event: { kind: "tool_call_requested", payload: { request: { call_id: "call-1", tool_id: "artifact.verify", arguments: { path: "results/pbmc.h5ad" }, idempotency_key: "verify-1" } } } },
        { run_id: "run-v3", project_id: "project-1", conversation_id: "conversation-1", sequence: 3, previous_hash: "b", event_hash: "c", occurred_at: "2026-08-16T08:00:02Z", event: { kind: "tool_call_finished", payload: { outcome: { call_id: "call-1", status: "succeeded", model_content: "verified", structured_result: { size_bytes: 42 }, error: null, truncated: false, provenance: ["remote:results/pbmc.h5ad", "sha256:abc"] } } } },
        { run_id: "run-v3", project_id: "project-1", conversation_id: "conversation-1", sequence: 4, previous_hash: "c", event_hash: "d", occurred_at: "2026-08-16T08:00:03Z", event: { kind: "completion_ledger_updated", payload: { ledger: { criteria: [{ id: "qc", description: "QC complete", evidence_sequences: [3] }], unresolved_errors: [], uncertain_side_effects: [], verified_artifacts: [{ path: "results/pbmc.h5ad", size_bytes: 42, sha256: "a".repeat(64), evidence_sequence: 3 }] } } } },
        { run_id: "run-v3", project_id: "project-1", conversation_id: "conversation-1", sequence: 5, previous_hash: "d", event_hash: "e", occurred_at: "2026-08-16T08:00:04Z", event: { kind: "review_completed", payload: { report: { cycle: 1, findings: [{ severity: "warn", summary: "Record package versions", evidence: ["results/report.html"] }] } } } },
        { run_id: "run-v3", project_id: "project-1", conversation_id: "conversation-1", sequence: 6, previous_hash: "e", event_hash: "f", occurred_at: "2026-08-16T08:00:05Z", event: { kind: "user_input_requested", payload: { question_id: "species", question: "Which species?" } } },
      ]} />);

    expect(screen.getByText("artifact.verify")).toBeInTheDocument();
    expect(screen.getByText("results/pbmc.h5ad")).toBeInTheDocument();
    expect(screen.getByText("Record package versions")).toBeInTheDocument();
    fireEvent.change(screen.getByRole("textbox", { name: "Answer: Which species?" }), { target: { value: "human" } });
    fireEvent.click(screen.getByRole("button", { name: "Submit answer" }));
    expect(onAnswerAgentQuestionV3).toHaveBeenCalledWith("run-v3", "species", "human");
  });

  it("coalesces adjacent Harness v3 model text deltas into one readable message", () => {
    const base = { run_id: "run-text", project_id: "project-1", conversation_id: "conversation-1" };
    const deltas = ["我", "先", "检查", "输入", "目录", "，", "项目", "可", "读"];
    render(<WorkspaceShell project={project} locale="zh-CN" onLocaleChange={() => undefined}
      runStarted activeRunId="run-text"
      agentRunEventsV3={[
        { ...base, sequence: 1, previous_hash: "0", event_hash: "1", occurred_at: "2026-08-17T00:00:00Z", event: { kind: "run_started" } },
        { ...base, sequence: 2, previous_hash: "1", event_hash: "2", occurred_at: "2026-08-17T00:00:01Z", event: { kind: "model_step_started", payload: { step: 1 } } },
        ...deltas.map((text, index) => ({ ...base, sequence: index + 3, previous_hash: String(index + 2), event_hash: String(index + 3), occurred_at: "2026-08-17T00:00:02Z", event: { kind: "model_text" as const, payload: { text } } })),
      ]} />);

    const output = screen.getByRole("article", { name: "模型输出" });
    expect(output).toHaveTextContent("我先检查输入目录，项目可读");
    expect(screen.getAllByRole("article", { name: "模型输出" })).toHaveLength(1);
  });

  it("shows only the latest Harness v3 completion ledger snapshot", () => {
    const base = { run_id: "run-ledger", project_id: "project-1", conversation_id: "conversation-1" };
    const criterion = { id: "input", description: "输入已检查", evidence_sequences: [] as number[] };
    render(<WorkspaceShell project={project} locale="zh-CN" onLocaleChange={() => undefined}
      runStarted activeRunId="run-ledger"
      agentRunEventsV3={[
        { ...base, sequence: 1, previous_hash: "0", event_hash: "1", occurred_at: "2026-08-17T00:00:00Z", event: { kind: "completion_ledger_updated", payload: { ledger: { criteria: [criterion], unresolved_errors: ["tool:remote.list: old transient error"], uncertain_side_effects: [], verified_artifacts: [] } } } },
        { ...base, sequence: 2, previous_hash: "1", event_hash: "2", occurred_at: "2026-08-17T00:00:01Z", event: { kind: "completion_ledger_updated", payload: { ledger: { criteria: [{ ...criterion, evidence_sequences: [2] }], unresolved_errors: [], uncertain_side_effects: [], verified_artifacts: [] } } } },
      ]} />);

    expect(screen.getAllByRole("article", { name: "完成账本" })).toHaveLength(1);
    expect(screen.queryByText("tool:remote.list: old transient error")).not.toBeInTheDocument();
    expect(screen.getByRole("article", { name: "完成账本" })).toHaveTextContent("1/1");
  });

  it("turns off every historical running indicator as soon as cancellation starts", () => {
    render(<WorkspaceShell project={project} locale="en-US" onLocaleChange={() => undefined} runStarted runStopping onCancelRun={() => undefined} agentRunEvents={[
      { run_id: "run-1", project_id: "project-1", sequence: 1, timestamp: "2026-08-12T08:00:00Z", kind: "model_waiting", title: "Evaluating remote evidence", content: "30s elapsed", iteration: 8 },
      { run_id: "run-1", project_id: "project-1", sequence: 2, timestamp: "2026-08-12T08:00:01Z", kind: "tool_waiting", title: "Remote command is still running", content: "20s elapsed", iteration: 8 },
      { run_id: "run-1", project_id: "project-1", sequence: 3, timestamp: "2026-08-12T08:00:02Z", kind: "cancel_requested", title: "Stopping remote agent", content: "Cancellation requested", iteration: null },
    ]} />);

    expect(screen.queryByText("The model is still working")).not.toBeInTheDocument();
    expect(screen.queryByText("The remote command is still running", { selector: ".agent-working" })).not.toBeInTheDocument();
    expect(screen.getByText("Stopping current operation…")).toBeInTheDocument();
  });

  it("uploads only through the explicit file selection action", () => {
    const onUploadFiles = vi.fn();
    render(<WorkspaceShell project={project} locale="zh-CN" onLocaleChange={() => undefined} onUploadFiles={onUploadFiles} remoteFiles={[{ relative_path: "results/umap.png", directory: false, size_bytes: 42, modified_unix_seconds: 1 }]} />);
    expect(screen.getByText("results")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("treeitem", { name: "展开文件夹：results" }));
    expect(screen.getByText("umap.png")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "选择上传文件" }));
    expect(onUploadFiles).toHaveBeenCalledOnce();
  });

  it("returns from the project workspace to the project library", () => {
    const onBackToProjects = vi.fn();
    render(<WorkspaceShell project={project} locale="zh-CN" onLocaleChange={() => undefined} onBackToProjects={onBackToProjects} />);
    fireEvent.click(screen.getByRole("button", { name: "返回项目主页" }));
    expect(onBackToProjects).toHaveBeenCalledOnce();
  });

  it("renders persisted conversation titles and wires selection and creation", () => {
    const onSelectConversation = vi.fn();
    const onNewConversation = vi.fn();
    const onDeleteConversation = vi.fn();
    const confirm = vi.spyOn(window, "confirm").mockReturnValue(true);
    render(<WorkspaceShell project={project} locale="zh-CN" onLocaleChange={() => undefined}
      conversations={[
        { id: "conversation-2", project_id: project.id, title: "比较两批 PBMC 的批次效应", status: "idle", model_profile_id: null, created_at: "2026-08-12T08:01:00Z", updated_at: "2026-08-12T08:01:00Z" },
        { id: "conversation-1", project_id: project.id, title: "检查 hg19 单细胞数据质量", status: "idle", model_profile_id: null, created_at: "2026-08-12T08:00:00Z", updated_at: "2026-08-12T08:00:00Z" },
      ]}
      activeConversationId="conversation-2" onSelectConversation={onSelectConversation} onNewConversation={onNewConversation} onDeleteConversation={onDeleteConversation} />);

    expect(screen.getByRole("heading", { name: "比较两批 PBMC 的批次效应" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "检查 hg19 单细胞数据质量" }));
    expect(onSelectConversation).toHaveBeenCalledWith("conversation-1");
    fireEvent.click(screen.getByRole("button", { name: "新建会话" }));
    expect(onNewConversation).toHaveBeenCalledOnce();
    fireEvent.click(screen.getByRole("button", { name: "删除会话：检查 hg19 单细胞数据质量" }));
    expect(confirm).toHaveBeenCalledWith("确定删除会话“检查 hg19 单细胞数据质量”吗？此操作无法撤销。");
    expect(onDeleteConversation).toHaveBeenCalledWith("conversation-1");
    expect(screen.queryByText("文献证据")).not.toBeInTheDocument();
    expect(screen.queryByText("报告生成")).not.toBeInTheDocument();
    confirm.mockRestore();
  });

  it("runs and explicitly promotes a saved exploration cell", async () => {
    const onExecuteKernel = vi.fn().mockResolvedValue(0);
    const onPromoteKernelCell = vi.fn().mockResolvedValue({ name: "探索代码步骤", version: 1, language: "python", code: "print(1)", code_sha256: "a".repeat(64) });
    render(<WorkspaceShell project={project} locale="zh-CN" onLocaleChange={() => undefined} kernelSessions={[{ id: "kernel-1", project_id: project.id, language: "python", state: "running" }]} onExecuteKernel={onExecuteKernel} onPromoteKernelCell={onPromoteKernelCell} />);

    fireEvent.click(screen.getByRole("tab", { name: "探索" }));
    fireEvent.change(screen.getByRole("textbox", { name: "探索代码" }), { target: { value: "print(1)" } });
    fireEvent.click(screen.getByRole("button", { name: "执行" }));
    await waitFor(() => expect(onExecuteKernel).toHaveBeenCalledWith("kernel-1", "print(1)", true, []));
    fireEvent.click(screen.getByRole("button", { name: "固化已保存单元 #1 为正式步骤" }));
    await waitFor(() => expect(onPromoteKernelCell).toHaveBeenCalledWith("kernel-1", 0, "探索代码步骤"));
    expect(await screen.findByText(/仍需进入正式计划审批/)).toBeInTheDocument();
  });

  it("shows traceable notebook facts and controls resumable transfers", async () => {
    const onSearchMemory = vi.fn();
    const onRetrySync = vi.fn();
    render(<WorkspaceShell project={project} locale="zh-CN" onLocaleChange={() => undefined}
      notebookEntries={[{ id: "note-1", project_id: project.id, conversation_id: null, turn_id: null, kind: "method", title: "方法", markdown: "使用项目内环境完成 QC", confidence: null, evidence_ids: ["command:run:1"], artifact_ids: [], created_at: "2026-08-12T00:00:00Z", updated_at: "2026-08-12T00:00:00Z" }]}
      memoryFacts={[{ id: "fact-1", project_id: project.id, conversation_id: null, run_id: "run-1", dimension: "environment", key: "python", value: "3.11", statement: "Python 环境已验证", evidence: [{ source_kind: "command", source_id: "run-1:1", excerpt: "python --version" }], conflicted_with: ["fact-2"], created_at: "2026-08-12T00:00:00Z" }]}
      syncEntries={[{ id: "sync-1", project_id: project.id, relative_path: "results/large.h5ad", local_relative_path: "results/large.h5ad", remote_path: "/srv/results/large.h5ad", direction: "remote_to_local", size_bytes: 100, sha256: "a".repeat(64), state: "paused", transferred_bytes: 40, retry_count: 1, error: null, updated_at: "2026-08-12T00:00:00Z" }]}
      onSearchMemory={onSearchMemory} onRetrySync={onRetrySync} />);

    expect(screen.getByText("传输任务")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "续传/重试" }));
    expect(onRetrySync).toHaveBeenCalledWith("sync-1");
    fireEvent.click(screen.getByRole("tab", { name: "实验记录" }));
    expect(screen.getByText("存在冲突事实，已保留双方来源")).toBeInTheDocument();
    fireEvent.change(screen.getByRole("textbox", { name: "检索记忆" }), { target: { value: "Python" } });
    fireEvent.click(screen.getByRole("button", { name: "检索" }));
    expect(onSearchMemory).toHaveBeenCalledWith("Python", undefined);
  });
});
