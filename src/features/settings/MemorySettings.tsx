import { useEffect, useRef, useState } from "react";

import {
  createProjectMemoryFile,
  deleteProjectMemoryFile,
  listProjectMemoryFiles,
  readProjectMemoryFile,
  updateProjectMemoryFile,
} from "../../memory-settings-api";
import type { MemoryFileSummaryV4, MemoryFileV4, WorkspaceProject } from "../../types";
import type { Locale } from "../workspace/copy";
import "../../memory.css";

export function MemorySettings({
  selectedProject,
  locale,
  onChanged,
}: {
  selectedProject: WorkspaceProject | null;
  locale: Locale;
  onChanged?: () => void;
}) {
  const zh = locale === "zh-CN";
  const [files, setFiles] = useState<MemoryFileSummaryV4[]>([]);
  const [selected, setSelected] = useState<MemoryFileV4 | null>(null);
  const [draft, setDraft] = useState("");
  const [filename, setFilename] = useState("");
  const [fileFilter, setFileFilter] = useState("");
  const [creating, setCreating] = useState(false);
  const [loading, setLoading] = useState(false);
  const [saving, setSaving] = useState(false);
  const [confirmingDelete, setConfirmingDelete] = useState(false);
  const [conflict, setConflict] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const sequence = useRef(0);
  const mutationInFlight = useRef(false);

  useEffect(() => {
    const request = ++sequence.current;
    setFiles([]);
    setSelected(null);
    setDraft("");
    setFilename("");
    setFileFilter("");
    setCreating(false);
    setConfirmingDelete(false);
    setConflict(false);
    setError(null);
    mutationInFlight.current = false;
    setSaving(false);
    if (!selectedProject) {
      setLoading(false);
      return;
    }
    setLoading(true);
    void (async () => {
      try {
        const nextFiles = await listProjectMemoryFiles(selectedProject.id);
        if (request !== sequence.current) return;
        setFiles(nextFiles);
        if (nextFiles.length > 0) {
          const next = await readProjectMemoryFile(selectedProject.id, nextFiles[0].name);
          if (request !== sequence.current) return;
          setSelected(next);
          setDraft(next.content);
        }
      } catch (reason) {
        if (request === sequence.current) setError(errorText(reason));
      } finally {
        if (request === sequence.current) setLoading(false);
      }
    })();
  }, [selectedProject?.id]);

  async function openFile(name: string) {
    if (!selectedProject || mutationInFlight.current) return;
    const request = ++sequence.current;
    setLoading(true);
    setError(null);
    setConflict(false);
    setConfirmingDelete(false);
    setCreating(false);
    try {
      const next = await readProjectMemoryFile(selectedProject.id, name);
      if (request !== sequence.current) return;
      setSelected(next);
      setDraft(next.content);
    } catch (reason) {
      if (request === sequence.current) setError(errorText(reason));
    } finally {
      if (request === sequence.current) setLoading(false);
    }
  }

  async function saveFile() {
    if (!selectedProject || (!creating && !selected) || mutationInFlight.current) return;
    const request = ++sequence.current;
    mutationInFlight.current = true;
    setSaving(true);
    setError(null);
    try {
      const saved = creating
        ? await createProjectMemoryFile(selectedProject.id, filename, draft)
        : await updateProjectMemoryFile(selectedProject.id, selected!.name, draft, selected!.sha256);
      if (request !== sequence.current) return;
      setSelected(saved);
      setDraft(saved.content);
      setFilename("");
      setCreating(false);
      setConflict(false);
      setFiles((current) => upsertSummary(current, saved));
      onChanged?.();
    } catch (reason) {
      if (request !== sequence.current) return;
      const message = errorText(reason);
      if (message.includes("memory_conflict")) setConflict(true);
      else setError(message);
    } finally {
      if (request === sequence.current) {
        mutationInFlight.current = false;
        setSaving(false);
      }
    }
  }

  async function deleteFile() {
    if (!selectedProject || !selected || mutationInFlight.current) return;
    const request = ++sequence.current;
    mutationInFlight.current = true;
    setSaving(true);
    setError(null);
    try {
      await deleteProjectMemoryFile(selectedProject.id, selected.name, selected.sha256);
      if (request !== sequence.current) return;
      setFiles((current) => current.filter((file) => file.name !== selected.name));
      setSelected(null);
      setDraft("");
      setConfirmingDelete(false);
      onChanged?.();
    } catch (reason) {
      if (request !== sequence.current) return;
      const message = errorText(reason);
      if (message.includes("memory_conflict")) setConflict(true);
      else setError(message);
    } finally {
      if (request === sequence.current) {
        mutationInFlight.current = false;
        setSaving(false);
      }
    }
  }

  const editorVisible = creating || selected;
  const normalizedFilter = fileFilter.trim().toLocaleLowerCase();
  const visibleFiles = files.filter((file) => !normalizedFilter || file.name.toLocaleLowerCase().includes(normalizedFilter));
  return (
    <main className="appearance-settings memory-settings">
      <header className="appearance-heading">
        <h3>{zh ? "记忆文件" : "Memory files"}</h3>
        <p>
          {zh
            ? "管理当前项目 .omicsops/memory 目录中的 Markdown 文件。这里显示的是磁盘上的项目事实，不是全局偏好或自动推理记忆。"
            : "Manage Markdown files in the current project's .omicsops/memory directory. These are project facts on disk, not global preferences or inferred memory."}
        </p>
      </header>

      {!selectedProject ? (
        <section className="appearance-card memory-empty">
          {zh
            ? "选择一个项目以管理该项目 .omicsops/memory 目录中的 Markdown 文件。"
            : "Select a project to manage Markdown files in its .omicsops/memory directory."}
        </section>
      ) : (
        <div className="memory-layout">
          <section className="appearance-card memory-file-list" aria-label={zh ? "项目记忆文件" : "Project memory files"}>
            <div className="appearance-group-heading memory-list-heading">
              <div>
                <h4>{selectedProject.name}</h4>
                <p>.omicsops/memory</p>
              </div>
              <button
                type="button"
                disabled={saving}
                onClick={() => {
                  if (mutationInFlight.current) return;
                  ++sequence.current;
                  setLoading(false);
                  setCreating(true);
                  setSelected(null);
                  setDraft("");
                  setFilename("");
                  setConflict(false);
                  setError(null);
                  setConfirmingDelete(false);
                }}
              >
                {zh ? "新建" : "New memory file"}
              </button>
            </div>
            {files.length > 0 && <input type="search" className="memory-file-filter" aria-label={zh ? "筛选记忆文件" : "Filter memory files"} placeholder={zh ? "按文件名筛选" : "Filter by filename"} value={fileFilter} onChange={(event) => setFileFilter(event.target.value)} />}
            {loading && files.length === 0 ? (
              <p className="memory-list-message">{zh ? "正在读取…" : "Loading…"}</p>
            ) : files.length === 0 ? (
              <p className="memory-list-message">{zh ? "还没有记忆文件。" : "No memory files yet."}</p>
            ) : visibleFiles.length === 0 ? (
              <p className="memory-list-message" role="status">{zh ? "没有匹配的文件名。" : "No matching filenames."}</p>
            ) : (
              <ul>
                {visibleFiles.map((file) => (
                  <li key={file.name}>
                    <button type="button" disabled={saving} className={selected?.name === file.name ? "active" : ""} onClick={() => void openFile(file.name)}>
                      <strong>{file.name}</strong>
                      <small>{formatBytes(file.size_bytes, locale)}</small>
                    </button>
                  </li>
                ))}
              </ul>
            )}
          </section>

          <section className="appearance-card memory-editor">
            {!editorVisible ? (
              <p className="memory-list-message">{zh ? "选择文件或新建一个 Markdown 文件。" : "Select a file or create a Markdown file."}</p>
            ) : (
              <>
                <div className="appearance-group-heading">
                  <h4>{creating ? (zh ? "新建记忆文件" : "New memory file") : selected!.name}</h4>
                  <p>{zh ? "内容在保存前只保留在当前草稿中。" : "Content remains in this draft until it is saved."}</p>
                </div>
                <div className="memory-form">
                  {creating && (
                    <label>
                      <span>{zh ? "文件名" : "Filename"}</span>
                      <input disabled={saving} aria-label={zh ? "文件名" : "Filename"} value={filename} onChange={(event) => setFilename(event.target.value)} placeholder="study.md" />
                    </label>
                  )}
                  <label>
                    <span>{zh ? "记忆内容" : "Memory content"}</span>
                    <textarea disabled={saving} aria-label={zh ? "记忆内容" : "Memory content"} value={draft} onChange={(event) => setDraft(event.target.value)} rows={15} />
                  </label>
                  {conflict && (
                    <div className="memory-conflict" role="alert">
                      <strong>{zh ? "磁盘文件已更改。重新读取后才能再次保存。" : "This file changed on disk. Reload it before saving again."}</strong>
                      {selected && <button type="button" onClick={() => void openFile(selected.name)}>{zh ? "重新读取文件" : "Reload file"}</button>}
                    </div>
                  )}
                  {error && <p className="memory-error" role="alert">{error}</p>}
                  <div className="memory-actions">
                    <button type="button" onClick={() => void saveFile()} disabled={loading || saving || conflict || !draft.trim() || (creating && !filename.trim())}>
                      {creating ? (zh ? "创建文件" : "Create file") : zh ? "保存更改" : "Save changes"}
                    </button>
                    {!creating && selected && !confirmingDelete && (
                      <button type="button" className="danger" onClick={() => setConfirmingDelete(true)} disabled={loading || saving || conflict}>{zh ? "删除文件" : "Delete file"}</button>
                    )}
                    {confirmingDelete && selected && (
                      <div className="memory-delete-confirm">
                        <span>{zh ? `删除 ${selected.name}？此操作无法撤销。` : `Delete ${selected.name}? This cannot be undone.`}</span>
                        <button type="button" className="danger" onClick={() => void deleteFile()} disabled={loading || saving || conflict}>{zh ? "确认删除" : "Confirm delete"}</button>
                        <button type="button" onClick={() => setConfirmingDelete(false)} disabled={saving}>{zh ? "取消" : "Cancel"}</button>
                      </div>
                    )}
                  </div>
                </div>
              </>
            )}
          </section>
        </div>
      )}
    </main>
  );
}

function upsertSummary(files: MemoryFileSummaryV4[], saved: MemoryFileV4): MemoryFileSummaryV4[] {
  const summary = { project_id: saved.project_id, name: saved.name, size_bytes: saved.size_bytes, sha256: saved.sha256 };
  return [...files.filter((file) => file.name !== saved.name), summary].sort((left, right) => left.name.localeCompare(right.name));
}

function errorText(reason: unknown): string {
  return reason instanceof Error ? reason.message : String(reason);
}

function formatBytes(bytes: number, locale: Locale): string {
  if (bytes < 1024) return `${new Intl.NumberFormat(locale).format(bytes)} B`;
  return `${new Intl.NumberFormat(locale, { maximumFractionDigits: 1 }).format(bytes / 1024)} KB`;
}
