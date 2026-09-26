import { describe, expect, it } from "vitest";
import { matchesModelProviderPreset, modelProviderPresets, isLocalLmStudioEndpoint } from "./modelProviderPresets";

describe("model provider preset endpoints", () => {
  it("matches saved official profiles by protocol, host, port, and exact path", () => {
    const minimax = modelProviderPresets.find((preset) => preset.id === "minimax-cn")!;
    expect(matchesModelProviderPreset({ provider: "anthropic", base_url: "https://api.minimax.cn:443/anthropic/" }, minimax)).toBe(true);
    for (const [provider, base_url] of [
      ["anthropic", "https://api.anthropic.com/"],
      ["anthropic", "https://api.minimax.io/anthropic"],
      ["open_ai_compatible", "https://api.minimax.cn/anthropic"],
      ["anthropic", "https://api.minimax.cn.evil.test/anthropic"],
      ["anthropic", "https://api.minimax.cn:8443/anthropic"],
      ["anthropic", "https://api.minimax.cn/anthropic/v1"],
      ["anthropic", "https://api.minimax.cn/anthropic?secret=x"],
      ["anthropic", "https://user@api.minimax.cn/anthropic"],
    ] as const) {
      expect(matchesModelProviderPreset({ provider, base_url }, minimax)).toBe(false);
    }
    const deepseek = modelProviderPresets.find((preset) => preset.id === "deepseek")!;
    expect(matchesModelProviderPreset({ provider: "open_ai_compatible", base_url: "https://api.deepseek.com" }, deepseek)).toBe(true);
    expect(matchesModelProviderPreset({ provider: "open_ai_compatible", base_url: "https://api.deepseek.com:443/v1/" }, deepseek)).toBe(true);
    const anthropic = modelProviderPresets.find((preset) => preset.id === "anthropic")!;
    expect(matchesModelProviderPreset({ provider: "anthropic", base_url: "https://api.anthropic.com/v1/" }, anthropic)).toBe(true);
    expect(matchesModelProviderPreset({ provider: "anthropic", base_url: "https://api.anthropic.com/v1?" }, anthropic)).toBe(false);
    expect(matchesModelProviderPreset({ provider: "anthropic", base_url: "https://api.anthropic.com/v1#" }, anthropic)).toBe(false);
  });

  it("recognizes only exact local LM Studio addresses for optional key guidance", () => {
    for (const base_url of ["http://localhost:1234", "http://127.0.0.1:1234/v1/", "http://[::1]:1234/v1"]) {
      expect(isLocalLmStudioEndpoint({ provider: "open_ai_compatible", base_url })).toBe(true);
    }
    for (const base_url of ["http://localhost:1235/v1", "https://localhost:1234/v1", "http://localhost.example:1234/v1", "http://localhost:1234/v1?x=1", "http://localhost:1234/v1?", "http://localhost:1234/v1#", "http://127.0.0.2:1234/v1"]) {
      expect(isLocalLmStudioEndpoint({ provider: "open_ai_compatible", base_url })).toBe(false);
    }
  });
});
