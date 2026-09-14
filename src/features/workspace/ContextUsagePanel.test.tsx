import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { useState } from "react";
import { useWindowEscapeLayer } from "../settings/BrowserSettings";
import { expect, it, vi } from "vitest";
import { WorkspaceShell } from "./WorkspaceShell";
import { ContextUsagePanel, type ContextUsageView } from "./ContextUsagePanel";
const value: ContextUsageView = { contextTokens: 1000, contextLimit: 4000, limitSource: "exact_catalog", estimated: false, inputTokens: 0, outputTokens: 20, incompleteAttempts: 1, serializedRequestBytes: 6000 };
it("keeps explicit zero distinct from missing usage and request bytes", () => {
  render(<ContextUsagePanel value={value} locale="en-US" onClose={vi.fn()} />);
  expect(screen.getByText("25.0%")).toBeInTheDocument();
  expect(screen.getByText("0")).toBeInTheDocument();
  expect(screen.getByRole("status")).toHaveTextContent("not complete totals");
  fireEvent.click(screen.getByRole("button", { name: "Usage facets and request budget" }));
  expect(screen.getByText("6,000")).toBeInTheDocument();
  expect(screen.getByText("Serialized request bytes")).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Compact context" })).toBeDisabled();
});
it("never invents a percentage when the context window is unknown", () => {
  render(<ContextUsagePanel value={{ ...value, contextLimit: null, limitSource: "unknown" }} locale="en-US" onClose={vi.fn()} />);
  expect(screen.queryByRole("progressbar")).toBeNull();
  expect(screen.getByText("Unknown limit")).toBeInTheDocument();
});
it("keeps a floating panel reachable after the viewport shrinks", () => {
  render(<ContextUsagePanel value={value} locale="en-US" onClose={vi.fn()} />);
  fireEvent.click(screen.getByRole("button", { name: "Float" }));
  const panel = screen.getByRole("dialog");
  vi.spyOn(panel, "getBoundingClientRect").mockReturnValue({ width: 420, height: 400 } as DOMRect);
  const width = window.innerWidth; const height = window.innerHeight;
  try {
    Object.defineProperty(window, "innerWidth", { configurable: true, value: 400 });
    Object.defineProperty(window, "innerHeight", { configurable: true, value: 100 });
    fireEvent(window, new Event("resize"));
    expect(panel).toHaveStyle({ left: "0px", top: "0px" });
    expect(screen.getByRole("button", { name: "Close usage details" })).toBeInTheDocument();
  } finally {
    Object.defineProperty(window, "innerWidth", { configurable: true, value: width });
    Object.defineProperty(window, "innerHeight", { configurable: true, value: height });
  }
});
it("supports docking and immediate window Escape with focus restoration", () => {
  const trigger = document.createElement("button"); document.body.append(trigger); trigger.focus();
  const close = vi.fn(); const view = render(<ContextUsagePanel value={null} locale="en-US" onClose={close} />);
  fireEvent.click(screen.getByRole("button", { name: "Float" }));
  expect(screen.getByRole("dialog")).toHaveClass("is-floating");
  fireEvent.click(screen.getByRole("button", { name: "Dock" }));
  expect(screen.getByRole("dialog")).toHaveClass("is-docked");
  fireEvent.keyDown(window, { key: "Escape" });
  expect(close).toHaveBeenCalledTimes(1);
  view.unmount(); expect(trigger).toHaveFocus(); trigger.remove();
});

it("closes only the top usage panel with one Escape", () => {
  const parentClose = vi.fn();
  function Parent() {
    const [open, setOpen] = useState(false);
    useWindowEscapeLayer(true, parentClose);
    return <div><button onClick={() => setOpen(true)}>Open usage</button>{open && <ContextUsagePanel value={null} locale="en-US" onClose={() => setOpen(false)} />}</div>;
  }
  render(<Parent />);
  fireEvent.click(screen.getByRole("button", { name: "Open usage" }));
  fireEvent.keyDown(window, { key: "Escape" });
  expect(screen.queryByRole("dialog")).toBeNull();
  expect(parentClose).not.toHaveBeenCalled();
  fireEvent.keyDown(window, { key: "Escape" });
  expect(parentClose).toHaveBeenCalledTimes(1);
});

it("opens usage details from the composer meter and closes them on conversation change", () => {
  const props = { project: { id: "p", name: "test", status: "ready" as const, template: "blank" as const }, locale: "en-US" as const, onLocaleChange: vi.fn(), contextUsage: value };
  const view = render(<WorkspaceShell {...props} activeConversationId="c" />);
  fireEvent.click(screen.getByRole("button", { name: "Inspect context usage" }));
  expect(screen.getByRole("dialog", { name: "Context usage" })).toBeInTheDocument();
  expect(screen.getByText("25.0%")).toBeInTheDocument();
  view.rerender(<WorkspaceShell {...props} activeConversationId="other" contextUsage={null} />);
  expect(screen.queryByRole("dialog")).toBeNull();
});

it("opens /context during a run instead of enqueueing it as a research message", async () => {
  const queue = vi.fn();
  render(<WorkspaceShell project={{ id: "p", name: "test", status: "ready", template: "blank" }} locale="en-US" onLocaleChange={vi.fn()} activeConversationId="c" agentBusy onQueue={queue} />);
  const input = screen.getByRole("textbox", { name: /Describe/ });
  fireEvent.change(input, { target: { value: "/context" } });
  await waitFor(() => expect(input).toBeEnabled());
  fireEvent.keyDown(input, { key: "Enter" });
  expect(await screen.findByRole("dialog", { name: "Context usage" })).toBeInTheDocument();
  expect(queue).not.toHaveBeenCalled();
});
