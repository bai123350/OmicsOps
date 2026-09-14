import { useLayoutEffect, useRef, useState } from "react";
import { useWindowEscapeLayer } from "../settings/BrowserSettings";
import type { Locale } from "./copy";
import "./context-usage.css";

/** Presentation values supplied by the scoped host snapshot, never inferred from text length. */
export interface ContextUsageView {
  breakdown?: Array<{ category: string; bytes?: number | null; tokens?: number | null; estimated: boolean }> | null;
  contextTokens?: number | null;
  contextLimit?: number | null;
  limitSource: "exact_catalog" | "configured_bound" | "unknown";
  estimated: boolean;
  inputTokens?: number | null;
  outputTokens?: number | null;
  reasoningTokens?: number | null;
  cacheReadTokens?: number | null;
  cacheCreationTokens?: number | null;
  observedInput?: number | null;
  observedOutput?: number | null;
  incompleteAttempts: number;
  serializedRequestBytes?: number | null;
  imageBoundTokens?: number | null;
}
export function ContextUsagePanel({ value, locale, onClose, onNewConversation, onCompact, compactDisabled = true, error = false }: {
  error?: boolean; value: ContextUsageView | null; locale: Locale; onClose: () => void;
  onNewConversation?: () => void; onCompact?: () => void; compactDisabled?: boolean;
}) {
  const zh = locale === "zh-CN";
  const [docked, setDocked] = useState(true);
  const [position, setPosition] = useState({ x: 20, y: 80 });
  const [details, setDetails] = useState(false);
  const panel = useRef<HTMLElement>(null);
  const close = useRef<HTMLButtonElement>(null);
  const drag = useRef<{ id: number; x: number; y: number; left: number; top: number } | null>(null);
  useLayoutEffect(() => {
    if (docked) return;
    const keepVisible = () => {
      const box = panel.current?.getBoundingClientRect();
      if (!box) return;
      setPosition((previous) => {
        const x = Math.max(0, Math.min(previous.x, window.innerWidth - box.width));
        const y = Math.max(0, Math.min(previous.y, window.innerHeight - box.height));
        return x === previous.x && y === previous.y ? previous : { x, y };
      });
    };
    keepVisible();
    window.addEventListener("resize", keepVisible);
    const observer = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(keepVisible);
    if (panel.current) observer?.observe(panel.current);
    return () => { window.removeEventListener("resize", keepVisible); observer?.disconnect(); };
  }, [docked]);
  useWindowEscapeLayer(true, onClose);
  useLayoutEffect(() => {
    const previous = document.activeElement;
    close.current?.focus();
    return () => { if (previous instanceof HTMLElement && previous.isConnected) previous.focus(); };
  }, []);
  const number = (n?: number | null) => n == null ? (zh ? "未提供" : "Not reported") : n.toLocaleString(locale);
  const percent = value?.contextTokens != null && value.contextLimit != null && value.contextLimit > 0
    ? value.contextTokens / value.contextLimit * 100 : null;
  const source = value?.limitSource === "exact_catalog" ? (zh ? "精确模型目录" : "Exact model catalog") : value?.limitSource === "configured_bound" ? (zh ? "本地配置上限" : "Locally configured bound") : (zh ? "上限未知" : "Unknown limit");
  return <section ref={panel} role="dialog" aria-label={zh ? "上下文用量" : "Context usage"} className={`context-usage-panel ${docked ? "is-docked" : "is-floating"}`} style={docked ? undefined : { left: position.x, top: position.y }}>
    <header onPointerDown={(event) => {
      if (docked || event.button !== 0 || (event.target as HTMLElement).closest("button")) return;
      const box = panel.current!.getBoundingClientRect();
      drag.current = { id: event.pointerId, x: event.clientX, y: event.clientY, left: box.left, top: box.top };
      event.currentTarget.setPointerCapture(event.pointerId);
    }} onPointerMove={(event) => {
      const start = drag.current; if (!start || start.id !== event.pointerId) return;
      const box = panel.current!.getBoundingClientRect();
      setPosition({ x: Math.max(0, Math.min(window.innerWidth - box.width, start.left + event.clientX - start.x)), y: Math.max(0, Math.min(window.innerHeight - 48, start.top + event.clientY - start.y)) });
    }} onPointerUp={() => { drag.current = null; }} onPointerCancel={() => { drag.current = null; }}>
      <strong>{zh ? "上下文用量" : "Context usage"}</strong>
      <button type="button" onClick={() => setDocked(!docked)}>{docked ? (zh ? "浮动" : "Float") : (zh ? "停靠" : "Dock")}</button>
      <button ref={close} type="button" aria-label={zh ? "关闭用量详情" : "Close usage details"} onClick={onClose}>×</button>
    </header>
    <div className="context-usage-content">
      {error && <p role="alert">{zh ? "用量暂时无法刷新，显示上次成功读取的数据。" : "Usage could not be refreshed; any displayed values are from the last successful read."}</p>}
      <p>{zh ? "最近请求的输入上下文" : "Input context of the latest request"}</p>
      <p className="context-usage-total">{value?.estimated && (zh ? "估算 " : "Estimated ")}{number(value?.contextTokens)} / {number(value?.contextLimit)} tokens</p>
      {percent !== null && <><progress max={100} value={Math.min(100, Math.max(0, percent))} aria-label={zh ? "上下文占用比例" : "Context window used"} /><p>{percent.toFixed(1)}%</p></>}
      <p>{source}</p>
      <h3>{zh ? "最近请求的提供商报告" : "Latest provider report"}</h3>
      <dl><dt>{zh ? "输入 tokens" : "Input tokens"}</dt><dd>{number(value?.inputTokens)}</dd><dt>{zh ? "输出 tokens" : "Output tokens"}</dt><dd>{number(value?.outputTokens)}</dd></dl>
      <h3>{zh ? "已观测累计用量" : "Observed consumption"}</h3>
      <dl><dt>{zh ? "输入 tokens" : "Input tokens"}</dt><dd>{number(value?.observedInput)}</dd><dt>{zh ? "输出 tokens" : "Output tokens"}</dt><dd>{number(value?.observedOutput)}</dd></dl>
      {Boolean(value?.incompleteAttempts) && <p role="status">{zh ? `至少 ${value!.incompleteAttempts} 次请求的用量不完整；累计值不是完整总量。` : `At least ${value!.incompleteAttempts} attempts have incomplete usage; observed values are not complete totals.`}</p>}
      <button type="button" aria-expanded={details} onClick={() => setDetails(!details)}>{zh ? "用量分项与请求预算" : "Usage facets and request budget"}</button>
      {details && <dl><dt>{zh ? "推理 tokens" : "Reasoning tokens"}</dt><dd>{number(value?.reasoningTokens)}</dd><dt>{zh ? "缓存读取 tokens" : "Cache read tokens"}</dt><dd>{number(value?.cacheReadTokens)}</dd><dt>{zh ? "缓存写入 tokens" : "Cache creation tokens"}</dt><dd>{number(value?.cacheCreationTokens)}</dd><dt>{zh ? "序列化请求 bytes" : "Serialized request bytes"}</dt><dd>{number(value?.serializedRequestBytes)}</dd><dt>{zh ? "图像 tokens 保守上界" : "Conservative image token bound"}</dt><dd>{number(value?.imageBoundTokens)}</dd></dl>}
      {details && Boolean(value?.breakdown?.length) && <dl>{value!.breakdown!.map((row, index) => <div className="context-usage-breakdown" key={`${row.category}:${index}`}><dt>{row.category}</dt><dd>{row.estimated ? "≈ " : ""}{row.tokens != null ? `${number(row.tokens)} tokens` : row.bytes != null ? `${number(row.bytes)} bytes` : number(null)}</dd></div>)}</dl>}
      <p className="context-usage-note">{zh ? "请求字节数与图像预算用于容量检查，不是提供商实际用量。缓存分项不重复计入输入或输出。" : "Request bytes and image bounds are admission estimates, not provider usage. Cache facets are not added again to input or output."}</p>
      <footer><button type="button" disabled={!onCompact || compactDisabled} onClick={onCompact}>{zh ? "压缩上下文" : "Compact context"}</button><button type="button" disabled={!onNewConversation} onClick={onNewConversation}>{zh ? "新建会话" : "New conversation"}</button></footer>
    </div>
  </section>;
}
