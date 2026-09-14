import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import ReactMarkdown from "react-markdown";
import rehypeKatex from "rehype-katex";
import remarkGfm from "remark-gfm";
import remarkMath from "remark-math";

export type ShareMessage = { id: string; role: string; markdown: string };
export const SHARE_STYLE = `*{box-sizing:border-box}body{margin:0;background:#fff}.share-document{color:#172b2a;font:16px/1.6 system-ui,"Segoe UI",sans-serif;padding:32px;overflow-wrap:anywhere}article{padding:16px 0;border-bottom:1px solid #ddd}h2{font-size:14px;color:#40645f}pre{white-space:pre-wrap;background:#f4f6f6;padding:12px}table{border-collapse:collapse;width:100%;table-layout:fixed}td,th{border:1px solid #ccd6d4;padding:8px;overflow-wrap:anywhere}blockquote{border-left:3px solid #a6bcb7;padding-left:16px}img{display:none}`;
export function normalizeShareWidth(value: number): number {
  if (!Number.isFinite(value)) throw new Error("Width must be a number (480–1600 px).");
  return Math.min(1600, Math.max(480, Math.round(value)));
}
export function selectShareMessages(messages: ShareMessage[], selected: Set<string>, keywords: string): ShareMessage[] {
  const secrets = keywords.split(/\r?\n/).filter(Boolean).sort((a, b) => b.length - a.length);
  const redact = (text: string) => {
    for (const secret of secrets) text = text.split(secret).join("[REDACTED]");
    // Mirrors omicsops-core/redaction.rs for exports made in browser preview too.
    return text.replace(/(api[_-]?key\s*[=:]\s*)[^\s,;]+/gi, "$1[REDACTED]").replace(/(authorization:\s*bearer\s+)[^\s]+/gi, "$1[REDACTED]");
  };
  return messages.filter((message) => selected.has(message.id)).map((message) => ({ ...message, role: redact(message.role), markdown: redact(message.markdown) }));
}

type MarkdownNode = { type?: string; lang?: string | null; children?: MarkdownNode[] };

function preserveFencedMathAsCode() {
  return (tree: MarkdownNode) => {
    const visit = (node: MarkdownNode) => {
      if (node.type === "code" && node.lang?.toLowerCase() === "math") node.lang = "text";
      node.children?.forEach(visit);
    };
    visit(tree);
  };
}

export function buildShareBody(messages: ShareMessage[]): string {
  return renderToStaticMarkup(createElement("div", { className: "share-document" }, messages.map((message) => createElement("article", { key: message.id }, createElement("h2", null, message.role), createElement(ReactMarkdown, { remarkPlugins: [remarkGfm, preserveFencedMathAsCode, remarkMath], rehypePlugins: [[rehypeKatex, { output: "mathml", trust: false }]], components: { img: ({ alt }) => createElement("span", null, alt ? `[${alt}]` : ""), a: ({ children }) => createElement("span", null, children) }, children: message.markdown })))));
}
export function buildShareHtml(messages: ShareMessage[], width: number): string {
  return `<!DOCTYPE html><html><head><meta charset="utf-8"><meta http-equiv="Content-Security-Policy" content="default-src 'none'; style-src 'unsafe-inline'; base-uri 'none'; form-action 'none'"><title>OmicsOps conversation</title><style>${SHARE_STYLE}body{max-width:${normalizeShareWidth(width)}px;margin:auto}</style></head><body>${buildShareBody(messages)}</body></html>`;
}
export async function buildSharePng(messages: ShareMessage[], requestedWidth: number): Promise<Blob> {
  const width = normalizeShareWidth(requestedWidth);
  const body = buildShareBody(messages);
  const frame = document.createElement("iframe");
  frame.style.cssText = `position:fixed;left:-10000px;top:0;width:${width}px;height:1px;border:0`;
  document.body.appendChild(frame);
  try {
    const doc = frame.contentDocument;
    if (!doc) throw new Error("Cannot measure export layout.");
    doc.open(); doc.write(buildShareHtml(messages, width)); doc.close();
    await doc.fonts?.ready;
    const height = Math.ceil(doc.body.scrollHeight);
    if (!height || height > 16000 || width * height > 16000000) throw new Error("PNG is too tall. Select fewer messages or export HTML.");
    const svg = `<svg xmlns="http://www.w3.org/2000/svg" width="${width}" height="${height}"><foreignObject width="100%" height="100%"><div xmlns="http://www.w3.org/1999/xhtml"><style>${SHARE_STYLE}</style>${body}</div></foreignObject></svg>`;
    const img = new Image();
    await new Promise<void>((resolve, reject) => { img.onload = () => resolve(); img.onerror = () => reject(new Error("PNG rendering failed.")); img.src = `data:image/svg+xml;charset=utf-8,${encodeURIComponent(svg)}`; });
    const canvas = document.createElement("canvas"); canvas.width = width; canvas.height = height;
    const context = canvas.getContext("2d");
    if (!context) throw new Error("PNG rendering is unavailable.");
    context.fillStyle = "white"; context.fillRect(0, 0, width, height); context.drawImage(img, 0, 0);
    return await new Promise<Blob>((resolve, reject) => canvas.toBlob((blob) => blob ? resolve(blob) : reject(new Error("PNG encoding failed.")), "image/png"));
  } finally { frame.remove(); }
}
