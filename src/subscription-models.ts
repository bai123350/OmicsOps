import type { ModelProfile } from "./types";

export function reviewedGoEfforts(model: string): string[] {
  switch (model) {
    case "grok-4.7": case "grok-4.6": return ["low", "medium", "high", "xhigh"];
    case "gpt-6-luna": case "gpt-5.6-luna": return ["none", "low", "medium", "high", "xhigh", "max"];
    case "muse-spark-1.3-contributor": case "muse-spark-1.2-contributor": return ["minimal", "low", "medium", "high", "xhigh"];
    default: return [];
  }
}
export function modelBackendLabel(profile: Pick<ModelProfile, "provider">, zh: boolean): string {
  switch (profile.provider) {
    case "open_ai_codex": return zh ? "Codex 订阅" : "Codex subscription";
    case "claude_code": return zh ? "Claude Code 订阅（生成禁用）" : "Claude Code subscription (generation disabled)";
    case "open_ai_responses": return "OpenCode Go · Responses";
    default: return profile.provider;
  }
}
export function modelSelectionRequest(profile: ModelProfile, model: string) {
  const provider = isOpenCodeGo(profile) ? openCodeGoProtocol(model) ?? profile.provider : profile.provider;
  return { id: profile.id, label: profile.label, provider, base_url: profile.base_url, model };
}

export const openCodeGoChatModels = ["glm-5.3-flash", "glm-5.3", "glm-5.2", "glm-5.1", "kimi-k3", "kimi-k2.7-code", "kimi-k2.6", "longcat-2.0", "deepseek-v4.1-flash", "deepseek-v4-pro", "deepseek-v4-flash", "deepseek-v4-flash-vision-exp", "mimo-v2.5", "mimo-v2.5-pro", "hy4-preview", "hy3"];
export const openCodeGoMessagesModels = ["minimax-m3", "minimax-m2.7", "minimax-m2.5", "qwen3.8-max", "qwen3.8-flash", "qwen3.7-max", "qwen3.7-plus", "qwen3.6-plus"];
export const openCodeGoResponsesModels = ["grok-4.7", "grok-4.6", "gpt-6-luna", "gpt-5.6-luna", "muse-spark-1.3-contributor", "muse-spark-1.2-contributor"];
export const openCodeGoModels = [...openCodeGoChatModels, ...openCodeGoMessagesModels, ...openCodeGoResponsesModels];

export function openCodeGoProtocol(model: string): ModelProfile["provider"] | null {
  if (openCodeGoChatModels.includes(model)) return "open_ai_compatible";
  if (openCodeGoMessagesModels.includes(model)) return "anthropic";
  if (openCodeGoResponsesModels.includes(model)) return "open_ai_responses";
  return null;
}

export function isOpenCodeGo(profile: Pick<ModelProfile, "provider" | "base_url">): boolean {
  if (profile.provider === "ollama") return false;
  try {
    const url = new URL(profile.base_url);
    return url.protocol === "https:"
      && url.hostname === "opencode.ai"
      && (url.port === "" || url.port === "443")
      && (url.pathname === "/zen/go/v1" || url.pathname === "/zen/go/v1/")
      && url.username === ""
      && url.password === ""
      && url.search === ""
      && url.hash === "";
  } catch {
    return false;
  }
}

