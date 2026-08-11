import { useState } from "react";
import { Archive, Bot, CheckCircle2, Cloud, KeyRound, LoaderCircle, Monitor, Server, ShieldCheck, Wrench, X, XCircle } from "lucide-react";
import type { ConnectionProfile, ConnectionTestResult, ModelProbeResult, ModelProfile, SkillPackage, WorkspaceProject } from "../../types";
import type { Locale } from "../workspace/copy";
import "./settings.css";
import "./model-form.css";
import "./remote-form.css";

type SaveModelRequest = { id?: string; label: string; provider: ModelProfile["provider"]; base_url: string; model: string; credential?: string };
type FormState = SaveModelRequest & { credential: string };

interface Props {
  locale: Locale;
  onClose: () => void;
  modelProfiles?: ModelProfile[];
  onSaveModel?: (request: SaveModelRequest) => Promise<void>;
  onProbeModel?: (profileId: string) => Promise<ModelProbeResult>;
  onListModels?: (profileId: string) => Promise<string[]>;
  skillPackages?: SkillPackage[];
  onImportSkill?: () => Promise<void>;
  onSetSkillEnabled?: (skillId: string, enabled: boolean) => Promise<SkillPackage>;
  connections?: ConnectionProfile[];
  selectedProject?: WorkspaceProject | null;
  onSaveConnection?: (profile: ConnectionProfile, secret: string) => Promise<void>;
  onTestConnection?: (profileId: string) => Promise<ConnectionTestResult>;
  onConfirmHostKey?: (profileId: string, fingerprint: string) => Promise<void>;
  onBindProjectRemote?: (connectionId: string, remoteRoot: string) => Promise<void>;
}

const defaults: Record<ModelProfile["provider"], FormState> = {
  anthropic: { provider: "anthropic", label: "Anthropic", base_url: "https://api.anthropic.com/", model: "", credential: "" },
  open_ai_compatible: { provider: "open_ai_compatible", label: "OpenAI-compatible", base_url: "https://api.openai.com/", model: "", credential: "" },
  ollama: { provider: "ollama", label: "Ollama", base_url: "http://127.0.0.1:11434/", model: "", credential: "" },
};

