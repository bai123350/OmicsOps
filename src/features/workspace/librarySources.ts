import { workspaceSource, type WorkspaceSourceRef } from "../../workspace-navigation-types";

/** Offsets received from a JS selection are converted to the host's UTF-8 offsets. */
export async function messageLibrarySource(projectId: string, conversationId: string, messageId: string, markdown: string, start = 0, end = markdown.length): Promise<WorkspaceSourceRef> {
  if (start < 0 || end <= start || end > markdown.length) throw new Error("Invalid source selection");
  const encoder = new TextEncoder();
  const digest = await crypto.subtle.digest("SHA-256", encoder.encode(markdown));
  return workspaceSource(projectId, "message", messageId, { conversation_id: conversationId, content_sha256: Array.from(new Uint8Array(digest), (byte) => byte.toString(16).padStart(2, "0")).join(""), start: encoder.encode(markdown.slice(0, start)).length, end: encoder.encode(markdown.slice(0, end)).length });
}
