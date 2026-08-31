import { useEffect, useRef, useState } from "react";
import {
  AlertTriangle,
  CheckCircle2,
  ExternalLink,
  Globe2,
  Link2,
  LoaderCircle,
  Play,
  Plus,
  RefreshCw,
  Save,
  ShieldCheck,
  Trash2,
  X,
} from "lucide-react";

import {
  browserGetSettings,
  browserListAuthorizations,
  browserRevokeAuthorization,
  browserSaveSettings,
  browserSetup,
} from "../../tauri-api";
import type {
  BrowserApprovalScopeV4,
  BrowserAuthorizationV4,
  BrowserConfigV4,
  BrowserSessionKindV4,
  BrowserSettingsResponseV4,
  BrowserStatusV4,
} from "../../types";
import "./BrowserSettings.css";

export interface BrowserSettingsProps {
  locale?: "zh-CN" | "en-US" | string;
}

type DomainKey = "disabled_domains" | "preferred_domains";

const DEFAULT_CONFIG: BrowserConfigV4 = {
  auto_launch: true,
  auto_close_turn_tabs: true,
  browser_path: null,
  default_search_provider: "default",
  disabled_domains: [],
  preferred_domains: [],
};

const SEARCH_PROVIDERS: Array<{
  value: BrowserConfigV4["default_search_provider"];
  en: string;
  zh: string;
}> = [
  { value: "default", en: "System default", zh: "系统默认" },
  { value: "google", en: "Google", zh: "Google" },
  { value: "bing", en: "Bing", zh: "Bing" },
  { value: "duckduckgo", en: "DuckDuckGo", zh: "DuckDuckGo" },
];

const SESSIONS: BrowserSessionKindV4[] = ["shared", "workspace"];

type EscapeLayer = { close: () => void };
const escapeStack: EscapeLayer[] = [];
let escapeListenerAttached = false;

function handleWindowEscape(event: KeyboardEvent) {
  if (event.key !== "Escape" || event.defaultPrevented) return;
  const layer = escapeStack.at(-1);
  if (!layer) return;

  event.preventDefault();
  event.stopImmediatePropagation();
  layer.close();
}

/**
 * Register a closeable surface in the app-wide Escape stack.
 *
 * SettingsPanel uses this for the parent dialog and BrowserSettings uses it
 * for the revoke confirmation, so an Escape closes only the visible top layer.
 */