export function SettingsPanel({ locale, onClose, modelProfiles = [], onSaveModel, onProbeModel, onListModels, skillPackages = [], onImportSkill, onSetSkillEnabled, connections = [], selectedProject, onSaveConnection, onTestConnection, onConfirmHostKey, onBindProjectRemote }: Props) {
  const zh = locale === "zh-CN";
  const [form, setForm] = useState<FormState | null>(null);
  const [saving, setSaving] = useState(false);
  const [section, setSection] = useState<"models" | "remote" | "skills">("models");
  const [skillsBusy, setSkillsBusy] = useState(false);
  const [skillError, setSkillError] = useState("");
  const [modelTests, setModelTests] = useState<Record<string, { state: "testing" | "success" | "error"; result?: ModelProbeResult; message?: string }>>({});
  const [modelChoices, setModelChoices] = useState<Record<string, string[]>>({});
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

  async function testModel(profileId: string) {
    if (!onProbeModel) return;
    setModelTests((current) => ({ ...current, [profileId]: { state: "testing" } }));
    try {
      const result = await onProbeModel(profileId);
      setModelTests((current) => ({ ...current, [profileId]: { state: "success", result } }));
    } catch (error) {
      setModelTests((current) => ({ ...current, [profileId]: { state: "error", message: error instanceof Error ? error.message : String(error) } }));
    }
  }

  async function discoverModels(profileId: string) {
    if (!onListModels) return;
    try {
      const models = await onListModels(profileId);
      setModelChoices((current) => ({ ...current, [profileId]: models }));
    } catch (error) {
      setModelTests((current) => ({ ...current, [profileId]: { state: "error", message: error instanceof Error ? error.message : String(error) } }));
    }
  }

  return <div className="settings-backdrop"><section className="settings-panel" role="dialog" aria-modal="true" aria-label={zh ? "工作台设置" : "Workspace settings"}>
    <header><div><small>OmicsOps Desktop</small><h2>{zh ? "工作台设置" : "Workspace settings"}</h2></div><button aria-label="Close" onClick={onClose}><X size={19} /></button></header>
    <div className="settings-layout">
      <nav><button className={section === "models" ? "active" : ""} onClick={() => setSection("models")}><Bot size={16} />{zh ? "模型提供方" : "Model providers"}</button><button className={section === "remote" ? "active" : ""} onClick={() => setSection("remote")}><Server size={16} />{zh ? "远端计算" : "Remote compute"}</button><button className={section === "skills" ? "active" : ""} onClick={() => setSection("skills")}><Wrench size={16} />{zh ? "技能与 MCP" : "Skills and MCP"}</button><button><ShieldCheck size={16} />{zh ? "隐私与权限" : "Privacy and permissions"}</button><button><Archive size={16} />{zh ? "历史运行只读" : "Read-only legacy runs"}</button></nav>
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
        {modelProfiles.length > 0 && <div className="configured-models">{modelProfiles.map((profile) => { const probe = modelTests[profile.id]; const choices = modelChoices[profile.id] ?? []; return <div key={profile.id}><span><b>{profile.label}</b><small>{profile.model} · {profile.provider}</small></span><button onClick={() => setForm({ id: profile.id, label: profile.label, provider: profile.provider, base_url: profile.base_url, model: profile.model, credential: "" })}>{zh ? "编辑" : "Edit"}</button><button disabled={!onListModels} onClick={() => void discoverModels(profile.id)}>{zh ? "可用模型" : "Models"}</button><button disabled={probe?.state === "testing" || !onProbeModel} onClick={() => void testModel(profile.id)}>{probe?.state === "testing" ? <><LoaderCircle className="spin" size={13} />{zh ? "测试中" : "Testing"}</> : (zh ? "测试" : "Test")}</button>{choices.length > 0 && <div className="model-choices"><small>{zh ? "网关当前可用，点击后保存：" : "Available now; click to edit:"}</small>{choices.map((model) => <button key={model} onClick={() => setForm({ id: profile.id, label: profile.label, provider: profile.provider, base_url: profile.base_url, model, credential: "" })}>{model}</button>)}</div>}{probe?.state === "success" && probe.result && <div className="model-probe-result success" role="status"><CheckCircle2 size={15} /><span><b>{zh ? "连接成功" : "Connection succeeded"}</b><small>{probe.result.model} · {probe.result.latency_ms} ms · {probe.result.endpoint}</small><code>{probe.result.response_preview}</code></span></div>}{probe?.state === "error" && <div className="model-probe-result error" role="alert"><XCircle size={15} /><span><b>{zh ? "测试失败" : "Test failed"}</b><small>{probe.message}</small></span></div>}</div>; })}</div>}
        <div className="settings-note"><ShieldCheck size={18} /><span><b>{zh ? "默认无遥测" : "Telemetry off by default"}</b><small>{zh ? "诊断包仅在主动导出时生成，并经过凭据脱敏。" : "Diagnostic bundles are generated only on export and redact credentials."}</small></span></div>
        <div className="legacy-row"><Archive size={18} /><span><b>{zh ? "历史运行只读" : "Read-only legacy runs"}</b><small>{zh ? "旧 V1/V2 计划、日志与审计可查看和导出，但不能启动新任务。" : "Legacy plans, logs, and audit records remain viewable and exportable."}</small></span><button>{zh ? "查看历史" : "View history"}</button></div>
      </main> : section === "remote" ? <RemoteSettings locale={locale} connections={connections} selectedProject={selectedProject} onSave={onSaveConnection} onTest={onTestConnection} onConfirm={onConfirmHostKey} onBind={onBindProjectRemote} /> : <main><div className="settings-heading skill-heading"><div><h3>{zh ? "科研技能" : "Research skills"}</h3><p>{zh ? "从 Agent Skills 兼容目录导入；来源哈希、版本和能力声明会被固定。" : "Import Agent Skills directories with pinned source hashes, versions, and capabilities."}</p></div><button className="skill-import" disabled={skillsBusy || !onImportSkill} onClick={() => void importSkill()}>{skillsBusy ? (zh ? "校验中…" : "Validating…") : (zh ? "导入技能目录" : "Import skill directory")}</button></div>
        {skillError && <div className="skill-error" role="alert">{skillError}</div>}
        <div className="skill-list">{skillPackages.length === 0 ? <div className="skill-empty">{zh ? "尚未导入科研技能" : "No research skills imported"}</div> : skillPackages.map((skill) => <article key={skill.id}><div className="skill-title"><span><b>{skill.name}</b><small>v{skill.version} · SHA-256 {skill.sha256.slice(0, 12)}</small></span><button disabled={skillsBusy || !onSetSkillEnabled} onClick={() => void onSetSkillEnabled?.(skill.id, !skill.enabled)}>{skill.enabled ? (zh ? "停用" : "Disable") : (zh ? "启用" : "Enable")}</button></div><div className="skill-capabilities">{skill.capabilities.length === 0 ? <em>{zh ? "无额外能力" : "No additional capabilities"}</em> : skill.capabilities.map((capability) => <em key={capability}>{capability}</em>)}</div><small className="skill-state">{skill.enabled ? (zh ? "已启用" : "Enabled") : (zh ? "已安装，等待启用" : "Installed, awaiting enablement")}</small></article>)}</div>
        <div className="settings-note"><ShieldCheck size={18} /><span><b>{zh ? "导入默认不启用" : "Imports are disabled by default"}</b><small>{zh ? "未知能力、路径穿越、逃逸符号链接和超限软件包会被拒绝。" : "Unknown capabilities, path traversal, escaping symlinks, and oversized packages are rejected."}</small></span></div>
      </main>}
    </div>
  </section></div>;
}

