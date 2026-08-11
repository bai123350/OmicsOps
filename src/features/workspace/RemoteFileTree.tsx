import { Download, FileText, Folder, RefreshCw, Upload } from "lucide-react";

import type { RemoteFileEntry } from "../../types";
import type { Locale } from "./copy";

interface Props {
  locale: Locale;
  remoteFiles?: RemoteFileEntry[];
  busy: boolean;
  notice?: string;
  onUpload?: () => Promise<void> | void;
  onRefresh?: () => Promise<void> | void;
  onDownload?: (relativePath: string) => Promise<void> | void;
}

export function RemoteFileTree({ locale, remoteFiles, busy, notice, onUpload, onRefresh, onDownload }: Props) {
  const zh = locale === "zh-CN";
  const demoFiles: RemoteFileEntry[] = [
    { relative_path: "data", directory: true, size_bytes: 0, modified_unix_seconds: 0 },
    { relative_path: "analysis", directory: true, size_bytes: 0, modified_unix_seconds: 0 },
    { relative_path: "results/umap.png", directory: false, size_bytes: 1_258_291, modified_unix_seconds: 0 },
    { relative_path: "results/markers.csv", directory: false, size_bytes: 86_016, modified_unix_seconds: 0 },
  ];
  const files = remoteFiles ?? demoFiles;

  return <div className="file-tree">
    <div className="context-heading file-heading">
      <div><b>{zh ? "项目文件" : "Project files"}</b><small>{zh ? "仅同步明确选择的文件" : "Explicit selective sync"}</small></div>
      <div className="file-heading-actions">
        <button aria-label={zh ? "刷新远端目录" : "Refresh remote directory"} disabled={busy || !onRefresh} onClick={() => void onRefresh?.()}><RefreshCw size={14} /></button>
        <button className="upload-action" aria-label={zh ? "选择上传文件" : "Choose files to upload"} disabled={busy || !onUpload} onClick={() => void onUpload?.()}><Upload size={14} /><span>{zh ? "上传" : "Upload"}</span></button>
      </div>
    </div>
    {notice && <div className="file-notice" role="status">{notice}</div>}
    {files.length === 0 && <div className="empty-file-tree">{zh ? "远端目录为空" : "The remote directory is empty"}</div>}
    {files.map((entry) => entry.directory
      ? <div className="tree-folder" key={entry.relative_path}><Folder size={15} /><span className="tree-path">{entry.relative_path}</span><em>{zh ? "远端" : "remote"}</em></div>
      : <div className="tree-file" key={entry.relative_path}><FileText size={15} /><span className="tree-path">{entry.relative_path}</span><em>{formatBytes(entry.size_bytes)}</em>{onDownload && <button aria-label={`${zh ? "下载" : "Download"} ${entry.relative_path}`} disabled={busy} onClick={() => void onDownload(entry.relative_path)}><Download size={13} /></button>}</div>)}
  </div>;
}

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${Math.round(bytes / 1024)} KB`;
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}
