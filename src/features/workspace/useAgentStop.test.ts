import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import * as api from "../../tauri-api";
import type { StopRunReceiptV4 } from "../../types";
import { useAgentStop } from "./useAgentStop";

const scope = { projectId: "project-a", conversationId: "conversation-a", runId: "run-a" };

function receipt(overrides: Partial<StopRunReceiptV4> = {}): StopRunReceiptV4 {
  return {
    request_id: "stop-request-1",
    project_id: scope.projectId,
    conversation_id: scope.conversationId,
    run_id: scope.runId,
    status: "requested",
    created_at: "2026-09-14T00:00:00.000Z",
    updated_at: "2026-09-14T00:00:00.000Z",
    ...overrides,
  };
}

function deferred<T>() {
  let resolve!: (value: T | PromiseLike<T>) => void;
  const promise = new Promise<T>((resolvePromise) => { resolve = resolvePromise; });
  return { promise, resolve };
}

beforeEach(() => vi.restoreAllMocks());

describe("useAgentStop", () => {
  it("hydrates a requested receipt and clears stopping only after local terminal observation", async () => {
    const getStop = vi.spyOn(api, "agentV4GetStop").mockResolvedValue(receipt());
    const { result } = renderHook(() => useAgentStop(scope));

    await waitFor(() => expect(getStop).toHaveBeenCalledWith(scope.projectId, scope.conversationId, scope.runId));
    await waitFor(() => expect(result.current.stopping).toBe(true));

    act(() => result.current.markTerminal(scope.runId));
    expect(result.current.stopping).toBe(false);
  });

  it("reuses one request id after an uncertain request failure", async () => {
    vi.spyOn(api, "agentV4GetStop").mockResolvedValue(null);
    const requestStop = vi.spyOn(api, "agentV4RequestStop")
      .mockRejectedValueOnce(new Error("connection lost"))
      .mockResolvedValueOnce(receipt());
    const { result } = renderHook(() => useAgentStop(scope));
    await waitFor(() => expect(result.current.loading).toBe(false));

    await act(async () => {
      await expect(result.current.requestStop()).rejects.toThrow("connection lost");
    });
    expect(result.current.stopping).toBe(false);

    await act(async () => {
      await result.current.requestStop();
    });
    expect(requestStop).toHaveBeenCalledTimes(2);
    expect(requestStop.mock.calls[0][0].request_id).toBe(requestStop.mock.calls[1][0].request_id);
    expect(requestStop.mock.calls[0][0]).toMatchObject({
      project_id: scope.projectId,
      conversation_id: scope.conversationId,
      run_id: scope.runId,
    });
    expect(result.current.stopping).toBe(true);
  });

  it("ignores a late stop lookup from a previous conversation scope", async () => {
    const first = deferred<StopRunReceiptV4 | null>();
    const second = deferred<StopRunReceiptV4 | null>();
    const getStop = vi.spyOn(api, "agentV4GetStop").mockImplementation((_projectId, conversationId) => (
      conversationId === "conversation-a" ? first.promise : second.promise
    ));
    const { result, rerender } = renderHook(
      ({ conversationId }) => useAgentStop("project-a", conversationId, "run-a"),
      { initialProps: { conversationId: "conversation-a" } },
    );
    rerender({ conversationId: "conversation-b" });

    await act(async () => second.resolve(null));
    await waitFor(() => expect(getStop).toHaveBeenCalledWith("project-a", "conversation-b", "run-a"));
    await act(async () => first.resolve(receipt()));
    expect(result.current.receipt).toBeNull();
    expect(result.current.stopping).toBe(false);
  });

  it("accepts a native observed receipt without keeping the stop button busy", async () => {
    const observed = receipt({ status: "observed", updated_at: "2026-09-14T00:00:02.000Z" });
    vi.spyOn(api, "agentV4GetStop").mockResolvedValue(null);
    vi.spyOn(api, "agentV4RequestStop").mockResolvedValue(observed);
    const { result } = renderHook(() => useAgentStop(scope));
    await waitFor(() => expect(result.current.loading).toBe(false));

    await act(async () => { await result.current.requestStop(); });
    expect(result.current.receipt).toEqual(observed);
    expect(result.current.stopping).toBe(false);
  });

  it("seeds the retry id from a receipt restored after reload", async () => {
    const restored = receipt({ request_id: "persisted-stop-request" });
    vi.spyOn(api, "agentV4GetStop").mockResolvedValue(restored);
    const requestStop = vi.spyOn(api, "agentV4RequestStop").mockResolvedValue(restored);
    const { result } = renderHook(() => useAgentStop(scope));
    await waitFor(() => expect(result.current.stopping).toBe(true));

    act(() => result.current.clear());
    await act(async () => { await result.current.requestStop(); });
    expect(requestStop).toHaveBeenCalledWith(expect.objectContaining({ request_id: restored.request_id }));
  });
});
