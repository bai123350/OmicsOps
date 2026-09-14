import { afterEach, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { getSideChatTurn, listSideChatTurns, sendSideChatTurn } from "./side-chat-api";
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
afterEach(() => { delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__; vi.clearAllMocks(); });
it("uses scoped native commands and passes the complete request", async () => {
  Object.defineProperty(window, "__TAURI_INTERNALS__", { configurable: true, value: {} });
  const request = { request_id: "r", project_id: "p", conversation_id: "c", model_profile_id: "m", question_markdown: "Compare", references: [], attachments: ["a"] };
  await sendSideChatTurn(request);
  expect(invoke).toHaveBeenCalledWith("side_chat_send_v4", { request });
  await listSideChatTurns("p", "c");
  expect(invoke).toHaveBeenCalledWith("side_chat_list_v4", { projectId: "p", conversationId: "c", limit: 50 });
  await getSideChatTurn("p", "c", "r");
  expect(invoke).toHaveBeenCalledWith("side_chat_get_v4", { projectId: "p", conversationId: "c", requestId: "r" });
});
it("does not fabricate accepted turns in browser preview", async () => {
  await expect(sendSideChatTurn({ request_id: "r", project_id: "p", conversation_id: "c", model_profile_id: "m", question_markdown: "Question", references: [], attachments: [] })).rejects.toMatchObject({ kind: "rejected" });
  expect(invoke).not.toHaveBeenCalled();
});
