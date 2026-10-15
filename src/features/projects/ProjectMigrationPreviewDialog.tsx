import { useEffect, useRef, useState } from "react";
import { previewProjectMigration } from "../../tauri-api";
import { listLocalComposerFiles } from "../../composer-file-api";
import { getWorkspaceJourney, getWorkspaceSourceDetail } from "../../workspace-navigation-api";
import type { ProjectFileSelectionV1, WorkspaceSourceRef, WorkspaceSourceSnapshot } from "../../workspace-navigation-types";
import type { MigrationMetadataCategoryV1, ProjectMigrationPreviewV1, WorkspaceProject } from "../../types";
import type { Locale } from "../workspace/copy";
import { useWindowEscapeLayer } from "../settings/BrowserSettings";
import "./project-migration-preview.css";

interface Props { project: WorkspaceProject; locale: Locale; onClose: () => void }
interface CatalogRow { key: string; file: ProjectFileSelectionV1 | null }
const categories: MigrationMetadataCategoryV1[] = ["conversations", "publications", "scientific_metadata"];
const categoryLabels = { conversations: ["会话", "Conversations"], publications: ["出版物", "Publications"], scientific_metadata: ["科学 metadata", "Scientific metadata"] };
const fileKey = (file: ProjectFileSelectionV1) => JSON.stringify([file.backend_id, file.relative_path]);
const bytes = (text: string) => new TextEncoder().encode(text).length;
// Conservative selector filter only. Native validation remains authoritative.
function safePath(path: string): boolean {
  if (!path || bytes(path) > 1024 || /[\\:*?"<>|\x00-\x1f\x7f]/.test(path) || path.startsWith("/")) return false;
  return path.split("/").every((part) => !!part && part !== "." && part !== ".." && !/[. ]$/.test(part)
    && !/^(?:con|prn|aux|nul|com[1-9]|lpt[1-9])(?:\.|$)/i.test(part)
    && !/^(?:\.omicsops|\.git|\.hg|\.svn|node_modules|target)$/i.test(part)
    && !/^(?:\.env(?:\.|$)|\.ssh$|credentials?(?:\.|$)|id_(?:rsa|ed25519)(?:\.|$))/i.test(part));
}
function sameSource(left: WorkspaceSourceRef, right: WorkspaceSourceRef): boolean {
  return (["project_id", "kind", "id", "conversation_id", "run_id", "sequence", "event_hash", "content_sha256", "start", "end"] as const).every((key) => left[key] === right[key]);
}
function savedFile(project: WorkspaceProject, source: WorkspaceSourceRef, detail: WorkspaceSourceSnapshot): ProjectFileSelectionV1 | null {
  const backend = detail.metadata?.backend_id; const path = detail.metadata?.relative_path;
  if (source.project_id !== project.id || source.kind !== "artifact" || !sameSource(source, detail.source)
    || detail.availability !== "available" || detail.metadata.content_mode !== "reference_only"
    || !project.remote_root?.trim() || !project.connection_id || !/^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/.test(project.connection_id)
    || backend !== `ssh:${project.connection_id}` || bytes(backend) > 128 || typeof path !== "string" || !safePath(path)) return null;
  return { backend_id: backend, relative_path: path, expected_full_sha256: null };
}

export function ProjectMigrationPreviewDialog(props: Props) {
  // A new binding, even on the same project ID, gets an independent lifetime.
  return <MigrationPreview key={JSON.stringify([props.project.id, props.project.connection_id, props.project.remote_root, props.project.local_root])} {...props} />;
}
function MigrationPreview({ project, locale, onClose }: Props) {
  const zh = locale === "zh-CN"; const text = (cn: string, en: string) => zh ? cn : en;
  const [selectedCategories, setCategories] = useState<MigrationMetadataCategoryV1[]>([]);
  const [selectedFiles, setFiles] = useState<ProjectFileSelectionV1[]>([]);
  const [localRows, setLocalRows] = useState<CatalogRow[]>([]); const [savedRows, setSavedRows] = useState<CatalogRow[]>([]);
  const [nextOffset, setNextOffset] = useState<number | null>(null);
  const [catalogBusy, setCatalogBusy] = useState<"local" | "saved" | null>(null); const catalogPending = useRef(false);
  const [catalogError, setCatalogError] = useState("");
  const [report, setReport] = useState<{ value: ProjectMigrationPreviewV1; revision: number } | null>(null);
  const [error, setError] = useState(""); const [busy, setBusy] = useState(false);
  const lifetime = useRef(0); const alive = useRef(true); const generation = useRef(0); const revision = useRef(0); const pending = useRef(false);
  const currentSelectionDigest = useRef("");
  currentSelectionDigest.current = JSON.stringify({ project_id: project.id, categories: selectedCategories, files: selectedFiles });
  useEffect(() => { alive.current = true; lifetime.current++; return () => { alive.current = false; lifetime.current++; generation.current++; }; }, []);
  function close() { alive.current = false; lifetime.current++; generation.current++; onClose(); }
  useWindowEscapeLayer(true, close);
  function changed() { revision.current++; generation.current++; pending.current = false; setBusy(false); setError(""); }
  function toggleCategory(category: MigrationMetadataCategoryV1) {
    changed(); setCategories((current) => categories.filter((item) => item === category ? !current.includes(item) : current.includes(item)));
  }
  function toggleFile(file: ProjectFileSelectionV1) {
    if (!selectedFiles.some((item) => fileKey(item) === fileKey(file)) && selectedFiles.length >= 32) return;
    changed(); setFiles((current) => current.some((item) => fileKey(item) === fileKey(file)) ? current.filter((item) => fileKey(item) !== fileKey(file)) : [...current, file]);
  }
  async function loadCatalog(kind: "local" | "saved", offset = 0) {
    if (catalogPending.current || !alive.current) return;
    catalogPending.current = true; setCatalogBusy(kind); setCatalogError("");
    const ticket = lifetime.current; const current = () => alive.current && ticket === lifetime.current;
    try {
      if (kind === "local") {
        const entries = await listLocalComposerFiles(project.id);
        if (!current()) return;
        const unique = new Map<string, CatalogRow>();
        for (const entry of entries) {
          if (entry.directory || !safePath(entry.relative_path)) continue;
          const file = { backend_id: "local", relative_path: entry.relative_path, expected_full_sha256: null };
          unique.set(fileKey(file), { key: fileKey(file), file });
        }
        setLocalRows([...unique.values()]);
      } else {
        const page = await getWorkspaceJourney({ project_id: project.id, query: "", kind: "artifact", offset, limit: 20 });
        if (!current()) return;
        if (!Array.isArray(page.entries) || page.entries.length > 20 || (page.next_offset !== null && (!Number.isSafeInteger(page.next_offset) || page.next_offset <= offset))) throw new Error();
        const rows: CatalogRow[] = [];
        // At most 20 saved metadata snapshots per explicit page, serially.
        for (const [index, entry] of page.entries.entries()) {
          if (!current()) return;
          let file: ProjectFileSelectionV1 | null = null;
          if (entry.source.project_id === project.id && entry.source.kind === "artifact") {
            try { const detail = await getWorkspaceSourceDetail(entry.source); if (!current()) return; file = savedFile(project, entry.source, detail); }
            catch { if (!current()) return; }
          }
          if (!file || !rows.some((row) => row.file && fileKey(row.file) === fileKey(file!))) rows.push({ key: `${offset}-${index}`, file });
        }
        if (current()) { setSavedRows(rows); setNextOffset(page.next_offset); }
      }
    } catch { if (current()) setCatalogError(text("无法读取文件 metadata，请重试。", "Could not read file metadata. Please retry.")); }
    finally { if (current()) { catalogPending.current = false; setCatalogBusy(null); } }
  }
  async function generate() {
    if (!alive.current || pending.current) return;
    const ticket = ++generation.current; const selectionRevision = revision.current; const scope = lifetime.current;
    const request = { project_id: project.id, categories: [...selectedCategories], files: selectedFiles.map((file) => ({ ...file })) };
    const selectionDigest = JSON.stringify(request);
    const current = () => alive.current && scope === lifetime.current && ticket === generation.current && selectionRevision === revision.current && selectionDigest === currentSelectionDigest.current;
    pending.current = true; setBusy(true); setError("");
    try {
      const value = await previewProjectMigration(request);
      if (!current()) return;
      if (value.project.source_project_id !== project.id) throw new Error();
      setReport({ value, revision: selectionRevision });
    } catch { if (current()) setError(text("无法生成预览，请检查选择后重试。", "Could not generate preview. Check the selection and retry.")); }
    finally { if (current()) { pending.current = false; setBusy(false); } }
  }
  const stale = report !== null && report.revision !== revision.current;
  const unavailable = text("历史 backend/path 未记录、无效或不兼容；不可选择。", "Historical backend/path missing, invalid or incompatible; unavailable for selection.");
  function row(row: CatalogRow, index: number) {
    const checked = !!row.file && selectedFiles.some((file) => fileKey(file) === fileKey(row.file!));
    return <label key={row.key}><input type="checkbox" checked={checked} disabled={!row.file || (!checked && selectedFiles.length >= 32)} onChange={() => { if (row.file) toggleFile(row.file); }} />{row.file ? row.file.relative_path : `${text("已保存引用", "Saved reference")} ${index + 1}: ${unavailable}`}</label>;
  }
  const value = report?.value;
  return <div className="migration-preview-backdrop"><section className="migration-preview-dialog" role="dialog" aria-modal="true" aria-label={text("迁移清单预览", "Migration preview")}>
    <header><h2>{text("迁移清单预览", "Migration preview")}</h2><button type="button" onClick={close} aria-label={text("关闭迁移预览", "Close migration preview")}>×</button></header>
    <div className="migration-preview-body">
      <p>{text("只读清单；生成时始终包含公开项目 metadata。", "Read-only manifest; public project metadata is always included when generated.")}</p>
      <fieldset><legend>{text("metadata 类别（默认不选）", "Metadata categories (none selected by default)")}</legend>{categories.map((category) => <label key={category}><input type="checkbox" checked={selectedCategories.includes(category)} onChange={() => toggleCategory(category)} />{categoryLabels[category][zh ? 0 : 1]}</label>)}</fieldset>
      <fieldset><legend>{text("显式选择文件（最多 32 项）", "Explicit file selection (up to 32)")}</legend>
        <button type="button" disabled={catalogBusy !== null} onClick={() => void loadCatalog("local")}>{text("读取本地文件 metadata", "Load local file metadata")}</button>{localRows.map(row)}
        <button type="button" disabled={catalogBusy !== null} onClick={() => void loadCatalog("saved")}>{text("读取已保存 SSH 引用", "Load saved SSH references")}</button>{savedRows.map(row)}
        {nextOffset !== null && <button type="button" disabled={catalogBusy !== null} onClick={() => void loadCatalog("saved", nextOffset)}>{text("下一页已保存引用", "Next saved references")}</button>}
        <p>{text("只读取 metadata 与历史引用，不连接或刷新 SSH。未知 size/hash 不视为零。", "Metadata and saved references only; no SSH connection or refresh. Unknown size/hash is not zero.")}</p>
        <p>{text("已选择", "Selected")}: {selectedFiles.length}/32</p>
        {selectedFiles.map((file) => <button type="button" key={fileKey(file)} onClick={() => toggleFile(file)}>{text("移除", "Remove")} {file.backend_id === "local" ? text("本地", "Local") : text("已保存 SSH", "Saved SSH")}: {file.relative_path}</button>)}
      </fieldset>
      {catalogError && <p role="alert">{catalogError}</p>}
      <button type="button" disabled={busy} onClick={() => void generate()}>{busy ? text("正在生成预览…", "Generating preview…") : text("生成预览", "Generate preview")}</button>
      {error && <p role="alert">{error}</p>}
      {stale && <p role="status">{text("选择已变化，旧预览已过期；请重新生成。", "Selection changed; previous preview is stale. Generate again.")}</p>}
      {value && <section className={stale ? "migration-preview-result stale" : "migration-preview-result"} aria-label={text("预览结果", "Preview result")}>
        <h3>{value.project.name}</h3><p>{value.project.description}</p><p>{value.project.template} · {value.project.created_at}</p>
        <p>{text("项目身份", "Source project")}: {value.project.source_project_id}</p><p>{text("观察时间", "Observed at")}: {value.observed_at}</p><p>{text("科学 revision", "Scientific revision")}: {value.scientific_revision ?? text("未知", "unknown")}</p>
        {value.categories.map((category) => <section key={category.category}><h4>{categoryLabels[category.category][zh ? 0 : 1]}: {category.record_count} ({category.selected ? text("已选择", "selected") : text("未选择", "not selected")})</h4>{category.kind_counts.map((kind) => <p key={kind.kind}>{kind.kind}: {kind.record_count}</p>)}<ul>{category.examples.slice(0, 20).map((example) => <li key={`${example.kind}-${example.source_id}`}>{example.kind} · {example.label} · {example.status}{example.revision !== null && ` · revision ${example.revision}`}</li>)}</ul>{category.examples_truncated && <p>{text("仅显示有界例子。", "Bounded examples only.")}</p>}</section>)}
        <p>{text(`已选择的已知字节：${value.selected_known_file_bytes}；未知大小文件：${value.unknown_size_file_count}`, `Known selected bytes: ${value.selected_known_file_bytes}; unknown size files: ${value.unknown_size_file_count}`)}</p>
        <ul>{value.files.map((file, index) => <li key={index}><b>{file.backend_id === "local" ? text("本地", "Local") : text("已保存 SSH 引用", "Saved SSH reference")}: {file.relative_path ?? text("路径不可公开", "Path withheld")}</b><p>{file.state} · {file.reason}</p><p>{text("字节", "Bytes")}: {file.size_bytes ?? text("未知", "unknown")}</p><p>{file.hash ? `SHA256 (${file.hash.scope}): ${file.hash.sha256}` : text("Hash：未知", "Hash: unknown")}</p></li>)}</ul>
        <h4>{text("需要重新配置", "Reconfiguration required")}</h4><ul>{value.rebind.map((item) => <li key={item.kind}>{item.kind}: {item.count} · {item.message}</li>)}</ul>
        <h4>{text("排除项", "Exclusions")}</h4><ul>{value.exclusions.map((item, index) => <li key={index}>{item}</li>)}</ul>
        <p>{text("观察内容指纹，不是批准或未来导出保证。数据库与逐文件观察不是原子快照。", "Observation fingerprint, not approval or a future export guarantee. Database and per-file observations are not an atomic snapshot.")}</p><code>{value.preview_sha256}</code>
      </section>}
    </div>
  </section></div>;
}
