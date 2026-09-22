import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";

import { useWindowEscapeLayer } from "../settings/BrowserSettings";
import type { Locale } from "./copy";
import "./MessageSelectionActions.css";

const MAX_SELECTION_BYTES = 16 * 1024;

export interface MessageSelectionQuote {
  text: string;
  role: "user" | "assistant";
}

interface SelectionSnapshot extends MessageSelectionQuote {
  messageId?: string;
  left: number;
  top: number;
  bottom: number;
}

interface ToolbarPosition {
  left: number;
  top: number;
}

interface MessageSelectionActionsProps {
  enabled: boolean;
  locale: Locale;
  projectId: string;
  conversationId: string;
  onQuote: (selection: MessageSelectionQuote) => void;
  onSave?: (selection: MessageSelectionQuote & { messageId: string }) => Promise<void>;
}

function selectionBody(node: Node | null): HTMLElement | null {
  const element = node instanceof Element ? node : node?.parentElement;
  return element?.closest<HTMLElement>("[data-message-selection-body]") ?? null;
}

function captureSelection(projectId: string, conversationId: string): SelectionSnapshot | null {
  const selection = window.getSelection();
  if (!selection || selection.rangeCount !== 1 || selection.isCollapsed) return null;

  const anchorBody = selectionBody(selection.anchorNode);
  const focusBody = selectionBody(selection.focusNode);
  if (!anchorBody || anchorBody !== focusBody) return null;
  if (anchorBody.dataset.projectId !== projectId || anchorBody.dataset.conversationId !== conversationId) return null;

  const role = anchorBody.dataset.messageRole;
  if (role !== "user" && role !== "assistant") return null;

  const range = selection.getRangeAt(0);
  if (!anchorBody.contains(range.commonAncestorContainer)) return null;
  const text = selection.toString();
  if (!text.trim() || new TextEncoder().encode(text).byteLength > MAX_SELECTION_BYTES) return null;

  const rect = range.getBoundingClientRect();
  return {
    text,
    role,
    messageId: anchorBody.dataset.messageId,
    left: rect.left,
    top: rect.top,
    bottom: rect.bottom,
  };
}

export function MessageSelectionActions({
  enabled,
  locale,
  projectId,
  conversationId,
  onQuote,
  onSave,
}: MessageSelectionActionsProps) {
  const [snapshot, setSnapshot] = useState<SelectionSnapshot | null>(null);
  const [position, setPosition] = useState<ToolbarPosition | null>(null);
  const [error, setError] = useState<string | null>(null);
  const toolbarRef = useRef<HTMLDivElement | null>(null);
  const generation = useRef(0);
  const saveBusy = useRef(false);
  const close = useCallback(() => {
    generation.current += 1;
    setSnapshot(null);
    setPosition(null);
    setError(null);
  }, []);

  useWindowEscapeLayer(snapshot !== null, close);

  useEffect(() => {
    close();
  }, [close, conversationId, enabled, projectId]);

  useEffect(() => {
    if (!enabled) return undefined;

    const refresh = () => {
      generation.current += 1;
      setError(null);
      setPosition(null);
      setSnapshot(captureSelection(projectId, conversationId));
    };
    const dismiss = () => close();
    document.addEventListener("selectionchange", refresh);
    window.addEventListener("blur", dismiss);
    window.addEventListener("resize", dismiss);
    window.addEventListener("scroll", dismiss, true);
    const scaleObserver = new MutationObserver(dismiss);
    scaleObserver.observe(document.documentElement, { attributes: true, attributeFilter: ["data-omicsops-scale"] });
    return () => {
      document.removeEventListener("selectionchange", refresh);
      window.removeEventListener("blur", dismiss);
      window.removeEventListener("resize", dismiss);
      window.removeEventListener("scroll", dismiss, true);
      scaleObserver.disconnect();
    };
  }, [close, conversationId, enabled, projectId]);

  useLayoutEffect(() => {
    if (!snapshot || !toolbarRef.current) return;
    const rect = toolbarRef.current.getBoundingClientRect();
    const maxLeft = Math.max(8, window.innerWidth - rect.width - 8);
    const maxTop = Math.max(8, window.innerHeight - rect.height - 8);
    const above = snapshot.top - rect.height - 8;
    const preferredTop = above >= 8 ? above : snapshot.bottom + 8;
    setPosition({
      left: Math.max(8, Math.min(snapshot.left, maxLeft)),
      top: Math.max(8, Math.min(preferredTop, maxTop)),
    });
  }, [error, locale, snapshot]);

  if (!snapshot) return null;

  const copy = async () => {
    const requestGeneration = generation.current;
    try {
      await navigator.clipboard.writeText(snapshot.text);
      if (requestGeneration !== generation.current) return;
      window.getSelection()?.removeAllRanges();
      close();
    } catch {
      if (requestGeneration !== generation.current) return;
      setError(locale === "zh-CN"
        ? "复制失败。请重试或手动复制所选文字。"
        : "Copy failed. Retry or select the text manually.");
    }
  };

  const quote = () => {
    onQuote({ text: snapshot.text, role: snapshot.role });
    window.getSelection()?.removeAllRanges();
    close();
  };

  return createPortal(<div
    ref={toolbarRef}
    className="message-selection-actions"
    role="toolbar"
    aria-label={locale === "zh-CN" ? "消息选区操作" : "Message selection actions"}
    style={position ? { left: position.left, top: position.top } : { left: 0, top: 0, visibility: "hidden" }}
  >
    <button type="button" onMouseDown={(event) => event.preventDefault()} onClick={() => void copy()}>
      {locale === "zh-CN" ? "复制" : "Copy selection"}
    </button>
    <button type="button" onMouseDown={(event) => event.preventDefault()} onClick={quote}>
      {locale === "zh-CN" ? "引用到草稿" : "Quote in draft"}
    </button>
    {onSave && snapshot.messageId && <button type="button" onMouseDown={(event) => event.preventDefault()} onClick={async () => {
      if (saveBusy.current || !snapshot.messageId) return;
      saveBusy.current = true;
      const current = generation.current;
      try { await onSave({ text: snapshot.text, role: snapshot.role, messageId: snapshot.messageId }); if (current === generation.current) { window.getSelection()?.removeAllRanges(); close(); } }
      catch { if (current === generation.current) setError(locale === "zh-CN" ? "无法收藏此选区，请重试或收藏完整消息。" : "Could not save this selection. Retry or save the full message."); }
      finally { saveBusy.current = false; }
    }}>{locale === "zh-CN" ? "收藏选区到资料库" : "Save selection to library"}</button>}
    {error ? <span role="alert">{error}</span> : null}
  </div>, document.body);
}
