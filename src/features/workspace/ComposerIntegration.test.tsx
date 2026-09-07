import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import type { ComputeBackendAvailabilityV4 } from "../../types";
import { WorkspaceShell } from "./WorkspaceShell";

const project = {
  id: "project-composer",
  name: "Composer test project",
  status: "ready" as const,
  template: "blank" as const,
};

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

function renderShell(overrides: Partial<React.ComponentProps<typeof WorkspaceShell>> = {}) {
  return render(
    <WorkspaceShell
      project={project}
      locale="en-US"
      onLocaleChange={vi.fn()}
      computeBackends={[localBackend]}
      computeBackendId="local"
      {...overrides}
    />,
  );
}

describe("WorkspaceShell composer integration", () => {
  it("closes the plus menu immediately on Escape", () => {
    renderShell();

    const plus = screen.getByRole("button", { name: "Add context or choose mode" });
    fireEvent.click(plus);
    expect(screen.getByRole("menu", { name: "Compose actions" })).toBeInTheDocument();

    fireEvent.keyDown(window, { key: "Escape" });

    expect(screen.queryByRole("menu", { name: "Compose actions" })).not.toBeInTheDocument();
    expect(plus).toHaveAttribute("aria-expanded", "false");
  });

  it("closes nested compute before its orbit permissions parent", () => {
    renderShell();

    fireEvent.click(screen.getByRole("button", { name: "Agent permissions" }));
    const permissions = screen.getByRole("menu", { name: "Agent permission options" });
    fireEvent.click(screen.getByRole("menuitem", { name: /^Compute/ }));
    expect(screen.getByRole("region", { name: "V4 compute backend" })).toBeInTheDocument();

    fireEvent.keyDown(window, { key: "Escape" });
    expect(screen.queryByRole("region", { name: "V4 compute backend" })).not.toBeInTheDocument();
    expect(permissions).toBeInTheDocument();

    fireEvent.keyDown(window, { key: "Escape" });
    expect(screen.queryByRole("menu", { name: "Agent permission options" })).not.toBeInTheDocument();
  });

  it("reports truthful Python and R availability and closes runtime dialogs on Escape", () => {
    renderShell();

    const python = screen.getByRole("button", { name: "Python environment" });
    const r = screen.getByRole("button", { name: "R environment" });
    expect(python).toHaveTextContent("AVAILABLE");
    expect(r).toHaveTextContent("UNAVAILABLE");

    fireEvent.click(python);
    expect(screen.getByRole("dialog", { name: "Runtimes" })).toHaveTextContent("Available");
    fireEvent.keyDown(window, { key: "Escape" });
    expect(screen.queryByRole("dialog", { name: "Runtimes" })).not.toBeInTheDocument();

    fireEvent.click(r);
    expect(screen.getByRole("dialog", { name: "Runtimes" })).toHaveTextContent("Unavailable");
    fireEvent.keyDown(window, { key: "Escape" });
    expect(screen.queryByRole("dialog", { name: "Runtimes" })).not.toBeInTheDocument();
  });

  it("appends the prepare-environment request to the draft without sending it", () => {
    const onSend = vi.fn();
    renderShell({ onSend });

    fireEvent.click(screen.getByRole("button", { name: "Python environment" }));
    fireEvent.click(screen.getByRole("button", { name: "Ask Agent to prepare environment" }));

    expect(onSend).not.toHaveBeenCalled();
    expect((screen.getByRole("textbox", { name: /Describe a research goal/ }) as HTMLTextAreaElement).value).toContain("Check the Python interpreter");
    expect(screen.queryByRole("dialog", { name: "Runtimes" })).not.toBeInTheDocument();
  });

  it("routes model selection to onModelChange and closes the model menu", () => {
    const onModelChange = vi.fn();
    renderShell({
      modelLabel: "Current model",
      modelOptions: [
        { id: "model-a", label: "Model A" },
        { id: "model-b", label: "Model B" },
      ],
      modelId: "model-a",
      onModelChange,
    });

    const modelButton = screen.getByRole("button", { name: "Choose model" });
    fireEvent.click(modelButton);
    fireEvent.click(screen.getByRole("menuitemradio", { name: "Model B" }));
    expect(onModelChange).toHaveBeenCalledWith("model-b");
    expect(screen.queryByRole("menu")).not.toBeInTheDocument();

    fireEvent.click(modelButton);
    expect(screen.getByRole("menu")).toBeInTheDocument();
    fireEvent.keyDown(window, { key: "Escape" });
    expect(screen.queryByRole("menu")).not.toBeInTheDocument();
  });

  it("routes plan mode through the durable callback and disables mode changes while locked", () => {
    const onAgentModeChange = vi.fn();
    const view = renderShell({ agentMode: "agent", onAgentModeChange });

    fireEvent.click(screen.getByRole("button", { name: "Agent permissions" }));
    const orbitPlan = screen.getByRole("menuitemcheckbox", { name: "Plan first" });
    expect(orbitPlan).toBeEnabled();
    fireEvent.click(orbitPlan);
    expect(onAgentModeChange).toHaveBeenCalledWith("plan");

    fireEvent.click(screen.getByRole("button", { name: "Send options" }));
    const sendPlan = screen.getByRole("menuitemradio", { name: "Plan first" });
    expect(sendPlan).toBeEnabled();
    fireEvent.click(sendPlan);
    expect(onAgentModeChange).toHaveBeenLastCalledWith("plan");
    expect(screen.queryByRole("menu")).not.toBeInTheDocument();

    view.rerender(
      <WorkspaceShell
        project={project}
        locale="en-US"
        onLocaleChange={vi.fn()}
        computeBackends={[localBackend]}
        computeBackendId="local"
        agentMode="agent"
        conversationLocked
        onAgentModeChange={onAgentModeChange}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: "Agent permissions" }));
    expect(screen.getByRole("menuitemcheckbox", { name: "Plan first" })).toBeDisabled();
    fireEvent.click(screen.getByRole("button", { name: "Send options" }));
    expect(screen.getByRole("menuitemradio", { name: "Plan first" })).toBeDisabled();
  });

  it("opens Skills settings from the compose menu", () => {
    const onOpenSettings = vi.fn();
    renderShell({ onOpenSettings });

    const plus = screen.getByRole("button", { name: "Add context or choose mode" });
    fireEvent.click(plus);
    fireEvent.click(screen.getByRole("menuitem", { name: /Manage skills/ }));

    expect(onOpenSettings).toHaveBeenCalledWith("skills");
    expect(plus).toHaveAttribute("aria-expanded", "false");
  });
});
