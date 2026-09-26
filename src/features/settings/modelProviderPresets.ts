import type { ModelProfile } from "../../types";

type EndpointProfile = Pick<ModelProfile, "provider" | "base_url">;
type ProviderKind = ModelProfile["provider"];

export interface ModelProviderPreset {
  id: string;
  label: string;
  provider: ProviderKind;
  base_url: string;
  detailZh: string;
  detailEn: string;
}

export const modelProviderPresets: readonly ModelProviderPreset[] = [
  { id: "anthropic", label: "Anthropic", provider: "anthropic", base_url: "https://api.anthropic.com/", detailZh: "Messages API · 工具调用", detailEn: "Messages API · tool use" },
  { id: "openai", label: "OpenAI", provider: "open_ai_compatible", base_url: "https://api.openai.com/v1", detailZh: "官方 API · Chat Completions", detailEn: "Official API · Chat Completions" },
  { id: "deepseek", label: "DeepSeek", provider: "open_ai_compatible", base_url: "https://api.deepseek.com/v1", detailZh: "官方 API · Flash / Pro", detailEn: "Official API · Flash / Pro" },
  { id: "kimi", label: "Kimi", provider: "open_ai_compatible", base_url: "https://api.moonshot.cn/v1", detailZh: "按量 API · Chat Completions", detailEn: "Usage API · Chat Completions" },
  { id: "kimi-coding", label: "Kimi Coding", provider: "open_ai_compatible", base_url: "https://api.kimi.com/coding/v1", detailZh: "Coding 套餐 · 专属密钥", detailEn: "Coding plan · dedicated key" },
  { id: "glm", label: "GLM", provider: "open_ai_compatible", base_url: "https://open.bigmodel.cn/api/paas/v4", detailZh: "按量 API · Chat Completions", detailEn: "Usage API · Chat Completions" },
  { id: "glm-coding", label: "GLM Coding", provider: "open_ai_compatible", base_url: "https://open.bigmodel.cn/api/coding/paas/v4", detailZh: "Coding 套餐 · 专属密钥", detailEn: "Coding plan · dedicated key" },
  { id: "qwen-cn", label: "Qwen China", provider: "open_ai_compatible", base_url: "https://dashscope.aliyuncs.com/compatible-mode/v1", detailZh: "中国端点 · Chat Completions", detailEn: "China endpoint · Chat Completions" },
  { id: "qwen-intl", label: "Qwen International", provider: "open_ai_compatible", base_url: "https://dashscope-intl.aliyuncs.com/compatible-mode/v1", detailZh: "国际端点 · Chat Completions", detailEn: "International endpoint · Chat Completions" },
  { id: "minimax-cn", label: "MiniMax China", provider: "anthropic", base_url: "https://api.minimax.cn/anthropic", detailZh: "中国端点 · Messages API", detailEn: "China endpoint · Messages API" },
  { id: "minimax-intl", label: "MiniMax International", provider: "anthropic", base_url: "https://api.minimax.io/anthropic", detailZh: "国际端点 · Messages API", detailEn: "International endpoint · Messages API" },
  { id: "ollama", label: "Ollama", provider: "ollama", base_url: "http://127.0.0.1:11434/", detailZh: "本地模型 · Ollama API", detailEn: "Local models · Ollama API" },
  { id: "lm-studio", label: "LM Studio", provider: "open_ai_compatible", base_url: "http://127.0.0.1:1234/v1", detailZh: "本地模型 · 可选密钥", detailEn: "Local models · optional key" },
  { id: "openrouter", label: "OpenRouter", provider: "open_ai_compatible", base_url: "https://openrouter.ai/api/v1", detailZh: "模型路由 · 完整模型 ID", detailEn: "Model routing · full model ID" },
  { id: "opencode-go", label: "OpenCode Go", provider: "open_ai_compatible", base_url: "https://opencode.ai/zen/go/v1", detailZh: "官方端点 · Chat / Messages", detailEn: "Official endpoint · Chat / Messages" },
  { id: "custom", label: "OpenAI-compatible", provider: "open_ai_compatible", base_url: "https://api.openai.com/", detailZh: "Chat Completions · 自定义 Base URL", detailEn: "Chat Completions · custom Base URL" },
];

function parsedEndpoint(base_url: string): URL | null {
  try {
    const url = new URL(base_url);
    if (url.username || url.password || url.href.includes("?") || url.href.includes("#")) return null;
    return url;
  } catch {
    return null;
  }
}

export function isLocalLmStudioEndpoint(profile: EndpointProfile): boolean {
  if (profile.provider !== "open_ai_compatible") return false;
  const url = parsedEndpoint(profile.base_url);
  return !!url && url.protocol === "http:"
    && ["localhost", "127.0.0.1", "[::1]"].includes(url.hostname)
    && url.port === "1234"
    && ["/", "/v1", "/v1/"].includes(url.pathname);
}

export function matchesModelProviderPreset(profile: EndpointProfile, preset: ModelProviderPreset): boolean {
  if (preset.id === "custom") return false;
  if (preset.id === "lm-studio") return isLocalLmStudioEndpoint(profile);
  if (preset.id === "opencode-go") {
    if (profile.provider !== "anthropic" && profile.provider !== "open_ai_compatible") return false;
  } else if (profile.provider !== preset.provider) return false;
  const actual = parsedEndpoint(profile.base_url);
  const expected = parsedEndpoint(preset.base_url);
  if (!actual || !expected || actual.protocol !== expected.protocol || actual.hostname !== expected.hostname || actual.port !== expected.port) return false;
  const path = actual.pathname.replace(/\/$/, "") || "/";
  const expectedPath = expected.pathname.replace(/\/$/, "") || "/";
  if (["anthropic", "openai", "deepseek"].includes(preset.id)) return path === "/" || path === "/v1";
  return path === expectedPath;
}
