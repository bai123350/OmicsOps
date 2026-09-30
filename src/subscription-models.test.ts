import { expect, it } from "vitest";
import { modelSelectionRequest, openCodeGoResponsesModels, reviewedGoEfforts } from "./subscription-models";
import type { ModelProfile } from "./types";
const profile: ModelProfile = { id: "go", label: "Go", provider: "anthropic", base_url: "https://opencode.ai/zen/go/v1", model: "minimax-m3", credential_reference: "model/go", supports_tools: true, supports_vision: false };
it("switches each exact Go model protocol without exporting host credentials or bindings", () => {
  for (const id of openCodeGoResponsesModels) expect(modelSelectionRequest(profile, id)).toEqual({ id: "go", label: "Go", provider: "open_ai_responses", base_url: profile.base_url, model: id });
  expect(modelSelectionRequest(profile, "unreviewed-full-id").provider).toBe("anthropic");
  expect(modelSelectionRequest({ ...profile, base_url: "https://unknown.example/v1" }, "grok-4.7").provider).toBe("anthropic");
  expect(reviewedGoEfforts("grok-4.7-sibling")).toEqual([]);
});
