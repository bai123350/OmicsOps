import { afterEach, describe, expect, it } from "vitest";

import { latestUsedConversation } from "./tauri-api";

describe("latestUsedConversation", () => {
  afterEach(() => {
    delete (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__;
  });

  it("returns no candidate outside the Tauri host", async () => {
    delete (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__;

    await expect(latestUsedConversation("project-1")).resolves.toBeNull();
  });
});
