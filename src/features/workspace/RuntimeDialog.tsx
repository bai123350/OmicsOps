import { SshEnvironmentObservations, sshEnvironmentDisplayLabel, type SshEnvironmentProbe } from "./SshEnvironmentObservations";
import { CircleHelp, Code2, Monitor, Server, X } from "lucide-react";
import { useLayoutEffect, useRef } from "react";
import { useWindowEscapeLayer } from "../settings/BrowserSettings";
import type { ComputeBackendAvailabilityV4, KernelInspectionV4, KernelInspectionStatusV4, KernelLanguage, RuntimeBoundaryLimitV4, RuntimeBoundaryViewV4 } from "../../types";
import "./runtime-dialog.css";

export interface RuntimeDialogProps {
  zh: boolean;
  language: KernelLanguage;
  onLanguageChange: (language: KernelLanguage) => void;
  backend?: ComputeBackendAvailabilityV4;
  sshEnvironments?: SshEnvironmentProbe;
  onEnvironmentChange?: (environment: string) => void;
  environment: string;
  boundary: RuntimeBoundaryViewV4 | null;
  boundaryLoading: boolean;
  boundaryError: string;
  onRetryBoundary: () => void;
  onClose: () => void;
  onSettings?: () => void;
  onPrepare?: (language: KernelLanguage) => void;
  onOpenRemoteJobs?: () => void;
  kernelInspection?: KernelInspectionV4 | null;
  kernelInspectionLoading?: boolean;
  kernelInspectionError?: boolean;
  onRefreshKernelInspection?: () => void;
  kernelInspectionOpen?: boolean;
  onOpenKernelInspection?: () => void;
  onCloseKernelInspection?: () => void;
  focusActive?: boolean;
  frozen?: boolean;
}

const limits: Record<RuntimeBoundaryLimitV4, [string, string]> = {
  same_user_permissions: ["与当前用户相同的权限", "same user permissions"],
  project_cwd_not_access_control: ["项目工作目录不限制文件访问", "project working directory does not restrict file access"],
  project_mount_read_write: ["项目挂载可读写", "project mount is read-write"],
  read_only_rootfs: ["容器根文件系统只读", "read-only container root filesystem"],
  capabilities_dropped: ["已移除容器特权能力", "container capabilities dropped"],
  no_new_privileges: ["禁止获取新特权", "no new privileges"],
  pids_limit256: ["最多 256 个进程", "256 process limit"],
  shared_kernel_not_vm: ["与宿主共享内核，并非虚拟机", "shared kernel, not a virtual machine"],
};

function backendName(kind: string): string {
  return kind === "local" ? "Local" : kind === "ssh" ? "SSH" : kind === "docker" ? "Docker" : "Podman";
}

function probeStatus(language: KernelLanguage, backend?: ComputeBackendAvailabilityV4, zh = false): string {
  const status = language === "python" ? backend?.python_status : backend?.r_status;
  if (status === "available") return zh ? "已找到解释器" : "Available · interpreter found";
  if (status === "unavailable") return zh ? "未找到解释器" : "Unavailable · interpreter not found";
  if (status === "unverified") return zh ? "解释器未验证" : "Interpreter unverified";
  return zh ? "尚未探测" : "Not detected";
}

function useDialogFocus(active: boolean) {
  const dialogRef = useRef<HTMLElement>(null); const closeRef = useRef<HTMLButtonElement>(null);
  const activeRef = useRef(active); activeRef.current = active;
  useLayoutEffect(() => {
    const previous = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    const dialog = dialogRef.current; if (activeRef.current) closeRef.current?.focus();
    function trap(event: KeyboardEvent) {
      if (event.key !== "Tab" || !activeRef.current) return;
      const elements = Array.from(dialog?.querySelectorAll<HTMLElement>('button:not([disabled]), summary, [tabindex="0"]') ?? []).filter((item) => !item.hidden && window.getComputedStyle(item).display !== "none" && window.getComputedStyle(item).visibility !== "hidden");
      event.preventDefault(); event.stopImmediatePropagation();
      const index = elements.indexOf(document.activeElement as HTMLElement);
      (elements[index < 0 ? (event.shiftKey ? elements.length - 1 : 0) : (index + (event.shiftKey ? -1 : 1) + elements.length) % elements.length] ?? closeRef.current)?.focus();
    }
    window.addEventListener("keydown", trap);
    return () => { window.removeEventListener("keydown", trap); if (previous?.isConnected && (dialog?.contains(document.activeElement) || document.activeElement === document.body)) previous.focus(); };
  }, []);
  // A focused control can disappear when a boundary finishes loading.
  useLayoutEffect(() => {
    if (!activeRef.current) return;
    const active = document.activeElement;
    if (active === document.body || (active instanceof HTMLButtonElement && active.disabled && dialogRef.current?.contains(active))) closeRef.current?.focus();
  });
  return { dialogRef, closeRef };
}

