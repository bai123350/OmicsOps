import { afterEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import * as api from "./tauri-api";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

afterEach(() => {
  Reflect.deleteProperty(window, "__TAURI_INTERNALS__");
  vi.clearAllMocks();
});

describe("bundled MCP API", () => {
  it("uses the host catalog and passes only the chosen ID for registration", async () => {
    Object.defineProperty(window, "__TAURI_INTERNALS__", { value: {}, configurable: true });
    const catalog = [{ id: "ensembl", name: "Ensembl", description: "Genome", description_zh: "基因组", tool_count: 4 }];
    vi.mocked(invoke).mockResolvedValueOnce(catalog).mockResolvedValueOnce({ id: "mcp-ensembl" });
    await expect(api.listBundledMcpPresets()).resolves.toEqual(catalog);
    expect(invoke).toHaveBeenNthCalledWith(1, "list_bundled_mcp_presets");
    await expect(api.addBundledMcpServer({ preset_id: "ensembl" })).resolves.toEqual({ id: "mcp-ensembl" });
    expect(invoke).toHaveBeenNthCalledWith(2, "add_bundled_mcp_server", { request: { preset_id: "ensembl" } });
    expect(invoke).toHaveBeenCalledTimes(2);
  });
});