function RemoteSettings({ locale, connections, selectedProject, onSave, onTest, onConfirm, onBind }: { locale: Locale; connections: ConnectionProfile[]; selectedProject?: WorkspaceProject | null; onSave?: Props["onSaveConnection"]; onTest?: Props["onTestConnection"]; onConfirm?: Props["onConfirmHostKey"]; onBind?: Props["onBindProjectRemote"] }) {
  const zh = locale === "zh-CN";
  const [form, setForm] = useState<{ id: string; label: string; host: string; port: number; username: string; authentication: ConnectionProfile["authentication"]; secret: string; keyPath: string; passphrase: string }>({ id: crypto.randomUUID(), label: "", host: "", port: 22, username: "", authentication: "password", secret: "", keyPath: "", passphrase: "" });
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [results, setResults] = useState<Record<string, ConnectionTestResult>>({});
  const [connectionId, setConnectionId] = useState(selectedProject?.connection_id ?? "");
  const [remoteRoot, setRemoteRoot] = useState(selectedProject?.remote_root ?? "");
  const editingExisting = connections.some((profile) => profile.id === form.id);
  const selectedConnection = connections.find((profile) => profile.id === connectionId);

  async function saveConnection() {
    if (!onSave) return;
    setBusy(true); setError("");
    try {
      const secret = form.authentication === "password" ? form.secret : form.keyPath.trim() ? JSON.stringify({ path: form.keyPath, passphrase: form.passphrase || null }) : "";
      await onSave({ id: form.id, label: form.label, host: form.host, port: form.port, username: form.username, authentication: form.authentication, authentication_reference: `ssh/${form.id}`, host_key_fingerprint: null }, secret);
      setForm((current) => ({ ...current, secret: "", passphrase: "" }));
    } catch (reason) { setError(reason instanceof Error ? reason.message : String(reason)); }
    finally { setBusy(false); }
  }

  async function test(profileId: string) {
    if (!onTest) return;
    setBusy(true); setError("");
    try {
      const result = await onTest(profileId);
      setResults((current) => ({ ...current, [profileId]: result }));
    }
    catch (reason) { setError(reason instanceof Error ? reason.message : String(reason)); }
    finally { setBusy(false); }
  }

  async function confirm(profileId: string, fingerprint: string) {
    if (!onConfirm) return;
    setBusy(true); setError("");
    try { await onConfirm(profileId, fingerprint); await test(profileId); }
    catch (reason) { setError(reason instanceof Error ? reason.message : String(reason)); setBusy(false); }
  }

  return <main className="remote-settings"><div className="settings-heading"><h3>{zh ? "远端 Linux 计算" : "Remote Linux compute"}</h3><p>{zh ? "首次连接只读取主机指纹；确认后才发送凭据。密码仅保存到 Windows Credential Manager。" : "The first connection reads only the host key. Credentials are sent only after confirmation and stay in Windows Credential Manager."}</p></div>
    <section className="remote-form"><div className="remote-grid"><label>{zh ? "名称" : "Label"}<input aria-label="SSH label" value={form.label} onChange={(event) => setForm({ ...form, label: event.target.value })} /></label><label>{zh ? "主机" : "Host"}<input aria-label="SSH host" value={form.host} onChange={(event) => setForm({ ...form, host: event.target.value })} /></label><label>{zh ? "端口" : "Port"}<input aria-label="SSH port" type="number" min="1" max="65535" value={form.port} onChange={(event) => setForm({ ...form, port: Number(event.target.value) })} /></label><label>{zh ? "用户名" : "Username"}<input aria-label="SSH username" value={form.username} onChange={(event) => setForm({ ...form, username: event.target.value })} /></label><label>{zh ? "认证" : "Authentication"}<select aria-label="SSH authentication" value={form.authentication} onChange={(event) => setForm({ ...form, authentication: event.target.value as ConnectionProfile["authentication"] })}><option value="password">{zh ? "密码" : "Password"}</option><option value="private_key">{zh ? "私钥" : "Private key"}</option></select></label>{form.authentication === "password" ? <label>{editingExisting ? (zh ? "新密码（留空则保持不变）" : "New password (leave blank to keep)") : (zh ? "密码" : "Password")}<input aria-label="SSH password" type="password" autoComplete="new-password" value={form.secret} onChange={(event) => setForm({ ...form, secret: event.target.value })} /></label> : <><label>{zh ? "私钥路径" : "Private key path"}<input aria-label="SSH private key path" value={form.keyPath} onChange={(event) => setForm({ ...form, keyPath: event.target.value })} /></label><label>{zh ? "私钥口令（可选）" : "Passphrase (optional)"}<input aria-label="SSH passphrase" type="password" value={form.passphrase} onChange={(event) => setForm({ ...form, passphrase: event.target.value })} /></label></>}</div><button className="primary" disabled={busy || !form.label.trim() || !form.host.trim() || !form.username.trim() || (!editingExisting && (form.authentication === "password" ? !form.secret : !form.keyPath.trim()))} onClick={() => void saveConnection()}>{zh ? "保存连接" : "Save connection"}</button></section>
    <div className="connection-list">{connections.map((profile) => { const result = results[profile.id]; return <article key={profile.id}><div><b>{profile.label}</b><small>{profile.username}@{profile.host}:{profile.port} · {profile.authentication}</small></div><div className="connection-actions"><button disabled={busy} onClick={() => setForm({ id: profile.id, label: profile.label, host: profile.host, port: profile.port, username: profile.username, authentication: profile.authentication, secret: "", keyPath: "", passphrase: "" })}>{zh ? "编辑" : "Edit"}</button><button disabled={busy || !onTest} onClick={() => void test(profile.id)}>{zh ? "测试连接" : "Test"}</button></div>{result && <div className="connection-result"><code>{result.fingerprint}</code><span>{result.trusted ? (zh ? "主机已信任" : "Host trusted") : (zh ? "等待确认主机指纹" : "Confirm host fingerprint")}</span>{!result.trusted && <button disabled={busy || !onConfirm} onClick={() => void confirm(profile.id, result.fingerprint)}>{zh ? "确认此指纹" : "Trust fingerprint"}</button>}{result.authenticated && <small>{result.serverOs} · {result.remoteUsername} · {result.latencyMs} ms · SFTP {result.sftpAvailable ? "✓" : "✗"} · Python {result.pythonAvailable ? "✓" : "✗"} · R {result.rAvailable ? "✓" : "✗"}</small>}</div>}</article>; })}</div>
    {selectedProject && <section className="remote-binding"><b>{zh ? `绑定项目：${selectedProject.name}` : `Bind project: ${selectedProject.name}`}</b><select aria-label="Project SSH connection" value={connectionId} onChange={(event) => setConnectionId(event.target.value)}><option value="">{zh ? "选择连接" : "Select connection"}</option>{connections.map((profile) => <option key={profile.id} value={profile.id}>{profile.label}{profile.host_key_fingerprint ? "" : (zh ? "（未信任）" : " (untrusted)")}</option>)}</select><input aria-label="Remote project root" placeholder="/home/user/omicsops/project" value={remoteRoot} onChange={(event) => setRemoteRoot(event.target.value)} /><button disabled={busy || !selectedConnection?.host_key_fingerprint || !remoteRoot.startsWith("/") || !onBind} onClick={() => void onBind?.(connectionId, remoteRoot)}>{zh ? "保存项目绑定" : "Save project binding"}</button>{selectedConnection && !selectedConnection.host_key_fingerprint && <small>{zh ? "请先测试并确认该服务器的主机指纹。" : "Test and trust this server's host fingerprint first."}</small>}</section>}
    {error && <div className="skill-error" role="alert">{error}</div>}
  </main>;
}

function Provider({ icon: Icon, name, detail, configured, onConfigure }: { icon: typeof Cloud; name: string; detail: string; configured: boolean; onConfigure: () => void }) {
  return <article><span><Icon size={19} /></span><div><b>{name}</b><small>{detail}{configured ? " · configured" : ""}</small></div><button aria-label={`Configure ${name}`} onClick={onConfigure}>Configure</button></article>;
}
