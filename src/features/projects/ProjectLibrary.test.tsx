import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { ProjectLibrary } from "./ProjectLibrary";

const baseProps = {
  projects: [],
  locale: "zh-CN" as const,
  onLocaleChange: () => undefined,
  onOpen: () => undefined,
  onDelete: vi.fn().mockResolvedValue(undefined),
  onSettings: () => undefined,
};

const recentProject = {
  id: "project-1",
  name: "PBMC 图谱",
  description: "",
  local_root: "E:/Science/pbmc",
  remote_root: null,
  connection_id: null,
  template: "single_cell_rna_seq" as const,
  status: "ready" as const,
  ollama_only: false,
  created_at: "2026-08-14T00:00:00Z",
  updated_at: "2026-08-14T00:00:00Z",
};

describe("ProjectLibrary project location flow", () => {
  it("closes the create dialog on window Escape without moving focus into it", () => {
    render(<ProjectLibrary {...baseProps} onChooseLocalRoot={vi.fn().mockResolvedValue(null)} onCreate={vi.fn()} />);
    fireEvent.click(screen.getByRole("button", { name: /单细胞 RNA 测序/ }));
    expect(screen.getByRole("button", { name: "创建项目" })).toBeInTheDocument();
    fireEvent.keyDown(window, { key: "Escape" });
    expect(screen.queryByRole("button", { name: "创建项目" })).not.toBeInTheDocument();
  });

  it("makes a local-only project explicit before creation", async () => {
    const onCreate = vi.fn().mockResolvedValue(undefined);
    render(<ProjectLibrary {...baseProps} connections={[]} onChooseLocalRoot={vi.fn().mockResolvedValue("E:/Science/pbmc")} onCreate={onCreate} />);

    fireEvent.click(screen.getByRole("button", { name: /单细胞 RNA 测序/ }));
    expect(screen.getByRole("radio", { name: /仅本地/ })).toBeChecked();
    expect(screen.getByText(/SSH 分析和远端探索暂不可用/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "选择目录" }));
    expect(await screen.findByText("E:/Science/pbmc")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "创建项目" }));
    await waitFor(() => expect(onCreate).toHaveBeenCalledWith(expect.objectContaining({ localRoot: "E:/Science/pbmc", connectionId: null, remoteRoot: null })));
  });

  it("shows the exact trusted server address for hybrid projects", async () => {
    const connection = { id: "connection-1", label: "Lab Linux", host: "compute.example.org", port: 20090, username: "researcher", authentication: "password" as const, authentication_reference: "ssh/connection-1", host_key_fingerprint: "SHA256:trusted" };
    const onCreate = vi.fn().mockResolvedValue(undefined);
    render(<ProjectLibrary {...baseProps} connections={[connection]} onChooseLocalRoot={vi.fn().mockResolvedValue("E:/Science/pbmc")} onCreate={onCreate} />);

    fireEvent.click(screen.getByRole("button", { name: /单细胞 RNA 测序/ }));
    expect(screen.getByRole("radio", { name: /本地工作区 \+ 远端 Linux 计算/ })).toBeChecked();
    expect(screen.getAllByText(/researcher@compute.example.org:20090/).length).toBeGreaterThan(0);
    fireEvent.click(screen.getByRole("button", { name: "选择目录" }));
    expect(await screen.findByText("E:/Science/pbmc")).toBeInTheDocument();
    fireEvent.change(screen.getByLabelText("服务器项目目录"), { target: { value: "/home/researcher/pbmc" } });
    fireEvent.click(screen.getByRole("button", { name: "创建项目" }));
    await waitFor(() => expect(onCreate).toHaveBeenCalledWith(expect.objectContaining({ connectionId: "connection-1", remoteRoot: "/home/researcher/pbmc" })));
  });

  it("confirms and deletes a recent project without opening it", async () => {
    const onOpen = vi.fn();
    const onDelete = vi.fn().mockResolvedValue(undefined);
    const confirm = vi.spyOn(window, "confirm").mockReturnValue(true);
    render(<ProjectLibrary {...baseProps} projects={[recentProject]} onOpen={onOpen} onDelete={onDelete} onChooseLocalRoot={vi.fn()} onCreate={vi.fn()} />);

    fireEvent.click(screen.getByRole("button", { name: "删除项目：PBMC 图谱" }));

    expect(confirm).toHaveBeenCalledWith(expect.stringContaining("本地目录 E:/Science/pbmc 及远端文件不会被删除"));
    await waitFor(() => expect(onDelete).toHaveBeenCalledWith("project-1"));
    expect(onOpen).not.toHaveBeenCalled();
  });

  it("keeps the project when deletion is canceled or fails", async () => {
    const onDelete = vi.fn().mockRejectedValue(new Error("请先停止活动任务"));
    const confirm = vi.spyOn(window, "confirm");
    confirm.mockReturnValueOnce(false).mockReturnValueOnce(true);
    render(<ProjectLibrary {...baseProps} projects={[recentProject]} onDelete={onDelete} onChooseLocalRoot={vi.fn()} onCreate={vi.fn()} />);

    const deleteButton = screen.getByRole("button", { name: "删除项目：PBMC 图谱" });
    fireEvent.click(deleteButton);
    expect(onDelete).not.toHaveBeenCalled();
    fireEvent.click(deleteButton);

    expect(await screen.findByRole("alert")).toHaveTextContent("请先停止活动任务");
    expect(screen.getByText("PBMC 图谱")).toBeInTheDocument();
  });
});
