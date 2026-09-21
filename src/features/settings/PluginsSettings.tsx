import { useCallback, useEffect, useRef, useState } from "react";
import { PackageOpen, PlugZap, ShieldAlert } from "lucide-react";

import {
  choosePluginDirectory,
  settingsInspectPlugin,
  settingsInstallPlugin,
  settingsListPlugins,
  settingsRemovePlugin,
  settingsSetPluginEnabled,
} from "../../integration-packages-api";
import type { InstalledPlugin, PluginInspection } from "../../types";
import type { Locale } from "../workspace/copy";
import { useWindowEscapeLayer } from "./BrowserSettings";
import "./PluginsSettings.css";

export function PluginsSettings({ locale, onOpenConnections, onChanged }: { locale: Locale; onOpenConnections: () => void; onChanged?: () => void }) {
  const zh = locale === "zh-CN";
  const listGeneration = useRef(0);
  const mounted = useRef(true);
  const [plugins, setPlugins] = useState<InstalledPlugin[]>([]);
  const [inspection, setInspection] = useState<PluginInspection | null>(null);
  const [removeTarget, setRemoveTarget] = useState<InstalledPlugin | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState("");

  const refresh = useCallback(async () => {
    const request = ++listGeneration.current;
    setLoading(true);
    try {
      const value = await settingsListPlugins();
      if (mounted.current && listGeneration.current === request) setPlugins(value);
    } catch (cause) {
      if (mounted.current && listGeneration.current === request) setError(message(cause));
    } finally {
      if (mounted.current && listGeneration.current === request) setLoading(false);
    }
  }, []);

  useEffect(() => {
    void refresh();
    return () => { mounted.current = false; listGeneration.current += 1; };
  }, [refresh]);

  async function inspect() {
    if (busy) return;
    setError("");
    setBusy("inspect");
    try {
      const source = await choosePluginDirectory();
      if (!source) return;
      const value = await settingsInspectPlugin(source);
      if (mounted.current) setInspection(value);
    } catch (cause) {
      if (mounted.current) setError(message(cause));
    } finally {
      if (mounted.current) setBusy(null);
    }
  }

  async function install(preview: PluginInspection, retry?: InstalledPlugin) {
    if (busy) return;
    setError("");
    setBusy(`install:${preview.package_id}`);
    try {
      const current = retry ?? plugins.find((plugin) => plugin.installation_id === preview.existing_installation_id);
      await settingsInstallPlugin(preview.source_path, preview.manifest_digest, current?.digest ?? null);
      if (!mounted.current) return;
      setInspection(null);
      await refresh();
      onChanged?.();
    } catch (cause) {
      if (mounted.current) setError(message(cause));
    } finally {
      if (mounted.current) setBusy(null);
    }
  }

  async function toggle(plugin: InstalledPlugin) {
    if (busy) return;
    setError("");
    setBusy(`toggle:${plugin.installation_id}`);
    try {
      const updated = await settingsSetPluginEnabled(plugin.installation_id, !plugin.enabled);
      if (mounted.current) setPlugins((current) => current.map((item) => item.installation_id === updated.installation_id ? updated : item));
      if (mounted.current) onChanged?.();
    } catch (cause) { if (mounted.current) setError(message(cause)); }
    finally { if (mounted.current) setBusy(null); }
  }

  async function retry(plugin: InstalledPlugin) {
    if (busy) return;
    setError("");
    setBusy(`retry:${plugin.installation_id}`);
    try {
      const preview = await settingsInspectPlugin(plugin.source_path);
      if (!mounted.current) return;
      await settingsInstallPlugin(preview.source_path, preview.manifest_digest, plugin.digest);
      if (!mounted.current) return;
      await refresh();
      onChanged?.();
    } catch (cause) { if (mounted.current) setError(message(cause)); }
    finally { if (mounted.current) setBusy(null); }
  }

  async function remove(plugin: InstalledPlugin) {
    if (busy) return;
    setError("");
    setBusy(`remove:${plugin.installation_id}`);
    try {
      const result = await settingsRemovePlugin(plugin.installation_id, plugin.digest);
      if (!mounted.current) return;
      setRemoveTarget(null);
      await refresh();
      if (mounted.current) onChanged?.();
      if (result.preserved_files.length) setError(result.message);
    } catch (cause) { if (mounted.current) setError(message(cause)); }
    finally { if (mounted.current) setBusy(null); }
  }

  return <main className="plugins-settings">
    <div className="settings-heading plugin-heading"><div><h3>{zh ? "插件" : "Plugins"}</h3><p>{zh ? "安装严格声明的本地包。检查和安装不会执行脚本；包始终标记为本地未验证。" : "Install strictly declared local packages. Inspection and installation execute no scripts; packages remain locally unverified."}</p></div><button type="button" disabled={Boolean(busy)} onClick={() => void inspect()}>{busy === "inspect" ? (zh ? "检查中…" : "Inspecting…") : (zh ? "选择本地包" : "Choose local package")}</button></div>
    <div className="plugin-safety"><ShieldAlert size={18} /><span><b>{zh ? "有限声明格式" : "Bounded declaration format"}</b><small>{zh ? "schema 1 只接受独立 SKILL.md 与已编译 MCP preset 引用。启用插件不会启动或批准共享 MCP。" : "Schema 1 accepts standalone SKILL.md files and compiled MCP preset references only. Enabling a plugin does not start or approve shared MCP."}</small></span></div>
    {error && <p className="plugin-error" role="alert">{error}</p>}
    {loading ? <p role="status">{zh ? "正在读取插件…" : "Loading plugins…"}</p> : plugins.length === 0 ? <div className="plugin-empty"><PackageOpen size={28} /><b>{zh ? "尚未安装本地插件" : "No local plugins installed"}</b><small>{zh ? "包目录必须包含 omicsops-plugin.json。" : "A package directory must contain omicsops-plugin.json."}</small></div> : <div className="plugin-list">{plugins.map((plugin) => <article key={plugin.installation_id}>
      <div className="plugin-title"><span><b>{plugin.name}</b><small>{plugin.package_id} · {plugin.version} · {phaseLabel(plugin.phase, zh)}</small></span><em className={plugin.enabled ? "enabled" : ""}>{plugin.enabled ? (zh ? "已启用" : "Enabled") : (zh ? "已停用" : "Disabled")}</em></div>
      <p>{plugin.skills.length} Skills · {plugin.mcp_bindings.length} MCP {zh ? "共享引用" : "shared references"}</p>
      {plugin.last_error && <small className="plugin-warning">{plugin.last_error}</small>}
      <div className="plugin-actions"><button type="button" disabled={Boolean(busy) || plugin.phase !== "installed"} aria-label={`${plugin.enabled ? (zh ? "停用" : "Disable") : (zh ? "启用" : "Enable")} ${plugin.name}`} onClick={() => void toggle(plugin)}>{plugin.enabled ? (zh ? "停用" : "Disable") : (zh ? "启用" : "Enable")}</button>{plugin.phase === "needs_attention" && <button type="button" disabled={Boolean(busy)} onClick={() => void retry(plugin)}>{zh ? "重试" : "Retry"}</button>}<button type="button" disabled={Boolean(busy)} aria-label={`${zh ? "移除" : "Remove"} ${plugin.name}`} onClick={() => setRemoveTarget(plugin)}>{zh ? "移除" : "Remove"}</button></div>
      {plugin.mcp_bindings.length > 0 && <button className="plugin-mcp-link" type="button" onClick={onOpenConnections}><PlugZap size={14} />{zh ? "查看共享 MCP 状态" : "View shared MCP status"}</button>}
    </article>)}</div>}
    <details className="plugin-template"><summary>{zh ? "查看 schema 1 模板" : "View schema 1 template"}</summary><pre>{manifestTemplate}</pre></details>
    {inspection && <PluginPreview inspection={inspection} busy={Boolean(busy)} zh={zh} onClose={() => setInspection(null)} onInstall={() => void install(inspection)} />}
    {removeTarget && <PluginRemoveConfirm plugin={removeTarget} busy={Boolean(busy)} zh={zh} onClose={() => setRemoveTarget(null)} onConfirm={() => void remove(removeTarget)} />}
  </main>;
}

