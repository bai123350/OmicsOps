import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { getSessionReviewV4, listSessionReviewsV4, startSessionReviewV4 } from "../../session-review-api";
import type { SessionReviewRecordV4 } from "../../types";
import { useSessionReviews } from "./useSessionReviews";

vi.mock("../../session-review-api", async (importOriginal) => ({
  ...await importOriginal<typeof import("../../session-review-api")>(),
  getSessionReviewV4: vi.fn(),
  listSessionReviewsV4: vi.fn(),
  startSessionReviewV4: vi.fn(),
}));

const listReviews = vi.mocked(listSessionReviewsV4);
const getReview = vi.mocked(getSessionReviewV4);
const startReview = vi.mocked(startSessionReviewV4);

const profile = { id: "profile-a" };

function review(overrides: Partial<SessionReviewRecordV4> = {}): SessionReviewRecordV4 {
  return {
    id: "review-1",
    project_id: "project-a",
    conversation_id: "conversation-a",
    reviewer_profile_id: "profile-a",
    reviewer_configuration_hash: "config-hash",
    source_snapshot_sha256: "source-hash",
    source_message_count: 1,
    sources: [{ message_id: "message-1", sequence: 1, role: "user", text: "Evidence" }],
    status: "running",
    report: null,
    error: null,
    created_at: "2026-09-14T00:00:00.000Z",
    updated_at: "2026-09-14T00:00:00.000Z",
    ...overrides,
  };
}

function deferred<T>() {
  let resolve!: (value: T | PromiseLike<T>) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise;
    reject = rejectPromise;
  });
  return { promise, resolve, reject };
}

beforeEach(() => {
  vi.clearAllMocks();
  vi.useRealTimers();
  listReviews.mockResolvedValue([]);
  getReview.mockResolvedValue(null);
});

describe("useSessionReviews", () => {
  it("loads scoped history, polls a running native record, and stops after completion", async () => {
    const running = review();
    const completed = review({ status: "completed", report: { summary: "Saved", findings: [] }, updated_at: "2026-09-14T00:00:02.000Z" });
    listReviews.mockResolvedValue([running]);
    getReview.mockResolvedValue(completed);
    vi.useFakeTimers();
    const { result } = renderHook(() => useSessionReviews("project-a", "conversation-a", profile));

    await act(async () => { await Promise.resolve(); await Promise.resolve(); });
    expect(result.current.loading).toBe(false);
    expect(result.current.busy).toBe(true);
    await act(async () => {
      vi.advanceTimersByTime(1_500);
      await Promise.resolve();
      await Promise.resolve();
    });
    expect(result.current.records[0].status).toBe("completed");
    expect(result.current.busy).toBe(false);
    expect(getReview).toHaveBeenCalledWith("project-a", "conversation-a", "review-1");
  });

  it("keeps persisted running status authoritative across unmount and remount", async () => {
    const running = review();
    listReviews.mockResolvedValue([running]);
    const first = renderHook(() => useSessionReviews("project-a", "conversation-a", profile));
    await waitFor(() => expect(first.result.current.busy).toBe(true));
    first.unmount();

    const second = renderHook(() => useSessionReviews("project-a", "conversation-a", profile));
    await waitFor(() => expect(second.result.current.loading).toBe(false));
    expect(second.result.current.records[0].status).toBe("running");
    expect(second.result.current.busy).toBe(true);
  });

  it("ignores a stale history response after the conversation changes", async () => {
    const first = deferred<SessionReviewRecordV4[]>();
    const second = deferred<SessionReviewRecordV4[]>();
    listReviews.mockImplementation((_, conversationId) => conversationId === "conversation-a" ? first.promise : second.promise);
    const { result, rerender } = renderHook(({ conversationId }) => useSessionReviews("project-a", conversationId, profile), { initialProps: { conversationId: "conversation-a" } });
    rerender({ conversationId: "conversation-b" });
    await act(async () => second.resolve([review({ id: "review-b", conversation_id: "conversation-b" })]));
    await waitFor(() => expect(result.current.records[0].id).toBe("review-b"));
    await act(async () => first.resolve([review({ id: "review-a" })]));
    expect(result.current.records[0].id).toBe("review-b");
  });

  it("does not lock normal sending after a definite preflight rejection", async () => {
    startReview.mockRejectedValueOnce({ kind: "rejected", message: "private detail" })
      .mockResolvedValueOnce(review({ status: "completed" }));
    const { result } = renderHook(() => useSessionReviews("project-a", "conversation-a", profile));
    await waitFor(() => expect(result.current.loading).toBe(false));
    await act(async () => { await result.current.startReview(); });
    expect(result.current.busy).toBe(false);
    expect(result.current.error).toContain("Review was not started");
    expect(result.current.error).not.toContain("private detail");
    expect(getReview).not.toHaveBeenCalled();
    const rejectedId = startReview.mock.calls[0][0].request_id;
    await act(async () => { await result.current.startReview(); });
    expect(startReview.mock.calls[1][0].request_id).not.toBe(rejectedId);
  });

  it("prevents duplicate starts and preserves the same request UUID after ambiguous transport failure", async () => {
    const firstStart = deferred<SessionReviewRecordV4>();
    startReview.mockReturnValueOnce(firstStart.promise).mockResolvedValueOnce(review({ id: "review-2" }));
    getReview.mockRejectedValueOnce(new Error("transport detail")).mockResolvedValueOnce(null);
    const { result } = renderHook(() => useSessionReviews("project-a", "conversation-a", profile));
    await waitFor(() => expect(result.current.loading).toBe(false));

    await act(async () => {
      const first = result.current.startReview();
      const second = result.current.startReview();
      expect(startReview).toHaveBeenCalledTimes(1);
      firstStart.reject(new Error("ambiguous"));
      await first;
      await second;
    });
    expect(result.current.error).toContain("Could not start the review");
    expect(result.current.busy).toBe(true);
    const firstRequestId = vi.mocked(startReview).mock.calls[0][0].request_id;

    await act(async () => { await result.current.retry(); });
    expect(startReview).toHaveBeenCalledTimes(2);
    expect(startReview.mock.calls[1][0].request_id).toBe(firstRequestId);
    expect(result.current.records[0].id).toBe("review-2");
  });

  it("recovers an already persisted record after the start response is lost", async () => {
    startReview.mockRejectedValueOnce(new Error("lost response"));
    const recovered = review({ id: "recovered", status: "failed", error: "private detail" });
    getReview.mockResolvedValueOnce(recovered);
    const { result } = renderHook(() => useSessionReviews("project-a", "conversation-a", profile));
    await waitFor(() => expect(result.current.loading).toBe(false));

    await act(async () => { await result.current.startReview(); });
    expect(result.current.records[0]).toMatchObject({ id: "recovered", status: "failed" });
    expect(result.current.busy).toBe(false);
    expect(result.current.error).toBe("");
  });

  it("does not mutate the active scope after an in-flight start resolves late", async () => {
    const pending = deferred<SessionReviewRecordV4>();
    startReview.mockReturnValue(pending.promise);
    const { result, rerender } = renderHook(({ conversationId }) => useSessionReviews("project-a", conversationId, profile), { initialProps: { conversationId: "conversation-a" } });
    await waitFor(() => expect(result.current.loading).toBe(false));
    let requestPromise!: Promise<void>;
    await act(async () => { requestPromise = result.current.startReview(); });
    rerender({ conversationId: "conversation-b" });
    await act(async () => { pending.resolve(review()); await requestPromise; });
    expect(result.current.records).toEqual([]);
  });
});
