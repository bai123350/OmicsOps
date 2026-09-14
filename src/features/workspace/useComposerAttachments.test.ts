import { act, renderHook, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import {
  MAX_COMPOSER_ATTACHMENT_BYTES,
  MAX_COMPOSER_ATTACHMENT_TOTAL_BYTES,
  chooseComposerAttachments,
  stageComposerAttachment,
} from "../../composer-attachment-api";
import type { ComposerAttachmentReceipt } from "../../types";
import { useComposerAttachments } from "./useComposerAttachments";

vi.mock("../../composer-attachment-api", () => ({
  chooseComposerAttachments: vi.fn(),
  stageComposerAttachment: vi.fn(),
  COMPOSER_ATTACHMENT_BROWSER_ERROR: "File attachments are unavailable in browser preview.",
  MAX_COMPOSER_ATTACHMENT_BYTES: 20 * 1024 * 1024,
  MAX_COMPOSER_ATTACHMENTS: 8,
  MAX_COMPOSER_ATTACHMENT_TOTAL_BYTES: 40 * 1024 * 1024,
}));

const firstReceipt: ComposerAttachmentReceipt = {
  id: "attachment-1",
  project_id: "project-1",
  conversation_id: "conversation-1",
  name: "counts.csv",
  relative_path: ".omicsops/attachments/attachment-1/counts.csv",
  size_bytes: 5,
  sha256: "hash-1",
  media_type: "text/csv",
};

const secondReceipt: ComposerAttachmentReceipt = {
  ...firstReceipt,
  id: "attachment-2",
  name: "markers.csv",
  relative_path: ".omicsops/attachments/attachment-2/markers.csv",
  sha256: "hash-2",
};

function file(name: string, size = 5, type = "text/csv") {
  const value = new File(["bytes"], name, { type });
  Object.defineProperty(value, "size", { configurable: true, value: size });
  return value;
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise;
    reject = rejectPromise;
  });
  return { promise, resolve, reject };
}

afterEach(() => {
  vi.mocked(chooseComposerAttachments).mockReset();
  vi.mocked(stageComposerAttachment).mockReset();
});

