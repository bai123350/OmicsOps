import { useEffect, useRef, useState } from "react";

import { settingsReadSkillFile, settingsRemoveSkill, settingsSkillDetail } from "../../skill-settings-api";
import type { SkillFilePreview, SkillRemovalMode, SkillRemovalResult, SkillSettingsDetail } from "../../types";
import type { Locale } from "../workspace/copy";
import { useWindowEscapeLayer } from "./BrowserSettings";
import "./SkillDetails.css";

const detailDialogStyle = {
  width: "min(720px, calc(100% - 36px))",
  maxHeight: "calc(100% - 48px)",
};

const previewDialogStyle = {
  width: "min(780px, calc(100% - 28px))",
  maxHeight: "calc(100% - 32px)",
};

export function SkillDetails({
  skillId,
  locale,
  onClose,
  onRemoved,
  onOperationsChanged,
  onNavigatePlugins,
}: {
  skillId: string;
  locale: Locale;
  onClose: () => void;
  onRemoved?: () => Promise<void>;
  onOperationsChanged?: () => Promise<void>;
  onNavigatePlugins?: () => void;
}) {
  const zh = locale === "zh-CN";
  const generation = useRef(0);
  const previewOperation = useRef(false);
  const removalOperation = useRef(false);
  const [detail, setDetail] = useState<SkillSettingsDetail | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState(false);
  const [preview, setPreview] = useState<SkillFilePreview | null>(null);
  const [previewBusy, setPreviewBusy] = useState(false);
  const [previewError, setPreviewError] = useState(false);
  const [removalMode, setRemovalMode] = useState<SkillRemovalMode | null>(null);
  const [removalBusy, setRemovalBusy] = useState(false);
  const [removalError, setRemovalError] = useState(false);
  const [removalResult, setRemovalResult] = useState<SkillRemovalResult | null>(null);

  useWindowEscapeLayer(true, onClose);

  useEffect(() => {
    const request = ++generation.current;
    setDetail(null);
    setLoading(true);
    setError(false);
    setPreview(null);
    setPreviewBusy(false);
    setPreviewError(false);
    setRemovalMode(null);
    setRemovalBusy(false);
    setRemovalError(false);
    setRemovalResult(null);
    previewOperation.current = false;
    removalOperation.current = false;
    void settingsSkillDetail(skillId)
      .then((value) => {
        if (generation.current === request) setDetail(value);
      })
      .catch(() => {
        if (generation.current === request) setError(true);
      })
      .finally(() => {
        if (generation.current === request) setLoading(false);
      });
    return () => {
      generation.current += 1;
      previewOperation.current = false;
      removalOperation.current = false;
    };
  }, [skillId]);

  async function openFile(relativePath: string) {
    if (!detail || previewOperation.current) return;
    const request = generation.current;
    previewOperation.current = true;
    setPreviewBusy(true);
    setPreviewError(false);
    try {
      const value = await settingsReadSkillFile(detail.skill.id, relativePath, detail.skill.sha256);
      if (generation.current === request) setPreview(value);
    } catch {
      if (generation.current === request) setPreviewError(true);
    } finally {
      if (generation.current === request) {
        previewOperation.current = false;
        setPreviewBusy(false);
      }
    }
  }

  async function removeSkill(mode: SkillRemovalMode) {
    if (!detail || removalOperation.current) return;
    const request = generation.current;
    removalOperation.current = true;
    setRemovalBusy(true);
    setRemovalError(false);
    try {
      const result = await settingsRemoveSkill(detail.skill.id, detail.skill.sha256, mode);
      if (generation.current !== request) return;
      setRemovalResult(result);
      setRemovalMode(null);
      const refreshes = await Promise.allSettled([
        result.removed_from_library ? onRemoved?.() : undefined,
        onOperationsChanged?.(),
      ]);
      if (generation.current !== request) return;
      if (refreshes.some((refresh) => refresh.status === "rejected")) {
        setRemovalError(true);
        return;
      }
      if (mode === "library_only" || (result.files_removed && !result.preserved_files)) onClose();
    } catch {
      if (generation.current === request) setRemovalError(true);
    } finally {
      if (generation.current === request) {
        removalOperation.current = false;
        setRemovalBusy(false);
      }
    }
  }

  return <div className="skill-details-overlay" role="presentation">
    <section className="skill-details-dialog" style={detailDialogStyle} role="dialog" aria-modal="true" aria-label={zh ? "技能详情" : "Skill details"}>
      <header>
        <div><small>{zh ? "已安装技能" : "Installed skill"}</small><h3>{detail?.skill.name ?? (zh ? "技能详情" : "Skill details")}</h3></div>
        <button type="button" onClick={onClose} aria-label={zh ? "关闭技能详情" : "Close skill details"}>×</button>
      </header>
      {loading ? <p role="status">{zh ? "正在验证技能包…" : "Validating skill package…"}</p> : error ? <p role="alert">{zh ? "无法读取技能详情。" : "Could not load skill details."}</p> : detail ? <>
        <dl className="skill-detail-metadata">
          <div><dt>{zh ? "来源" : "Origin"}</dt><dd>{originLabel(detail.origin, zh)}</dd></div>
          <div><dt>{zh ? "完整性" : "Integrity"}</dt><dd>{detail.integrity}</dd></div>
          <div><dt>{zh ? "版本" : "Version"}</dt><dd>{detail.skill.version}</dd></div>
          <div><dt>SHA-256</dt><dd><code>{detail.skill.sha256}</code></dd></div>
          <div><dt>{zh ? "安装位置" : "Installed at"}</dt><dd><code>{detail.skill.source_path}</code></dd></div>
        </dl>
        {detail.blocking_reasons.length > 0 && <div className="skill-detail-notice">{detail.blocking_reasons.map((reason) => <p key={reason}>{reason}</p>)}</div>}
        <div className="skill-detail-section-heading"><h4>{zh ? "包文件" : "Package files"}</h4><small>{detail.inventory_complete ? `${detail.files.length}` : (zh ? "清单不完整" : "Inventory incomplete")}</small></div>
        {previewError && <p role="alert" className="skill-detail-error">{zh ? "无法安全预览该文件。详情仍保持打开。" : "Could not safely preview that file. Details remain open."}</p>}
        <ul className="skill-file-list">
          {detail.files.map((file) => <li key={file.relative_path}>
            <span><code>{file.relative_path}</code><small>{formatBytes(file.size_bytes)}</small></span>
            <button type="button" disabled={!file.previewable || previewBusy} onClick={() => void openFile(file.relative_path)}>{file.previewable ? (previewBusy ? (zh ? "读取中…" : "Reading…") : (zh ? "预览" : "Preview")) : (zh ? "不可预览" : "Not previewable")}</button>
          </li>)}
        </ul>
        <section className="skill-removal-actions" aria-label={zh ? "技能移除" : "Skill removal"}>
          <div><h4>{zh ? "移除技能" : "Remove skill"}</h4><p>{zh ? "从库移除始终保留安装文件；只有宿主确认拥有的受管文件才能物理删除。" : "Library removal always keeps installed files. Physical deletion is limited to files the host can still verify as owned."}</p></div>
          {removalError && <p role="alert" className="skill-detail-error">{zh ? "技能移除或刷新未完成。请检查下方清理状态后重试。" : "Skill removal or refresh did not finish. Check cleanup status below and retry."}</p>}
          {removalResult && <p role="status" aria-label={zh ? "技能移除结果" : "Skill removal result"} className={removalResult.preserved_files ? "skill-removal-partial" : ""}>{removalResult.message}</p>}
          <div className="skill-removal-buttons">
            {detail.origin === "plugin_owned" ? <button type="button" disabled={!onNavigatePlugins} onClick={onNavigatePlugins}>{zh ? "在插件中管理" : "Manage in Plugins"}</button> : <>
              {detail.can_remove_from_library && <button type="button" disabled={removalBusy} onClick={() => setRemovalMode("library_only")}>{zh ? "从库移除（保留安装文件）" : "Remove from library (keep installed files)"}</button>}
              {detail.can_delete_files && <button type="button" className="danger" disabled={removalBusy} onClick={() => setRemovalMode("owned_files")}>{zh ? "删除自有安装文件" : "Delete owned installation files"}</button>}
            </>}
          </div>
        </section>
      </> : null}
    </section>
    {preview && <SkillPreviewDialog preview={preview} zh={zh} onClose={() => setPreview(null)} />}
    {removalMode && detail && <SkillRemovalConfirmation mode={removalMode} zh={zh} busy={removalBusy} onCancel={() => setRemovalMode(null)} onConfirm={() => void removeSkill(removalMode)} />}
  </div>;
}

