import { act, renderHook } from "@testing-library/react";
import { expect, it, vi } from "vitest";
import * as api from "../../composer-queue-api";
import { useComposerReplacement } from "./useComposerReplacement";
import type { ReplaceComposerTurnRequestV4, ComposerReplacementReceiptV4 } from "../../types";
vi.mock("../../composer-queue-api", () => ({ replaceComposerTurn: vi.fn() }));
const input = { project_id: "p", conversation_id: "c", mode: "agent" as const, message_markdown: "replace", model_profile_id: "model", compute_selection: { schema_version: 4 as const, backend_id: "local", backend_kind: "local" as const, autonomy_mode: "supervised" as const, approval_policy: "risk_based" as const, environment: "system", network_policy: "host_inherited" as const, container_image: null }, references: [], attachments: ["file"] };
const target = { run_id: "old-run", sequence: 3, event_hash: "hash" };
function receipt(request: ReplaceComposerTurnRequestV4): ComposerReplacementReceiptV4 {
  return { request_id: request.turn.request_id, project_id: "p", conversation_id: "c", target_run_id: request.target_run_id, source_event_sequence: request.expected_event_sequence, source_event_hash: request.expected_event_hash, source_message_id: null, stop: { request_id: request.stop_request_id, project_id: "p", conversation_id: "c", run_id: request.target_run_id, status: "requested", created_at: "now", updated_at: "now" }, accepted_at: "now" };
}
it("retains the entire unknown replacement and reconciles the same target and IDs", async () => {
  vi.mocked(api.replaceComposerTurn).mockReset().mockRejectedValueOnce(new Error("lost"));
  const { result } = renderHook(() => useComposerReplacement("p", "c", true));
  await act(async () => { expect(await result.current.send(input, target)).toBe(false); });
  const original = vi.mocked(api.replaceComposerTurn).mock.calls[0][0];
  expect(original.turn.attachments).toEqual(["file"]);
  expect(result.current.pending).toBe(true);
  await act(async () => { expect(await result.current.send({ ...input, message_markdown: "new draft" }, { ...target, run_id: "new-run" })).toBe(false); });
  expect(api.replaceComposerTurn).toHaveBeenCalledTimes(1);
  vi.mocked(api.replaceComposerTurn).mockImplementation(async (request) => receipt(request));
  await act(async () => { expect(await result.current.retry()).toBe(true); });
  expect(vi.mocked(api.replaceComposerTurn).mock.calls[1][0]).toEqual(original);
  expect(result.current.pending).toBe(false);
});
it("allows a fresh explicitly requested replacement after definite rejection", async () => {
  vi.mocked(api.replaceComposerTurn).mockReset().mockRejectedValueOnce({ kind: "rejected" });
  const { result } = renderHook(() => useComposerReplacement("p", "c", true));
  await act(async () => { await result.current.send(input, target); });
  expect(result.current.pending).toBe(false);
  vi.mocked(api.replaceComposerTurn).mockImplementation(async (request) => receipt(request));
  await act(async () => { await result.current.send(input, { ...target, sequence: 4 }); });
  expect(vi.mocked(api.replaceComposerTurn).mock.calls[1][0].turn.request_id).not.toBe(vi.mocked(api.replaceComposerTurn).mock.calls[0][0].turn.request_id);
});
it("keeps a late failed request in its original conversation", async () => {
  let reject!: (error: unknown) => void;
  vi.mocked(api.replaceComposerTurn).mockReset().mockImplementation(() => new Promise((_, no) => { reject = no; }));
  const { result, rerender } = renderHook(({ id }) => useComposerReplacement("p", id, true), { initialProps: { id: "c" } });
  let work!: Promise<boolean>;
  act(() => { work = result.current.send(input, target); });
  rerender({ id: "other" });
  await act(async () => { reject(new Error("lost")); await work; });
  expect(result.current.pending).toBe(false);
  expect(result.current.error).toBe(false);
  rerender({ id: "c" });
  expect(result.current.pending).toBe(true);
});
