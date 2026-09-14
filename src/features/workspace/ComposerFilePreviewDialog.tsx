import { useCallback, useEffect, useLayoutEffect, useRef, useState, type KeyboardEvent as ReactKeyboardEvent } from "react";
import { CircleAlert, FileText, LoaderCircle, RefreshCw, Quote, X } from "lucide-react";

import { createComposerQuote, previewComposerFileText, type WorkspaceFileReference } from "../../composer-file-api";
import type { ComposerCatalogItem, ComposerTextPreview } from "../../types";
import { useWindowEscapeLayer } from "../settings/BrowserSettings";
import "./composer-file-preview.css";

const MAX_PREVIEW_BYTES = 64 * 1024;
const MAX_QUOTE_BYTES = 8 * 1024;

const FOCUSABLE_SELECTOR = [
  "a[href]",
  "button:not([disabled])",
  "input:not([disabled])",
  "select:not([disabled])",
  "textarea:not([disabled])",
  "[contenteditable=\"true\"]",
  "[tabindex]:not([tabindex=\"-1\"])",
].join(",");

interface SelectionRange {
  start: number;
  end: number;
}

export interface ComposerFilePreviewDialogProps {
  reference: WorkspaceFileReference;
  conversationId: string;
  zh: boolean;
  disabled?: boolean;
  onClose: () => void;
  onAttach: (item: ComposerCatalogItem) => void;
}

function utf8ByteLength(value: string): number {
  return new TextEncoder().encode(value).length;
}

function focusableElements(container: HTMLElement | null): HTMLElement[] {
  if (!container) return [];
  return Array.from(container.querySelectorAll<HTMLElement>(FOCUSABLE_SELECTOR)).filter((element) => {
    if (!element.isConnected || element.hidden || element.getAttribute("aria-hidden") === "true") return false;
    if (element.closest("fieldset[disabled]")) return false;
    if (typeof window === "undefined") return true;
    const style = window.getComputedStyle(element);
    return style.display !== "none" && style.visibility !== "hidden";
  });
}

function previewErrorMessage(zh: boolean): string {
  return zh ? "无法加载此文件预览，请重试。" : "Could not load this file preview. Please retry.";
}

function quoteErrorMessage(zh: boolean): string {
  return zh ? "无法创建文本引用，请重试。" : "Could not create the quote. Please retry.";
}

function sourceLabel(reference: WorkspaceFileReference, zh: boolean): string {
  if (reference.backend_id === "local") return zh ? "本地" : "Local";
  if (reference.backend_id.startsWith("ssh:")) return "SSH";
  return zh ? "工作区" : "Workspace";
}

function isMatchingPreview(preview: ComposerTextPreview, reference: WorkspaceFileReference): boolean {
  return preview.project_id === reference.project_id
    && preview.backend_id === reference.backend_id
    && preview.relative_path === reference.relative_path;
}

function validQuoteItem(item: ComposerCatalogItem, projectId: string): boolean {
  return Boolean(item && item.reference && item.reference.kind === "quote" && item.reference.project_id === projectId);
}

