import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, expect, it, vi } from "vitest";
import { getSideChatTurn, listSideChatTurns, sendSideChatTurn } from "../../side-chat-api";
import type { SideChatSendRequestV4, SideChatTurnV4 } from "../../types";
import { useSideChat } from "./useSideChat";
vi.mock("../../side-chat-api", () => ({ getSideChatTurn: vi.fn(), listSideChatTurns: vi.fn(), sendSideChatTurn: vi.fn() }));
const input = { question_markdown: "Why this result?", references: [], attachments: ["file"] };
function deferred<T>() {
  let resolve!: (value: T | PromiseLike<T>) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((done, fail) => { resolve = done; reject = fail; });
  return { promise, resolve, reject };
}
function receipt(request: SideChatSendRequestV4): SideChatTurnV4 {
  return { ...request, id: request.request_id, model_label: "Side model", source_snapshot_sha256: "snapshot", source_watermark: { message_count: 0, event_count: 0, event_heads: [] }, sources: [], status: "running", cited_source_ids: [], created_at: "2026-09-14T00:00:00Z", updated_at: "2026-09-14T00:00:00Z" };
}
function historyPage(prefix: string): SideChatTurnV4[] {
  return Array.from({ length: 50 }, (_, index) => ({ ...receipt({ request_id: `${prefix}-${String(index).padStart(2, "0")}`, project_id: "p", conversation_id: "c", model_profile_id: "model", ...input }), status: "completed" as const }));
}
beforeEach(() => { vi.resetAllMocks(); vi.mocked(listSideChatTurns).mockResolvedValue([]); vi.mocked(getSideChatTurn).mockResolvedValue(null); });
it("orders fractional timestamps chronologically and accepts a later receipt within the second", async () => {
  const first = { ...receipt({ request_id: "first", project_id: "p", conversation_id: "c", model_profile_id: "model", ...input }), status: "completed" as const };
  const later = { ...first, id: "later", request_id: "later", created_at: "2026-09-14T00:00:00.001Z", updated_at: "2026-09-14T00:00:00.001Z" };
  vi.mocked(listSideChatTurns).mockResolvedValue([later, first]);
  const { result } = renderHook(() => useSideChat("p", "c", true, "model"));
  await waitFor(() => expect(result.current.ready).toBe(true));
  expect(result.current.records.map((turn) => turn.id)).toEqual(["first", "later"]);
  vi.mocked(listSideChatTurns).mockResolvedValue([{ ...first, updated_at: "2026-09-14T00:00:00.002Z", answer_markdown: "updated" }]);
  await act(async () => { await result.current.refresh(); });
  expect(result.current.records.find((turn) => turn.id === "first")?.answer_markdown).toBe("updated");
});
it("retains the complete request and identity after unknown acceptance", async () => {
  vi.mocked(sendSideChatTurn).mockRejectedValueOnce(new Error("lost reply")).mockImplementation(async (request) => receipt(request));
  const { result } = renderHook(() => useSideChat("p", "c", true, "model"));
  await waitFor(() => expect(result.current.loading).toBe(false));
  await act(async () => { expect(await result.current.send(input)).toBe(false); });
  const first = vi.mocked(sendSideChatTurn).mock.calls[0][0];
  expect(first.attachments).toEqual(["file"]);
  expect(result.current.pending).toBe(true);
  await act(async () => { expect(await result.current.retry()).toBe(true); });
  expect(vi.mocked(sendSideChatTurn).mock.calls[1][0]).toEqual(first);
  expect(result.current.pending).toBe(false);
});
it("reconciles an accepted request without issuing another send", async () => {
  vi.mocked(sendSideChatTurn).mockRejectedValue(new Error("lost"));
  const { result } = renderHook(() => useSideChat("p", "c", true, "model"));
  await waitFor(() => expect(result.current.loading).toBe(false));
  await act(async () => { await result.current.send(input); });
  vi.mocked(getSideChatTurn).mockResolvedValue(receipt(vi.mocked(sendSideChatTurn).mock.calls[0][0]));
  await act(async () => { expect(await result.current.retry()).toBe(true); });
  expect(sendSideChatTurn).toHaveBeenCalledTimes(1);
});
it("keeps late results and model selection in their original conversation", async () => {
  let resolve!: (turn: SideChatTurnV4) => void;
  vi.mocked(sendSideChatTurn).mockImplementation(() => new Promise((done) => { resolve = done; }));
  const { result, rerender } = renderHook(({ conversation }) => useSideChat("p", conversation, true, "primary"), { initialProps: { conversation: "one" } });
  await waitFor(() => expect(result.current.loading).toBe(false));
  act(() => result.current.setModelId("side"));
  let sending!: Promise<boolean>;
  act(() => { sending = result.current.send(input); });
  await waitFor(() => expect(sendSideChatTurn).toHaveBeenCalledOnce());
  const request = vi.mocked(sendSideChatTurn).mock.calls[0][0];
  rerender({ conversation: "two" });
  await act(async () => { resolve(receipt(request)); await sending; });
  expect(result.current.records).toEqual([]);
  expect(result.current.modelId).toBe("primary");
  rerender({ conversation: "one" });
  expect(result.current.modelId).toBe("side");
});
it("requires hydration and keeps a rejected draft eligible for a fresh request", async () => {
  vi.mocked(sendSideChatTurn).mockRejectedValue({ kind: "rejected" });
  const { result } = renderHook(() => useSideChat("p", "c", true, "model"));
  await waitFor(() => expect(result.current.loading).toBe(false));
  await act(async () => { await result.current.send(input); });
  expect(result.current.pending).toBe(false);
  await act(async () => { await result.current.send(input); });
  expect(vi.mocked(sendSideChatTurn).mock.calls[0][0].request_id).not.toBe(vi.mocked(sendSideChatTurn).mock.calls[1][0].request_id);
});
it("loads older history with a stable composite cursor", async () => {
  const page = Array.from({ length: 50 }, (_, index) => ({ ...receipt({ request_id: `r-${String(index).padStart(2, "0")}`, project_id: "p", conversation_id: "c", model_profile_id: "model", ...input }), status: "completed" as const }));
  vi.mocked(listSideChatTurns).mockResolvedValueOnce(page).mockResolvedValueOnce([]);
  const { result } = renderHook(() => useSideChat("p", "c", true, "model"));
  await waitFor(() => expect(result.current.hasMore).toBe(true));
  await act(async () => { await result.current.loadOlder(); });
  expect(listSideChatTurns).toHaveBeenLastCalledWith("p", "c", { createdAt: page[0].created_at, requestId: "r-00" });
  expect(result.current.hasMore).toBe(false);
});
it("reconciles a cached active turn that is outside the newest history page", async () => {
  const request = { request_id: "old", project_id: "p", conversation_id: "c", model_profile_id: "model", ...input };
  vi.mocked(listSideChatTurns).mockResolvedValueOnce([receipt(request)]).mockResolvedValue([]);
  vi.mocked(getSideChatTurn).mockResolvedValue({ ...receipt(request), status: "completed", updated_at: "2026-09-14T00:01:00Z" });
  const { result } = renderHook(() => useSideChat("p", "c", true, "model"));
  await waitFor(() => expect(result.current.busy).toBe(true));
  await act(async () => { await result.current.refresh(); });
  expect(getSideChatTurn).toHaveBeenCalledWith("p", "c", "old");
  expect(result.current.busy).toBe(false);
});
it("serializes refreshes so an older completion cannot overwrite newer pagination state", async () => {
  const first = deferred<SideChatTurnV4[]>();
  const second = deferred<SideChatTurnV4[]>();
  vi.mocked(listSideChatTurns).mockImplementationOnce(() => first.promise).mockImplementationOnce(() => second.promise);
  const { result } = renderHook(() => useSideChat("p", "c", true, "model"));
  await waitFor(() => expect(listSideChatTurns).toHaveBeenCalledTimes(1));

  let newerRefresh!: Promise<void>;
  act(() => { newerRefresh = result.current.refresh(); });
  expect(listSideChatTurns).toHaveBeenCalledTimes(1);

  await act(async () => { first.reject(new Error("older refresh failed")); });
  await waitFor(() => expect(listSideChatTurns).toHaveBeenCalledTimes(2));
  await act(async () => { second.resolve(historyPage("newer")); await newerRefresh; });

  expect(result.current.error).toBe(false);
  expect(result.current.hasMore).toBe(true);
  expect(result.current.loading).toBe(false);
});
