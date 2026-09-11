import { useState } from "react";
import { Grid2X2, Languages, Settings, X } from "lucide-react";
import type { ConversationCapabilitiesV4 } from "../../types";
import { useWindowEscapeLayer } from "../settings/BrowserSettings";
import type { Locale } from "./copy";

interface Props {
  projectId: string;
  conversationId?: string | null;
  summary?: ConversationCapabilitiesV4 | null;
  loading?: boolean;
  error?: string;
  onRefresh?: () => void;
  locale: Locale;
  onLocaleChange: (locale: Locale) => void;
  onOpenSettings?: (section?: "models" | "remote" | "skills") => void;
}

export function ConversationCapabilities({ projectId, conversationId, summary, loading = false, error, onRefresh, locale, onLocaleChange, onOpenSettings }: Props) {
  const zh = locale === "zh-CN";
  const [open, setOpen] = useState(false);
  useWindowEscapeLayer(open, () => setOpen(false));
  const current = !loading && !error && conversationId && summary?.conversation_id === conversationId && summary.project_id === projectId ? summary : null;
  const skills = current?.skills.filter((skill) => skill.enabled).length;
  const servers = current?.mcp_servers.filter((server) => server.enabled).length;
  const counts = `${skills ?? "—"} skills · ${servers ?? "—"} MCP · ${current?.memory_count ?? "—"} mem`;
  const status = !conversationId ? (zh ? "选择对话以查看可用能力" : "Select a conversation to view capabilities") : loading ? (zh ? "正在加载能力…" : "Loading capabilities…") : error ? (zh ? "能力加载失败" : "Capabilities could not be loaded") : !current ? (zh ? "能力信息暂不可用" : "Capabilities are unavailable") : (zh ? "当前对话可用的 Skills、MCP 服务和项目记忆" : "Skills, MCP servers, and project memories available to this conversation");
  return <>
    <div className="rail-footer capability-footer">
      <div className="capability-counts" aria-live="polite" aria-label={status} title={status}>{counts}</div>
      <button onClick={() => setOpen(true)} aria-haspopup="dialog"><Grid2X2 size={18} />Capabilities</button>
      <button disabled={!onOpenSettings} onClick={() => onOpenSettings?.("models")}><Settings size={18} />{zh ? "设置" : "Settings"}</button>
      <button className="rail-language" onClick={() => onLocaleChange(zh ? "en-US" : "zh-CN")}><Languages size={14} />{zh ? "English" : "简体中文"}</button>
    </div>
    {open && <div className="capabilities-backdrop" onClick={(event) => { if (event.target === event.currentTarget) setOpen(false); }}>
      <section className="capabilities-dialog" role="dialog" aria-modal="true" aria-labelledby="conversation-capabilities-title">
        <header><div><h2 id="conversation-capabilities-title">Capabilities</h2><p>{zh ? "当前对话可用的能力" : "Available to this conversation"}</p></div><button aria-label={zh ? "关闭能力面板" : "Close capabilities"} onClick={() => setOpen(false)}><X size={20} /></button></header>
        <div className="capabilities-body">
          <p className="capabilities-scope">{zh ? "Skills 和 MCP 配置由应用共享；记忆由当前项目共享。数量表示可供 Agent 检索使用的能力，不表示已加载进本轮提示词或已执行。" : "Skills and MCP settings are shared across the app; memories are shared within this project. Counts indicate capabilities available to the Agent, not content already loaded or executed in this turn."}</p>
          {!current && <div className="capabilities-unavailable" role={error ? "alert" : "status"}><p>{status}</p>{error && <p>{error}</p>}{conversationId && !loading && onRefresh && <button onClick={onRefresh}>{zh ? "重试" : "Retry"}</button>}</div>}
          {current && <>
            <section className="capability-section"><h3>Skills <span>{zh ? `${skills} 可用 / ${current.skills.length} 已安装` : `${skills} available / ${current.skills.length} installed`}</span></h3>
              {current.skills.length ? <ul>{current.skills.map((skill) => <li key={skill.id}><span>{skill.name}</span><small className={skill.enabled ? "enabled" : ""}>{skill.enabled ? (zh ? "可用" : "Available") : (zh ? "未启用" : "Disabled")}</small></li>)}</ul> : <p>{zh ? "尚未安装 Skills" : "No skills installed"}</p>}
            </section>
            <section className="capability-section"><h3>MCP <span>{zh ? `${servers} 已启用 / ${current.mcp_servers.length} 已配置服务` : `${servers} enabled / ${current.mcp_servers.length} configured servers`}</span></h3>
              {current.mcp_servers.length ? <ul>{current.mcp_servers.map((server) => <li key={server.id}><span>{server.name}<small>{zh ? `${server.tool_count} 个工具` : `${server.tool_count} tools`}</small></span><small className={server.enabled ? "enabled" : ""}>{server.enabled ? (zh ? "已启用" : "Enabled") : (zh ? "未启用" : "Disabled")}</small></li>)}</ul> : <p>{zh ? "尚未配置 MCP 服务" : "No MCP servers configured"}</p>}
              <p>{zh ? "MCP 数量按服务统计。工具调用仍需遵守授权和审批规则。" : "MCP counts servers. Tool calls still follow authorization and approval rules."}</p>
            </section>
            <section className="capability-section"><h3>Memory <span>{zh ? "项目共享" : "Project shared"}</span></h3><p>{zh ? `${current.memory_count} 条记忆` : `${current.memory_count} memories`}</p></section>
          </>}
        </div>
        <footer><button disabled={!onOpenSettings} onClick={() => { setOpen(false); onOpenSettings?.("skills"); }}>{zh ? "管理 Skills 和 MCP" : "Manage Skills and MCP"}</button></footer>
      </section>
    </div>}
  </>;
}
