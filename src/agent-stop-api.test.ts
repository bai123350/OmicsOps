import { beforeEach, describe, expect, it, vi } from "vitest";

import { invoke } from "@tauri-apps/api/core";
import { agentV4GetStop, agentV4RequestStop } from "./agent-stop-api";
import type { StopRunReceiptV4, StopRunRequestV4 } from "./types";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

const invokeMock = vi.mocked(invoke);

const request: StopRunRequestV4 = {
  request_id: "stop-request-1",
  project_id: "project-a",
  conversation_id: "conversation-a",
  run_id: "run-a",
};

const receipt: StopRunReceiptV4 = {
  ...request,
  status: "requested",
  created_at: "2026-09-14T00:00:00.000Z",
  updated_at: "2026-09-14T00:00:00.000Z",
};

beforeEach(() => {
  vi.clearAllMocks();
  Reflect.deleteProperty(window, "__TAURI_INTERNALS__");
});

describe("agent stop API", () => {
  it("does not claim a stop in browser preview", async () => {
    await expect(agentV4GetStop("project-a", "conversation-a", "run-a")).resolves.toBeNull();
    await expect(agentV4RequestStop(request)).rejects.toThrow("desktop app");
    expect(invokeMock).not.toHaveBeenCalled();
  });

  it("forwards the scoped request and lookup to native commands", async () => {
    Object.defineProperty(window, "__TAURI_INTERNALS__", { value: {}, configurable: true });
    invokeMock.mockResolvedValueOnce(receipt).mockResolvedValueOnce(receipt);

    await expect(agentV4RequestStop(request)).resolves.toEqual(receipt);
    await expect(agentV4GetStop(request.project_id, request.conversation_id, request.run_id)).resolves.toEqual(receipt);
    expect(invokeMock).toHaveBeenNthCalledWith(1, "agent_v4_request_stop", { request });
    expect(invokeMock).toHaveBeenNthCalledWith(2, "agent_v4_get_stop", {
      projectId: request.project_id,
      conversationId: request.conversation_id,
      runId: request.run_id,
    });
  });
});
