import { useState } from "react";
import { Copy, FileText, LayoutGrid, List } from "lucide-react";
import type { ComputeBackendAvailabilityV4, ProjectArtifact } from "../../types";
import type { Locale } from "./copy";
import { isSidebarPreviewImage } from "./sidebarData";
import type { collectProvenance, DelegatedTask, NotebookCell } from "./sidebarData";

export function SidebarEmpty({ title, children }: { title: string; children: string }) {
  return <div className="sidebar-empty"><FileText size={25} /><b>{title}</b><p>{children}</p></div>;
}

export function CodeNotebook({ cells, locale }: { cells: NotebookCell[]; locale: Locale }) {
  const zh = locale === "zh-CN";
  const [copyError, setCopyError] = useState("");
  const status: Record<NotebookCell["status"], string> = zh
    ? { source: "生成的代码 · 未执行", requested: "请求已记录 · 尚无结果", returned: "工具已返回", failed: "工具失败", uncertain: "派发结果不确定", dispatched: "后台任务已派发 · 尚未验证完成" }
    : { source: "Generated source · not executed", requested: "Request recorded · no result", returned: "Tool returned", failed: "Tool failed", uncertain: "Dispatch uncertain", dispatched: "Background job dispatched · completion unverified" };
  if (!cells.length) return <SidebarEmpty title={zh ? "暂无代码单元" : "No notebook cells"}>{zh ? "对话中的代码块与 Python、R、Shell 调用会汇集在这里。" : "Code blocks and Python, R, and shell calls from this conversation appear here."}</SidebarEmpty>;
  return <div className="sidebar-code-cells">{copyError && <p role="alert">{copyError}</p>}{cells.map((cell, index) => <section className="sidebar-code-cell" key={cell.id}>
    <header><span>[{index}]</span><b>{cell.language}</b><small>{cell.origin}</small><button aria-label={zh ? `复制代码 ${index}` : `Copy code ${index}`} onClick={async () => { setCopyError(""); try { await navigator.clipboard.writeText(cell.source); } catch { setCopyError(zh ? "复制失败，请手动选择代码。" : "Copy failed. Select the code manually."); } }}><Copy size={14} /></button></header>
    <pre><code>{cell.source}</code></pre><div className={`sidebar-cell-status ${cell.status}`}>{status[cell.status]}</div>
    {cell.output && <details open={cell.status === "failed"}><summary>{zh ? "输出" : "Output"}</summary><pre>{cell.output}</pre></details>}
  </section>)}</div>;
}

export function DelegatedAgents({ tasks, locale }: { tasks: DelegatedTask[]; locale: Locale }) {
  const zh = locale === "zh-CN";
  if (!tasks.length) return <SidebarEmpty title={zh ? "暂无委派任务" : "No delegated tasks"}>{zh ? "Agent 委派的子任务、依赖和返回结果会显示在这里。" : "Delegated tasks, dependencies, and returned results appear here."}</SidebarEmpty>;
  return <div className="sidebar-records">{tasks.map((task) => <article className="sidebar-data-card" key={task.id}><header><b>{task.nodeId}</b><span>{task.status}</span></header><p>{task.objective}</p><small>Run: {task.runId}</small>{task.dependencies.length > 0 && <small>{zh ? "依赖" : "Dependencies"}: {task.dependencies.join(", ")}</small>}{task.error && <p className="sidebar-error">{task.error}</p>}{task.output && <details><summary>{zh ? "返回结果" : "Result"}</summary><pre>{task.output}</pre></details>}</article>)}</div>;
}

