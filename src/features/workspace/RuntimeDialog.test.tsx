import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import type { ComputeBackendAvailabilityV4, RuntimeBoundaryViewV4 } from "../../types";
import { RuntimeDialog } from "./RuntimeDialog";

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
  it("does not infer a probe when none is provided and states that variable inspection is unavailable", () => {
    renderDialog({ backend: undefined });

    expect(screen.getByRole("dialog", { name: "Runtimes" })).toBeInTheDocument();
    expect(screen.queryByText("Available · interpreter found")).not.toBeInTheDocument();
    expect(screen.getByText("Local host process")).toBeInTheDocument();
    expect(screen.getByText("Not available", { exact: true })).toBeInTheDocument();
    expect(screen.getByText(/Variable inspection is not available here yet/i)).toBeInTheDocument();
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
