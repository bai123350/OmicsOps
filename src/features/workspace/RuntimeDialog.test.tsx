import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import type { ComputeBackendAvailabilityV4, RuntimeBoundaryViewV4 } from "../../types";
import { RuntimeDialog } from "./RuntimeDialog";
import * as api from "../../tauri-api";

const localBackend: ComputeBackendAvailabilityV4 = {
  descriptor: {
    schema_version: 4,
    backend_id: "local",
    kind: "local",
    isolation: "process",
    available: true,
    supports_python: true,
    supports_r: false,
    supports_network_policy: false,
  },
  selectable: true,
  reason: null,
  python_status: "available",
  r_status: "unavailable",
  resolved_image_id: null,
};

const sshBackend: ComputeBackendAvailabilityV4 = {
  ...localBackend,
  descriptor: { ...localBackend.descriptor, backend_id: "ssh:lab", kind: "ssh", supports_r: true },
  python_status: "available",
  r_status: "unverified",
};

const localBoundary: RuntimeBoundaryViewV4 = {
  project_id: "project-1", source: { kind: "draft_selection" },
  compute_selection: { schema_version: 4, backend_id: "local", backend_kind: "local", autonomy_mode: "supervised", environment: "system", network_policy: "host_inherited" },
  execution_location: "local_host", isolation: "process", limits: ["same_user_permissions", "project_cwd_not_access_control"],
  interactive_lifecycle: "run_scoped_no_restart_reconnect", detached_job_lifecycle: "unsupported", verification: "not_checked_by_this_view",
};

function renderDialog(overrides: Partial<React.ComponentProps<typeof RuntimeDialog>> = {}) {
  return render(
    <RuntimeDialog
      zh={false}
      language="python"
      onLanguageChange={vi.fn()}
      backend={localBackend}
      environment="system"
      onClose={vi.fn()}
      onPrepare={vi.fn()}
      boundary={localBoundary}
      boundaryLoading={false}
      boundaryError=""
      onRetryBoundary={vi.fn()}
      {...overrides}
    />,
  );
}

