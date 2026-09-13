import { useEffect, useState } from "react";
import type { BundledMcpPreset, McpServerProfile } from "../../types";

export interface BundledMcpProps {
  onListBundledMcpPresets?: () => Promise<BundledMcpPreset[]>;
  onConfigurePubMedMcp?: (request: { api_key?: string; admin_email?: string }) => Promise<McpServerProfile>;
  onAddBundledMcp?: (request: { preset_id: string }) => Promise<McpServerProfile>;
}

export function BundledMcpPresets({ zh, busy, runAction, onListBundledMcpPresets, onAddBundledMcp, onConfigurePubMedMcp }: BundledMcpProps & {
  zh: boolean;
  busy: boolean;
  runAction: (key: string, action: () => Promise<unknown>) => Promise<void>;
}) {
  const [presets, setPresets] = useState<BundledMcpPreset[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");
  const [revision, setRevision] = useState(0);
  const [search, setSearch] = useState("");
  const [selectedId, setSelectedId] = useState("");
  const [added, setAdded] = useState("");
  useEffect(() => {
    if (!onListBundledMcpPresets) return;
    let active = true;
    setLoading(true);
    setError("");
    onListBundledMcpPresets().then((items) => {
      if (active) setPresets(items);
    }).catch((reason: unknown) => {
      if (active) setError(reason instanceof Error ? reason.message : String(reason));
    }).finally(() => { if (active) setLoading(false); });
    return () => { active = false; };
  }, [onListBundledMcpPresets, revision]);
  const query = search.trim().toLowerCase();
  const filtered = presets.filter((preset) => [preset.id, preset.name, preset.description, preset.description_zh].some((value) => value.toLowerCase().includes(query)));
  const selected = filtered.find((preset) => preset.id === selectedId);
  return <section className="mcp-preset" aria-label={zh ? "内置科学 MCP" : "Bundled scientific MCP"}>
    <div><b>{zh ? "内置科学 MCP" : "Bundled scientific MCP"}</b><small>{zh ? "来自 wisp-science 的科学数据库工具。启动时自动导入；调用前需检查并授权；调用会向对应数据库发送查询。" : "Scientific database tools from wisp-science. Imported automatically at startup; inspect and approve before calling. Calls send queries to the selected database."}</small></div>
    {loading && <small>{zh ? "正在加载目录…" : "Loading catalog…"}</small>}
    {error && <div role="alert">{error}<button disabled={loading} onClick={() => setRevision((current) => current + 1)}>{zh ? "重试加载目录" : "Retry catalog"}</button></div>}
    <div className="mcp-preset-grid">
      <label>{zh ? "搜索内置 MCP" : "Search bundled MCP"}<input value={search} onChange={(event) => { setSearch(event.target.value); setAdded(""); }} /></label>
      <label>{zh ? "选择内置 MCP" : "Select bundled MCP"}<select value={selected?.id ?? ""} disabled={loading || busy} onChange={(event) => { setSelectedId(event.target.value); setAdded(""); }}>
        <option value="">{zh ? "选择数据库" : "Select a database"}</option>
        {filtered.map((preset) => <option key={preset.id} value={preset.id}>{preset.name} · {preset.tool_count} {zh ? "个工具" : "tools"}</option>)}
      </select></label>
    </div>
    {selected && <small>{zh ? selected.description_zh || selected.description : selected.description}</small>}
    {selected?.id === "pubmed" && <PubMedCredentials zh={zh} busy={busy} configure={onConfigurePubMedMcp} runAction={runAction} />}
    {!loading && !error && filtered.length === 0 && <small>{zh ? "没有匹配的内置 MCP" : "No matching bundled MCP"}</small>}
    {added && <small role="status">{zh ? `已添加 ${added}，请在下方检查并授权。` : `Added ${added}. Inspect and approve it below.`}</small>}
    <div className="mcp-form-actions"><button className="primary" disabled={busy || loading || !selected || !onAddBundledMcp} onClick={() => {
      if (!selected || !onAddBundledMcp) return;
      setAdded("");
      void runAction("bundled", async () => { await onAddBundledMcp({ preset_id: selected.id }); setAdded(selected.name); });
    }}>{zh ? "添加内置 MCP" : "Add bundled MCP"}</button></div>
  </section>;
}

function PubMedCredentials({ zh, busy, configure, runAction }: {
  zh: boolean; busy: boolean; configure: BundledMcpProps["onConfigurePubMedMcp"];
  runAction: (key: string, action: () => Promise<unknown>) => Promise<void>;
}) {
  const [apiKey, setApiKey] = useState("");
  const [email, setEmail] = useState("");
  const [saved, setSaved] = useState(false);
  return <details className="mcp-credential-config">
    <summary>{zh ? "配置凭据" : "Configure credentials"}</summary>
    <small>{zh ? "API key 保存在系统凭据库。留空保留已有密钥；修改后仍需检查并授权工具。" : "The API key is stored in the system credential vault. Leave it blank to keep the existing key; inspect and approve tools after changes."}</small>
    <div className="mcp-preset-grid">
      <label>{zh ? "NCBI API key（可选）" : "NCBI API key (optional)"}<input aria-label="NCBI API key" type="password" autoComplete="new-password" disabled={busy} value={apiKey} onChange={event => { setApiKey(event.target.value); setSaved(false); }} /></label>
      <label>{zh ? "管理员邮箱（可选）" : "Admin email (optional)"}<input aria-label="NCBI admin email" type="email" disabled={busy} value={email} onChange={event => { setEmail(event.target.value); setSaved(false); }} /></label>
    </div>
    {saved && <small role="status">{zh ? "凭据已保存，请检查并授权工具。" : "Credentials saved. Inspect and approve tools."}</small>}
    <div className="mcp-form-actions"><button disabled={busy || !configure} onClick={() => {
      if (!configure) return;
      setSaved(false);
      void runAction("pubmed-credentials", async () => {
        await configure({ api_key: apiKey, admin_email: email });
        setApiKey(""); setSaved(true);
      });
    }}>{zh ? "保存 PubMed 凭据" : "Save PubMed credentials"}</button></div>
  </details>;
}