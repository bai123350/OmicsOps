import { useMemo, useState } from "react";
import { ChevronDown, ChevronRight, Download, FileCode2, FileImage, FileJson, FileSpreadsheet, FileText, Folder, FolderOpen, RefreshCw, Upload } from "lucide-react";

import type { RemoteFileEntry, SyncEntry } from "../../types";
import type { Locale } from "./copy";

interface Props {
  locale: Locale;
  remoteFiles?: RemoteFileEntry[];
  busy: boolean;
  notice?: string;
  onUpload?: () => Promise<void> | void;
  onRefresh?: () => Promise<void> | void;
  onDownload?: (relativePath: string) => Promise<void> | void;
  syncEntries?: SyncEntry[];
  onPauseSync?: (id: string) => Promise<void> | void;
  onCancelSync?: (id: string) => Promise<void> | void;
  onRetrySync?: (id: string) => Promise<void> | void;
}

interface RemoteTreeNode {
  name: string;
  relativePath: string;
  directory: boolean;
  sizeBytes: number;
  children: RemoteTreeNode[];
}

export function RemoteFileTree({ locale, remoteFiles, busy, notice, onUpload, onRefresh, onDownload, syncEntries = [], onPauseSync, onCancelSync, onRetrySync }: Props) {
  const zh = locale === "zh-CN";
  const demoFiles: RemoteFileEntry[] = [
    { relative_path: "data", directory: true, size_bytes: 0, modified_unix_seconds: 0 },
    { relative_path: "analysis", directory: true, size_bytes: 0, modified_unix_seconds: 0 },
    { relative_path: "results/umap.png", directory: false, size_bytes: 1_258_291, modified_unix_seconds: 0 },
    { relative_path: "results/markers.csv", directory: false, size_bytes: 86_016, modified_unix_seconds: 0 },
  ];
  const files = remoteFiles ?? demoFiles;
  const tree = useMemo(() => buildRemoteTree(files), [files]);
  const [expandedPaths, setExpandedPaths] = useState<Set<string>>(() => new Set());

  function toggleDirectory(relativePath: string) {
    setExpandedPaths((current) => {
      const next = new Set(current);
      if (next.has(relativePath)) next.delete(relativePath);
      else next.add(relativePath);
      return next;
    });
  }

  return <div className="file-tree">
    <div className="context-heading file-heading">
      <div><b>{zh ? "项目文件" : "Project files"}</b><small>{zh ? "仅同步明确选择的文件" : "Explicit selective sync"}</small></div>
      <div className="file-heading-actions">
        <button aria-label={zh ? "刷新远端目录" : "Refresh remote directory"} disabled={busy || !onRefresh} onClick={() => void onRefresh?.()}><RefreshCw size={14} /></button>
        <button className="upload-action" aria-label={zh ? "选择上传文件" : "Choose files to upload"} disabled={busy || !onUpload} onClick={() => void onUpload?.()}><Upload size={14} /><span>{zh ? "上传" : "Upload"}</span></button>
      </div>
    </div>
    {notice && <div className="file-notice" role="status">{notice}</div>}
    {syncEntries.length > 0 && <div className="sync-queue"><b>{zh ? "传输任务" : "Transfers"}</b>{syncEntries.slice(-5).reverse().map((entry) => <article key={entry.id}><span>{entry.relative_path}</span><small>{entry.state} · {formatBytes(entry.transferred_bytes)} / {formatBytes(entry.size_bytes)} · {entry.retry_count} retries</small><progress max={Math.max(1, entry.size_bytes)} value={entry.transferred_bytes} />{entry.error && <em>{entry.error}</em>}<div>{entry.state === "transferring" && <><button onClick={() => void onPauseSync?.(entry.id)}>{zh ? "暂停" : "Pause"}</button><button onClick={() => void onCancelSync?.(entry.id)}>{zh ? "取消" : "Cancel"}</button></>}{(["paused", "failed", "canceled"] as string[]).includes(entry.state) && <button onClick={() => void onRetrySync?.(entry.id)}>{zh ? "续传/重试" : "Resume/retry"}</button>}</div></article>)}</div>}
    {files.length === 0 && <div className="empty-file-tree">{zh ? "远端目录为空" : "The remote directory is empty"}</div>}
    {tree.length > 0 && <div className="remote-file-tree" role="tree" aria-label={zh ? "远端项目文件" : "Remote project files"}>{tree.map((node) => <RemoteTreeItem key={node.relativePath} node={node} depth={0} locale={locale} expandedPaths={expandedPaths} busy={busy} onToggle={toggleDirectory} onDownload={onDownload} />)}</div>}
  </div>;
}

