import { useCallback, useEffect, useLayoutEffect, useRef, useState, type KeyboardEvent as ReactKeyboardEvent } from "react";
import { CircleAlert, LoaderCircle, Save, X } from "lucide-react";

import type { Locale } from "./copy";
import type { ModelProfile, ReviewerBackendChoiceV4, ReviewerSettingsV4 } from "../../types";
import { useWindowEscapeLayer } from "../settings/BrowserSettings";
import { useReviewerSettings } from "./useReviewerSettings";
import "./reviewer-settings.css";

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

export interface ReviewerSettingsDialogProps {
  zh: boolean;
  modelProfiles?: ModelProfile[];
  onClose: () => void;
  onSaved?: (settings: ReviewerSettingsV4) => void;
}

function backendLabel(backend: ReviewerBackendChoiceV4, zh: boolean): string {
  if (backend.kind === "follow_session") return zh ? "跟随当前会话" : "Follow active session";
  if (backend.kind === "default_http") return zh ? "默认 HTTP 模型" : "Default HTTP profile";
  return zh ? "指定 HTTP 模型" : "Specific HTTP profile";
}

function cloneReviewerSettings(settings: ReviewerSettingsV4): ReviewerSettingsV4 {
  return {
    backend: settings.backend.kind === "http_profile"
      ? { kind: "http_profile", profile_id: settings.backend.profile_id }
      : { kind: settings.backend.kind },
    default_http_profile_id: settings.default_http_profile_id ?? null,
  };
}

