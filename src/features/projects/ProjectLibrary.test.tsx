import { useState } from "react";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { ProjectLibrary } from "./ProjectLibrary";
import { useWindowEscapeLayer } from "../settings/BrowserSettings";

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

afterEach(() => vi.restoreAllMocks());

const templateCases = [
  { label: "单细胞 RNA 测序", template: "single_cell_rna_seq", localRoot: "E:/Science/single-cell" },
  { label: "Bulk RNA 测序", template: "bulk_rna_seq", localRoot: "E:/Science/bulk-rna" },
  { label: "文献综述", template: "literature_review", localRoot: "E:/Science/literature" },
  { label: "空白研究项目", template: "blank", localRoot: "E:/Science/blank" },
] as const;

describe("ProjectLibrary homepage actions", () => {
  it("keeps the brand promise and routes search, locale, and settings actions", () => {
    const onOpenSearch = vi.fn();
    const onLocaleChange = vi.fn();
    const onSettings = vi.fn();
    render(<ProjectLibrary {...baseProps} onOpenSearch={onOpenSearch} onLocaleChange={onLocaleChange} onSettings={onSettings} onChooseLocalRoot={vi.fn()} onCreate={vi.fn()} />);

    expect(screen.getByText("OmicsOps")).toBeInTheDocument();
    expect(screen.getByText("面向可复现科研的本地工作台")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "搜索工作区" }));
    fireEvent.click(screen.getByRole("button", { name: "切换为 English" }));
    fireEvent.click(screen.getByRole("button", { name: "设置" }));

    expect(onOpenSearch).toHaveBeenCalledOnce();
    expect(onLocaleChange).toHaveBeenCalledWith("en-US");
    expect(onSettings).toHaveBeenCalledOnce();
  });

  it.each(templateCases)("creates the $label template with its default name and mapping", async ({ label, template, localRoot }) => {
    const onCreate = vi.fn().mockResolvedValue(undefined);
    render(<ProjectLibrary {...baseProps} onChooseLocalRoot={vi.fn().mockResolvedValue({ path: localRoot, usedFallback: false })} onCreate={onCreate} />);

    fireEvent.click(screen.getByRole("button", { name: new RegExp(`^${label}`) }));
    const dialog = screen.getByRole("dialog", { name: "创建新项目" });
    expect(within(dialog).getByRole("textbox", { name: "项目名称" })).toHaveValue(label);
    fireEvent.click(within(dialog).getByRole("button", { name: "选择目录" }));
    expect(await within(dialog).findByText(localRoot)).toBeInTheDocument();
    fireEvent.click(within(dialog).getByRole("button", { name: "创建项目" }));

    await waitFor(() => expect(onCreate).toHaveBeenCalledWith({
      template,
      name: label,
      localRoot,
      connectionId: null,
      remoteRoot: null,
    }));
  });

  it("keeps project creation available when there are no recent projects", () => {
    render(<ProjectLibrary {...baseProps} onChooseLocalRoot={vi.fn()} onCreate={vi.fn()} />);

    expect(screen.getByRole("heading", { name: "还没有项目" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "创建空白项目" }));
    expect(screen.getByRole("dialog", { name: "创建新项目" })).toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: "项目名称" })).toHaveValue("空白研究项目");
  });

  it("opens a project without deleting it and deletes it without opening it again", async () => {
    const onOpen = vi.fn();
    const onDelete = vi.fn().mockResolvedValue(undefined);
    vi.spyOn(window, "confirm").mockReturnValue(true);
    render(<ProjectLibrary {...baseProps} projects={[recentProject]} onOpen={onOpen} onDelete={onDelete} onChooseLocalRoot={vi.fn()} onCreate={vi.fn()} />);

    fireEvent.click(screen.getByRole("button", { name: "打开项目：PBMC 图谱" }));
    expect(onOpen).toHaveBeenCalledOnce();
    expect(onOpen).toHaveBeenCalledWith(recentProject);
    expect(onDelete).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole("button", { name: "删除项目：PBMC 图谱" }));
    await waitFor(() => expect(onDelete).toHaveBeenCalledWith("project-1"));
    expect(onOpen).toHaveBeenCalledOnce();
  });

  it("preserves a remote path and reports a missing saved connection binding", () => {
    const missingRemoteProject = {
      ...recentProject,
      connection_id: "connection-no-longer-present",
      remote_root: "/srv/omics/pbmc-atlas",
    };
    render(<ProjectLibrary {...baseProps} projects={[missingRemoteProject]} connections={[]} onChooseLocalRoot={vi.fn()} onCreate={vi.fn()} />);

    expect(screen.getByText("远端连接未找到")).toBeInTheDocument();
    expect(screen.getByText(/\/srv\/omics\/pbmc-atlas/)).toBeInTheDocument();
    expect(screen.queryByText(/仅本地，未启用远端计算/)).not.toBeInTheDocument();
  });

  it("distinguishes same-name project actions by their accessible descriptions", () => {
    const localProject = {
      ...recentProject,
      id: "project-local-same-name",
      name: "同名项目",
      local_root: "E:/Science/local-same-name",
    };
    const remoteProject = {
      ...recentProject,
      id: "project-remote-same-name",
      name: "同名项目",
      local_root: "D:/Science/remote-same-name",
      remote_root: "/srv/omics/remote-same-name",
      connection_id: "connection-trusted-same-name",
    };
    const trustedConnection = {
      id: "connection-trusted-same-name",
      label: "Trusted Linux",
      host: "compute.example.org",
      port: 20090,
      username: "researcher",
      authentication: "password" as const,
      authentication_reference: "ssh/connection-trusted-same-name",
      host_key_fingerprint: "SHA256:trusted-same-name",
    };
    render(
      <ProjectLibrary
        {...baseProps}
        projects={[localProject, remoteProject]}
        connections={[trustedConnection]}
        onChooseLocalRoot={vi.fn()}
        onCreate={vi.fn()}
      />,
    );

    const [localOpen, remoteOpen] = screen.getAllByRole("button", { name: "打开项目：同名项目" });
    expect(localOpen).toHaveAttribute("aria-label", "打开项目：同名项目");
    expect(remoteOpen).toHaveAttribute("aria-label", "打开项目：同名项目");
    expect(localOpen).toHaveAccessibleDescription(/E:\/Science\/local-same-name.*仅本地，未启用远端计算/);
    expect(remoteOpen).toHaveAccessibleDescription(/D:\/Science\/remote-same-name.*researcher@compute\.example\.org:20090.*\/srv\/omics\/remote-same-name/);

    const [localDelete, remoteDelete] = screen.getAllByRole("button", { name: "删除项目：同名项目" });
    expect(localDelete).toHaveAttribute("aria-label", "删除项目：同名项目");
    expect(remoteDelete).toHaveAttribute("aria-label", "删除项目：同名项目");
    expect(localDelete).toHaveAccessibleDescription(/E:\/Science\/local-same-name.*仅本地，未启用远端计算/);
    expect(remoteDelete).toHaveAccessibleDescription(/D:\/Science\/remote-same-name.*researcher@compute\.example\.org:20090.*\/srv\/omics\/remote-same-name/);
  });
});

