import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import type { ComposerAttachmentReceipt } from "../../types";
import { ComposerAttachments, type ComposerAttachmentItem } from "./ComposerAttachments";

const imageReceipt: ComposerAttachmentReceipt = {
  id: "image-1",
  project_id: "project-1",
  conversation_id: "conversation-1",
  name: "cells.png",
  relative_path: ".omicsops/attachments/image-1/cells.png",
  size_bytes: 2_048,
  sha256: "hash-image",
  media_type: "image/png",
};

const items: ComposerAttachmentItem[] = [
  {
    key: "key-uploading",
    clientKey: "key-uploading",
    name: "counts.csv",
    sizeBytes: 1_024,
    mediaType: "text/csv",
    status: "uploading",
  },
  {
    key: "key-ready",
    clientKey: "key-ready",
    name: "cells.png",
    sizeBytes: imageReceipt.size_bytes,
    mediaType: imageReceipt.media_type,
    status: "ready",
    receipt: imageReceipt,
  },
  {
    key: "key-error",
    clientKey: "key-error",
    name: "broken.csv",
    sizeBytes: 128,
    mediaType: "text/csv",
    status: "error",
    error: "Could not upload attachment. Try again.",
  },
];

describe("ComposerAttachments", () => {
  it("renders file names, statuses, sizes, and an image indicator without a fake preview", () => {
    render(<ComposerAttachments items={items} zh={false} onRemove={vi.fn()} onRetry={vi.fn()} />);

    expect(screen.getByText("counts.csv")).toBeInTheDocument();
    expect(screen.getByText("Uploading…")).toBeInTheDocument();
    expect(screen.getByText("cells.png")).toBeInTheDocument();
    expect(screen.getByText("Ready")).toBeInTheDocument();
    expect(screen.getByLabelText("Image")).toBeInTheDocument();
    expect(screen.queryByRole("img")).not.toBeInTheDocument();
    expect(screen.getByText("broken.csv")).toBeInTheDocument();
    expect(screen.getByText("Could not upload attachment. Try again.")).toHaveAttribute("role", "alert");
  });

  it("uses the bilingual action labels and sends stable keys", () => {
    const onRemove = vi.fn();
    const onRetry = vi.fn();
    render(<ComposerAttachments items={items} zh onRemove={onRemove} onRetry={onRetry} />);

    fireEvent.click(screen.getByRole("button", { name: "重试 broken.csv" }));
    fireEvent.click(screen.getByRole("button", { name: "移除 broken.csv" }));

    expect(onRetry).toHaveBeenCalledWith("key-error");
    expect(onRemove).toHaveBeenCalledWith("key-error");
  });

  it("disables actions while the parent composer is locked", () => {
    render(<ComposerAttachments items={items} zh={false} disabled onRemove={vi.fn()} onRetry={vi.fn()} />);

    expect(screen.getByRole("button", { name: "Retry broken.csv" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Remove broken.csv" })).toBeDisabled();
    expect(screen.getByRole("list", { name: "Attachments" })).toHaveAttribute("aria-busy", "true");
  });

  it("renders nothing when there are no staged or pending files", () => {
    const { container } = render(<ComposerAttachments items={[]} zh={false} onRemove={vi.fn()} onRetry={vi.fn()} />);
    expect(container).toBeEmptyDOMElement();
  });
});
