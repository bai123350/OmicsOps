import { useRef, useState } from "react";
import { Star } from "lucide-react";
import { saveWorkspaceLibraryItem } from "../../workspace-navigation-api";
import type { LibraryKind, SaveWorkspaceLibraryItemRequest, WorkspaceSourceRef } from "../../workspace-navigation-types";

export function CollectSourceButton({ source, title, kind, zh }: { source: () => Promise<WorkspaceSourceRef>; title: string; kind: LibraryKind; zh: boolean }) {
  const pending = useRef<SaveWorkspaceLibraryItemRequest | null>(null);
  const lock = useRef(false);
  const [status, setStatus] = useState<"idle" | "busy" | "saved" | "failed">("idle");
  async function save() {
    if (lock.current || status === "saved") return;
    lock.current = true; setStatus("busy");
    try {
      pending.current ??= { request_id: crypto.randomUUID(), title: title.slice(0, 160), kind, source: await source() };
      await saveWorkspaceLibraryItem(pending.current);
      setStatus("saved");
    } catch { setStatus("failed"); }
    finally { lock.current = false; }
  }
  return <span className="collect-source-action"><button type="button" aria-label={zh ? `收藏到资料库：${title}` : `Save to library: ${title}`} disabled={status === "busy" || status === "saved"} onClick={() => void save()}><Star size={13} />{status === "saved" ? (zh ? "已收藏" : "Saved") : (zh ? "收藏" : "Save")}</button>{status === "failed" && <small role="alert">{zh ? "收藏失败，可重试。" : "Save failed. Retry."}</small>}</span>;
}
