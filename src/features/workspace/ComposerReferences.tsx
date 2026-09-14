import { useEffect, useMemo, useRef, useState, type RefObject } from "react";

import type { ComposerCatalogItem, ComposerReference } from "../../types";
import { useWindowEscapeLayer } from "../settings/BrowserSettings";
import "./composer-references.css";

export type { ComposerCatalogItem, ComposerReference } from "../../types";

export type ComposerReferenceKind = ComposerReference["kind"];

export interface ComposerReferenceTrigger {
  start: number;
  end: number;
  kind: ComposerReferenceKind;
  query: string;
}

export type ComposerPickerCommand = {
  id: string;
  label: string;
  description: string;
  onSelect: () => boolean | void;
};

type ComposerPickerEntry =
  | { type: "command"; command: ComposerPickerCommand }
  | { type: "reference"; item: ComposerCatalogItem };

const EMPTY_COMMANDS: ComposerPickerCommand[] = [];

const triggerKinds: Record<string, ComposerReferenceKind> = {
  "@": "artifact",
  "#": "session",
  "/": "skill",
};

function isWhitespace(value: string | undefined): boolean {
  return value !== undefined && /\s/u.test(value);
}

function isValidTriggerQuery(marker: string, query: string): boolean {
  // A second trigger marker, slash, or backslash is an address/path rather
  // than a single composer reference token. Slash commands use identifier
  // characters so paths such as /data/result cannot open the picker.
  if (/[\\/@#]/u.test(query)) return false;
  if (marker === "/" && !/^[\p{L}\p{N}_-]*$/u.test(query)) return false;
  return true;
}

function referenceBelongsToTrigger(reference: ComposerReference, kind: ComposerReferenceKind): boolean {
  if (kind === "artifact") return reference.kind === "artifact" || reference.kind === "execution_context" || reference.kind === "runtime";
  if (kind === "session") return reference.kind === "session" || reference.kind === "project";
  return reference.kind === "skill" || reference.kind === "workflow";
}

/**
 * Finds the reference trigger directly before a textarea caret.
 *
 * The returned end is the caret position, which lets a caller replace only
 * the text already typed before the caret and preserve any text after it.
 */
export function parseComposerTrigger(text: string, caret: number): ComposerReferenceTrigger | null {
  if (typeof text !== "string" || !Number.isFinite(caret)) return null;

  const end = Math.min(text.length, Math.max(0, Math.trunc(caret)));
  if (end === 0) return null;

  let start = end;
  while (start > 0 && !isWhitespace(text[start - 1])) start -= 1;

  const token = text.slice(start, end);
  if (token.length === 0) return null;

  const marker = token[0];
  const kind = triggerKinds[marker];
  if (!kind || (start > 0 && !isWhitespace(text[start - 1]))) return null;

  const query = token.slice(1);
  if (!isValidTriggerQuery(marker, query)) return null;

  return { start, end, kind, query };
}

/** Produces a stable identity for a reference, including project scope. */
export function referenceKey(reference: ComposerReference): string {
  switch (reference.kind) {
    case "skill":
      return `skill:${reference.id}`;
    case "execution_context":
      return `execution_context:${reference.project_id}:${reference.backend_id}`;
    case "workspace_file":
      return `workspace_file:${reference.project_id}:${reference.backend_id}:${reference.relative_path}`;
    case "runtime":
      return `runtime:${reference.project_id}:${reference.backend_id}:${reference.language}`;
    default:
      return `${reference.kind}:${reference.project_id}:${reference.id}`;
  }
}

export interface ComposerReferencePickerProps {
  items: ComposerCatalogItem[];
  trigger: ComposerReferenceTrigger | null;
  onSelect: (item: ComposerCatalogItem) => boolean | void;
  onClose: () => void;
  zh: boolean;
  loading?: boolean;
  error?: string | null;
  inputRef?: RefObject<HTMLTextAreaElement | null>;
  commands?: ComposerPickerCommand[];
}

function isImeEvent(event: KeyboardEvent): boolean {
  return event.isComposing || event.keyCode === 229;
}

function isPickerKeyboardTarget(
  target: EventTarget | null,
  pickerRef: RefObject<HTMLDivElement | null>,
  inputRef?: RefObject<HTMLTextAreaElement | null>,
): boolean {
  if (!(target instanceof HTMLElement)) return false;
  if (inputRef ? target === inputRef.current : target instanceof HTMLTextAreaElement) return true;
  const listbox = target.closest('[role="listbox"]');
  return Boolean(listbox && pickerRef.current?.contains(listbox));
}

function itemSearchText(item: ComposerCatalogItem): string {
  return `${item.label}\n${item.description}`.toLowerCase();
}

function commandSearchText(command: ComposerPickerCommand): string {
  return `${command.label}\n${command.description}`.toLowerCase();
}

function kindLabel(kind: ComposerReferenceKind, zh: boolean): string {
  if (kind === "quote") return zh ? "引用文本" : "Quote";
  if (kind === "workspace_file") return zh ? "文件引用" : "File reference";
  if (kind === "artifact") return zh ? "产物" : "Artifact";
  if (kind === "session") return zh ? "会话" : "Session";
  if (kind === "project") return zh ? "项目" : "Project";
  if (kind === "execution_context") return zh ? "执行环境" : "Execution context";
  if (kind === "runtime") return zh ? "运行时" : "Runtime";
  if (kind === "workflow") return zh ? "工作流" : "Workflow";
  return zh ? "技能" : "Skill";
}

function triggerMarker(kind: ComposerReferenceKind): string {
  if (kind === "quote") return "❝";
  if (kind === "workspace_file") return "";
  if (kind === "artifact") return "@";
  if (kind === "execution_context" || kind === "runtime") return "@";
  if (kind === "session" || kind === "project") return "#";
  return "/";
}

function referenceGroup(kind: ComposerReferenceKind, zh: boolean): string {
  if (kind === "artifact") return zh ? "产物" : "Artifacts";
  if (kind === "execution_context") return zh ? "执行环境" : "Execution contexts";
  if (kind === "runtime") return zh ? "运行时" : "Runtimes";
  if (kind === "project") return zh ? "项目" : "Projects";
  if (kind === "session") return zh ? "会话" : "Sessions";
  if (kind === "workflow") return zh ? "工作流" : "Workflows";
  return zh ? "技能" : "Skills";
}

const REFERENCE_GROUP_ORDER: ComposerReferenceKind[] = [
  "artifact",
  "execution_context",
  "runtime",
  "project",
  "session",
  "workflow",
  "skill",
];

export function ComposerReferencePicker({
  items,
  trigger,
  onSelect,
  onClose,
  zh,
  loading = false,
  error = null,
  inputRef,
  commands = EMPTY_COMMANDS,
}: ComposerReferencePickerProps) {
  const [activeIndex, setActiveIndex] = useState(0);
  const pickerRef = useRef<HTMLDivElement>(null);
  const activeIndexRef = useRef(activeIndex);
  const entriesRef = useRef<ComposerPickerEntry[]>([]);
  const onSelectRef = useRef(onSelect);
  const onCloseRef = useRef(onClose);
  onSelectRef.current = onSelect;
  onCloseRef.current = onClose;
  activeIndexRef.current = activeIndex;

  const kind = trigger?.kind;
  const query = trigger?.query ?? "";
  const normalizedQuery = query.toLowerCase();
  const matchingCommands = useMemo(() => {
    if (kind !== "skill") return [];
    return commands.filter((command) => commandSearchText(command).includes(normalizedQuery));
  }, [commands, kind, normalizedQuery]);
  const matchingReferences = useMemo(() => {
    if (!kind || loading || error) return [];
    return items.filter((item) => referenceBelongsToTrigger(item.reference, kind) && itemSearchText(item).includes(normalizedQuery));
  }, [error, items, kind, loading, normalizedQuery]);
  const matchingEntries = useMemo<ComposerPickerEntry[]>(() => {
    if (!kind) return [];
    const references = loading || error ? [] : matchingReferences;
    return [
      ...(kind === "skill" ? matchingCommands.map((command) => ({ type: "command" as const, command })) : []),
      ...references.map((item) => ({ type: "reference" as const, item })),
    ];
  }, [error, items, kind, loading, matchingCommands, matchingReferences, normalizedQuery]);
  entriesRef.current = matchingEntries;

  const matchingSignature = useMemo(
    () => matchingEntries.map((entry) => entry.type === "command"
      ? `command:${entry.command.id}\u0000${entry.command.label}\u0000${entry.command.description}`
      : `${referenceKey(entry.item.reference)}\u0000${entry.item.label}\u0000${entry.item.description}`).join("\u0001"),
    [matchingEntries],
  );

  const selectEntry = (entry: ComposerPickerEntry) => {
    if (entry.type === "command") {
      if (entry.command.onSelect() !== false) onCloseRef.current();
      return;
    }
    if (onSelectRef.current(entry.item) !== false) onCloseRef.current();
  };

  // Reset to the first result whenever the active token or its result set
  // changes. This avoids pressing Enter against an index from the old query.
  useEffect(() => {
    activeIndexRef.current = 0;
    setActiveIndex(0);
  }, [kind, query, matchingSignature]);

  useEffect(() => {
    if (matchingEntries.length === 0) return;
    const activeOption = pickerRef.current?.querySelector<HTMLElement>('[aria-selected="true"]');
    activeOption?.scrollIntoView?.({ block: "nearest" });
  }, [matchingEntries.length, matchingSignature, activeIndex]);

  useWindowEscapeLayer(Boolean(trigger), onClose);

  useEffect(() => {
    if (!trigger) return undefined;

    const onWindowKeyDown = (event: KeyboardEvent) => {
      const targetIsPicker = isPickerKeyboardTarget(event.target, pickerRef, inputRef);
      if (isImeEvent(event)) {
        // The Escape layer listens during the bubble phase. Keep a composing
        // Escape from closing the picker while allowing normal Escape to use
        // the shared top-level stack.
        if (event.key === "Escape" && targetIsPicker) {
          event.preventDefault();
          event.stopImmediatePropagation();
        }
        return;
      }

      if (event.key === "Escape") return;
      if (!targetIsPicker) return;

      if (event.key === "ArrowDown" || event.key === "ArrowUp") {
        const count = entriesRef.current.length;
        if (count === 0) return;
        event.preventDefault();
        setActiveIndex((current) => {
          const safeCurrent = current >= 0 && current < count ? current : 0;
          const next = event.key === "ArrowDown"
            ? (safeCurrent + 1) % count
            : (safeCurrent + count - 1) % count;
          activeIndexRef.current = next;
          return next;
        });
        return;
      }

      if (event.key === "Enter" && !event.shiftKey) {
        const entry = entriesRef.current[activeIndexRef.current];
        if (entry) {
          event.preventDefault();
          selectEntry(entry);
        }
      }
    };

    // Capture is required because the composer textarea's own Enter handler
    // sends the draft during the bubble phase.
    window.addEventListener("keydown", onWindowKeyDown, true);
    return () => window.removeEventListener("keydown", onWindowKeyDown, true);
  }, [inputRef, trigger]);

  if (!trigger) return null;

  const listboxLabel = zh ? "引用选择器" : "Reference picker";
  const activeOptionId = matchingEntries.length > 0
    ? `composer-reference-option-${activeIndex < matchingEntries.length ? activeIndex : 0}`
    : undefined;

  const renderOption = (entry: ComposerPickerEntry, index: number) => {
    const command = entry.type === "command" ? entry.command : null;
    const item = entry.type === "reference" ? entry.item : null;
    const label = command?.label ?? item?.label ?? "";
    const description = command?.description ?? item?.description ?? "";
    return (
      <button
        type="button"
        role="option"
        id={`composer-reference-option-${index}`}
        key={entry.type === "command" ? `command:${command?.id}-${index}` : `${referenceKey(item!.reference)}-${index}`}
        aria-selected={index === activeIndex}
        className={`composer-reference-option${index === activeIndex ? " is-active" : ""}`}
        onMouseDown={(event) => event.preventDefault()}
        onClick={() => selectEntry(entry)}
      >
        <span className="composer-reference-option-kind">{command ? "/ Command" : `${triggerMarker(item!.reference.kind)} ${kindLabel(item!.reference.kind, zh)}`}</span>
        <span className="composer-reference-option-copy">
          <strong>{label}</strong>
          {description && <small>{description}</small>}
        </span>
      </button>
    );
  };

  const commandEntries = matchingEntries.filter((entry): entry is Extract<ComposerPickerEntry, { type: "command" }> => entry.type === "command");
  const catalogEntries = matchingEntries.filter((entry): entry is Extract<ComposerPickerEntry, { type: "reference" }> => entry.type === "reference");
  const groupedReferenceEntries = REFERENCE_GROUP_ORDER
    .filter((groupKind) => groupKind !== "skill" || kind === "skill")
    .map((groupKind) => ({
      kind: groupKind,
      entries: catalogEntries.filter((entry) => entry.item.reference.kind === groupKind),
    }))
    .filter((group) => group.entries.length > 0);

  return (
    <div className="composer-reference-picker" aria-live="polite" ref={pickerRef}>
      <div
        className="composer-reference-listbox"
        role="listbox"
        aria-label={listboxLabel}
        aria-busy={loading || undefined}
        aria-activedescendant={activeOptionId}
      >
        {loading && <div className="composer-reference-state" role="status">{zh ? "正在加载引用…" : "Loading references…"}</div>}
        {!loading && error && <div className="composer-reference-state composer-reference-error" role="alert">{error}</div>}
        {commandEntries.length > 0 && <section className="composer-reference-group" role="group" aria-label={zh ? "命令" : "Commands"}>
          <h4>{zh ? "命令" : "Commands"}</h4>
          {commandEntries.map((entry) => renderOption(entry, matchingEntries.indexOf(entry)))}
        </section>}
        {groupedReferenceEntries.map((group) => (
          <section className="composer-reference-group" role="group" aria-label={referenceGroup(group.kind, zh)} key={group.kind}>
            <h4>{referenceGroup(group.kind, zh)}</h4>
            {group.entries.map((entry) => renderOption(entry, matchingEntries.indexOf(entry)))}
          </section>
        ))}
        {!loading && !error && matchingEntries.length === 0 && <div className="composer-reference-state" role="status">{zh ? "没有匹配的引用" : "No matching references"}</div>}
      </div>
    </div>
  );
}

/*
 * The picker implementation above intentionally keeps the old public
 * selection behavior for artifact and session triggers. The skill-only
 * command branch is resolved before remote skill results so local commands
 * remain usable while a catalog request is loading or has failed.
 */

export interface ComposerReferenceChipsProps {
  references: ComposerReference[];
  items?: ComposerCatalogItem[];
  onRemove: (reference: ComposerReference, index: number) => void;
  zh?: boolean;
  disabled?: boolean;
}

export function ComposerReferenceChips({ references, items = [], onRemove, zh = false, disabled = false }: ComposerReferenceChipsProps) {
  if (references.length === 0) return null;

  const labels = new Map(items.map((item) => [referenceKey(item.reference), item.label]));
  const descriptions = new Map(items.map((item) => [referenceKey(item.reference), item.description]));
  return (
    <div className="composer-reference-chips" aria-label={zh ? "已选引用" : "Selected references"}>
      {references.map((reference, index) => {
        const label = labels.get(referenceKey(reference)) || referenceFallbackLabel(reference);
        const source = reference.kind === "workspace_file" ? (reference.backend_id === "local" ? (zh ? "本地" : "Local") : "SSH") : null;
        return (
          <span className="composer-reference-chip" title={reference.kind === "workspace_file" ? `${source} · ${reference.backend_id} · ${reference.relative_path}` : descriptions.get(referenceKey(reference))} key={`${referenceKey(reference)}-${index}`}>
            <span className="composer-reference-chip-marker" aria-hidden={source ? undefined : true}>{source ?? triggerMarker(reference.kind)}</span>
            <span className="composer-reference-chip-label">{label}</span>
            <button
              type="button"
              disabled={disabled}
              aria-label={zh ? `移除引用：${label}` : `Remove reference: ${label}`}
              onClick={() => onRemove(reference, index)}
            >
              ×
            </button>
          </span>
        );
      })}
    </div>
  );
}

function referenceFallbackLabel(reference: ComposerReference): string {
  switch (reference.kind) {
    case "workspace_file":
      return `${reference.relative_path} · ${reference.backend_id}`;
    case "skill":
    case "project":
      return reference.id;
    case "execution_context":
      return reference.backend_id;
    case "runtime":
      return `${reference.language} · ${reference.backend_id}`;
    default:
      return reference.id;
  }
}
