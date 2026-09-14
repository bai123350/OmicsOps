import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { WorkspaceShell } from "./WorkspaceShell";
import * as attachmentApi from "../../composer-attachment-api";
import type { ComposerAttachmentReceipt, ComputeBackendAvailabilityV4 } from "../../types";

afterEach(() => vi.restoreAllMocks());
const project = { id: "attachment-project", name: "Attachment project", status: "ready" as const, template: "blank" as const };
const backend: ComputeBackendAvailabilityV4 = { descriptor: { schema_version: 4, backend_id: "local", kind: "local", isolation: "process", available: true, supports_python: true, supports_r: false, supports_network_policy: false }, selectable: true, reason: null, python_status: "available", r_status: "unavailable", resolved_image_id: null };
const receipt: ComposerAttachmentReceipt = { id: "attachment-1", project_id: project.id, conversation_id: "conversation", name: "plot.png", relative_path: ".omicsops/attachments/attachment-1/bytes.png", size_bytes: 4, sha256: "a".repeat(64), media_type: "image/png" };
const props = { project, activeConversationId: "conversation", locale: "en-US" as const, onLocaleChange: vi.fn(), computeBackends: [backend], computeBackendId: "local" };

describe("composer attachment integration", () => {
  it("blocks send while image paste uploads and retains files and text after a rejected send", async () => {
    let complete!: (value: ComposerAttachmentReceipt) => void;
    vi.spyOn(attachmentApi, "stageComposerAttachment").mockReturnValue(new Promise((resolve) => { complete = resolve; }));
    const onSend = vi.fn().mockResolvedValueOnce(false).mockResolvedValueOnce(true);
    render(<WorkspaceShell {...props} onSend={onSend} />);
    const input = screen.getByRole("textbox", { name: /Describe a research goal/ });
    fireEvent.change(input, { target: { value: "Inspect this plot" } });
    fireEvent.paste(input, { clipboardData: { files: [new File(["data"], "plot.png", { type: "image/png" })] } });
    expect(screen.getByRole("button", { name: "Send" })).toBeDisabled();
    await act(async () => complete(receipt));
    await waitFor(() => expect(screen.getByRole("button", { name: "Send" })).toBeEnabled());
    fireEvent.click(screen.getByRole("button", { name: "Send" }));
    await waitFor(() => expect(onSend).toHaveBeenCalledWith("Inspect this plot", "chat", [], [receipt.id]));
    await waitFor(() => expect(screen.getByRole("button", { name: "Send" })).toBeEnabled());
    expect(input).toHaveValue("Inspect this plot");
    expect(screen.getByText("plot.png")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Send" }));
    await waitFor(() => expect(input).toHaveValue(""));
    expect(screen.queryByText("plot.png")).not.toBeInTheDocument();
  });

  it("uses the native attachment picker from Add files and /upload without uploading to SSH", async () => {
    const choose = vi.spyOn(attachmentApi, "chooseComposerAttachments").mockResolvedValue([]);
    const remoteUpload = vi.fn();
    render(<WorkspaceShell {...props} onUploadFiles={remoteUpload} />);
    fireEvent.click(screen.getByRole("button", { name: "Add context or choose mode" }));
    fireEvent.click(screen.getByRole("menuitem", { name: /Attach files/ }));
    await waitFor(() => expect(choose).toHaveBeenCalledTimes(1));
    const input = screen.getByRole("textbox", { name: /Describe a research goal/ });
    fireEvent.change(input, { target: { value: "/upload" } });
    fireEvent.click(screen.getByRole("button", { name: "Send" }));
    await waitFor(() => expect(choose).toHaveBeenCalledTimes(2));
    expect(remoteUpload).not.toHaveBeenCalled();
  });

  it("accepts a dropped file without a typed goal and leaves ordinary text paste alone", async () => {
    const stage = vi.spyOn(attachmentApi, "stageComposerAttachment").mockResolvedValue(receipt);
    const onSend = vi.fn().mockResolvedValue(true);
    render(<WorkspaceShell {...props} onSend={onSend} />);
    const input = screen.getByRole("textbox", { name: /Describe a research goal/ });
    fireEvent.paste(input, { clipboardData: { files: [], getData: () => "ordinary text" } });
    expect(stage).not.toHaveBeenCalled();
    fireEvent.drop(input, { dataTransfer: { files: [new File(["data"], "plot.png", { type: "image/png" })], types: ["Files"] } });
    await waitFor(() => expect(screen.getByRole("button", { name: "Send" })).toBeEnabled());
    fireEvent.click(screen.getByRole("button", { name: "Send" }));
    await waitFor(() => expect(onSend).toHaveBeenCalledWith("Please inspect the attached files.", "chat", [], [receipt.id]));
  });
});
