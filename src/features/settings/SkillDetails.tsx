import { useEffect, useRef, useState } from "react";

import { settingsReadSkillFile, settingsSkillDetail } from "../../skill-settings-api";
import type { SkillFilePreview, SkillSettingsDetail } from "../../types";
import type { Locale } from "../workspace/copy";
import { useWindowEscapeLayer } from "./BrowserSettings";
import "./SkillDetails.css";

export function SkillDetails({
  skillId,
  locale,
  onClose,
}: {
  skillId: string;
  locale: Locale;
  onClose: () => void;
}) {
  const zh = locale === "zh-CN";
  const generation = useRef(0);
  const previewOperation = useRef(false);
  const [detail, setDetail] = useState<SkillSettingsDetail | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState(false);
  const [preview, setPreview] = useState<SkillFilePreview | null>(null);
  const [previewBusy, setPreviewBusy] = useState(false);
  const [previewError, setPreviewError] = useState(false);

  useWindowEscapeLayer(true, onClose);

  useEffect(() => {
    const request = ++generation.current;
    setDetail(null);
    setLoading(true);
    setError(false);
    setPreview(null);
    setPreviewError(false);
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

  return <div className="skill-details-overlay" role="presentation">
    <section className="skill-details-dialog" role="dialog" aria-modal="true" aria-label={zh ? "技能详情" : "Skill details"}>
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
      </> : null}
    </section>
    {preview && <SkillPreviewDialog preview={preview} zh={zh} onClose={() => setPreview(null)} />}
  </div>;
}

function SkillPreviewDialog({ preview, zh, onClose }: { preview: SkillFilePreview; zh: boolean; onClose: () => void }) {
  useWindowEscapeLayer(true, onClose);
  return <section className="skill-preview-dialog" role="dialog" aria-modal="true" aria-label={zh ? "技能文件预览" : "Skill file preview"}>
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