function SkillRemovalConfirmation({ mode, zh, busy, onCancel, onConfirm }: { mode: SkillRemovalMode; zh: boolean; busy: boolean; onCancel: () => void; onConfirm: () => void }) {
  useWindowEscapeLayer(true, onCancel);
  const libraryOnly = mode === "library_only";
  return <section className="skill-removal-confirm" role="dialog" aria-modal="true" aria-label={zh ? "确认移除技能" : "Confirm skill removal"}>
    <h3>{libraryOnly ? (zh ? "从库移除？" : "Remove from library?") : (zh ? "删除自有安装文件？" : "Delete owned installation files?")}</h3>
    <p>{libraryOnly
      ? (zh ? "将移除库记录。安装文件会保留，不会释放存储空间。" : "The library entry will be removed. Installed files will remain and no storage space will be reclaimed.")
      : (zh ? "只删除仍可验证属于此受管安装的文件。已修改或无法验证的文件会保留。" : "Only files still verified as owned by this managed installation will be deleted. Changed or unverified files will be preserved.")}</p>
    <div><button type="button" disabled={busy} onClick={onCancel}>{zh ? "取消" : "Cancel"}</button><button type="button" className="danger" disabled={busy} onClick={onConfirm}>{busy ? (zh ? "处理中…" : "Working…") : libraryOnly ? (zh ? "确认从库移除" : "Confirm library removal") : (zh ? "确认删除自有文件" : "Confirm owned-file deletion")}</button></div>
  </section>;
}

