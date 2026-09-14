import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, expect, it, vi } from "vitest";
import { WorkspaceShell } from "./WorkspaceShell";
import * as api from "../../conversation-branch-api";
vi.mock("../../conversation-branch-api", () => ({ getConversationBranch: vi.fn(), conversationBranchCheckpoint: vi.fn(), createConversationBranch: vi.fn() }));
const checkpoint = { source_message_id: "u1", source_sequence: 1, source_head_sequence: 2, checkpoint_kind: "after_response" as const, boundary_hash: "hash" };
const branch = { ...checkpoint, request_id: "q", branch_conversation_id: "b1", project_id: "p1", source_conversation_id: "c1", request_hash: "request-hash", state: "active" as const, created_at: "now", updated_at: "now" };
const base = { project: { id: "p1", name: "test", status: "ready" as const, template: "blank" as const }, locale: "en-US" as const, onLocaleChange: vi.fn(), activeConversationId: "c1", messages: [{ id: "u1", role: "user" as const, markdown: "First task" }, { id: "a1", role: "assistant" as const, markdown: "First response" }] };
beforeEach(() => { vi.clearAllMocks(); vi.mocked(api.getConversationBranch).mockResolvedValue(null); vi.mocked(api.conversationBranchCheckpoint).mockResolvedValue(checkpoint); vi.mocked(api.createConversationBranch).mockResolvedValue(branch); });
it("uses composer text only as the branch title without sending it", async () => {
  const send = vi.fn(); const open = vi.fn().mockResolvedValue(true);
  render(<WorkspaceShell {...base} onSend={send} onOpenBranch={open} />);
  fireEvent.change(screen.getByRole("textbox", { name: /Describe/ }), { target: { value: "Alternative analysis" } });
  fireEvent.click(screen.getByRole("button", { name: "Send options" }));
  fireEvent.click(screen.getByRole("menuitem", { name: /Branch conversation/ }));
  await waitFor(() => expect(open).toHaveBeenCalledWith(branch));
  expect(api.conversationBranchCheckpoint).toHaveBeenCalledWith("p1", "c1", "u1", "after_response");
  expect(api.createConversationBranch).toHaveBeenCalledWith(expect.objectContaining({ title: "Alternative analysis", source_message_id: "u1" }));
  expect(send).not.toHaveBeenCalled();
});
it("resolves assistant branch actions to the preceding user turn", async () => {
  render(<WorkspaceShell {...base} onOpenBranch={vi.fn().mockResolvedValue(true)} />);
  fireEvent.click(screen.getByRole("button", { name: "Branch after this response" }));
  await waitFor(() => expect(api.conversationBranchCheckpoint).toHaveBeenCalledWith("p1", "c1", "u1", "after_response"));
});
it("keeps branch actions disabled while the conversation is active", () => {
  render(<WorkspaceShell {...base} agentBusy onOpenBranch={vi.fn()} />);
  expect(screen.getByRole("button", { name: "Branch before this message" })).toBeDisabled();
  fireEvent.click(screen.getByRole("button", { name: "Send options" }));
  expect(screen.getByRole("menuitem", { name: /Branch conversation/ })).toBeDisabled();
  expect(api.createConversationBranch).not.toHaveBeenCalled();
});
it("restores persisted source navigation when opening a branch", async () => {
  vi.mocked(api.getConversationBranch).mockResolvedValue(branch);
  const select = vi.fn();
  render(<WorkspaceShell {...base} activeConversationId="b1" onOpenBranch={vi.fn()} onSelectConversation={select} />);
  fireEvent.click(await screen.findByRole("button", { name: "Open source conversation" }));
  expect(select).toHaveBeenCalledWith("c1");
});

it("sends the composer draft through branch-and-send when the native flow is available", async () => {
  const branchSend = vi.fn().mockResolvedValue(true); const normalSend = vi.fn();
  const computeBackends = [{ descriptor: { schema_version: 4 as const, backend_id: "local", kind: "local" as const, isolation: "process" as const, available: true, supports_python: true, supports_r: true, supports_network_policy: false }, selectable: true, reason: null, python_status: "available" as const, r_status: "available" as const, resolved_image_id: null }];
  render(<WorkspaceShell {...base} computeBackendId="local" computeBackends={computeBackends} onSend={normalSend} onOpenBranch={vi.fn()} onBranchSend={branchSend} />);
  const input = screen.getByRole("textbox", { name: /Describe/ });
  fireEvent.change(input, { target: { value: "Explore an alternative" } });
  fireEvent.click(screen.getByRole("button", { name: "Send options" }));
  fireEvent.click(screen.getByRole("menuitem", { name: /Branch conversation/ }));
  await waitFor(() => expect(branchSend).toHaveBeenCalledWith("u1", "Explore an alternative", "chat", [], []));
  expect(normalSend).not.toHaveBeenCalled(); expect(api.createConversationBranch).not.toHaveBeenCalled();
  expect(input).toHaveValue("");
});

it("restores the selected user turn as a draft after message-level branching", async () => {
  render(<WorkspaceShell {...base} onOpenBranch={vi.fn().mockResolvedValue(true)} />);
  fireEvent.click(screen.getByRole("button", { name: "Branch before this message" }));
  await waitFor(() => expect(screen.getByRole("textbox", { name: /Describe/ })).toHaveValue("First task"));
});

it("preserves the new scope draft after a delayed branch-and-send receipt", async () => {
  let finish!: (accepted: boolean) => void;
  const branchSend = vi.fn(() => new Promise<boolean>((resolve) => { finish = resolve; }));
  const computeBackends = [{ descriptor: { schema_version: 4 as const, backend_id: "local", kind: "local" as const, isolation: "process" as const, available: true, supports_python: true, supports_r: true, supports_network_policy: false }, selectable: true, reason: null, python_status: "available" as const, r_status: "available" as const, resolved_image_id: null }];
  const props = { ...base, computeBackendId: "local", computeBackends, onOpenBranch: vi.fn(), onBranchSend: branchSend };
  const view = render(<WorkspaceShell {...props} />);
  fireEvent.change(screen.getByRole("textbox", { name: /Describe/ }), { target: { value: "Same draft" } });
  fireEvent.click(screen.getByRole("button", { name: "Send options" }));
  fireEvent.click(screen.getByRole("menuitem", { name: /Branch conversation/ }));
  view.rerender(<WorkspaceShell {...props} activeConversationId="other" />);
  fireEvent.change(screen.getByRole("textbox", { name: /Describe/ }), { target: { value: "Same draft" } });
  await act(async () => { finish(true); });
  expect(screen.getByRole("textbox", { name: /Describe/ })).toHaveValue("Same draft");
});
