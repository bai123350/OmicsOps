import { useCallback, useEffect, useLayoutEffect, useRef, useState, type KeyboardEvent as ReactKeyboardEvent } from "react";
import { CircleAlert, ClipboardCheck, LoaderCircle, RefreshCw, X } from "lucide-react";

import type { Locale } from "./copy";
import type { ModelProfile, SessionReviewRecordV4, SessionReviewSeverityV4 } from "../../types";
import { useWindowEscapeLayer } from "../settings/BrowserSettings";
import "./session-review.css";

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

export interface SessionReviewDialogProps {
  locale: Locale;
  records: SessionReviewRecordV4[];
  loading?: boolean;
  busy?: boolean;
  startDisabled?: boolean;
  startDisabledReason?: string;
  error?: string;
  modelProfiles?: ModelProfile[];
  onStartReview: () => Promise<void> | void;
  onRetry?: () => Promise<void> | void;
  onOpenReviewerSettings?: () => void;
  onClose: () => void;
}

function statusLabel(status: SessionReviewRecordV4["status"], zh: boolean): string {
  if (status === "running") return zh ? "审核中" : "Running";
  if (status === "completed") return zh ? "已完成" : "Completed";
  if (status === "failed") return zh ? "失败" : "Failed";
  return zh ? "已中断" : "Abandoned";
}

function reviewErrorMessage(raw: string, zh: boolean): string {
  const message = raw.trim();
  if (!zh) {
    return message === "Could not load review history. Please retry."
      || message === "Could not start the review. Please retry."
      || message === "Could not refresh the review status. Please retry."
      || message === "Choose a model before requesting a review."
      || message === "Review was not started. Check the conversation and reviewer model settings, then retry."
      ? message
      : "The review could not be completed. Please retry.";
  }
  if (message === "Could not load review history. Please retry.") return "无法加载审核记录，请重试。";
  if (message === "Could not start the review. Please retry.") return "无法发起审核，请重试。";
  if (message === "Could not refresh the review status. Please retry.") return "无法刷新审核状态，请重试。";
  if (message === "Choose a model before requesting a review.") return "请先选择审核模型。";
  if (message === "Review was not started. Check the conversation and reviewer model settings, then retry.") {
    return "审核未启动，请检查会话和审核模型设置后重试。";
  }
  return "审核操作未完成，请重试。";
}

function statusDescription(status: SessionReviewRecordV4["status"], zh: boolean): string {
  if (status === "running") return zh ? "只读审核请求正在处理。关闭此窗口不会取消它。" : "The read-only review is still processing. Closing this window will not cancel it.";
  if (status === "completed") return zh ? "报告已保存，可回看当时的证据摘录。" : "The report is saved with the evidence excerpts captured at review time.";
  if (status === "failed") return zh ? "审核没有生成可用报告。可以发起一次新的审核。" : "The review did not produce a report. You can start a new review.";
  return zh ? "此请求在当前应用进程中没有可用的所有者。可以发起一次新的审核。" : "This request has no active owner in the current app process. You can start a new review.";
}

function severityLabel(severity: SessionReviewSeverityV4, zh: boolean): string {
  if (severity === "error") return zh ? "问题" : "Issue";
  if (severity === "warn") return zh ? "提醒" : "Warning";
  return zh ? "通过" : "OK";
}

function formatTimestamp(value: string): string {
  const parsed = Date.parse(value);
  return Number.isFinite(parsed) ? new Date(parsed).toLocaleString() : value;
}

function sourceCoverage(record: SessionReviewRecordV4, zh: boolean): string {
  return `${record.sources.length} / ${record.source_message_count} ${zh ? "条消息已摘录" : "messages excerpted"}`;
}

function newestReview(records: SessionReviewRecordV4[]): SessionReviewRecordV4 | null {
  return records.reduce<SessionReviewRecordV4 | null>((latest, record) => {
    if (!latest) return record;
    const latestTime = Date.parse(latest.updated_at || latest.created_at);
    const recordTime = Date.parse(record.updated_at || record.created_at);
    if (Number.isFinite(recordTime) && (!Number.isFinite(latestTime) || recordTime > latestTime)) return record;
    return latest;
  }, null);
}