describe("useComposerAttachments", () => {
  it("stages attachments under the app's StrictMode lifecycle", async () => {
    vi.mocked(stageComposerAttachment).mockResolvedValue(firstReceipt);
    const { result } = renderHook(() => useComposerAttachments("project-1", "conversation-1"), { reactStrictMode: true });
    await act(async () => result.current.addFiles([file("counts.csv")]));
    expect(result.current.receipts).toEqual([firstReceipt]);
  });

  it("accepts a second drop while the first upload is pending", async () => {
    const first = deferred<ComposerAttachmentReceipt>();
    vi.mocked(stageComposerAttachment).mockReturnValueOnce(first.promise).mockResolvedValueOnce(secondReceipt);
    const { result } = renderHook(() => useComposerAttachments("project-1", "conversation-1"));
    act(() => { void result.current.addFiles([file("counts.csv")]); });
    await act(async () => result.current.addFiles([file("markers.csv")]));
    expect(result.current.items).toHaveLength(2);
    await act(async () => first.resolve(firstReceipt));
    expect(result.current.receipts).toHaveLength(2);
  });

  it("stages dropped files and exposes ready receipts", async () => {
    vi.mocked(stageComposerAttachment).mockResolvedValueOnce(firstReceipt);
    const { result } = renderHook(() => useComposerAttachments("project-1", "conversation-1"));
    const dropped = file("counts.csv");

    await act(async () => {
      await result.current.addFiles([dropped]);
    });

    expect(stageComposerAttachment).toHaveBeenCalledWith("project-1", "conversation-1", dropped);
    expect(result.current.items).toHaveLength(1);
    expect(result.current.items[0]).toMatchObject({
      key: expect.any(String),
      clientKey: expect.any(String),
      name: "counts.csv",
      status: "ready",
      receipt: firstReceipt,
    });
    expect(result.current.receipts).toEqual([firstReceipt]);
    expect(result.current.busy).toBe(false);
  });

  it("keeps the failed file for retry and does not expose the rejection internals", async () => {
    vi.mocked(stageComposerAttachment)
      .mockRejectedValueOnce(new Error("private native path detail"))
      .mockResolvedValueOnce(firstReceipt);
    const { result } = renderHook(() => useComposerAttachments("project-1", "conversation-1"));
    const dropped = file("counts.csv");

    await act(async () => {
      await result.current.addFiles([dropped]);
    });
    expect(result.current.items[0]).toMatchObject({ status: "error" });
    expect(result.current.items[0].error).toMatch(/upload|retry|failed/i);
    expect(result.current.items[0].error).not.toContain("private native path detail");
    const key = result.current.items[0].key;

    await act(async () => {
      await result.current.retry(key);
    });

    expect(stageComposerAttachment).toHaveBeenNthCalledWith(2, "project-1", "conversation-1", dropped);
    expect(result.current.items[0]).toMatchObject({ status: "ready", receipt: firstReceipt });
  });

  it("rejects per-file and message-size limits before staging", async () => {
    vi.mocked(stageComposerAttachment).mockResolvedValue(firstReceipt);
    const { result } = renderHook(() => useComposerAttachments("project-1", "conversation-1"));
    const oversized = file("large.bin", MAX_COMPOSER_ATTACHMENT_BYTES + 1, "application/octet-stream");

    await act(async () => {
      await result.current.addFiles([oversized]);
    });
    expect(stageComposerAttachment).not.toHaveBeenCalled();
    expect(result.current.items[0]).toMatchObject({ status: "error", name: "large.bin" });
    expect(result.current.items[0].error).toMatch(/20 MiB/);

    const next = renderHook(() => useComposerAttachments("project-1", "conversation-1"));
    const belowLimit = file("first.bin", MAX_COMPOSER_ATTACHMENT_TOTAL_BYTES / 2, "application/octet-stream");
    const atLimit = file("second.bin", MAX_COMPOSER_ATTACHMENT_TOTAL_BYTES / 2, "application/octet-stream");
    const overTotal = file("third.bin", 1, "application/octet-stream");
    await act(async () => {
      await next.result.current.addFiles([belowLimit, atLimit, overTotal]);
    });
    expect(stageComposerAttachment).toHaveBeenCalledTimes(2);
    expect(next.result.current.items).toHaveLength(3);
    expect(next.result.current.items.map((item) => item.status)).toEqual(["ready", "ready", "error"]);
  });

  it("enforces the eight-item cap and ignores duplicate selections", async () => {
    vi.mocked(stageComposerAttachment).mockImplementation(async (_project, _conversation, dropped) => ({
      ...firstReceipt,
      id: `receipt-${dropped.name}`,
      name: dropped.name,
      size_bytes: dropped.size,
    }));
    const { result } = renderHook(() => useComposerAttachments("project-1", "conversation-1"));
    const files = Array.from({ length: 9 }, (_, index) => file(`file-${index}.csv`));

    await act(async () => {
      await result.current.addFiles([...files, files[0]]);
    });

    expect(result.current.items).toHaveLength(8);
    expect(stageComposerAttachment).toHaveBeenCalledTimes(8);
    expect(result.current.limitReached).toBe(true);
    expect(new Set(result.current.items.map((item) => item.name)).size).toBe(8);

    act(() => result.current.clearAccepted(result.current.receipts.map((receipt) => receipt.id)));
    expect(result.current.limitReached).toBe(false);
  });

  it("does not bypass the per-file limit through Retry", async () => {
    const { result } = renderHook(() => useComposerAttachments("project-1", "conversation-1"));
    await act(async () => result.current.addFiles([file("huge.csv", MAX_COMPOSER_ATTACHMENT_BYTES + 1)]));
    const rejected = result.current.items[0];
    await act(async () => result.current.retry(rejected.key));
    expect(stageComposerAttachment).not.toHaveBeenCalled();
    expect(result.current.items[0].status).toBe("error");
  });

  it("ignores late completion after removal", async () => {
    const pending = deferred<ComposerAttachmentReceipt>();
    vi.mocked(stageComposerAttachment).mockReturnValueOnce(pending.promise);
    const { result } = renderHook(() => useComposerAttachments("project-1", "conversation-1"));
    const dropped = file("counts.csv");
    let addPromise!: Promise<void>;
    act(() => { addPromise = result.current.addFiles([dropped]); });
    await waitFor(() => expect(result.current.items[0]?.status).toBe("uploading"));
    const key = result.current.items[0].key;

    act(() => result.current.remove(key));
    pending.resolve(firstReceipt);
    await addPromise;

    expect(result.current.items).toEqual([]);
    expect(result.current.receipts).toEqual([]);
  });

  it("clears only accepted ready receipt IDs", async () => {
    vi.mocked(stageComposerAttachment).mockResolvedValueOnce(firstReceipt).mockResolvedValueOnce(secondReceipt);
    const { result } = renderHook(() => useComposerAttachments("project-1", "conversation-1"));
    await act(async () => { await result.current.addFiles([file("counts.csv"), file("markers.csv")]); });

    act(() => result.current.clearAccepted([firstReceipt.id]));
    expect(result.current.receipts).toEqual([secondReceipt]);
    expect(result.current.items).toHaveLength(1);
  });

  it("prevents duplicate native picker calls and retries a failed picker", async () => {
    const pending = deferred<ComposerAttachmentReceipt[]>();
    vi.mocked(chooseComposerAttachments).mockReturnValueOnce(pending.promise).mockRejectedValueOnce(new Error("picker internals"));
    const { result } = renderHook(() => useComposerAttachments("project-1", "conversation-1"));

    const firstCall = result.current.chooseFiles();
    const secondCall = result.current.chooseFiles();
    expect(chooseComposerAttachments).toHaveBeenCalledTimes(1);
    await waitFor(() => expect(result.current.busy).toBe(true));
    pending.resolve([firstReceipt, firstReceipt]);
    await act(async () => { await Promise.all([firstCall, secondCall]); });
    expect(result.current.items).toHaveLength(1);
    expect(result.current.items[0]).toMatchObject({ status: "ready", receipt: firstReceipt });

    await act(async () => { await result.current.chooseFiles(); });
    expect(result.current.items.some((item) => item.status === "error")).toBe(true);
    const failed = result.current.items.find((item) => item.status === "error")!;
    vi.mocked(chooseComposerAttachments).mockResolvedValueOnce([secondReceipt]);
    await act(async () => { await result.current.retry(failed.key); });
    expect(result.current.items.some((item) => item.receipt?.id === secondReceipt.id)).toBe(true);
  });

  it("shows a picker-busy notice instead of staging a drop during the native chooser", async () => {
    const pending = deferred<ComposerAttachmentReceipt[]>();
    vi.mocked(chooseComposerAttachments).mockReturnValueOnce(pending.promise);
    vi.mocked(stageComposerAttachment).mockResolvedValue(firstReceipt);
    const { result } = renderHook(() => useComposerAttachments("project-1", "conversation-1"));

    let choosePromise!: Promise<void>;
    act(() => { choosePromise = result.current.chooseFiles(); });
    await waitFor(() => expect(result.current.busy).toBe(true));
    await act(async () => result.current.addFiles([file("late.csv")]));

    expect(stageComposerAttachment).not.toHaveBeenCalled();
    expect(result.current.items).toEqual([]);
    expect(result.current.pickerBusyNotice).toBe(true);

    pending.resolve([]);
    await act(async () => choosePromise);
  });

  it("does not open the native chooser while a dropped upload is pending", async () => {
    const pending = deferred<ComposerAttachmentReceipt>();
    vi.mocked(stageComposerAttachment).mockReturnValueOnce(pending.promise);
    const { result } = renderHook(() => useComposerAttachments("project-1", "conversation-1"));

    let addPromise!: Promise<void>;
    act(() => { addPromise = result.current.addFiles([file("counts.csv")]); });
    await waitFor(() => expect(result.current.items[0]?.status).toBe("uploading"));
    await act(async () => result.current.chooseFiles());

    expect(chooseComposerAttachments).not.toHaveBeenCalled();
    expect(result.current.pickerBusyNotice).toBe(true);

    pending.resolve(firstReceipt);
    await act(async () => addPromise);
  });

  it("passes the remaining native picker slot and byte budget", async () => {
    vi.mocked(stageComposerAttachment).mockResolvedValueOnce(firstReceipt);
    vi.mocked(chooseComposerAttachments).mockResolvedValueOnce([]);
    const { result } = renderHook(() => useComposerAttachments("project-1", "conversation-1"));

    await act(async () => result.current.addFiles([file("counts.csv")]));
    await act(async () => result.current.chooseFiles());

    expect(chooseComposerAttachments).toHaveBeenCalledWith(
      "project-1",
      "conversation-1",
      {
        maxFiles: 7,
        maxBytes: MAX_COMPOSER_ATTACHMENT_TOTAL_BYTES - firstReceipt.size_bytes,
      },
    );
    expect(result.current.pickerBusyNotice).toBe(false);
  });

  it("sets the limit state without opening the chooser when no slot remains", async () => {
    vi.mocked(stageComposerAttachment).mockResolvedValue(firstReceipt);
    const { result } = renderHook(() => useComposerAttachments("project-1", "conversation-1"));
    const files = Array.from({ length: 8 }, (_, index) => file(`file-${index}.csv`));

    await act(async () => result.current.addFiles(files));
    await act(async () => result.current.chooseFiles());

    expect(chooseComposerAttachments).not.toHaveBeenCalled();
    expect(result.current.limitReached).toBe(true);
  });

  it("drops stale results when the active conversation changes", async () => {
    const pending = deferred<ComposerAttachmentReceipt>();
    vi.mocked(stageComposerAttachment).mockReturnValueOnce(pending.promise);
    const { result, rerender } = renderHook(
      ({ conversationId }) => useComposerAttachments("project-1", conversationId),
      { initialProps: { conversationId: "conversation-1" } },
    );
    let addPromise!: Promise<void>;
    act(() => { addPromise = result.current.addFiles([file("counts.csv")]); });
    await waitFor(() => expect(result.current.items[0]?.status).toBe("uploading"));

    rerender({ conversationId: "conversation-2" });
    await waitFor(() => expect(result.current.items).toEqual([]));
    pending.resolve(firstReceipt);
    await addPromise;
    expect(result.current.items).toEqual([]);
  });

  it("does not stage or open a chooser without an active conversation", async () => {
    const { result } = renderHook(() => useComposerAttachments("project-1", undefined));
    await act(async () => {
      await result.current.addFiles([file("counts.csv")]);
      await result.current.chooseFiles();
    });
    expect(stageComposerAttachment).not.toHaveBeenCalled();
    expect(chooseComposerAttachments).not.toHaveBeenCalled();
    expect(result.current.items).toEqual([]);
  });
});

it("restores scoped queue receipts without restaging or overwriting existing material", () => {
  const { result } = renderHook(() => useComposerAttachments("project-1", "conversation-1"));
  act(() => expect(result.current.restoreReceipts([{ ...firstReceipt, conversation_id: "other" }])).toBe(false));
  expect(result.current.receipts).toEqual([]);
  act(() => expect(result.current.restoreReceipts([firstReceipt])).toBe(true));
  expect(result.current.receipts).toEqual([firstReceipt]);
  act(() => expect(result.current.restoreReceipts([secondReceipt])).toBe(false));
  expect(result.current.receipts).toEqual([firstReceipt]);
  expect(stageComposerAttachment).not.toHaveBeenCalled();
});
