import { useEffect, useMemo, useRef, useState } from "react";

import { settingsUsageConversations, settingsUsagePage } from "../../usage-settings-api";
import type {
  ObservedCounterV4, UsageAggregatePage, UsageConversationRow, UsageDay, UsageFilter,
  UsageGroup, UsageTool, UsageTotalsV4, WorkspaceProject,
} from "../../types";
import type { Locale } from "../workspace/copy";
import "./UsageSettings.css";

type Range = "7" | "30" | "90" | "all";

export function UsageSettings({
  locale,
  projects,
  selectedProjectId,
  onOpenConversation,
}: {
  locale: Locale;
  projects: WorkspaceProject[];
  selectedProjectId?: string | null;
  onOpenConversation?: (projectId: string, conversationId: string) => void | Promise<void>;
}) {
  const zh = locale === "zh-CN";
  const [projectId, setProjectId] = useState(selectedProjectId ?? "");
  const [range, setRange] = useState<Range>("30");
  const [refreshVersion, setRefreshVersion] = useState(0);
  const filter = useMemo(() => usageFilter(projectId, range), [projectId, range, refreshVersion]);
  const generation = useRef(0);
  const aggregateOperation = useRef<object | null>(null);
  const conversationOperation = useRef<object | null>(null);
  const aggregateSeen = useRef(new Set<string>());
  const conversationSeen = useRef(new Set<string>());
  const aggregateRef = useRef<UsageAggregatePage | null>(null);
  const [aggregate, setAggregate] = useState<UsageAggregatePage | null>(null);
  const [aggregateBusy, setAggregateBusy] = useState(false);
  const [aggregateError, setAggregateError] = useState(false);
  const [aggregateNeedsReset, setAggregateNeedsReset] = useState(false);
  const [aggregateRetryCursor, setAggregateRetryCursor] = useState<string | null>(null);
  const [conversations, setConversations] = useState<UsageConversationRow[]>([]);
  const [conversationCursor, setConversationCursor] = useState<string | null>(null);
  const [conversationBusy, setConversationBusy] = useState(false);
  const [conversationError, setConversationError] = useState(false);
  const [conversationNeedsReset, setConversationNeedsReset] = useState(false);
  const [conversationOpenBusy, setConversationOpenBusy] = useState(false);
  const [conversationOpenError, setConversationOpenError] = useState(false);

  async function loadAggregate(startCursor: string | null, requestGeneration: number) {
    if (aggregateOperation.current) return;
    const operation = {};
    aggregateOperation.current = operation;
    setAggregateBusy(true);
    setAggregateError(false);
    let cursor = startCursor;
    try {
      while (requestGeneration === generation.current) {
        const requestKey = cursor ?? "__first__";
        if (aggregateSeen.current.has(requestKey)) throw new Error("repeated cursor");
        const page = await settingsUsagePage(filter, cursor);
        if (requestGeneration !== generation.current) return;
        aggregateSeen.current.add(requestKey);
        const merged = aggregateRef.current ? mergeAggregatePages(aggregateRef.current, page) : page;
        aggregateRef.current = merged;
        setAggregate(merged);
        setAggregateRetryCursor(page.next_cursor ?? null);
        if (!page.next_cursor) return;
        if (aggregateSeen.current.has(page.next_cursor)) throw new Error("repeated cursor");
        cursor = page.next_cursor;
      }
    } catch (reason) {
      if (requestGeneration === generation.current) {
        setAggregateRetryCursor(cursor);
        setAggregateNeedsReset(String(reason).includes("refresh required") || String(reason).includes("snapshot changed"));
        setAggregateError(true);
      }
    } finally {
      if (aggregateOperation.current === operation) {
        aggregateOperation.current = null;
        if (requestGeneration === generation.current) setAggregateBusy(false);
      }
    }
  }

  async function loadConversations(cursor: string | null, append: boolean, requestGeneration: number) {
    if (conversationOperation.current) return;
    const operation = {};
    conversationOperation.current = operation;
    setConversationBusy(true);
    setConversationError(false);
    try {
      const requestKey = cursor ?? "__first__";
      if (conversationSeen.current.has(requestKey)) throw new Error("repeated cursor");
      const page = await settingsUsageConversations(filter, cursor);
      if (requestGeneration !== generation.current) return;
      conversationSeen.current.add(requestKey);
      setConversations((current) => append ? [...current, ...page.items] : page.items);
      setConversationCursor(page.next_cursor ?? null);
    } catch (reason) {
      if (requestGeneration === generation.current) {
        setConversationNeedsReset(String(reason).includes("refresh required"));
        setConversationError(true);
      }
    } finally {
      if (conversationOperation.current === operation) {
        conversationOperation.current = null;
        if (requestGeneration === generation.current) setConversationBusy(false);
      }
    }
  }

  function retryAggregate() {
    if (aggregateNeedsReset) {
      setRefreshVersion((current) => current + 1);
      return;
    }
    const cursor = aggregateRetryCursor;
    const requestKey = cursor ?? "__first__";
    if (aggregateSeen.current.has(requestKey)) {
      aggregateSeen.current = new Set();
      aggregateRef.current = null;
      setAggregate(null);
      setAggregateRetryCursor(null);
      void loadAggregate(null, generation.current);
      return;
    }
    void loadAggregate(cursor, generation.current);
  }

  function retryConversations() {
    if (conversationNeedsReset) {
      setRefreshVersion((current) => current + 1);
      return;
    }
    const requestKey = conversationCursor ?? "__first__";
    if (conversationSeen.current.has(requestKey)) {
      conversationSeen.current = new Set();
      setConversations([]);
      setConversationCursor(null);
      void loadConversations(null, false, generation.current);
      return;
    }
    void loadConversations(conversationCursor, conversations.length > 0, generation.current);
  }

  async function openConversation(row: UsageConversationRow) {
    if (!onOpenConversation || conversationOpenBusy) return;
    setConversationOpenBusy(true);
    setConversationOpenError(false);
    try {
      await onOpenConversation(row.project_id, row.conversation_id);
    } catch {
      setConversationOpenError(true);
    } finally {
      setConversationOpenBusy(false);
    }
  }

  useEffect(() => {
    setProjectId(selectedProjectId ?? "");
  }, [selectedProjectId]);

  useEffect(() => {
    const requestGeneration = ++generation.current;
    aggregateOperation.current = null;
    conversationOperation.current = null;
    aggregateSeen.current = new Set();
    conversationSeen.current = new Set();
    aggregateRef.current = null;
    setAggregate(null);
    setAggregateError(false);
    setAggregateNeedsReset(false);
    setAggregateRetryCursor(null);
    setConversations([]);
    setConversationCursor(null);
    setConversationError(false);
    setConversationNeedsReset(false);
    setConversationOpenError(false);
    void loadAggregate(null, requestGeneration);
    void loadConversations(null, false, requestGeneration);
    return () => { generation.current += 1; };
  }, [filter.project_id, filter.from, filter.until, refreshVersion]);

  const weeks = aggregate ? weeklyActivity(aggregate.days) : [];
  const aggregateIsSubtotal = aggregateBusy || aggregateError || Boolean(aggregate?.next_cursor);

  return <main className="usage-settings">
    <div className="settings-heading">
      <h3>{zh ? "用量" : "Usage"}</h3>
      <p>{zh ? "统计 Agent V4 的持久化模型用量观察与实际工具派发。日期和周均使用 UTC。" : "Review durable Agent V4 model-usage observations and actual tool dispatches. Dates and weeks use UTC."}</p>
    </div>
    <section className="usage-filters" aria-label={zh ? "用量筛选" : "Usage filters"}>
      <label>{zh ? "项目" : "Project"}<select aria-label={zh ? "项目" : "Project"} value={projectId} onChange={(event) => setProjectId(event.target.value)}><option value="">{zh ? "全部项目" : "All projects"}</option>{projects.map((project) => <option value={project.id} key={project.id}>{project.name}</option>)}</select></label>
      <label>{zh ? "UTC 范围" : "UTC range"}<select aria-label={zh ? "UTC 范围" : "UTC range"} value={range} onChange={(event) => setRange(event.target.value as Range)}><option value="7">{zh ? "最近 7 天" : "Past 7 days"}</option><option value="30">{zh ? "最近 30 天" : "Past 30 days"}</option><option value="90">{zh ? "最近 90 天" : "Past 90 days"}</option><option value="all">{zh ? "全部时间" : "All time"}</option></select></label>
      {aggregateBusy && <span role="status">{zh ? `统计中 · 已扫描 ${aggregate?.scanned_runs ?? 0} 个运行` : `Calculating · ${aggregate?.scanned_runs ?? 0} runs scanned`}</span>}
      <button type="button" disabled={aggregateBusy || conversationBusy} onClick={() => setRefreshVersion((current) => current + 1)}>{zh ? "重新统计" : "Recalculate"}</button>
    </section>

    {aggregateError && <section className="usage-message error" role="alert"><span>{aggregateNeedsReset ? (zh ? "用量历史已变化。请重新统计固定快照。" : "Usage history changed. Recalculate from a fresh snapshot.") : (zh ? "用量读取中断。已完成的页仍保留，可从中断处重试。" : "Usage loading stopped. Completed pages are retained; retry from the interrupted page.")}</span><button type="button" disabled={aggregateBusy} onClick={retryAggregate}>{aggregateNeedsReset ? (zh ? "重新统计" : "Recalculate") : (zh ? "重试" : "Retry")}</button></section>}
    {!aggregate && !aggregateBusy && !aggregateError && <p className="usage-message">{zh ? "没有可显示的 Agent V4 用量。" : "No Agent V4 usage is available."}</p>}

    {aggregate && <>
      {aggregateBusy && <p className="usage-message warning" role="status">{zh ? "正在合并固定快照的后续页；当前卡片是已观察小计。" : "More fixed-snapshot pages are being merged; current cards are observed subtotals."}</p>}
      {!aggregateBusy && (aggregate.completeness !== "complete" || aggregate.omitted_runs > 0 || aggregate.unattributed_events > 0) && <p className="usage-message warning" role="status">{zh ? `当前是已观察小计：省略运行 ${aggregate.omitted_runs}，无法归属事件 ${aggregate.unattributed_events}。` : `Observed subtotal: ${aggregate.omitted_runs} runs omitted and ${aggregate.unattributed_events} events unattributed.`}</p>}
      <section className="usage-token-grid" aria-label={zh ? "Token 汇总" : "Token totals"}>
        <CounterCard label={zh ? "输入" : "Input"} counter={aggregate.totals.input_tokens} zh={zh} forceSubtotal={aggregateIsSubtotal} />
        <CounterCard label={zh ? "输出" : "Output"} counter={aggregate.totals.output_tokens} zh={zh} forceSubtotal={aggregateIsSubtotal} />
        <CounterCard label={zh ? "推理" : "Reasoning"} counter={aggregate.totals.reasoning_tokens} zh={zh} forceSubtotal={aggregateIsSubtotal} />
        <CounterCard label={zh ? "缓存读取" : "Cache read"} counter={aggregate.totals.cache_read_input_tokens} zh={zh} forceSubtotal={aggregateIsSubtotal} />
        <CounterCard label={zh ? "缓存创建" : "Cache creation"} counter={aggregate.totals.cache_creation_input_tokens} zh={zh} forceSubtotal={aggregateIsSubtotal} />
        <CounterCard label={zh ? "提供方报告总量" : "Provider reported total"} counter={aggregate.totals.reported_total_tokens} zh={zh} forceSubtotal={aggregateIsSubtotal} />
      </section>
      <p className="usage-scope">{zh ? `${aggregate.scanned_runs} 个运行已扫描 · ${aggregate.totals.observed_attempts} 次模型尝试 · 快照 ${aggregate.snapshot_at}` : `${aggregate.scanned_runs} runs scanned · ${aggregate.totals.observed_attempts} model attempts · snapshot ${aggregate.snapshot_at}`}</p>

      <UsageGroups title={zh ? "按项目" : "By project"} groups={aggregate.projects} zh={zh} onSelect={(key) => setProjectId(key)} />
      <UsageGroups title={zh ? "按历史模型身份" : "By historical model identity"} groups={aggregate.models} zh={zh} />

      <section className="usage-section"><h4>{zh ? "UTC 日活动" : "UTC daily activity"}</h4>{aggregate.days.length ? <table><thead><tr><th>{zh ? "日期" : "Date"}</th><th>{zh ? "模型尝试" : "Attempts"}</th><th>{zh ? "工具派发" : "Tool dispatches"}</th></tr></thead><tbody>{aggregate.days.map((day) => <tr key={day.date}><td>{day.date}</td><td>{day.attempts}</td><td>{day.tools}</td></tr>)}</tbody></table> : <p>{zh ? "范围内没有活动。" : "No activity in this range."}</p>}</section>
      <section className="usage-section"><h4>{zh ? "UTC 周活动" : "UTC weekly activity"}</h4>{weeks.length ? <ul className="usage-week-list">{weeks.map((week) => <li key={week.week}><span>{week.week}</span><b>{week.attempts} {zh ? "次尝试" : "attempts"}</b><b>{week.tools} {zh ? "次派发" : "dispatches"}</b></li>)}</ul> : <p>{zh ? "范围内没有周活动。" : "No weekly activity in this range."}</p>}</section>
      <section className="usage-section"><h4>{zh ? "工具派发排行" : "Tool dispatch ranking"}</h4><p>{zh ? "只统计 ToolDispatchStarted；Skills 提示上下文不会伪装成工具调用。" : "Counts ToolDispatchStarted only. Skill prompt context is not presented as a tool call."}</p>{aggregate.tools.length ? <table><thead><tr><th>{zh ? "工具" : "Tool"}</th><th>{zh ? "派发" : "Dispatched"}</th><th>{zh ? "成功" : "Succeeded"}</th><th>{zh ? "失败" : "Failed"}</th><th>{zh ? "不确定" : "Uncertain"}</th></tr></thead><tbody>{[...aggregate.tools].sort((a, b) => b.dispatched - a.dispatched || a.tool_id.localeCompare(b.tool_id)).map((tool) => <tr key={tool.tool_id}><td><code>{tool.tool_id}</code></td><td>{tool.dispatched}</td><td>{tool.succeeded}</td><td>{tool.failed}</td><td>{tool.uncertain}</td></tr>)}</tbody></table> : <p>{zh ? "范围内没有工具派发。" : "No tool dispatches in this range."}</p>}</section>
    </>}

    <section className="usage-section usage-conversations"><h4>{zh ? "会话" : "Conversations"}</h4>{conversationError && <div className="usage-message error" role="alert"><span>{conversationNeedsReset ? (zh ? "会话快照已变化，请重新统计。" : "The conversation snapshot changed; recalculate.") : (zh ? "无法读取会话用量。" : "Could not load conversation usage.")}</span><button type="button" disabled={conversationBusy} onClick={retryConversations}>{conversationNeedsReset ? (zh ? "重新统计" : "Recalculate") : (zh ? "重试" : "Retry")}</button></div>}{conversationOpenError && <p className="usage-message error" role="alert">{zh ? "无法打开此会话；它可能已被删除。" : "Could not open this conversation; it may have been deleted."}</p>}{conversations.length > 0 && <table><thead><tr><th>{zh ? "会话" : "Conversation"}</th><th>{zh ? "最近活动" : "Latest activity"}</th><th>{zh ? "输入" : "Input"}</th><th /></tr></thead><tbody>{conversations.map((row) => <tr key={`${row.project_id}:${row.conversation_id}`}><td>{row.label}{row.incomplete && <small>{zh ? "已观察小计" : "Observed subtotal"}</small>}</td><td>{row.latest_activity}</td><td>{counterText(row.totals.input_tokens, zh)}</td><td><button type="button" disabled={!onOpenConversation || conversationOpenBusy} onClick={() => void openConversation(row)}>{conversationOpenBusy ? (zh ? "打开中…" : "Opening…") : (zh ? "打开" : "Open")}</button></td></tr>)}</tbody></table>}{!conversationBusy && !conversationError && conversations.length === 0 && <p>{zh ? "没有会话用量。" : "No conversation usage."}</p>}{conversationCursor && <button className="usage-more" type="button" disabled={conversationBusy} onClick={() => void loadConversations(conversationCursor, true, generation.current)}>{conversationBusy ? (zh ? "读取中…" : "Loading…") : (zh ? "更多" : "More")}</button>}</section>
  </main>;
}

