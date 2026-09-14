import { fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import * as api from "../../tauri-api";
import { GuidanceDialog } from "./GuidanceDialog";

vi.mock("../../tauri-api", () => ({
  agentV4ListGuidance: vi.fn(),
  agentV4SubmitGuidance: vi.fn(),
  onAgentV4Event: vi.fn(),
}));

const props = {
  runId: "run",
  projectId: "project",
  conversationId: "conversation",
  enabled: true,
  locale: "en-US" as const,
  onClose: vi.fn(),
};

beforeEach(() => {
  vi.mocked(api.agentV4ListGuidance).mockReset().mockResolvedValue([]);
  vi.mocked(api.agentV4SubmitGuidance).mockReset();
  vi.mocked(api.onAgentV4Event).mockReset().mockResolvedValue(vi.fn());
  props.onClose.mockReset();
});

describe("GuidanceDialog", () => {
  it("focuses the dialog and restores the launch control when it unmounts", () => {
    const launch = document.createElement("button");
    launch.type = "button";
    launch.textContent = "Launch guidance";
    document.body.append(launch);
    launch.focus();

    const view = render(<GuidanceDialog {...props} />);
    expect(screen.getByRole("button", { name: "Close guidance" })).toHaveFocus();
    view.unmount();
    expect(launch).toHaveFocus();
    launch.remove();
  });

  it("closes on an immediate window Escape even when focus is outside the dialog", () => {
    const outside = document.createElement("button");
    outside.type = "button";
    document.body.append(outside);
    outside.focus();
    render(<GuidanceDialog {...props} />);

    fireEvent.keyDown(window, { key: "Escape", code: "Escape" });

    expect(props.onClose).toHaveBeenCalledTimes(1);
    outside.remove();
  });

  it("wraps Tab focus within the dialog", () => {
    render(<GuidanceDialog {...props} />);
    const dialog = screen.getByRole("dialog", { name: "Add guidance" });
    const close = screen.getByRole("button", { name: "Close guidance" });
    const textarea = screen.getByLabelText("Additional guidance");

    close.focus();
    fireEvent.keyDown(dialog, { key: "Tab" });
    expect(textarea).toHaveFocus();
    fireEvent.keyDown(dialog, { key: "Tab", shiftKey: true });
    expect(close).toHaveFocus();
  });

  it("does not carry a composer draft containing attachments or references", () => {
    render(<GuidanceDialog {...props} initialDraft="message with files" composerHasAttachments />);

    expect(screen.getByLabelText("Additional guidance")).toHaveValue("");
    expect(screen.getByRole("note")).toHaveTextContent("attachments");
  });

  it("keeps the history read-only when the run is waiting", () => {
    render(<GuidanceDialog {...props} enabled={false} />);

    expect(screen.queryByLabelText("Additional guidance")).not.toBeInTheDocument();
    expect(screen.getByText("The run is not accepting new guidance right now."))
      .toBeInTheDocument();
  });
});
