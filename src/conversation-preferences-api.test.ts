import { afterEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";

import {
  DEFAULT_CONVERSATION_AGENT_PREFERENCES,
  getConversationAgentPreferencesV4,
  saveConversationAgentPreferencesV4,
} from "./conversation-preferences-api";
import type { ConversationAgentPreferencesV4 } from "./types";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

const preferences: ConversationAgentPreferencesV4 = {
  delegation_enabled: false,
  auto_review: true,
  memory_enabled: false,
};

function enableTauri() {
  Object.defineProperty(window, "__TAURI_INTERNALS__", { value: {}, configurable: true });
}

afterEach(() => {
  Reflect.deleteProperty(window, "__TAURI_INTERNALS__");
  vi.clearAllMocks();
});

describe("conversation agent preferences API", () => {
  it("returns all-enabled defaults in browser preview without invoking native commands", async () => {
    await expect(getConversationAgentPreferencesV4("project-a", "conversation-a")).resolves.toEqual(DEFAULT_CONVERSATION_AGENT_PREFERENCES);
    await expect(saveConversationAgentPreferencesV4("project-a", "conversation-a", preferences)).rejects.toThrow("desktop app");
    expect(invoke).not.toHaveBeenCalled();
  });

  it("uses the native get command with the active scope", async () => {
    enableTauri();
    vi.mocked(invoke).mockResolvedValueOnce(preferences);

    await expect(getConversationAgentPreferencesV4("project-a", "conversation-a")).resolves.toEqual(preferences);
    expect(invoke).toHaveBeenCalledWith("conversation_get_agent_preferences_v4", {
      projectId: "project-a",
      conversationId: "conversation-a",
    });
  });

  it("uses the native save command with the complete preference payload", async () => {
    enableTauri();
    vi.mocked(invoke).mockResolvedValueOnce(preferences);

    await expect(saveConversationAgentPreferencesV4("project-a", "conversation-a", preferences)).resolves.toEqual(preferences);
    expect(invoke).toHaveBeenCalledWith("conversation_save_agent_preferences_v4", {
      projectId: "project-a",
      conversationId: "conversation-a",
      preferences,
    });
  });
});