const inspectionLabels: Record<KernelInspectionStatusV4, [string, string]> = {
  ready: ["可观察 · 内存快照", "Ready · memory snapshot"], not_started: ["未启动 / 当前不可观察", "Not started / currently unobservable"], busy: ["忙碌，请稍后手动刷新", "Busy; refresh manually later"], stale_session: ["会话已改变，请手动刷新", "Stale session; refresh manually"], unavailable: ["当前不可观察", "Unavailable"],
};
function KernelInspectionDialog({ zh, language, value, loading, error, refresh, close, focusActive }: { focusActive: boolean; zh: boolean; language: KernelLanguage; value: KernelInspectionV4 | null; loading: boolean; error: boolean; refresh?: () => void; close: () => void }) {
  const { dialogRef, closeRef } = useDialogFocus(focusActive); useWindowEscapeLayer(true, close);
  const unknown = zh ? "未知" : "Unknown";
  return <div className="runtime-dialog-backdrop" style={{ zIndex: 130 }}>
    <section ref={dialogRef} className="runtime-dialog" style={{ height: "auto", maxHeight: "85vh", overflow: "auto" }} role="dialog" aria-modal="true" aria-labelledby="kernel-inspection-title">
      <header className="runtime-dialog-header"><div><h2 id="kernel-inspection-title">{zh ? "内核元数据" : "Kernel metadata"}</h2><p>{language === "python" ? "Python" : "R"} · {zh ? "内存快照，不是科研证据" : "Memory snapshot, not research evidence"}</p></div><button ref={closeRef} type="button" aria-label={zh ? "关闭内核元数据" : "Close kernel metadata"} onClick={close}><X size={19} /></button></header>
      <div className="runtime-details">
        <p>{zh ? "仅查看本次运行仍存活的内核。不会启动内核，也不保证暂停后仍可观察。仅手动刷新，无自动查询。" : "Observe an existing live kernel for this run. Viewing does not start a kernel or keep it alive while paused. Manual refresh only."}</p>
        {value && <small>{zh ? "来源" : "Source"}: {value.run_id} · {value.language}{value.session_id ? ` · ${value.session_id}` : ""}</small>}
        <p role={error ? "alert" : "status"}>{error ? (zh ? "无法读取内核元数据，请手动重试。" : "Kernel metadata unavailable; retry manually.") : loading ? (zh ? "内核观察请求尚未结束；请在结束后手动刷新。关闭视图不会取消请求。" : "An inspection request is still pending; refresh manually when it finishes. Closing the view does not cancel the request.") : value ? inspectionLabels[value.status][zh ? 0 : 1] : (zh ? "尚未查看，请手动刷新。" : "Not viewed; refresh manually.")}</p>
        {!loading && !error && value?.status === "ready" && <>
          <p>{value.objects.length} {zh ? "个对象（最多 64 个）" : "objects (at most 64)"}{value.truncated ? ` · ${zh ? "结果已截断" : "Truncated"}` : ""}</p>
          <div style={{ overflowX: "auto" }}><table style={{ width: "100%", textAlign: "left", overflowWrap: "anywhere" }}><thead><tr>{(zh ? ["名称", "类型", "维度", "元素数", "摘要"] : ["Name", "Type", "Dimensions", "Elements", "Summary"]).map((label) => <th key={label}>{label}</th>)}</tr></thead><tbody>{value.objects.map((object, index) => <tr key={index}><td>{object.name}</td><td>{object.type_label}</td><td>{object.dimensions === null ? unknown : object.dimensions.length ? object.dimensions.join(" × ") : (zh ? "标量" : "Scalar")}</td><td>{object.element_count ?? unknown}</td><td>{object.summary}</td></tr>)}</tbody></table></div>
          {value.objects.length === 0 && <p>{zh ? "没有可显示的安全元数据。" : "No safe metadata to display."}</p>}
        </>}
        <footer className="runtime-dialog-actions"><button type="button" className="runtime-secondary-button" disabled={loading || !refresh} onClick={() => { closeRef.current?.focus(); refresh?.(); }}>{zh ? "刷新元数据" : "Refresh metadata"}</button></footer>
      </div>
    </section>
  </div>;
}

