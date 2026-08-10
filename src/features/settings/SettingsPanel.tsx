import { Archive, Bot, Cloud, KeyRound, Monitor, Server, ShieldCheck, Wrench, X } from "lucide-react";
import type { Locale } from "../workspace/copy";
import "./settings.css";

export function SettingsPanel({ locale, onClose }: { locale: Locale; onClose: () => void }) {
  const zh = locale === "zh-CN";
  return <div className="settings-backdrop"><section className="settings-panel" role="dialog" aria-modal="true" aria-label={zh ? "工作台设置" : "Workspace settings"}>
    <header><div><small>OmicsOps Desktop</small><h2>{zh ? "工作台设置" : "Workspace settings"}</h2></div><button aria-label="Close" onClick={onClose}><X size={19} /></button></header>
    <div className="settings-layout">
      <nav><button className="active"><Bot size={16} />{zh ? "模型提供方" : "Model providers"}</button><button><Server size={16} />{zh ? "远端计算" : "Remote compute"}</button><button><Wrench size={16} />{zh ? "技能与 MCP" : "Skills and MCP"}</button><button><ShieldCheck size={16} />{zh ? "隐私与权限" : "Privacy and permissions"}</button><button><Archive size={16} />{zh ? "历史运行只读" : "Read-only legacy runs"}</button></nav>
      <main><div className="settings-heading"><h3>{zh ? "模型提供方" : "Model providers"}</h3><p>{zh ? "密钥保存在 Windows Credential Manager，项目只记录引用。" : "Keys stay in Windows Credential Manager; projects store references only."}</p></div>
        <div className="provider-grid"><Provider icon={Cloud} name="Anthropic" detail="Messages API · tool use" /><Provider icon={KeyRound} name="OpenAI-compatible" detail="Chat Completions · custom Base URL" /><Provider icon={Monitor} name="Ollama" detail={zh ? "本地模型 · 可设为项目强制策略" : "Local models · optional project-only policy"} /></div>
        <div className="settings-note"><ShieldCheck size={18} /><span><b>{zh ? "默认无遥测" : "Telemetry off by default"}</b><small>{zh ? "诊断包仅在主动导出时生成，并经过凭据脱敏。" : "Diagnostic bundles are generated only on export and redact credentials."}</small></span></div>
        <div className="legacy-row"><Archive size={18} /><span><b>{zh ? "历史运行只读" : "Read-only legacy runs"}</b><small>{zh ? "旧 V1/V2 计划、日志与审计可查看和导出，但不能启动新任务。" : "Legacy plans, logs, and audit records remain viewable and exportable."}</small></span><button>{zh ? "查看历史" : "View history"}</button></div>
      </main>
    </div>
  </section></div>;
}

function Provider({ icon: Icon, name, detail }: { icon: typeof Cloud; name: string; detail: string }) {
  return <article><span><Icon size={19} /></span><div><b>{name}</b><small>{detail}</small></div><button>{name === "Ollama" ? "Detect" : "Configure"}</button></article>;
}
