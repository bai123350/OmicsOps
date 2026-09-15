import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: mocks.invoke }));

import { deleteCredential, replaceCredential } from "./credentials-settings-api";

describe("credentials settings API", () => {
  beforeEach(() => mocks.invoke.mockReset());

  it("passes an entity target and expected host reference", async () => {
    mocks.invoke.mockResolvedValue({});
    const request = {
      target: { kind: "mcp_binding" as const, server_id: "server-1", name: "TOKEN" },
      expected_reference: "settings/entry-1",
      expected_value_kind: "api_key" as const,
      secret: "new-value",
    };
    await replaceCredential(request);
    expect(mocks.invoke).toHaveBeenCalledWith("settings_replace_credential", { request });
    expect(mocks.invoke.mock.calls[0][1]).not.toHaveProperty("account");
  });

  it("deletes only by managed entry id", async () => {
    mocks.invoke.mockResolvedValue({ kind: "deleted" });
    await deleteCredential("entry-1");
    expect(mocks.invoke).toHaveBeenCalledWith("settings_delete_credential", { id: "entry-1" });
  });
});
