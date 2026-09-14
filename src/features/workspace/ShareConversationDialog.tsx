import { useMemo, useRef, useState } from "react";
import { saveConversationExport, type ConversationExportFormat } from "../../conversation-export-api";
import { useWindowEscapeLayer } from "../settings/BrowserSettings";
import { buildShareBody, buildShareHtml, buildSharePng, normalizeShareWidth, selectShareMessages, type ShareMessage } from "./conversationShare";
import "./share-conversation.css";

export function ShareConversationDialog({ messages, locale, onClose }: { messages: ShareMessage[]; locale: "zh-CN" | "en-US"; onClose: () => void }) {
  const zh = locale === "zh-CN";
  const [selected, setSelected] = useState(() => new Set(messages.filter((message) => message.role !== "thinking").map((message) => message.id)));
  const [keywords, setKeywords] = useState("");
  const [format, setFormat] = useState<ConversationExportFormat>("png");
  const [width, setWidth] = useState("800");
  const [busy, setBusy] = useState(false);
  const saving = useRef(false);
  const [error, setError] = useState("");
  const [saved, setSaved] = useState("");
  useWindowEscapeLayer(true, () => { if (!saving.current) onClose(); });
  const preview = useMemo(() => selectShareMessages(messages, selected, keywords), [messages, selected, keywords]);
  async function exportConversation() {
    if (saving.current || !preview.length) return;
    saving.current = true; setBusy(true); setError(""); setSaved("");
    try {
      const exportWidth = normalizeShareWidth(width.trim() ? Number(width) : NaN);
      setWidth(String(exportWidth));
      const blob = format === "html" ? new Blob([buildShareHtml(preview, exportWidth)], { type: "text/html;charset=utf-8" }) : await buildSharePng(preview, exportWidth);
      const path = await saveConversationExport(format, blob);
      if (path !== null) setSaved(path === "download" ? (zh ? "已发起浏览器下载" : "Browser download started") : path);
    } catch (reason) { setError(String(reason)); }
    finally { saving.current = false; setBusy(false); }
  }
  return <div className="share-backdrop"><section className="share-dialog" role="dialog" aria-modal="true" aria-label={zh ? "分享会话" : "Share conversation"}>
    <header><h2>{zh ? "分享会话" : "Share conversation"}</h2><button onClick={onClose} disabled={busy}>{zh ? "关闭" : "Close"}</button></header>
    <fieldset disabled={busy}><div className="share-controls"><button onClick={() => setSelected(new Set(messages.map((message) => message.id)))}>{zh ? "全选" : "Select all"}</button><button onClick={() => setSelected(new Set())}>{zh ? "全不选" : "Select none"}</button><span>{preview.length} / {messages.length}</span></div>
      <div className="share-selection">{messages.map((message, index) => <label key={message.id}><input type="checkbox" checked={selected.has(message.id)} onChange={() => setSelected((current) => { const next = new Set(current); if (next.has(message.id)) next.delete(message.id); else next.add(message.id); return next; })} />{index + 1}. {message.role}</label>)}</div>
      <label>{zh ? "脱敏关键词（每行一个）" : "Redaction keywords (one per line)"}<textarea value={keywords} onChange={(event) => setKeywords(event.target.value)} /></label>
      <div className="share-controls"><label>{zh ? "格式" : "Format"}<select value={format} onChange={(event) => setFormat(event.target.value as ConversationExportFormat)}><option value="png">PNG</option><option value="html">HTML</option></select></label><label>{zh ? "宽度（480–1600 px）" : "Width (480–1600 px)"}<input type="number" min="480" max="1600" value={width} onChange={(event) => setWidth(event.target.value)} /></label></div>
    </fieldset>
    <div className="share-preview" aria-label={zh ? "预览" : "Preview"} dangerouslySetInnerHTML={{ __html: buildShareBody(preview) }} />
    {error && <p role="alert">{error}</p>}{saved && <p role="status">{saved}</p>}
    <footer><button disabled={busy || !preview.length} onClick={() => void exportConversation()}>{busy ? (zh ? "导出中…" : "Exporting…") : (zh ? "导出" : "Export")}</button></footer>
  </section></div>;
}
