import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, expect, it, vi } from "vitest";
import * as api from "../../conversation-branch-api";
import { useConversationBranchSend } from "./useConversationBranchSend";
import type { CreateConversationBranchAndSendRequestV4 } from "../../types";
vi.mock("../../conversation-branch-api", () => ({ conversationBranchCheckpoint: vi.fn(), createConversationBranchAndSend: vi.fn() }));
const input = { sourceMessageId: "u", message_markdown: "branch task", mode: "agent" as const, model_profile_id: "model", compute_selection: { schema_version: 4 as const, backend_id: "local", backend_kind: "local" as const, autonomy_mode: "supervised" as const, approval_policy: "risk_based" as const, environment: "system", network_policy: "host_inherited" as const, container_image: null }, references: [{ kind: "artifact" as const, project_id: "p", id: "a" }], attachments: ["file"] };
function receipt(request: CreateConversationBranchAndSendRequestV4) { return { branch: { ...request.branch, branch_conversation_id: "branch", source_sequence: 1, source_head_sequence: 2, boundary_hash: "hash", request_hash: "hash", state: "active" as const, created_at: "now", updated_at: "now" }, queue: { message_markdown: request.message_markdown, mode: request.mode, references: request.references, attachments: request.attachments, attachment_receipts: [], position: 1, revision: 1, status: "pending", created_at: "now", updated_at: "now", frozen: { model_profile_id: request.model_profile_id, model_configuration_hash: "hash", compute_selection: request.compute_selection, conversation_preferences: { delegation_enabled: true, auto_review: true, memory_enabled: true }, service_tier: {}, delegated_model: null, reviewer_model: null }, project_id: request.branch.project_id, message_id: request.queue_message_id, run_id: request.queue_run_id, request_id: request.queue_request_id, conversation_id: "branch" } } as Awaited<ReturnType<typeof api.createConversationBranchAndSend>>; }
beforeEach(() => { vi.clearAllMocks(); vi.mocked(api.conversationBranchCheckpoint).mockResolvedValue({ source_message_id: "u", source_sequence: 1, source_head_sequence: 2, checkpoint_kind: "after_response", boundary_hash: "hash" }); vi.mocked(api.createConversationBranchAndSend).mockImplementation(async (request) => receipt(request)); });
it("retries unknown branch-send outcomes with identical IDs and the complete original payload", async () => {
  vi.mocked(api.createConversationBranchAndSend).mockRejectedValueOnce(new Error("response lost"));
  const opened = vi.fn().mockResolvedValue(true); const { result } = renderHook(() => useConversationBranchSend("p", "c", opened));
  await act(async () => expect(await result.current.submit(input)).toBe(false));
  const first = vi.mocked(api.createConversationBranchAndSend).mock.calls[0][0];
  await act(async () => expect(await result.current.retry()).toBe(true));
  expect(vi.mocked(api.createConversationBranchAndSend).mock.calls[1][0]).toEqual(first);
  expect(first.attachments).toEqual(["file"]); expect(first.references).toEqual(input.references);
  expect(api.conversationBranchCheckpoint).toHaveBeenCalledTimes(1);
});
it("does not enqueue again after the committed branch could not be opened", async () => {
  const opened = vi.fn().mockRejectedValueOnce(new Error("list failed")).mockResolvedValue(true);
  const { result } = renderHook(() => useConversationBranchSend("p", "c", opened));
  await act(async () => expect(await result.current.submit(input)).toBe(false));
  await act(async () => expect(await result.current.retry()).toBe(true));
  expect(api.createConversationBranchAndSend).toHaveBeenCalledTimes(1);
});
it("keeps long markdown intact while bounding only the title", async () => {
  const { result } = renderHook(() => useConversationBranchSend("p", "c", vi.fn().mockResolvedValue(true)));
  const markdown = "分析".repeat(1000);
  await act(async () => { await result.current.submit({ ...input, message_markdown: markdown }); });
  const request = vi.mocked(api.createConversationBranchAndSend).mock.calls[0][0];
  expect(request.message_markdown).toBe(markdown);
  expect(new TextEncoder().encode(request.branch.title).length).toBeLessThanOrEqual(240);
});

it("retains a committed receipt across scope switches without navigating or sending twice", async () => {
  let resolve!: (value: Awaited<ReturnType<typeof api.createConversationBranchAndSend>>) => void;
  vi.mocked(api.createConversationBranchAndSend).mockImplementation(() => new Promise((yes) => { resolve = yes; }));
  const opened = vi.fn().mockResolvedValue(true);
  const { result, rerender } = renderHook(({ id }) => useConversationBranchSend("p", id, opened), { initialProps: { id: "c" } });
  let work!: Promise<boolean>;
  act(() => { work = result.current.submit(input); });
  await waitFor(() => expect(api.createConversationBranchAndSend).toHaveBeenCalledTimes(1));
  rerender({ id: "other" });
  await act(async () => { resolve(receipt(vi.mocked(api.createConversationBranchAndSend).mock.calls[0][0])); expect(await work).toBe(false); });
  expect(opened).not.toHaveBeenCalled();
  rerender({ id: "c" });
  await act(async () => expect(await result.current.retry()).toBe(true));
  expect(opened).toHaveBeenCalledTimes(1); expect(api.createConversationBranchAndSend).toHaveBeenCalledTimes(1);
});