describe("ProjectLibrary project location flow", () => {
  it("closes only the create dialog on the first immediate window Escape", () => {
    const closeParent = vi.fn();
    function Host() {
      const [open, setOpen] = useState(true);
      useWindowEscapeLayer(open, () => { closeParent(); setOpen(false); });
      return open ? <ProjectLibrary {...baseProps} onChooseLocalRoot={vi.fn().mockResolvedValue({ path: null, usedFallback: false })} onCreate={vi.fn()} /> : null;
    }
    render(<Host />);
    fireEvent.click(screen.getByRole("button", { name: /单细胞 RNA 测序/ }));
    expect(screen.getByRole("button", { name: "创建项目" })).toBeInTheDocument();
    fireEvent.keyDown(window, { key: "Escape" });
    expect(screen.queryByRole("button", { name: "创建项目" })).not.toBeInTheDocument();
    expect(closeParent).not.toHaveBeenCalled();
    fireEvent.keyDown(window, { key: "Escape" });
    expect(closeParent).toHaveBeenCalledOnce();
  });

  it("makes a local-only project explicit before creation", async () => {
    const onCreate = vi.fn().mockResolvedValue(undefined);
    render(<ProjectLibrary {...baseProps} connections={[]} onChooseLocalRoot={vi.fn().mockResolvedValue({ path: "E:/Science/pbmc", usedFallback: false })} onCreate={onCreate} />);

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
    render(<ProjectLibrary {...baseProps} connections={[connection]} onChooseLocalRoot={vi.fn().mockResolvedValue({ path: "E:/Science/pbmc", usedFallback: false })} onCreate={onCreate} />);

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

  it("reports when a missing saved start folder falls back to the system picker", async () => {
    render(<ProjectLibrary {...baseProps} onChooseLocalRoot={vi.fn().mockResolvedValue({ path: "E:/Replacement", usedFallback: true })} onCreate={vi.fn()} />);
    fireEvent.click(screen.getByRole("button", { name: /单细胞 RNA 测序/ }));

    fireEvent.click(screen.getByRole("button", { name: "选择目录" }));

    expect(await screen.findByRole("status")).toHaveTextContent("保存的起始目录已不可用，文件夹选择器已使用系统默认位置");
    expect(screen.getByText("E:/Replacement")).toBeInTheDocument();
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
