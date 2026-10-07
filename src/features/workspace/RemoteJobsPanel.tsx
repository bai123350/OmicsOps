import { useCallback, useLayoutEffect, useRef, useState, type KeyboardEvent, type ReactNode } from "react";
import { useWindowEscapeLayer } from "../settings/BrowserSettings";
import type { RemoteJobObservationStatusV4, RemoteJobQueryEligibilityV4, RemoteJobRecordedStateV4 } from "../../types";
import type { Locale } from "./copy";
import { useRemoteJobs } from "./useRemoteJobs";
import "./remote-jobs.css";

const states: Record<RemoteJobRecordedStateV4, [string, string]> = {
  reserved: ["已保留", "Reserved"], running: ["运行中", "Running"], unknown: ["未知", "Unknown"], succeeded: ["成功", "Succeeded"], failed: ["失败", "Failed"],
};
const statuses: Record<RemoteJobObservationStatusV4, [string, string]> = {
  running: ["运行中", "Running"], unknown: ["未知", "Unknown"], completed: ["已完成", "Completed"], timeout: ["查询超时", "Query timed out"], unavailable: ["无法查询", "Unavailable"], rejected: ["查询被拒绝", "Rejected"],
};
const eligibility: Record<RemoteJobQueryEligibilityV4, [string, string]> = {
  needs_root_check: ["查询前由宿主验证原始绑定、信任和规范路径。", "The host verifies the original binding, trust and canonical root before querying."],
  binding_changed: ["项目绑定已改变，无法查询此原始作业。", "The project binding has changed; this original job cannot be queried."],
  unsupported: ["此记录不支持远端状态查询。", "This record does not support remote status queries."],
  integrity_mismatch: ["记录身份校验失败，无法查询。", "Recorded identity verification failed; querying is disabled."],
  recorded_receipt: ["本地验证保留的回执，不连接 SSH；列表未验证回执内容。", "Verify retained receipt locally without SSH; the list has not verified its contents."],
};
const errors: Record<string, [string, string]> = {
  remote_job_rejected: ["作业查询被拒绝；请检查原始记录与项目绑定。", "Job query rejected. Check the original record and project binding."],
  remote_job_unavailable: ["暂时无法读取作业，请重试。", "Job information is unavailable. Please retry."],
  remote_job_timeout: ["远端状态查询超时，计算状态仍未知。", "Remote status query timed out; compute status remains unknown."],
  remote_job_not_found: ["未找到此项目中的作业记录。", "The job record was not found in this project."],
};

