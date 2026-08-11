import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { ProjectLibrary } from "./ProjectLibrary";

const baseProps = {
  projects: [],
  locale: "zh-CN" as const,
  onLocaleChange: () => undefined,
  onOpen: () => undefined,
  onSettings: () => undefined,
};

describe("ProjectLibrary project location flow", () => {
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
});