function SkillPreviewDialog({ preview, zh, onClose }: { preview: SkillFilePreview; zh: boolean; onClose: () => void }) {
  useWindowEscapeLayer(true, onClose);
  return <section className="skill-preview-dialog" style={previewDialogStyle} role="dialog" aria-modal="true" aria-label={zh ? "技能文件预览" : "Skill file preview"}>
    <header><div><small>{preview.redacted ? (zh ? "敏感内容已脱敏" : "Sensitive content redacted") : (zh ? "只读预览" : "Read-only preview")}</small><h3>{preview.relative_path}</h3></div><button type="button" onClick={onClose} aria-label={zh ? "关闭文件预览" : "Close file preview"}>×</button></header>
    <pre>{preview.content}</pre>
  </section>;
}

function originLabel(origin: SkillSettingsDetail["origin"], zh: boolean) {
  const labels = zh
    ? { bundled: "内置", managed_import: "受管导入", plugin_owned: "插件管理", legacy_unknown: "旧版来源未知", external: "外部" }
    : { bundled: "Bundled", managed_import: "Managed import", plugin_owned: "Plugin managed", legacy_unknown: "Legacy origin unknown", external: "External" };
  return labels[origin];
}

function formatBytes(bytes: number) {
  if (bytes < 1024) return `${bytes} B`;
  return `${(bytes / 1024).toFixed(1)} KiB`;
}