function PluginPreview({ inspection, busy, zh, onClose, onInstall }: { inspection: PluginInspection; busy: boolean; zh: boolean; onClose: () => void; onInstall: () => void }) {
  useWindowEscapeLayer(true, onClose);
  return <div className="plugin-modal-overlay"><section className="plugin-dialog" role="dialog" aria-modal="true" aria-label={zh ? "插件包预览" : "Plugin package preview"}><header><div><small>{zh ? "只读检查" : "Read-only inspection"}</small><h3>{inspection.name} {inspection.version}</h3></div><button type="button" aria-label={zh ? "关闭插件预览" : "Close plugin preview"} onClick={onClose}>×</button></header><div className="plugin-dialog-body"><p><code>{inspection.package_id}</code></p><p className="plugin-digest"><code>{inspection.manifest_digest}</code></p><h4>{zh ? "变更" : "Changes"}</h4><ul>{inspection.changes.map((change) => <li key={change}><span>{changeLabel(change, zh)}</span></li>)}</ul><h4>{zh ? "声明文件" : "Declared files"}</h4><ul>{inspection.files.map((file) => <li key={file.relative_path}><code>{file.relative_path}</code><small>{file.size_bytes} B</small></li>)}</ul><h4>{zh ? "共享 MCP 引用" : "Shared MCP references"}</h4>{inspection.bindings.length ? <ul>{inspection.bindings.map((binding) => <li key={binding.preset_id}><code>{binding.preset_id}</code><small>{binding.configured ? (binding.enabled ? (zh ? "全局已启用" : "Globally enabled") : (zh ? "已配置、全局停用" : "Configured, globally disabled")) : (zh ? "未配置" : "Not configured")}</small></li>)}</ul> : <p>{zh ? "无" : "None"}</p>}</div><footer><button type="button" onClick={onClose}>{zh ? "取消" : "Cancel"}</button><button className="primary" type="button" disabled={busy} onClick={onInstall}>{busy ? (zh ? "安装中…" : "Installing…") : inspection.existing_installation_id ? (zh ? "更新包" : "Update package") : (zh ? "安装包" : "Install package")}</button></footer></section></div>;
}

