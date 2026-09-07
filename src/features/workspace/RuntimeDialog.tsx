import { CheckCircle2, CircleHelp, Code2, ExternalLink, Monitor, Server, X } from "lucide-react";

import type { ComputeBackendAvailabilityV4, KernelLanguage } from "../../types";
import "./runtime-dialog.css";

export interface RuntimeDialogProps {
  zh: boolean;
  language: KernelLanguage;
  onLanguageChange: (language: KernelLanguage) => void;
  backend?: ComputeBackendAvailabilityV4;
  environment: string;
  onClose: () => void;
  onSettings?: () => void;
  onPrepare: (language: KernelLanguage) => void;
}

type InterpreterStatus = "available" | "unavailable" | "unverified" | "unknown";

const LANGUAGES: KernelLanguage[] = ["python", "r"];

function languageLabel(language: KernelLanguage): string {
  return language === "python" ? "Python" : "R";
}

function statusFor(language: KernelLanguage, backend?: ComputeBackendAvailabilityV4): InterpreterStatus {
  if (!backend) return "unknown";
  return language === "python" ? backend.python_status : backend.r_status;
}

function statusLabel(status: InterpreterStatus, zh: boolean): string {
  switch (status) {
    case "available": return zh ? "可用" : "Available";
    case "unavailable": return zh ? "不可用" : "Unavailable";
    case "unverified": return zh ? "待验证" : "Unverified";
    case "unknown": return zh ? "未检测" : "Not detected";
  }
}

function statusDescription(status: InterpreterStatus, zh: boolean): string {
  switch (status) {
    case "available": return zh ? "当前计算后端报告已找到解释器。" : "The selected compute backend reports an interpreter.";
    case "unavailable": return zh ? "当前计算后端没有报告可用解释器。" : "The selected compute backend did not report an available interpreter.";
    case "unverified": return zh ? "当前后端尚未验证镜像内的解释器。" : "The interpreter has not been verified inside the selected image.";
    case "unknown": return zh ? "尚未选择或探测计算后端。" : "No compute backend has been selected or probed yet.";
  }
}

function backendLabel(backend: ComputeBackendAvailabilityV4 | undefined, zh: boolean): string {
  if (!backend) return zh ? "未选择计算后端" : "No compute backend";
  switch (backend.descriptor.kind) {
    case "local": return "Local";
    case "ssh": return "SSH";
    case "docker": return "Docker";
    case "podman": return "Podman";
  }
}

function backendIcon(backend: ComputeBackendAvailabilityV4 | undefined) {
  return backend?.descriptor.kind === "ssh" ? Server : Monitor;
}

function sessionDescription(backend: ComputeBackendAvailabilityV4 | undefined, language: KernelLanguage, environment: string, zh: boolean): string {
  const runtime = backendLabel(backend, zh);
  const selectedLanguage = languageLabel(language);
  if (backend?.descriptor.kind === "ssh") {
    return zh
      ? `${runtime} 的 Agent 运行使用 ${selectedLanguage} 和项目环境 “${backendEnvironment(environment)}”。`
      : `Agent runs use ${selectedLanguage} on ${runtime} for the selected project environment.`;
  }
  return zh
    ? `${runtime} 的 Agent 运行使用 ${selectedLanguage} 解释器。`
    : `Agent runs use the ${selectedLanguage} interpreter through ${runtime}.`;
}

function backendEnvironment(environment: string): string {
  return environment.trim() || "system";
}