export function ReviewerSettingsDialog({ zh, modelProfiles = [], onClose, onSaved }: ReviewerSettingsDialogProps) {
  const reviewer = useReviewerSettings();
  const dialogRef = useRef<HTMLElement>(null);
  const closeRef = useRef<HTMLButtonElement>(null);
  const previousFocusRef = useRef<HTMLElement | null>(null);
  const mountedRef = useRef(false);
  const saveInFlightRef = useRef(false);
  const draftDirtyRef = useRef(false);
  const persistedDraftRef = useRef<ReviewerSettingsV4>(cloneReviewerSettings(reviewer.settings));
  const [draft, setDraft] = useState<ReviewerSettingsV4>(reviewer.settings);
  const [actionError, setActionError] = useState("");

  useLayoutEffect(() => {
    mountedRef.current = true;
    if (typeof document !== "undefined") previousFocusRef.current = document.activeElement instanceof HTMLElement ? document.activeElement : null;
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

  useEffect(() => {
    if (!reviewer.saving && !draftDirtyRef.current) {
      const next = cloneReviewerSettings(reviewer.settings);
      persistedDraftRef.current = next;
      setDraft(next);
    }
  }, [reviewer.saving, reviewer.settings]);

  useWindowEscapeLayer(true, onClose);

  const handleDialogKeyDown = useCallback((event: ReactKeyboardEvent<HTMLElement>) => {
    if (event.key !== "Tab") return;
    const focusables = focusableElements(dialogRef.current);
    if (!focusables.length) {
      event.preventDefault();
      return;
    }
    const active = typeof document !== "undefined" && document.activeElement instanceof HTMLElement ? document.activeElement : null;
    const index = active ? focusables.indexOf(active) : -1;
    const direction = event.shiftKey ? -1 : 1;
    const next = index < 0 ? (event.shiftKey ? focusables.length - 1 : 0) : (index + direction + focusables.length) % focusables.length;
    event.preventDefault();
    focusables[next].focus();
  }, []);

  function changeBackend(kind: ReviewerBackendChoiceV4["kind"]) {
    if (kind === "follow_session") setDraft((current) => ({ ...current, backend: { kind } }));
    else if (kind === "default_http") setDraft((current) => ({ ...current, backend: { kind } }));
    else {
      const profileId = currentHttpProfileId(modelProfiles, draft.default_http_profile_id) ?? modelProfiles[0]?.id ?? "";
      setDraft((current) => ({ ...current, backend: { kind: "http_profile", profile_id: profileId } }));
    }
    draftDirtyRef.current = true;
    setActionError("");
  }

  function currentHttpProfileId(profiles: ModelProfile[], preferred: string | null | undefined): string | undefined {
    if (preferred && profiles.some((profile) => profile.id === preferred)) return preferred;
    return profiles[0]?.id;
  }

  async function save() {
    if (saveInFlightRef.current || reviewer.busy || !canSave) return;
    saveInFlightRef.current = true;
    setActionError("");
    try {
      const accepted = await reviewer.save(draft);
      draftDirtyRef.current = false;
      if (accepted) {
        persistedDraftRef.current = cloneReviewerSettings(draft);
        if (mountedRef.current) {
          setDraft(persistedDraftRef.current);
          onSaved?.(reviewer.settings);
        }
      } else if (mountedRef.current) {
        // The hook rolls its durable state back when the native save fails.
        // Reset the local form explicitly as well; relying on the settings
        // effect alone races with the hook's asynchronous state updates.
        setDraft(cloneReviewerSettings(persistedDraftRef.current));
      }
    } catch {
      if (mountedRef.current) setActionError(zh ? "无法保存审核模型设置，请重试。" : "Could not save reviewer settings. Please retry.");
    } finally {
      saveInFlightRef.current = false;
    }
  }

  const needsProfile = draft.backend.kind === "default_http" || draft.backend.kind === "http_profile";
  const selectedProfileId = draft.backend.kind === "http_profile" ? draft.backend.profile_id : draft.default_http_profile_id ?? "";
  const selectedProfileExists = Boolean(selectedProfileId && modelProfiles.some((profile) => profile.id === selectedProfileId));
  const canSave = !reviewer.loading && !reviewer.loadError && !reviewer.saving && (!needsProfile || selectedProfileExists);
  const error = actionError || reviewer.error;

  return <div className="reviewer-settings-backdrop">
    <section ref={dialogRef} className="reviewer-settings-dialog" role="dialog" aria-modal="true" aria-label={zh ? "审核模型" : "Reviewer model"} aria-busy={reviewer.busy || undefined} onKeyDown={handleDialogKeyDown}>
      <header className="reviewer-settings-header"><div><span className="reviewer-settings-kicker">OmicsOps</span><h2>{zh ? "审核模型" : "Reviewer model"}</h2><p>{zh ? "为新的手动或自动审核选择只读模型绑定。跟随会话会使用当前明确选择的模型配置。" : "Choose the read-only model binding for new manual or automatic reviews. Follow session uses the explicitly selected model profile."}</p></div><button ref={closeRef} type="button" className="reviewer-settings-close" aria-label={zh ? "关闭审核模型设置" : "Close reviewer model settings"} onClick={onClose}><X size={18} /></button></header>
      {reviewer.loading && <div className="reviewer-settings-state" role="status"><LoaderCircle size={16} className="reviewer-settings-spinner" aria-hidden="true" />{zh ? "正在加载审核模型设置…" : "Loading reviewer settings…"}</div>}
      {error && <div className="reviewer-settings-alert" role="alert"><CircleAlert size={16} aria-hidden="true" /><span>{error}</span><button type="button" onClick={() => reviewer.retry()} disabled={reviewer.saving}>{zh ? "重试" : "Retry"}</button></div>}
      <form className="reviewer-settings-form" onSubmit={(event) => { event.preventDefault(); void save(); }}>
        <fieldset disabled={reviewer.loading || reviewer.saving}>
          <label className="reviewer-settings-field"><span>{zh ? "审核后端" : "Backend"}</span><select aria-label={zh ? "审核后端" : "Reviewer backend"} value={draft.backend.kind} onChange={(event) => changeBackend(event.target.value as ReviewerBackendChoiceV4["kind"])}><option value="follow_session">{backendLabel({ kind: "follow_session" }, zh)}</option><option value="default_http">{backendLabel({ kind: "default_http" }, zh)}</option><option value="http_profile">{backendLabel({ kind: "http_profile", profile_id: "" }, zh)}</option></select><small>{draft.backend.kind === "follow_session" ? (zh ? "沿用当前会话的明确模型，并以独立只读请求运行。" : "Use the active session's explicit model in an isolated read-only request.") : (zh ? "必须选择一个明确保存的 HTTP 模型配置。" : "An explicitly saved HTTP model profile is required.")}</small></label>
          {draft.backend.kind === "default_http" && <label className="reviewer-settings-field"><span>{zh ? "默认 HTTP 模型" : "Default HTTP profile"}</span><select aria-label={zh ? "默认 HTTP 模型" : "Default HTTP profile"} value={draft.default_http_profile_id ?? ""} onChange={(event) => { draftDirtyRef.current = true; setDraft((current) => ({ ...current, default_http_profile_id: event.target.value || null })); }}><option value="">{zh ? "请选择模型" : "Choose a profile"}</option>{modelProfiles.map((profile) => <option key={profile.id} value={profile.id}>{profile.label} · {profile.model}</option>)}</select></label>}
          {draft.backend.kind === "http_profile" && <label className="reviewer-settings-field"><span>{zh ? "指定 HTTP 模型" : "Specific HTTP profile"}</span><select aria-label={zh ? "指定 HTTP 模型" : "Reviewer HTTP profile"} value={draft.backend.profile_id} onChange={(event) => { draftDirtyRef.current = true; setDraft((current) => ({ ...current, backend: { kind: "http_profile", profile_id: event.target.value } })); }}><option value="">{zh ? "请选择模型" : "Choose a profile"}</option>{modelProfiles.map((profile) => <option key={profile.id} value={profile.id}>{profile.label} · {profile.model}</option>)}</select></label>}
          {needsProfile && !selectedProfileExists && <p className="reviewer-settings-inline-error" role="status">{zh ? "请先选择一个当前存在的 HTTP 模型配置。" : "Choose an existing HTTP model profile before saving."}</p>}
          <p className="reviewer-settings-note">{zh ? "审核不会执行工具、修改文件或替代正式结果核验。" : "Reviews do not execute tools, modify files, or replace formal result checks."}</p>
        </fieldset>
        <footer className="reviewer-settings-actions"><button type="button" className="reviewer-settings-secondary" onClick={onClose}>{zh ? "取消" : "Cancel"}</button><button type="submit" className="reviewer-settings-primary" disabled={!canSave}>{reviewer.saving ? <><LoaderCircle size={14} className="reviewer-settings-spinner" aria-hidden="true" />{zh ? "保存中…" : "Saving…"}</> : <><Save size={14} aria-hidden="true" />{zh ? "保存审核模型设置" : "Save reviewer settings"}</>}</button></footer>
      </form>
    </section>
  </div>;
}
