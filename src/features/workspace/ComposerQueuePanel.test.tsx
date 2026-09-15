import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { ComposerQueueItemV4 } from "../../types";
import { ComposerQueuePanel } from "./ComposerQueuePanel";

const item: ComposerQueueItemV4 = {
  request_id: "q1", message_id: "m1", run_id: "r1", project_id: "p1", conversation_id: "c1",
  mode: "agent", message_markdown: "Inspect counts", frozen: { model_profile_id: "model1", model_configuration_hash: "a".repeat(64), conversation_preferences: { delegation_enabled: true, auto_review: true, memory_enabled: true }, service_tier: { fast_mode: null }, delegated_model: null, reviewer_model: null,
  compute_selection: { schema_version: 4, backend_id: "local", backend_kind: "local", autonomy_mode: "supervised", approval_policy: "risk_based", environment: "system", network_policy: "host_inherited", container_image: null } }, attachment_receipts: [],
  references: [{ kind: "artifact", project_id: "p1", id: "a1" }], attachments: ["file1"],
  position: 1, revision: 3, status: "pending", created_at: "2026-09-14T00:00:00Z", updated_at: "2026-09-14T00:00:00Z",
};
const props = () => ({ projectId: "p1", conversationId: "c1", locale: "en-US" as const, items: [item], onUpdate: vi.fn().mockResolvedValue(undefined), onAction: vi.fn().mockResolvedValue(undefined) });

it("explains replacement cancellation and prevents edits or priority changes", () => {
  render(<ComposerQueuePanel {...props()} items={[{ ...item, replacement_target_run_id: "old" }, { ...item, request_id: "ordinary", position: 2 }]} cutInAvailable />);
  expect(screen.getByText(/Cancelling this message does not undo Stop/)).toBeInTheDocument();
  expect(screen.getAllByRole("button", { name: "Edit queued message" })[0]).toBeDisabled();
  expect(screen.getAllByRole("button", { name: "Move queued message down" })[0]).toBeDisabled();
  expect(screen.getAllByRole("button", { name: "Move queued message up" })[1]).toBeDisabled();
  expect(screen.getAllByRole("button", { name: "Cancel queued message" })[0]).toBeEnabled();
});

it("shows the durable source and Stop receipt from a restored queue row", () => {
  render(<ComposerQueuePanel {...props()} items={[{ ...item, replacement_target_run_id: "old-run", replacement_receipt: { request_id: item.request_id, project_id: item.project_id, conversation_id: item.conversation_id, target_run_id: "old-run", source_message_id: "source-message", source_event_sequence: 3, source_event_hash: "source-hash", accepted_at: "now", stop: { request_id: "durable-stop", project_id: item.project_id, conversation_id: item.conversation_id, run_id: "old-run", status: "requested", created_at: "now", updated_at: "now" } } }]} />);
  fireEvent.click(screen.getByText("Accepted replacement receipt"));
  expect(screen.getByText(/Stop ID: durable-stop/)).toBeInTheDocument();
  expect(screen.getByText(/SHA-256: source-hash/)).toBeInTheDocument();
  expect(screen.getByText(/Source message: source-message/)).toBeInTheDocument();
});

