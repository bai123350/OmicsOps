import { invoke } from "@tauri-apps/api/core";
export type ConversationExportFormat = "html" | "png";
export async function saveConversationExport(format: ConversationExportFormat, blob: Blob): Promise<string | null> {
  if (blob.size > 24 * 1024 * 1024) throw new Error("Export exceeds the 24 MiB limit.");
  if ("__TAURI_INTERNALS__" in window) {
    const bytes = new Uint8Array(await blob.arrayBuffer());
    let binary = "";
    for (let i = 0; i < bytes.length; i += 8192) binary += String.fromCharCode(...bytes.subarray(i, i + 8192));
    return invoke<string | null>("save_conversation_export", { request: { format, content_base64: btoa(binary) } });
  }
  const url = URL.createObjectURL(blob);
  const anchor = document.createElement("a"); anchor.href = url; anchor.download = `omicsops-conversation.${format}`;
  document.body.appendChild(anchor); anchor.click(); anchor.remove();
  setTimeout(() => URL.revokeObjectURL(url), 1000);
  return "download";
}
