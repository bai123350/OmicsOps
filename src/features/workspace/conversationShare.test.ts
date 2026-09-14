import { describe, expect, it } from "vitest";
import { buildShareBody, buildShareHtml, normalizeShareWidth, selectShareMessages, SHARE_STYLE } from "./conversationShare";

describe("conversation export", () => {
  it("sets export typography on the document root even without a body ancestor", () => {
    const style = document.createElement("style");
    style.textContent = SHARE_STYLE;
    const container = document.createElement("div");
    container.style.font = "30px serif";
    container.innerHTML = buildShareBody([{ id: "a", role: "user", markdown: "Text that must wrap consistently." }]);
    document.head.appendChild(style);
    document.body.appendChild(container);
    try {
      const root = getComputedStyle(container.querySelector(".share-document")!);
      expect(root.fontSize).toBe("16px");
      expect(root.lineHeight).toBe("1.6");
      expect(root.color).toBe("rgb(23, 43, 42)");
    } finally { style.remove(); container.remove(); }
  });
  it("selects only checked messages and redacts literal keywords and credentials", () => {
    const result = selectShareMessages([{ id: "a", role: "user", markdown: "a.* api_key=secret" }, { id: "b", role: "assistant", markdown: "excluded" }], new Set(["a"]), "a.*");
    expect(result).toEqual([{ id: "a", role: "user", markdown: "[REDACTED] api_key=[REDACTED]" }]);
  });
  it("produces standalone safe Markdown tables without scripts or remote images", () => {
    const html = buildShareHtml([{ id: "a", role: "<script>x</script>", markdown: '<script>alert(1)</script>\n\n![tracking](https://example.com/a.png)\n\n| A | B |\n|---|---|\n| 1 | 2 |' }], 800);
    expect(html).toContain("<table>");
    expect(html).not.toMatch(/<script|<img|href=/);
    expect(html).toContain("Content-Security-Policy");
  });
  it("renders inline and display math as native MathML", () => {
    const html = buildShareBody([{ id: "a", role: "assistant", markdown: "Inline $E=mc^2$ and display:\n\n$$\n\\int_0^1 x^2 dx\n$$" }]);
    expect(html).toContain("<math");
    expect(html).toContain("E=mc^2");
    expect(html).toContain("\\int_0^1 x^2 dx");
    expect(html).toContain('display="block"');
    expect(html).not.toContain("katex-html");
  });
  it("keeps fenced math-looking code as code", () => {
    const html = buildShareBody([{ id: "a", role: "assistant", markdown: "```math\nE=mc^2\n```" }]);
    expect(html).not.toContain("<math");
    expect(html).toContain("E=mc^2");
    expect(html).toContain("<pre>");
  });
  it("renders malformed math safely without throwing", () => {
    let html = "";
    expect(() => { html = buildShareBody([{ id: "a", role: "assistant", markdown: "Malformed $\\notacommand$" }]); }).not.toThrow();
    expect(html).toContain("<math");
    expect(html).toContain("\\notacommand");
    expect(html).not.toContain("<script");
  });
  it("does not create remote content for unsafe math commands", () => {
    const html = buildShareHtml([{ id: "a", role: "assistant", markdown: "Unsafe $\\href{https://example.com/remote}{go}$" }], 800);
    expect(html).not.toMatch(/(?:href|src)=/i);
    expect(html).not.toMatch(/<iframe|<object|<embed/i);
  });
  it("bounds width and rejects invalid numbers", () => {
    expect(normalizeShareWidth(12)).toBe(480);
    expect(normalizeShareWidth(9999)).toBe(1600);
    expect(() => normalizeShareWidth(NaN)).toThrow();
  });
});
