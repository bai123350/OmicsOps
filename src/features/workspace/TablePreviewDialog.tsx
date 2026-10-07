import { useCallback, useEffect, useLayoutEffect, useRef, useState, type KeyboardEvent } from "react";
import { X } from "lucide-react";
import { previewProjectTable, type WorkspaceFileReference } from "../../composer-file-api";
import type { ProjectTablePreview, TablePreviewErrorCode, TablePreviewTruncation } from "../../types";
import { useWindowEscapeLayer } from "../settings/BrowserSettings";
import type { Locale } from "./copy";
import "./table-preview.css";

const errors: Record<TablePreviewErrorCode, [string, string]> = {
  invalid_path: ["此路径无法预览。", "This path cannot be previewed."],
  unsupported_format: ["仅支持 CSV 和 TSV 文件。", "Only CSV and TSV files are supported."],
  invalid_utf8: ["文件不是有效的 UTF-8 文本。", "The file is not valid UTF-8 text."],
  malformed_table: ["表格格式无效。", "The table format is invalid."],
  column_limit: ["表格超过 64 列上限。", "The table exceeds the 64-column limit."],
  source_changed: ["读取期间文件发生变化，请重试。", "The file changed during reading. Please retry."],
  read_failed: ["无法读取表格，请重试。", "Could not read the table. Please retry."],
  timeout: ["表格预览超时，请重试。", "The table preview timed out. Please retry."],
  unavailable_backend: ["文件来源不可用，请检查连接后重试。", "The file source is unavailable. Check the connection and retry."],
};
const truncations: Record<TablePreviewTruncation, [string, string]> = {
  byte_limit: ["字节上限：只读取前 256 KiB", "Byte limit: only the first 256 KiB read"],
  row_limit: ["行数上限：最多显示 200 条数据记录", "Row limit: at most 200 data records displayed"],
  cell_limit: ["单元格显示截断：最多 4 KiB UTF-8", "Cell display truncated: at most 4 KiB UTF-8"],
  incomplete_record: ["不完整记录：读取边界处的记录未显示", "Incomplete record: the record at the read boundary is omitted"],
  incomplete_utf8: ["不完整 UTF-8：读取边界处的半字符未显示", "Incomplete UTF-8: the partial character at the read boundary is omitted"],
};

