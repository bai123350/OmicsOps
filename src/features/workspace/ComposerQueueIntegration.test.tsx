import type { AgentRunEventV4, ComposerQueueItemV4 } from "../../types";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { expect, it, vi } from "vitest";
import { WorkspaceShell } from "./WorkspaceShell";
const base = { project: { id: "p", name: "test", status: "ready" as const, template: "blank" as const }, locale: "en-US" as const, activeConversationId: "c", computeBackendId: "local", computeBackends: [{ descriptor: { schema_version: 4 as const, backend_id: "local", kind: "local" as const, isolation: "process" as const, available: true, supports_python: true, supports_r: true, supports_network_policy: false }, selectable: true, reason: null, python_status: "available" as const, r_status: "available" as const, resolved_image_id: null }] };
it("queues a busy send while keeping an independent Stop action", async () => {
  const send = vi.fn(); const queue = vi.fn().mockResolvedValue(true); const stop = vi.fn();
  render(<WorkspaceShell {...base} onSend={send} onQueue={queue} onCancelRun={stop} agentBusy runStarted activeRunId="run" />);
  const input = screen.getByRole("textbox", { name: /Describe/ });
  expect(input).toBeEnabled();
  fireEvent.change(input, { target: { value: "Next analysis" } });
  expect(screen.getByRole("button", { name: "Stop current run" })).toBeEnabled();
  await waitFor(() => expect(screen.getByRole("button", { name: "Add to queue" })).toBeEnabled());
  fireEvent.click(screen.getByRole("button", { name: "Add to queue" }));
  await waitFor(() => expect(queue).toHaveBeenCalledWith("Next analysis", "chat"));
  expect(send).not.toHaveBeenCalled();
  expect(input).toHaveValue("");
  fireEvent.click(screen.getByRole("button", { name: "Stop current run" }));
  expect(stop).toHaveBeenCalledTimes(1);
});
it("uses the durable queue for idle sends without inserting an optimistic user bubble", async () => {
  const send = vi.fn(); const queue = vi.fn().mockResolvedValue(true);
  render(<WorkspaceShell {...base} onSend={send} onQueue={queue} />);
  const input = screen.getByRole("textbox", { name: /Describe/ });
  fireEvent.change(input, { target: { value: "First atomic send" } });
  await waitFor(() => expect(screen.getByRole("button", { name: "Send" })).toBeEnabled());
  fireEvent.keyDown(input, { key: "Enter" });
  await waitFor(() => expect(queue).toHaveBeenCalled());
  expect(send).not.toHaveBeenCalled();
  expect(screen.queryByText("First atomic send")).toBeNull();
});
it("keeps an unconfirmed queued draft and accepts text while awaiting plan approval", async () => {
  const queue = vi.fn().mockResolvedValue(false);
  render(<WorkspaceShell {...base} agentMode="plan" conversationLocked onQueue={queue} />);
  const input = screen.getByRole("textbox", { name: /Describe/ });
  expect(input).toBeEnabled();
  fireEvent.change(input, { target: { value: "After this plan" } });
  await waitFor(() => expect(screen.getByRole("button", { name: "Add to queue" })).toBeEnabled());
  fireEvent.click(screen.getByRole("button", { name: "Add to queue" }));
  await waitFor(() => expect(queue).toHaveBeenCalledWith("After this plan", "plan"));
  expect(input).toHaveValue("After this plan");
  expect(screen.getByText(/draft is preserved/)).toBeInTheDocument();
});
it("does not enable queue input until hydration and queue recovery finish", () => {
  render(<WorkspaceShell {...base} onQueue={vi.fn()} queueLoading />);
  expect(screen.getByRole("textbox", { name: /Describe/ })).toBeDisabled();
});

