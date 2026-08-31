import test from "node:test";
import assert from "node:assert/strict";
import { detectCaptcha, scanPage, waitForPageStable } from "../content-script-core.js";

test("stable wait has a deterministic observer-unavailable fallback", async () => {
  assert.deepEqual(await waitForPageStable({ root: null }), { stable: true, waited_ms: 0, reason: "observer_unavailable" });
});

test("CAPTCHA is detected but never solved", () => {
  const documentLike = {
    body: { innerText: "Please verify that you are human" },
    querySelector: (selector) => selector.includes("captcha") ? { id: "captcha" } : null,
  };
  const result = detectCaptcha(documentLike);
  assert.equal(result.detected, true);
  assert.ok(result.signals.some((signal) => signal.startsWith("selector:")));
});

test("scan returns structure and omits form values", async () => {
  const heading = { tagName: "H1", innerText: "A paper" };
  const link = { getAttribute: () => "/paper", innerText: "Paper" };
  const field = { name: "email", id: "email", type: "email", required: true, value: "secret@example.org" };
  const form = { action: "https://example.org/search", method: "get", querySelectorAll: () => [field] };
  const documentLike = {
    documentElement: { lang: "en" },
    title: "A paper",
    body: { innerText: "Visible paper text" },
    querySelector: (selector) => selector === "main,article" ? null : selector.startsWith("meta") ? { content: "Description" } : null,
    querySelectorAll: (selector) => selector === "h1,h2,h3" ? [heading] : selector === "a[href]" ? [link] : selector === "form" ? [form] : [],
  };
  const scan = await scanPage({ documentLike, locationLike: { href: "https://example.org/paper" } });
  assert.equal(scan.kind, "structured_page_scan");
  assert.equal(scan.headings[0].text, "A paper");
  assert.equal(scan.links[0].url, "https://example.org/paper");
  assert.deepEqual(scan.forms[0].fields[0], { name: "email", type: "email", required: true });
  assert.equal("value" in scan.forms[0].fields[0], false);
});