export function ComposerFilePreviewDialog({ reference, conversationId, zh, disabled = false, onClose, onAttach }: ComposerFilePreviewDialogProps) {
  const requestIdentity = JSON.stringify([reference.project_id, reference.backend_id, reference.relative_path, conversationId]);
  const [preview, setPreview] = useState<ComposerTextPreview | null>(null);
  const [loadedIdentity, setLoadedIdentity] = useState("");
  const [previewLoading, setPreviewLoading] = useState(true);
  const [previewError, setPreviewError] = useState("");
  const [retryVersion, setRetryVersion] = useState(0);
  const [quoteBusy, setQuoteBusy] = useState(false);
  const [quoteError, setQuoteError] = useState("");
  const [selection, setSelection] = useState<SelectionRange>({ start: 0, end: 0 });
  const mountedRef = useRef(false);
  const generationRef = useRef(0);
  const quoteBusyRef = useRef(false);
  const requestIdentityRef = useRef(requestIdentity);
  const selectionRef = useRef<SelectionRange>({ start: 0, end: 0 });
  const restoreSelectionRef = useRef<SelectionRange | null>(null);
  const dialogRef = useRef<HTMLElement>(null);
  const closeRef = useRef<HTMLButtonElement>(null);
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const previousFocusRef = useRef<HTMLElement | null>(null);
  const onCloseRef = useRef(onClose);
  const onAttachRef = useRef(onAttach);
  const zhRef = useRef(zh);

  onCloseRef.current = onClose;
  onAttachRef.current = onAttach;
  zhRef.current = zh;
  requestIdentityRef.current = requestIdentity;

  useLayoutEffect(() => {
    mountedRef.current = true;
    if (previousFocusRef.current === null && typeof document !== "undefined") {
      previousFocusRef.current = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    }
    closeRef.current?.focus();

    return () => {
      mountedRef.current = false;
      generationRef.current += 1;
      quoteBusyRef.current = false;

      const previous = previousFocusRef.current;
      const active = typeof document === "undefined" ? null : document.activeElement;
      const dialog = dialogRef.current;
      const focusIsInDialog = Boolean(dialog && active && dialog.contains(active));
      const focusIsUnowned = active === null || (typeof document !== "undefined" && active === document.body);
      if (previous?.isConnected && (focusIsInDialog || focusIsUnowned)) previous.focus();
    };
  }, []);

  useEffect(() => {
    const generation = ++generationRef.current;
    setPreview(null);
    setLoadedIdentity("");
    setPreviewLoading(true);
    setPreviewError("");
    setQuoteError("");
    const nextSelection = { start: 0, end: 0 };
    selectionRef.current = nextSelection;
    setSelection(nextSelection);

    void previewComposerFileText(reference)
      .then((result) => {
        if (!mountedRef.current || generation !== generationRef.current || requestIdentityRef.current !== requestIdentity) return;
        if (!result || typeof result.text !== "string" || !isMatchingPreview(result, reference) || utf8ByteLength(result.text) > MAX_PREVIEW_BYTES) {
          throw new Error("invalid preview");
        }
        setPreview(result);
        setLoadedIdentity(requestIdentity);
      })
      .catch(() => {
        if (!mountedRef.current || generation !== generationRef.current || requestIdentityRef.current !== requestIdentity) return;
        setPreview(null);
        setLoadedIdentity("");
        setPreviewError(previewErrorMessage(zhRef.current));
      })
      .finally(() => {
        if (!mountedRef.current || generation !== generationRef.current || requestIdentityRef.current !== requestIdentity) return;
        setPreviewLoading(false);
      });
  }, [conversationId, reference.backend_id, reference.project_id, reference.relative_path, requestIdentity, retryVersion]);

  useWindowEscapeLayer(true, useCallback(() => onCloseRef.current(), []));

  useLayoutEffect(() => {
    const pendingSelection = restoreSelectionRef.current;
    if (!pendingSelection || !textareaRef.current || !preview) return;
    const max = preview.text.length;
    const start = Math.max(0, Math.min(pendingSelection.start, max));
    const end = Math.max(start, Math.min(pendingSelection.end, max));
    textareaRef.current.setSelectionRange(start, end);
    selectionRef.current = { start, end };
    setSelection({ start, end });
    restoreSelectionRef.current = null;
  }, [preview, quoteError, quoteBusy]);

  const rememberSelection = useCallback((): SelectionRange => {
    const textarea = textareaRef.current;
    const current = textarea && preview
      ? { start: textarea.selectionStart, end: textarea.selectionEnd }
      : selectionRef.current;
    const max = preview?.text.length ?? 0;
    const next = {
      start: Math.max(0, Math.min(current.start, max)),
      end: Math.max(0, Math.min(current.end, max)),
    };
    selectionRef.current = next;
    setSelection(next);
    return next;
  }, [preview]);

  function handleSelect() {
    rememberSelection();
    setQuoteError("");
  }

  function preserveSelection(range: SelectionRange) {
    restoreSelectionRef.current = range;
    if (textareaRef.current && preview) {
      const max = preview.text.length;
      const start = Math.max(0, Math.min(range.start, max));
      const end = Math.max(start, Math.min(range.end, max));
      textareaRef.current.setSelectionRange(start, end);
    }
  }

  async function quoteSelectedText() {
    const previewReady = Boolean(preview && loadedIdentity === requestIdentity && !previewLoading && !previewError);
    if (quoteBusyRef.current || disabled || !previewReady || !preview) return;
    const range = rememberSelection();
    const text = preview.text.slice(range.start, range.end);
    preserveSelection(range);
    if (!text) {
      setQuoteError(zh ? "请先选择文本。" : "Select some text first.");
      return;
    }
    if (utf8ByteLength(text) > MAX_QUOTE_BYTES) {
      setQuoteError(zh ? "所选文本不能超过 8 KiB。" : "Selected text exceeds 8 KiB.");
      return;
    }

    const generation = generationRef.current;
    quoteBusyRef.current = true;
    setQuoteBusy(true);
    setQuoteError("");
    try {
      const item = await createComposerQuote({
        project_id: preview.project_id,
        conversation_id: conversationId,
        backend_id: preview.backend_id,
        relative_path: preview.relative_path,
        sha256: preview.sha256,
        text,
      });
      if (!mountedRef.current || generation !== generationRef.current || requestIdentityRef.current !== requestIdentity) return;
      if (!validQuoteItem(item, preview.project_id)) throw new Error("invalid quote");
      onAttachRef.current(item);
    } catch {
      if (mountedRef.current && generation === generationRef.current && requestIdentityRef.current === requestIdentity) {
        preserveSelection(range);
        setQuoteError(quoteErrorMessage(zhRef.current));
      }
    } finally {
      if (mountedRef.current && generation === generationRef.current && requestIdentityRef.current === requestIdentity) {
        quoteBusyRef.current = false;
        setQuoteBusy(false);
      }
    }
  }

  function handleDialogKeyDown(event: ReactKeyboardEvent<HTMLElement>) {
    if (event.key !== "Tab") return;
    const focusables = focusableElements(dialogRef.current);
    if (focusables.length === 0) {
      event.preventDefault();
      return;
    }
    const active = typeof document !== "undefined" && document.activeElement instanceof HTMLElement ? document.activeElement : null;
    const index = active ? focusables.indexOf(active) : -1;
    const direction = event.shiftKey ? -1 : 1;
    const nextIndex = index < 0
      ? (event.shiftKey ? focusables.length - 1 : 0)
      : (index + direction + focusables.length) % focusables.length;
    event.preventDefault();
    focusables[nextIndex].focus();
  }

  const title = zh ? "预览工作区文件" : "Preview workspace file";
  const description = zh ? "查看只读文本，并选择一段附加到当前对话。" : "Inspect read-only text and select a passage to attach to this conversation.";
  const loadingLabel = zh ? "正在加载文件预览" : "Loading file preview";
  const quoteButtonLabel = zh ? "引用所选文本" : "Quote selected text";
  const previewReady = Boolean(preview && loadedIdentity === requestIdentity && !previewLoading && !previewError);
  const selectedBytes = previewReady && preview ? utf8ByteLength(preview.text.slice(selection.start, selection.end)) : 0;

  return (
    <div className="composer-file-preview-backdrop">
      <section
        ref={dialogRef}
        className="composer-file-preview-dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby="composer-file-preview-title"
        aria-describedby="composer-file-preview-description"
        aria-busy={previewLoading || quoteBusy || undefined}
        onKeyDown={handleDialogKeyDown}
      >
        <header className="composer-file-preview-header">
          <div>
            <span className="composer-file-preview-kicker">OmicsOps</span>
            <h2 id="composer-file-preview-title">{title}</h2>
            <p id="composer-file-preview-description">{description}</p>
          </div>
          <button ref={closeRef} type="button" className="composer-file-preview-close" aria-label={zh ? "关闭文件预览" : "Close file preview"} onClick={() => onCloseRef.current()}>
            <X size={18} aria-hidden="true" />
          </button>
        </header>

        <div className="composer-file-preview-source" aria-label={zh ? "文件来源" : "File source"}>
          <span className="composer-file-preview-source-kind"><FileText size={15} aria-hidden="true" /><b>{sourceLabel(reference, zh)}</b></span>
          <code title={reference.relative_path}>{reference.relative_path}</code>
        </div>

        <div className="composer-file-preview-body">
          {previewLoading && <div className="composer-file-preview-state" role="status" aria-label={loadingLabel}><LoaderCircle size={17} className="composer-file-preview-spinner" aria-hidden="true" />{loadingLabel}</div>}
          {previewError && <div className="composer-file-preview-state composer-file-preview-error" role="alert"><CircleAlert size={17} aria-hidden="true" /><span>{previewError}</span><button type="button" aria-label={zh ? "重试预览" : "Retry preview"} onClick={() => setRetryVersion((version) => version + 1)} disabled={previewLoading || quoteBusy}><RefreshCw size={14} aria-hidden="true" />{zh ? "重试" : "Retry"}</button></div>}
          {previewReady && preview && <textarea ref={textareaRef} className="composer-file-preview-text" aria-label={zh ? "文件文本" : "File text"} value={preview.text} readOnly disabled={quoteBusy} onSelect={handleSelect} spellCheck={false} />}
          {quoteError && <div className="composer-file-preview-quote-error" role="alert"><span>{quoteError}</span><button type="button" aria-label={zh ? "重试创建引用" : "Retry quote"} onClick={() => void quoteSelectedText()} disabled={disabled || quoteBusy || !previewReady}>{zh ? "重试" : "Retry"}</button></div>}
        </div>

        <footer className="composer-file-preview-footer">
          <small>{previewReady && preview ? (zh ? `已选择 ${selectedBytes} 字节` : `${selectedBytes} bytes selected`) : ""}</small>
          <button type="button" className="composer-file-preview-primary" aria-label={quoteButtonLabel} onClick={() => void quoteSelectedText()} disabled={disabled || quoteBusy || !previewReady}>
            {quoteBusy ? <LoaderCircle size={15} className="composer-file-preview-spinner" aria-hidden="true" /> : <Quote size={15} aria-hidden="true" />}
            {quoteBusy ? (zh ? "创建中…" : "Creating…") : quoteButtonLabel}
          </button>
        </footer>
      </section>
    </div>
  );
}