const queued: ComposerQueueItemV4 = {
  request_id: "q", message_id: "m", run_id: "r", project_id: "p", conversation_id: "c", mode: "agent", message_markdown: "Restore this task", position: 1, revision: 1, status: "pending", created_at: "now", updated_at: "now",
  frozen: { model_profile_id: "model", model_configuration_hash: "hash", compute_selection: { schema_version: 4, backend_id: "local", backend_kind: "local", autonomy_mode: "supervised", approval_policy: "risk_based", environment: "system", network_policy: "host_inherited", container_image: null }, conversation_preferences: { delegation_enabled: true, auto_review: true, memory_enabled: true }, service_tier: {}, delegated_model: null, reviewer_model: null },
  references: [{ kind: "artifact", project_id: "p", id: "artifact" }], attachments: ["file"], attachment_receipts: [{ id: "file", project_id: "p", conversation_id: "c", name: "counts.csv", size_bytes: 4, media_type: "text/csv", relative_path: ".omicsops/attachments/file/counts.csv", sha256: "hash" }],
};
it("shows attention in the conversation header after a terminal run", async () => {
  render(<WorkspaceShell {...base} activeRunId="old" runStarted agentRunEventsV4={[{
    schema_version: 4, run_id: "old", project_id: "p", conversation_id: "c", sequence: 1,
    previous_hash: "", event_hash: "hash", occurred_at: "2026-09-14T00:00:00Z",
    event: { kind: "run_needs_attention", message: "context limit" },
  }]} queueItems={[{ ...queued, status: "failed" }]} onQueue={vi.fn()} />);
  expect(document.querySelector(".conversation-header .live-status")).toHaveTextContent("Needs attention");
  fireEvent.change(screen.getByRole("textbox", { name: /Describe/ }), { target: { value: "New work" } });
  await waitFor(() => expect(screen.getByRole("button", { name: "Send" })).toBeEnabled());
});
it("keeps accepted pending work visible as a queue send", async () => {
  render(<WorkspaceShell {...base} queueItems={[queued]} onQueue={vi.fn()} />);
  fireEvent.change(screen.getByRole("textbox", { name: /Describe/ }), { target: { value: "Fifth task" } });
  await waitFor(() => expect(screen.getByRole("button", { name: "Add to queue" })).toBeEnabled());
  expect(document.querySelector(".conversation-header .live-status")).toHaveTextContent("Queued");
});
it.each(["pending", "dispatching", "running", "completed", "failed", "cancelled"] as const)("keeps %s queue records out of the composer area", async (status) => {
  const queue = vi.fn().mockResolvedValue(true);
  const action = vi.fn();
  render(<WorkspaceShell {...base} onQueue={queue} queueItems={[{ ...queued, status }]} onQueueUpdate={vi.fn()} onQueueAction={action} />);
  expect(screen.queryByRole("region", { name: "Send queue" })).not.toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "Restore to composer" })).not.toBeInTheDocument();
  const input = screen.getByRole("textbox", { name: /Describe/ });
  expect(input).toBeEnabled();
  fireEvent.change(input, { target: { value: "Next analysis" } });
  await waitFor(() => expect(screen.getByRole("button", { name: /^(Send|Add to queue)$/ })).toBeEnabled());
  fireEvent.keyDown(input, { key: "Enter" });
  await waitFor(() => expect(queue).toHaveBeenCalledWith("Next analysis", "chat"));
  expect(input).toHaveValue("");
  expect(action).not.toHaveBeenCalled();
});

function stoppedEvents(effect: "runtime" | "read_only" = "runtime"): AgentRunEventV4[] {
  return [
    { kind: "tool_dispatch_started", call_id: "call", tool_id: "runtime.execute", effect, idempotency_key: "key" },
    { kind: "tool_dispatch_uncertain", call_id: "call", tool_id: "runtime.execute" },
    { kind: "run_needs_attention", message: "run stop left side-effect dispatches unresolved: call" },
  ].map((event, sequence) => ({ schema_version: 4, run_id: "r", project_id: "p", conversation_id: "c", sequence: sequence + 1,
    previous_hash: "", event_hash: `hash-${sequence}`, occurred_at: "2026-10-21T00:00:00Z", event })) as AgentRunEventV4[];
}

it.each(["running", "uncertain"] as const)("blocks a stopped unresolved run with a %s receipt without losing the draft", async (status) => {
  const queue = vi.fn(); const send = vi.fn();
  render(<WorkspaceShell {...base} onQueue={queue} onSend={send} agentRunEventsV4={stoppedEvents()} queueItems={[{ ...queued, status }]} />);
  const input = screen.getByRole("textbox", { name: /Describe/ });
  expect(input).toBeEnabled();
  fireEvent.change(input, { target: { value: "Check Python and R" } });
  expect(screen.queryByRole("button", { name: "Add to queue" })).not.toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Needs verification" })).toBeDisabled();
  expect(screen.getByRole("status", { name: "Sending blocked" })).toHaveTextContent(/draft is preserved/);
  expect(document.querySelector(".conversation-header .live-status")).toHaveTextContent("Needs attention");
  fireEvent.keyDown(input, { key: "Enter" });
  expect(queue).not.toHaveBeenCalled(); expect(send).not.toHaveBeenCalled();
  expect(input).toHaveValue("Check Python and R");
});

it("blocks an uncertain host receipt even when run history is unavailable", () => {
  render(<WorkspaceShell {...base} onQueue={vi.fn()} composerBlockingRunIds={[]} queueItems={[{ ...queued, status: "uncertain" }]} />);
  expect(screen.getByRole("button", { name: "Needs verification" })).toBeDisabled();
});

