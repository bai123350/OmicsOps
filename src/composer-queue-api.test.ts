import { beforeEach, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { actComposerQueue, listComposerQueue, updateComposerQueue } from "./composer-queue-api";
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
beforeEach(() => { vi.clearAllMocks(); Reflect.deleteProperty(window, "__TAURI_INTERNALS__"); });
it("does not pretend that browser preview has a durable queue", async () => {
  await expect(listComposerQueue("p", "c")).rejects.toThrow("desktop app");
  expect(invoke).not.toHaveBeenCalled();
});
it("forwards exact scope, material and revision without allocating a new identity", async () => {
  Object.defineProperty(window, "__TAURI_INTERNALS__", { value: {}, configurable: true });
  const request = { project_id: "p", conversation_id: "c", request_id: "q", expected_revision: 4, message_markdown: "corrected", references: [], attachments: ["file"] };
  await updateComposerQueue(request);
  expect(invoke).toHaveBeenLastCalledWith("composer_queue_update", { request });
  const action = { project_id: "p", conversation_id: "c", request_id: "q", expected_revision: 4, action: "move_up" as const };
  await actComposerQueue(action);
  expect(invoke).toHaveBeenLastCalledWith("composer_queue_action", { request: action });
  await listComposerQueue("p", "c");
  expect(invoke).toHaveBeenLastCalledWith("composer_queue_list", { projectId: "p", conversationId: "c" });
});