export function RuntimeDialog({ zh, language, onLanguageChange, backend, environment, onClose, onSettings, onPrepare }: RuntimeDialogProps) {
  const selectedStatus = statusFor(language, backend);
  const SelectedBackendIcon = backendIcon(backend);
  const selectedLanguage = languageLabel(language);
  const environmentName = environment.trim() || "system";
  const isSsh = backend?.descriptor.kind === "ssh";

  return (
    <div className="runtime-dialog-backdrop">
      <section className="runtime-dialog" role="dialog" aria-modal="true" aria-labelledby="runtime-dialog-title">
        <header className="runtime-dialog-header">
          <div>
            <span className="runtime-dialog-kicker">{zh ? "运行时" : "Runtimes"}</span>
            <h2 id="runtime-dialog-title">{zh ? "运行时" : "Runtimes"}</h2>
            <p>{zh ? `${backendLabel(backend, zh)} · 环境 · ${environmentName}` : `${backendLabel(backend, zh)} · Environment · ${environmentName}`}</p>
          </div>
          <button type="button" className="runtime-dialog-close" aria-label={zh ? "关闭运行时" : "Close runtimes"} onClick={onClose}>
            <X size={19} />
          </button>
        </header>

        <div className="runtime-dialog-body">
          <nav className="runtime-language-list" aria-label={zh ? "运行时语言" : "Runtime languages"} role="tablist">
            <span className="runtime-dialog-section-label">{zh ? "语言" : "LANGUAGES"}</span>
            {LANGUAGES.map((item) => {
              const status = statusFor(item, backend);
              const active = item === language;
              return (
                <button
                  type="button"
                  role="tab"
                  aria-selected={active}
                  aria-label={`${languageLabel(item)} · ${statusLabel(status, zh)}`}
                  className={`runtime-language-card${active ? " is-active" : ""}`}
                  key={item}
                  onClick={() => onLanguageChange(item)}
                >
                  <span className="runtime-language-card-icon"><Code2 size={18} /></span>
                  <span className="runtime-language-card-copy">
                    <b>{languageLabel(item)}</b>
                    <small>{zh ? "解释器" : "Interpreter"}</small>
                  </span>
                  <span className={`runtime-status runtime-status-${status}`}>
                    {status === "available" && <CheckCircle2 size={13} />}
                    {status === "unverified" || status === "unknown" ? <CircleHelp size={13} /> : null}
                    <span>{statusLabel(status, zh)}</span>
                  </span>
                </button>
              );
            })}
            <p className="runtime-language-hint">
              {zh ? "状态来自当前计算后端探测。镜像内解释器仍需单独验证。" : "Status comes from the current compute backend probe. Image interpreters still require verification."}
            </p>
          </nav>

          <main className="runtime-details" aria-live="polite">
            <header className="runtime-details-header">
              <div className="runtime-details-title">
                <span className="runtime-details-icon"><SelectedBackendIcon size={20} /></span>
                <div>
                  <h3>{selectedLanguage} {zh ? "环境" : "Environment"}</h3>
                  <small>{backendLabel(backend, zh)} · {environmentName}</small>
                </div>
              </div>
              <span className={`runtime-status runtime-status-${selectedStatus}`}>
                {selectedStatus === "available" && <CheckCircle2 size={13} />}
                {selectedStatus === "unverified" || selectedStatus === "unknown" ? <CircleHelp size={13} /> : null}
                <span>{statusLabel(selectedStatus, zh)}</span>
              </span>
            </header>

            <div className="runtime-detail-grid">
              <section className="runtime-detail-card">
                <span className="runtime-detail-label">{zh ? "解释器可用性" : "Interpreter availability"}</span>
                <strong>{statusLabel(selectedStatus, zh)}</strong>
                <p>{statusDescription(selectedStatus, zh)}</p>
              </section>
              <section className="runtime-detail-card">
                <span className="runtime-detail-label">{zh ? "Agent 运行上下文" : "Agent run context"}</span>
                <strong>{zh ? "按运行保持" : "Persistent per run"}</strong>
                <p>{sessionDescription(backend, language, environmentName, zh)}</p>
              </section>
              <section className="runtime-detail-card runtime-detail-card-wide">
                <span className="runtime-detail-label">{zh ? "内存变量检查" : "In-memory variables"}</span>
                <strong>{zh ? "当前不可用" : "Not available"}</strong>
                <p>{zh ? "暂不支持在此查看内存变量。代码输出和验证结果显示在会话的执行记录中。" : "Variable inspection is not available here yet. Code output and verification results appear in the conversation run history."}</p>
              </section>
            </div>

            <div className="runtime-dialog-notice">
              <CircleHelp size={16} />
              <p>{zh ? "将环境检查和准备请求添加到输入框，确认后发送。安装进度与验证结果会显示在会话中。" : "Add an environment check and preparation request to your draft, then send it when ready. Follow installation and verification in the conversation."}</p>
            </div>

            <footer className="runtime-dialog-actions">
              {isSsh && onSettings && <button type="button" className="runtime-secondary-button" onClick={onSettings}><Server size={15} />{zh ? "配置 SSH" : "Configure SSH"}<ExternalLink size={13} /></button>}
              <button type="button" className="runtime-primary-button" onClick={() => onPrepare(language)}>{zh ? "让 Agent 准备环境" : "Ask Agent to prepare environment"}</button>
            </footer>
          </main>
        </div>
      </section>
    </div>
  );
}