function CounterCard({ label, counter, zh, forceSubtotal = false }: { label: string; counter: ObservedCounterV4; zh: boolean; forceSubtotal?: boolean }) {
  return <article><small>{label}</small><strong>{counterText(counter, zh, forceSubtotal)}</strong></article>;
}

function counterText(counter: ObservedCounterV4, zh: boolean, forceSubtotal = false) {
  if (counter.known == null) return zh ? "未报告" : "Not reported";
  if (forceSubtotal || counter.incomplete_attempts > 0) return zh ? `${counter.known.toLocaleString()} 已观察小计` : `${counter.known.toLocaleString()} observed subtotal`;
  return counter.known.toLocaleString();
}

function UsageGroups({ title, groups, zh, onSelect }: { title: string; groups: UsageGroup[]; zh: boolean; onSelect?: (key: string) => void }) {
  return <section className="usage-section"><h4>{title}</h4>{groups.length ? <ul className="usage-group-list">{groups.map((group) => <li key={group.key}><span><b>{group.label}</b><small>{counterText(group.totals.input_tokens, zh)} {zh ? "输入" : "input"}</small></span>{onSelect && <button type="button" onClick={() => onSelect(group.key)}>{zh ? "筛选" : "Filter"}</button>}</li>)}</ul> : <p>{zh ? "没有可分组的模型用量。" : "No grouped model usage."}</p>}</section>;
}

