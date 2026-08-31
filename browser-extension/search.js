import { assertAllowedUrl } from "./url-policy.js";

const SEARCH_ENDPOINTS = Object.freeze({
  default: "https://www.google.com/search",
  google: "https://www.google.com/search",
  bing: "https://www.bing.com/search",
  duckduckgo: "https://duckduckgo.com/",
});

export function buildSearchUrl(query, provider = "default") {
  if (typeof query !== "string" || query.trim().length === 0 || query.length > 2000) {
    const error = new Error("query must be a non-empty string within the size limit");
    error.code = "INVALID_PAYLOAD";
    throw error;
  }
  const endpoint = SEARCH_ENDPOINTS[provider];
  if (!endpoint) {
    const error = new Error("unsupported search provider");
    error.code = "INVALID_PAYLOAD";
    throw error;
  }
  const url = new URL(endpoint);
  url.searchParams.set("q", query);
  return assertAllowedUrl(url.toString());
}

export const SEARCH_PROVIDERS = Object.freeze(Object.keys(SEARCH_ENDPOINTS));
