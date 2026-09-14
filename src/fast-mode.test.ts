import { describe, expect, it } from "vitest";

import { supportsFastMode } from "./fast-mode";

const base = { provider: "open_ai_compatible" as const, base_url: "https://api.openai.com/v1", model: "gpt-5.6-luna" };

describe("supportsFastMode", () => {
  it.each([
    "gpt-6-astra",
    "gpt-5.6-sol",
    "gpt-5.6-terra",
    "gpt-5.6-luna",
  ])("accepts the exact reviewed model %s at the official endpoint", (model) => {
    expect(supportsFastMode({ ...base, model })).toBe(true);
  });

  it.each([
    { provider: "anthropic" as const, base_url: base.base_url, model: base.model },
    { provider: "ollama" as const, base_url: base.base_url, model: base.model },
    { ...base, model: "gpt-5.6-luna-preview" },
    { ...base, model: "GPT-5.6-LUNA" },
    { ...base, base_url: "http://api.openai.com/v1" },
    { ...base, base_url: "https://api.openai.com.example/v1" },
    { ...base, base_url: "https://api.openai.com:8443/v1" },
    { ...base, base_url: "https://user:secret@api.openai.com/v1" },
    { ...base, base_url: "https://api.openai.com/v1?mode=fast" },
    { ...base, base_url: "https://api.openai.com/v1#fast" },
    { ...base, base_url: "https://api.openai.com/v1/chat/completions" },
    { ...base, base_url: "invalid" },
  ])("rejects an unreviewed provider, endpoint, or model: $base_url $model", (profile) => {
    expect(supportsFastMode(profile)).toBe(false);
  });
});