function PluginRemoveConfirm({ plugin, busy, zh, onClose, onConfirm }: { plugin: InstalledPlugin; busy: boolean; zh: boolean; onClose: () => void; onConfirm: () => void }) {
  useWindowEscapeLayer(true, onClose);
  return <div className="plugin-modal-overlay"><section className="plugin-confirm" role="dialog" aria-modal="true" aria-label={zh ? "移除插件" : "Remove plugin"}><h3>{zh ? `移除 ${plugin.name}？` : `Remove ${plugin.name}?`}</h3><p>{zh ? "只移除该包拥有且未改动的文件。共享 MCP 配置、凭据和授权不会改变；用户修改或未知文件会保留。" : "Only unchanged files owned by this package are removed. Shared MCP configuration, credentials, and approvals stay unchanged; modified or unknown files are preserved."}</p><div><button type="button" onClick={onClose}>{zh ? "取消" : "Cancel"}</button><button className="danger" type="button" disabled={busy} onClick={onConfirm}>{zh ? "确认移除" : "Confirm removal"}</button></div></section></div>;
}

function phaseLabel(phase: InstalledPlugin["phase"], zh: boolean) {
  const labels = zh ? { staging: "登记中", installed: "已安装", updating: "更新中", removing: "移除中", needs_attention: "需要处理", removed: "已移除" } : { staging: "Staging", installed: "Installed", updating: "Updating", removing: "Removing", needs_attention: "Needs attention", removed: "Removed" };
  return labels[phase];
}

function message(cause: unknown) { return cause instanceof Error ? cause.message : String(cause); }

function changeLabel(change: string, zh: boolean) {
  if (change === "new_installation") return zh ? "新安装" : "New installation";
  if (change === "same_content") return zh ? "内容与当前安装相同" : "Content matches the current installation";
  if (change.startsWith("version:")) return `${zh ? "版本" : "Version"}: ${change.slice(8)}`;
  if (change.startsWith("skills:")) return `Skills: ${change.slice(7)}`;
  return change;
}

const manifestTemplate = `{
  "schema_version": 1,
  "id": "lab.qc",
  "version": "1.0.0",
  "name": "QC helpers",
  "skills": [{ "path": "skills/qc/SKILL.md", "sha256": "<64 lowercase hex>" }],
  "mcp_presets": []
}`;
