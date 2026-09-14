import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { expect, it, vi } from "vitest";
import { WorkspaceShell } from "./WorkspaceShell";
import type { AgentRunEventV4 } from "../../types";
const event: AgentRunEventV4 = { schema_version: 4, run_id: "run", project_id: "p", conversation_id: "c", sequence: 1, previous_hash: "", event_hash: "head", occurred_at: "2026-09-14T00:00:00Z", event: { kind: "run_created", mode: "execute" } };
const base = {
  project: { id: "p", name: "test", status: "ready" as const, template: "blank" as const }, locale: "en-US" as const, onLocaleChange: vi.fn(), activeConversationId: "c", activeRunId: "run", agentBusy: true, agentRunEventsV4: [event], onQueue: vi.fn(),
  computeBackendId: "local", computeBackends: [{ descriptor: { schema_version: 4 as const, backend_id: "local", kind: "local" as const, isolation: "process" as const, available: true, supports_python: true, supports_r: true, supports_network_policy: false }, selectable: true, reason: null, python_status: "available" as const, r_status: "available" as const, resolved_image_id: null }],
};
function controller() { return { busy: false, pending: false, error: false, send: vi.fn().mockResolvedValue(true), retry: vi.fn().mockResolvedValue(true) }; }
function open() { fireEvent.click(screen.getByRole("button", { name: "Send options" })); }
it("closes the replacement menu immediately with window Escape", () => {
  const replacement = controller(); render(<WorkspaceShell {...base} replacement={replacement} />);
  open(); expect(screen.getByRole("menuitem", { name: /Interrupt and replace/ })).toBeInTheDocument();
  fireEvent.keyDown(window, { key: "Escape" });
  expect(screen.queryByRole("menuitem", { name: /Interrupt and replace/ })).toBeNull();
  expect(screen.getByRole("textbox", { name: /Describe/ })).toBeInTheDocument();
  expect(replacement.send).not.toHaveBeenCalled();
});
it("binds the observed target and references without issuing ordinary Send or Stop", async () => {
  const replacement = controller(); const stop = vi.fn();
  const reference = { kind: "artifact" as const, project_id: "p", id: "artifact" };
  render(<WorkspaceShell {...base} replacement={replacement} onCancelRun={stop} searchRequest={{ key: "attach", kind: "attach", projectId: "p", conversationId: "c", item: { reference, label: "Counts", description: "" } }} />);
  const input = screen.getByRole("textbox", { name: /Describe/ }); fireEvent.change(input, { target: { value: "Use these instead" } });
  open(); await waitFor(() => expect(screen.getByRole("menuitem", { name: /Interrupt and replace/ })).toBeEnabled()); fireEvent.click(screen.getByRole("menuitem", { name: /Interrupt and replace/ }));
  await waitFor(() => expect(replacement.send).toHaveBeenCalledWith("Use these instead", "chat", { run_id: "run", sequence: 1, event_hash: "head" }, [reference], []));
  expect(stop).not.toHaveBeenCalled(); expect(base.onQueue).not.toHaveBeenCalled(); expect(input).toHaveValue("");
});
it("retains the new conversation draft after a late replacement receipt", async () => {
  const replacement = controller(); let finish!: (accepted: boolean) => void;
  replacement.send.mockImplementation(() => new Promise<boolean>((resolve) => { finish = resolve; }));
  const view = render(<WorkspaceShell {...base} replacement={replacement} />);
  fireEvent.change(screen.getByRole("textbox", { name: /Describe/ }), { target: { value: "Same words" } });
  open(); await waitFor(() => expect(screen.getByRole("menuitem", { name: /Interrupt and replace/ })).toBeEnabled()); fireEvent.click(screen.getByRole("menuitem", { name: /Interrupt and replace/ }));
  view.rerender(<WorkspaceShell {...base} activeConversationId="other" replacement={controller()} />);
  fireEvent.change(screen.getByRole("textbox", { name: /Describe/ }), { target: { value: "Same words" } });
  await act(async () => { finish(true); });
  expect(screen.getByRole("textbox", { name: /Describe/ })).toHaveValue("Same words");
});
it("keeps unknown requests separate from a newer active run", async () => {
  const replacement = { ...controller(), pending: true, error: true, originalMarkdown: "Original" };
  render(<WorkspaceShell {...base} activeRunId="new-run" replacement={replacement} />);
  open(); expect(screen.getByRole("menuitem", { name: /Interrupt and replace/ })).toBeDisabled();
  fireEvent.click(screen.getByRole("button", { name: "Reconcile original replacement" }));
  await waitFor(() => expect(replacement.retry).toHaveBeenCalledTimes(1));
  expect(replacement.send).not.toHaveBeenCalled();
});