function usageFilter(projectId: string, range: Range): UsageFilter {
  if (range === "all") return projectId ? { project_id: projectId } : {};
  const until = new Date();
  const from = new Date(until.getTime() - Number(range) * 24 * 60 * 60 * 1000);
  return { ...(projectId ? { project_id: projectId } : {}), from: from.toISOString(), until: until.toISOString() };
}

function weeklyActivity(days: UsageDay[]) {
  const weeks = new Map<string, { week: string; attempts: number; tools: number }>();
  for (const day of days) {
    const date = new Date(`${day.date}T00:00:00Z`);
    const offset = (date.getUTCDay() + 6) % 7;
    date.setUTCDate(date.getUTCDate() - offset);
    const key = date.toISOString().slice(0, 10);
    const value = weeks.get(key) ?? { week: key, attempts: 0, tools: 0 };
    value.attempts += day.attempts;
    value.tools += day.tools;
    weeks.set(key, value);
  }
  return [...weeks.values()].sort((a, b) => a.week.localeCompare(b.week));
}

function mergeAggregatePages(current: UsageAggregatePage, next: UsageAggregatePage): UsageAggregatePage {
  if (current.snapshot_at !== next.snapshot_at) throw new Error("snapshot changed");
  return {
    totals: mergeTotals(current.totals, next.totals),
    projects: mergeGroups(current.projects, next.projects),
    models: mergeGroups(current.models, next.models),
    days: mergeDays(current.days, next.days),
    tools: mergeTools(current.tools, next.tools),
    next_cursor: next.next_cursor,
    scanned_runs: current.scanned_runs + next.scanned_runs,
    omitted_runs: current.omitted_runs + next.omitted_runs,
    unattributed_events: current.unattributed_events + next.unattributed_events,
    snapshot_at: current.snapshot_at,
    completeness: current.completeness === "complete" && next.completeness === "complete" ? "complete" : "partial",
  };
}

