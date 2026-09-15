import { useCallback, useEffect, useRef, useState } from "react";
import { Globe2, LoaderCircle, PlugZap, ShieldCheck } from "lucide-react";

import { browserListAuthorizations, browserRevokeAuthorization } from "../../tauri-api";
import type { BrowserApprovalScopeV4, BrowserAuthorizationV4, McpServerProfile } from "../../types";
import type { Locale } from "../workspace/copy";
import "./PermissionsSettings.css";

interface Props {
  locale: Locale;
  mcpServers: McpServerProfile[];
  onSetMcpLaunchApproval?: (serverId: string, approved: boolean) => Promise<McpServerProfile>;
  onSetMcpToolApproval?: (serverId: string, tool: string, approved: boolean) => Promise<McpServerProfile>;
}

export function PermissionsSettings({ locale, mcpServers, onSetMcpLaunchApproval, onSetMcpToolApproval }: Props) {
  const zh = locale === "zh-CN";
  const [profiles, setProfiles] = useState(mcpServers);
  const [mcpError, setMcpError] = useState("");
  const [busyServers, setBusyServers] = useState<Set<string>>(new Set());
  const busyServersRef = useRef(new Set<string>());
  const [authorizations, setAuthorizations] = useState<BrowserAuthorizationV4[]>([]);
  const [browserLoading, setBrowserLoading] = useState(true);
  const [browserError, setBrowserError] = useState("");
  const [revokingAuthorization, setRevokingAuthorization] = useState<string | null>(null);
  const browserRequest = useRef(0);

  useEffect(() => setProfiles(mcpServers), [mcpServers]);

  const loadBrowserAuthorizations = useCallback(async () => {
    const request = ++browserRequest.current;
    setBrowserLoading(true);
    setBrowserError("");
    try {
      const result = await browserListAuthorizations();
      if (browserRequest.current !== request) return;
      setAuthorizations(Array.isArray(result) ? result : []);
    } catch (error) {
      if (browserRequest.current !== request) return;
      setBrowserError(errorMessage(error));
    } finally {
      if (browserRequest.current === request) setBrowserLoading(false);
    }
  }, []);

  useEffect(() => {
    void loadBrowserAuthorizations();
    return () => { ++browserRequest.current; };
  }, [loadBrowserAuthorizations]);

  async function updateMcpServer(serverId: string, action: () => Promise<McpServerProfile>) {
    if (busyServersRef.current.has(serverId)) return;
    busyServersRef.current.add(serverId);
    setBusyServers(new Set(busyServersRef.current));
    setMcpError("");
    try {
      const updated = await action();
      setProfiles((current) => current.map((profile) => profile.id === updated.id ? updated : profile));
    } catch (error) {
      setMcpError(errorMessage(error));
    } finally {
      busyServersRef.current.delete(serverId);
      setBusyServers(new Set(busyServersRef.current));
    }
  }

  async function revokeBrowserAuthorization(authorization: BrowserAuthorizationV4) {
    if (revokingAuthorization) return;
    setRevokingAuthorization(authorization.id);
    setBrowserError("");
    try {
      const revoked = await browserRevokeAuthorization(authorization.id);
      if (!revoked) throw new Error(zh ? "授权不存在或已经撤销。" : "The authorization was not found or was already revoked.");
      setAuthorizations((current) => current.filter((entry) => entry.id !== authorization.id));
    } catch (error) {
      setBrowserError(errorMessage(error));
    } finally {
      setRevokingAuthorization(null);
    }
  }

  const mcpPermissionCount = profiles.reduce((count, profile) => count + (profile.launch_approved ? 1 : 0) + profile.approved_tools.length, 0);

  return <main className="permissions-settings">
    <div className="settings-heading">
      <h3>{zh ? "权限" : "Permissions"}</h3>
      <p>{zh ? "查看并撤销宿主持久保存的 MCP 与浏览器授权。新授权仍需从原有审查入口明确批准。" : "Review and revoke MCP and browser permissions persisted by the host. New permissions still require explicit approval at their existing review points."}</p>
    </div>

    <section className="permissions-card" aria-labelledby="mcp-permissions-heading">
      <div className="permissions-card-heading"><PlugZap size={18} /><span><h4 id="mcp-permissions-heading">{zh ? "MCP 权限" : "MCP permissions"}</h4><p>{zh ? "启动授权与逐工具授权属于具体 server，不是通用系统权限。" : "Launch and per-tool permissions belong to a specific server; they are not general system permissions."}</p></span></div>
      {mcpError && <p className="permissions-error" role="alert">{mcpError}</p>}
      {mcpPermissionCount === 0 ? <p className="permissions-empty">{zh ? "暂无持久 MCP 权限。" : "No durable MCP permissions."}</p> : <div className="permissions-list">
        {profiles.map((profile) => {
          const busy = busyServers.has(profile.id);
          if (!profile.launch_approved && profile.approved_tools.length === 0) return null;
          return <article className="permissions-server" key={profile.id}>
            <header><span><b>{profile.name}</b><code>{profile.id}</code></span>{profile.enabled && <small>{zh ? "已启用" : "Enabled"}</small>}</header>
            {profile.launch_approved && <div className="permission-row"><span><b>{zh ? "Server 启动" : "Server launch"}</b><small>{zh ? "允许宿主为未来调用启动该本地 MCP 进程。撤销会停用 server、清空其工具授权并使当前 session 失效。" : "Allows the host to start this local MCP process for future calls. Revoking disables the server, clears its tool permissions, and invalidates its current session."}</small></span><button type="button" disabled={busy || !onSetMcpLaunchApproval} aria-label={zh ? `撤销 ${profile.name} 的启动权限` : `Revoke launch permission for ${profile.name}`} onClick={() => void updateMcpServer(profile.id, () => onSetMcpLaunchApproval!(profile.id, false))}>{busy && <LoaderCircle className="spin" size={13} />}{zh ? "撤销" : "Revoke"}</button></div>}
            {profile.approved_tools.map((tool) => <div className="permission-row" key={tool}><span><code>{tool}</code><small>{zh ? "允许此 server 的未来工具调用；撤销会使当前 MCP session 失效。" : "Allows future calls to this server tool; revoking invalidates the current MCP session."}</small></span><button type="button" disabled={busy || !onSetMcpToolApproval} aria-label={zh ? `撤销 ${profile.name} 的 ${tool} 权限` : `Revoke ${tool} permission for ${profile.name}`} onClick={() => void updateMcpServer(profile.id, () => onSetMcpToolApproval!(profile.id, tool, false))}>{busy && <LoaderCircle className="spin" size={13} />}{zh ? "撤销" : "Revoke"}</button></div>)}
          </article>;
        })}
      </div>}
    </section>

    <section className="permissions-card" aria-labelledby="browser-permissions-heading">
      <div className="permissions-card-heading"><Globe2 size={18} /><span><h4 id="browser-permissions-heading">{zh ? "浏览器授权" : "Browser authorizations"}</h4><p>{zh ? "每项授权保留原始 scope、capability、目标主机、session 与协议版本。" : "Each authorization retains its actual scope, capability, target host, session, and protocol version."}</p></span></div>
      {browserError && <div className="permissions-error-row"><p className="permissions-error" role="alert">{browserError}</p><button type="button" onClick={() => void loadBrowserAuthorizations()}>{zh ? "重试浏览器授权" : "Retry browser authorizations"}</button></div>}
      {browserLoading ? <p className="permissions-loading"><LoaderCircle className="spin" size={15} />{zh ? "正在读取浏览器授权…" : "Loading browser authorizations…"}</p> : authorizations.length === 0 ? <p className="permissions-empty">{zh ? "暂无持久浏览器授权。" : "No durable browser authorizations."}</p> : <div className="permissions-list">{authorizations.map((authorization) => {
        const context = [authorization.project_id && `${zh ? "项目" : "project"}: ${authorization.project_id}`, authorization.conversation_id && `${zh ? "会话" : "conversation"}: ${authorization.conversation_id}`].filter(Boolean).join(" · ");
        const host = authorization.binding.target_host || (zh ? "浏览器 session" : "Browser session");
        return <article className="permission-browser-row" key={authorization.id}>
          <span className="permission-browser-main"><span className="permission-tags"><b>{scopeLabel(authorization.scope, zh)}</b><code>{authorization.binding.capability}</code></span><strong>{host}</strong><small>{sessionLabel(authorization.binding.session, zh)} · protocol v{authorization.binding.protocol_version}{context ? ` · ${context}` : ""}</small><code className="permission-id">{authorization.id}</code></span>
          <button type="button" disabled={revokingAuthorization !== null} aria-label={zh ? `撤销 ${host} 的浏览器授权` : `Revoke browser authorization for ${host}`} onClick={() => void revokeBrowserAuthorization(authorization)}>{revokingAuthorization === authorization.id && <LoaderCircle className="spin" size={13} />}{zh ? "撤销" : "Revoke"}</button>
        </article>;
      })}</div>}
    </section>

    <div className="settings-note"><ShieldCheck size={18} /><span><b>{zh ? "只影响未来能力" : "Applies to future capability use"}</b><small>{zh ? "撤销不会改写冻结的运行快照，也不会取消已经派发的本地或远端计算。请在对应运行环境中单独确认已派发作业。" : "Revocation does not rewrite frozen run snapshots or cancel local or remote computation that has already been dispatched. Confirm dispatched jobs separately in their execution environment."}</small></span></div>
  </main>;
}

function scopeLabel(scope: BrowserApprovalScopeV4, zh: boolean) {
  const labels: Record<BrowserApprovalScopeV4, [string, string]> = {
    once: ["仅一次", "Once"],
    conversation: ["当前会话", "Conversation"],
    project: ["当前项目", "Project"],
    global: ["全局", "Global"],
  };
  return labels[scope][zh ? 0 : 1];
}

function sessionLabel(session: BrowserAuthorizationV4["binding"]["session"], zh: boolean) {
  if (session === "shared") return zh ? "共享 session" : "Shared session";
  return zh ? "工作区 session" : "Workspace session";
}

function errorMessage(error: unknown) {
  return error instanceof Error ? error.message : String(error);
}
