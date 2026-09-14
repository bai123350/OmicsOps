import { afterEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { saveConversationExport } from "./conversation-export-api";
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
afterEach(() => { delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__; vi.restoreAllMocks(); });
describe("conversation save transport", () => {
  it("returns native cancellation unchanged", async () => {
    (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {};
    vi.mocked(invoke).mockResolvedValue(null);
    const blob = new Blob(["test"]);
    blob.arrayBuffer = async () => new TextEncoder().encode("test").buffer;
    expect(await saveConversationExport("html", blob)).toBeNull();
    expect(invoke).toHaveBeenCalledWith("save_conversation_export", { request: { format: "html", content_base64: "dGVzdA==" } });
  });
  it("downloads an actual Blob in browser and reports download initiation", async () => {
    const blob = new Blob(["<!DOCTYPE html>"]);
    const create = vi.fn(() => "blob:test");
    vi.stubGlobal("URL", Object.assign(URL, { createObjectURL: create, revokeObjectURL: vi.fn() }));
    const click = vi.spyOn(HTMLAnchorElement.prototype, "click").mockImplementation(function (this: HTMLAnchorElement) { expect(this.download).toBe("omicsops-conversation.html"); expect(this.href).toBe("blob:test"); });
    expect(await saveConversationExport("html", blob)).toBe("download");
    expect(create).toHaveBeenCalledWith(blob);
    expect(click).toHaveBeenCalledOnce();
  });
});
