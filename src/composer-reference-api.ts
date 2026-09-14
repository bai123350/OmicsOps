import { invoke } from "@tauri-apps/api/core";
import type { ComposerCatalogItem, ComposerReference } from "./types";

export async function composerReferenceCatalog(projectId: string): Promise<ComposerCatalogItem[]> {
  if (!("__TAURI_INTERNALS__" in window)) return [];
  return invoke("composer_reference_catalog", { projectId });
}

export async function validateComposerReferences(
  projectId: string,
  conversationId: string,
  references: ComposerReference[],
): Promise<void> {
  if (!("__TAURI_INTERNALS__" in window)) return;
  await invoke("validate_composer_references", { projectId, conversationId, references });
}
