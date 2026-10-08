import { act, renderHook } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import * as api from "../../context-usage-api";
import { useContextCompaction } from "./useContextCompaction";
import type { CompactContextRequestV4, ContextCompactionReceiptV4 } from "../../types";
const scope: CompactContextRequestV4 = { request_id: "operation", project_id: "p", conversation_id: "c", run_id: "r" };
const receipt: ContextCompactionReceiptV4 = { ...scope, status: "completed", source_through_sequence: 4, source_head_hash: "hash", before_bytes: 9000, after_bytes: 1000, created_at: "now", updated_at: "now" };
afterEach(() => vi.restoreAllMocks());
function deferred<T>() { let resolve!: (value: T) => void; const promise = new Promise<T>((r) => { resolve = r; }); return { promise, resolve }; }
it("manual_compact_duplicate_click_reuses_request_id", async () => {
  const pending = deferred<ContextCompactionReceiptV4>();
  const compact = vi.spyOn(api, "compactContextV4").mockReturnValue(pending.promise);
  const { result } = renderHook(() => useContextCompaction(scope, true));
  let first!: Promise<void>;
  act(() => { first = result.current.compact(); void result.current.compact(); });
  expect(result.current.busy).toBe(true); expect(compact).toHaveBeenCalledTimes(1);
  expect(compact).toHaveBeenCalledWith(scope);
  await act(async () => { pending.resolve(receipt); await first; });
  expect(result.current.receipt).toEqual(receipt); expect(result.current.busy).toBe(false);
});
it("retries an uncertain failure with the same ID and never exposes raw errors", async () => {
  const compact = vi.spyOn(api, "compactContextV4").mockRejectedValueOnce(new Error("SECRET")).mockResolvedValue(receipt);
  const { result } = renderHook(() => useContextCompaction(scope, true));
  await act(() => result.current.compact()); expect(result.current.error).toBe(true);
  await act(() => result.current.compact()); expect(compact.mock.calls.map(([r]) => r.request_id)).toEqual(["operation", "operation"]);
  expect(result.current.error).toBe(false); expect(result.current.receipt).toEqual(receipt);
});
it("discards late responses even after leaving and returning to the same scope", async () => {
  const pending = deferred<ContextCompactionReceiptV4>();
  const compact = vi.spyOn(api, "compactContextV4").mockReturnValue(pending.promise);
  const { result, rerender } = renderHook(({ current }) => useContextCompaction(current, true), { initialProps: { current: scope } });
  act(() => { void result.current.compact(); });
  rerender({ current: { ...scope, conversation_id: "other" } });
  expect(result.current.busy).toBe(false); expect(result.current.receipt).toBeNull();
  rerender({ current: scope });
  act(() => { void result.current.compact(); }); expect(compact).toHaveBeenCalledTimes(1);
  await act(async () => pending.resolve(receipt));
  expect(result.current.receipt).toBeNull(); expect(result.current.busy).toBe(false);
});
it("refuses disabled calls and mismatched host receipts", async () => {
  const compact = vi.spyOn(api, "compactContextV4").mockResolvedValue({ ...receipt, run_id: "other" });
  const { result, rerender } = renderHook(({ enabled }) => useContextCompaction(scope, enabled), { initialProps: { enabled: false } });
  await act(() => result.current.compact()); expect(compact).not.toHaveBeenCalled();
  rerender({ enabled: true }); await act(() => result.current.compact());
  expect(result.current.error).toBe(true); expect(result.current.receipt).toBeNull();
});
import { useContextUsage } from "./useContextUsage";
import type { ContextUsageSnapshotV4 } from "../../types";
it("explicit post-compaction refresh waits for old usage then reads the new snapshot", async () => {
  const counter = { known: 0, incomplete_attempts: 0 };
  const snapshot: ContextUsageSnapshotV4 = { project_id: "p", conversation_id: "c", observed_total: { input_tokens: counter, output_tokens: counter, reasoning_tokens: counter, cache_read_input_tokens: counter, cache_creation_input_tokens: counter, reported_total_tokens: counter, observed_attempts: 0, final_attempts: 0, partial_attempts: 0, interrupted_attempts: 0, unknown_attempts: 0 }, current_context: { limit_source: { kind: "unknown" }, estimated: false }, conservative_budget: { serialized_request_bytes: 9000, host_context_max_bytes: 100000, image_count: 0 } };
  const old = deferred<ContextUsageSnapshotV4>();
  const read = vi.spyOn(api, "getContextUsage").mockReturnValueOnce(old.promise).mockResolvedValue({ ...snapshot, conservative_budget: { ...snapshot.conservative_budget, serialized_request_bytes: 1000 } });
  const { result } = renderHook(() => useContextUsage("p", "c", true));
  let refresh!: Promise<void>;
  act(() => { refresh = result.current.refresh(); });
  await act(async () => { old.resolve(snapshot); await refresh; });
  expect(read).toHaveBeenCalledTimes(2); expect(result.current.value?.serializedRequestBytes).toBe(1000);
});
