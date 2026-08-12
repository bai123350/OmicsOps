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

  it("sends a research message and requires explicit plan approval", () => {
    render(<WorkspaceShell project={project} locale="zh-CN" onLocaleChange={() => undefined} />);
    fireEvent.change(screen.getByRole("textbox", { name: /描述研究目标/ }), { target: { value: "先检查双细胞率" } });
    fireEvent.click(screen.getByRole("button", { name: "发送" }));
    expect(screen.getByText("先检查双细胞率")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "批准计划" }));
    expect(screen.getByRole("button", { name: "已批准" })).toBeDisabled();
  });

  it("shows the real agent state instead of a fixed remote progress value", async () => {
    let finish!: (value: boolean) => void;
    const onSend = vi.fn(() => new Promise<boolean>((resolve) => { finish = resolve; }));
    const { rerender } = render(<WorkspaceShell project={project} locale="zh-CN" onLocaleChange={() => undefined} onSend={onSend} />);

    expect(screen.queryByText("65%")).not.toBeInTheDocument();
    fireEvent.change(screen.getByRole("textbox", { name: /描述研究目标/ }), { target: { value: "检查 hg19 数据" } });
    fireEvent.click(screen.getByRole("button", { name: "发送" }));
    expect(screen.getByRole("textbox", { name: /描述研究目标/ })).toHaveValue("");

    rerender(<WorkspaceShell project={project} locale="zh-CN" onLocaleChange={() => undefined} onSend={onSend} agentBusy agentNotice="503 model_not_found" />);
    expect(screen.getByRole("status")).toHaveTextContent("正在等待模型响应");
    expect(screen.getByRole("alert")).toHaveTextContent("503 model_not_found");
    finish(true);
    await waitFor(() => expect(onSend).toHaveBeenCalledWith("检查 hg19 数据"));
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

  it("shows public evaluation content and collapses repeated waiting heartbeats", () => {
    render(<WorkspaceShell project={project} locale="en-US" onLocaleChange={() => undefined} runStarted agentRunEvents={[
      { run_id: "run-1", project_id: "project-1", sequence: 1, timestamp: "2026-08-12T08:00:00Z", kind: "model_waiting", title: "Evaluating remote evidence", content: "10s elapsed", iteration: 1 },
      { run_id: "run-1", project_id: "project-1", sequence: 2, timestamp: "2026-08-12T08:00:10Z", kind: "model_waiting", title: "Evaluating remote evidence", content: "20s elapsed", iteration: 1 },
      { run_id: "run-1", project_id: "project-1", sequence: 3, timestamp: "2026-08-12T08:00:11Z", kind: "model_progress", title: "Agent evaluation", content: "Verified the matrix and barcode files exist.", iteration: 1 },
      { run_id: "run-1", project_id: "project-1", sequence: 4, timestamp: "2026-08-12T08:00:12Z", kind: "model_progress", title: "Agent evaluation", content: "Next I will inspect their headers without modifying data.", iteration: 1 },
    ]} />);

    expect(screen.queryByText("10s elapsed")).not.toBeInTheDocument();
    expect(screen.getByText("20s elapsed")).toBeInTheDocument();
    expect(screen.getByText(/Verified the matrix.*Next I will inspect/s)).toBeInTheDocument();
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
    expect(screen.getByText("results/umap.png")).toBeInTheDocument();
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
    render(<WorkspaceShell project={project} locale="zh-CN" onLocaleChange={() => undefined}
      conversations={[
        { id: "conversation-2", project_id: project.id, title: "比较两批 PBMC 的批次效应", status: "idle", model_profile_id: null, created_at: "2026-08-12T08:01:00Z", updated_at: "2026-08-12T08:01:00Z" },
        { id: "conversation-1", project_id: project.id, title: "检查 hg19 单细胞数据质量", status: "idle", model_profile_id: null, created_at: "2026-08-12T08:00:00Z", updated_at: "2026-08-12T08:00:00Z" },
      ]}
      activeConversationId="conversation-2" onSelectConversation={onSelectConversation} onNewConversation={onNewConversation} />);

    expect(screen.getByRole("heading", { name: "比较两批 PBMC 的批次效应" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: /检查 hg19 单细胞数据质量/ }));
    expect(onSelectConversation).toHaveBeenCalledWith("conversation-1");
    fireEvent.click(screen.getByRole("button", { name: "新建会话" }));
    expect(onNewConversation).toHaveBeenCalledOnce();
    expect(screen.queryByText("文献证据")).not.toBeInTheDocument();
    expect(screen.queryByText("报告生成")).not.toBeInTheDocument();
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
});