function mergeGroups(current: UsageGroup[], next: UsageGroup[]) {
  const values = new Map(current.map((group) => [group.key, { ...group }]));
  for (const group of next) {
    const existing = values.get(group.key);
    values.set(group.key, existing ? { ...existing, totals: mergeTotals(existing.totals, group.totals) } : group);
  }
  return [...values.values()];
}

function mergeDays(current: UsageDay[], next: UsageDay[]) {
  const values = new Map(current.map((day) => [day.date, { ...day }]));
  for (const day of next) {
    const existing = values.get(day.date);
    values.set(day.date, existing ? { date: day.date, attempts: existing.attempts + day.attempts, tools: existing.tools + day.tools, totals: mergeTotals(existing.totals, day.totals) } : day);
  }
  return [...values.values()].sort((a, b) => a.date.localeCompare(b.date));
}

function mergeTools(current: UsageTool[], next: UsageTool[]) {
  const values = new Map(current.map((tool) => [tool.tool_id, { ...tool }]));
  for (const tool of next) {
    const existing = values.get(tool.tool_id);
    values.set(tool.tool_id, existing ? { tool_id: tool.tool_id, dispatched: existing.dispatched + tool.dispatched, succeeded: existing.succeeded + tool.succeeded, failed: existing.failed + tool.failed, uncertain: existing.uncertain + tool.uncertain } : tool);
  }
  return [...values.values()];
}

function mergeTotals(current: UsageTotalsV4, next: UsageTotalsV4): UsageTotalsV4 {
  const counter = (left: ObservedCounterV4, right: ObservedCounterV4): ObservedCounterV4 => ({
    known: left.known == null ? right.known : right.known == null ? left.known : left.known + right.known,
    incomplete_attempts: left.incomplete_attempts + right.incomplete_attempts,
  });
  return {
    input_tokens: counter(current.input_tokens, next.input_tokens), output_tokens: counter(current.output_tokens, next.output_tokens),
    reasoning_tokens: counter(current.reasoning_tokens, next.reasoning_tokens), cache_read_input_tokens: counter(current.cache_read_input_tokens, next.cache_read_input_tokens),
    cache_creation_input_tokens: counter(current.cache_creation_input_tokens, next.cache_creation_input_tokens), reported_total_tokens: counter(current.reported_total_tokens, next.reported_total_tokens),
    observed_attempts: current.observed_attempts + next.observed_attempts, final_attempts: current.final_attempts + next.final_attempts,
    partial_attempts: current.partial_attempts + next.partial_attempts, interrupted_attempts: current.interrupted_attempts + next.interrupted_attempts,
    unknown_attempts: current.unknown_attempts + next.unknown_attempts,
  };
}
