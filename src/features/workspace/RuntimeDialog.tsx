import { CircleHelp, Code2, Monitor, Server, X } from "lucide-react";
import type { ComputeBackendAvailabilityV4, KernelLanguage, RuntimeBoundaryLimitV4, RuntimeBoundaryViewV4 } from "../../types";
import "./runtime-dialog.css";

export interface RuntimeDialogProps {
  zh: boolean;
  language: KernelLanguage;
  onLanguageChange: (language: KernelLanguage) => void;
  backend?: ComputeBackendAvailabilityV4;
  environment: string;
  boundary: RuntimeBoundaryViewV4 | null;
  boundaryLoading: boolean;
  boundaryError: string;
  onRetryBoundary: () => void;
  onClose: () => void;
  onSettings?: () => void;
  onPrepare?: (language: KernelLanguage) => void;
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

export function RuntimeDialog({ zh, language, onLanguageChange, backend, boundary, boundaryLoading, boundaryError, onRetryBoundary, onClose, onSettings, onPrepare, frozen = false }: RuntimeDialogProps) {
  const isFrozen = frozen || boundary?.source.kind === "frozen_run";
  const selection = boundary?.compute_selection;
  const matchingProbe = !isFrozen && boundary?.source.kind === "draft_selection" && backend !== undefined && selection !== undefined
    && backend.descriptor.backend_id === selection.backend_id && backend.descriptor.kind === selection.backend_kind;
  const runtimeName = selection ? backendName(selection.backend_kind) : (zh ? "计算环境" : "Compute environment");
  const LocationIcon = boundary?.execution_location === "ssh_host" ? Server : Monitor;
  const sourceLabel = isFrozen ? (zh ? "已记录的运行" : "Recorded run") : (zh ? "下一次运行的配置" : "Next run configuration");
  return <div className="runtime-dialog-backdrop">
    <section className="runtime-dialog" role="dialog" aria-modal="true" aria-labelledby="runtime-dialog-title">
      <header className="runtime-dialog-header"><div><span className="runtime-dialog-kicker">{sourceLabel}</span><h2 id="runtime-dialog-title">{zh ? "运行时" : "Runtimes"}</h2><p>{selection ? `${runtimeName} · ${selection.environment}` : (zh ? "正在读取运行配置" : "Reading runtime configuration")}</p></div><button type="button" className="runtime-dialog-close" aria-label={zh ? "关闭运行时" : "Close runtimes"} onClick={onClose}><X size={19} /></button></header>
      <div className="runtime-dialog-body">
        <nav className="runtime-language-list" aria-label={zh ? "运行时语言" : "Runtime languages"} role="tablist">
          <span className="runtime-dialog-section-label">{zh ? "语言" : "LANGUAGES"}</span>
          {(["python", "r"] as KernelLanguage[]).map((item) => <button key={item} type="button" role="tab" aria-selected={item === language} aria-label={`${item === "python" ? "Python" : "R"}${matchingProbe ? ` · ${probeStatus(item, backend, zh)}` : ""}`} className={`runtime-language-card${item === language ? " is-active" : ""}`} onClick={() => onLanguageChange(item)}><span className="runtime-language-card-icon"><Code2 size={18} /></span><span className="runtime-language-card-copy"><b>{item === "python" ? "Python" : "R"}</b><small>{zh ? "解释器" : "Interpreter"}</small></span>{matchingProbe && <span className="runtime-status runtime-status-unknown">{probeStatus(item, backend, zh)}</span>}</button>)}
          {matchingProbe && <p className="runtime-language-hint">{zh ? "最近探测仅反映当前计算后端。找到解释器不代表科研依赖已安装。" : "Recent probe of the current compute backend. Finding an interpreter does not verify research dependencies."}</p>}
        </nav>
        <main className="runtime-details" aria-live="polite">
          <header className="runtime-details-header"><div className="runtime-details-title"><span className="runtime-details-icon"><LocationIcon size={20} /></span><div><h3>{language === "python" ? "Python" : "R"} {zh ? "运行环境" : "environment"}</h3><small>{sourceLabel}{boundary?.source.kind === "frozen_run" ? ` · ${boundary.source.run_id}` : ""}</small></div></div></header>
          {boundaryError ? <div className="runtime-boundary-message" role="alert"><CircleHelp size={16} /><p>{zh ? "运行边界加载失败：" : "Could not load runtime boundary: "}{boundaryError}</p><button type="button" onClick={onRetryBoundary}>{zh ? "重试" : "Retry boundary"}</button></div>
            : boundaryLoading ? <p className="runtime-boundary-message" role="status">{zh ? "正在加载运行边界…" : "Loading runtime boundary…"}</p>
              : !boundary ? <p className="runtime-boundary-message" role="status">{zh ? "配置未完成，无法说明运行边界。" : "Configuration incomplete. Runtime boundary unavailable."}</p>
                : <>
                  <div className="runtime-detail-grid">
                    <section className="runtime-detail-card"><span className="runtime-detail-label">{zh ? "执行位置与隔离" : "Location and isolation"}</span><strong>{boundary.execution_location === "local_host" ? (zh ? "本机进程" : "Local host process") : boundary.execution_location === "ssh_host" ? (zh ? "SSH 主机进程" : "SSH host process") : (zh ? "本机计算容器" : "Local compute container")}</strong><p>{boundary.isolation === "process" ? (zh ? "进程级隔离" : "Process isolation") : (zh ? "容器级隔离" : "Container isolation")}</p></section>
                    <section className="runtime-detail-card"><span className="runtime-detail-label">{zh ? "环境与策略" : "Environment and policy"}</span><strong>{selection?.environment}</strong><p>{selection?.autonomy_mode === "full_auto" ? (zh ? "全自动" : "Full auto") : (zh ? "受监督" : "Supervised")} · {selection?.approval_policy === "full_access" ? (zh ? "完全访问" : "Full access") : selection?.approval_policy === "request_approval" ? (zh ? "逐次审批" : "Request approval") : selection?.approval_policy === "auto_approve_except_local_deletion" ? (zh ? "自动批准操作，仅检测到本地删除时询问" : "Automatically approve operations; ask when local deletion is detected") : (zh ? "基于风险的审批" : "Risk-based approval")}</p></section>
                    <section className="runtime-detail-card runtime-detail-card-wide"><span className="runtime-detail-label">{zh ? "宿主边界" : "Host limits"}</span><ul>{boundary.limits.map((item) => <li key={item}>{limits[item][zh ? 0 : 1]}</li>)}</ul>{boundary.execution_location === "local_container" ? <p>{zh ? "无网络仅作用于计算容器；模型和 MCP 服务仍可能使用网络。未设置 CPU 或内存配额。" : "No network applies only to the compute container; model and MCP services may still use network. No CPU or memory quota is asserted."}</p> : <p>{zh ? "沿用宿主网络；项目目录不是访问控制。" : "Host network is inherited; the project directory is not access control."}</p>}</section>
                    <section className="runtime-detail-card runtime-detail-card-wide"><span className="runtime-detail-label">{zh ? "运行与恢复" : "Run and recovery"}</span><p>{zh ? "交互内核只在本次运行期间保持，应用重启后无法重连。停止本地等待不代表远端计算已终止。" : "Interactive kernels are scoped to a run and cannot reconnect after app restart. Stopping local wait does not establish remote termination."}</p><p>{boundary.detached_job_lifecycle === "ssh_linux_only" ? (zh ? "独立后台作业仅支持 Linux SSH；此处未验证主机系统。无自动轮询或取消；Stop 不确认远端终止。" : "Detached jobs: Linux SSH only; host OS is not checked here. No automatic polling or cancellation. Stop does not confirm remote termination.") : (zh ? "此后端不支持断线后重连的独立后台作业。" : "Detached jobs are unsupported on this backend.")}</p></section>
                    <section className="runtime-detail-card runtime-detail-card-wide"><span className="runtime-detail-label">{zh ? "内存变量检查" : "In-memory variables"}</span><strong>{zh ? "当前不可用" : "Not available"}</strong><p>{zh ? "暂不支持在此查看内存变量。代码输出和验证结果显示在会话的执行记录中。" : "Variable inspection is not available here yet. Code output and verification results appear in the conversation run history."}</p></section>
                  </div>
                  <div className="runtime-dialog-notice"><CircleHelp size={16} /><p>{zh ? "此视图说明配置边界；解释器、科研依赖和运行结果均未通过此视图验证。" : "This view describes configuration boundaries. Interpreters, research dependencies have not been verified by this view; run results are not checked."}</p></div>
                  <details className="runtime-boundary-identity"><summary>{zh ? "完整计算身份" : "Full compute identity"}</summary><p>{selection?.backend_id}{selection?.container_image ? ` · ${selection.container_image.reference} · ${selection.container_image.image_id}` : ""}</p></details>
                </>}
          <footer className="runtime-dialog-actions">{!isFrozen && boundary?.source.kind === "draft_selection" && <>{boundary.execution_location === "ssh_host" && onSettings && <button type="button" className="runtime-secondary-button" onClick={onSettings}>{zh ? "配置 SSH" : "Configure SSH"}</button>}{onPrepare && <button type="button" className="runtime-primary-button" onClick={() => onPrepare(language)}>{zh ? "让 Agent 准备环境" : "Ask Agent to prepare environment"}</button>}</>}</footer>
        </main>
      </div>
    </section>
  </div>;
}
