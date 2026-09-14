import type { ModelProfile } from "./types";

const FAST_MODELS = new Set([
  "gpt-6-astra",
  "gpt-5.6-sol",
  "gpt-5.6-terra",
  "gpt-5.6-luna",
]);

/**
 * Fast mode is an exact provider capability. Custom gateways and model
 * families are intentionally left unavailable until the host reviews them.
 */
export function supportsFastMode(
  profile: Pick<ModelProfile, "provider" | "base_url" | "model">,
): boolean {
  if (profile.provider !== "open_ai_compatible" || !FAST_MODELS.has(profile.model)) return false;
  try {
    const url = new URL(profile.base_url);
    return url.protocol === "https:"
      && url.hostname.toLowerCase() === "api.openai.com"
      && (url.port === "" || url.port === "443")
      && (url.pathname === "/" || url.pathname === "/v1" || url.pathname === "/v1/")
      && url.username === ""
      && url.password === ""
      && url.search === ""
      && url.hash === "";
  } catch {
    return false;
  }
}