export function SessionReviewDialog({ locale, records, loading = false, busy = false, startDisabled = false, startDisabledReason, error = "", modelProfiles = [], onStartReview, onRetry, onOpenReviewerSettings, onClose }: SessionReviewDialogProps) {
  const zh = locale === "zh-CN";
  const dialogRef = useRef<HTMLElement>(null);
  const closeRef = useRef<HTMLButtonElement>(null);
  const previousFocusRef = useRef<HTMLElement | null>(null);
  const mountedRef = useRef(false);
  const actionRef = useRef(false);
  const [actionBusy, setActionBusy] = useState(false);
  const [actionError, setActionError] = useState("");
  const [selectedId, setSelectedId] = useState<string | null>(null);

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
    if (!records.length) {
      setSelectedId(null);
      return;
    }
    const newest = newestReview(records);
    setSelectedId((current) => current && records.some((record) => record.id === current) ? current : newest?.id ?? records[0].id);
  }, [records]);

  useWindowEscapeLayer(true, onClose);

  const selected = records.find((record) => record.id === selectedId) ?? newestReview(records) ?? null;
  const reviewerProfile = selected ? modelProfiles.find((profile) => profile.id === selected.reviewer_profile_id) : undefined;

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

  async function start() {
    if (actionRef.current || busy || loading) return;
    actionRef.current = true;
    setActionBusy(true);
    setActionError("");
    try {
      await onStartReview();
    } catch {
      if (mountedRef.current) setActionError(zh ? "无法发起审核，请重试。" : "Could not start the review. Please retry.");
    } finally {
      actionRef.current = false;
      if (mountedRef.current) setActionBusy(false);
    }
  }

  async function retry() {
    if (actionRef.current || !onRetry) return;
    actionRef.current = true;
    setActionBusy(true);
    setActionError("");
    try {
      await onRetry();
    } catch {
      if (mountedRef.current) setActionError(zh ? "重试失败，请稍后再试。" : "Retry failed. Please try again.");
    } finally {
      actionRef.current = false;
      if (mountedRef.current) setActionBusy(false);
    }
  }

  const actionDisabled = actionBusy || busy || loading || startDisabled;
  const report = selected?.report;
  const displayedError = error ? reviewErrorMessage(error, zh) : "";

  return <div className="session-review-backdrop">
    <section ref={dialogRef} className="session-review-dialog" role="dialog" aria-modal="true" aria-label={zh ? "会话审核" : "Session review"} aria-busy={loading || busy || actionBusy || undefined} onKeyDown={handleDialogKeyDown}>
      <header className="session-review-header">
        <div>
          <span className="session-review-kicker"><ClipboardCheck size={14} aria-hidden="true" />{zh ? "只读证据回看" : "READ-ONLY EVIDENCE REVIEW"}</span>
          <h2>{zh ? "会话审核" : "Session review"}</h2>
          <p>{zh ? "回看已保存的会话证据，帮助发现方法、推理和记录中的缺口。该报告不会验证实验或替代正式结果核验。" : "Review the saved conversation evidence for gaps in methods, reasoning, and records. This report does not verify an experiment or replace formal result checks."}</p>
        </div>
        <button ref={closeRef} type="button" className="session-review-close" aria-label={zh ? "关闭会话审核" : "Close session review"} onClick={onClose}><X size={18} /></button>
      </header>

      <div className="session-review-toolbar">
        <div><strong>{zh ? "回顾记录" : "Review history"}</strong><small>{records.length ? `${records.length} ${zh ? "条记录" : records.length === 1 ? "record" : "records"}` : (zh ? "尚无保存记录" : "No saved records")}</small></div>
        <div className="session-review-toolbar-actions">
          {onOpenReviewerSettings && <button type="button" className="session-review-secondary" onClick={onOpenReviewerSettings} disabled={actionBusy}>{zh ? "审核模型" : "Reviewer model"}</button>}
          <button type="button" className="session-review-primary" onClick={() => void start()} disabled={actionDisabled}>{actionBusy || busy ? (zh ? "审核中…" : "Reviewing…") : (zh ? "发起审核" : "Request review")}</button>
        </div>
      </div>

      {startDisabled && <p className="session-review-muted" role="status">{startDisabledReason ?? (zh ? "请等待当前运行或设置操作完成后再发起审核。" : "Finish the current run or settings operation before requesting a review.")}</p>}
      {displayedError && <div className="session-review-alert" role="alert"><CircleAlert size={16} aria-hidden="true" /><span>{displayedError}</span>{onRetry && <button type="button" onClick={() => void retry()} disabled={actionBusy}>{zh ? "重试" : "Retry review"}</button>}</div>}
      {actionError && <div className="session-review-alert" role="alert"><CircleAlert size={16} aria-hidden="true" /><span>{actionError}</span></div>}
      {loading && <div className="session-review-state" role="status"><LoaderCircle size={17} className="session-review-spinner" aria-hidden="true" />{zh ? "正在加载审核记录…" : "Loading review history…"}</div>}
      {!loading && !records.length && !error && <div className="session-review-empty" role="status"><strong>{zh ? "还没有审核记录" : "No review history yet"}</strong><span>{zh ? "发起审核后，报告和证据摘录会保存在当前会话中。" : "Request a review to save a report with its evidence excerpts for this conversation."}</span></div>}

      {records.length > 0 && <div className="session-review-content">
        <nav className="session-review-history" aria-label={zh ? "审核记录" : "Review history"}>
          {records.map((record) => <button type="button" key={record.id} className={`session-review-history-item${selected?.id === record.id ? " is-selected" : ""}`} aria-current={selected?.id === record.id ? "true" : undefined} onClick={() => setSelectedId(record.id)}>
            <span><b>{statusLabel(record.status, zh)}</b><small>{formatTimestamp(record.updated_at || record.created_at)}</small></span>
            <em>{sourceCoverage(record, zh)}</em>
          </button>)}
        </nav>

        {selected && <article className="session-review-report" aria-label={zh ? "审核报告" : "Review report"}>
          <div className="session-review-report-heading"><div><span className={`session-review-status status-${selected.status}`}>{statusLabel(selected.status, zh)}</span><h3>{selected.status === "completed" ? (zh ? "审核报告" : "Review report") : statusDescription(selected.status, zh)}</h3></div><time dateTime={selected.updated_at}>{formatTimestamp(selected.updated_at || selected.created_at)}</time></div>
          <p className="session-review-status-description">{statusDescription(selected.status, zh)}</p>
          {report && <>
            <section className="session-review-summary"><h4>{zh ? "摘要" : "Summary"}</h4><p>{report.summary}</p></section>
            <section className="session-review-findings" aria-label={zh ? "审核发现" : "Review findings"}><h4>{zh ? "发现" : "Findings"}</h4>{report.findings.length ? <ul>{report.findings.map((finding, index) => <li key={`${finding.code}-${index}`}><span className={`finding-severity severity-${finding.severity}`}>{severityLabel(finding.severity, zh)}</span><div><strong>{finding.code}</strong><p>{finding.message}</p>{finding.source_ids.length > 0 && <small>{zh ? "证据消息" : "Evidence messages"}: {finding.source_ids.map((sourceId, sourceIndex) => <span key={sourceId}>{sourceIndex > 0 ? ", " : ""}<a href={`#session-review-source-${sourceId}`}>{sourceId}</a></span>)}</small>}</div></li>)}</ul> : <p className="session-review-muted">{zh ? "报告没有结构化发现。" : "The report contains no structured findings."}</p>}</section>
          </>}

          <dl className="session-review-metadata">
            <div><dt>{zh ? "证据覆盖" : "Evidence coverage"}</dt><dd aria-label={sourceCoverage(selected, zh)}>{sourceCoverage(selected, zh)}{selected.sources.length < selected.source_message_count && <small>{zh ? "已截断为保存的摘录" : "Coverage is truncated to the stored excerpts."}</small>}</dd></div>
            <div><dt>{zh ? "审核模型" : "Reviewer model"}</dt><dd>{reviewerProfile?.label ?? (zh ? "已保存的模型配置" : "Stored model profile")}<small>{reviewerProfile?.model ? `${zh ? "当前配置模型（可能已更改）" : "Current saved model (may have changed)"}: ${reviewerProfile.model}` : (zh ? "模型标签可能已变化" : "The saved model label may have changed")}</small></dd></div>
            <div><dt>{zh ? "审核配置 ID" : "Reviewer profile ID"}</dt><dd><code>{selected.reviewer_profile_id}</code></dd></div>
            <div><dt>{zh ? "配置哈希" : "Configuration hash"}</dt><dd><code>{selected.reviewer_configuration_hash}</code></dd></div>
            <div><dt>{zh ? "证据快照哈希" : "Source snapshot hash"}</dt><dd><code>{selected.source_snapshot_sha256}</code></dd></div>
          </dl>

          <section className="session-review-sources" aria-label={zh ? "证据摘录" : "Evidence excerpts"}><h4>{zh ? "证据摘录" : "Evidence excerpts"}</h4>{selected.sources.length ? <ol>{selected.sources.map((source) => <li id={`session-review-source-${source.message_id}`} key={source.message_id}><header><span>#{source.sequence} · {source.role}</span><code>{source.message_id}</code></header><p>{source.text}</p></li>)}</ol> : <p className="session-review-muted">{zh ? "没有保存可显示的摘录。" : "No stored excerpts are available."}</p>}</section>
        </article>}
      </div>}
      {selected?.status === "running" && <div className="session-review-running-note" role="status"><LoaderCircle size={15} className="session-review-spinner" aria-hidden="true" />{zh ? "审核仍在后台运行；可关闭窗口，稍后从会话菜单重新打开。" : "The review continues in the background. You can close this window and reopen it from the session menu later."}</div>}
      {!selected && !loading && !error && <div className="session-review-tip"><RefreshCw size={14} aria-hidden="true" />{zh ? "每次发起都会创建新的只读请求。" : "Each request creates a new read-only review."}</div>}
    </section>
  </div>;
}