export function RemoteJobsPanel({ projectId, conversationId, bindingVersion, locale, onClose }: { projectId: string; conversationId: string | null; bindingVersion: string; locale: Locale; onClose: () => void }): ReactNode {
  const zh = locale === "zh-CN"; const language = zh ? 0 : 1;
  const [allProject, setAllProject] = useState(conversationId === null);
  // The filter can span conversations, but owner navigation still ends this
  // local observation window. This token never crosses the native API boundary.
  const ownerVersion = JSON.stringify([projectId, conversationId, bindingVersion]);
  const jobs = useRemoteJobs({ projectId, conversationId: allProject ? null : conversationId, bindingVersion: ownerVersion }, true);
  const selected = jobs.jobs.find((job) => job.job_id === jobs.selectedJobId);
  const dialogRef = useRef<HTMLElement>(null); const closeRef = useRef<HTMLButtonElement>(null);
  const onCloseRef = useRef(onClose); onCloseRef.current = onClose;
  const close = useCallback(() => { jobs.stopPolling(); onCloseRef.current(); }, [jobs.stopPolling]);
  useWindowEscapeLayer(true, close);
  useLayoutEffect(() => {
    const previous = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    const dialog = dialogRef.current; closeRef.current?.focus();
    return () => { if (previous?.isConnected && (dialog?.contains(document.activeElement) || document.activeElement === document.body)) previous.focus(); };
  }, []);
  useLayoutEffect(() => {
    const active = document.activeElement;
    if (active instanceof HTMLButtonElement && active.disabled && dialogRef.current?.contains(active)) closeRef.current?.focus();
  }, [jobs.loading, jobs.polling, jobs.observation, selected]);
  function trapFocus(event: KeyboardEvent<HTMLElement>) {
    if (event.key !== "Tab") return;
    const elements = Array.from(dialogRef.current?.querySelectorAll<HTMLElement>('button:not([disabled]), [tabindex="0"]') ?? []).filter((item) => !item.hidden && window.getComputedStyle(item).display !== "none" && window.getComputedStyle(item).visibility !== "hidden");
    event.preventDefault(); if (!elements.length) return;
    const index = elements.indexOf(document.activeElement as HTMLElement);
    elements[index < 0 ? (event.shiftKey ? elements.length - 1 : 0) : (index + (event.shiftKey ? -1 : 1) + elements.length) % elements.length].focus();
  }
  function reload() { closeRef.current?.focus(); void jobs.reload(); }
  const canRefresh = selected && ["needs_root_check", "recorded_receipt"].includes(selected.query_eligibility);
  const terminal = selected?.recorded_state === "succeeded" || selected?.recorded_state === "failed" || jobs.observation?.status === "completed" || jobs.observation?.status === "rejected";
  return <div className="remote-jobs-backdrop">
    <section className="remote-jobs-panel" ref={dialogRef} role="dialog" aria-modal="true" aria-labelledby="remote-jobs-title" onKeyDown={trapFocus}>
      <header><div><h2 id="remote-jobs-title">{zh ? "远端后台作业" : "Remote background jobs"}</h2><p>{zh ? "列表仅显示本地记录，未检查远端存活状态。最多显示 100 条记录。" : "The list shows local records only. Remote liveness has not been checked. At most 100 records are displayed."}</p></div><button ref={closeRef} type="button" aria-label={zh ? "关闭远端作业" : "Close remote jobs"} onClick={close}>×</button></header>
      <nav aria-label={zh ? "作业范围" : "Job scope"}><button type="button" aria-pressed={!allProject} disabled={conversationId === null} onClick={() => { closeRef.current?.focus(); setAllProject(false); }}>{zh ? "当前会话" : "Current conversation"}</button><button type="button" aria-pressed={allProject} onClick={() => { closeRef.current?.focus(); setAllProject(true); }}>{zh ? "项目全部作业" : "All project jobs"}</button></nav>
      {jobs.error && <div role="alert"><p>{(errors[jobs.error] ?? errors.remote_job_unavailable)[language]}</p><button type="button" onClick={reload}>{zh ? "重试列表" : "Retry list"}</button></div>}
      {jobs.loading && <p role="status">{zh ? "正在读取作业信息，或等待尚未结束的状态查询。查询结束前无法发起另一查询。" : "Reading job information or waiting for a pending status query to settle. Another query is unavailable until it settles."}</p>}
      <div className="remote-jobs-body">
        <ul className="remote-jobs-list" aria-label={zh ? "已记录作业" : "Recorded jobs"}>{jobs.jobs.map((job) => <li key={job.job_id}><button type="button" aria-pressed={job.job_id === jobs.selectedJobId} onClick={() => jobs.selectJob(job.job_id)}>{job.job_id} · {job.language} · {states[job.recorded_state][language]}</button></li>)}</ul>
        {!jobs.loading && !jobs.error && jobs.jobs.length === 0 && <p>{zh ? "此范围暂无作业记录。" : "No job records in this scope."}</p>}
        {selected && <article className="remote-job-detail">
          <p>{zh ? "记录状态：" : "Recorded state: "}{states[selected.recorded_state][language]}</p>
          <p>{zh ? "记录时间：" : "Recorded time: "}{selected.recorded_at ?? (zh ? "未知" : "Unknown")}</p>
          <dl><dt>{zh ? "会话 / 运行" : "Conversation / Run"}</dt><dd>{selected.conversation_id} / {selected.run_id}</dd><dt>{zh ? "后端 / 环境" : "Backend / Environment"}</dt><dd>{selected.backend_id} / {selected.environment}</dd><dt>{zh ? "调用" : "Call"}</dt><dd>{selected.call_id}</dd><dt>SHA-256</dt><dd>{selected.request_sha256}</dd></dl>
          <p>{eligibility[selected.query_eligibility][language]}</p>
          <div className="remote-job-actions"><button type="button" disabled={!canRefresh || jobs.loading} onClick={() => { closeRef.current?.focus(); void jobs.refresh(); }}>{selected.query_eligibility === "recorded_receipt" ? (zh ? "验证记录回执" : "Verify recorded receipt") : (zh ? "刷新状态" : "Refresh status")}</button>{jobs.polling ? <button type="button" onClick={() => { closeRef.current?.focus(); jobs.stopPolling(); }}>{zh ? "停止本地查询" : "Stop local queries"}</button> : <button type="button" disabled={jobs.loading || terminal || selected.query_eligibility !== "needs_root_check"} onClick={() => { closeRef.current?.focus(); jobs.startPolling(); }}>{zh ? "开启自动查询" : "Start automatic queries"}</button>}</div>
          <p>{zh ? "本窗口查询次数：" : "Queries in this window: "}{jobs.attempts} / 10</p>
          {jobs.observation && <section className="remote-job-observation" aria-label={zh ? "查询结果" : "Observation"}><h3>{zh ? "查询结果：" : "Observation: "}{statuses[jobs.observation.status][language]}</h3><p>{zh ? "查询时间：" : "Observed at: "}{jobs.observation.observed_at}</p><p>{zh ? "输出摘录分别最多 4 KiB；安全产物/捕获引用最多 128 项，不代表完整结果或科研证据。" : "Output excerpts are each limited to 4 KiB; at most 128 safe artifact/capture references. These are not complete results or scientific evidence."}</p>{jobs.observation.stdout_excerpt && <pre aria-label="stdout">{jobs.observation.stdout_excerpt}</pre>}{jobs.observation.stderr_excerpt && <pre aria-label="stderr">{jobs.observation.stderr_excerpt}</pre>}<ul>{jobs.observation.captures.map((capture, index) => <li key={index}><code>{capture.relative_path}</code> · SHA-256 {capture.sha256} · {capture.size_bytes === null ? (zh ? "大小未知" : "Size unknown") : `${capture.size_bytes} bytes`}</li>)}</ul></section>}
        </article>}
      </div>
      <footer><p>{zh ? "自动查询默认关闭：仅查询所选作业，按 2/4/8/16/30 秒串行退避，每个窗口最多 10 次。停止或关闭仅停止本地查询，不取消远端计算、不重提作业、不恢复 Agent。" : "Automatic queries are off by default: one selected job, serial 2/4/8/16/30-second backoff, at most 10 queries per window. Stopping or closing stops local queries only; it does not cancel remote compute, resubmit jobs or resume the Agent."}</p><button type="button" disabled={jobs.loading} onClick={reload}>{zh ? "重新加载列表" : "Reload list"}</button></footer>
    </section>
  </div>;
}
