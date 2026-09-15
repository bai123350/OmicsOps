import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Bot, CheckCircle2, Cloud, FolderOpen, Globe2, KeyRound, Layers3, LoaderCircle, Monitor, Server, ShieldCheck, Wrench, X, XCircle } from "lucide-react";
import type { ConnectionProfile, ConnectionTestResult, McpEnvBinding, McpServerProfile, ModelProbeResult, ModelProfile, SkillPackage, WorkspaceProject } from "../../types";
import type { Locale } from "../workspace/copy";
import { supportsFastMode } from "../../fast-mode";
import { BrowserSettings, useWindowEscapeLayer } from "./BrowserSettings";
import { BundledMcpPresets, type BundledMcpProps } from "./BundledMcpPresets";
import { AgentSettings } from "./AgentSettings";
import "./settings.css";
import "./model-form.css";
import "./remote-form.css";

type SaveModelRequest = { id?: string; label: string; provider: ModelProfile["provider"]; base_url: string; model: string; credential?: string; refresh_catalog?: boolean; reasoning_effort?: ModelProfile["reasoning_effort"]; fast_mode?: ModelProfile["fast_mode"]; delegated_model_profile_id?: string | null };
type FormState = SaveModelRequest & { credential: string };
type McpEnvFormBinding = McpEnvBinding & { mode: "literal" | "credential" };
type SaveMcpServerRequest = { id?: string; name: string; command: string; args: string[]; cwd?: string | null; timeout_secs?: number | null; env_bindings?: McpEnvBinding[] };

export type SettingsSection = "models" | "remote" | "skills" | "browser" | "agent" | "privacy";

