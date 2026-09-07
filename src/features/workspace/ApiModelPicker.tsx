import { useEffect, useRef, useState } from "react";
import { Check, ChevronDown, RefreshCw, Settings } from "lucide-react";
import type { ModelProfile } from "../../types";
import { listModelProfileModels } from "../../tauri-api";
import { useWindowEscapeLayer } from "../settings/BrowserSettings";
import "./api-model-picker.css";

interface Props {
  zh: boolean;
  profiles: ModelProfile[];
  activeProfileId: string | null;
  disabled: boolean;
  onProfileChange: (id: string) => void;
  onModelSelect: (profile: ModelProfile, model: string) => Promise<void>;
  onManage: () => void;
}

export function ApiModelPicker({ zh, profiles, activeProfileId, disabled, onProfileChange, onModelSelect, onManage }: Props) {
  const profile = profiles.find((item) => item.id === activeProfileId);
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const [models, setModels] = useState<string[]>([]);
  const [loading, setLoading] = useState(false);
  const [loadFailed, setLoadFailed] = useState(false);
  const [saveFailed, setSaveFailed] = useState(false);
  const [saving, setSaving] = useState(false);
  const [revision, setRevision] = useState(0);
  const savingRef = useRef(false);
  const generation = useRef(0);
  useWindowEscapeLayer(open, () => setOpen(false));

  useEffect(() => {
    const request = ++generation.current;
    setModels([]);
    setQuery("");
    setSaveFailed(false);
    setLoadFailed(false);
    if (!open || !profile) { setLoading(false); return; }
    setLoading(true);
    void listModelProfileModels(profile.id).then((items) => {
      if (generation.current !== request) return;
      setModels([...new Set(items.map((item) => item.trim()).filter(Boolean))].sort());
    }).catch(() => {
      if (generation.current === request) setLoadFailed(true);
    }).finally(() => {
      if (generation.current === request) setLoading(false);
    });
    return () => { generation.current += 1; };
  }, [open, profile?.id, profile?.base_url, profile?.provider, revision]);

  async function choose(model: string) {
    if (!profile || disabled || savingRef.current) return;
    if (profile.model === model) { setOpen(false); return; }
    const request = generation.current;
    savingRef.current = true;
    setSaving(true);
    setSaveFailed(false);
    try {
      await onModelSelect(profile, model);
      if (generation.current === request) setOpen(false);
    } catch {
      if (generation.current === request) setSaveFailed(true);
    } finally {
      savingRef.current = false;
      setSaving(false);
    }
  }

  const options = [...new Set([...(profile?.model ? [profile.model] : []), ...models])];
  const visible = options.filter((model) => model.toLowerCase().includes(query.trim().toLowerCase()));
  return <div className="composer-menu-anchor model-anchor api-model-picker">
    <button className="composer-model" aria-label={zh ? "选择模型" : "Choose model"} aria-expanded={open} disabled={disabled || saving} title={profile?.model} onClick={() => setOpen((value) => !value)}><span>{profile?.model || (zh ? "选择模型" : "Choose model")}</span><ChevronDown size={12} /></button>
    {open && <div className="model-menu api-model-menu">
      {profiles.length > 1 && <label className="api-model-provider">API<select aria-label={zh ? "选择 API" : "Choose API"} value={activeProfileId ?? ""} disabled={disabled || saving} onChange={(event) => onProfileChange(event.target.value)}>{profiles.map((item) => <option key={item.id} value={item.id}>{item.label}</option>)}</select></label>}
      {profile && <>
        <div className="api-model-search"><input aria-label={zh ? "搜索 API 模型" : "Search API models"} placeholder={zh ? "搜索模型…" : "Search models…"} value={query} onChange={(event) => setQuery(event.target.value)} /><button aria-label={zh ? "刷新模型列表" : "Refresh models"} disabled={loading || saving || disabled} onClick={() => setRevision((value) => value + 1)}><RefreshCw size={14} /></button></div>
        {loading && <p role="status">{zh ? "正在读取 API 模型列表…" : "Loading models from API…"}</p>}
        {loadFailed && <p role="alert">{zh ? "无法读取模型列表，请刷新重试或检查 API 配置。" : "Could not load models. Refresh to retry or check the API configuration."}</p>}
        {saveFailed && <p role="alert">{zh ? "模型切换失败，已保留原模型，请重试。" : "Could not switch models. The previous model is unchanged; please retry."}</p>}
        {saving && <p role="status">{zh ? "正在保存模型…" : "Saving model…"}</p>}
        {!loading && !loadFailed && models.length === 0 && <p>{zh ? "API 未返回模型，保留当前配置。" : "The API returned no models. Keeping the current configuration."}</p>}
        <div className="api-model-options" role="menu" aria-label={zh ? "API 模型" : "API models"}>{visible.map((model) => <button key={model} role="menuitemradio" aria-checked={profile.model === model} title={model} disabled={disabled || saving} onClick={() => void choose(model)}><span>{model}</span>{profile.model === model && <Check size={14} />}</button>)}</div>
        {visible.length === 0 && !loading && <p>{zh ? "没有匹配的模型" : "No matching models"}</p>}
      </>}
      <button className="api-model-manage" disabled={saving} onClick={() => { setOpen(false); onManage(); }}>{zh ? "管理模型" : "Manage models"}<Settings size={14} /></button>
    </div>}
  </div>;
}
