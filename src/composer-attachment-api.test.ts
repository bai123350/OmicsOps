import { afterEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";

import {
  COMPOSER_ATTACHMENT_BROWSER_ERROR,
  MAX_COMPOSER_ATTACHMENT_BYTES,
  chooseComposerAttachments,
  stageComposerAttachment,
  validateComposerAttachments,
} from "./composer-attachment-api";
import type { ComposerAttachmentReceipt } from "./types";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

const receipt: ComposerAttachmentReceipt = {
  id: "attachment-1",
  project_id: "project-1",
  conversation_id: "conversation-1",
  name: "counts.csv",
  relative_path: ".omicsops/attachments/attachment-1/counts.csv",
  size_bytes: 5,
  sha256: "hash",
  media_type: "text/csv",
};

function enableTauri() {
  Object.defineProperty(window, "__TAURI_INTERNALS__", { value: {}, configurable: true });
}

afterEach(() => {
  Reflect.deleteProperty(window, "__TAURI_INTERNALS__");
  vi.clearAllMocks();
});

describe("chooseComposerAttachments", () => {
  it("returns no invented receipts in browser preview", async () => {
    await expect(chooseComposerAttachments("project-1", "conversation-1")).resolves.toEqual([]);
    expect(invoke).not.toHaveBeenCalled();
  });

  it("delegates the native chooser with the active scope", async () => {
    enableTauri();
    vi.mocked(invoke).mockResolvedValueOnce([receipt]);

    await expect(chooseComposerAttachments("project-1", "conversation-1")).resolves.toEqual([receipt]);
    expect(invoke).toHaveBeenCalledWith("choose_composer_attachments", {
      projectId: "project-1",
      conversationId: "conversation-1",
    });
  });
});

describe("stageComposerAttachment", () => {
  it("rejects browser staging with an explicit unavailable error", async () => {
    const file = new File(["hello"], "counts.csv", { type: "text/csv" });

    await expect(stageComposerAttachment("project-1", "conversation-1", file)).rejects.toThrow(COMPOSER_ATTACHMENT_BROWSER_ERROR);
    expect(invoke).not.toHaveBeenCalled();
  });

  it("reads bytes as base64 and sends the host-owned staging request", async () => {
    enableTauri();
    vi.mocked(invoke).mockResolvedValueOnce(receipt);
    const file = new File(["hello"], "counts.csv", { type: "text/csv" });

    await expect(stageComposerAttachment("project-1", "conversation-1", file)).resolves.toEqual(receipt);
    expect(invoke).toHaveBeenCalledWith("stage_composer_attachment", {
      request: {
        project_id: "project-1",
        conversation_id: "conversation-1",
        name: "counts.csv",
        content_base64: "aGVsbG8=",
      },
    });
  });

  it("rejects an oversized file before reading it", async () => {
    enableTauri();
    const file = new File(["hello"], "large.bin", { type: "application/octet-stream" });
    Object.defineProperty(file, "size", { configurable: true, value: MAX_COMPOSER_ATTACHMENT_BYTES + 1 });
    const readAsDataUrl = vi.spyOn(FileReader.prototype, "readAsDataURL");

    await expect(stageComposerAttachment("project-1", "conversation-1", file)).rejects.toThrow(/20 MiB/);
    expect(readAsDataUrl).not.toHaveBeenCalled();
    expect(invoke).not.toHaveBeenCalled();
  });

  it("turns a reader failure into a safe retryable error", async () => {
    enableTauri();
    const file = new File(["hello"], "counts.csv", { type: "text/csv" });
    const readAsDataUrl = vi.spyOn(FileReader.prototype, "readAsDataURL").mockImplementation(function mockRead(this: FileReader) {
      queueMicrotask(() => this.onerror?.(new ProgressEvent("error") as ProgressEvent<FileReader>));
    });

    await expect(stageComposerAttachment("project-1", "conversation-1", file)).rejects.toThrow("Could not read attachment bytes.");
    expect(invoke).not.toHaveBeenCalled();
    readAsDataUrl.mockRestore();
  });
});

it("passes the remaining picker allowance to the host", async () => {
  enableTauri();
  vi.mocked(invoke).mockResolvedValueOnce([]);
  await chooseComposerAttachments("project-1", "conversation-1", { maxFiles: 1, maxBytes: 1024 });
  expect(invoke).toHaveBeenCalledWith("choose_composer_attachments", {
    projectId: "project-1", conversationId: "conversation-1", maxFiles: 1, maxBytes: 1024,
  });
});

describe("validateComposerAttachments", () => {
  it("is a no-op in browser preview", async () => {
    await expect(validateComposerAttachments("project-1", "conversation-1", [receipt.id])).resolves.toBeUndefined();
    expect(invoke).not.toHaveBeenCalled();
  });

  it("asks the host to validate the selected receipt IDs", async () => {
    enableTauri();
    vi.mocked(invoke).mockResolvedValueOnce(undefined);

    await expect(validateComposerAttachments("project-1", "conversation-1", [receipt.id])).resolves.toBeUndefined();
    expect(invoke).toHaveBeenCalledWith("validate_composer_attachments", {
      projectId: "project-1",
      conversationId: "conversation-1",
      attachments: [receipt.id],
    });
  });
});
