import { useCallback, useEffect, useRef, useState } from "react";

import { settingsStorageUsage } from "../../tauri-api";
import type {
  StorageScanIssueV4,
  StorageUsageCategoryV4,
  StorageUsageSnapshotV4,
  WorkspaceProject,
} from "../../types";
import type { Locale } from "../workspace/copy";
import "./StorageSettings.css";

type ScopeChoice = "managed" | "project";

export function StorageSettings({
  selectedProject,
  locale,
}: {
  selectedProject: WorkspaceProject | null;
  locale: Locale;
}) {
  const zh = locale === "zh-CN";
  const [scope, setScope] = useState<ScopeChoice>("managed");
  const [snapshot, setSnapshot] = useState<StorageUsageSnapshotV4 | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const requestSequence = useRef(0);

  useEffect(() => {
    if (!selectedProject && scope === "project") setScope("managed");
  }, [scope, selectedProject]);

  const load = useCallback(async () => {
    const request = ++requestSequence.current;
    const projectId = scope === "project" ? selectedProject?.id : undefined;
    setSnapshot(null);
    setLoading(true);
    setError(null);
    try {
      const next = await settingsStorageUsage(projectId);
      if (request === requestSequence.current) setSnapshot(next);
    } catch (reason) {
      if (request === requestSequence.current) {
        setError(reason instanceof Error ? reason.message : String(reason));
      }
    } finally {
      if (request === requestSequence.current) setLoading(false);
    }
  }, [scope, selectedProject?.id]);

  useEffect(() => {
    void load();
  }, [load]);

  const fullProject = scope === "project" && selectedProject;
  return (
    <main className="storage-settings">
      <header className="appearance-heading">
        <h3>{zh ? "存储" : "Storage"}</h3>
        <p>
          {zh
            ? "查看 OmicsOps 管理的数据和项目目录实际可读取的逻辑文件字节数。此页面不会删除或清理文件。"
            : "Inspect logical file bytes that OmicsOps can read in managed data and project directories. This page does not delete or clean files."}
        </p>
      </header>

      <section className="appearance-card storage-scope-card">
        <label className="appearance-row">
          <span className="appearance-copy">
            <strong>{zh ? "扫描范围" : "Scan scope"}</strong>
            <small>
              {fullProject
                ? zh
                  ? `受管应用数据和完整本地项目根目录：${selectedProject.name}`
                  : `Managed app data plus full local project root: ${selectedProject.name}`
                : zh
                  ? "仅应用受管数据和项目元数据"
                  : "Managed app data and project metadata only"}
            </small>
          </span>
          <select
            aria-label={zh ? "存储范围" : "Storage scope"}
            value={scope}
            onChange={(event) => setScope(event.target.value as ScopeChoice)}
          >
            <option value="managed">{zh ? "受管数据" : "Managed data"}</option>
            {selectedProject && (
              <option value="project">
                {zh ? `受管数据和完整项目根目录：${selectedProject.name}` : `Managed data plus full project root: ${selectedProject.name}`}
              </option>
            )}
          </select>
        </label>
        <button type="button" onClick={() => void load()} disabled={loading} aria-label={zh ? "刷新存储占用" : "Refresh storage usage"}>
          {loading ? (zh ? "扫描中…" : "Scanning…") : zh ? "刷新" : "Refresh"}
        </button>
      </section>

      {error && (
        <section className="appearance-card storage-error" role="alert">
          <strong>{zh ? "无法读取存储占用" : "Storage usage could not be read"}</strong>
          <p>{error}</p>
          <button type="button" onClick={() => void load()}>{zh ? "重试" : "Retry"}</button>
        </section>
      )}

      {snapshot && (
        <>
          <section className={`appearance-card storage-summary${snapshot.status === "partial" ? " is-partial" : ""}`} aria-live="polite">
            <div className="appearance-group-heading">
              <h4>{zh ? "逻辑文件字节数" : "Logical file bytes"}</h4>
              <p>
                {snapshot.status === "partial"
                  ? zh
                    ? `已知小计：${formatBytes(snapshot.known_logical_bytes, locale)}`
                    : `Known subtotal: ${formatBytes(snapshot.known_logical_bytes, locale)}`
                  : formatBytes(snapshot.known_logical_bytes, locale)}
              </p>
              <strong>
                {snapshot.status === "partial"
                  ? zh
                    ? "部分扫描"
                    : "Partial scan"
                  : zh
                    ? "扫描完成"
                    : "Scan complete"}
              </strong>
            </div>
            <p>
              {zh
                ? `最多扫描 ${formatCount(snapshot.limits.max_entries, locale)} 个条目或 ${formatCount(snapshot.limits.max_duration_ms, locale)} 毫秒。符号链接和 Windows 重解析点不会被跟随，其目标不计入范围。`
                : `Bounded to ${formatCount(snapshot.limits.max_entries, locale)} entries or ${formatCount(snapshot.limits.max_duration_ms, locale)} ms. Symbolic links and Windows reparse points are not followed, and their targets are outside this scope.`}
            </p>
          </section>

          <section className="appearance-card storage-categories">
            <div className="appearance-group-heading">
              <h4>{zh ? "分类" : "Categories"}</h4>
            </div>
            <ul>
              {snapshot.entries.map((entry, index) => (
                <li key={`${entry.category}:${entry.project_id ?? "app"}:${entry.path}:${index}`}>
                  <div>
                    <strong>{categoryLabel(entry.category, zh)}</strong>
                    <code>{entry.path}</code>
                  </div>
                  <span>
                    {entry.known_logical_bytes === null
                      ? zh
                        ? "未知"
                        : "Unknown"
                      : formatBytes(entry.known_logical_bytes, locale)}
                  </span>
                  {entry.issue && <small>{issueLabel(entry.issue, zh)}</small>}
                </li>
              ))}
            </ul>
          </section>
        </>
      )}
    </main>
  );
}

function categoryLabel(category: StorageUsageCategoryV4, zh: boolean): string {
  const labels: Record<StorageUsageCategoryV4, [string, string]> = {
    database: ["应用数据库", "Application database"],
    skills: ["受管 Skills", "Managed Skills"],
    browser: ["浏览器数据", "Browser data"],
    project_metadata: ["项目元数据", "Project metadata"],
    project_root: ["项目根目录", "Project root"],
  };
  return labels[category][zh ? 0 : 1];
}

function issueLabel(issue: StorageScanIssueV4, zh: boolean): string {
  const labels: Record<StorageScanIssueV4, [string, string]> = {
    not_created: ["尚未创建（已知为 0）", "Not created (known zero)"],
    missing: ["目录缺失，大小未知", "Directory missing; size unknown"],
    unreadable: ["部分内容不可读取", "Some content could not be read"],
    entry_limit: ["达到条目上限", "Entry limit reached"],
    time_limit: ["达到时间上限", "Time limit reached"],
  };
  return labels[issue][zh ? 0 : 1];
}

function formatCount(value: number, locale: Locale): string {
  return new Intl.NumberFormat(locale).format(value);
}

function formatBytes(bytes: number, locale: Locale): string {
  if (bytes < 1024) return `${formatCount(bytes, locale)} B`;
  const units = ["KB", "MB", "GB", "TB"];
  let value = bytes / 1024;
  let unit = units[0];
  for (let index = 1; index < units.length && value >= 1024; index += 1) {
    value /= 1024;
    unit = units[index];
  }
  return `${new Intl.NumberFormat(locale, { maximumFractionDigits: 1 }).format(value)} ${unit}`;
}
