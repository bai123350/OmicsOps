import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import type { ComputeBackendAvailabilityV4 } from "../../types";
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
      {...overrides}
    />,
  );
}

describe("RuntimeDialog", () => {
  it("shows interpreter availability while stating that variable inspection is unavailable", () => {
    renderDialog({ backend: undefined });

    expect(screen.getByRole("dialog", { name: "Runtimes" })).toBeInTheDocument();
    expect(screen.getAllByText("Not detected").length).toBeGreaterThan(0);
    expect(screen.getByText("Persistent per run")).toBeInTheDocument();
    expect(screen.getByText("Not available", { exact: true })).toBeInTheDocument();
    expect(screen.getByText(/Variable inspection is not available here yet/i)).toBeInTheDocument();
    expect(screen.queryByText(/^Running$/i)).not.toBeInTheDocument();
    expect(screen.queryByText(/x\s*=\s*1/i)).not.toBeInTheDocument();
  });

  it("switches languages and delegates preparation for the selected language", () => {
    const onLanguageChange = vi.fn();
    const onPrepare = vi.fn();
    const { rerender } = renderDialog({ zh: true, onLanguageChange, onPrepare });

    fireEvent.click(screen.getByRole("tab", { name: /R.*不可用/ }));
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
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: "让 Agent 准备环境" }));
    expect(onPrepare).toHaveBeenCalledWith("r");
  });

  it("offers SSH configuration only for an SSH backend", () => {
    const onSettings = vi.fn();
    const { rerender } = renderDialog({ backend: sshBackend, onSettings });

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
      />,
    );
    expect(screen.queryByRole("button", { name: "Configure SSH" })).not.toBeInTheDocument();
  });
});
