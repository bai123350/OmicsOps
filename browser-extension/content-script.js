/*
 * MV3 content scripts are classic scripts, so this file intentionally has no
 * import/export syntax.  Its implementation mirrors content-script-core.js
 * while keeping the loaded page isolated from the service worker and model
 * credentials.
 */
(function installOmicsOpsContentScript() {
  "use strict";

  const MAX_TEXT_LENGTH = 12_000;
  const MAX_ITEMS = 100;
  const clip = (value, maxLength) => String(value || "").replace(/\s+/g, " ").trim().slice(0, maxLength);
  const unique = (values) => [...new Set(values.filter(Boolean))];
  const sensitiveQuery = /(?:^|[_-])(?:access[_-]?token|api[_-]?key|auth|authorization|code|key|password|secret|signature|token)(?:$|[_-])/i;
  const isHttpUrl = (value) => {
    try {
      const parsed = new URL(value);
      return (parsed.protocol === "http:" || parsed.protocol === "https:") && !parsed.username && !parsed.password;
    } catch {
      return false;
    }
  };
  const reportUrl = (value) => {
    try {
      const parsed = new URL(value);
      if ((parsed.protocol !== "http:" && parsed.protocol !== "https:") || parsed.username || parsed.password) return undefined;
      for (const key of [...parsed.searchParams.keys()]) if (sensitiveQuery.test(key)) parsed.searchParams.set(key, "[redacted]");
      return parsed.toString();
    } catch { return undefined; }
  };

  function waitForPageStable({ quietMs = 450, maxWaitMs = 5_000 } = {}) {
    const root = document.documentElement;
    if (!root || typeof MutationObserver !== "function") return Promise.resolve({ stable: true, waited_ms: 0, reason: "observer_unavailable" });
    const quiet = Math.max(0, Math.min(Number(quietMs) || 450, 10_000));
    const maximum = Math.max(quiet, Math.min(Number(maxWaitMs) || 5_000, 30_000));
    return new Promise((resolve) => {
      const started = Date.now();
      let settled = false;
      let quietTimer;
      let maxTimer;
      const observer = new MutationObserver(() => armQuietTimer());
      const finish = (reason) => {
        if (settled) return;
        settled = true;
        clearTimeout(quietTimer);
        clearTimeout(maxTimer);
        observer.disconnect();
        resolve({ stable: reason === "quiet", waited_ms: Date.now() - started, reason });
      };
      function armQuietTimer() {
        clearTimeout(quietTimer);
        quietTimer = setTimeout(() => finish("quiet"), quiet);
      }
      observer.observe(root, { subtree: true, childList: true, attributes: true, characterData: true });
      maxTimer = setTimeout(() => finish("max_wait"), maximum);
      armQuietTimer();
    });
  }

  function detectCaptcha() {
    const signals = [];
    for (const selector of ["iframe[src*='captcha']", "iframe[title*='captcha']", "[id*='captcha']", "[class*='captcha']", "[data-sitekey]", "[aria-label*='captcha']"]) {
      try { if (document.querySelector(selector)) signals.push(`selector:${selector}`); } catch { /* skip unsupported selector */ }
    }
    const text = clip(document.body?.innerText || document.body?.textContent, 50_000);
    if (/\b(?:captcha|re-?captcha|hcaptcha|turnstile)\b/i.test(text)) signals.push("text:captcha");
    if (/verify\s+(?:that\s+)?you\s+are\s+(?:a\s+)?human|i am not a robot/i.test(text)) signals.push("text:human_verification");
    return { detected: signals.length > 0, signals: unique(signals) };
  }

  function scanPage(options = {}) {
    return waitForPageStable(options).then((stability) => {
      const captcha = detectCaptcha();
      const headings = [...document.querySelectorAll("h1,h2,h3")].map((node) => ({
        level: Number(node.tagName.slice(1)) || 0,
        text: clip(node.innerText || node.textContent, 500),
      })).filter((item) => item.text).slice(0, MAX_ITEMS);
      const links = [];
      for (const node of [...document.querySelectorAll("a[href]")].slice(0, MAX_ITEMS * 2)) {
        try {
          const url = new URL(node.getAttribute("href") || node.href, location.href);
          if ((url.protocol === "http:" || url.protocol === "https:") && !url.username && !url.password) links.push({ text: clip(node.innerText || node.textContent, 300), url: reportUrl(url.toString()) });
        } catch { /* skip blocked link */ }
        if (links.length >= MAX_ITEMS) break;
      }
      const forms = [...document.querySelectorAll("form")].slice(0, MAX_ITEMS).map((form) => ({
        action: isHttpUrl(form.action) ? reportUrl(form.action) : undefined,
        method: String(form.method || "get").toLowerCase(),
        fields: [...form.querySelectorAll("input,textarea,select")].slice(0, 50).map((field) => ({ name: clip(field.name || field.id, 200), type: clip(field.type || field.tagName, 80).toLowerCase(), required: Boolean(field.required) })),
      }));
      const candidate = document.querySelector("main,article") || document.body;
      return {
        kind: "structured_page_scan",
        captured_at: new Date().toISOString(),
        page: { url: isHttpUrl(location.href) ? reportUrl(location.href) : undefined, title: clip(document.title, 500), description: clip(document.querySelector("meta[name='description']")?.content, 1_000), language: clip(document.documentElement.lang, 32) },
        headings,
        links,
        forms,
        text: clip(candidate?.innerText || candidate?.textContent, MAX_TEXT_LENGTH),
        stability,
        captcha,
        requires_human: captcha.detected,
      };
    });
  }

  chrome.runtime.onMessage.addListener((message, sender, sendResponse) => {
    if (!message || (message.type !== "omicsops.scan" && message.type !== "omicsops.wait_stable")) return false;
    if (sender?.id && sender.id !== chrome.runtime.id) return false;
    const task = message.type === "omicsops.scan" ? scanPage(message.options || {}) : waitForPageStable(message.options || {});
    task.then((result) => {
      if (message.type === "omicsops.scan" && result.captcha?.detected) {
        // Detection only. No click, token extraction, or solver is attempted.
        chrome.runtime.sendMessage({ type: "omicsops.human_required", notice: { reason: "captcha_detected", url: location.href, signals: result.captcha.signals } });
      }
      sendResponse({ ok: true, result });
    }).catch((error) => sendResponse({ ok: false, error: String(error?.message || error) }));
    return true;
  });
})();
