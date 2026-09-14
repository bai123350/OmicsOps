import type { ComposerQueueItemV4 } from "../../types";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { expect, it, vi } from "vitest";
import { WorkspaceShell } from "./WorkspaceShell";
const base = { project: { id: "p", name: "test", status: "ready" as const, template: "blank" as const }, locale: "en-US" as const, onLocaleChange: vi.fn(), activeConversationId: "c", computeBackendId: "local", computeBackends: [{ descriptor: { schema_version: 4 as const, backend_id: "local", kind: "local" as const, isolation: "process" as const, available: true, supports_python: true, supports_r: true, supports_network_policy: false }, selectable: true, reason: null, python_status: "available" as const, r_status: "available" as const, resolved_image_id: null }] };
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
it("restores the complete queued payload only after cancellation is confirmed", async () => {
  const action = vi.fn().mockResolvedValue(undefined);
  render(<WorkspaceShell {...base} onQueue={vi.fn()} queueItems={[queued]} onQueueUpdate={vi.fn()} onQueueAction={action} />);
  fireEvent.click(screen.getByRole("button", { name: "Restore to composer" }));
  await waitFor(() => expect(screen.getByRole("textbox", { name: /Describe/ })).toHaveValue("Restore this task"));
  expect(action).toHaveBeenCalledWith(expect.objectContaining({ action: "cancel", request_id: "q", expected_revision: 1 }));
  expect(screen.getByText("counts.csv")).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Remove reference: artifact" })).toBeInTheDocument();
});
it("does not restore on an unknown cancel response or overwrite a new draft", async () => {
  const action = vi.fn().mockRejectedValue(new Error("lost response"));
  render(<WorkspaceShell {...base} onQueue={vi.fn()} queueItems={[queued]} onQueueUpdate={vi.fn()} onQueueAction={action} />);
  fireEvent.click(screen.getByRole("button", { name: "Restore to composer" }));
  await screen.findByRole("alert");
  expect(screen.getByRole("textbox", { name: /Describe/ })).toHaveValue("");
  fireEvent.change(screen.getByRole("textbox", { name: /Describe/ }), { target: { value: "A different draft" } });
  expect(screen.getByRole("button", { name: "Restore to composer" })).toBeDisabled();
});
