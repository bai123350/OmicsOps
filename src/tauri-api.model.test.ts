import { afterEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import * as api from "./tauri-api";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

afterEach(() => {
  Reflect.deleteProperty(window, "__TAURI_INTERNALS__");
  vi.clearAllMocks();
});

describe("model profile API", () => {
  const request = {
    id: "profile-1",
    label: "Lab model",
    provider: "open_ai_compatible" as const,
    base_url: "https://models.example/v1",
    model: "science-model",
    context_window_tokens: 64000,
  };

  it("passes an explicitly configured context budget to the native command", async () => {
    Object.defineProperty(window, "__TAURI_INTERNALS__", { value: {}, configurable: true });
    vi.mocked(invoke).mockResolvedValueOnce({ id: "profile-1" });

    await api.saveModelProfile(request);

    expect(invoke).toHaveBeenCalledWith("save_model_profile", { request });
  });

  it("returns the configured context budget from the demo save path", async () => {
    await expect(api.saveModelProfile(request)).resolves.toEqual(expect.objectContaining({
      id: "profile-1",
      context_window_tokens: 64000,
    }));
  });
});
