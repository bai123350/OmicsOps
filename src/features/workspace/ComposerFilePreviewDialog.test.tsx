import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { useState } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { createComposerQuote, previewComposerFileText, type WorkspaceFileReference } from "../../composer-file-api";
import type { ComposerCatalogItem, ComposerTextPreview } from "../../types";
import { ComposerFilePreviewDialog } from "./ComposerFilePreviewDialog";

vi.mock("../../composer-file-api", () => ({
  createComposerQuote: vi.fn(),
  previewComposerFileText: vi.fn(),
}));

const previewFile = vi.mocked(previewComposerFileText);
const createQuote = vi.mocked(createComposerQuote);

const reference: WorkspaceFileReference = {
  kind: "workspace_file",
  project_id: "project-a",
  backend_id: "local",
  relative_path: "results/notes.md",
};

const preview: ComposerTextPreview = {
  project_id: "project-a",
  backend_id: "local",
  relative_path: "results/notes.md",
  sha256: "hash-1",
  text: "alpha\nbeta\ngamma",
};

const quoteItem: ComposerCatalogItem = {
  reference: { kind: "quote", project_id: "project-a", id: "quote-1" },
  label: "notes.md selection",
  description: "Quoted file text",
};

function deferred<T>() {
  let resolve!: (value: T | PromiseLike<T>) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise;
    reject = rejectPromise;
  });
  return { promise, resolve, reject };
}

function renderDialog(overrides: Partial<React.ComponentProps<typeof ComposerFilePreviewDialog>> = {}) {
  const props: React.ComponentProps<typeof ComposerFilePreviewDialog> = {
    reference,
    conversationId: "conversation-a",
    zh: false,
    disabled: false,
    onClose: vi.fn(),
    onAttach: vi.fn(),
    ...overrides,
  };
  return render(<ComposerFilePreviewDialog {...props} />);
}

beforeEach(() => {
  vi.clearAllMocks();
  previewFile.mockResolvedValue(preview);
  createQuote.mockResolvedValue(quoteItem);
});

