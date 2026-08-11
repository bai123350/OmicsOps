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
