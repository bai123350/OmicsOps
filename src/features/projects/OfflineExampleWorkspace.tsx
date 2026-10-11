import { useState } from "react";
import type { OfflineExampleUnavailableReason, OfflineExampleWorkspaceView } from "../../types";
import type { Locale } from "../workspace/copy";

const reasons: Record<OfflineExampleUnavailableReason, { zh: string; en: string }> = {
  receipt_missing: { zh: "示例收据缺失。", en: "The example receipt is missing." },
  receipt_invalid: { zh: "示例收据无效。", en: "The example receipt is invalid." },
  root_unavailable: { zh: "示例目录不可访问。", en: "The example directory is unavailable." },
  identity_mismatch: { zh: "项目与示例来源身份不一致。", en: "The project and example source identities do not match." },
};

export function OfflineExampleWorkspace({ workspace, locale, onBack }: { workspace: OfflineExampleWorkspaceView; locale: Locale; onBack: () => void }) {
  const zh = locale === "zh-CN";
  const language = zh ? "zh" : "en";
  const [selectedPath, setSelectedPath] = useState<string | null>(null);
  const available = workspace.availability === "available";
  const file = available ? workspace.files.find((item) => item.relative_path === selectedPath && item.preview_text !== null && (item.availability === "available" || item.availability === "changed")) : null;
  const statuses = zh ? { available: "与预置文件一致", changed: "已更改", missing: "缺失", unavailable: "不可用" } : { available: "Matches preset file", changed: "Changed", missing: "Missing", unavailable: "Unavailable" };
  const sections: Record<string, string> = zh ? { goal: "目标", observations: "合成表", summary: "预置结果", reports: "报告" } : { goal: "Goal", observations: "Synthetic observations", summary: "Preset results", reports: "Reports" };
  return <main className="offline-example-workspace" aria-label={zh ? "离线合成示例" : "Offline synthetic example"}>
    <header><button type="button" onClick={onBack}>{zh ? "返回项目库" : "Back to projects"}</button><h1>{workspace.title[language]}</h1></header>
    <aside className="offline-example-banner" aria-label={zh ? "合成来源" : "Synthetic source"}>
      <strong>{workspace.disclaimer[language]}</strong>
      <p>{workspace.source_description[language]}</p>
      <small>{workspace.example_id} · {workspace.license_spdx}</small>
    </aside>
    {!available ? <p role="alert">{zh ? "示例不可用：" : "Example unavailable: "}{workspace.unavailable_reason ? reasons[workspace.unavailable_reason][language] : (zh ? "无法读取示例来源。" : "The example source could not be read.")}</p> : <>
      <div className="offline-example-sections">{workspace.entries.map((entry) => <section key={entry.id} data-example-entry={entry.id}><h2>{sections[entry.id] ?? entry.id}</h2><p>{entry.text[language]}</p></section>)}</div>
      <div className="offline-example-files">
        <nav aria-label={zh ? "示例文件" : "Example files"}>{workspace.files.map((item) => <button type="button" key={item.relative_path} aria-pressed={item.relative_path === selectedPath} disabled={item.preview_text === null || item.availability === "missing" || item.availability === "unavailable"} onClick={() => setSelectedPath(item.relative_path)}><span>{item.relative_path}</span><small>{statuses[item.availability]}</small></button>)}</nav>
        {file ? <section role="region" aria-label={zh ? "文本预览" : "Text preview"}><h2>{file.relative_path}</h2><p>{file.availability === "changed" ? (zh ? "当前用户文本；已更改，不再是原始预置文件。" : "Current user text; changed from the original preset file.") : (zh ? "预置文件文本；未执行计算。" : "Preset file text; no computation was executed.")}</p><dl><dt>{zh ? "预置 SHA-256" : "Expected SHA-256"}</dt><dd>{file.expected_sha256}</dd><dt>{zh ? "当前 SHA-256" : "Current SHA-256"}</dt><dd>{file.current_sha256 ?? "—"}</dd></dl><pre>{file.preview_text}</pre></section> : <p>{zh ? "选择文件以查看安全文本预览。" : "Select a file to view its literal text preview."}</p>}
      </div>
    </>}
  </main>;
}
