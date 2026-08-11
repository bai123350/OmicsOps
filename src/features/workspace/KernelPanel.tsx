import { useMemo, useState } from "react";
import { CircleStop, Play, RefreshCcw, Save, SquareTerminal } from "lucide-react";

import type { FormalStepProposal, KernelEvent, KernelLanguage, KernelSession } from "../../types";
import type { Locale } from "./copy";

interface Props {
  locale: Locale;
  sessions: KernelSession[];
  events: KernelEvent[];
  busy?: boolean;
  notice?: string;
  onStart?: (language: KernelLanguage, rebuildSessionId?: string) => Promise<void> | void;
  onExecute?: (sessionId: string, code: string, save: boolean, capturePaths: string[]) => Promise<number | null>;
  onInterrupt?: (sessionId: string) => Promise<void> | void;
  onStop?: (sessionId: string) => Promise<void> | void;
  onPromote?: (sessionId: string, cellIndex: number, name: string) => Promise<FormalStepProposal>;
}

export function KernelPanel({ locale, sessions, events, busy = false, notice, onStart, onExecute, onInterrupt, onStop, onPromote }: Props) {
  const zh = locale === "zh-CN";
  const [selectedId, setSelectedId] = useState<string>("");
  const [code, setCode] = useState("import sys\nprint(sys.version)");
  const [capture, setCapture] = useState("");
  const [saveCell, setSaveCell] = useState(true);
  const [savedCell, setSavedCell] = useState<number | null>(null);
  const [proposal, setProposal] = useState<FormalStepProposal | null>(null);
  const selected = sessions.find((session) => session.id === selectedId) ?? sessions.find((session) => session.state === "running") ?? sessions[0];
  const visibleEvents = useMemo(() => events.filter((event) => event.session_id === selected?.id).slice(-30), [events, selected?.id]);

  async function execute() {
    if (!selected || !onExecute || !code.trim()) return;
    const paths = capture.split(",").map((value) => value.trim()).filter(Boolean);
    setProposal(null);
    setSavedCell(await onExecute(selected.id, code, saveCell, paths));
  }

  async function promote() {
    if (!selected || savedCell === null || !onPromote) return;
    setProposal(await onPromote(selected.id, savedCell, zh ? "探索代码步骤" : "Exploration code step"));
  }

  return <section className="kernel-panel" aria-label={zh ? "远端探索会话" : "Remote exploration sessions"}>
    <header><div><b>{zh ? "Python / R 探索" : "Python / R exploration"}</b><small>{zh ? "远端持久会话；仅保存的代码可固化" : "Persistent remote sessions; only saved code can be promoted"}</small></div><div><button disabled={busy || !onStart} onClick={() => void onStart?.("python")}><Play size={13} />Python</button><button disabled={busy || !onStart} onClick={() => void onStart?.("r")}><Play size={13} />R</button></div></header>
    {sessions.length === 0 ? <p className="kernel-empty">{zh ? "尚无探索会话。项目需先配置可信 SSH 远端。" : "No exploration session yet. Configure a trusted SSH remote first."}</p> : <>
      <div className="kernel-sessions">{sessions.map((session) => <button key={session.id} className={selected?.id === session.id ? "active" : ""} onClick={() => setSelectedId(session.id)}><SquareTerminal size={13} /><span>{session.language.toUpperCase()}</span><i className={`kernel-state ${session.state}`}>{session.state}</i>{session.state === "interrupted" && <span className="kernel-rebuild" role="button" onClick={(event) => { event.stopPropagation(); void onStart?.(session.language, session.id); }}><RefreshCcw size={11} />{zh ? "重建" : "Rebuild"}</span>}</button>)}</div>
      {selected?.state === "running" && <div className="kernel-editor"><textarea aria-label={zh ? "探索代码" : "Exploration code"} value={code} onChange={(event) => setCode(event.target.value)} spellCheck={false} /><input aria-label={zh ? "捕获产物路径" : "Artifact capture paths"} value={capture} onChange={(event) => setCapture(event.target.value)} placeholder={zh ? "捕获产物路径，以逗号分隔（可选）" : "Artifact paths, comma separated (optional)"} /><label><input type="checkbox" checked={saveCell} onChange={(event) => setSaveCell(event.target.checked)} /><Save size={12} />{zh ? "保存代码单元，以便断线重建" : "Save cell for reconnect rebuild"}</label><div className="kernel-actions"><button disabled={busy} onClick={() => void execute()}><Play size={13} />{zh ? "执行" : "Run"}</button><button disabled={busy} onClick={() => void onInterrupt?.(selected.id)}><CircleStop size={13} />{zh ? "中断" : "Interrupt"}</button><button disabled={busy} onClick={() => void onStop?.(selected.id)}>{zh ? "停止" : "Stop"}</button></div></div>}
    </>}
    <div className="kernel-output" aria-live="polite">{visibleEvents.map((item) => <pre key={`${item.request_id}-${item.sequence}`} className={item.event.kind}>{formatEvent(item, zh)}</pre>)}</div>
    {savedCell !== null && <button className="kernel-promote" disabled={busy || !onPromote} onClick={() => void promote()}>{zh ? `固化已保存单元 #${savedCell + 1} 为正式步骤` : `Promote saved cell #${savedCell + 1} to formal step`}</button>}
    {proposal && <div className="kernel-proposal"><b>{proposal.name} v{proposal.version}</b><code>SHA-256 {proposal.code_sha256.slice(0, 16)}…</code><small>{zh ? "已生成提案，仍需进入正式计划审批后才能执行。" : "Proposal created; formal plan approval is still required before execution."}</small></div>}
    {notice && <p className="kernel-notice">{notice}</p>}
  </section>;
}

function formatEvent(item: KernelEvent, zh: boolean) {
  switch (item.event.kind) {
    case "stdout": return item.event.payload;
    case "stderr": return item.event.payload;
    case "failed": return `${zh ? "失败" : "Failed"}: ${item.event.payload.message}`;
    case "artifact": return `${zh ? "产物" : "Artifact"}: ${item.event.payload.relative_path} · SHA-256 ${item.event.payload.sha256.slice(0, 12)}`;
    case "started": return zh ? "执行开始" : "Execution started";
    case "completed": return zh ? "执行完成" : "Execution completed";
    case "stopped": return zh ? "会话已停止" : "Session stopped";
  }
}