export function useWindowEscapeLayer(active: boolean, onEscape: () => void) {
  const callbackRef = useRef(onEscape);
  callbackRef.current = onEscape;

  useEffect(() => {
    if (!active || typeof window === "undefined") return undefined;

    const layer: EscapeLayer = { close: () => callbackRef.current() };
    escapeStack.push(layer);
    if (!escapeListenerAttached) {
      window.addEventListener("keydown", handleWindowEscape);
      escapeListenerAttached = true;
    }

    return () => {
      const index = escapeStack.indexOf(layer);
      if (index >= 0) escapeStack.splice(index, 1);
      if (escapeListenerAttached && escapeStack.length === 0) {
        window.removeEventListener("keydown", handleWindowEscape);
        escapeListenerAttached = false;
      }
    };
  }, [active]);
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

function normalizeConfig(config?: Partial<BrowserConfigV4> | null): BrowserConfigV4 {
  const provider = config?.default_search_provider;
  const knownProvider = SEARCH_PROVIDERS.find((option) => option.value === provider)?.value
    ?? DEFAULT_CONFIG.default_search_provider;

  return {
    auto_launch: config?.auto_launch ?? DEFAULT_CONFIG.auto_launch,
    auto_close_turn_tabs: config?.auto_close_turn_tabs ?? DEFAULT_CONFIG.auto_close_turn_tabs,
    browser_path: config?.browser_path ?? DEFAULT_CONFIG.browser_path,
    default_search_provider: knownProvider,
    disabled_domains: [...(config?.disabled_domains ?? DEFAULT_CONFIG.disabled_domains)],
    preferred_domains: [...(config?.preferred_domains ?? DEFAULT_CONFIG.preferred_domains)],
  };
}

function scopeLabel(scope: BrowserApprovalScopeV4, zh: boolean): string {
  const labels = zh
    ? { once: "仅本次", conversation: "本次会话", project: "本项目", global: "全局" }
    : { once: "Once", conversation: "Conversation", project: "Project", global: "Global" };
  return labels[scope];
}

function sessionLabel(session: BrowserSessionKindV4, zh: boolean): string {
  return session === "shared" ? (zh ? "共享" : "Shared") : (zh ? "工作区" : "Workspace");
}

function statusLabel(status: BrowserStatusV4 | undefined, zh: boolean): string {
  if (!status) return zh ? "尚未读取" : "Not loaded";
  if (status.connected) return zh ? "已连接" : "Connected";
  if (status.listening) return zh ? "监听中，等待扩展" : "Listening; waiting for extension";
  return zh ? "未连接" : "Not connected";
}

function formatCreatedAt(createdAtMs: number): string {
  const date = new Date(createdAtMs);
  return Number.isNaN(date.getTime()) ? String(createdAtMs) : date.toISOString();
}

export function BrowserSettings({ locale = "en-US" }: BrowserSettingsProps) {
  const zh = locale === "zh-CN";
  const [response, setResponse] = useState<BrowserSettingsResponseV4 | null>(null);
  const [config, setConfig] = useState<BrowserConfigV4>(DEFAULT_CONFIG);
  const [authorizations, setAuthorizations] = useState<BrowserAuthorizationV4[]>([]);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [setupBusy, setSetupBusy] = useState<BrowserSessionKindV4 | null>(null);
  const [revoking, setRevoking] = useState(false);
  const [revokeTarget, setRevokeTarget] = useState<BrowserAuthorizationV4 | null>(null);
  const [domainDrafts, setDomainDrafts] = useState<Record<DomainKey, string>>({
    disabled_domains: "",
    preferred_domains: "",
  });
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  useWindowEscapeLayer(Boolean(revokeTarget), () => setRevokeTarget(null));

  useEffect(() => {
    void loadBrowserData();
  }, []);

  async function loadBrowserData() {
    setLoading(true);
    setError(null);
    setNotice(null);

    const [settingsResult, authorizationsResult] = await Promise.allSettled([
      browserGetSettings(),
      browserListAuthorizations(),
    ]);
    const errors: string[] = [];

    if (settingsResult.status === "fulfilled" && settingsResult.value) {
      setResponse(settingsResult.value);
      setConfig(normalizeConfig(settingsResult.value.config));
    } else {
      errors.push(settingsResult.status === "rejected" ? errorMessage(settingsResult.reason) : (zh ? "桌面端返回了空的浏览器设置。" : "The desktop host returned empty browser settings."));
    }

    if (authorizationsResult.status === "fulfilled") {
      setAuthorizations(Array.isArray(authorizationsResult.value) ? authorizationsResult.value : []);
    } else {
      errors.push(errorMessage(authorizationsResult.reason));
    }

    if (errors.length > 0) setError(errors.join(" · "));
    setLoading(false);
  }

  function updateConfig<K extends keyof BrowserConfigV4>(key: K, value: BrowserConfigV4[K]) {
    setConfig((current) => ({ ...current, [key]: value }));
    setError(null);
    setNotice(null);
  }

  function addDomains(key: DomainKey) {
    const additions = domainDrafts[key]
      .split(/[\s,;]+/)
      .map((domain) => domain.trim().toLowerCase())
      .filter(Boolean);
    if (additions.length === 0) return;

    setConfig((current) => ({
      ...current,
      [key]: Array.from(new Set([...current[key], ...additions])),
    }));
    setDomainDrafts((current) => ({ ...current, [key]: "" }));
    setError(null);
    setNotice(null);
  }

  function removeDomain(key: DomainKey, domain: string) {
    setConfig((current) => ({
      ...current,
      [key]: current[key].filter((candidate) => candidate !== domain),
    }));
    setError(null);
    setNotice(null);
  }

  async function saveSettings() {
    setSaving(true);
    setError(null);
    setNotice(null);
    const request: BrowserConfigV4 = {
      ...config,
      disabled_domains: [...config.disabled_domains],
      preferred_domains: [...config.preferred_domains],
    };

    try {
      const saved = await browserSaveSettings(request);
      if (saved) {
        setResponse(saved);
        setConfig(normalizeConfig(saved.config));
      }
      setNotice(zh ? "浏览器设置已保存" : "Browser settings saved");
    } catch (reason) {
      setError(errorMessage(reason));
    } finally {
      setSaving(false);
    }
  }

  async function setupSession(session: BrowserSessionKindV4) {
    setSetupBusy(session);
    setError(null);
    setNotice(null);
    try {
      const status = await browserSetup(session, true);
      setResponse((current) => {
        if (!current) return current;
        return session === "shared" ? { ...current, shared: status } : { ...current, workspace: status };
      });
      setNotice(zh ? `${sessionLabel(session, true)}浏览器 session 已启动` : `${sessionLabel(session, false)} browser session started`);
    } catch (reason) {
      setError(errorMessage(reason));
    } finally {
      setSetupBusy(null);
    }
  }

  async function revokeAuthorization() {
    if (!revokeTarget) return;
    const targetId = revokeTarget.id;
    setRevoking(true);
    setError(null);
    setNotice(null);
    try {
      const revoked = await browserRevokeAuthorization(targetId);
      if (revoked === false) throw new Error(zh ? "授权不存在或已经撤销。" : "The authorization was not found or was already revoked.");
      setAuthorizations((current) => current.filter((authorization) => authorization.id !== targetId));
      setRevokeTarget(null);
      setNotice(zh ? "授权已撤销" : "Authorization revoked");
    } catch (reason) {
      setError(errorMessage(reason));
    } finally {
      setRevoking(false);
    }
  }

  return (
    <main className="browser-settings" aria-label={zh ? "浏览器设置" : "Browser settings"}>
      <header className="browser-settings-heading">
        <div>
          <span className="browser-eyebrow">OmicsOps Browser Bridge</span>
          <h3>{zh ? "浏览器" : "Browser"}</h3>
          <p>{zh ? "为科研检索和本轮运行配置受控的浏览器连接。" : "Configure a controlled browser connection for research search and run-scoped work."}</p>
        </div>
        <button
          type="button"
          className="browser-icon-button"
          aria-label={zh ? "重新读取浏览器设置" : "Reload browser settings"}
          onClick={() => void loadBrowserData()}
          disabled={loading}
        >
          <RefreshCw size={16} className={loading ? "browser-spin" : undefined} />
        </button>
      </header>

      {loading && <div className="browser-message browser-message-loading" role="status"><LoaderCircle size={15} className="browser-spin" />{zh ? "正在读取浏览器设置…" : "Loading browser settings…"}</div>}
      {error && !revokeTarget && <div className="browser-message browser-message-error" role="alert"><AlertTriangle size={15} />{error}</div>}
      {notice && <div className="browser-message browser-message-success" role="status"><CheckCircle2 size={15} />{notice}</div>}

      <section className="browser-card" aria-labelledby="browser-session-heading">
        <div className="browser-section-heading">
          <span className="browser-section-icon"><Globe2 size={17} /></span>
          <div>
            <h4 id="browser-session-heading">{zh ? "连接状态" : "Connection status"}</h4>
            <p>{zh ? "共享和工作区 session 使用不同的 loopback 端口；启动不会授予网页访问权限。" : "Shared and workspace sessions use separate loopback ports; starting a session does not grant page access."}</p>
          </div>
        </div>
        <div className="browser-session-grid">
          {SESSIONS.map((session) => {
            const status = response?.[session];
            return <SessionCard key={session} session={session} status={status} zh={zh} busy={setupBusy === session} onSetup={() => void setupSession(session)} />;
          })}
        </div>
      </section>

      <section className="browser-card" aria-labelledby="browser-policy-heading">
        <div className="browser-section-heading">
          <span className="browser-section-icon"><ShieldCheck size={17} /></span>
          <div>
            <h4 id="browser-policy-heading">{zh ? "启动与搜索策略" : "Launch and search policy"}</h4>
            <p>{zh ? "这些选项只保存到浏览器配置；密钥、cookies 和页面内容不会写入设置。" : "These options are saved as browser configuration; keys, cookies, and page content are not stored here."}</p>
          </div>
        </div>
        <div className="browser-policy-grid">
          <label className="browser-toggle">
            <input type="checkbox" aria-label={zh ? "自动启动浏览器" : "Start browser automatically"} checked={config.auto_launch} onChange={(event) => updateConfig("auto_launch", event.target.checked)} />
            <span><strong>{zh ? "自动启动浏览器" : "Start browser automatically"}</strong><small>{zh ? "需要浏览器工具时自动启动所选 session。" : "Start the selected session when a browser tool needs it."}</small></span>
          </label>
          <label className="browser-toggle">
            <input type="checkbox" aria-label={zh ? "自动关闭本轮标签" : "Close this run's tabs automatically"} checked={config.auto_close_turn_tabs} onChange={(event) => updateConfig("auto_close_turn_tabs", event.target.checked)} />
            <span><strong>{zh ? "自动关闭本轮标签" : "Close this run's tabs automatically"}</strong><small>{zh ? "只关闭本轮创建的标签，不触碰其他标签。" : "Close only tabs created by this run; leave other tabs untouched."}</small></span>
          </label>
          <label className="browser-field browser-field-wide">
            <span>{zh ? "浏览器路径" : "Browser executable path"}</span>
            <input aria-label={zh ? "浏览器路径" : "Browser executable path"} value={config.browser_path ?? ""} placeholder={zh ? "留空以自动发现" : "Leave blank for system discovery"} onChange={(event) => updateConfig("browser_path", event.target.value || null)} />
            <small>{zh ? "填写绝对路径后仅使用该可执行文件。" : "An absolute path is used when provided."}</small>
          </label>
          <label className="browser-field">
            <span>{zh ? "默认搜索提供方" : "Default search provider"}</span>
            <select aria-label={zh ? "默认搜索提供方" : "Default search provider"} value={config.default_search_provider} onChange={(event) => updateConfig("default_search_provider", event.target.value as BrowserConfigV4["default_search_provider"])}>
              {SEARCH_PROVIDERS.map((provider) => <option key={provider.value} value={provider.value}>{zh ? provider.zh : provider.en}</option>)}
            </select>
          </label>
        </div>
      </section>

      <section className="browser-card" aria-labelledby="browser-domains-heading">
        <div className="browser-section-heading">
          <span className="browser-section-icon"><Link2 size={17} /></span>
          <div>
            <h4 id="browser-domains-heading">{zh ? "域名策略" : "Domain policy"}</h4>
            <p>{zh ? "禁用域名不会访问；优先域名用于检索时的排序提示。" : "Blocked domains are never visited; preferred domains guide research search ordering."}</p>
          </div>
        </div>
        <div className="browser-domain-grid">
          <DomainEditor
            keyName="disabled_domains"
            values={config.disabled_domains}
            draft={domainDrafts.disabled_domains}
            zh={zh}
            onDraftChange={(value) => setDomainDrafts((current) => ({ ...current, disabled_domains: value }))}
            onAdd={() => addDomains("disabled_domains")}
            onRemove={(domain) => removeDomain("disabled_domains", domain)}
          />
          <DomainEditor
            keyName="preferred_domains"
            values={config.preferred_domains}
            draft={domainDrafts.preferred_domains}
            zh={zh}
            onDraftChange={(value) => setDomainDrafts((current) => ({ ...current, preferred_domains: value }))}
            onAdd={() => addDomains("preferred_domains")}
            onRemove={(domain) => removeDomain("preferred_domains", domain)}
          />
        </div>
      </section>

      <section className="browser-card" aria-labelledby="browser-extension-heading">
        <div className="browser-section-heading">
          <span className="browser-section-icon"><ExternalLink size={17} /></span>
          <div>
            <h4 id="browser-extension-heading">{zh ? "浏览器扩展" : "Browser extension"}</h4>
            <p>{zh ? "安装入口由桌面端随应用提供；安装扩展不会自动授予域名权限。" : "The desktop host provides the packaged install entry; installing the extension does not grant domain access."}</p>
          </div>
        </div>
        <div className="browser-extension-grid">
          <label className="browser-field browser-field-wide">
            <span>{zh ? "扩展安装路径" : "Extension install path"}</span>
            <input aria-label={zh ? "扩展安装路径" : "Extension install path"} readOnly value={response?.extension_path ?? ""} placeholder={zh ? "桌面端尚未返回路径" : "The desktop host has not returned a path"} />
          </label>
          <div className="browser-extension-entry">
            <span>{zh ? "安装入口" : "Install entry"}</span>
            <strong>{zh ? "浏览器扩展管理器 → 开发者模式 → 加载已解压的扩展" : "Browser extension manager → Developer mode → Load unpacked"}</strong>
            {response?.shared?.extension_id && <small>{zh ? "扩展 ID" : "Extension ID"}: <code>{response.shared.extension_id}</code></small>}
          </div>
        </div>
      </section>

      <section className="browser-card" aria-labelledby="browser-authorizations-heading">
        <div className="browser-section-heading">
          <span className="browser-section-icon"><ShieldCheck size={17} /></span>
          <div>
            <h4 id="browser-authorizations-heading">{zh ? "浏览器授权" : "Browser authorizations"}</h4>
            <p>{zh ? "授权绑定 capability、目标主机、session 和协议版本；可以随时撤销。" : "Each authorization binds a capability, target host, session, and protocol version; revoke it at any time."}</p>
          </div>
        </div>
        {authorizations.length === 0
          ? <div className="browser-empty">{zh ? "暂无持久浏览器授权。" : "No durable browser authorizations."}</div>
          : <div className="browser-authorization-list">{authorizations.map((authorization) => <AuthorizationRow key={authorization.id} authorization={authorization} zh={zh} onRevoke={() => { setError(null); setNotice(null); setRevokeTarget(authorization); }} />)}</div>}
      </section>

      <footer className="browser-settings-footer">
        <small>{zh ? "保存后配置才会用于新的浏览器 session。" : "Saved configuration is used by new browser sessions."}</small>
        <button type="button" className="browser-primary-button" disabled={loading || saving} onClick={() => void saveSettings()}>
          {saving ? <LoaderCircle size={15} className="browser-spin" /> : <Save size={15} />}
          {saving ? (zh ? "保存中…" : "Saving…") : (zh ? "保存浏览器设置" : "Save browser settings")}
        </button>
      </footer>

      {revokeTarget && (
        <div className="browser-confirm-overlay">
          <section className="browser-confirm-dialog" role="dialog" aria-modal="true" aria-labelledby="browser-revoke-title">
            <header>
              <div>
                <span className="browser-eyebrow">{zh ? "需要确认" : "Confirmation required"}</span>
                <h3 id="browser-revoke-title">{zh ? "撤销浏览器授权？" : "Revoke browser authorization?"}</h3>
              </div>
              <button type="button" className="browser-icon-button" aria-label={zh ? "关闭确认" : "Close confirmation"} onClick={() => setRevokeTarget(null)}><X size={17} /></button>
            </header>
            <p>{zh ? "撤销后，匹配此 capability、目标主机、session 和协议版本的下一次操作需要重新批准。" : "After revoking, the next matching operation must be approved again for this capability, host, session, and protocol version."}</p>
            <div className="browser-confirm-target"><code>{revokeTarget.binding.capability}</code><strong>{revokeTarget.binding.target_host}</strong><small>{sessionLabel(revokeTarget.binding.session, zh)} · protocol v{revokeTarget.binding.protocol_version}</small></div>
            {error && <div className="browser-message browser-message-error" role="alert"><AlertTriangle size={15} />{error}</div>}
            <div className="browser-confirm-actions">
              <button type="button" className="browser-secondary-button" disabled={revoking} onClick={() => setRevokeTarget(null)}>{zh ? "取消" : "Cancel"}</button>
              <button type="button" className="browser-danger-button" disabled={revoking} onClick={() => void revokeAuthorization()}>{revoking && <LoaderCircle size={15} className="browser-spin" />}{revoking ? (zh ? "撤销中…" : "Revoking…") : (zh ? "撤销授权" : "Revoke authorization")}</button>
            </div>
          </section>
        </div>
      )}
    </main>
  );
}

function SessionCard({ session, status, zh, busy, onSetup }: { session: BrowserSessionKindV4; status: BrowserStatusV4 | undefined; zh: boolean; busy: boolean; onSetup: () => void }) {
  const connected = Boolean(status?.connected);
  const listening = Boolean(status?.listening);
  const sessionName = sessionLabel(session, zh);
  return (
    <article className={`browser-session-card ${connected ? "connected" : "disconnected"}`} data-session={session}>
      <div className="browser-session-card-heading">
        <div><span className="browser-status-dot" /><strong>{sessionName} {zh ? "session" : "session"}</strong></div>
        <span className={`browser-status-badge ${connected ? "connected" : ""}`}>{statusLabel(status, zh)}</span>
      </div>
      <p>{status ? `${listening ? (zh ? "监听端口" : "Listening on") : (zh ? "端口" : "Port")} ${status.port} · protocol v${status.protocol_version}` : (zh ? "等待桌面端返回状态" : "Waiting for the desktop host")}</p>
      {status && status.capabilities?.length > 0 && <div className="browser-capabilities" aria-label={zh ? "浏览器能力" : "Browser capabilities"}>{status.capabilities.map((capability) => <code key={capability}>{capability}</code>)}</div>}
      <button type="button" className="browser-secondary-button" disabled={busy} onClick={onSetup}><Play size={14} />{busy ? (zh ? "启动中…" : "Starting…") : (zh ? `启动${sessionName} session` : `Start ${sessionName.toLowerCase()} session`)}</button>
    </article>
  );
}

function DomainEditor({ keyName, values, draft, zh, onDraftChange, onAdd, onRemove }: { keyName: DomainKey; values: string[]; draft: string; zh: boolean; onDraftChange: (value: string) => void; onAdd: () => void; onRemove: (domain: string) => void }) {
  const disabled = keyName === "disabled_domains";
  const title = disabled ? (zh ? "禁用域名" : "Blocked domains") : (zh ? "优先域名" : "Preferred domains");
  const label = disabled ? (zh ? "添加禁用域名" : "Add blocked domain") : (zh ? "添加优先域名" : "Add preferred domain");
  return (
    <div className={`browser-domain-editor ${disabled ? "blocked" : "preferred"}`}>
      <label>
        <span>{title}</span>
        <div className="browser-domain-input">
          <input aria-label={label} value={draft} placeholder="example.org" onChange={(event) => onDraftChange(event.target.value)} onKeyDown={(event) => { if (event.key === "Enter") { event.preventDefault(); onAdd(); } }} />
          <button type="button" aria-label={`${label} ${zh ? "确认" : "submit"}`} onClick={onAdd}><Plus size={14} /></button>
        </div>
        <small>{zh ? "可用逗号或空格分隔多个域名。" : "Separate multiple domains with commas or spaces."}</small>
      </label>
      <div className="browser-domain-chips" aria-label={title}>
        {values.map((domain) => <span key={domain}><code>{domain}</code><button type="button" aria-label={`${zh ? "移除" : "Remove"} ${domain}`} onClick={() => onRemove(domain)}><Trash2 size={12} /></button></span>)}
        {values.length === 0 && <em>{zh ? "名单为空" : "List is empty"}</em>}
      </div>
    </div>
  );
}

function AuthorizationRow({ authorization, zh, onRevoke }: { authorization: BrowserAuthorizationV4; zh: boolean; onRevoke: () => void }) {
  const target = authorization.binding.target_host || (zh ? "浏览器 session" : "Browser session");
  const revokeLabel = zh ? "撤销授权" : "Revoke authorization";
  const context = [authorization.project_id && `${zh ? "项目" : "project"}: ${authorization.project_id}`, authorization.conversation_id && `${zh ? "会话" : "conversation"}: ${authorization.conversation_id}`].filter(Boolean).join(" · ");
  return (
    <article className="browser-authorization-row">
      <div className="browser-authorization-main">
        <div className="browser-authorization-tags"><span>{scopeLabel(authorization.scope, zh)}</span><code>{authorization.binding.capability}</code></div>
        <strong>{target}</strong>
        <small>{sessionLabel(authorization.binding.session, zh)} · protocol v{authorization.binding.protocol_version}{context ? ` · ${context}` : ""}</small>
        <time dateTime={new Date(authorization.created_at_ms).toISOString()}>{zh ? "创建于" : "Created"}: {formatCreatedAt(authorization.created_at_ms)}</time>
        <small className="browser-authorization-id" title={authorization.id}>{zh ? "授权 ID" : "Authorization ID"}: {authorization.id}</small>
      </div>
      <button type="button" className="browser-revoke-button" aria-label={`${revokeLabel} ${target}`} onClick={onRevoke}>{revokeLabel}</button>
    </article>
  );
}
