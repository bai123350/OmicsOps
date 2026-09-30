import { SubscriptionResourceLink } from "../settings/SubscriptionResourceLink";
import { useEffect, useRef, useState } from "react";
import { Check, ChevronDown, RefreshCw, Settings } from "lucide-react";
import type { ModelProfile } from "../../types";
import { listModelProfileModelDiscovery } from "../../tauri-api";
import { modelBackendLabel, reviewedGoEfforts } from "../../subscription-models";
import { useWindowEscapeLayer } from "../settings/BrowserSettings";
import "./api-model-picker.css";

interface Props {
  zh: boolean;
  profiles: ModelProfile[];
  activeProfileId: string | null;
  disabled: boolean;
  onProfileChange: (id: string) => void;
  onModelSelect: (profile: ModelProfile, model: string) => Promise<void>;
  onReasoningEffortChange?: (profile: ModelProfile, effort: ModelProfile["reasoning_effort"]) => Promise<void>;
  onManage: () => void;
}

export function ApiModelPicker({ zh, profiles, activeProfileId, disabled, onProfileChange, onModelSelect, onReasoningEffortChange, onManage }: Props) {
  const profile = profiles.find((item) => item.id === activeProfileId);
  const [open, setOpen] = useState(false);
  const [effortOpen, setEffortOpen] = useState(false);
  const [effortFailed, setEffortFailed] = useState(false);
  const [query, setQuery] = useState("");
  const [models, setModels] = useState<string[]>([]);
  const [loading, setLoading] = useState(false);
  const [loadFailed, setLoadFailed] = useState(false);
  const [saveFailed, setSaveFailed] = useState(false);
  const [saving, setSaving] = useState(false);
  const [revision, setRevision] = useState(0);
  const [discovery, setDiscovery] = useState<{ source: "provider" | "configured_only"; can_refresh: boolean } | null>(null);
  const savingRef = useRef(false);
  const generation = useRef(0);
  useWindowEscapeLayer(open, () => setOpen(false));
  useWindowEscapeLayer(open && effortOpen, () => setEffortOpen(false));

  useEffect(() => {
    setEffortOpen(false);
    setEffortFailed(false);
  }, [open, profile?.id, profile?.model, profile?.base_url, profile?.provider]);

  useEffect(() => {
    const request = ++generation.current;
    setModels([]);
    setQuery("");
    setSaveFailed(false);
    setLoadFailed(false);
    setDiscovery(null);
    if (!open || !profile) { setLoading(false); return; }
    setLoading(true);
    void listModelProfileModelDiscovery(profile.id).then((result) => {
      if (generation.current !== request) return;
      setDiscovery({ source: result.source, can_refresh: result.can_refresh });
      setModels([...new Set(result.models.map((item) => item.trim()).filter(Boolean))].sort());
    }).catch(() => {
      if (generation.current === request) setLoadFailed(true);
    }).finally(() => {
      if (generation.current === request) setLoading(false);
    });
    return () => { generation.current += 1; };
  }, [open, profile?.id, profile?.model, profile?.base_url, profile?.provider, revision]);

  async function chooseEffort(effort: ModelProfile["reasoning_effort"]) {
    if (!profile || !onReasoningEffortChange || disabled || savingRef.current) return;
    if ((profile.reasoning_effort ?? null) === effort) { setEffortOpen(false); return; }
    const request = generation.current;
    savingRef.current = true;
    setSaving(true);
    setEffortFailed(false);
    try {
      await onReasoningEffortChange(profile, effort);
      if (generation.current === request) setEffortOpen(false);
    } catch {
      if (generation.current === request) setEffortFailed(true);
    } finally {
      savingRef.current = false;
      setSaving(false);
    }
  }

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
  const effortLabel = zh ? "推理强度" : "Reasoning effort";
  const defaultLabel = zh ? "默认" : "Default";
  const currentEffort = profile?.reasoning_effort ?? null;
  const subscription = profile?.provider === "open_ai_codex" || profile?.provider === "claude_code" || profile?.provider === "open_ai_responses";
  const availableEfforts = profile?.provider === "claude_code" ? [] : profile?.provider === "open_ai_codex" ? ["none", "minimal", "low", "medium", "high", "xhigh"] : profile?.provider === "open_ai_responses" ? reviewedGoEfforts(profile.model).filter(v => profile.catalog_capabilities?.reasoning_efforts?.includes(v)) : profile?.catalog_capabilities?.reasoning_efforts ?? [];
  const efforts = [...new Set(availableEfforts)].filter((value): value is NonNullable<ModelProfile["reasoning_effort"]> => ["none", "minimal", "low", "medium", "high", "xhigh", "max", "ultra"].includes(value));
  return <div className="composer-menu-anchor model-anchor api-model-picker">
    <button className="composer-model" aria-label={zh ? "选择模型" : "Choose model"} aria-expanded={open} disabled={disabled || saving} title={profile?.model} onClick={() => setOpen((value) => !value)}><span>{profile?.model || (zh ? "选择模型" : "Choose model")}</span><ChevronDown size={12} /></button>
    {open && <div className="model-menu api-model-menu">
      {profiles.length > 1 && <label className="api-model-provider">{zh ? "模型来源" : "Model source"}<select aria-label={subscription ? (zh ? "选择模型来源" : "Choose model source") : (zh ? "选择 API" : "Choose API")} value={activeProfileId ?? ""} disabled={disabled || saving} onChange={(event) => onProfileChange(event.target.value)}>{profiles.map((item) => <option key={item.id} value={item.id}>{item.label} · {modelBackendLabel(item, zh)}</option>)}</select></label>}
      {profile && <>
        <p>{modelBackendLabel(profile, zh)}</p>
        {profile.provider === "open_ai_responses" && profile.model.startsWith("muse-spark-") && <p>{zh ? "Muse Spark Contributor 的提示词和回复可能用于模型训练。" : "Muse Spark Contributor prompts and completions may be used for model training."} <SubscriptionResourceLink resource="go_privacy" zh={zh}>{zh ? "隐私说明" : "Privacy details"}</SubscriptionResourceLink></p>}
        <div className="api-model-search"><input aria-label={subscription ? (zh ? "完整模型 ID" : "Full model ID") : (zh ? "搜索 API 模型" : "Search API models")} placeholder={zh ? "搜索或输入完整模型 ID…" : "Search or enter full model ID…"} value={query} onChange={(event) => setQuery(event.target.value)} /><button aria-label={zh ? "刷新模型列表" : "Refresh models"} disabled={loading || saving || disabled || discovery?.can_refresh === false} onClick={() => setRevision((value) => value + 1)}><RefreshCw size={14} /></button></div>
        {query.trim() && !options.includes(query.trim()) && <button disabled={disabled || saving} onClick={() => void choose(query.trim())}>{zh ? "使用完整模型 ID" : "Use full model ID"}</button>}
        {discovery?.source === "configured_only" && <p role="status">{zh ? "仅显示已配置模型；可手填完整 ID。" : "Configured models only; enter a full ID manually."}</p>}
        {loading && <p role="status">{zh ? "正在读取 API 模型列表…" : "Loading models from API…"}</p>}
        {loadFailed && <p role="alert">{zh ? "无法读取模型列表，请刷新重试或检查 API 配置。" : "Could not load models. Refresh to retry or check the API configuration."}</p>}
        {saveFailed && <p role="alert">{zh ? "模型切换失败，已保留原模型，请重试。" : "Could not switch models. The previous model is unchanged; please retry."}</p>}
        {effortFailed && <p role="alert">{zh ? "推理强度保存失败，已保留原值，请重试。" : "Could not save reasoning effort. The previous value is unchanged; please retry."}</p>}
        {saving && <p role="status">{zh ? "正在保存模型…" : "Saving model…"}</p>}
        {!loading && !loadFailed && discovery?.source !== "configured_only" && models.length === 0 && <p>{zh ? "API 未返回模型，保留当前配置。" : "The API returned no models. Keeping the current configuration."}</p>}
        <div className="api-model-options" role="menu" aria-label={zh ? "API 模型" : "API models"}>{visible.map((model) => <button key={model} role="menuitemradio" aria-checked={profile.model === model} title={model} disabled={disabled || saving} onClick={() => void choose(model)}><span>{model}</span>{profile.model === model && <Check size={14} />}</button>)}</div>
        {visible.length === 0 && !loading && <p>{zh ? "没有匹配的模型" : "No matching models"}</p>}
        {onReasoningEffortChange && <div className="api-effort-anchor">
          <button aria-label={`${effortLabel}: ${currentEffort ?? defaultLabel}`} aria-expanded={effortOpen} aria-haspopup="menu" disabled={disabled || saving || profile.provider === "claude_code"} onClick={() => setEffortOpen((value) => !value)}><span>{effortLabel}</span><span>{currentEffort ?? defaultLabel}</span><ChevronDown size={12} /></button>
          {effortOpen && <div className="api-effort-menu" role="menu" aria-label={effortLabel}>
            {[null, ...efforts].map((effort) => <button key={effort ?? "default"} role="menuitemradio" aria-checked={currentEffort === effort} disabled={disabled || saving} onClick={() => void chooseEffort(effort)}><span>{effort ?? defaultLabel}</span>{currentEffort === effort && <Check size={14} />}</button>)}
            {currentEffort && !efforts.includes(currentEffort) && <button role="menuitemradio" aria-checked="true" disabled><span>{currentEffort} ({zh ? "当前目录不可用" : "unavailable in current catalog"})</span><Check size={14} /></button>}
            {!profile.catalog_capabilities && <p>{zh ? "此 API 和模型的目录能力未知。" : "Catalog capabilities for this API and model are unknown."}</p>}
          </div>}
        </div>}
      </>}
      <button className="api-model-manage" disabled={saving} onClick={() => { setOpen(false); onManage(); }}>{zh ? "管理模型" : "Manage models"}<Settings size={14} /></button>
    </div>}
  </div>;
}
