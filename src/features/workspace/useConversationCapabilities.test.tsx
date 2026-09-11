import { act, renderHook, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import * as api from "../../tauri-api";
import type { ConversationCapabilitiesV4 } from "../../types";
import { useConversationCapabilities } from "./useConversationCapabilities";

const snapshot = (id: string, count: number): ConversationCapabilitiesV4 => ({ project_id: "p", conversation_id: id, skills: [], mcp_servers: [], memory_count: count });
afterEach(() => vi.restoreAllMocks());

describe("conversation capabilities lifecycle", () => {
  it("clears the prior conversation immediately and ignores late responses", async () => {
    let resolveOld!: (value: ConversationCapabilitiesV4) => void;
    const request = vi.spyOn(api, "getConversationCapabilitiesV4")
      .mockImplementationOnce(() => new Promise((resolve) => { resolveOld = resolve; }))
      .mockResolvedValueOnce(snapshot("b", 2));
    const { result, rerender } = renderHook(({ id }) => useConversationCapabilities("p", id, "0"), { initialProps: { id: "a" } });
    await waitFor(() => expect(request).toHaveBeenCalledWith("p", "a"));
    rerender({ id: "b" });
    expect(result.current.summary).toBeNull();
    await waitFor(() => expect(result.current.summary?.memory_count).toBe(2));
    await act(async () => resolveOld(snapshot("a", 99)));
    expect(result.current.summary?.conversation_id).toBe("b");
    expect(result.current.summary?.memory_count).toBe(2);
  });

  it("refreshes after configuration changes, exposes failures, and retries", async () => {
    const request = vi.spyOn(api, "getConversationCapabilitiesV4").mockResolvedValueOnce(snapshot("a", 1))
      .mockRejectedValueOnce(new Error("snapshot unavailable")).mockResolvedValueOnce(snapshot("a", 3));
    const { result, rerender } = renderHook(({ revision }) => useConversationCapabilities("p", "a", revision), { initialProps: { revision: "0" } });
    await waitFor(() => expect(result.current.summary?.memory_count).toBe(1));
    rerender({ revision: "1" });
    await waitFor(() => expect(result.current.error).toBe("snapshot unavailable"));
    expect(result.current.summary).toBeNull();
    act(() => result.current.refresh());
    await waitFor(() => expect(result.current.summary?.memory_count).toBe(3));
    expect(result.current.error).toBe("");
    expect(request).toHaveBeenCalledTimes(3);
  });

  it("never queries without a conversation and rejects mismatched identity", async () => {
    const request = vi.spyOn(api, "getConversationCapabilitiesV4").mockResolvedValue(snapshot("other", 99));
    const { result, rerender } = renderHook(({ id }: { id: string | null }) => useConversationCapabilities("p", id, "0"), { initialProps: { id: null as string | null } });
    expect(request).not.toHaveBeenCalled();
    expect(result.current.summary).toBeNull();
    rerender({ id: "a" });
    await waitFor(() => expect(result.current.error).not.toBe(""));
    expect(result.current.summary).toBeNull();
  });
});
