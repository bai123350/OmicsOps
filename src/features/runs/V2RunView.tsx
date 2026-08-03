import { useCallback, useEffect, useState } from "react";

import * as api from "../../tauri-api";
import type { ArtifactRecordV2, EnvironmentLock, RunCheckpointV2, RunEventV2, StepAttempt } from "../../types";

export function V2RunView({ selectedRunId }: { selectedRunId: string }) {
  const [checkpoint, setCheckpoint] = useState<RunCheckpointV2 | null>(null);
  const [events, setEvents] = useState<RunEventV2[]>([]);
  const [attempts, setAttempts] = useState<StepAttempt[]>([]);
  const [artifacts, setArtifacts] = useState<ArtifactRecordV2[]>([]);
  const [environmentLock, setEnvironmentLock] = useState<EnvironmentLock | null>(null);
  const [error, setError] = useState("");
  const [bundlePath, setBundlePath] = useState("");

  const refresh = useCallback(async () => {
    if (!selectedRunId) return;
    try {
      const [runs, nextEvents, nextAttempts, nextArtifacts, nextLock] = await Promise.all([
        api.listRunsV2(), api.listRunEventsV2(selectedRunId), api.listStepAttemptsV2(selectedRunId),
        api.listArtifactsV2(selectedRunId), api.getEnvironmentLockV2(selectedRunId),
      ]);
      setCheckpoint(runs.find((run) => run.run_id === selectedRunId) ?? null);
      setEvents(nextEvents); setAttempts(nextAttempts); setArtifacts(nextArtifacts); setEnvironmentLock(nextLock);
    } catch (caught) { setError(caught instanceof Error ? caught.message : String(caught)); }
  }, [selectedRunId]);

  useEffect(() => { void refresh(); }, [refresh]);
  if (!selectedRunId) return null;

  return <section className="panel" aria-label="V2 运行与轨迹回放">
    <div className="section-heading"><div><span className="eyebrow">VERIFIED RUN V2</span><h2>{selectedRunId}</h2></div><button onClick={() => void refresh()}>刷新</button></div>
    {checkpoint && <div className="approval-box">
      <strong>状态：{checkpoint.state}</strong>
      <span>已验证步骤：{checkpoint.completed_steps.join(", ") || "无"}</span>
      {checkpoint.attention_reason && <span className="danger">{checkpoint.attention_reason}</span>}
      <div className="button-row">
        <button disabled={checkpoint.state === "needs_attention"} onClick={() => void api.resumeRunV2(selectedRunId).then(refresh).catch((caught) => setError(String(caught)))}>恢复并对账</button>
        <button onClick={() => void api.cancelRun(selectedRunId).then(refresh).catch((caught) => setError(String(caught)))}>取消进程组</button>
      </div>
      <span>资源限制：内存 prlimit / 墙钟超时 / PGID 取消为硬保护；CPU 亲和性与磁盘前后检查依主机能力执行。</span>
    </div>}

    <h3>步骤尝试与验证</h3>
    {attempts.map((attempt) => <article className="stage-card" key={`${attempt.step_id}:${attempt.attempt}`}>
      <header><strong>{attempt.step_id} / 尝试 {attempt.attempt}</strong><code>{attempt.action_hash}</code></header>
      <span>退出码：{attempt.exit_code ?? "运行中"} · PGID：{attempt.process_group_id ?? "未知"}</span>
      <span>manifest：{attempt.manifest_path}</span>
      {attempt.verifications.map((verification, index) => <div className="step-row" key={index}>
        <strong>{verification.passed ? "通过" : "失败"}</strong>
        <code>{JSON.stringify(verification.specification)}</code>
        <span>{verification.observed || "无输出"}</span>
      </div>)}
    </article>)}

    <h3>可回放事件链</h3>
    <ol className="event-list">{events.map((event) => <li key={event.sequence}>
      <strong>{event.sequence}. {event.kind}</strong><span>{event.message}</span>
      {Object.keys(event.details).length > 0 && <code>{JSON.stringify(event.details)}</code>}
      <code>prev={event.prev_hash ?? "GENESIS"} hash={event.event_hash}</code>
    </li>)}</ol>

    <h3>已验证产物</h3>
    {artifacts.map((artifact) => <div className="artifact-row" key={artifact.remote_path}>
      <span>{artifact.remote_path}</span><span>来源：{artifact.source_step_id}</span><code>{artifact.sha256}</code>
    </div>)}
    {environmentLock && <div className="approval-box"><strong>Micromamba 环境锁</strong><span>{environmentLock.remote_path}</span><code>{environmentLock.sha256}</code></div>}
    <div className="approval-box"><strong>导出审计运行包</strong><input aria-label="运行包本地路径" value={bundlePath} onChange={(event) => setBundlePath(event.target.value)} placeholder="C:\\audit\\run.json" /><button disabled={!bundlePath} onClick={() => void api.exportRunBundle(selectedRunId, bundlePath).catch((caught) => setError(String(caught)))}>导出（不覆盖已有文件）</button></div>
    {error && <p role="alert">{error}</p>}
  </section>;
}
