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
const originalInnerWidth = window.innerWidth;
const originalInnerHeight = window.innerHeight;

beforeEach(() => {
  Object.defineProperty(Range.prototype, "getBoundingClientRect", {
    configurable: true,
    value: vi.fn(() => ({ x: 20, y: 30, top: 30, left: 20, right: 120, bottom: 48, width: 100, height: 18, toJSON: () => ({}) })),
  });
});

afterEach(() => {
  window.getSelection()?.removeAllRanges();
  document.documentElement.removeAttribute("data-omicsops-scale");
  Object.defineProperty(window, "innerWidth", { configurable: true, value: originalInnerWidth });
  Object.defineProperty(window, "innerHeight", { configurable: true, value: originalInnerHeight });
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

  it("ignores real input and textarea selections", () => {
    render(<><input aria-label="Draft input" defaultValue="input text" /><textarea aria-label="Draft textarea" defaultValue="textarea text" /><MessageSelectionActions enabled locale="en-US" projectId="project-1" conversationId="conversation-1" onQuote={vi.fn()} /></>);

    const input = screen.getByRole("textbox", { name: "Draft input" }) as HTMLInputElement;
    input.focus();
    input.setSelectionRange(0, input.value.length);
    act(() => document.dispatchEvent(new Event("selectionchange")));
    expect(screen.queryByRole("toolbar")).not.toBeInTheDocument();

    const textarea = screen.getByRole("textbox", { name: "Draft textarea" }) as HTMLTextAreaElement;
    textarea.focus();
    textarea.setSelectionRange(0, textarea.value.length);
    act(() => document.dispatchEvent(new Event("selectionchange")));
    expect(screen.queryByRole("toolbar")).not.toBeInTheDocument();
  });

  it("uses the measured English toolbar size in physical viewport coordinates at 120 percent", async () => {
    Object.defineProperty(window, "innerWidth", { configurable: true, value: 1000 });
    Object.defineProperty(window, "innerHeight", { configurable: true, value: 850 });
    document.documentElement.dataset.omicsopsScale = "1.2";
    vi.mocked(Range.prototype.getBoundingClientRect).mockReturnValue({ x: 821.05, y: 167.58, top: 167.58, left: 821.05, right: 900, bottom: 185.58, width: 78.95, height: 18, toJSON: () => ({}) });
    vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(function (this: HTMLElement) {
      if (this.classList.contains("message-selection-actions")) return { x: 0, y: 0, top: 0, left: 0, right: 286, bottom: 54, width: 286, height: 54, toJSON: () => ({}) };
      return { x: 0, y: 0, top: 0, left: 0, right: 0, bottom: 0, width: 0, height: 0, toJSON: () => ({}) };
    });
    const root = document.createElement("div");
    root.id = "root";
    document.body.append(root);
    render(<>{message("message-1", "assistant", "Right edge selection")}<MessageSelectionActions enabled locale="en-US" projectId="project-1" conversationId="conversation-1" onQuote={vi.fn()} /></>, { container: root });
    selectText(screen.getByText("Right edge selection").firstChild as Text);

    const toolbar = await screen.findByRole("toolbar", { name: "Message selection actions" });
    await waitFor(() => expect(toolbar).toHaveStyle({ left: "706px" }));
    expect(Number.parseFloat(toolbar.style.top)).toBeCloseTo(105.58);
    expect(toolbar.parentElement).toBe(document.body);
    expect(Number.parseFloat(toolbar.style.left) + 286).toBeLessThanOrEqual(992);
  });

  it("dismisses stale placement on viewport resize and app scale changes", async () => {
    render(<>{message("message-1", "assistant", "Selection to dismiss")}<MessageSelectionActions enabled locale="en-US" projectId="project-1" conversationId="conversation-1" onQuote={vi.fn()} /></>);
    selectText(screen.getByText("Selection to dismiss").firstChild as Text);
    expect(screen.getByRole("toolbar")).toBeInTheDocument();
    fireEvent(window, new Event("resize"));
    expect(screen.queryByRole("toolbar")).not.toBeInTheDocument();

    selectText(screen.getByText("Selection to dismiss").firstChild as Text);
    expect(screen.getByRole("toolbar")).toBeInTheDocument();
    document.documentElement.dataset.omicsopsScale = "1.1";
    await waitFor(() => expect(screen.queryByRole("toolbar")).not.toBeInTheDocument());
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
