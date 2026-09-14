import { invoke } from "@tauri-apps/api/core";
import type { ComposerCatalogItem, ComposerReference, RemoteFileEntry, ComposerTextPreview, CreateComposerQuoteRequest } from "./types";

export const WORKSPACE_FILE_DRAG_TYPE = "application/x-omicsops-workspace-file";
export type WorkspaceFileReference = Extract<ComposerReference, { kind: "workspace_file" }>;

export function parseWorkspaceFileDrag(value: string): WorkspaceFileReference | null {
  if (value.length > 8_192) return null;
  try {
    const item = JSON.parse(value);
    if (!item || item.kind !== "workspace_file" || typeof item.project_id !== "string" || typeof item.backend_id !== "string" || typeof item.relative_path !== "string" || !item.relative_path) return null;
    return { kind: "workspace_file", project_id: item.project_id, backend_id: item.backend_id, relative_path: item.relative_path };
  } catch { return null; }
}

export function parseClipboardFilePaths(value: string): string[] | null {
  if (value.length > 32_768 || /[\x00-\x08\x0b\x0c\x0e-\x1f]/.test(value)) return null;
  const lines = value.trim().split(/\r?\n/).filter(Boolean);
  if (!lines.length || lines.length > 12) return null;
  const paths: string[] = [];
  for (const line of lines) {
    const trimmed = line.trim();
    const quoted = trimmed.startsWith('"') && trimmed.endsWith('"');
    const path = quoted ? trimmed.slice(1, -1) : trimmed;
    const absolute = /^[a-z]:[\\/]/i.test(path) || path.startsWith("\\\\") || /^\/(?:[^/]+\/|[^/]+\.[^/]+)/.test(path);
    const relative = /^(?:\.\.?[\\/]|[^\s:]+[\\/][^\s:]+\.[^\s:]+$)/.test(path);
    if (!path || (!absolute && !relative) || /^(?:https?|file):/i.test(path) || (!absolute && !quoted && /\s/.test(path))) return null;
    paths.push(path);
  }
  return [...new Set(paths)];
}

export async function listLocalComposerFiles(projectId: string): Promise<RemoteFileEntry[]> {
  if (!("__TAURI_INTERNALS__" in window)) return [];
  return invoke("list_local_composer_files", { projectId });
}

export async function resolveComposerClipboardPaths(projectId: string, paths: string[]): Promise<ComposerCatalogItem[]> {
  if (!("__TAURI_INTERNALS__" in window)) throw new Error("File references require the desktop app.");
  return invoke("resolve_composer_clipboard_paths", { projectId, paths });
}

export async function previewComposerFileText(reference: WorkspaceFileReference): Promise<ComposerTextPreview> {
  if (!("__TAURI_INTERNALS__" in window)) throw new Error("File previews require the desktop app.");
  return invoke("preview_composer_file_text", { projectId: reference.project_id, backendId: reference.backend_id, relativePath: reference.relative_path });
}

export async function createComposerQuote(request: CreateComposerQuoteRequest): Promise<ComposerCatalogItem> {
  if (!("__TAURI_INTERNALS__" in window)) throw new Error("File quotes require the desktop app.");
  return invoke("create_composer_quote", { request });
}
