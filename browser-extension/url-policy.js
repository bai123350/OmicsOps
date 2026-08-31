/** URLs accepted by browser commands and reported by the bridge. */

export const ALLOWED_URL_PROTOCOLS = Object.freeze(["http:", "https:"]);
const SENSITIVE_QUERY_KEYS = /(?:^|[_-])(?:access[_-]?token|api[_-]?key|auth|authorization|code|key|password|secret|signature|token)(?:$|[_-])/i;

export class UrlPolicyError extends Error {
  constructor(message, details = undefined) {
    super(message);
    this.name = "UrlPolicyError";
    this.code = "URL_BLOCKED";
    this.details = details;
  }
}

export function parseAllowedUrl(value, { maxLength = 8192 } = {}) {
  if (typeof value !== "string" || value.length === 0 || value.length > maxLength) {
    throw new UrlPolicyError("URL must be a non-empty string within the size limit");
  }
  if (value.trim() !== value || /[\u0000-\u001f\u007f]/.test(value)) {
    throw new UrlPolicyError("URL contains leading/trailing whitespace or control characters");
  }
  let parsed;
  try {
    parsed = new URL(value);
  } catch {
    throw new UrlPolicyError("URL is not valid");
  }
  if (!ALLOWED_URL_PROTOCOLS.includes(parsed.protocol)) {
    throw new UrlPolicyError("only http and https URLs are allowed", { protocol: parsed.protocol });
  }
  if (!parsed.hostname || parsed.username || parsed.password) {
    throw new UrlPolicyError("URL must have a host and must not contain credentials");
  }
  return parsed;
}

export function assertAllowedUrl(value, options = undefined) {
  const parsed = parseAllowedUrl(value, options);
  if ([...parsed.searchParams.keys()].some((key) => SENSITIVE_QUERY_KEYS.test(key))) {
    throw new UrlPolicyError("URL contains a sensitive query parameter");
  }
  return parsed.toString();
}

export function isAllowedUrl(value) {
  try {
    parseAllowedUrl(value);
    return true;
  } catch {
    return false;
  }
}

export function isBlockedUrl(value) {
  return !isAllowedUrl(value);
}

export function sanitizeUrlForReport(value) {
  let parsed;
  try {
    parsed = parseAllowedUrl(value);
  } catch {
    return "[blocked-url]";
  }
  for (const key of [...parsed.searchParams.keys()]) {
    if (SENSITIVE_QUERY_KEYS.test(key)) {
      parsed.searchParams.set(key, "[redacted]");
    }
  }
  return parsed.toString();
}

export function originForReport(value) {
  const parsed = parseAllowedUrl(value);
  return parsed.origin;
}

export function resolveAllowedUrl(value, base) {
  if (typeof value !== "string" || typeof base !== "string") {
    throw new UrlPolicyError("URL and base URL must be strings");
  }
  let resolved;
  try {
    resolved = new URL(value, base).toString();
  } catch {
    throw new UrlPolicyError("URL is not valid");
  }
  return assertAllowedUrl(resolved);
}