export function TablePreviewDialog({ reference, locale, onClose }: { reference: WorkspaceFileReference; locale: Locale; onClose: () => void }) {
  const zh = locale === "zh-CN";
  const identity = JSON.stringify([reference.project_id, reference.backend_id, reference.relative_path]);
  const identityRef = useRef(identity);
  identityRef.current = identity;
  const generationRef = useRef(0);
  const [retry, setRetry] = useState(0);
  const [state, setState] = useState<{ identity: string; preview?: ProjectTablePreview; error?: TablePreviewErrorCode | "unknown" }>({ identity: "" });
  const dialogRef = useRef<HTMLElement>(null);
  const closeRef = useRef<HTMLButtonElement>(null);
  useLayoutEffect(() => {
    const previous = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    const dialog = dialogRef.current;
    closeRef.current?.focus();
    return () => {
      generationRef.current += 1;
      if (previous?.isConnected && (dialog?.contains(document.activeElement) || document.activeElement === document.body)) previous.focus();
    };
  }, []);
  useWindowEscapeLayer(true, useCallback(onClose, [onClose]));
  useEffect(() => {
    const generation = ++generationRef.current;
    const current = () => generationRef.current === generation && identityRef.current === identity;
    setState({ identity });
    void previewProjectTable(reference).then((preview) => {
      if (!current()) return;
      if (!preview || preview.project_id !== reference.project_id || preview.backend_id !== reference.backend_id || preview.relative_path !== reference.relative_path) throw new Error("source mismatch");
      setState({ identity, preview });
    }).catch((error: unknown) => {
      if (!current()) return;
      // Native failures are fixed string codes. Never render arbitrary host errors.
      const code = typeof error === "string" && Object.hasOwn(errors, error) ? error as TablePreviewErrorCode : "unknown";
      setState({ identity, error: code });
    });
    return () => { generationRef.current += 1; };
  }, [identity, reference.project_id, reference.backend_id, reference.relative_path, retry]);
  const preview = state.identity === identity ? state.preview : undefined;
  const error = state.identity === identity ? state.error : undefined;
  const loading = !preview && !error;
  function trapFocus(event: KeyboardEvent<HTMLElement>) {
    if (event.key !== "Tab") return;
    const elements = Array.from(dialogRef.current?.querySelectorAll<HTMLElement>('button:not([disabled]), [tabindex="0"]') ?? []).filter((element) => !element.hidden && window.getComputedStyle(element).display !== "none" && window.getComputedStyle(element).visibility !== "hidden");
    if (!elements.length) { event.preventDefault(); return; }
    const index = elements.indexOf(document.activeElement as HTMLElement);
    const next = index < 0 ? (event.shiftKey ? elements.length - 1 : 0) : (index + (event.shiftKey ? -1 : 1) + elements.length) % elements.length;
    event.preventDefault(); elements[next].focus();
  }
  return <div className="table-preview-backdrop">
    <section ref={dialogRef} className="table-preview-dialog" role="dialog" aria-modal="true" aria-labelledby="table-preview-title" aria-describedby="table-preview-description" aria-busy={loading || undefined} onKeyDown={trapFocus}>
      <header><div><h2 id="table-preview-title">{zh ? "预览表格" : "Preview table"}</h2><p id="table-preview-description">{zh ? "只读 CSV/TSV；首行作为表头。最多读取 256 KiB，显示 200 条数据记录、64 列，单元格最多 4 KiB UTF-8。" : "Read-only CSV/TSV; the first record is the header. Reads at most 256 KiB, displays 200 data records and 64 columns, with at most 4 KiB UTF-8 per cell."}</p></div><button ref={closeRef} type="button" aria-label={zh ? "关闭表格预览" : "Close table preview"} onClick={onClose}><X size={18} /></button></header>
      <div className="table-preview-source"><b>{reference.backend_id === "local" ? (zh ? "本地" : "Local") : "SSH"}</b><code>{reference.relative_path}</code><small>{zh ? "项目" : "Project"}: {reference.project_id} · {reference.backend_id}</small></div>
      {loading && <p role="status">{zh ? "正在加载表格预览" : "Loading table preview"}</p>}
      {error && <div role="alert" className="table-preview-error"><p>{error === "unknown" ? (zh ? "无法加载表格预览，请重试。" : "Could not load the table preview. Please retry.") : errors[error][zh ? 0 : 1]}</p><button type="button" aria-label={zh ? "重试预览" : "Retry preview"} onClick={() => { closeRef.current?.focus(); setRetry((value) => value + 1); }}>{zh ? "重试" : "Retry"}</button></div>}
      {preview && <>
        <dl className="table-preview-accounting">
          <div><dt>{zh ? "源文件大小" : "Source size"}</dt><dd aria-label={zh ? "源文件大小" : "Source size"}>{preview.size_bytes === null ? (zh ? "未知" : "Unknown") : `${preview.size_bytes} ${zh ? "字节" : "bytes"}`}</dd></div>
          <div><dt>{zh ? "读取字节" : "Read bytes"}</dt><dd>{preview.read_bytes}</dd></div>
          <div><dt>{zh ? "解析字节" : "Parsed bytes"}</dt><dd>{preview.parsed_bytes}</dd></div>
          <div><dt>{zh ? "已显示行数：" : "Displayed rows: "}{preview.displayed_rows}</dt><dd>{preview.source_stability === "checked" ? (zh ? "稳定性已检查" : "Stability checked") : (zh ? "稳定性未知" : "Stability unknown")}</dd></div>
          <div className="table-preview-hash"><dt>SHA-256 · {preview.hash_scope === "full" ? (zh ? "完整源文件" : "Full source") : (zh ? "仅前缀" : "Prefix only")}</dt><dd><code>{preview.sha256}</code><small>{zh ? "哈希覆盖原始读取字节，并非显示的单元格。" : "The hash covers raw bytes read, not displayed cells."}</small></dd></div>
        </dl>
        {preview.truncation_reasons.length > 0 && <ul className="table-preview-limits" aria-label={zh ? "预览截断原因" : "Preview truncation reasons"}>{preview.truncation_reasons.map((reason) => <li key={reason}>{truncations[reason][zh ? 0 : 1]}</li>)}</ul>}
        {preview.headers.length === 0 ? <p>{zh ? "空表格" : "Empty table"}</p> : <><div className="table-preview-scroll" tabIndex={0} role="region" aria-label={zh ? "只读表格" : "Read-only table"}><table><thead><tr>{preview.headers.map((header, index) => <th scope="col" key={index}>{header}</th>)}</tr></thead><tbody>{preview.rows.map((row, index) => <tr key={index}>{row.map((cell, column) => <td key={column}>{cell}</td>)}</tr>)}</tbody></table></div>{preview.rows.length === 0 && <p>{zh ? "无数据行" : "No data rows"}</p>}</>}
      </>}
    </section>
  </div>;
}
