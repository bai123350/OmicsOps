import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import {
  COMPOSER_ATTACHMENT_BROWSER_ERROR,
  MAX_COMPOSER_ATTACHMENT_BYTES,
  MAX_COMPOSER_ATTACHMENT_TOTAL_BYTES,
  MAX_COMPOSER_ATTACHMENTS,
  chooseComposerAttachments,
  stageComposerAttachment,
} from "../../composer-attachment-api";
import type { ComposerAttachmentReceipt } from "../../types";

export type ComposerAttachmentStatus = "uploading" | "ready" | "error";

export interface ComposerAttachmentItem {
  key: string;
  /** Alias retained for callers that use the staging vocabulary. */
  clientKey: string;
  name: string;
  sizeBytes: number;
  mediaType: string;
  status: ComposerAttachmentStatus;
  receipt?: ComposerAttachmentReceipt;
  error?: string;
}

type AttachmentSource =
  | { kind: "file"; file: File }
  | { kind: "native" };

interface AttachmentRecord {
  item: ComposerAttachmentItem;
  source: AttachmentSource;
  generation: number;
}

type Operation = { generation: number } | null;

let nextClientKey = 0;

function makeClientKey(prefix = "file"): string {
  nextClientKey += 1;
  return `composer-attachment-${prefix}-${nextClientKey}`;
}

function fileIdentity(file: File): string {
  return `${file.name}\u0000${file.size}\u0000${file.lastModified}\u0000${file.type}`;
}

function genericUploadError(reason: unknown): string {
  if (reason instanceof Error && reason.message === COMPOSER_ATTACHMENT_BROWSER_ERROR) return reason.message;
  return "Could not upload attachment. Try again.";
}

function genericChooseError(): string {
  return "Could not choose attachments. Try again.";
}

function limitError(file: File, currentBytes: number, acceptedBytes: number): string | null {
  if (file.size > MAX_COMPOSER_ATTACHMENT_BYTES) return "Attachment exceeds the 20 MiB per-file limit.";
  if (currentBytes + acceptedBytes + file.size > MAX_COMPOSER_ATTACHMENT_TOTAL_BYTES) {
    return "Attachments exceed the 40 MiB per-message limit.";
  }
  return null;
}

function fileItem(key: string, file: File, generation: number): AttachmentRecord {
  return {
    item: {
      key,
      clientKey: key,
      name: file.name,
      sizeBytes: file.size,
      mediaType: file.type || "application/octet-stream",
      status: "uploading",
    },
    source: { kind: "file", file },
    generation,
  };
}

function nativeItem(receipt: ComposerAttachmentReceipt, generation: number): AttachmentRecord {
  const key = makeClientKey(`native-${receipt.id}`);
  return {
    item: {
      key,
      clientKey: key,
      name: receipt.name,
      sizeBytes: receipt.size_bytes,
      mediaType: receipt.media_type || "application/octet-stream",
      status: "ready",
      receipt,
    },
    source: { kind: "native" },
    generation,
  };
}

function nativeErrorItem(generation: number): AttachmentRecord {
  const key = makeClientKey("native-error");
  return {
    item: {
      key,
      clientKey: key,
      name: "File chooser",
      sizeBytes: 0,
      mediaType: "application/octet-stream",
      status: "error",
      error: genericChooseError(),
    },
    source: { kind: "native" },
    generation,
  };
}

export interface UseComposerAttachmentsResult {
  items: ComposerAttachmentItem[];
  receipts: ComposerAttachmentReceipt[];
  busy: boolean;
  limitReached: boolean;
  pickerBusyNotice: boolean;
  addFiles: (files: File[]) => Promise<void>;
  chooseFiles: () => Promise<void>;
  retry: (key: string) => Promise<void>;
  remove: (key: string) => void;
  clearAccepted: (ids: string[]) => void;
  restoreReceipts: (receipts: ComposerAttachmentReceipt[]) => boolean;
}

