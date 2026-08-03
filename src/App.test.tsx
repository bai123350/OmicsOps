import { fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import App from "./App";
import * as api from "./tauri-api";

afterEach(() => {
  vi.restoreAllMocks();
});

describe("OmicsOps desktop workflow", () => {
  it("presents the five business stages", () => {
    render(<App />);

    expect(screen.getByRole("button", { name: /服务器连接/ })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /远端项目/ })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /方案确认/ })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /运行监控/ })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /结果/ })).toBeInTheDocument();
  });

  it("moves between project and run views without losing task identity", () => {
    render(<App />);

    fireEvent.click(screen.getByRole("button", { name: /运行监控/ }));
    expect(screen.getByText("RUN-2026-0001")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: /结果/ }));
    expect(screen.getByText(/远端产物/)).toBeInTheDocument();
  });

  it("shows the exact pending action before approval can resume a run", async () => {
    vi.spyOn(api, "listRuns").mockResolvedValue([
      {
        run_id: "a0000000-0000-4000-8000-000000000001",
        profile_id: "a0000000-0000-4000-8000-000000000002",
        project_id: "a0000000-0000-4000-8000-000000000003",
        plan_id: "a0000000-0000-4000-8000-000000000004",
        state: "paused_for_approval",
        stage_index: 2,
        step_index: 1,
        pending_approval: {
          id: "a0000000-0000-4000-8000-000000000005",
          run_id: "a0000000-0000-4000-8000-000000000001",
          reason: "将覆盖已验证产物",
          proposed_action: "python export.py --overwrite results/pbmc.h5ad",
          impact: "替换现有 h5ad",
          alternatives: ["取消运行"],
        },
      },
    ]);

    render(<App />);
    fireEvent.click(screen.getByRole("button", { name: /运行监控/ }));

    expect(await screen.findByText("将覆盖已验证产物")).toBeInTheDocument();
    expect(
      screen.getByText("python export.py --overwrite results/pbmc.h5ad"),
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "批准并继续" })).toBeInTheDocument();
  });

  it("opens a dialog that keeps server progress visible", async () => {
    vi.spyOn(api, "listRuns").mockResolvedValue([
      {
        run_id: "b0000000-0000-4000-8000-000000000001",
        profile_id: "b0000000-0000-4000-8000-000000000002",
        project_id: "b0000000-0000-4000-8000-000000000003",
        plan_id: "b0000000-0000-4000-8000-000000000004",
        state: "running",
        stage_index: 0,
        step_index: 0,
        pending_approval: null,
      },
    ]);
    vi.spyOn(api, "listRunEvents").mockResolvedValue([
      {
        sequence: 1,
        timestamp: "2026-07-30T10:00:00Z",
        run_id: "b0000000-0000-4000-8000-000000000001",
        stage_id: "analysis",
        step_id: "qc",
        attempt: 0,
        action: "step_started",
        state: "running",
        log_reference: null,
        reason: "服务器正在执行质量控制",
      },
    ]);

    render(<App />);
    fireEvent.click(screen.getByRole("button", { name: /运行监控/ }));
    fireEvent.click(await screen.findByRole("button", { name: /查看进度/ }));

    const dialog = screen.getByRole("dialog", { name: "服务器任务进度" });
    expect(dialog).toBeInTheDocument();
    expect(within(dialog).getByText("服务器正在执行质量控制")).toBeInTheDocument();
    expect(screen.getByRole("progressbar", { name: "服务器任务完成进度" })).toBeInTheDocument();
  });
});
