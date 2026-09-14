import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, expect, it, vi } from "vitest";
import { getContextUsage } from "../../context-usage-api";
import type { ContextUsageSnapshotV4 } from "../../types";
import { contextUsageView, useContextUsage } from "./useContextUsage";
vi.mock("../../context-usage-api", () => ({ getContextUsage: vi.fn() }));
const counter = { known: 0, incomplete_attempts: 0 };
const snapshot: ContextUsageSnapshotV4 = { project_id: "p", conversation_id: "c", observed_total: { input_tokens: counter, output_tokens: counter, reasoning_tokens: counter, cache_read_input_tokens: counter, cache_creation_input_tokens: counter, reported_total_tokens: counter, observed_attempts: 0, final_attempts: 0, partial_attempts: 0, interrupted_attempts: 0, unknown_attempts: 0 }, current_context: { limit_source: { kind: "unknown" }, estimated: false }, conservative_budget: { serialized_request_bytes: 99000, host_context_max_bytes: 100000, image_count: 0 } };
beforeEach(() => { vi.clearAllMocks(); vi.mocked(getContextUsage).mockResolvedValue(snapshot); });
it("does not substitute request bytes or cumulative consumption for current-context tokens", () => {
  const view = contextUsageView(snapshot);
  expect(view.contextTokens).toBeUndefined(); expect(view.contextLimit).toBeUndefined();
  expect(view.serializedRequestBytes).toBe(99000); expect(view.observedInput).toBe(0);
  expect(view.inputTokens).toBeUndefined();
});
it("rejects a cross-conversation snapshot", async () => {
  vi.mocked(getContextUsage).mockResolvedValue({ ...snapshot, conversation_id: "foreign" });
  const { result } = renderHook(() => useContextUsage("p", "c", true));
  await waitFor(() => expect(result.current.error).toBe(true));
  expect(result.current.value).toBeNull();
});
it("drops old values and late reads on a scope change", async () => {
  const { result, rerender } = renderHook(({ id }) => useContextUsage("p", id, true), { initialProps: { id: "c" } });
  await waitFor(() => expect(result.current.value?.observedInput).toBe(0));
  let resolve!: (value: ContextUsageSnapshotV4) => void;
  vi.mocked(getContextUsage).mockImplementationOnce(() => new Promise((yes) => { resolve = yes; }));
  rerender({ id: "other" }); expect(result.current.value).toBeNull();
  rerender({ id: "third" });
  await act(async () => resolve({ ...snapshot, conversation_id: "other" }));
  expect(result.current.value).toBeNull();
});
it("does not read usage before hydration or in browser preview", () => {
  renderHook(() => useContextUsage("p", "c", false));
  expect(getContextUsage).not.toHaveBeenCalled();
});