export function useComposerAttachments(
  projectId: string,
  conversationId?: string | null,
): UseComposerAttachmentsResult {
  const recordsRef = useRef<Map<string, AttachmentRecord>>(new Map());
  const generationRef = useRef(0);
  const mountedRef = useRef(true);
  const operationRef = useRef<Operation>(null);
  const [items, setItems] = useState<ComposerAttachmentItem[]>([]);
  const [busy, setBusy] = useState(false);
  const [limitReached, setLimitReached] = useState(false);
  const [pickerBusyNotice, setPickerBusyNotice] = useState(false);

  const publish = useCallback(() => {
    setItems(Array.from(recordsRef.current.values(), (record) => record.item));
    setBusy(Boolean(operationRef.current) || Array.from(recordsRef.current.values()).some((record) => record.item.status === "uploading"));
  }, []);

  const isGenerationActive = useCallback((generation: number) => (
    mountedRef.current && generationRef.current === generation
  ), []);

  const isCurrentRecord = useCallback((key: string, generation: number) => (
    isGenerationActive(generation) && recordsRef.current.get(key)?.generation === generation
  ), [isGenerationActive]);

  const beginOperation = useCallback((generation: number): boolean => {
    if (!isGenerationActive(generation) || operationRef.current) return false;
    operationRef.current = { generation };
    setBusy(true);
    return true;
  }, [isGenerationActive]);

  const endOperation = useCallback((generation: number) => {
    if (operationRef.current?.generation !== generation) return;
    operationRef.current = null;
    if (isGenerationActive(generation)) publish();
  }, [isGenerationActive, publish]);

  const stageRecord = useCallback(async (record: AttachmentRecord, generation: number) => {
    if (record.source.kind !== "file" || !isCurrentRecord(record.item.key, generation)) return;
    const { key } = record.item;
    try {
      const receipt = await stageComposerAttachment(projectId, conversationId!, record.source.file);
      if (!isCurrentRecord(key, generation)) return;
      if (receipt.project_id !== projectId || receipt.conversation_id !== conversationId) throw new Error("Attachment scope mismatch");
      const current = recordsRef.current.get(key);
      if (!current) return;
      recordsRef.current.set(key, {
        ...current,
        item: {
          ...current.item,
          name: receipt.name || current.item.name,
          sizeBytes: receipt.size_bytes,
          mediaType: receipt.media_type || current.item.mediaType,
          status: "ready",
          receipt,
          error: undefined,
        },
      });
      publish();
    } catch (reason: unknown) {
      if (!isCurrentRecord(key, generation)) return;
      const current = recordsRef.current.get(key);
      if (!current) return;
      recordsRef.current.set(key, {
        ...current,
        item: { ...current.item, status: "error", error: genericUploadError(reason), receipt: undefined },
      });
      publish();
    }
  }, [conversationId, isCurrentRecord, projectId, publish]);

  const addFiles = useCallback(async (files: File[]) => {
    const generation = generationRef.current;
    if (!conversationId || !isGenerationActive(generation)) return;
    const candidates = Array.from(files || []).filter((file): file is File => file instanceof File);
    if (!candidates.length) return;
    if (operationRef.current) {
      setPickerBusyNotice(true);
      return;
    }

    const existingIdentities = new Set<string>();
    let currentBytes = 0;
    for (const record of recordsRef.current.values()) {
      if (record.source.kind === "file") existingIdentities.add(fileIdentity(record.source.file));
      currentBytes += record.item.sizeBytes;
    }

    const seen = new Set(existingIdentities);
    const accepted: AttachmentRecord[] = [];
    const rejected: AttachmentRecord[] = [];
    const planned: AttachmentRecord[] = [];
    let plannedBytes = 0;
    for (const file of candidates) {
      const identity = fileIdentity(file);
      if (seen.has(identity)) continue;
      seen.add(identity);
      if (recordsRef.current.size + accepted.length + rejected.length >= MAX_COMPOSER_ATTACHMENTS) { setLimitReached(true); break; }
      const error = limitError(file, currentBytes, plannedBytes);
      if (error) {
        const key = makeClientKey("rejected");
        const record = fileItem(key, file, generation);
        rejected.push({ ...record, item: { ...record.item, status: "error", error } });
        planned.push(rejected[rejected.length - 1]);
        continue;
      }
      const record = fileItem(makeClientKey(), file, generation);
      accepted.push(record);
      planned.push(record);
      plannedBytes += file.size;
    }

    if (!accepted.length && !rejected.length) return;
    setPickerBusyNotice(false);
    for (const record of planned) recordsRef.current.set(record.item.key, record);
    if (isGenerationActive(generation)) publish();
    if (!accepted.length) return;
    // Each batch preserves selection order; later drops reserve their own slots immediately.
    for (const record of accepted) {
      if (!isGenerationActive(generation)) return;
      await stageRecord(record, generation);
    }
  }, [conversationId, isGenerationActive, publish, stageRecord]);

  const chooseFiles = useCallback(async () => {
    const generation = generationRef.current;
    if (!conversationId || !isGenerationActive(generation)) return;
    if (operationRef.current || Array.from(recordsRef.current.values()).some((record) => record.item.status === "uploading")) {
      setPickerBusyNotice(true);
      return;
    }
    const currentBytes = Array.from(recordsRef.current.values()).reduce((sum, record) => sum + record.item.sizeBytes, 0);
    const maxFiles = MAX_COMPOSER_ATTACHMENTS - recordsRef.current.size;
    const maxBytes = MAX_COMPOSER_ATTACHMENT_TOTAL_BYTES - currentBytes;
    if (maxFiles <= 0 || maxBytes <= 0) {
      setLimitReached(true);
      return;
    }
    if (!beginOperation(generation)) {
      if (isGenerationActive(generation)) setPickerBusyNotice(true);
      return;
    }
    setPickerBusyNotice(false);
    try {
      const chosen = await chooseComposerAttachments(projectId, conversationId, { maxFiles, maxBytes });
      if (!isGenerationActive(generation)) return;
      const existingIds = new Set(
        Array.from(recordsRef.current.values(), (record) => record.item.receipt?.id).filter(Boolean),
      );
      let totalBytes = Array.from(recordsRef.current.values()).reduce((sum, record) => sum + record.item.sizeBytes, 0);
      for (const receipt of chosen) {
        if (recordsRef.current.size >= MAX_COMPOSER_ATTACHMENTS) { setLimitReached(true); break; }
        if (existingIds.has(receipt.id)) continue;
        if (receipt.project_id !== projectId || receipt.conversation_id !== conversationId) continue;
        if (receipt.size_bytes > MAX_COMPOSER_ATTACHMENT_BYTES) { setLimitReached(true); continue; }
        if (totalBytes + receipt.size_bytes > MAX_COMPOSER_ATTACHMENT_TOTAL_BYTES) { setLimitReached(true); break; }
        const record = nativeItem(receipt, generation);
        recordsRef.current.set(record.item.key, record);
        existingIds.add(receipt.id);
        totalBytes += receipt.size_bytes;
      }
      publish();
    } catch {
      if (!isGenerationActive(generation)) return;
      if (recordsRef.current.size < MAX_COMPOSER_ATTACHMENTS) {
        const record = nativeErrorItem(generation);
        recordsRef.current.set(record.item.key, record);
        publish();
      }
    } finally {
      endOperation(generation);
    }
  }, [beginOperation, conversationId, endOperation, isGenerationActive, projectId, publish]);

  const retry = useCallback(async (key: string) => {
    const generation = generationRef.current;
    const record = recordsRef.current.get(key);
    if (!record || !isGenerationActive(generation) || operationRef.current) return;
    if (record.source.kind === "native") {
      recordsRef.current.delete(key);
      publish();
      await chooseFiles();
      return;
    }
    if (record.item.status !== "error") return;
    const otherBytes = Array.from(recordsRef.current.values()).reduce(
      (sum, other) => sum + (other.item.key === key ? 0 : other.item.sizeBytes), 0,
    );
    const error = limitError(record.source.file, otherBytes, 0);
    if (error) {
      recordsRef.current.set(key, { ...record, item: { ...record.item, error } });
      publish();
      return;
    }
    setPickerBusyNotice(false);
    recordsRef.current.set(key, {
      ...record,
      item: { ...record.item, status: "uploading", error: undefined, receipt: undefined },
    });
    publish();
    await stageRecord(record, generation);
  }, [chooseFiles, isGenerationActive, publish, stageRecord]);

  const restoreReceipts = useCallback((receipts: ComposerAttachmentReceipt[]) => {
    if (recordsRef.current.size || operationRef.current || receipts.length > MAX_COMPOSER_ATTACHMENTS) return false;
    if (new Set(receipts.map((receipt) => receipt.id)).size !== receipts.length || receipts.some((receipt) => receipt.project_id !== projectId || receipt.conversation_id !== conversationId || receipt.size_bytes < 0 || receipt.size_bytes > MAX_COMPOSER_ATTACHMENT_BYTES) || receipts.reduce((sum, receipt) => sum + receipt.size_bytes, 0) > MAX_COMPOSER_ATTACHMENT_TOTAL_BYTES) return false;
    for (const receipt of receipts) { const record = nativeItem(receipt, generationRef.current); recordsRef.current.set(record.item.key, record); }
    publish(); return true;
  }, [projectId, conversationId, publish]);

  const remove = useCallback((key: string) => {
    recordsRef.current.delete(key);
    setLimitReached(false);
    setPickerBusyNotice(false);
    publish();
  }, [publish]);

  const clearAccepted = useCallback((ids: string[]) => {
    const acceptedIds = new Set(ids);
    for (const [key, record] of recordsRef.current) {
      if (record.item.status === "ready" && record.item.receipt && acceptedIds.has(record.item.receipt.id)) {
        recordsRef.current.delete(key);
      }
    }
    setLimitReached(false);
    setPickerBusyNotice(false);
    publish();
  }, [publish]);

  useEffect(() => {
    generationRef.current += 1;
    recordsRef.current.clear();
    operationRef.current = null;
    setItems([]);
    setBusy(false);
    setLimitReached(false);
    setPickerBusyNotice(false);
  }, [conversationId, projectId]);

  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
      generationRef.current += 1;
      operationRef.current = null;
    };
  }, []);

  const receipts = useMemo(
    () => items.flatMap((item) => item.status === "ready" && item.receipt ? [item.receipt] : []),
    [items],
  );

  return { items, receipts, busy, limitReached, pickerBusyNotice, addFiles, chooseFiles, retry, remove, clearAccepted, restoreReceipts };
}
