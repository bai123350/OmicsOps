import { useState } from "react";
import { Archive, Bot, Cloud, KeyRound, Monitor, Server, ShieldCheck, Wrench, X } from "lucide-react";
import type { ModelProfile, SkillPackage } from "../../types";
import type { Locale } from "../workspace/copy";
import "./settings.css";
import "./model-form.css";

type SaveModelRequest = { id?: string; label: string; provider: ModelProfile["provider"]; base_url: string; model: string; credential?: string };
type FormState = SaveModelRequest & { credential: string };

interface Props {
  locale: Locale;
  onClose: () => void;
  modelProfiles?: ModelProfile[];
  onSaveModel?: (request: SaveModelRequest) => Promise<void>;
  onProbeModel?: (profileId: string) => Promise<void>;
  skillPackages?: SkillPackage[];
  onImportSkill?: () => Promise<void>;
  onSetSkillEnabled?: (skillId: string, enabled: boolean) => Promise<SkillPackage>;
}

const defaults: Record<ModelProfile["provider"], FormState> = {
  anthropic: { provider: "anthropic", label: "Anthropic", base_url: "https://api.anthropic.com/", model: "", credential: "" },
  open_ai_compatible: { provider: "open_ai_compatible", label: "OpenAI-compatible", base_url: "https://api.openai.com/", model: "", credential: "" },
  ollama: { provider: "ollama", label: "Ollama", base_url: "http://127.0.0.1:11434/", model: "", credential: "" },
};

