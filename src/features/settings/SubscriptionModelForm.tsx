import { SubscriptionResourceLink } from "./SubscriptionResourceLink";
import { useEffect, useRef, useState } from "react";
import * as api from "../../tauri-api";
import type { BeginCodexLoginResponse, CodexLoginStateResponse, ModelProfile, SaveModelProfileRequest, SubscriptionModelStatus } from "../../types";
import { useWindowEscapeLayer } from "./BrowserSettings";

interface Props {
  provider: "open_ai_codex" | "claude_code";
  profile: ModelProfile | null;
  onSaved: (profile: ModelProfile) => void;
  onCancel: () => void;
  zh: boolean;
  modelProfiles?: ModelProfile[];
}

export function SubscriptionModelForm({ provider, profile, onSaved, onCancel, zh, modelProfiles = [] }: Props) {
  const codex = provider === "open_ai_codex";
  const [label, setLabel] = useState(profile?.label ?? (codex ? "Codex subscription" : "Claude Code subscription"));
  const [model, setModel] = useState(profile?.model ?? "");
  const [executable, setExecutable] = useState(profile?.cli_executable ?? "");
  const [windowDraft, setWindowDraft] = useState(String(profile?.context_window_tokens ?? ""));
  const [windowDirty, setWindowDirty] = useState(false);
  const [effort, setEffort] = useState<ModelProfile["reasoning_effort"]>(profile?.reasoning_effort ?? null);
  const [effortDirty, setEffortDirty] = useState(false);
  const [delegated, setDelegated] = useState(profile?.delegated_model_profile_id ?? "");
  const [delegatedDirty, setDelegatedDirty] = useState(false);
  const [challenge, setChallenge] = useState<BeginCodexLoginResponse | null>(null);
  const [loginState, setLoginState] = useState<CodexLoginStateResponse["state"] | null>(null);
  const [status, setStatus] = useState<SubscriptionModelStatus | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState(false);
  const lifecycle = useRef(0);
  const currentLogin = useRef<string | null>(null);
  const cancelled = useRef(new Set<string>());
  const busyRef = useRef(false);
  const mutationRef = useRef(false);
  const savedLogin = useRef(false);
  function cancelOnce(id: string) {
    if (cancelled.current.has(id)) return;
    cancelled.current.add(id);
    void api.cancelCodexLogin(id).catch(() => undefined);
  }
  useEffect(() => {
    ++lifecycle.current;
    return () => { ++lifecycle.current; if (currentLogin.current && !savedLogin.current) cancelOnce(currentLogin.current); };
  }, []);
  useWindowEscapeLayer(true, () => { if (!mutationRef.current) onCancel(); });
  useEffect(() => {
    if (!profile) return;
    let active = true;
    void api.subscriptionModelStatus(profile.id).then(value => { if (active) setStatus(value); }).catch(() => { if (active) setError(true); });
    return () => { active = false; };
  }, [profile]);
  useEffect(() => {
    if (!challenge) return;
    let active = true;
    let timer: ReturnType<typeof setTimeout>;
    async function poll() {
      try {
        const value = await api.pollCodexLogin(challenge!.login_id);
        if (!active) return;
        setLoginState(value.state);
        if (value.state === "pending") timer = setTimeout(() => void poll(), 1000);
      } catch { if (active) { setError(true); setLoginState("failed"); } }
    }
    void poll();
    return () => { active = false; clearTimeout(timer); };
  }, [challenge]);
  async function begin() {
    if (busyRef.current) return;
    busyRef.current = true; setBusy(true); setError(false);
    const generation = lifecycle.current;
    try {
      if (currentLogin.current) cancelOnce(currentLogin.current);
      currentLogin.current = null; setChallenge(null); setLoginState(null);
      const value = await api.beginCodexLogin(profile?.id);
      if (lifecycle.current !== generation) { cancelOnce(value.login_id); return; }
      currentLogin.current = value.login_id; setChallenge(value); setLoginState("pending");
    } catch { if (lifecycle.current === generation) setError(true); }
    finally { busyRef.current = false; if (lifecycle.current === generation) setBusy(false); }
  }
  const windowValid = !windowDirty || !windowDraft.trim() || (/^[1-9]\d*$/.test(windowDraft.trim()) && Number(windowDraft) <= 4_294_967_295);
  const canSave = !busy && label.trim() && model.trim() && windowValid && (!codex || (challenge ? loginState === "authorized" : Boolean(profile?.subscription_account_ref)));
  async function save() {
    if (!canSave || busyRef.current) return;
    mutationRef.current = true;
    busyRef.current = true; setBusy(true); setError(false);
    const generation = lifecycle.current;
    const request: SaveModelProfileRequest = {
      ...(profile ? { id: profile.id } : {}), label: label.trim(), provider,
      base_url: codex ? "https://chatgpt.com/backend-api" : "claude-code://local", model: model.trim(),
      ...(!codex ? { cli_executable: executable.trim() || null } : {}),
      ...(windowDirty && windowDraft.trim() ? { context_window_tokens: Number(windowDraft) } : {}),
      ...(codex && effortDirty ? { reasoning_effort: effort } : {}),
      ...(delegatedDirty ? { delegated_model_profile_id: delegated || null } : {}),
    };
    try {
      const value = codex && challenge
        ? await api.finishCodexLogin({ login_id: challenge.login_id, profile: request })
        : await api.saveModelProfile(request);
      savedLogin.current = true;
      if (lifecycle.current === generation) onSaved(value);
    } catch { if (lifecycle.current === generation) setError(true); }
    finally { mutationRef.current = false; busyRef.current = false; if (lifecycle.current === generation) setBusy(false); }
  }
  async function disconnect() {
    if (!profile || busyRef.current) return;
    mutationRef.current = true;
    busyRef.current = true; setBusy(true); setError(false);
    const generation = lifecycle.current;
    try { const value = await api.disconnectCodex(profile.id); if (lifecycle.current === generation) onSaved(value); }
    catch { if (lifecycle.current === generation) setError(true); }
    finally { mutationRef.current = false; busyRef.current = false; if (lifecycle.current === generation) setBusy(false); }
  }
  return <section className="model-form" aria-label={zh ? "订阅模型配置" : "Subscription model configuration"}>
    <h4>{codex ? (zh ? "Codex 订阅" : "Codex subscription") : (zh ? "Claude Code 订阅" : "Claude Code subscription")}</h4>
    <div className="model-form-grid">
      <label>{zh ? "配置名称" : "Profile label"}<input aria-label="Profile label" value={label} onChange={e => setLabel(e.target.value)} /></label>
      <label>{zh ? "完整模型 ID" : "Full model ID"}<input aria-label="Model" value={model} onChange={e => setModel(e.target.value)} /></label>
      <small className="wide">{zh ? "此入口仅保留已配置模型；可手填完整模型 ID，账户可用性需通过实际请求确认。提示词与工具结果会发送到模型服务。" : "Only configured models are shown; enter a full model ID and verify account availability with a request. Prompts and tool results are sent to the model service."}</small>
      <label className="wide">{zh ? "配置上下文预算" : "Configured context budget"}<input aria-label="Configured context budget" inputMode="numeric" value={windowDraft} onChange={e => { setWindowDraft(e.target.value); setWindowDirty(true); }} /></label>
      {!windowValid && <p role="alert">{zh ? "预算必须是正整数。" : "Enter a positive integer budget."}</p>}
      {codex ? <>
        <label>{zh ? "请求推理档位" : "Requested reasoning effort"}<select aria-label="Requested reasoning effort" value={effort ?? ""} onChange={e => { setEffort((e.target.value || null) as ModelProfile["reasoning_effort"]); setEffortDirty(true); }}><option value="">{zh ? "服务端默认" : "Provider default"}</option>{["none", "minimal", "low", "medium", "high", "xhigh"].map(v => <option key={v} value={v}>{v}</option>)}</select></label>
        <button type="button" disabled={busy} onClick={() => void begin()}>{zh ? "登录 Codex 订阅" : "Sign in to Codex"}</button>
        {challenge && <div className="wide" role="status"><SubscriptionResourceLink resource="codex_login" zh={zh}>{zh ? "打开官方登录页面" : "Open official sign-in page"}</SubscriptionResourceLink><code>{challenge.user_code}</code><p>{loginState === "authorized" ? (zh ? "已授权，点击保存完成绑定。" : "Authorized. Save to bind this profile.") : loginState === "pending" ? (zh ? "等待授权…" : "Waiting for authorization…") : (zh ? "登录已结束，请重新登录。" : "Login ended. Sign in again.")}</p></div>}
        {profile?.subscription_account_ref && <button disabled={busy} onClick={() => void disconnect()}>{zh ? "退出此订阅配置" : "Disconnect this subscription profile"}</button>}
      </> : <>
        <label className="wide">{zh ? "原生 claude.exe 路径" : "Native claude.exe path"}<input aria-label="Native claude.exe path" placeholder="C:\\Tools\\claude.exe" value={executable} onChange={e => setExecutable(e.target.value)} /></label>
        <p className="wide">{zh ? "请自行安装官方 Windows 原生 Claude Code，并在终端运行 claude auth login。留空时从 PATH 查找 claude.exe；不接受 .cmd/.bat 或参数。保存后可只读检查登录状态。" : "Install official native Claude Code for Windows and run claude auth login in a terminal. An empty path uses claude.exe from PATH; .cmd/.bat wrappers and arguments are unsupported. Save first to inspect login status."} <SubscriptionResourceLink resource="claude_setup" zh={zh}>{zh ? "官方安装说明" : "Official setup"}</SubscriptionResourceLink></p>
        <p className="wide" role="status">{zh ? "当前无法完整核实受管策略，Claude 模型生成已禁用；登录成功也不能绕过此限制。" : "Managed policy cannot be verified completely; Claude generation is disabled even when signed in."}</p>
      </>}
      {status && <p className="wide" role="status">{status.authenticated ? (zh ? "已登录" : "Signed in") : (zh ? "未登录" : "Not signed in")}{status.masked_account_label ? ` · ${status.masked_account_label}` : ""}{status.cli_version ? ` · ${status.cli_version}` : ""}</p>}
      <label className="wide">{zh ? "只读子 Agent 模型" : "Read-only subagent model"}<select aria-label="Read-only subagent model" value={delegated} onChange={e => { setDelegated(e.target.value); setDelegatedDirty(true); }}><option value="">{zh ? "沿用主模型" : "Inherit main model"}</option>{modelProfiles.filter(p => p.id !== profile?.id && p.supports_tools).map(p => <option key={p.id} value={p.id}>{p.label} · {p.model}</option>)}{delegated && !modelProfiles.some(p => p.id === delegated) && <option value={delegated}>{zh ? "保留当前绑定" : "Keep current binding"}</option>}</select></label>
    </div>
    {error && <p role="alert">{zh ? "操作失败，请检查登录或配置后重试。" : "Operation failed. Check sign-in or configuration and retry."}</p>}
    <div className="model-form-actions"><button disabled={busy && mutationRef.current} onClick={onCancel}>{zh ? "取消" : "Cancel"}</button><button className="primary" disabled={!canSave} onClick={() => void save()}>{zh ? "保存配置" : "Save profile"}</button></div>
  </section>;
}
