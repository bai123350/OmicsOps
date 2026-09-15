import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { MessageSelectionActions } from "./MessageSelectionActions";

function selectText(start: Text, end: Text = start, endOffset = end.data.length) {
  const range = document.createRange();
  range.setStart(start, 0);
  range.setEnd(end, endOffset);
  const selection = window.getSelection()!;
  selection.removeAllRanges();
  selection.addRange(range);
  act(() => document.dispatchEvent(new Event("selectionchange")));
}

function message(messageId: string, role: "user" | "assistant", text: string) {
  return <article data-message-id={messageId}>
    <div data-message-selection-body data-message-id={messageId} data-message-role={role} data-project-id="project-1" data-conversation-id="conversation-1">{text}</div>
  </article>;
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

const originalRangeRect = Object.getOwnPropertyDescriptor(Range.prototype, "getBoundingClientRect");

beforeEach(() => {
  Object.defineProperty(Range.prototype, "getBoundingClientRect", {
    configurable: true,
    value: vi.fn(() => ({ x: 20, y: 30, top: 30, left: 20, right: 120, bottom: 48, width: 100, height: 18, toJSON: () => ({}) })),
  });
});

afterEach(() => {
  window.getSelection()?.removeAllRanges();
  vi.restoreAllMocks();
  if (originalRangeRect) Object.defineProperty(Range.prototype, "getBoundingClientRect", originalRangeRect);
  else delete (Range.prototype as Partial<Range>).getBoundingClientRect;
});

describe("MessageSelectionActions", () => {
  it("copies the exact text selected within one visible message body", async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", { value: { writeText }, configurable: true });
    render(<>{message("message-1", "assistant", "Exact selected text")}<MessageSelectionActions enabled locale="en-US" projectId="project-1" conversationId="conversation-1" onQuote={vi.fn()} /></>);
    selectText(screen.getByText("Exact selected text").firstChild as Text);

    fireEvent.mouseDown(screen.getByRole("button", { name: "Copy selection" }));
    fireEvent.click(screen.getByRole("button", { name: "Copy selection" }));

    expect(writeText).toHaveBeenCalledWith("Exact selected text");
    await waitFor(() => expect(screen.queryByRole("toolbar", { name: "Message selection actions" })).not.toBeInTheDocument());
  });

  it("quotes once without sending and closes first on Escape", () => {
    const onQuote = vi.fn();
    render(<>{message("message-1", "user", "Selected finding")}<MessageSelectionActions enabled locale="en-US" projectId="project-1" conversationId="conversation-1" onQuote={onQuote} /></>);
    selectText(screen.getByText("Selected finding").firstChild as Text);
    expect(screen.getByRole("toolbar", { name: "Message selection actions" })).toBeInTheDocument();

    fireEvent.keyDown(window, { key: "Escape" });
    expect(screen.queryByRole("toolbar", { name: "Message selection actions" })).not.toBeInTheDocument();
    expect(onQuote).not.toHaveBeenCalled();

    selectText(screen.getByText("Selected finding").firstChild as Text);
    fireEvent.click(screen.getByRole("button", { name: "Quote in draft" }));
    expect(onQuote).toHaveBeenCalledOnce();
    expect(onQuote).toHaveBeenCalledWith({ text: "Selected finding", role: "user" });
  });

  it("rejects cross-message, input, disabled, and over-limit selections", () => {
    const first = message("message-1", "assistant", "First message");
    const second = message("message-2", "assistant", "Second message");
    const view = render(<>{first}{second}<input aria-label="Draft" defaultValue="input text" /><MessageSelectionActions enabled locale="en-US" projectId="project-1" conversationId="conversation-1" onQuote={vi.fn()} /></>);
    selectText(screen.getByText("First message").firstChild as Text, screen.getByText("Second message").firstChild as Text);
    expect(screen.queryByRole("toolbar")).not.toBeInTheDocument();

    view.rerender(<>{message("large", "assistant", "x".repeat(16 * 1024 + 1))}<MessageSelectionActions enabled locale="en-US" projectId="project-1" conversationId="conversation-1" onQuote={vi.fn()} /></>);
    selectText(screen.getByText("x".repeat(16 * 1024 + 1)).firstChild as Text);
    expect(screen.queryByRole("toolbar")).not.toBeInTheDocument();

    view.rerender(<>{message("message-3", "assistant", "Disabled selection")}<MessageSelectionActions enabled={false} locale="en-US" projectId="project-1" conversationId="conversation-1" onQuote={vi.fn()} /></>);
    selectText(screen.getByText("Disabled selection").firstChild as Text);
    expect(screen.queryByRole("toolbar")).not.toBeInTheDocument();
  });

  it("keeps a failed clipboard action open for retry", async () => {
    const writeText = vi.fn().mockRejectedValueOnce(new Error("denied")).mockResolvedValueOnce(undefined);
    Object.defineProperty(navigator, "clipboard", { value: { writeText }, configurable: true });
    render(<>{message("message-1", "assistant", "Copy me")}<MessageSelectionActions enabled locale="en-US" projectId="project-1" conversationId="conversation-1" onQuote={vi.fn()} /></>);
    selectText(screen.getByText("Copy me").firstChild as Text);

    fireEvent.click(screen.getByRole("button", { name: "Copy selection" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Copy failed. Retry or select the text manually.");
    fireEvent.click(screen.getByRole("button", { name: "Copy selection" }));
    expect(writeText).toHaveBeenCalledTimes(2);
  });

  it.each(["success", "failure"] as const)("does not let a stale clipboard %s clear or annotate a newer selection", async (outcome) => {
    const oldWrite = deferred<void>();
    const writeText = vi.fn().mockReturnValueOnce(oldWrite.promise);
    Object.defineProperty(navigator, "clipboard", { value: { writeText }, configurable: true });
    const onQuote = vi.fn();
    render(<>{message("message-1", "assistant", "First selection")}{message("message-2", "user", "Second selection")}<MessageSelectionActions enabled locale="en-US" projectId="project-1" conversationId="conversation-1" onQuote={onQuote} /></>);
    selectText(screen.getByText("First selection").firstChild as Text);
    fireEvent.click(screen.getByRole("button", { name: "Copy selection" }));

    selectText(screen.getByText("Second selection").firstChild as Text);
    await act(async () => {
      if (outcome === "success") oldWrite.resolve();
      else oldWrite.reject(new Error("denied"));
      await oldWrite.promise.catch(() => undefined);
    });

    expect(screen.getByRole("toolbar", { name: "Message selection actions" })).toBeInTheDocument();
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Quote in draft" }));
    expect(onQuote).toHaveBeenCalledWith({ text: "Second selection", role: "user" });
  });
});
