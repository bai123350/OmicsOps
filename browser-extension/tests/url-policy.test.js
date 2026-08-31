import test from "node:test";
import assert from "node:assert/strict";
import { assertAllowedUrl, isAllowedUrl, sanitizeUrlForReport } from "../url-policy.js";

test("URL policy accepts only HTTP(S)", () => {
  assert.equal(assertAllowedUrl("https://example.org/paper"), "https://example.org/paper");
  for (const blocked of ["chrome://settings", "file:///secret", "javascript:alert(1)", "data:text/html,hi", "ftp://example.org/a", "https://user:pass@example.org/"]) {
    assert.equal(isAllowedUrl(blocked), false, blocked);
    assert.throws(() => assertAllowedUrl(blocked), /URL/);
  }
  assert.throws(() => assertAllowedUrl("https://example.org/paper?session_token=secret"), /sensitive/);
});

test("reported URLs redact credential-like query values", () => {
  assert.equal(sanitizeUrlForReport("https://example.org/a?token=secret&x=1"), "https://example.org/a?token=%5Bredacted%5D&x=1");
  assert.equal(sanitizeUrlForReport("file:///tmp/a"), "[blocked-url]");
});