export function SettingsPanel({ locale, onClose, modelProfiles = [], onSaveModel, onProbeModel, skillPackages = [], onImportSkill, onSetSkillEnabled }: Props) {
  const zh = locale === "zh-CN";
  const [form, setForm] = useState<FormState | null>(null);
  const [saving, setSaving] = useState(false);
  const [section, setSection] = useState<"models" | "skills">("models");
  const [skillsBusy, setSkillsBusy] = useState(false);
  const [skillError, setSkillError] = useState("");
  const configure = (provider: ModelProfile["provider"]) => setForm({ ...defaults[provider] });

  async function saveProvider() {
    if (!form || !onSaveModel) return;
    setSaving(true);
    try {
      await onSaveModel({ ...form, credential: form.provider === "ollama" ? undefined : form.credential });
      setForm(null);
    } finally {
      setSaving(false);
    }
  }

  async function importSkill() {
    if (!onImportSkill) return;
    setSkillsBusy(true);
    setSkillError("");
    try {
      await onImportSkill();
    } catch (error) {
      setSkillError(error instanceof Error ? error.message : String(error));
    } finally {
      setSkillsBusy(false);
    }
  }

  return <div className="settings-backdrop"><section className="settings-panel" role="dialog" aria-modal="true" aria-label={zh ? "工作台设置" : "Workspace settings"}>
    <header><div><small>OmicsOps Desktop</small><h2>{zh ? "工作台设置" : "Workspace settings"}</h2></div><button aria-label="Close" onClick={onClose}><X size={19} /></button></header>
    <div className="settings-layout">
      <nav><button className={section === "models" ? "active" : ""} onClick={() => setSection("models")}><Bot size={16} />{zh ? "模型提供方" : "Model providers"}</button><button><Server size={16} />{zh ? "远端计算" : "Remote compute"}</button><button className={section === "skills" ? "active" : ""} onClick={() => setSection("skills")}><Wrench size={16} />{zh ? "技能与 MCP" : "Skills and MCP"}</button><button><ShieldCheck size={16} />{zh ? "隐私与权限" : "Privacy and permissions"}</button><button><Archive size={16} />{zh ? "历史运行只读" : "Read-only legacy runs"}</button></nav>
      {section === "models" ? <main><div className="settings-heading"><h3>{zh ? "模型提供方" : "Model providers"}</h3><p>{zh ? "密钥保存在 Windows Credential Manager，项目只记录引用。" : "Keys stay in Windows Credential Manager; projects store references only."}</p></div>
        <div className="provider-grid">
          <Provider icon={Cloud} name="Anthropic" detail="Messages API · tool use" configured={modelProfiles.some((profile) => profile.provider === "anthropic")} onConfigure={() => configure("anthropic")} />
          <Provider icon={KeyRound} name="OpenAI-compatible" detail="Chat Completions · custom Base URL" configured={modelProfiles.some((profile) => profile.provider === "open_ai_compatible")} onConfigure={() => configure("open_ai_compatible")} />
          <Provider icon={Monitor} name="Ollama" detail={zh ? "本地模型 · 可设为项目强制策略" : "Local models · optional project-only policy"} configured={modelProfiles.some((profile) => profile.provider === "ollama")} onConfigure={() => configure("ollama")} />
        </div>
        {form && <section className="model-form" aria-label={zh ? "模型配置" : "Model configuration"}>
          <div className="model-form-grid">
            <label>{zh ? "配置名称" : "Profile label"}<input aria-label="Profile label" value={form.label} onChange={(event) => setForm({ ...form, label: event.target.value })} /></label>
            <label>{zh ? "模型" : "Model"}<input aria-label="Model" value={form.model} onChange={(event) => setForm({ ...form, model: event.target.value })} /></label>
            <label className="wide">Base URL<input aria-label="Base URL" value={form.base_url} onChange={(event) => setForm({ ...form, base_url: event.target.value })} /></label>
            {form.provider !== "ollama" && <label className="wide">API key<input aria-label="API key" type="password" autoComplete="new-password" value={form.credential} onChange={(event) => setForm({ ...form, credential: event.target.value })} /></label>}
          </div>
          <div className="model-form-actions"><button onClick={() => setForm(null)}>{zh ? "取消" : "Cancel"}</button><button className="primary" disabled={saving || !form.label.trim() || !form.model.trim()} onClick={saveProvider}>{zh ? "保存提供方" : "Save provider"}</button></div>
        </section>}
        {modelProfiles.length > 0 && <div className="configured-models">{modelProfiles.map((profile) => <div key={profile.id}><span><b>{profile.label}</b><small>{profile.model} · {profile.provider}</small></span><button onClick={() => onProbeModel?.(profile.id)}>{zh ? "测试" : "Test"}</button></div>)}</div>}
        <div className="settings-note"><ShieldCheck size={18} /><span><b>{zh ? "默认无遥测" : "Telemetry off by default"}</b><small>{zh ? "诊断包仅在主动导出时生成，并经过凭据脱敏。" : "Diagnostic bundles are generated only on export and redact credentials."}</small></span></div>
        <div className="legacy-row"><Archive size={18} /><span><b>{zh ? "历史运行只读" : "Read-only legacy runs"}</b><small>{zh ? "旧 V1/V2 计划、日志与审计可查看和导出，但不能启动新任务。" : "Legacy plans, logs, and audit records remain viewable and exportable."}</small></span><button>{zh ? "查看历史" : "View history"}</button></div>
      </main> : <main><div className="settings-heading skill-heading"><div><h3>{zh ? "科研技能" : "Research skills"}</h3><p>{zh ? "从 Agent Skills 兼容目录导入；来源哈希、版本和能力声明会被固定。" : "Import Agent Skills directories with pinned source hashes, versions, and capabilities."}</p></div><button className="skill-import" disabled={skillsBusy || !onImportSkill} onClick={() => void importSkill()}>{skillsBusy ? (zh ? "校验中…" : "Validating…") : (zh ? "导入技能目录" : "Import skill directory")}</button></div>
        {skillError && <div className="skill-error" role="alert">{skillError}</div>}
        <div className="skill-list">{skillPackages.length === 0 ? <div className="skill-empty">{zh ? "尚未导入科研技能" : "No research skills imported"}</div> : skillPackages.map((skill) => <article key={skill.id}><div className="skill-title"><span><b>{skill.name}</b><small>v{skill.version} · SHA-256 {skill.sha256.slice(0, 12)}</small></span><button disabled={skillsBusy || !onSetSkillEnabled} onClick={() => void onSetSkillEnabled?.(skill.id, !skill.enabled)}>{skill.enabled ? (zh ? "停用" : "Disable") : (zh ? "启用" : "Enable")}</button></div><div className="skill-capabilities">{skill.capabilities.length === 0 ? <em>{zh ? "无额外能力" : "No additional capabilities"}</em> : skill.capabilities.map((capability) => <em key={capability}>{capability}</em>)}</div><small className="skill-state">{skill.enabled ? (zh ? "已启用" : "Enabled") : (zh ? "已安装，等待启用" : "Installed, awaiting enablement")}</small></article>)}</div>
        <div className="settings-note"><ShieldCheck size={18} /><span><b>{zh ? "导入默认不启用" : "Imports are disabled by default"}</b><small>{zh ? "未知能力、路径穿越、逃逸符号链接和超限软件包会被拒绝。" : "Unknown capabilities, path traversal, escaping symlinks, and oversized packages are rejected."}</small></span></div>
      </main>}
    </div>
  </section></div>;
}

function Provider({ icon: Icon, name, detail, configured, onConfigure }: { icon: typeof Cloud; name: string; detail: string; configured: boolean; onConfigure: () => void }) {
  return <article><span><Icon size={19} /></span><div><b>{name}</b><small>{detail}{configured ? " · configured" : ""}</small></div><button aria-label={`Configure ${name}`} onClick={onConfigure}>Configure</button></article>;
}
