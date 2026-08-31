/**
 * execute_js is explicit and caller initiated.  The bridge never generates
 * page prompts, asks a page's AI to act, or forwards model credentials.
 */

const FORBIDDEN_PAGE_AI_PATTERNS = Object.freeze([
  /\b(?:chatgpt|openai|anthropic|claude|gemini|copilot|perplexity)\b/i,
  /\b(?:window|globalThis|document)\.(?:openai|anthropic|gemini|copilot)\b/i,
  /\b(?:prompt|confirm)\s*\(/i,
  /\/api\/(?:chat|completions|messages)\b/i,
]);

const FORBIDDEN_SENSITIVE_API_PATTERNS = Object.freeze([
  /\bdocument\s*(?:\.\s*cookie|\[\s*["']cookie["']\s*\])/i,
  /\b(?:localStorage|sessionStorage|indexedDB)\b/i,
  /\bnavigator\s*(?:\.\s*(?:credentials|clipboard)|\[\s*["'](?:credentials|clipboard)["']\s*\])/i,
  /\b(?:fetch|XMLHttpRequest|WebSocket|EventSource|sendBeacon)\s*(?:\.|\()/i,
]);

export class ExecuteJsPolicyError extends Error {
  constructor(message) {
    super(message);
    this.name = "ExecuteJsPolicyError";
    this.code = "EXECUTE_JS_BLOCKED";
  }
}

export function assertExecuteJsAllowed(code, { maxLength = 64 * 1024 } = {}) {
  if (typeof code !== "string" || code.length === 0 || code.length > maxLength) {
    throw new ExecuteJsPolicyError("code must be a non-empty string within the size limit");
  }
  if (FORBIDDEN_PAGE_AI_PATTERNS.some((pattern) => pattern.test(code))) {
    throw new ExecuteJsPolicyError("page AI prompting and model API calls are not permitted");
  }
  if (FORBIDDEN_SENSITIVE_API_PATTERNS.some((pattern) => pattern.test(code))) {
    throw new ExecuteJsPolicyError("credential, storage, clipboard, and page-network APIs are not permitted");
  }
  return code;
}

export function evaluateInIsolatedWorld(source) {
  // This function is serialized by chrome.scripting.executeScript.  It has no
  // bridge objects or model APIs in its scope and is run only on a controlled
  // tab selected by the service worker.
  const executor = new Function(`"use strict"; return (async () => {\n${source}\n})()`);
  return executor();
}
