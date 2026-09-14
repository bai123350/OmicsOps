import { invoke } from "@tauri-apps/api/core";

import type { ComposerAttachmentReceipt } from "./types";

export const MAX_COMPOSER_ATTACHMENT_BYTES = 20 * 1024 * 1024;
export const MAX_COMPOSER_ATTACHMENTS = 8;
export const MAX_COMPOSER_ATTACHMENT_TOTAL_BYTES = 40 * 1024 * 1024;

export const COMPOSER_ATTACHMENT_BROWSER_ERROR = "File attachments are unavailable in browser preview.";

const FILE_SIZE_ERROR = "Attachment exceeds the 20 MiB per-file limit.";
const FILE_READ_ERROR = "Could not read attachment bytes.";

function isNativeRuntime(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

function readFileAsBase64(file: File): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    let settled = false;
    const fail = () => {
      if (settled) return;
      settled = true;
      reject(new Error(FILE_READ_ERROR));
    };
    reader.onload = () => {
      if (settled) return;
      const result = reader.result;
      if (typeof result !== "string") {
        fail();
        return;
      }
      const separator = result.indexOf(",");
      settled = true;
      resolve(separator >= 0 ? result.slice(separator + 1) : result);
    };
    reader.onerror = fail;
    reader.onabort = fail;
    try {
      reader.readAsDataURL(file);
    } catch {
      fail();
    }
  });
}

export async function chooseComposerAttachments(
  projectId: string,
  conversationId: string,
  limits?: { maxFiles: number; maxBytes: number },
): Promise<ComposerAttachmentReceipt[]> {
  if (!isNativeRuntime()) return [];
  return invoke<ComposerAttachmentReceipt[]>("choose_composer_attachments", { projectId, conversationId, ...limits });
}

export async function stageComposerAttachment(
  projectId: string,
  conversationId: string,
  file: File,
): Promise<ComposerAttachmentReceipt> {
  if (!isNativeRuntime()) throw new Error(COMPOSER_ATTACHMENT_BROWSER_ERROR);
  if (file.size > MAX_COMPOSER_ATTACHMENT_BYTES) throw new Error(FILE_SIZE_ERROR);

  const contentBase64 = await readFileAsBase64(file);
  return invoke<ComposerAttachmentReceipt>("stage_composer_attachment", {
    request: {
      project_id: projectId,
      conversation_id: conversationId,
      name: file.name,
      content_base64: contentBase64,
    },
  });
}

export async function validateComposerAttachments(
  projectId: string,
  conversationId: string,
  attachments: string[],
  modelProfileId?: string,
): Promise<void> {
  if (!isNativeRuntime()) return;
  await invoke("validate_composer_attachments", { projectId, conversationId, attachments, ...(modelProfileId ? { modelProfileId } : {}) });
}
