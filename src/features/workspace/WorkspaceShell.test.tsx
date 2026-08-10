import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

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
});
