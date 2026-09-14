import { useCallback, useLayoutEffect, useRef, type KeyboardEvent as ReactKeyboardEvent } from "react";
import { CircleAlert, X } from "lucide-react";

import { useWindowEscapeLayer } from "../settings/BrowserSettings";
import { GuidancePanel, type GuidancePanelProps } from "./GuidancePanel";
import "./GuidanceDialog.css";

const FOCUSABLE_SELECTOR = [
  "a[href]",
  "button:not([disabled])",
  "input:not([disabled])",
  "select:not([disabled])",
  "textarea:not([disabled])",
  "[contenteditable=\"true\"]",
  "[tabindex]:not([tabindex=\"-1\"])",
].join(",");

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

export interface GuidanceDialogProps {
  runId: string;
  projectId: string;
  conversationId: string;
  enabled: boolean;
  locale: "zh-CN" | "en-US";
  initialDraft?: string;
  pendingRequest?: GuidancePanelProps["pendingRequest"];
  composerHasAttachments?: boolean;
  composerHasReferences?: boolean;
  onClose: () => void;
  onAccepted?: (markdown: string) => void;
}

export function GuidanceDialog({
  runId,
  projectId,
  conversationId,
  enabled,
  locale,
  initialDraft,
  pendingRequest,
  composerHasAttachments = false,
  composerHasReferences = false,
  onClose,
  onAccepted,
}: GuidanceDialogProps) {
  const zh = locale === "zh-CN";
  const dialogRef = useRef<HTMLElement>(null);
  const closeRef = useRef<HTMLButtonElement>(null);
  const previousFocusRef = useRef<HTMLElement | null>(null);
  const mountedRef = useRef(false);
  const onCloseRef = useRef(onClose);
  const onAcceptedRef = useRef(onAccepted);
  onCloseRef.current = onClose;
  onAcceptedRef.current = onAccepted;

  const carryBlocked = composerHasAttachments || composerHasReferences;
  const carriedDraft = carryBlocked ? undefined : initialDraft;

  useLayoutEffect(() => {
    mountedRef.current = true;
    if (previousFocusRef.current === null && typeof document !== "undefined") {
      previousFocusRef.current = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    }
    closeRef.current?.focus();

    return () => {
      mountedRef.current = false;
      const previous = previousFocusRef.current;
      const active = typeof document === "undefined" ? null : document.activeElement;
      const dialog = dialogRef.current;
      const focusIsInDialog = Boolean(dialog && active && dialog.contains(active));
      const focusIsUnowned = active === null || (typeof document !== "undefined" && active === document.body);
      if (previous?.isConnected && (focusIsInDialog || focusIsUnowned)) previous.focus();
    };
  }, []);

  useWindowEscapeLayer(true, useCallback(() => onCloseRef.current(), []));

  const handleDialogKeyDown = useCallback((event: ReactKeyboardEvent<HTMLElement>) => {
    if (event.key !== "Tab") return;
    const focusables = focusableElements(dialogRef.current);
    if (focusables.length === 0) {
      event.preventDefault();
      return;
    }
    const active = typeof document !== "undefined" && document.activeElement instanceof HTMLElement ? document.activeElement : null;
    const index = active ? focusables.indexOf(active) : -1;
    const direction = event.shiftKey ? -1 : 1;
    const next = index < 0
      ? (event.shiftKey ? focusables.length - 1 : 0)
      : (index + direction + focusables.length) % focusables.length;
    event.preventDefault();
    focusables[next].focus();
  }, []);

  const closeLabel = zh ? "关闭追加指导" : "Close guidance";
  const title = zh ? "追加指导" : "Add guidance";
  const description = zh
    ? "向当前普通 Agent 运行追加一条纯文本指导。指导会在下一模型边界应用，不能扩大执行权限。"
    : "Add plain-text guidance to the current ordinary Agent run. It is applied at the next model boundary and cannot expand execution permissions.";

  return (
    <div className="guidance-dialog-backdrop">
      <section
        ref={dialogRef}
        className="guidance-dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby="guidance-dialog-title"
        aria-describedby="guidance-dialog-description"
        aria-busy={undefined}
        onKeyDown={handleDialogKeyDown}
      >
        <header className="guidance-dialog-header">
          <div>
            <span className="guidance-dialog-kicker">OmicsOps</span>
            <h2 id="guidance-dialog-title">{title}</h2>
            <p id="guidance-dialog-description">{description}</p>
          </div>
          <button ref={closeRef} type="button" className="guidance-dialog-close" aria-label={closeLabel} onClick={() => onCloseRef.current()}>
            <X size={18} aria-hidden="true" />
          </button>
        </header>

        {carryBlocked && <div className="guidance-dialog-note" role="note">
          <CircleAlert size={16} aria-hidden="true" />
          <span>{zh
            ? "当前主消息草稿包含附件或引用，未将其带入指导；附件和引用仍保留在主消息中。"
            : "The composer draft contains attachments or references, so it was not carried into guidance. They remain in the main message."}</span>
        </div>}
        {!enabled && <p className="guidance-dialog-readonly" role="status">
          {zh ? "当前运行暂不接受新的指导，可查看已接收历史。" : "The run is not accepting new guidance right now."}
        </p>}

        <div className="guidance-dialog-body">
          <GuidancePanel
            key={`${projectId}:${conversationId}:${runId}`}
            runId={runId}
            projectId={projectId}
            conversationId={conversationId}
            enabled={enabled}
            locale={locale}
            initialDraft={carriedDraft}
            pendingRequest={pendingRequest}
            onAccepted={(markdown) => {
              if (mountedRef.current) onAcceptedRef.current?.(markdown);
            }}
          />
        </div>
      </section>
    </div>
  );
}