describe("ComposerFilePreviewDialog", () => {
  it("loads a read-only preview with explicit source and path metadata", async () => {
    renderDialog();

    const dialog = await screen.findByRole("dialog", { name: "Preview workspace file" });
    expect(within(dialog).getByText("Local")).toBeInTheDocument();
    expect(within(dialog).getByText("results/notes.md")).toBeInTheDocument();
    expect(within(dialog).getByRole("textbox", { name: "File text" })).toHaveValue(preview.text);
    expect(within(dialog).getByRole("textbox", { name: "File text" })).toHaveAttribute("readonly");
    expect(within(dialog).getByRole("button", { name: "Quote selected text" })).toBeEnabled();
    expect(previewFile).toHaveBeenCalledWith(reference);
  });

  it("shows SSH as the source label and keeps the selected path visible", async () => {
    const sshReference: WorkspaceFileReference = { ...reference, backend_id: "ssh:connection-a", relative_path: "data/counts.tsv" };
    previewFile.mockResolvedValue({ ...preview, backend_id: sshReference.backend_id, relative_path: sshReference.relative_path });
    renderDialog({ reference: sshReference });

    const dialog = await screen.findByRole("dialog", { name: "Preview workspace file" });
    expect(within(dialog).getByText("SSH")).toBeInTheDocument();
    expect(within(dialog).getByText("data/counts.tsv")).toBeInTheDocument();
  });

  it("quotes the current selection with the preview hash and conversation", async () => {
    const onAttach = vi.fn();
    renderDialog({ onAttach });

    const textarea = await screen.findByRole("textbox", { name: "File text" }) as HTMLTextAreaElement;
    textarea.focus();
    textarea.setSelectionRange(0, 5);
    fireEvent.select(textarea);
    fireEvent.click(screen.getByRole("button", { name: "Quote selected text" }));

    await waitFor(() => expect(createQuote).toHaveBeenCalledTimes(1));
    expect(createQuote).toHaveBeenCalledWith({
      project_id: "project-a",
      conversation_id: "conversation-a",
      backend_id: "local",
      relative_path: "results/notes.md",
      sha256: "hash-1",
      text: "alpha",
    });
    expect(onAttach).toHaveBeenCalledWith(quoteItem);
  });

  it("rejects an empty selection and selections over 8 KiB before creating a quote", async () => {
    const longText = "a".repeat(8_193);
    previewFile.mockResolvedValue({ ...preview, text: longText });
    renderDialog();
    const textarea = await screen.findByRole("textbox", { name: "File text" }) as HTMLTextAreaElement;

    textarea.focus();
    textarea.setSelectionRange(0, 0);
    fireEvent.select(textarea);
    fireEvent.click(screen.getByRole("button", { name: "Quote selected text" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Select some text");
    expect(createQuote).not.toHaveBeenCalled();

    textarea.setSelectionRange(0, longText.length);
    fireEvent.select(textarea);
    fireEvent.click(screen.getByRole("button", { name: "Quote selected text" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("8 KiB");
    expect(createQuote).not.toHaveBeenCalled();
    expect(textarea.selectionStart).toBe(0);
    expect(textarea.selectionEnd).toBe(longText.length);
  });

  it("keeps the text and selection after a quote failure and ignores duplicate clicks", async () => {
    const request = deferred<ComposerCatalogItem>();
    createQuote.mockReturnValue(request.promise);
    renderDialog();
    const textarea = await screen.findByRole("textbox", { name: "File text" }) as HTMLTextAreaElement;
    textarea.focus();
    textarea.setSelectionRange(6, 10);
    fireEvent.select(textarea);

    const quoteButton = screen.getByRole("button", { name: "Quote selected text" });
    fireEvent.click(quoteButton);
    fireEvent.click(quoteButton);
    expect(createQuote).toHaveBeenCalledTimes(1);
    expect(quoteButton).toBeDisabled();

    request.reject(new Error("private backend details"));
    expect(await screen.findByRole("alert")).toHaveTextContent("Could not create the quote");
    expect(screen.queryByText("private backend details")).not.toBeInTheDocument();
    expect(textarea).toHaveValue(preview.text);
    expect(textarea.selectionStart).toBe(6);
    expect(textarea.selectionEnd).toBe(10);
    expect(quoteButton).toBeEnabled();
  });

  it("renders a safe preview error and retries without losing the dialog", async () => {
    previewFile.mockRejectedValueOnce(new Error("private path details"));
    renderDialog();

    expect(await screen.findByRole("alert")).toHaveTextContent("Could not load this file preview");
    expect(screen.queryByText("private path details")).not.toBeInTheDocument();
    const retry = screen.getByRole("button", { name: "Retry preview" });
    previewFile.mockResolvedValueOnce(preview);
    fireEvent.click(retry);

    expect(await screen.findByRole("textbox", { name: "File text" })).toHaveValue(preview.text);
    expect(previewFile).toHaveBeenCalledTimes(2);
  });

  it("ignores stale preview completions after the source or conversation changes", async () => {
    const first = deferred<ComposerTextPreview>();
    const second = deferred<ComposerTextPreview>();
    previewFile.mockReturnValueOnce(first.promise).mockReturnValueOnce(second.promise);
    const { rerender } = renderDialog();
    const nextReference: WorkspaceFileReference = { ...reference, relative_path: "results/other.md" };

    rerender(<ComposerFilePreviewDialog reference={nextReference} conversationId="conversation-b" zh={false} onClose={vi.fn()} onAttach={vi.fn()} />);
    first.resolve(preview);
    await Promise.resolve();
    expect(screen.queryByRole("textbox", { name: "File text" })).not.toBeInTheDocument();

    second.resolve({ ...preview, relative_path: nextReference.relative_path });
    expect(await screen.findByRole("textbox", { name: "File text" })).toHaveValue(preview.text);
  });

  it("traps focus, closes immediately on window Escape, and restores the launch focus", async () => {
    function Host() {
      const [open, setOpen] = useState(false);
      return <>
        <button type="button" onClick={() => setOpen(true)}>Launch preview</button>
        {open && <ComposerFilePreviewDialog {...{
          reference,
          conversationId: "conversation-a",
          zh: false,
          onClose: () => setOpen(false),
          onAttach: vi.fn(),
        }} />}
      </>;
    }

    render(<Host />);
    const launch = screen.getByRole("button", { name: "Launch preview" });
    launch.focus();
    fireEvent.click(launch);
    const dialog = await screen.findByRole("dialog", { name: "Preview workspace file" });
    const close = within(dialog).getByRole("button", { name: "Close file preview" });
    expect(document.activeElement).toBe(close);

    fireEvent.keyDown(close, { key: "Tab" });
    expect(document.activeElement).toBe(within(dialog).getByRole("textbox", { name: "File text" }));
    fireEvent.keyDown(document.activeElement!, { key: "Tab", shiftKey: true });
    expect(document.activeElement).toBe(close);

    fireEvent.keyDown(window, { key: "Escape" });
    await waitFor(() => expect(screen.queryByRole("dialog", { name: "Preview workspace file" })).not.toBeInTheDocument());
    expect(document.activeElement).toBe(launch);
  });

  it("does not update state after unmount while preview or quote work is pending", async () => {
    const pendingPreview = deferred<ComposerTextPreview>();
    previewFile.mockReturnValue(pendingPreview.promise);
    const { unmount } = renderDialog();
    unmount();
    pendingPreview.resolve(preview);
    await Promise.resolve();
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });
});