describe("durable composer queue panel", () => {
  it("edits text with the original full material and revision", async () => {
    const p = props(); render(<ComposerQueuePanel {...p} />);
    fireEvent.click(screen.getByRole("button", { name: "Edit queued message" }));
    fireEvent.change(screen.getByRole("textbox", { name: "Queued message" }), { target: { value: "Inspect corrected counts" } });
    fireEvent.click(screen.getByRole("button", { name: "Save changes" }));
    await waitFor(() => expect(p.onUpdate).toHaveBeenCalledWith({ project_id: "p1", conversation_id: "c1", request_id: "q1", expected_revision: 3, message_markdown: "Inspect corrected counts", references: item.references, attachments: item.attachments }));
  });
  it("closes the editor on window Escape immediately and restores its trigger", () => {
    render(<ComposerQueuePanel {...props()} />);
    const edit = screen.getByRole("button", { name: "Edit queued message" });
    fireEvent.click(edit);
    fireEvent.keyDown(window, { key: "Escape" });
    expect(screen.queryByRole("textbox")).toBeNull();
    expect(edit).toHaveFocus();
    expect(screen.getByText("Inspect counts")).toBeInTheDocument();
  });
  it("retains a rejected draft through close/reopen and never shows raw host errors", async () => {
    const p = props(); p.onUpdate.mockRejectedValue(new Error("credential=private"));
    render(<ComposerQueuePanel {...p} />);
    fireEvent.click(screen.getByRole("button", { name: "Edit queued message" }));
    fireEvent.change(screen.getByRole("textbox"), { target: { value: "retained correction" } });
    fireEvent.click(screen.getByRole("button", { name: "Save changes" }));
    await screen.findByRole("alert");
    expect(screen.queryByText(/credential=private/)).toBeNull();
    fireEvent.keyDown(window, { key: "Escape" });
    fireEvent.click(screen.getByRole("button", { name: "Edit queued message" }));
    expect(screen.getByRole("textbox")).toHaveValue("retained correction");
  });
  it("sends scoped CAS actions and does not offer editing for running rows", async () => {
    const p = props(); render(<ComposerQueuePanel {...p} items={[item, { ...item, request_id: "q2", run_id: "r2", message_id: "m2", position: 2, status: "running" }]} />);
    expect(screen.getAllByRole("button", { name: "Edit queued message" })).toHaveLength(1);
    fireEvent.click(screen.getByRole("button", { name: "Cancel queued message" }));
    await waitFor(() => expect(p.onAction).toHaveBeenCalledWith({ project_id: "p1", conversation_id: "c1", request_id: "q1", expected_revision: 3, action: "cancel" }));
  });
  it("keeps rejected edits and drops stale errors after switching scope", async () => {
    let reject!: (error: Error) => void;
    const p = props(); p.onUpdate.mockImplementation(() => new Promise((_, no) => { reject = no; }));
    const { rerender } = render(<ComposerQueuePanel {...p} />);
    fireEvent.click(screen.getByRole("button", { name: "Edit queued message" }));
    fireEvent.change(screen.getByRole("textbox"), { target: { value: "new draft" } });
    fireEvent.click(screen.getByRole("button", { name: "Save changes" }));
    rerender(<ComposerQueuePanel {...p} conversationId="c2" items={[]} />);
    await act(async () => reject(new Error("private backend details")));
    expect(screen.queryByRole("alert")).toBeNull();
    expect(screen.queryByText("Inspect counts")).toBeNull();
  });
});

it("allows only plain-text guidance cut-in and marks its receipt separately from cancellation", async () => {
  const p = props(); const restore = vi.fn();
  const view = render(<ComposerQueuePanel {...p} cutInAvailable onRestore={restore} />);
  expect(screen.getByRole("button", { name: "Send as guidance" })).toBeDisabled();
  view.rerender(<ComposerQueuePanel {...p} items={[{ ...item, attachments: [], references: [] }]} cutInAvailable onRestore={restore} />);
  fireEvent.click(screen.getByRole("button", { name: "Send as guidance" }));
  await waitFor(() => expect(p.onAction).toHaveBeenCalledWith(expect.objectContaining({ action: "cut_in", request_id: "q1", expected_revision: 3 })));
  view.rerender(<ComposerQueuePanel {...p} items={[{ ...item, status: "cancelled", cut_in_message_id: "guidance" }]} onRestore={restore} />);
  expect(screen.getByText("Delivered as guidance")).toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "Restore to composer" })).toBeNull();
});

it("restores keyboard focus after a successful asynchronous edit", async () => {
  render(<ComposerQueuePanel {...props()} />);
  const edit = screen.getByRole("button", { name: "Edit queued message" });
  fireEvent.click(edit);
  fireEvent.change(screen.getByRole("textbox"), { target: { value: "Edited" } });
  fireEvent.click(screen.getByRole("button", { name: "Save changes" }));
  await waitFor(() => expect(screen.queryByRole("textbox")).toBeNull());
  await waitFor(() => expect(edit).toHaveFocus());
});

it("offers failed payload recovery without silently retrying execution", async () => {
  const restore = vi.fn().mockResolvedValue(undefined); const p = props();
  const failed = { ...item, status: "failed" as const, failure_code: "configuration_changed" as const };
  render(<ComposerQueuePanel {...p} items={[failed]} onRestore={restore} />);
  fireEvent.click(screen.getByRole("button", { name: "Restore to composer" }));
  await waitFor(() => expect(restore).toHaveBeenCalledWith(failed));
  expect(p.onAction).not.toHaveBeenCalled();
});

it("keeps oversized guidance in the queue for editing", () => {
  render(<ComposerQueuePanel {...props()} items={[{ ...item, attachments: [], references: [], message_markdown: "分析".repeat(400) }]} cutInAvailable />);
  expect(screen.getByRole("button", { name: "Send as guidance" })).toBeDisabled();
  expect(screen.getByRole("button", { name: "Edit queued message" })).toBeEnabled();
});
