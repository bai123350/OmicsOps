import { useEffect, useRef, useState } from "react";
import { FolderOpen, LoaderCircle, MonitorCog, Network, Server } from "lucide-react";

import type { GeneralNativePreferences, GeneralSystemStatus, SystemInterpreterDiagnostics } from "../../types";
import type { Locale } from "../workspace/copy";
import {
  chooseProjectDirectoryStartingAt,
  settingsGeneralPreferences,
  settingsGeneralSystemStatus,
  settingsProbeSystemInterpreters,
  settingsSaveGeneralPreferences,
} from "../../general-settings-api";
import "./GeneralAdvancedSettings.css";

type Destination = "models" | "connections" | "remote";

export function GeneralAdvancedSettings({ locale, onNavigate }: { locale: Locale; onNavigate: (destination: Destination) => void }) {
  const zh = locale === "zh-CN";
  const [preferences, setPreferences] = useState<GeneralNativePreferences | null>(null);
  const [system, setSystem] = useState<GeneralSystemStatus | null>(null);
  const [diagnostics, setDiagnostics] = useState<SystemInterpreterDiagnostics | null>(null);
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState<"directory" | "system" | "probe" | null>(null);
  const [error, setError] = useState("");
  const [directoryNotice, setDirectoryNotice] = useState("");
  const loadGeneration = useRef(0);

  async function load() {
    const generation = ++loadGeneration.current;
    setLoading(true);
    setError("");
    try {
      const [nextPreferences, nextSystem] = await Promise.all([
        settingsGeneralPreferences(),
        settingsGeneralSystemStatus(),
      ]);
      if (generation !== loadGeneration.current) return;
      setPreferences(nextPreferences);
      setSystem(nextSystem);
    } catch (reason) {
      if (generation === loadGeneration.current) setError(reason instanceof Error ? reason.message : String(reason));
    } finally {
      if (generation === loadGeneration.current) setLoading(false);
    }
  }

  useEffect(() => {
    void load();
    return () => { loadGeneration.current += 1; };
  }, []);

  async function changeDirectory() {
    if (!preferences) return;
    setBusy("directory");
    setError("");
    setDirectoryNotice("");
    try {
      const selected = await chooseProjectDirectoryStartingAt(preferences.project_directory_start);
      if (selected.usedFallback) setDirectoryNotice(zh ? "保存的起始目录不可用，已改用系统默认位置。" : "The saved start folder was unavailable, so the system default was used.");
      if (!selected.path) return;
      setPreferences(await settingsSaveGeneralPreferences({ project_directory_start: selected.path }));
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
    } finally {
      setBusy(null);
    }
  }

  async function clearDirectory() {
    setBusy("directory");
    setError("");
    try {
      setPreferences(await settingsSaveGeneralPreferences({ project_directory_start: null }));
      setDirectoryNotice("");
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
    } finally {
      setBusy(null);
    }
  }

  async function reloadSystem() {
    setBusy("system");
    setError("");
    try { setSystem(await settingsGeneralSystemStatus()); }
    catch (reason) { setError(reason instanceof Error ? reason.message : String(reason)); }
    finally { setBusy(null); }
  }

  async function probe() {
    setBusy("probe");
    setError("");
    try { setDiagnostics(await settingsProbeSystemInterpreters()); }
    catch (reason) { setError(reason instanceof Error ? reason.message : String(reason)); }
    finally { setBusy(null); }
  }

  if (loading) return <section className="general-advanced-loading"><LoaderCircle className="spin" size={16} />{zh ? "正在读取本机设置…" : "Loading native settings…"}</section>;
  if (!preferences || !system) return <section className="general-advanced-error" role="alert"><span>{error || (zh ? "无法读取本机设置。" : "Could not load native settings.")}</span><button onClick={() => void load()}>{zh ? "重试" : "Retry"}</button></section>;

  return <div className="general-advanced">
    {error && <div className="general-advanced-error" role="alert">{error}</div>}
    <section className="general-advanced-card">
      <FolderOpen size={18} />
      <div><b>{zh ? "新项目目录起始位置" : "New project folder start"}</b><p>{zh ? "只决定创建项目时文件夹选择器的起始位置，不移动现有项目、数据库或文件。" : "Sets where the folder picker starts for new projects. It does not move existing projects, the database, or files."}</p><code>{preferences.project_directory_start || (zh ? "系统默认位置" : "System default")}</code>{directoryNotice && <small role="status">{directoryNotice}</small>}</div>
      <div className="general-advanced-actions"><button disabled={busy !== null} onClick={() => void changeDirectory()}>{zh ? "选择…" : "Choose…"}</button><button disabled={busy !== null || !preferences.project_directory_start} onClick={() => void clearDirectory()}>{zh ? "清除" : "Clear"}</button></div>
    </section>
    <section className="general-advanced-card">
      <MonitorCog size={18} />
      <div><b>{zh ? "应用与更新状态" : "App and update status"}</b><p>{zh ? `版本 ${system.app_version}` : `Version ${system.app_version}`}</p><code>{system.app_data_directory}</code><small>{system.update_source_configured ? (zh ? "已配置更新源；此页面尚未执行网络更新检查。" : "An update source is configured; this page has not performed a network update check.") : (zh ? "未配置更新源。" : "No update source is configured.")}</small></div>
      <button disabled={busy !== null} onClick={() => void reloadSystem()}>{zh ? "重新读取配置" : "Reload configuration"}</button>
    </section>
    <section className="general-advanced-card">
      <Server size={18} />
      <div><b>{zh ? "System Python / R" : "System Python / R"}</b><p>{zh ? "只检查 system PATH 中的 python 和 Rscript，不安装环境，也不验证项目依赖。" : "Checks python and Rscript on the system PATH. It does not install environments or verify project dependencies."}</p>{diagnostics && <div className="general-probe-results">{[diagnostics.python, diagnostics.r].map((item) => <span key={item.program} data-status={item.status}><code>{item.program}</code><small>{item.status === "found" ? (zh ? "找到可执行文件；项目依赖未验证" : "Executable found; project dependencies not verified") : item.status === "missing" ? (zh ? "system PATH 中未找到" : "Not found on the system PATH") : (item.detail || (zh ? "检查失败" : "Probe failed"))}</small></span>)}</div>}</div>
      <div className="general-advanced-actions"><button disabled={busy !== null} onClick={() => void probe()}>{busy === "probe" ? (zh ? "检查中…" : "Checking…") : (zh ? "检查解释器" : "Check interpreters")}</button><button disabled={busy !== null} onClick={() => onNavigate("remote")}>{zh ? "管理环境" : "Manage environments"}</button></div>
    </section>
    <section className="general-advanced-card">
      <Network size={18} />
      <div><b>{zh ? "模型与 MCP 连接" : "Model and MCP connections"}</b><p>{zh ? "模型页面管理实际 HTTP 服务地址；Connections 管理 MCP stdio 命令与检查。当前没有统一网络代理设置。" : "Models manages actual HTTP service URLs. Connections manages MCP stdio commands and inspection. There is no unified network proxy setting."}</p></div>
      <div className="general-advanced-actions"><button onClick={() => onNavigate("models")}>{zh ? "模型" : "Models"}</button><button onClick={() => onNavigate("connections")}>Connections</button></div>
    </section>
  </div>;
}