describe("RuntimeDialog", () => {
  it("lets a draft select a discovered R environment even when system is selected and no automatic suggestion exists", () => {
    const onEnvironmentChange = vi.fn();
    const system = { environment: "system", label: "system", interpreters: [{ language: "python" as const, status: "available" as const, version: "Python 3.12.10" }, { language: "r" as const, status: "not_found" as const }] };
    const r = { environment: "envs:r443", label: ".envs/r443", interpreters: [{ language: "r" as const, status: "available" as const, version: "R 4.4.3" }] };
    const probe = { selected: system, refresh: vi.fn(), loading: false, error: false, report: { project_id: "project-1", connection_id: "lab", remote_root: "/work/project", requested_environment: "system", observed_at: "now", suggested_environment: null, truncated: true, environments: [system, r] } };
    const boundary = { ...localBoundary, execution_location: "ssh_host" as const, compute_selection: { ...localBoundary.compute_selection, backend_id: "ssh:lab", backend_kind: "ssh" as const } };
    renderDialog({ backend: sshBackend, boundary, sshEnvironments: probe, onEnvironmentChange });
    const choices = screen.getByRole("combobox", { name: "Discovered SSH environments" });
    expect(choices).toHaveValue("system");
    for (let index = 0; index < 4; index++) fireEvent.keyDown(window, { key: "Tab" });
    expect(choices).toHaveFocus();
    expect(screen.getByRole("region", { name: "SSH interpreter detection" })).toHaveTextContent("Python 3.12.10");
    expect(screen.getByRole("region", { name: "SSH interpreter detection" })).toHaveTextContent("Interpreter not found");
    fireEvent.change(choices, { target: { value: "envs:r443" } });
    expect(onEnvironmentChange).toHaveBeenCalledWith("envs:r443");
    expect(screen.getByText(/compute configuration menu/)).toBeInTheDocument();
  });

  it("does not offer a live environment selector or probe on a frozen SSH run even when callbacks are supplied", () => {
    const onEnvironmentChange = vi.fn(); const refresh = vi.fn();
    const system = { environment: "system", label: "system", interpreters: [] };
    const probe = { selected: system, refresh, loading: false, error: false, report: { project_id: "project-1", connection_id: "lab", remote_root: "/work/project", requested_environment: "system", observed_at: "now", suggested_environment: null, truncated: false, environments: [system] } };
    renderDialog({ frozen: true, backend: sshBackend, boundary: { ...localBoundary, source: { kind: "frozen_run", run_id: "historical" }, execution_location: "ssh_host", compute_selection: { ...localBoundary.compute_selection, backend_id: "ssh:lab", backend_kind: "ssh" } }, sshEnvironments: probe, onEnvironmentChange });
    expect(screen.queryByRole("combobox", { name: "Discovered SSH environments" })).not.toBeInTheDocument();
    expect(screen.queryByRole("region", { name: "SSH interpreter detection" })).not.toBeInTheDocument();
    expect(onEnvironmentChange).not.toHaveBeenCalled(); expect(refresh).not.toHaveBeenCalled();
  });

  it.each(["error", "timeout"] as const)("reports an R interpreter %s without claiming that no interpreter exists", (status) => {
    const system = { environment: "system", label: "system", interpreters: [{ language: "r" as const, status }] };
    const probe = { selected: system, refresh: vi.fn(), loading: false, error: false, report: { project_id: "project-1", connection_id: "lab", remote_root: "/work/project", requested_environment: "system", observed_at: "now", suggested_environment: null, truncated: false, environments: [system] } };
    renderDialog({ backend: sshBackend, boundary: { ...localBoundary, execution_location: "ssh_host", compute_selection: { ...localBoundary.compute_selection, backend_id: "ssh:lab", backend_kind: "ssh" } }, sshEnvironments: probe, onEnvironmentChange: vi.fn() });
    const observations = screen.getByRole("region", { name: "SSH interpreter detection" });
    expect(observations).toHaveTextContent(status === "error" ? "Detection failed" : "Detection timed out");
    expect(observations).not.toHaveTextContent("Interpreter not found");
  });
  it("renders frozen per-language project environment paths and their exact identity without probing SSH", () => {
    const query = vi.spyOn(api, "probeSshProjectEnvironments");
    renderDialog({ frozen: true, backend: sshBackend, boundary: { ...localBoundary, source: { kind: "frozen_run", run_id: "historical" }, execution_location: "ssh_host", compute_selection: { ...localBoundary.compute_selection, backend_id: "ssh:lab", backend_kind: "ssh", environment: "envs:python311:r443" } } });
    const dialog = screen.getByRole("dialog", { name: "Runtimes" });
    expect(dialog).toHaveTextContent("Python .envs/python311");
    expect(dialog).toHaveTextContent("R .envs/r443");
    fireEvent.click(screen.getByText("Full compute identity"));
    expect(screen.getByText("envs:python311:r443")).toBeInTheDocument();
    expect(query).not.toHaveBeenCalled();
  });
  it("shows project interpreter versions only for the matching draft SSH environment", () => {
    const selected = { environment: "envs:python311:r443", label: "Python .envs/python311 · R .envs/r443", interpreters: [{ language: "python" as const, status: "available" as const, version: "Python 3.11.18" }] };
    const probe = { selected, refresh: vi.fn(), loading: false, error: false, report: { project_id: "project-1", connection_id: "lab", remote_root: "/work/project", requested_environment: selected.environment, observed_at: "now", suggested_environment: selected.environment, truncated: false, environments: [selected] } };
    const boundary = { ...localBoundary, compute_selection: { ...localBoundary.compute_selection, backend_id: "ssh:lab", backend_kind: "ssh" as const, environment: selected.environment }, execution_location: "ssh_host" as const };
    const view = renderDialog({ backend: sshBackend, boundary, sshEnvironments: probe });
    expect(screen.getByRole("region", { name: "SSH interpreter detection" })).toHaveTextContent("Python 3.11.18");
    view.rerender(<RuntimeDialog zh={false} language="python" environment="system" boundaryLoading={false} boundaryError="" onRetryBoundary={vi.fn()} onLanguageChange={vi.fn()} onClose={vi.fn()} backend={sshBackend} boundary={{ ...boundary, compute_selection: { ...boundary.compute_selection, environment: "system" } }} sshEnvironments={probe} />);
    expect(screen.queryByText("Python 3.11.18")).not.toBeInTheDocument();
  });
  it("keeps parent focus owned when a loading transition removes controls and traps Tab from body", () => {
    const { rerender } = renderDialog();
    const prepare = screen.getByRole("button", { name: "Ask Agent to prepare environment" }); prepare.focus();
    rerender(<RuntimeDialog zh={false} language="python" onLanguageChange={vi.fn()} environment="system" boundary={null} boundaryLoading boundaryError="" onRetryBoundary={vi.fn()} onClose={vi.fn()} />);
    const close = screen.getByRole("button", { name: "Close runtimes" }); expect(close).toHaveFocus();
    close.blur(); expect(document.activeElement).toBe(document.body); fireEvent.keyDown(window, { key: "Tab", shiftKey: true });
    expect(screen.getByRole("tab", { name: "R" })).toHaveFocus();
    fireEvent.keyDown(window, { key: "Tab" }); expect(close).toHaveFocus();
  });
  it("inspection_busy_is_not_ready and metadata is content-free with unknown sizes", () => {
    const refresh = vi.fn();
    const inspection = { project_id: "project-1", conversation_id: "c", run_id: "run-1", language: "python" as const, session_id: "s", status: "busy" as const, objects: [], truncated: false };
    const view = renderDialog({ kernelInspectionOpen: true, kernelInspection: inspection, onRefreshKernelInspection: refresh, onCloseKernelInspection: vi.fn() });
    const child = screen.getByRole("dialog", { name: "Kernel metadata" });
    expect(child).toHaveTextContent("Busy"); expect(child).not.toHaveTextContent("Ready");
    expect(refresh).not.toHaveBeenCalled(); fireEvent.click(screen.getByRole("button", { name: "Refresh metadata" })); expect(refresh).toHaveBeenCalledOnce();
    view.rerender(<RuntimeDialog zh={false} language="python" onLanguageChange={vi.fn()} environment="system" boundary={localBoundary} boundaryLoading={false} boundaryError="" onRetryBoundary={vi.fn()} onClose={vi.fn()} kernelInspectionOpen kernelInspection={{ ...inspection, status: "ready", objects: [{ name: "table", type_label: "opaque", dimensions: null, element_count: null, summary: "opaque" }], truncated: true }} onCloseKernelInspection={vi.fn()} onRefreshKernelInspection={refresh} />);
    expect(child).toHaveTextContent("Unknown"); expect(child).toHaveTextContent("Truncated"); expect(child).toHaveTextContent("Memory snapshot"); expect(child).toHaveTextContent("not research evidence");
    expect(screen.getByRole("columnheader", { name: "Dimensions" })).toBeVisible();
  });
  it.each(["not_started", "stale_session", "unavailable"] as const)("shows %s without old objects", (status) => {
    renderDialog({ kernelInspectionOpen: true, kernelInspection: { project_id: "p", conversation_id: "c", run_id: "run", language: "python", session_id: null, status, objects: [{ name: "old_session_object", type_label: "opaque", dimensions: null, element_count: null, summary: "opaque" }], truncated: false }, onCloseKernelInspection: vi.fn(), onRefreshKernelInspection: vi.fn() });
    expect(screen.queryByText("old_session_object")).not.toBeInTheDocument();
    expect(screen.getByRole("dialog", { name: "Kernel metadata" })).toHaveTextContent(status === "not_started" ? "Not started" : status === "stale_session" ? "Stale session" : "Unavailable");
  });
  it.each([false, true])("opens read-only remote jobs even when frozen=%s without preparing or changing language", (frozen) => {
    const onOpenRemoteJobs = vi.fn(); const onPrepare = vi.fn(); const onLanguageChange = vi.fn();
    renderDialog({ frozen, onOpenRemoteJobs, onPrepare, onLanguageChange });
    fireEvent.click(screen.getByRole("button", { name: "Remote jobs" }));
    expect(onOpenRemoteJobs).toHaveBeenCalledOnce();
    expect(onPrepare).not.toHaveBeenCalled(); expect(onLanguageChange).not.toHaveBeenCalled();
  });
  it("does not infer a probe and directs read-only metadata viewing through a run", () => {
    renderDialog({ backend: undefined });

    expect(screen.getByRole("dialog", { name: "Runtimes" })).toBeInTheDocument();
    expect(screen.queryByText("Available · interpreter found")).not.toBeInTheDocument();
    expect(screen.getByText("Local host process")).toBeInTheDocument();
    expect(screen.getByText(/Read-only observation of safe metadata/i)).toBeInTheDocument();
    expect(screen.getByText(/Open from a run record/i)).toBeInTheDocument();
    expect(screen.queryByText(/^Running$/i)).not.toBeInTheDocument();
    expect(screen.queryByText(/x\s*=\s*1/i)).not.toBeInTheDocument();
  });

  it("switches languages and delegates preparation for the selected language", () => {
    const onLanguageChange = vi.fn();
    const onPrepare = vi.fn();
    const { rerender } = renderDialog({ zh: true, onLanguageChange, onPrepare });

    fireEvent.click(screen.getByRole("tab", { name: /R.*未找到解释器/ }));
    expect(onLanguageChange).toHaveBeenCalledWith("r");

    rerender(
      <RuntimeDialog
        zh
        language="r"
        onLanguageChange={onLanguageChange}
        backend={localBackend}
        environment="system"
        onClose={vi.fn()}
        onPrepare={onPrepare}
        boundary={localBoundary} boundaryLoading={false} boundaryError="" onRetryBoundary={vi.fn()}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: "让 Agent 准备环境" }));
    expect(onPrepare).toHaveBeenCalledWith("r");
  });

  it("offers SSH configuration only for an SSH backend", () => {
    const onSettings = vi.fn();
    const sshBoundary: RuntimeBoundaryViewV4 = { ...localBoundary, execution_location: "ssh_host", detached_job_lifecycle: "ssh_linux_only", compute_selection: { ...localBoundary.compute_selection, backend_id: "ssh:lab", backend_kind: "ssh" } };
    const { rerender } = renderDialog({ backend: sshBackend, boundary: sshBoundary, onSettings });

    fireEvent.click(screen.getByRole("button", { name: "Configure SSH" }));
    expect(onSettings).toHaveBeenCalledTimes(1);

    rerender(
      <RuntimeDialog
        zh={false}
        language="python"
        onLanguageChange={vi.fn()}
        backend={localBackend}
        environment="system"
        onClose={vi.fn()}
        onSettings={onSettings}
        onPrepare={vi.fn()}
        boundary={localBoundary} boundaryLoading={false} boundaryError="" onRetryBoundary={vi.fn()}
      />,
    );
    expect(screen.queryByRole("button", { name: "Configure SSH" })).not.toBeInTheDocument();
  });

  it("describes local and SSH process limits without claiming dependency verification", () => {
    const { rerender } = renderDialog();
    expect(screen.getByRole("dialog")).toHaveTextContent("same user permissions");
    expect(screen.getByRole("dialog")).toHaveTextContent("dependencies have not been verified");
    rerender(<RuntimeDialog zh={false} language="python" onLanguageChange={vi.fn()} backend={sshBackend} environment="system" onClose={vi.fn()} boundary={{ ...localBoundary, execution_location: "ssh_host", detached_job_lifecycle: "ssh_linux_only", compute_selection: { ...localBoundary.compute_selection, backend_id: "ssh:lab", backend_kind: "ssh" } }} boundaryLoading={false} boundaryError="" onRetryBoundary={vi.fn()} />);
    expect(screen.getByRole("dialog")).toHaveTextContent("Linux SSH only");
    expect(screen.getByRole("dialog")).toHaveTextContent("Stop does not confirm remote termination");
    expect(screen.getByRole("dialog")).not.toHaveTextContent("image interpreter");
  });

  it("describes container limits and scopes network isolation to compute", () => {
    const boundary: RuntimeBoundaryViewV4 = { ...localBoundary, execution_location: "local_container", isolation: "container", limits: ["project_mount_read_write", "read_only_rootfs", "capabilities_dropped", "no_new_privileges", "pids_limit256", "shared_kernel_not_vm"], compute_selection: { ...localBoundary.compute_selection, backend_id: "docker", backend_kind: "docker", network_policy: "none", container_image: { reference: "python:3.12", image_id: "sha256:fixed" } } };
    renderDialog({ boundary, backend: { ...localBackend, descriptor: { ...localBackend.descriptor, backend_id: "docker", kind: "docker", isolation: "container" }, python_status: "unverified" } });
    expect(screen.getByRole("dialog")).toHaveTextContent("project mount is read-write");
    expect(screen.getByRole("dialog")).toHaveTextContent("model and MCP services may still use network");
    expect(screen.getByRole("dialog")).toHaveTextContent("shared kernel");
  });

  it.each([
    { name: "Local English", zh: false, kind: "local" as const, expected: ["Local host process", "same user permissions", "project working directory does not restrict file access", "Host network is inherited", "cannot reconnect after app restart", "Detached jobs are unsupported"] },
    { name: "Local Chinese", zh: true, kind: "local" as const, expected: ["本机进程", "与当前用户相同的权限", "项目工作目录不限制文件访问", "沿用宿主网络", "应用重启后无法重连", "不支持断线后重连"] },
    { name: "SSH English", zh: false, kind: "ssh" as const, expected: ["SSH host process", "same user permissions", "Host network is inherited", "Linux SSH only", "Stop does not confirm remote termination"] },
    { name: "SSH Chinese", zh: true, kind: "ssh" as const, expected: ["SSH 主机进程", "与当前用户相同的权限", "沿用宿主网络", "仅支持 Linux SSH", "Stop 不确认远端终止"] },
    { name: "Docker English", zh: false, kind: "docker" as const, expected: ["Local compute container", "project mount is read-write", "read-only container root filesystem", "shared kernel, not a virtual machine", "model and MCP services may still use network", "No CPU or memory quota", "Detached jobs are unsupported"] },
    { name: "Docker Chinese", zh: true, kind: "docker" as const, expected: ["本机计算容器", "项目挂载可读写", "容器根文件系统只读", "与宿主共享内核", "模型和 MCP 服务仍可能使用网络", "未设置 CPU 或内存配额", "不支持断线后重连"] },
    { name: "Podman English", zh: false, kind: "podman" as const, expected: ["Local compute container", "project mount is read-write", "read-only container root filesystem", "shared kernel, not a virtual machine", "model and MCP services may still use network", "No CPU or memory quota", "Detached jobs are unsupported"] },
    { name: "Podman Chinese", zh: true, kind: "podman" as const, expected: ["本机计算容器", "项目挂载可读写", "容器根文件系统只读", "与宿主共享内核", "模型和 MCP 服务仍可能使用网络", "未设置 CPU 或内存配额", "不支持断线后重连"] },
  ])("shows $name host limits, network scope and recovery semantics", ({ zh, kind, expected }) => {
    const container = kind === "docker" || kind === "podman";
    const boundary: RuntimeBoundaryViewV4 = {
      ...localBoundary,
      compute_selection: {
        ...localBoundary.compute_selection,
        backend_id: kind === "ssh" ? "ssh:lab" : kind,
        backend_kind: kind,
        environment: kind === "ssh" ? "micromamba-lab" : "system",
        network_policy: container ? "none" : "host_inherited",
        container_image: container ? { reference: "python:3.12", image_id: "sha256:fixed" } : null,
      },
      execution_location: kind === "ssh" ? "ssh_host" : container ? "local_container" : "local_host",
      isolation: container ? "container" : "process",
      limits: container
        ? ["project_mount_read_write", "read_only_rootfs", "capabilities_dropped", "no_new_privileges", "pids_limit256", "shared_kernel_not_vm"]
        : ["same_user_permissions", "project_cwd_not_access_control"],
      detached_job_lifecycle: kind === "ssh" ? "ssh_linux_only" : "unsupported",
    };
    const backend: ComputeBackendAvailabilityV4 = {
      ...localBackend,
      descriptor: { ...localBackend.descriptor, backend_id: boundary.compute_selection.backend_id, kind, isolation: container ? "container" : "process" },
      python_status: container ? "unverified" : "available",
    };
    renderDialog({ zh, boundary, backend });
    const dialog = screen.getByRole("dialog");
    for (const phrase of expected) expect(dialog).toHaveTextContent(phrase);
    if (kind === "ssh") expect(dialog).toHaveTextContent("micromamba-lab");
  });

  it("keeps frozen history separate from current probes and preparation", () => {
    const onPrepare = vi.fn();
    renderDialog({ boundary: { ...localBoundary, source: { kind: "frozen_run", run_id: "run-1" } }, backend: localBackend, onPrepare });
    expect(screen.getByRole("dialog")).toHaveTextContent("Recorded run");
    expect(screen.getByRole("dialog")).toHaveTextContent("run-1");
    expect(screen.getByRole("dialog")).not.toHaveTextContent("Recent probe");
    expect(screen.queryByRole("button", { name: "Ask Agent to prepare environment" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Configure SSH" })).not.toBeInTheDocument();
    expect(onPrepare).not.toHaveBeenCalled();
  });

  it("distinguishes the new local deletion policy from a frozen historical risk policy", () => {
    const { rerender } = renderDialog({ zh: true, boundary: { ...localBoundary, compute_selection: { ...localBoundary.compute_selection, approval_policy: "auto_approve_except_local_deletion" } } });
    expect(screen.getByRole("dialog")).toHaveTextContent("自动批准操作，仅检测到本地删除时询问");
    rerender(<RuntimeDialog zh language="python" onLanguageChange={vi.fn()} environment="system" onClose={vi.fn()}
      boundary={{ ...localBoundary, source: { kind: "frozen_run", run_id: "historical" }, compute_selection: { ...localBoundary.compute_selection, approval_policy: "risk_based" } }}
      boundaryLoading={false} boundaryError="" onRetryBoundary={vi.fn()} />);
    expect(screen.getByRole("dialog")).toHaveTextContent("基于风险的审批");
    expect(screen.getByRole("dialog")).not.toHaveTextContent("自动批准操作，仅检测到本地删除时询问");
  });

  it("shows incomplete, loading and retryable errors without making boundary claims", () => {
    const onRetryBoundary = vi.fn();
    const { rerender } = renderDialog({ boundary: null, onRetryBoundary });
    expect(screen.getByRole("dialog")).toHaveTextContent("Configuration incomplete");
    expect(screen.getByRole("dialog")).not.toHaveTextContent("same user permissions");
    rerender(<RuntimeDialog zh={false} language="python" onLanguageChange={vi.fn()} environment="system" onClose={vi.fn()} boundary={null} boundaryLoading boundaryError="" onRetryBoundary={onRetryBoundary} />);
    expect(screen.getByRole("dialog")).toHaveTextContent("Loading runtime boundary");
    rerender(<RuntimeDialog zh={false} language="python" onLanguageChange={vi.fn()} environment="system" onClose={vi.fn()} boundary={null} boundaryLoading={false} boundaryError="boundary missing" onRetryBoundary={onRetryBoundary} />);
    fireEvent.click(screen.getByRole("button", { name: "Retry boundary" }));
    expect(onRetryBoundary).toHaveBeenCalledOnce();
  });
});