export function RuntimeDialog({ zh, language, onLanguageChange, backend, sshEnvironments, onEnvironmentChange, boundary, boundaryLoading, boundaryError, onRetryBoundary, onClose, onSettings, onPrepare, onOpenRemoteJobs, kernelInspection = null, kernelInspectionLoading = false, kernelInspectionError = false, onRefreshKernelInspection, kernelInspectionOpen = false, onOpenKernelInspection, onCloseKernelInspection, focusActive = true, frozen = false }: RuntimeDialogProps) {
  // Runtime covers the lower trajectory regardless of sibling DOM order;
  // its child, remote jobs and global Search can suspend its focus ownership.
  const { dialogRef, closeRef } = useDialogFocus(focusActive && !kernelInspectionOpen);
  const isFrozen = frozen || boundary?.source.kind === "frozen_run";
  const selection = boundary?.compute_selection;
  const matchingProbe = !isFrozen && boundary?.source.kind === "draft_selection" && backend !== undefined && selection !== undefined
    && backend.descriptor.backend_id === selection.backend_id && backend.descriptor.kind === selection.backend_kind;
  const matchingSshProbe = matchingProbe && backend?.descriptor.kind === "ssh" && sshEnvironments?.report?.project_id === boundary?.project_id
    && backend.descriptor.backend_id === `ssh:${sshEnvironments.report.connection_id}` && sshEnvironments.selected?.environment === selection?.environment;
  const displayedBackend = backend?.descriptor.kind === "ssh" && sshEnvironments ? {
    ...backend,
    python_status: matchingSshProbe ? observedStatus("python") : "unverified" as const,
    r_status: matchingSshProbe ? observedStatus("r") : "unverified" as const,
  } : backend;
  function observedStatus(language: KernelLanguage): "available" | "unavailable" | "unverified" {
    const observation = sshEnvironments?.selected?.interpreters.find((item) => item.language === language);
    return observation?.status === "available" ? "available" : observation?.status === "not_found" ? "unavailable" : "unverified";
  }
  const environmentLabel = matchingSshProbe ? sshEnvironments.selected!.label : sshEnvironmentDisplayLabel(selection?.environment, zh);
  const runtimeName = selection ? backendName(selection.backend_kind) : (zh ? "计算环境" : "Compute environment");
  const LocationIcon = boundary?.execution_location === "ssh_host" ? Server : Monitor;
  const sourceLabel = isFrozen ? (zh ? "已记录的运行" : "Recorded run") : (zh ? "下一次运行的配置" : "Next run configuration");
  return <div className="runtime-dialog-backdrop">
    <section ref={dialogRef} className="runtime-dialog" role="dialog" aria-modal="true" aria-labelledby="runtime-dialog-title">
      <header className="runtime-dialog-header"><div><span className="runtime-dialog-kicker">{sourceLabel}</span><h2 id="runtime-dialog-title">{zh ? "运行时" : "Runtimes"}</h2><p>{selection ? `${runtimeName} · ${environmentLabel}` : (zh ? "正在读取运行配置" : "Reading runtime configuration")}</p></div><button ref={closeRef} type="button" className="runtime-dialog-close" aria-label={zh ? "关闭运行时" : "Close runtimes"} onClick={onClose}><X size={19} /></button></header>
      <div className="runtime-dialog-body">
        <nav className="runtime-language-list" aria-label={zh ? "运行时语言" : "Runtime languages"} role="tablist">
          <span className="runtime-dialog-section-label">{zh ? "语言" : "LANGUAGES"}</span>
          {(["python", "r"] as KernelLanguage[]).map((item) => <button key={item} type="button" role="tab" aria-selected={item === language} aria-label={`${item === "python" ? "Python" : "R"}${matchingProbe ? ` · ${probeStatus(item, displayedBackend, zh)}` : ""}`} className={`runtime-language-card${item === language ? " is-active" : ""}`} onClick={() => onLanguageChange(item)}><span className="runtime-language-card-icon"><Code2 size={18} /></span><span className="runtime-language-card-copy"><b>{item === "python" ? "Python" : "R"}</b><small>{zh ? "解释器" : "Interpreter"}</small></span>{matchingProbe && <span className="runtime-status runtime-status-unknown">{probeStatus(item, displayedBackend, zh)}</span>}</button>)}
          {matchingProbe && <p className="runtime-language-hint">{zh ? "最近探测仅反映当前计算后端。找到解释器不代表科研依赖已安装。" : "Recent probe of the current compute backend. Finding an interpreter does not verify research dependencies."}</p>}
        </nav>
        <main className="runtime-details" aria-live="polite">
          {!isFrozen && selection?.backend_kind === "ssh" && sshEnvironments && (matchingSshProbe || !sshEnvironments.selected) && <SshEnvironmentObservations probe={sshEnvironments} environment={selection.environment} zh={zh} onEnvironmentChange={matchingProbe ? onEnvironmentChange : undefined} />}
          <header className="runtime-details-header"><div className="runtime-details-title"><span className="runtime-details-icon"><LocationIcon size={20} /></span><div><h3>{language === "python" ? "Python" : "R"} {zh ? "运行环境" : "environment"}</h3><small>{sourceLabel}{boundary?.source.kind === "frozen_run" ? ` · ${boundary.source.run_id}` : ""}</small></div></div></header>
          {boundaryError ? <div className="runtime-boundary-message" role="alert"><CircleHelp size={16} /><p>{zh ? "运行边界加载失败：" : "Could not load runtime boundary: "}{boundaryError}</p><button type="button" onClick={onRetryBoundary}>{zh ? "重试" : "Retry boundary"}</button></div>
            : boundaryLoading ? <p className="runtime-boundary-message" role="status">{zh ? "正在加载运行边界…" : "Loading runtime boundary…"}</p>
              : !boundary ? <p className="runtime-boundary-message" role="status">{zh ? "配置未完成，无法说明运行边界。" : "Configuration incomplete. Runtime boundary unavailable."}</p>
                : <>
                  <div className="runtime-detail-grid">
                    <section className="runtime-detail-card"><span className="runtime-detail-label">{zh ? "执行位置与隔离" : "Location and isolation"}</span><strong>{boundary.execution_location === "local_host" ? (zh ? "本机进程" : "Local host process") : boundary.execution_location === "ssh_host" ? (zh ? "SSH 主机进程" : "SSH host process") : (zh ? "本机计算容器" : "Local compute container")}</strong><p>{boundary.isolation === "process" ? (zh ? "进程级隔离" : "Process isolation") : (zh ? "容器级隔离" : "Container isolation")}</p></section>
                    <section className="runtime-detail-card"><span className="runtime-detail-label">{zh ? "环境与策略" : "Environment and policy"}</span><strong>{environmentLabel}</strong><p>{selection?.autonomy_mode === "full_auto" ? (zh ? "全自动" : "Full auto") : (zh ? "受监督" : "Supervised")} · {selection?.approval_policy === "full_access" ? (zh ? "完全访问" : "Full access") : selection?.approval_policy === "request_approval" ? (zh ? "逐次审批" : "Request approval") : selection?.approval_policy === "auto_approve_except_local_deletion" ? (zh ? "自动批准操作，仅检测到本地删除时询问" : "Automatically approve operations; ask when local deletion is detected") : (zh ? "基于风险的审批" : "Risk-based approval")}</p></section>
                    <section className="runtime-detail-card runtime-detail-card-wide"><span className="runtime-detail-label">{zh ? "宿主边界" : "Host limits"}</span><ul>{boundary.limits.map((item) => <li key={item}>{limits[item][zh ? 0 : 1]}</li>)}</ul>{boundary.execution_location === "local_container" ? <p>{zh ? "无网络仅作用于计算容器；模型和 MCP 服务仍可能使用网络。未设置 CPU 或内存配额。" : "No network applies only to the compute container; model and MCP services may still use network. No CPU or memory quota is asserted."}</p> : <p>{zh ? "沿用宿主网络；项目目录不是访问控制。" : "Host network is inherited; the project directory is not access control."}</p>}</section>
                    <section className="runtime-detail-card runtime-detail-card-wide"><span className="runtime-detail-label">{zh ? "运行与恢复" : "Run and recovery"}</span><p>{zh ? "交互内核只在本次运行期间保持，应用重启后无法重连。停止本地等待不代表远端计算已终止。" : "Interactive kernels are scoped to a run and cannot reconnect after app restart. Stopping local wait does not establish remote termination."}</p><p>{boundary.detached_job_lifecycle === "ssh_linux_only" ? (zh ? "独立后台作业仅支持 Linux SSH；此处未验证主机系统。远端作业面板默认手动查询，可明确开启最多 10 次自动查询；无作业取消。Stop 不确认远端终止。" : "Detached jobs: Linux SSH only; host OS is not checked here. Remote jobs default to manual queries; explicitly enable at most 10 automatic queries in the panel. No job cancellation. Stop does not confirm remote termination.") : (zh ? "此后端不支持断线后重连的独立后台作业。" : "Detached jobs are unsupported on this backend.")}</p></section>
                    <section className="runtime-detail-card runtime-detail-card-wide"><span className="runtime-detail-label">{zh ? "内存变量检查" : "In-memory variables"}</span><p>{zh ? "只读观察仍存活的 V4 内核安全元数据，不显示对象内容，不启动内核。请从运行记录打开查看。" : "Read-only observation of safe metadata from an existing V4 kernel. No object contents; no kernel startup. Open from a run record to view."}</p></section>
                  </div>
                  <div className="runtime-dialog-notice"><CircleHelp size={16} /><p>{zh ? "此视图说明配置边界；解释器、科研依赖和运行结果均未通过此视图验证。" : "This view describes configuration boundaries. Interpreters, research dependencies have not been verified by this view; run results are not checked."}</p></div>
                  <details className="runtime-boundary-identity"><summary>{zh ? "完整计算身份" : "Full compute identity"}</summary><p>{selection?.backend_id}{selection?.container_image ? ` · ${selection.container_image.reference} · ${selection.container_image.image_id}` : ""}</p><p>{selection?.environment}</p></details>
                </>}
          <footer className="runtime-dialog-actions">{onOpenKernelInspection && <button type="button" className="runtime-secondary-button" onClick={onOpenKernelInspection}>{zh ? "查看内核元数据" : "View kernel metadata"}</button>}{onOpenRemoteJobs && <button type="button" className="runtime-secondary-button" onClick={onOpenRemoteJobs}>{zh ? "远端作业" : "Remote jobs"}</button>}{!isFrozen && boundary?.source.kind === "draft_selection" && <>{boundary.execution_location === "ssh_host" && onSettings && <button type="button" className="runtime-secondary-button" onClick={onSettings}>{zh ? "配置 SSH" : "Configure SSH"}</button>}{onPrepare && <button type="button" className="runtime-primary-button" onClick={() => onPrepare(language)}>{zh ? "让 Agent 准备环境" : "Ask Agent to prepare environment"}</button>}</>}</footer>
        </main>
      </div>
    </section>
    {kernelInspectionOpen && onCloseKernelInspection && <KernelInspectionDialog focusActive={focusActive} zh={zh} language={language} value={kernelInspection} loading={kernelInspectionLoading} error={kernelInspectionError} refresh={onRefreshKernelInspection} close={onCloseKernelInspection} />}
  </div>;
}