export function ProvenancePanel({ rows, locale }: { rows: ReturnType<typeof collectProvenance>; locale: Locale }) {
  const zh = locale === "zh-CN";
  const [query, setQuery] = useState("");
  if (!rows.length) return <SidebarEmpty title={zh ? "暂无来源记录" : "No provenance records"}>{zh ? "工具返回后可在这里核查运行、调用、来源和原始返回内容。" : "Inspect runs, calls, sources, and original tool results here after tools return."}</SidebarEmpty>;
  const filtered = rows.filter((row) => `${row.toolId} ${row.runId} ${row.sources.join(" ")}`.toLowerCase().includes(query.toLowerCase()));
  return <div className="sidebar-records"><input className="sidebar-filter" aria-label={zh ? "筛选来源记录" : "Filter provenance"} placeholder={zh ? "工具、运行或来源…" : "Tool, run, or source…"} value={query} onChange={(e) => setQuery(e.target.value)} />{filtered.length === 0 && <p>{zh ? "无匹配记录" : "No matching records"}</p>}{filtered.map((row) => <article className="sidebar-data-card" key={row.id}><header><b>{row.toolId}</b><span>{row.succeeded ? (zh ? "工具成功" : "Tool succeeded") : (zh ? "工具失败" : "Tool failed")}</span></header><small>{row.occurredAt}</small><small>Run: {row.runId} · Event #{row.sequence}</small><small>Call: {row.callId}</small>{row.reused && <small>{zh ? "复用的工具结果" : "Reused tool result"}</small>}{row.sources.map((source, index) => <code className="sidebar-source" key={index}>{source}</code>)}<details><summary>{zh ? "工具返回文本与事件指纹" : "Tool response text and event hash"}</summary><pre>{row.output}</pre><small>{row.eventHash}</small></details>{row.data != null && <details><summary>{zh ? "结构化工具返回" : "Structured tool response"}</summary><pre>{JSON.stringify(row.data, null, 2)}</pre></details>}</article>)}</div>;
}

export function EnvironmentContexts({ backends, selectedId, environment, locale }: { backends: ComputeBackendAvailabilityV4[]; selectedId: string; environment: string; locale: Locale }) {
  const zh = locale === "zh-CN";
  return <div className="sidebar-records"><h3>{zh ? "执行环境" : "Execution contexts"} <small>{backends.length}</small></h3>{!backends.length && <p>{zh ? "暂无执行环境信息" : "No execution context information"}</p>}{backends.map((backend) => <article className={`sidebar-data-card ${backend.descriptor.backend_id === selectedId ? "selected" : ""}`} key={backend.descriptor.backend_id}><header><b>{backend.descriptor.backend_id}</b><span>{backend.descriptor.backend_id === selectedId ? (zh ? "当前" : "Current") : backend.descriptor.kind}</span></header><small>{backend.descriptor.kind.toUpperCase()} · {backend.descriptor.isolation}</small><small>Python: {backend.python_status} · R: {backend.r_status}</small>{backend.descriptor.backend_id === selectedId && <small>{zh ? "环境" : "Environment"}: {environment}</small>}{backend.reason && <p>{backend.reason}</p>}</article>)}<p>{zh ? "解释器可用不表示科研依赖已安装。" : "Interpreter availability does not verify scientific dependencies."}</p></div>;
}

export function ArtifactCatalog({ artifacts, locale, onSelect }: { artifacts: ProjectArtifact[]; locale: Locale; onSelect: (path: string) => void }) {
  const zh = locale === "zh-CN";
  const [grid, setGrid] = useState(false);
  if (!artifacts.length) return <SidebarEmpty title={zh ? "暂无登记产物" : "No registered artifacts"}>{zh ? "Agent 登记的分析产物将显示在这里。" : "Analysis outputs registered by the Agent appear here."}</SidebarEmpty>;
  return <div className="sidebar-artifacts"><div className="sidebar-view-modes"><button aria-label={zh ? "列表视图" : "List view"} aria-pressed={!grid} onClick={() => setGrid(false)}><List size={15} /></button><button aria-label={zh ? "网格视图" : "Grid view"} aria-pressed={grid} onClick={() => setGrid(true)}><LayoutGrid size={15} /></button></div><div className={grid ? "sidebar-artifact-grid" : "sidebar-artifact-list"}>{artifacts.map((artifact) => <article className="sidebar-data-card" key={artifact.id}><FileText size={20} /><b>{artifact.relative_path}</b><small>{artifact.media_type} · {artifact.size_bytes} B</small><small>{artifact.verified ? (zh ? "已核验" : "Verified") : (zh ? "未核验" : "Unverified")}</small>{artifact.run_id && <small>Run: {artifact.run_id}</small>}<details><summary>{zh ? "来源信息" : "Provenance"}</summary><small>SHA-256 {artifact.sha256}</small>{artifact.remote_path && <small>{artifact.remote_path}</small>}</details>{isSidebarPreviewImage(artifact.relative_path) && /^image\//.test(artifact.media_type) && <button onClick={() => onSelect(artifact.relative_path)}>{zh ? "预览" : "Preview"}</button>}</article>)}</div></div>;
}