it("uses host clearance to send in the same conversation while retaining unresolved historical evidence", async () => {
  const queue = vi.fn().mockResolvedValue(true);
  const props = { ...base, onQueue: queue, composerBlockingRunIds: [], agentRunEventsV4: stoppedEvents(), queueItems: [{ ...queued, status: "failed" as const }] };
  const view = render(<WorkspaceShell {...props} />);
  const input = screen.getByRole("textbox", { name: /Describe/ });
  fireEvent.change(input, { target: { value: "A new check" } });
  await waitFor(() => expect(screen.getByRole("button", { name: "Send" })).toBeEnabled());
  expect(screen.queryByRole("status", { name: "Sending blocked" })).not.toBeInTheDocument();
  expect(screen.getByText("run stop left side-effect dispatches unresolved: call")).toBeInTheDocument();
  fireEvent.keyDown(input, { key: "Enter" });
  await waitFor(() => expect(queue).toHaveBeenCalledWith("A new check", "chat"));
  view.rerender(<WorkspaceShell {...props} activeRunId="new-run" runStarted agentRunEventsV4={[...stoppedEvents(), { ...stoppedEvents()[0], run_id: "new-run", event: { kind: "run_created", mode: "execute" } }]} />);
  expect(document.querySelector(".conversation-header .live-status")).toHaveTextContent("Running");
});

it("honors another blocking historical run reported by the host when its events are not loaded", () => {
  render(<WorkspaceShell {...base} onQueue={vi.fn()} composerBlockingRunIds={["other-old-run"]} queueItems={[{ ...queued, status: "failed" }]} />);
  expect(screen.getByRole("button", { name: "Needs verification" })).toBeDisabled();
});

it("keeps an unresolved legacy run blocked even with an invalid post-terminal resolution for another call", () => {
  const events = stoppedEvents();
  events.push({ ...events[2], sequence: 4, event_hash: "invalid", event: { kind: "tool_dispatch_resolved", call_id: "other-call", resolution: "side_effect_not_observed", evidence: "legacy record" } });
  render(<WorkspaceShell {...base} onQueue={vi.fn()} agentRunEventsV4={events} />);
  expect(screen.getByRole("button", { name: "Needs verification" })).toBeDisabled();
});

it("offers an explicit branch send while retaining the original draft until accepted", async () => {
  const queue = vi.fn(); const branch = vi.fn().mockResolvedValue(false);
  render(<WorkspaceShell {...base} onQueue={queue} onSend={vi.fn()} onOpenBranch={vi.fn()} onBranchSend={branch}
    messages={[{ id: "anchor", role: "user", markdown: "Previous check" }]} agentRunEventsV4={stoppedEvents()} queueItems={[{ ...queued, status: "uncertain" }]} />);
  const input = screen.getByRole("textbox", { name: /Describe/ });
  fireEvent.change(input, { target: { value: "Check Python and R" } });
  await waitFor(() => expect(screen.getByRole("button", { name: "Branch and send" })).toBeEnabled());
  fireEvent.click(screen.getByRole("button", { name: "Branch and send" }));
  await waitFor(() => expect(branch).toHaveBeenCalledWith("anchor", "Check Python and R", "chat", [], []));
  expect(queue).not.toHaveBeenCalled();
  expect(input).toHaveValue("Check Python and R");
});

it.each(["cancelled", "resolved", "read_only", "foreign_scope"] as const)("allows sending after %s without an unresolved local queue", async (kind) => {
  let events = stoppedEvents(kind === "read_only" ? "read_only" : "runtime");
  if (kind === "cancelled") events = [{ ...events[2], event: { kind: "run_cancelled" } }];
  if (kind === "resolved") events = [events[0], events[1], { ...events[2], event_hash: "resolved", event: { kind: "tool_dispatch_resolved", call_id: "call", resolution: "side_effect_not_observed", evidence: "verified" } }, { ...events[2], sequence: 4 }];
  if (kind === "foreign_scope") events = events.map((event) => ({ ...event, conversation_id: "elsewhere" }));
  const queue = vi.fn().mockResolvedValue(true);
  render(<WorkspaceShell {...base} onQueue={queue} agentRunEventsV4={events} queueItems={[{ ...queued, status: "failed" }, { ...queued, request_id: "foreign", conversation_id: "elsewhere", status: "uncertain" }]} />);
  const input = screen.getByRole("textbox", { name: /Describe/ });
  fireEvent.change(input, { target: { value: "New check" } });
  await waitFor(() => expect(screen.getByRole("button", { name: "Send" })).toBeEnabled());
  fireEvent.keyDown(input, { key: "Enter" });
  await waitFor(() => expect(queue).toHaveBeenCalledWith("New check", "chat"));
});
