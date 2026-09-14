import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, expect, it, vi } from "vitest";
import * as api from "../../composer-queue-api";
import { useComposerQueue } from "./useComposerQueue";
import type { ComposerQueueItemV4, EnqueueComposerTurnRequestV4 } from "../../types";
vi.mock("../../composer-queue-api", () => ({ reconcileComposerQueue: vi.fn(), enqueueComposerTurn: vi.fn(), updateComposerQueue: vi.fn(), actComposerQueue: vi.fn() }));
const input = { project_id: "p", conversation_id: "c", mode: "agent" as const, message_markdown: "inspect", model_profile_id: "model", compute_selection: { schema_version: 4 as const, backend_id: "local", backend_kind: "local" as const, autonomy_mode: "supervised" as const, approval_policy: "risk_based" as const, environment: "system", network_policy: "host_inherited" as const, container_image: null }, references: [], attachments: ["file"] };
function receipt(request: EnqueueComposerTurnRequestV4): ComposerQueueItemV4 { return { ...request, frozen: { model_profile_id: request.model_profile_id, model_configuration_hash: "hash", compute_selection: request.compute_selection, conversation_preferences: { delegation_enabled: true, auto_review: true, memory_enabled: true }, service_tier: {}, reviewer_model: null, delegated_model: null }, attachment_receipts: [], position: 1, revision: 1, status: "pending", created_at: "now", updated_at: "now" }; }
beforeEach(() => { vi.clearAllMocks(); vi.mocked(api.reconcileComposerQueue).mockResolvedValue([]); vi.mocked(api.enqueueComposerTurn).mockImplementation(async (request) => receipt(request)); });
it("keeps all three reserved IDs after an unknown enqueue response", async () => {
  vi.mocked(api.enqueueComposerTurn).mockRejectedValueOnce(new Error("lost response"));
  const { result } = renderHook(() => useComposerQueue("p", "c", true));
  await waitFor(() => expect(result.current.loading).toBe(false));
  await act(async () => { expect(await result.current.enqueue(input)).toBe(false); });
  const original = vi.mocked(api.enqueueComposerTurn).mock.calls[0][0];
  await act(async () => { expect(await result.current.enqueue(input)).toBe(true); });
  expect(vi.mocked(api.enqueueComposerTurn).mock.calls[1][0]).toEqual(original);
  expect(result.current.items[0].attachments).toEqual(["file"]);
});
it("reconciles a saved pending request without invoking enqueue again", async () => {
  vi.mocked(api.enqueueComposerTurn).mockRejectedValueOnce(new Error("lost response"));
  const { result } = renderHook(() => useComposerQueue("p", "c", true));
  await waitFor(() => expect(result.current.loading).toBe(false));
  await act(async () => { await result.current.enqueue(input); });
  vi.mocked(api.reconcileComposerQueue).mockResolvedValue([receipt(vi.mocked(api.enqueueComposerTurn).mock.calls[0][0])]);
  await act(async () => { await result.current.refresh(); });
  await act(async () => { expect(await result.current.enqueue(input)).toBe(true); });
  expect(api.enqueueComposerTurn).toHaveBeenCalledTimes(1);
  await act(async () => { await result.current.enqueue(input); });
  expect(api.enqueueComposerTurn).toHaveBeenCalledTimes(2);
});
it("does not poll or accept work before conversation hydration finishes", async () => {
  const { result } = renderHook(() => useComposerQueue("p", "c", false));
  await act(async () => { expect(await result.current.enqueue(input)).toBe(false); });
  expect(api.reconcileComposerQueue).not.toHaveBeenCalled();
  expect(api.enqueueComposerTurn).not.toHaveBeenCalled();
});
it("does not put an old scope response into the newly selected conversation", async () => {
  let resolve!: (value: ComposerQueueItemV4[]) => void;
  vi.mocked(api.reconcileComposerQueue).mockImplementationOnce(() => new Promise((yes) => { resolve = yes; }));
  const { result, rerender } = renderHook(({ id }) => useComposerQueue("p", id, true), { initialProps: { id: "c" } });
  rerender({ id: "other" });
  await act(async () => { resolve([receipt({ ...input, request_id: "q", message_id: "m", run_id: "r" })]); });
  expect(result.current.items).toEqual([]);
});

it("retries conversation recovery after a failed refresh even when queue status stays unchanged", async () => {
  const running = { ...receipt({ ...input, request_id: "q", message_id: "m", run_id: "r" }), status: "running" as const };
  vi.mocked(api.reconcileComposerQueue).mockResolvedValue([running]);
  const recover = vi.fn().mockRejectedValueOnce(new Error("temporary read failure")).mockResolvedValue(undefined);
  const { result } = renderHook(() => useComposerQueue("p", "c", true, recover));
  await waitFor(() => expect(result.current.error).toBe(true));
  await act(async () => { await result.current.refresh(); });
  expect(recover).toHaveBeenCalledTimes(2);
  await act(async () => { await result.current.refresh(); });
  expect(recover).toHaveBeenCalledTimes(2);
  expect(result.current.error).toBe(false);
});

it("preserves the draft if an unconfirmed request was cancelled in another window", async () => {
  vi.mocked(api.enqueueComposerTurn).mockRejectedValueOnce(new Error("lost response"));
  const { result } = renderHook(() => useComposerQueue("p", "c", true));
  await waitFor(() => expect(result.current.loading).toBe(false));
  await act(async () => { await result.current.enqueue(input); });
  const original = vi.mocked(api.enqueueComposerTurn).mock.calls[0][0];
  vi.mocked(api.reconcileComposerQueue).mockResolvedValue([{ ...receipt(original), status: "cancelled" }]);
  await act(async () => { await result.current.refresh(); });
  await act(async () => { expect(await result.current.enqueue(input)).toBe(false); });
  expect(api.enqueueComposerTurn).toHaveBeenCalledTimes(1);
  await act(async () => { expect(await result.current.enqueue(input)).toBe(true); });
  expect(vi.mocked(api.enqueueComposerTurn).mock.calls[1][0].request_id).not.toBe(original.request_id);
});

it("recognizes an already delivered guidance receipt without enqueueing the original draft twice", async () => {
  vi.mocked(api.enqueueComposerTurn).mockRejectedValueOnce(new Error("lost response"));
  const { result } = renderHook(() => useComposerQueue("p", "c", true));
  await waitFor(() => expect(result.current.loading).toBe(false));
  const textOnly = { ...input, attachments: [] };
  await act(async () => { await result.current.enqueue(textOnly); });
  const original = vi.mocked(api.enqueueComposerTurn).mock.calls[0][0];
  vi.mocked(api.reconcileComposerQueue).mockResolvedValue([{ ...receipt(original), status: "cancelled", cut_in_message_id: "guidance" }]);
  await act(async () => { await result.current.refresh(); });
  await act(async () => expect(await result.current.enqueue(textOnly)).toBe(true));
  expect(api.enqueueComposerTurn).toHaveBeenCalledTimes(1);
});