interface Props extends BundledMcpProps {
  locale: Locale;
  initialSection?: SettingsSection;
  onClose: () => void;
  modelProfiles?: ModelProfile[];
  onSaveModel?: (request: SaveModelRequest) => Promise<void>;
  onProbeModel?: (profileId: string) => Promise<ModelProbeResult>;
  onListModels?: (profileId: string) => Promise<string[]>;
  skillPackages?: SkillPackage[];
  onImportSkill?: () => Promise<void>;
  onSetSkillEnabled?: (skillId: string, enabled: boolean) => Promise<SkillPackage>;
  mcpServers?: McpServerProfile[];
  onSaveMcpServer?: (request: SaveMcpServerRequest) => Promise<McpServerProfile>;
  onInspectMcpServer?: (serverId: string) => Promise<void>;
  onSetMcpServerEnabled?: (serverId: string, enabled: boolean) => Promise<McpServerProfile>;
  onSetMcpLaunchApproval?: (serverId: string, approved: boolean) => Promise<McpServerProfile>;
  onSetMcpToolApproval?: (serverId: string, tool: string, approved: boolean) => Promise<McpServerProfile>;
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

const deepSeekDefault: FormState = { provider: "open_ai_compatible", label: "DeepSeek", base_url: "https://api.deepseek.com/v1", model: "deepseek-v4-flash", credential: "" };
const deepSeekModels = ["deepseek-v4-flash", "deepseek-v4-pro", "deepseek-v4-flash-vision-exp"];

function isDeepSeek(profile: Pick<ModelProfile, "provider" | "base_url">): boolean {
  if (profile.provider !== "open_ai_compatible") return false;
  try {
    const url = new URL(profile.base_url);
    return url.protocol === "https:" && url.hostname === "api.deepseek.com" && url.port === "";
  } catch {
    return false;
  }
}

export function SettingsPanel({ locale, initialSection = "models", onClose, modelProfiles = [], onSaveModel, onProbeModel, onListModels, skillPackages = [], onImportSkill, onSetSkillEnabled, mcpServers = [], onSaveMcpServer, onInspectMcpServer, onSetMcpServerEnabled, onSetMcpLaunchApproval, onSetMcpToolApproval, onListBundledMcpPresets, onConfigurePubMedMcp, onAddBundledMcp, connections = [], selectedProject, onSaveConnection, onTestConnection, onConfirmHostKey, onBindProjectRemote }: Props) {
  const zh = locale === "zh-CN";
  const [form, setForm] = useState<FormState | null>(null);
  const [saving, setSaving] = useState(false);
  const [modelSaveError, setModelSaveError] = useState("");
  const [section, setSection] = useState<SettingsSection>(initialSection);
  const [skillsBusy, setSkillsBusy] = useState(false);
  const [skillError, setSkillError] = useState("");
  const [modelTests, setModelTests] = useState<Record<string, { state: "testing" | "success" | "error"; result?: ModelProbeResult; message?: string }>>({});
  const [modelChoices, setModelChoices] = useState<Record<string, string[]>>({});
  const fastModeAvailable = form ? supportsFastMode(form) : false;
  useWindowEscapeLayer(true, onClose);
  useWindowEscapeLayer(form !== null && section === "models", () => setForm(null));
  const configure = (provider: ModelProfile["provider"]) => setForm({ ...defaults[provider] });
  const navigate = (nextSection: SettingsSection) => {
    setForm(null);
    setSection(nextSection);
  };

  async function saveProvider() {
    if (!form || !onSaveModel) return;
    setSaving(true);
    setModelSaveError("");
    try {
      await onSaveModel({ ...form, credential: form.provider === "ollama" ? undefined : form.credential });
      setForm(null);
    } catch {
      setModelSaveError(zh ? "保存失败，请检查模型配置后重试。" : "Could not save. Check the model configuration and retry.");
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
      <nav aria-label={zh ? "设置页面" : "Settings pages"}><button aria-current={section === "agent" ? "page" : undefined} className={section === "agent" ? "active" : ""} onClick={() => navigate("agent")}><Bot size={16} />{zh ? "会话" : "Session"}</button><button aria-current={section === "models" ? "page" : undefined} className={section === "models" ? "active" : ""} onClick={() => navigate("models")}><Bot size={16} />{zh ? "模型提供方" : "Model providers"}</button><button aria-current={section === "remote" ? "page" : undefined} className={section === "remote" ? "active" : ""} onClick={() => navigate("remote")}><Server size={16} />{zh ? "远端计算" : "Remote compute"}</button><button aria-current={section === "skills" ? "page" : undefined} className={section === "skills" ? "active" : ""} onClick={() => navigate("skills")}><Wrench size={16} />{zh ? "技能与 MCP" : "Skills and MCP"}</button><button aria-current={section === "browser" ? "page" : undefined} className={section === "browser" ? "active" : ""} onClick={() => navigate("browser")}><Globe2 size={16} />{zh ? "浏览器" : "Browser"}</button><button aria-current={section === "privacy" ? "page" : undefined} className={section === "privacy" ? "active" : ""} onClick={() => navigate("privacy")}><ShieldCheck size={16} />{zh ? "隐私与权限" : "Privacy and permissions"}</button></nav>
      {section === "models" ? <main><div className="settings-heading"><h3>{zh ? "模型提供方" : "Model providers"}</h3><p>{zh ? "密钥保存在 Windows Credential Manager，项目只记录引用。" : "Keys stay in Windows Credential Manager; projects store references only."}</p></div>
        <div className="provider-grid">
          <Provider icon={Cloud} name="Anthropic" detail="Messages API · tool use" configured={modelProfiles.some((profile) => profile.provider === "anthropic")} onConfigure={() => configure("anthropic")} />
          <Provider icon={KeyRound} name="OpenAI-compatible" detail="Chat Completions · custom Base URL" configured={modelProfiles.some((profile) => profile.provider === "open_ai_compatible")} onConfigure={() => configure("open_ai_compatible")} />
          <Provider icon={Cloud} name="DeepSeek" detail={zh ? "官方 API · Flash / Pro" : "Official API · Flash / Pro"} configured={modelProfiles.some(isDeepSeek)} onConfigure={() => setForm({ ...deepSeekDefault })} />
          <Provider icon={Monitor} name="Ollama" detail={zh ? "本地模型 · Ollama API" : "Local models · Ollama API"} configured={modelProfiles.some((profile) => profile.provider === "ollama")} onConfigure={() => configure("ollama")} />
        </div>
        {form && <section className="model-form" aria-label={zh ? "模型配置" : "Model configuration"}>
          <div className="model-form-grid">
            <label>{zh ? "配置名称" : "Profile label"}<input aria-label="Profile label" value={form.label} onChange={(event) => setForm({ ...form, label: event.target.value })} /></label>
            <label>{zh ? "模型" : "Model"}<input aria-label="Model" list={isDeepSeek(form) ? "deepseek-models" : undefined} value={form.model} onChange={(event) => setForm({ ...form, model: event.target.value })} />{isDeepSeek(form) && <><datalist id="deepseek-models">{deepSeekModels.map((model) => <option key={model} value={model} />)}</datalist><small>{zh ? "可选择预设或输入模型 ID；保存后可查询当前可用模型。" : "Choose a preset or enter a model ID; discover available models after saving."}</small></>}</label>
            <label className="wide">Base URL<input aria-label="Base URL" value={form.base_url} onChange={(event) => setForm({ ...form, base_url: event.target.value })} /></label>
            {form.provider !== "ollama" && <label className="wide">API key<input aria-label="API key" type="password" autoComplete="new-password" value={form.credential} onChange={(event) => setForm({ ...form, credential: event.target.value })} /></label>}
            {form.provider === "open_ai_compatible" && <label className="wide">{zh ? "请求推理档位" : "Requested reasoning effort"}<select aria-label="Requested reasoning effort" value={form.reasoning_effort ?? ""} onChange={(event) => setForm({ ...form, reasoning_effort: (event.target.value || null) as ModelProfile["reasoning_effort"] })}>
              <option value="">{zh ? "服务端默认" : "Provider default"}</option>
              {["none", "minimal", "low", "medium", "high", "xhigh", "max", "ultra"].map((effort) => <option key={effort} value={effort}>{effort}</option>)}
            </select><small>{zh ? "按所选值发送，不自动降档。测试可检查请求是否被接受，不能确认实际生效档位。" : "Sent as selected, with no automatic downgrade. Test checks request acceptance, not the effective effort."}</small></label>}
            <label className="wide">{zh ? "Fast 模式" : "Fast mode"}<select aria-label={zh ? "Fast 模式" : "Fast mode"} value={form.fast_mode === true ? "fast" : form.fast_mode === false ? "standard" : "default"} onChange={(event) => setForm({ ...form, fast_mode: event.target.value === "fast" ? true : event.target.value === "standard" ? false : null })}>
              <option value="default">{zh ? "模型默认" : "Model default"}</option>
              <option value="standard">{zh ? "标准" : "Standard"}</option>
              <option value="fast" disabled={!fastModeAvailable && form.fast_mode !== true}>Fast</option>
            </select><small>{zh ? "请求 Fast 处理；可用性取决于提供方。" : "Requests Fast processing; availability depends on the provider."}</small></label>
            {form.id && <label className="wide"><span><input type="checkbox" aria-label="Refresh catalog capabilities" checked={form.refresh_catalog ?? false} onChange={(event) => setForm({ ...form, refresh_catalog: event.target.checked })} />{zh ? "保存时采用当前目录能力" : "Adopt current catalog capabilities on save"}</span><small>{zh ? "更新能力快照并使用目录上下文额度。保留请求推理档位；能力或预算变化可能使旧运行无法恢复。目录未收录的模型无法刷新。" : "Updates the capability snapshot and uses the catalog context allowance. Keeps requested effort; capability or budget changes may prevent old runs from resuming. Requires an exact catalog entry."}</small></label>}
            <label className="wide">{zh ? "只读子 Agent 模型" : "Read-only subagent model"}<select aria-label="Read-only subagent model" value={form.delegated_model_profile_id ?? ""} onChange={(event) => setForm({ ...form, delegated_model_profile_id: event.target.value || null })}>
              <option value="">{zh ? "沿用主模型" : "Inherit main model"}</option>
              {modelProfiles.filter((profile) => profile.id !== form.id && profile.supports_tools).map((profile) => <option key={profile.id} value={profile.id}>{profile.label} · {profile.model}</option>)}
              {form.delegated_model_profile_id && !modelProfiles.some((profile) => profile.id === form.delegated_model_profile_id && profile.supports_tools) && <option value={form.delegated_model_profile_id}>{zh ? "配置不可用，请重新选择" : "Profile unavailable; select again"}</option>}
            </select><small>{zh ? "用于新建普通 Agent 任务的只读委派；运行中任务保留原配置。" : "Used for read-only delegation in new ordinary Agent runs; existing runs keep their configuration."}</small></label>
          </div>
          {modelSaveError && <p role="alert">{modelSaveError}</p>}
          <div className="model-form-actions"><button onClick={() => setForm(null)}>{zh ? "取消" : "Cancel"}</button><button className="primary" disabled={saving || !form.label.trim() || !form.model.trim()} onClick={saveProvider}>{zh ? "保存提供方" : "Save provider"}</button></div>
        </section>}
        {modelProfiles.length > 0 && <div className="configured-models">{modelProfiles.map((profile) => { const probe = modelTests[profile.id]; const choices = modelChoices[profile.id] ?? []; const editProfile = (model: string) => setForm({ id: profile.id, label: profile.label, provider: profile.provider, base_url: profile.base_url, model, credential: "", reasoning_effort: profile.reasoning_effort ?? null, delegated_model_profile_id: profile.delegated_model_profile_id ?? null, ...(profile.fast_mode !== undefined ? { fast_mode: profile.fast_mode } : {}) }); return <div key={profile.id}><span><b>{profile.label}</b><small>{profile.model} · {profile.provider}</small>{profile.catalog_capabilities && <small>{zh ? "目录快照" : "Catalog snapshot"} (models.dev / {profile.catalog_capabilities.source_provider}) · {zh ? "上下文上限" : "Context limit"} {profile.catalog_capabilities.context_limit} · {zh ? "输出上限" : "Output limit"} {profile.catalog_capabilities.output_limit}</small>}</span><button onClick={() => editProfile(profile.model)}>{zh ? "编辑" : "Edit"}</button><button disabled={!onListModels} onClick={() => void discoverModels(profile.id)}>{zh ? "可用模型" : "Models"}</button><button disabled={probe?.state === "testing" || !onProbeModel} onClick={() => void testModel(profile.id)}>{probe?.state === "testing" ? <><LoaderCircle className="spin" size={13} />{zh ? "测试中" : "Testing"}</> : (zh ? "测试" : "Test")}</button>{choices.length > 0 && <div className="model-choices"><small>{zh ? "网关当前可用，点击后保存：" : "Available now; click to edit:"}</small>{choices.map((model) => <button key={model} onClick={() => editProfile(model)}>{model}</button>)}</div>}{probe?.state === "success" && probe.result && <div className="model-probe-result success" role="status"><CheckCircle2 size={15} /><span><b>{zh ? "连接成功" : "Connection succeeded"}</b><small>{probe.result.model} · {probe.result.latency_ms} ms · {probe.result.endpoint}</small><code>{probe.result.response_preview}</code></span></div>}{probe?.state === "error" && <div className="model-probe-result error" role="alert"><XCircle size={15} /><span><b>{zh ? "测试失败" : "Test failed"}</b><small>{probe.message}</small></span></div>}</div>; })}</div>}
        <div className="settings-note"><ShieldCheck size={18} /><span><b>{zh ? "默认无遥测" : "Telemetry off by default"}</b><small>{zh ? "诊断包仅在主动导出时生成，并经过凭据脱敏。" : "Diagnostic bundles are generated only on export and redact credentials."}</small></span></div>
      </main> : section === "remote" ? <RemoteSettings locale={locale} connections={connections} selectedProject={selectedProject} onSave={onSaveConnection} onTest={onTestConnection} onConfirm={onConfirmHostKey} onBind={onBindProjectRemote} /> : section === "skills" ? <SkillsAndMcpSettings locale={locale} skillPackages={skillPackages} skillsBusy={skillsBusy} skillError={skillError} onImportSkill={onImportSkill ? importSkill : undefined} onSetSkillEnabled={onSetSkillEnabled} mcpServers={mcpServers} selectedProject={selectedProject} onSaveMcpServer={onSaveMcpServer} onInspectMcpServer={onInspectMcpServer} onSetMcpServerEnabled={onSetMcpServerEnabled} onSetMcpLaunchApproval={onSetMcpLaunchApproval} onSetMcpToolApproval={onSetMcpToolApproval} onListBundledMcpPresets={onListBundledMcpPresets} onAddBundledMcp={onAddBundledMcp} onConfigurePubMedMcp={onConfigurePubMedMcp} /> : section === "agent" ? <AgentSettings locale={locale} /> : section === "browser" ? <BrowserSettings locale={locale} /> : <PrivacySettings locale={locale} selectedProject={selectedProject} onNavigate={navigate} />}
    </div>
  </section></div>;
}

function PrivacySettings({ locale, selectedProject, onNavigate }: { locale: Locale; selectedProject?: WorkspaceProject | null; onNavigate: (section: SettingsSection) => void }) {
  const zh = locale === "zh-CN";
  return <main className="privacy-settings">
    <div className="settings-heading"><h3>{zh ? "隐私与权限" : "Privacy and permissions"}</h3><p>{zh ? "查看数据可能离开设备的边界，以及实际执行权限由谁裁决。" : "Review when data may leave the device and who decides execution permissions."}</p></div>
    <div className="privacy-grid">
      <article><Globe2 size={18} /><div><b>{zh ? "外部传输" : "External transfers"}</b><p>{zh ? "模型、MCP 或外部服务可能接收提示词、结果或元数据；浏览器访问的网站也会接收正常网页请求。请在启用和调用前检查目标服务。" : "Models, MCP servers, or external services may receive prompts, results, or metadata. Websites opened in the browser also receive normal web requests. Review the destination before enabling or using it."}</p><div className="privacy-actions"><button onClick={() => onNavigate("skills")}>{zh ? "管理技能与 MCP" : "Manage Skills and MCP"}</button><button onClick={() => onNavigate("browser")}>{zh ? "管理浏览器" : "Manage browser"}</button></div></div></article>
      <article><KeyRound size={18} /><div><b>{zh ? "凭据存储" : "Credential storage"}</b><p>{zh ? "API key、密码和凭据由现有 Windows Credential Manager 或系统 keyring 保存；SQLite、日志、模型上下文和项目导出只保存引用或脱敏信息。" : "API keys, passwords, and credentials use the existing Windows Credential Manager or system keyring. SQLite, logs, model context, and project exports keep references or redacted information only."}</p><button onClick={() => onNavigate("models")}>{zh ? "管理模型提供方" : "Manage model providers"}</button></div></article>
      <article><FolderOpen size={18} /><div><b>{zh ? "本地与远端数据" : "Local and remote data"}</b><p>{zh ? "大型远端数据不会默认完整同步到本地。项目可以保存远端引用、校验和与元数据；文件传输需由你明确发起或选择同步范围。" : "Large remote datasets are not fully synchronized by default. A project can keep remote references, checksums, and metadata; file transfer requires an explicit action or selected sync scope."}</p>{!selectedProject && <small>{zh ? "当前未打开项目；打开项目后可管理它的远端计算与同步范围。" : "No project is open. Open one to manage its remote compute and sync scope."}</small>}<button onClick={() => onNavigate("remote")}>{zh ? "管理远端计算" : "Manage remote compute"}</button></div></article>
      <article><ShieldCheck size={18} /><div><b>{zh ? "权限与远端运行" : "Permissions and remote runs"}</b><p>{zh ? "宿主应用负责执行能力和审批裁决；模型、Skills 与 MCP 不能绕过批准或扩大授权。停止 Agent 不会取消已经派发的远端计算，请在对应运行环境中单独确认作业状态。" : "The host application decides execution capabilities and approvals. Models, Skills, and MCP cannot bypass approval or broaden authorization. Stopping the Agent does not cancel remote computation that was already dispatched; confirm job state in its execution environment."}</p></div></article>
    </div>
  </main>;
}

function emptyMcpForm() {
  return { name: "", command: "", args: "", cwd: "", timeout_secs: 60, env_bindings: [] as McpEnvFormBinding[] };
}

function SkillsAndMcpSettings({ locale, skillPackages, skillsBusy, skillError, onImportSkill, onSetSkillEnabled, mcpServers, selectedProject, onSaveMcpServer, onInspectMcpServer, onSetMcpServerEnabled, onSetMcpLaunchApproval, onSetMcpToolApproval, onListBundledMcpPresets, onConfigurePubMedMcp, onAddBundledMcp }: BundledMcpProps & { locale: Locale; skillPackages: SkillPackage[]; skillsBusy: boolean; skillError: string; onImportSkill?: () => Promise<void>; onSetSkillEnabled?: Props["onSetSkillEnabled"]; mcpServers: McpServerProfile[]; selectedProject?: WorkspaceProject | null; onSaveMcpServer?: Props["onSaveMcpServer"]; onInspectMcpServer?: Props["onInspectMcpServer"]; onSetMcpServerEnabled?: Props["onSetMcpServerEnabled"]; onSetMcpLaunchApproval?: Props["onSetMcpLaunchApproval"]; onSetMcpToolApproval?: Props["onSetMcpToolApproval"] }) {
  const zh = locale === "zh-CN";
  const [mcpForm, setMcpForm] = useState<{ id?: string; name: string; command: string; args: string; cwd: string; timeout_secs: number; env_bindings: McpEnvFormBinding[] }>(emptyMcpForm());
  const [mcpBusy, setMcpBusy] = useState<string | null>(null);
  const [mcpError, setMcpError] = useState("");
  const [inspectionApprovals, setInspectionApprovals] = useState<Record<string, boolean>>({});
  const categorizedSkills = skillPackages.filter((skill) => skill.category);
  const ungroupedSkills = skillPackages.filter((skill) => !skill.category);
  const skillGroups = Array.from(new Set(categorizedSkills.map((skill) => skill.category!)))
    .sort()
    .map((category) => [category, categorizedSkills.filter((skill) => skill.category === category)] as const);

  async function saveServer() {
    if (!onSaveMcpServer) return;
    setMcpBusy("save"); setMcpError("");
    try {
      const env_bindings: McpEnvBinding[] = mcpForm.env_bindings.map((binding): McpEnvBinding => ({
        name: binding.name.trim(),
        ...(binding.mode === "credential"
          ? { credential_reference: binding.credential_reference?.trim() || null }
          : { value: binding.value ?? "" }),
      })).filter((binding) => binding.name.length > 0);
      if (mcpForm.env_bindings.some((binding) => !binding.name.trim())) {
        throw new Error(zh ? "环境变量名称不能为空。" : "Environment variable names cannot be empty.");
      }
      if (env_bindings.some((binding) => !binding.value && !binding.credential_reference)) {
        throw new Error(zh ? "请为每个环境变量填写值或凭据引用。" : "Provide a value or credential reference for every environment variable.");
      }
      if (!Number.isInteger(mcpForm.timeout_secs) || mcpForm.timeout_secs < 1 || mcpForm.timeout_secs > 3600) {
        throw new Error(zh ? "超时必须是 1–3600 秒。" : "Timeout must be between 1 and 3600 seconds.");
      }
      const request: SaveMcpServerRequest = {
        ...(mcpForm.id ? { id: mcpForm.id } : {}),
        name: mcpForm.name,
        command: mcpForm.command,
        args: mcpForm.args.split(/\r?\n/).map((value) => value.trim()).filter(Boolean),
      };
      // Keep the original request shape for new installations whose host has
      // not yet migrated the advanced MCP columns. Existing profiles send the
      // explicit values so clearing an advanced field is possible.
      if (mcpForm.id || mcpForm.cwd.trim()) request.cwd = mcpForm.cwd.trim() || null;
      if (mcpForm.id || mcpForm.timeout_secs !== 60) request.timeout_secs = mcpForm.timeout_secs;
      if (mcpForm.id || env_bindings.length > 0) request.env_bindings = env_bindings;
      await onSaveMcpServer(request);
      setMcpForm(emptyMcpForm());
    } catch (reason) { setMcpError(reason instanceof Error ? reason.message : String(reason)); }
    finally { setMcpBusy(null); }
  }

  async function runMcpAction(key: string, action: () => Promise<unknown>) {
    setMcpBusy(key); setMcpError("");
    try { await action(); }
    catch (reason) { setMcpError(reason instanceof Error ? reason.message : String(reason)); }
    finally { setMcpBusy(null); }
  }

  function editMcpServer(server: McpServerProfile) {
    setMcpForm({
      id: server.id,
      name: server.name,
      command: server.command,
      args: server.args.join("\n"),
      cwd: server.cwd ?? "",
      timeout_secs: server.timeout_secs ?? 60,
      // Literal values are intentionally not loaded into the webview. The
      // host may keep them, or the user can enter a replacement value.
      env_bindings: (server.env_bindings ?? []).map((binding) => ({
        name: binding.name,
        mode: binding.credential_reference ? "credential" : "literal",
        value: "",
        credential_reference: binding.credential_reference ?? "",
      })),
    });
  }

  function addEnvBinding() {
    setMcpForm((current) => ({ ...current, env_bindings: [...current.env_bindings, { name: "", mode: "credential", credential_reference: "" }] }));
  }

  function updateEnvBinding(index: number, update: Partial<McpEnvFormBinding>) {
    setMcpForm((current) => ({ ...current, env_bindings: current.env_bindings.map((binding, candidate) => candidate === index ? { ...binding, ...update } : binding) }));
  }

  function removeEnvBinding(index: number) {
    setMcpForm((current) => ({ ...current, env_bindings: current.env_bindings.filter((_, candidate) => candidate !== index) }));
  }

  return <main className="skills-mcp-settings">
    <div className="settings-heading skill-heading"><div><h3>{zh ? "科研 Skills" : "Research Skills"}</h3><p>{zh ? "随应用提供的科研 Skills 来自固定 GitHub 快照；Agent 会读取已启用 Skill 及其依赖、示例和使用指南，并据此生成分析代码。也可以导入其他 Agent Skills 兼容目录。" : "Bundled research Skills come from a pinned GitHub snapshot. The Agent reads enabled Skills, dependencies, examples, and usage guides to generate analysis code. Other Agent Skills directories can also be imported."}</p></div><button className="skill-import" disabled={skillsBusy || !onImportSkill} onClick={() => void onImportSkill?.()}>{skillsBusy ? (zh ? "校验中…" : "Validating…") : (zh ? "导入技能目录" : "Import skill directory")}</button></div>
    {skillError && <div className="skill-error" role="alert">{skillError}</div>}
    {skillPackages.length === 0 ? <div className="skill-empty skill-library-empty">{zh ? "尚未导入科研技能" : "No research skills imported"}</div> : <div className="skill-library">
      {skillGroups.length > 0 && <section className="skill-domain" aria-label={zh ? "组学技能" : "Omics skills"}>
        <div className="skill-domain-title"><Layers3 size={18} /><span><b>{zh ? "组学技能" : "Omics skills"}</b><small>{zh ? `${skillGroups.length} 个分区 · ${categorizedSkills.length} Skills` : `${skillGroups.length} categories · ${categorizedSkills.length} Skills`}</small></span></div>
        <div className="skill-category-list">{skillGroups.map(([category, skills]) => <details className="skill-category" key={category} open>
          <summary><FolderOpen size={17} /><span><b>{skillCategoryLabel(category, zh)}</b><small>{skills.length} Skills · {skills.filter((skill) => skill.enabled).length} {zh ? "已启用" : "enabled"}</small></span></summary>
          <div className="skill-list">{skills.map((skill) => <SkillCard key={skill.id} skill={skill} zh={zh} skillsBusy={skillsBusy} onSetSkillEnabled={onSetSkillEnabled} />)}</div>
        </details>)}</div>
      </section>}
      {ungroupedSkills.length > 0 && <section className="skill-domain ungrouped" aria-label={zh ? "未分组技能" : "Uncategorized skills"}>
        <div className="skill-domain-title"><FolderOpen size={18} /><span><b>{zh ? "未分组技能" : "Uncategorized skills"}</b><small>{zh ? "手动导入或尚未分类" : "Imported manually or not yet categorized"}</small></span></div>
        <div className="skill-list">{ungroupedSkills.map((skill) => <SkillCard key={skill.id} skill={skill} zh={zh} skillsBusy={skillsBusy} onSetSkillEnabled={onSetSkillEnabled} />)}</div>
      </section>}
    </div>}

    <div className="mcp-divider" />
    <div className="settings-heading"><h3>{zh ? "MCP servers" : "MCP servers"}</h3><p>{zh ? "配置本地 stdio server，先显式批准一次检查，再逐个批准可调用的工具。保存配置不会启动进程。" : "Configure local stdio servers, explicitly approve inspection, then approve callable tools one by one. Saving never launches a process."}</p></div>
    <BundledMcpPresets zh={zh} busy={mcpBusy !== null} runAction={runMcpAction} onListBundledMcpPresets={onListBundledMcpPresets} onAddBundledMcp={onAddBundledMcp} onConfigurePubMedMcp={onConfigurePubMedMcp} />

    <section className="mcp-form" aria-label={zh ? "MCP server 配置" : "MCP server configuration"}>
      <div className="mcp-form-grid"><label>{zh ? "名称" : "Name"}<input aria-label="MCP server name" value={mcpForm.name} onChange={(event) => setMcpForm({ ...mcpForm, name: event.target.value })} /></label><label>{zh ? "启动命令" : "Command"}<input aria-label="MCP server command" placeholder="npx" value={mcpForm.command} onChange={(event) => setMcpForm({ ...mcpForm, command: event.target.value })} /></label><label>{zh ? "工作目录（可选）" : "Working directory (optional)"}<input aria-label="MCP working directory" placeholder={zh ? "继承项目目录" : "Inherit project directory"} value={mcpForm.cwd} onChange={(event) => setMcpForm({ ...mcpForm, cwd: event.target.value })} /></label><label>{zh ? "超时（秒）" : "Timeout (seconds)"}<input aria-label="MCP timeout seconds" type="number" min={1} max={3600} step={1} value={mcpForm.timeout_secs} onChange={(event) => setMcpForm({ ...mcpForm, timeout_secs: Number(event.target.value) })} /></label><label className="wide">{zh ? "参数（每行一个）" : "Arguments (one per line)"}<textarea aria-label="MCP server arguments" rows={3} placeholder={"-y\n@modelcontextprotocol/server-filesystem\nE:\\Science\\project"} value={mcpForm.args} onChange={(event) => setMcpForm({ ...mcpForm, args: event.target.value })} /></label></div>
      <div className="mcp-env-bindings"><div className="mcp-subheading"><span><b>{zh ? "环境变量绑定" : "Environment bindings"}</b><small>{zh ? "建议使用凭据引用；literal 值不会显示在已保存的 server 卡片中。" : "Credential references are preferred; literal values are never shown on saved server cards."}</small></span><button type="button" onClick={addEnvBinding}>{zh ? "添加变量" : "Add variable"}</button></div>{mcpForm.env_bindings.length === 0 ? <small className="mcp-muted">{zh ? "未配置环境变量" : "No environment variables configured"}</small> : mcpForm.env_bindings.map((binding, index) => <div className="mcp-env-row" key={`${index}-${binding.name}`}><input aria-label={`MCP env name ${index + 1}`} placeholder="NCBI_API_KEY" value={binding.name} onChange={(event) => updateEnvBinding(index, { name: event.target.value })} /><select aria-label={`MCP env mode ${index + 1}`} value={binding.mode} onChange={(event) => updateEnvBinding(index, { mode: event.target.value as McpEnvFormBinding["mode"], value: "", credential_reference: "" })}><option value="credential">{zh ? "凭据引用" : "Credential reference"}</option><option value="literal">literal</option></select>{binding.mode === "credential" ? <input aria-label={`MCP credential reference ${index + 1}`} placeholder="ncbi/api-key" value={binding.credential_reference ?? ""} onChange={(event) => updateEnvBinding(index, { credential_reference: event.target.value })} /> : <input aria-label={`MCP literal value ${index + 1}`} type="password" autoComplete="new-password" placeholder={zh ? "仅在保存时提交" : "Sent only when saved"} value={binding.value ?? ""} onChange={(event) => updateEnvBinding(index, { value: event.target.value })} />}<button type="button" aria-label={`${zh ? "移除环境变量" : "Remove environment variable"} ${index + 1}`} onClick={() => removeEnvBinding(index)}>×</button></div>)}</div>
      <div className="mcp-form-actions">{mcpForm.id && <button onClick={() => setMcpForm(emptyMcpForm())}>{zh ? "取消编辑" : "Cancel edit"}</button>}<button className="primary" disabled={mcpBusy !== null || !onSaveMcpServer || !mcpForm.name.trim() || !mcpForm.command.trim()} onClick={() => void saveServer()}>{zh ? "保存 MCP server" : "Save MCP server"}</button></div>
    </section>
    {mcpError && <div className="skill-error" role="alert">{mcpError}</div>}
    <div className="mcp-list">{mcpServers.length === 0 ? <div className="skill-empty">{zh ? "尚未配置 MCP server" : "No MCP servers configured"}</div> : mcpServers.map((server) => <article key={server.id}>
      <div className="mcp-server-title"><span><b>{server.name}</b><code>{[server.command, ...server.args].join(" ")}</code><small>{server.last_inspected_at ? (zh ? `已发现 ${server.tools.length} 个工具` : `${server.tools.length} tools discovered`) : (zh ? "尚未检查" : "Not inspected")}</small></span><div><button disabled={mcpBusy !== null} onClick={() => editMcpServer(server)}>{zh ? "编辑" : "Edit"}</button><button disabled={mcpBusy !== null || !server.last_inspected_at || !onSetMcpServerEnabled} onClick={() => void runMcpAction(`enable:${server.id}`, () => onSetMcpServerEnabled!(server.id, !server.enabled))}>{server.enabled ? (zh ? "停用" : "Disable") : (zh ? "启用" : "Enable")}</button></div></div>
      <div className="mcp-profile-meta"><span className={`mcp-status-badge ${server.status === "ready" || server.enabled ? "ready" : server.status === "failed" || server.last_error ? "failed" : ""}`}>{mcpStatusLabel(server, zh)}</span>{server.timeout_secs && <small>{server.timeout_secs}s timeout</small>}{server.cwd && <small>{server.cwd}</small>}{server.env_bindings && server.env_bindings.length > 0 && <small>{server.env_bindings.length} env bindings</small>}</div>
      {(server.last_error || server.stderr_tail) && <details className="mcp-error-details"><summary>{zh ? "最近一次诊断" : "Latest diagnostics"}</summary><code>{[server.last_error, server.stderr_tail].filter(Boolean).join("\n")}</code></details>}
      <label className="mcp-launch-approval"><input type="checkbox" checked={inspectionApprovals[server.id] ?? false} onChange={(event) => setInspectionApprovals((current) => ({ ...current, [server.id]: event.target.checked }))} />{zh ? "我批准本次启动该本地进程，仅用于 initialize 和 tools/list" : "Approve one process launch for initialize and tools/list only"}</label>
      <button className="mcp-inspect" disabled={mcpBusy !== null || !selectedProject || !inspectionApprovals[server.id] || !onInspectMcpServer} onClick={() => void runMcpAction(`inspect:${server.id}`, async () => { await onInspectMcpServer!(server.id); setInspectionApprovals((current) => ({ ...current, [server.id]: false })); })}>{mcpBusy === `inspect:${server.id}` ? (zh ? "检查中…" : "Inspecting…") : (zh ? "检查并发现工具" : "Inspect and discover tools")}</button>
      {server.launch_approved && <button className="mcp-inspect" disabled={mcpBusy !== null} onClick={() => void runMcpAction(`launch:${server.id}`, async () => { const updated = onSetMcpLaunchApproval ? await onSetMcpLaunchApproval(server.id, false) : await invoke<McpServerProfile>("set_mcp_launch_approval", { request: { server_id: server.id, approved: false } }); Object.assign(server, updated); })}>{zh ? "撤销启动授权" : "Revoke launch approval"}</button>}
      {!selectedProject && <small className="mcp-project-hint">{zh ? "打开一个项目后才能执行隔离检查；配置仍可先保存。" : "Open a project to run an isolated inspection; configuration can still be saved."}</small>}
      {server.tools.length > 0 && <div className="mcp-tools"><b>{zh ? "逐工具权限" : "Per-tool permissions"}</b>{server.tools.map((tool) => { const approved = server.approved_tools.includes(tool.name); return <div key={tool.name}><span><code>{tool.name}</code><small>{tool.description ?? (zh ? "server 未提供描述" : "No description provided")}</small></span><button className={approved ? "approved" : ""} disabled={mcpBusy !== null || !onSetMcpToolApproval} onClick={() => void runMcpAction(`tool:${server.id}:${tool.name}`, () => onSetMcpToolApproval!(server.id, tool.name, !approved))}>{approved ? (zh ? "已批准，点击撤销" : "Approved · revoke") : (zh ? "批准调用" : "Approve calls")}</button></div>; })}</div>}
      <small className={`mcp-state ${server.enabled ? "enabled" : ""}`}>{server.enabled ? (zh ? "server 已启用；实际工具调用仍需命中上方批准清单" : "Server enabled; calls must still match the approval list") : (zh ? "默认停用，不会自动启动" : "Disabled by default; never auto-started")}</small>
    </article>)}</div>
    <div className="settings-note"><ShieldCheck size={18} /><span><b>{zh ? "技能与 MCP 均默认不启用" : "Skills and MCP are disabled by default"}</b><small>{zh ? "MCP 检查、启用和工具权限相互独立；修改启动命令或重新检查都会撤销已有工具授权。" : "Inspection, enablement, and tool approval are separate; changing the launch command or inspecting again revokes tool approvals."}</small></span></div>
  </main>;
}

function mcpStatusLabel(server: McpServerProfile, zh: boolean) {
  if (server.status === "connecting") return zh ? "连接中" : "Connecting";
  if (server.status === "stale") return zh ? "工具目录已过期，请重新检查" : "Tool catalog stale; inspect again";
  if (server.status === "failed" || server.last_error) return zh ? "最近一次运行失败" : "Last run failed";
  if (server.status === "ready") return zh ? "连接就绪" : "Ready";
  if (server.status === "stopping") return zh ? "正在停止" : "Stopping";
  if (server.enabled) return zh ? "已启用，等待调用" : "Enabled; waiting for a call";
  return server.last_inspected_at ? (zh ? "已检查，默认停用" : "Inspected; disabled by default") : (zh ? "未检查" : "Not inspected");
}

function skillCategoryLabel(category: string, zh: boolean) {
  if (category === "single_cell") return zh ? "单细胞组学" : "Single-cell omics";
  return category.replace(/_/g, " ");
}

function SkillCard({ skill, zh, skillsBusy, onSetSkillEnabled }: { skill: SkillPackage; zh: boolean; skillsBusy: boolean; onSetSkillEnabled?: Props["onSetSkillEnabled"] }) {
  return <article><div className="skill-title"><span><b>{skill.name}</b><small>v{skill.version} · SHA-256 {skill.sha256.slice(0, 12)}</small></span><button disabled={skillsBusy || !onSetSkillEnabled} onClick={() => void onSetSkillEnabled?.(skill.id, !skill.enabled)}>{skill.enabled ? (zh ? "停用" : "Disable") : (zh ? "启用" : "Enable")}</button></div><div className="skill-capabilities">{skill.capabilities.length === 0 ? <em>{zh ? "无额外能力" : "No additional capabilities"}</em> : skill.capabilities.map((capability) => <em key={capability}>{capability}</em>)}</div><small className="skill-state">{skill.enabled ? (zh ? "已启用" : "Enabled") : (zh ? "已安装，等待启用" : "Installed, awaiting enablement")}</small></article>;
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
