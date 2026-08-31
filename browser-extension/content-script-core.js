const DEFAULT_QUIET_MS = 450;
const DEFAULT_MAX_WAIT_MS = 5_000;
const MAX_TEXT_LENGTH = 12_000;
const MAX_ITEMS = 100;
const SENSITIVE_QUERY = /(?:^|[_-])(?:access[_-]?token|api[_-]?key|auth|authorization|code|key|password|secret|signature|token)(?:$|[_-])/i;

function clip(value, maxLength) {
  return String(value ?? "").replace(/\s+/g, " ").trim().slice(0, maxLength);
}

function unique(values) {
  return [...new Set(values.filter(Boolean))];
}

function reportUrl(value) {
  try {
    const parsed = new URL(value);
    if ((parsed.protocol !== "http:" && parsed.protocol !== "https:") || parsed.username || parsed.password) return undefined;
    for (const key of [...parsed.searchParams.keys()]) if (SENSITIVE_QUERY.test(key)) parsed.searchParams.set(key, "[redacted]");
    return parsed.toString();
  } catch {
    return undefined;
  }
}

export function waitForPageStable({
  root = globalThis.document?.documentElement,
  quietMs = DEFAULT_QUIET_MS,
  maxWaitMs = DEFAULT_MAX_WAIT_MS,
  observerFactory = globalThis.MutationObserver,
  timer = globalThis.setTimeout,
  clearTimer = globalThis.clearTimeout,
} = {}) {
  const quiet = Math.max(0, Math.min(Number(quietMs) || DEFAULT_QUIET_MS, 10_000));
  const maximum = Math.max(quiet, Math.min(Number(maxWaitMs) || DEFAULT_MAX_WAIT_MS, 30_000));
  if (!root || typeof observerFactory !== "function") {
    return Promise.resolve({ stable: true, waited_ms: 0, reason: "observer_unavailable" });
  }
  return new Promise((resolve) => {
    const started = Date.now();
    let settled = false;
    let quietTimer;
    let maxTimer;
    const observer = new observerFactory(() => {
      armQuietTimer();
    });
    const finish = (reason) => {
      if (settled) return;
      settled = true;
      clearTimer(quietTimer);
      clearTimer(maxTimer);
      observer.disconnect();
      resolve({ stable: reason === "quiet", waited_ms: Date.now() - started, reason });
    };
    function armQuietTimer() {
      clearTimer(quietTimer);
      quietTimer = timer(() => finish("quiet"), quiet);
    }
    observer.observe(root, { subtree: true, childList: true, attributes: true, characterData: true });
    maxTimer = timer(() => finish("max_wait"), maximum);
    armQuietTimer();
  });
}

export function detectCaptcha(documentLike = globalThis.document) {
  if (!documentLike) return { detected: false, signals: [] };
  const signals = [];
  const selectors = [
    "iframe[src*='captcha']",
    "iframe[title*='captcha']",
    "[id*='captcha']",
    "[class*='captcha']",
    "[data-sitekey]",
    "[aria-label*='captcha']",
  ];
  for (const selector of selectors) {
    try {
      if (documentLike.querySelector(selector)) signals.push(`selector:${selector}`);
    } catch {
      // A browser's selector implementation may reject a vendor-specific selector.
    }
  }
  const bodyText = clip(documentLike.body?.innerText || documentLike.body?.textContent || "", 50_000);
  if (/\b(?:captcha|re-?captcha|hcaptcha|turnstile)\b/i.test(bodyText)) signals.push("text:captcha");
  if (/verify\s+(?:that\s+)?you\s+are\s+(?:a\s+)?human|i am not a robot/i.test(bodyText)) signals.push("text:human_verification");
  return { detected: signals.length > 0, signals: unique(signals) };
}

function collectHeadings(documentLike) {
  return [...(documentLike.querySelectorAll?.("h1,h2,h3") || [])]
    .map((node) => ({ level: Number(node.tagName?.slice(1)) || 0, text: clip(node.innerText || node.textContent, 500) }))
    .filter((item) => item.text)
    .slice(0, MAX_ITEMS);
}

function collectLinks(documentLike, locationLike) {
  const output = [];
  for (const node of [...(documentLike.querySelectorAll?.("a[href]") || [])].slice(0, MAX_ITEMS * 2)) {
    const raw = node.getAttribute?.("href") || node.href;
    try {
      const url = new URL(raw, locationLike?.href || undefined);
      if ((url.protocol === "http:" || url.protocol === "https:") && url.username === "" && url.password === "") {
        const reported = reportUrl(url.toString());
        if (reported) output.push({ text: clip(node.innerText || node.textContent, 300), url: reported });
      }
    } catch {
      // Skip javascript:, data:, malformed, and other non-web links.
    }
    if (output.length >= MAX_ITEMS) break;
  }
  return output;
}

function collectForms(documentLike) {
  return [...(documentLike.querySelectorAll?.("form") || [])].slice(0, MAX_ITEMS).map((form) => ({
    action: reportUrl(form.action),
    method: String(form.method || "get").toLowerCase(),
    fields: [...(form.querySelectorAll?.("input,textarea,select") || [])].slice(0, 50).map((field) => ({
      name: clip(field.name || field.id, 200),
      type: clip(field.type || field.tagName, 80).toLowerCase(),
      required: Boolean(field.required),
    })),
  }));
}

function pageText(documentLike) {
  const candidate = documentLike.querySelector?.("main,article") || documentLike.body;
  return clip(candidate?.innerText || candidate?.textContent || "", MAX_TEXT_LENGTH);
}

export async function scanPage({
  documentLike = globalThis.document,
  locationLike = globalThis.location,
  quietMs = DEFAULT_QUIET_MS,
  maxWaitMs = DEFAULT_MAX_WAIT_MS,
} = {}) {
  if (!documentLike) throw new Error("document is unavailable");
  const stability = await waitForPageStable({ root: documentLike.documentElement, quietMs, maxWaitMs });
  const captcha = detectCaptcha(documentLike);
  return {
    kind: "structured_page_scan",
    captured_at: new Date().toISOString(),
    page: {
      url: reportUrl(locationLike?.href),
      title: clip(documentLike.title, 500),
      description: clip(documentLike.querySelector?.("meta[name='description']")?.content, 1_000),
      language: clip(documentLike.documentElement?.lang, 32),
    },
    headings: collectHeadings(documentLike),
    links: collectLinks(documentLike, locationLike),
    forms: collectForms(documentLike),
    text: pageText(documentLike),
    stability,
    captcha,
    requires_human: captcha.detected,
  };
}

export function humanPauseNotice(tabUrl, signals = []) {
  return {
    type: "human_required",
    reason: "captcha_detected",
    url: reportUrl(tabUrl),
    signals: unique(signals).slice(0, 20),
    message: "CAPTCHA detected; complete it in the browser, then resume the same run.",
  };
}
