import { useCallback, useEffect, useRef, useState } from "react";
import { ArrowDownToLine, ArrowUpFromLine, RefreshCw } from "lucide-react";

import {
  cancelSyncTransfer,
  chooseProjectFiles,
  downloadProjectFile,
  listSyncEntries,
  pauseSyncTransfer,
  retrySyncTransfer,
  uploadSelectedFiles,
} from "../../tauri-api";
import type { SyncEntry, WorkspaceProject } from "../../types";
import type { Locale } from "../workspace/copy";
import "./RemoteAccessSettings.css";

export interface RemoteAccessSettingsProps {
  selectedProject: WorkspaceProject | null;
  locale: Locale;
  onOpenEnvironments: () => void;
}

export function RemoteAccessSettings({
  selectedProject,
  locale,
  onOpenEnvironments,
}: RemoteAccessSettingsProps) {
  const zh = locale === "zh-CN";
  const [entries, setEntries] = useState<SyncEntry[]>([]);
  const [loading, setLoading] = useState(false);
  const [listError, setListError] = useState<string | null>(null);
  const [actionError, setActionError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [downloadPath, setDownloadPath] = useState("");
  const [entryBusy, setEntryBusy] = useState<Set<string>>(() => new Set());
  const [uploadBusy, setUploadBusy] = useState(false);
  const [downloadBusy, setDownloadBusy] = useState(false);
  const projectGeneration = useRef(0);
  const snapshotRequest = useRef(0);
  const entryGuards = useRef(new Set<string>());
  const pageGuards = useRef(new Set<"upload" | "download">());

  const boundProject = Boolean(
    selectedProject?.connection_id && selectedProject.remote_root,
  );

  const loadEntries = useCallback(async (
    project: WorkspaceProject,
    generation: number,
  ) => {
    const request = ++snapshotRequest.current;
    setLoading(true);
    setListError(null);
    try {
      const next = await listSyncEntries(project.id);
      if (projectGeneration.current === generation && snapshotRequest.current === request) {
        setEntries(next.filter((item) => item.project_id === project.id));
      }
    } catch (reason) {
      if (projectGeneration.current === generation && snapshotRequest.current === request) {
        setListError(errorMessage(reason));
      }
    } finally {
      if (projectGeneration.current === generation && snapshotRequest.current === request) {
        setLoading(false);
      }
    }
  }, []);

  useEffect(() => {
    const generation = ++projectGeneration.current;
    ++snapshotRequest.current;
    entryGuards.current.clear();
    pageGuards.current.clear();
    setEntries([]);
    setLoading(false);
    setListError(null);
    setActionError(null);
    setNotice(null);
    setDownloadPath("");
    setEntryBusy(new Set());
    setUploadBusy(false);
    setDownloadBusy(false);
    if (selectedProject && selectedProject.connection_id && selectedProject.remote_root) {
      void loadEntries(selectedProject, generation);
    }
    return () => {
      if (projectGeneration.current === generation) {
        ++projectGeneration.current;
        ++snapshotRequest.current;
      }
    };
  }, [loadEntries, selectedProject?.connection_id, selectedProject?.id, selectedProject?.remote_root]);

  const refresh = useCallback(() => {
    if (!selectedProject || !boundProject) return;
    setActionError(null);
    setNotice(null);
    void loadEntries(selectedProject, projectGeneration.current);
  }, [boundProject, loadEntries, selectedProject]);

  async function runEntryAction(
    item: SyncEntry,
    action: "pause" | "cancel" | "retry",
  ) {
    if (!selectedProject || item.project_id !== selectedProject.id || entryGuards.current.has(item.id)) return;
    const generation = projectGeneration.current;
    entryGuards.current.add(item.id);
    setEntryBusy((current) => new Set(current).add(item.id));
    setActionError(null);
    setNotice(null);
    try {
      if (action === "pause") {
        await pauseSyncTransfer(item.id);
        if (projectGeneration.current === generation) await loadEntries(selectedProject, generation);
      } else if (action === "cancel") {
        await cancelSyncTransfer(item.id);
        if (projectGeneration.current === generation) await loadEntries(selectedProject, generation);
      } else {
        const next = await retrySyncTransfer(item.id);
        if (projectGeneration.current === generation) {
          ++snapshotRequest.current;
          setLoading(false);
          setEntries((current) => mergeEntries(current, [next], selectedProject.id));
        }
      }
    } catch (reason) {
      if (projectGeneration.current === generation) setActionError(errorMessage(reason));
    } finally {
      if (projectGeneration.current === generation) {
        entryGuards.current.delete(item.id);
        setEntryBusy((current) => {
          const next = new Set(current);
          next.delete(item.id);
          return next;
        });
      }
    }
  }

  async function upload() {
    if (!selectedProject || !boundProject || pageGuards.current.has("upload")) return;
    const project = selectedProject;
    const generation = projectGeneration.current;
    pageGuards.current.add("upload");
    setUploadBusy(true);
    setActionError(null);
    setNotice(null);
    try {
      const relativePaths = await chooseProjectFiles(project.local_root);
      if (projectGeneration.current !== generation) return;
      if (relativePaths.length === 0) return;
      const uploaded = await uploadSelectedFiles(project.id, relativePaths);
      if (projectGeneration.current === generation) {
        ++snapshotRequest.current;
        setLoading(false);
        setEntries((current) => mergeEntries(current, uploaded, project.id));
        setNotice(zh
          ? `${uploaded.length} 个文件传输已完成`
          : `${uploaded.length} file transfer${uploaded.length === 1 ? "" : "s"} finished`);
      }
    } catch (reason) {
      if (projectGeneration.current === generation) setActionError(errorMessage(reason));
    } finally {
      if (projectGeneration.current === generation) {
        pageGuards.current.delete("upload");
        setUploadBusy(false);
      }
    }
  }

  async function download() {
    const relativePath = downloadPath.trim();
    if (!selectedProject || !boundProject || !relativePath || pageGuards.current.has("download")) return;
    const project = selectedProject;
    const generation = projectGeneration.current;
    pageGuards.current.add("download");
    setDownloadBusy(true);
    setActionError(null);
    setNotice(null);
    try {
      const result = await downloadProjectFile(project.id, relativePath);
      if (projectGeneration.current === generation) {
        ++snapshotRequest.current;
        setLoading(false);
        setEntries((current) => mergeEntries(current, [result.entry], project.id));
        setNotice(result.conflict
          ? zh
            ? `本地文件不同，已保留冲突副本：${result.entry.relative_path}`
            : `Local file differed; kept conflict copy: ${result.entry.relative_path}`
          : zh
            ? `下载完成并通过校验：${result.entry.relative_path}`
            : `Download finished and verified: ${result.entry.relative_path}`);
        setDownloadPath("");
      }
    } catch (reason) {
      if (projectGeneration.current === generation) setActionError(errorMessage(reason));
    } finally {
      if (projectGeneration.current === generation) {
        pageGuards.current.delete("download");
        setDownloadBusy(false);
      }
    }
  }

  return (
    <main className="remote-access-settings">
      <header className="appearance-heading">
        <h3>{zh ? "远程访问" : "Remote Access"}</h3>
        <p>
          {zh
            ? "管理当前项目本地目录与已绑定 SSH 主机之间的文件传输、状态和恢复操作。"
            : "Manage file transfers, status, and recovery between the current project's local directory and its bound SSH host."}
        </p>
      </header>

      {!selectedProject && (
        <section className="appearance-card remote-access-empty">
          <strong>{zh ? "未打开项目" : "No project is open"}</strong>
          <p>{zh ? "打开项目后才能管理它的 SSH 文件传输。" : "Open a project to manage its SSH file transfers."}</p>
          <button type="button" onClick={onOpenEnvironments}>{zh ? "打开环境设置" : "Open Environments"}</button>
        </section>
      )}

      {selectedProject && !boundProject && (
        <section className="appearance-card remote-access-empty">
          <strong>{zh ? `项目“${selectedProject.name}”尚未绑定 SSH` : `${selectedProject.name} is not bound to SSH`}</strong>
          <p>
            {zh
              ? "此项目没有同时配置 SSH 连接和远端根目录。请先在环境设置中完成连接测试、主机信任和项目绑定。"
              : "This project does not have an SSH connection and remote root. Test and trust a host, then bind the project in Environments."}
          </p>
          <button type="button" onClick={onOpenEnvironments}>{zh ? "打开环境设置" : "Open Environments"}</button>
        </section>
      )}

      {selectedProject && boundProject && (
        <>
          <section className="appearance-card remote-access-scope" aria-labelledby="remote-access-scope-heading">
            <div className="appearance-group-heading">
              <h4 id="remote-access-scope-heading">{zh ? "项目绑定" : "Project binding"}</h4>
              <p>{zh ? "所有操作只使用当前项目已保存的本地根目录、SSH 连接和远端根目录。" : "Every operation uses this project's saved local root, SSH connection, and remote root."}</p>
            </div>
            <dl>
              <div><dt>{zh ? "项目" : "Project"}</dt><dd>{selectedProject.name}</dd></div>
              <div><dt>{zh ? "本地根目录" : "Local root"}</dt><dd><code>{selectedProject.local_root}</code></dd></div>
              <div><dt>{zh ? "SSH 连接" : "SSH connection"}</dt><dd><code>{selectedProject.connection_id}</code></dd></div>
              <div><dt>{zh ? "远端根目录" : "Remote root"}</dt><dd><code>{selectedProject.remote_root}</code></dd></div>
            </dl>
            <button type="button" className="remote-access-link-button" onClick={onOpenEnvironments}>
              {zh ? "管理项目绑定" : "Manage project binding"}
            </button>
          </section>

          <section className="appearance-card remote-access-transfer" aria-labelledby="remote-access-new-heading">
            <div className="appearance-group-heading">
              <h4 id="remote-access-new-heading">{zh ? "显式传输" : "Explicit transfer"}</h4>
              <p>{zh ? "上传选择器仅接受项目根目录内的文件；下载路径相对于远端项目根目录。" : "The upload picker accepts only files inside the project root. Download paths are relative to the remote project root."}</p>
            </div>
            <div className="remote-access-transfer-row">
              <div>
                <strong>{zh ? "从本地上传到 SSH" : "Upload local files to SSH"}</strong>
                <small>{zh ? "选择一个或多个当前项目文件。" : "Choose one or more files from the current project."}</small>
              </div>
              <button type="button" disabled={uploadBusy} onClick={() => void upload()} aria-label={zh ? "选择项目文件上传" : "Choose project files to upload"}>
                <ArrowUpFromLine size={14} />{uploadBusy ? (zh ? "上传中…" : "Uploading…") : (zh ? "选择文件" : "Choose files")}
              </button>
            </div>
            <div className="remote-access-download-row">
              <label>
                <span>{zh ? "从 SSH 下载到本地" : "Download from SSH to local"}</span>
                <small>{zh ? "输入项目内相对路径，例如 results/counts.tsv。" : "Enter a project-relative path, such as results/counts.tsv."}</small>
                <input
                  aria-label={zh ? "远端项目相对路径" : "Remote project-relative path"}
                  value={downloadPath}
                  onChange={(event) => setDownloadPath(event.target.value)}
                  placeholder="results/counts.tsv"
                />
              </label>
              <button type="button" disabled={downloadBusy || !downloadPath.trim()} onClick={() => void download()} aria-label={zh ? "从 SSH 下载" : "Download from SSH"}>
                <ArrowDownToLine size={14} />{downloadBusy ? (zh ? "下载中…" : "Downloading…") : (zh ? "下载" : "Download")}
              </button>
            </div>
          </section>

          {(actionError || listError) && (
            <section className="appearance-card remote-access-message remote-access-error" role="alert">
              <strong>{zh ? "传输操作未完成" : "Transfer action did not finish"}</strong>
              <p>{actionError ?? listError}</p>
              {listError && <button type="button" onClick={refresh}>{zh ? "重试刷新" : "Retry refresh"}</button>}
            </section>
          )}
          {notice && <div className="appearance-card remote-access-message" role="status">{notice}</div>}

          <section className="appearance-card remote-access-list" aria-labelledby="remote-access-list-heading">
            <div className="appearance-group-heading remote-access-list-heading">
              <div>
                <h4 id="remote-access-list-heading">{zh ? "传输记录" : "Transfer records"}</h4>
                <p>{zh ? "状态来自当前项目已持久化的实际传输记录。不会自动重试失败或不确定的派发。" : "States come from persisted transfer records for this project. Failed or uncertain dispatches are never retried automatically."}</p>
              </div>
              <button type="button" onClick={refresh} disabled={loading} aria-label={zh ? "刷新传输记录" : "Refresh transfer records"}>
                <RefreshCw size={14} className={loading ? "remote-access-spin" : undefined} />
                {loading ? (zh ? "刷新中…" : "Refreshing…") : (zh ? "刷新" : "Refresh")}
              </button>
            </div>
            {!loading && entries.length === 0 && !listError && (
              <p className="remote-access-no-records">{zh ? "当前项目还没有传输记录。" : "This project has no transfer records yet."}</p>
            )}
            {entries.length > 0 && (
              <ul>
                {entries.map((item) => {
                  const busy = entryBusy.has(item.id);
                  return (
                    <li key={item.id}>
                      <div className="remote-access-entry-main">
                        <div className="remote-access-entry-title">
                          <strong>{item.relative_path}</strong>
                          <span data-state={item.state}>{stateLabel(item.state, zh)}</span>
                        </div>
                        <div className="remote-access-entry-meta">
                          <span>{item.direction === "local_to_remote" ? (zh ? "本地 → SSH" : "Local → SSH") : (zh ? "SSH → 本地" : "SSH → local")}</span>
                          <span>{formatBytes(item.transferred_bytes, locale)} / {formatBytes(item.size_bytes, locale)}</span>
                          <span>{zh ? `重试 ${item.retry_count} 次` : `${item.retry_count} retries`}</span>
                        </div>
                        <progress max={Math.max(1, item.size_bytes)} value={item.transferred_bytes} />
                        <div className="remote-access-paths">
                          {item.local_relative_path && <code>{zh ? "本地：" : "Local: "}{item.local_relative_path}</code>}
                          {item.remote_path && <code>{zh ? "远端：" : "Remote: "}{item.remote_path}</code>}
                        </div>
                        {item.error && <p className="remote-access-entry-error">{item.error}</p>}
                      </div>
                      <div className="remote-access-entry-actions">
                        {item.state === "transferring" && (
                          <>
                            <button type="button" disabled={busy} aria-label={`${zh ? "暂停" : "Pause"} ${item.relative_path}`} onClick={() => void runEntryAction(item, "pause")}>{zh ? "暂停" : "Pause"}</button>
                            <button type="button" disabled={busy} aria-label={`${zh ? "取消" : "Cancel"} ${item.relative_path}`} onClick={() => void runEntryAction(item, "cancel")}>{zh ? "取消" : "Cancel"}</button>
                          </>
                        )}
                        {(["paused", "failed", "canceled"] as SyncEntry["state"][]).includes(item.state) && (
                          <button type="button" disabled={busy} aria-label={`${zh ? "重试" : "Retry"} ${item.relative_path}`} onClick={() => void runEntryAction(item, "retry")}>{busy ? (zh ? "处理中…" : "Working…") : (zh ? "续传/重试" : "Resume/retry")}</button>
                        )}
                      </div>
                    </li>
                  );
                })}
              </ul>
            )}
            <footer>
              {zh
                ? "在此取消只会停止选中的文件传输，不会取消 Agent 运行或远端计算。"
                : "Canceling here stops only the selected file transfer. It does not cancel an Agent run or remote computation."}
            </footer>
          </section>
        </>
      )}
    </main>
  );
}

function mergeEntries(current: SyncEntry[], incoming: SyncEntry[], projectId: string): SyncEntry[] {
  const merged = new Map(current.filter((item) => item.project_id === projectId).map((item) => [item.id, item]));
  for (const item of incoming) {
    if (item.project_id === projectId) merged.set(item.id, item);
  }
  return [...merged.values()].sort((left, right) => right.updated_at.localeCompare(left.updated_at));
}

function errorMessage(reason: unknown): string {
  return reason instanceof Error ? reason.message : String(reason);
}

function stateLabel(state: SyncEntry["state"], zh: boolean): string {
  const labels: Record<SyncEntry["state"], [string, string]> = {
    pending: ["等待中", "Pending"],
    transferring: ["传输中", "Transferring"],
    paused: ["已暂停", "Paused"],
    canceled: ["已取消", "Canceled"],
    synced: ["已同步", "Synced"],
    conflict: ["冲突副本", "Conflict copy"],
    failed: ["失败", "Failed"],
  };
  return labels[state][zh ? 0 : 1];
}

function formatBytes(bytes: number, locale: Locale): string {
  if (bytes < 1024) return `${new Intl.NumberFormat(locale).format(bytes)} B`;
  const units = ["KB", "MB", "GB", "TB"];
  let value = bytes / 1024;
  let unit = units[0];
  for (let index = 1; index < units.length && value >= 1024; index += 1) {
    value /= 1024;
    unit = units[index];
  }
  return `${new Intl.NumberFormat(locale, { maximumFractionDigits: 1 }).format(value)} ${unit}`;
}