function RemoteTreeItem({ node, depth, locale, expandedPaths, busy, onToggle, onDownload }: { node: RemoteTreeNode; depth: number; locale: Locale; expandedPaths: Set<string>; busy: boolean; onToggle: (relativePath: string) => void; onDownload?: (relativePath: string) => Promise<void> | void }) {
  const zh = locale === "zh-CN";
  const expanded = expandedPaths.has(node.relativePath);
  const paddingLeft = 10 + depth * 16;
  if (node.directory) {
    return <>
      <button className="remote-tree-directory" role="treeitem" aria-level={depth + 1} aria-expanded={expanded} aria-label={`${expanded ? (zh ? "折叠文件夹" : "Collapse folder") : (zh ? "展开文件夹" : "Expand folder")}：${node.name}`} title={node.relativePath} style={{ paddingLeft }} onClick={() => onToggle(node.relativePath)}>
        {expanded ? <ChevronDown className="remote-tree-chevron" size={15} /> : <ChevronRight className="remote-tree-chevron" size={15} />}
        {expanded ? <FolderOpen className="remote-tree-folder-icon" size={16} /> : <Folder className="remote-tree-folder-icon" size={16} />}
        <span>{node.name}</span>
      </button>
      {expanded && node.children.length > 0 && <div role="group">{node.children.map((child) => <RemoteTreeItem key={child.relativePath} node={child} depth={depth + 1} locale={locale} expandedPaths={expandedPaths} busy={busy} onToggle={onToggle} onDownload={onDownload} />)}</div>}
    </>;
  }
  return <div className="remote-tree-file" role="treeitem" aria-level={depth + 1} title={node.relativePath} style={{ paddingLeft: paddingLeft + 20 }}>
    <RemoteFileIcon relativePath={node.relativePath} />
    <span>{node.name}</span>
    <em>{formatBytes(node.sizeBytes)}</em>
    {onDownload && <button aria-label={`${zh ? "下载" : "Download"} ${node.relativePath}`} disabled={busy} onClick={() => void onDownload(node.relativePath)}><Download size={13} /></button>}
  </div>;
}

function RemoteFileIcon({ relativePath }: { relativePath: string }) {
  const extension = relativePath.split(".").pop()?.toLowerCase();
  if (["png", "jpg", "jpeg", "gif", "webp", "svg", "tif", "tiff"].includes(extension ?? "")) return <FileImage className="remote-tree-image-icon" size={16} />;
  if (["py", "r", "rs", "ts", "tsx", "js", "jsx", "sh"].includes(extension ?? "")) return <FileCode2 className="remote-tree-code-icon" size={16} />;
  if (["json", "jsonl", "yaml", "yml"].includes(extension ?? "")) return <FileJson className="remote-tree-json-icon" size={16} />;
  if (["csv", "tsv", "xlsx", "xls"].includes(extension ?? "")) return <FileSpreadsheet className="remote-tree-table-icon" size={16} />;
  return <FileText className="remote-tree-document-icon" size={16} />;
}

function buildRemoteTree(entries: RemoteFileEntry[]): RemoteTreeNode[] {
  interface MutableNode extends RemoteTreeNode { childMap: Map<string, MutableNode> }
  const root = new Map<string, MutableNode>();
  for (const entry of entries) {
    const normalized = entry.relative_path.replaceAll("\\", "/").replace(/^\/+|\/+$/g, "");
    if (!normalized) continue;
    const parts = normalized.split("/").filter(Boolean);
    let children = root;
    let currentPath = "";
    parts.forEach((name, index) => {
      currentPath = currentPath ? `${currentPath}/${name}` : name;
      const leaf = index === parts.length - 1;
      const directory = !leaf || entry.directory;
      let node = children.get(name);
      if (!node) {
        node = { name, relativePath: currentPath, directory, sizeBytes: leaf ? entry.size_bytes : 0, children: [], childMap: new Map() };
        children.set(name, node);
      } else if (directory) {
        node.directory = true;
      } else if (leaf) {
        node.sizeBytes = entry.size_bytes;
      }
      children = node.childMap;
    });
  }

  function finish(children: Map<string, MutableNode>): RemoteTreeNode[] {
    return [...children.values()]
      .map((node) => ({ name: node.name, relativePath: node.relativePath, directory: node.directory, sizeBytes: node.sizeBytes, children: finish(node.childMap) }))
      .sort((left, right) => Number(right.directory) - Number(left.directory) || left.name.localeCompare(right.name, undefined, { numeric: true, sensitivity: "base" }));
  }
  return finish(root);
}

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${Math.round(bytes / 1024)} KB`;
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}
