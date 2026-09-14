import type { ComposerAttachmentReceipt } from "../../types";
import type { ComposerAttachmentItem } from "./useComposerAttachments";
import "./composer-attachments.css";

export type { ComposerAttachmentReceipt } from "../../types";
export type { ComposerAttachmentItem, ComposerAttachmentStatus } from "./useComposerAttachments";

export interface ComposerAttachmentsProps {
  items: ComposerAttachmentItem[];
  zh: boolean;
  disabled?: boolean;
  onRemove: (key: string) => void;
  onRetry: (key: string) => void;
}

function formatBytes(size: number, zh: boolean): string {
  if (!Number.isFinite(size) || size < 0) return zh ? "大小未知" : "Size unavailable";
  if (size < 1024) return `${Math.round(size)} B`;
  if (size < 1024 * 1024) return `${(size / 1024).toFixed(size >= 10 * 1024 ? 0 : 1)} KB`;
  if (size < 1024 * 1024 * 1024) return `${(size / (1024 * 1024)).toFixed(size >= 10 * 1024 * 1024 ? 0 : 1)} MB`;
  return `${(size / (1024 * 1024 * 1024)).toFixed(1)} GB`;
}

function statusLabel(status: ComposerAttachmentItem["status"], zh: boolean): string {
  if (status === "uploading") return zh ? "正在上传…" : "Uploading…";
  if (status === "ready") return zh ? "已就绪" : "Ready";
  return zh ? "失败" : "Failed";
}

export function ComposerAttachments({
  items,
  zh,
  disabled = false,
  onRemove,
  onRetry,
}: ComposerAttachmentsProps) {
  if (items.length === 0) return null;

  const busy = disabled || items.some((item) => item.status === "uploading");
  return (
    <div
      className="composer-attachments"
      role="list"
      aria-label={zh ? "附件" : "Attachments"}
      aria-busy={busy || undefined}
    >
      {items.map((item) => {
        const image = item.mediaType.toLowerCase().startsWith("image/");
        return (
          <div className={`composer-attachment composer-attachment-${item.status}`} role="listitem" key={item.key}>
            <div className="composer-attachment-copy">
              <div className="composer-attachment-heading">
                <strong title={item.name}>{item.name}</strong>
                {image && <span className="composer-attachment-image" aria-label={zh ? "图片" : "Image"}>Image</span>}
              </div>
              <span className="composer-attachment-meta">
                <span>{formatBytes(item.sizeBytes, zh)}</span>
                <span aria-hidden="true">·</span>
                <span>{statusLabel(item.status, zh)}</span>
              </span>
              {item.error && <span className="composer-attachment-error" role="alert">{item.error}</span>}
            </div>
            <div className="composer-attachment-actions">
              {item.status === "error" && (
                <button
                  type="button"
                  className="composer-attachment-action"
                  disabled={disabled}
                  aria-label={zh ? `重试 ${item.name}` : `Retry ${item.name}`}
                  onClick={() => onRetry(item.key)}
                >
                  {zh ? "重试" : "Retry"}
                </button>
              )}
              <button
                type="button"
                className="composer-attachment-remove"
                disabled={disabled}
                aria-label={zh ? `移除 ${item.name}` : `Remove ${item.name}`}
                onClick={() => onRemove(item.key)}
              >
                {zh ? "移除" : "Remove"}
              </button>
            </div>
          </div>
        );
      })}
    </div>
  );
}
