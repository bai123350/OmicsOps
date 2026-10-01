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

const networkLoginError = [
  "Unable to reach OpenAI. Check your internet connection and Windows system proxy, then retry.",
  "无法连接 OpenAI。请检查网络连接和 Windows 系统代理后重试。",
] as const;
const protocolLoginError = [
  "OpenAI returned an unexpected sign-in response. Retry; if this continues, update OmicsOps or report the issue.",
  "OpenAI 返回了无法识别的登录响应。请重试；若问题持续，请更新 OmicsOps 或反馈此问题。",
] as const;
const codexLoginErrors = {
  operation_failed: ["Operation failed. Check sign-in or configuration and retry.", "操作失败，请检查登录或配置后重试。"],
  codex_auth_network_uncertain: networkLoginError,
  codex_auth_connection_failed: networkLoginError,
  codex_auth_timeout: ["OpenAI sign-in timed out. Check your network connection and retry.", "OpenAI 登录请求超时。请检查网络连接后重试。"],
  codex_device_region_unsupported: ["OpenAI rejected sign-in from this network region. Check that your network location is supported and review your Windows system proxy before retrying.", "OpenAI 拒绝了当前网络地区的登录请求。请确认网络位置处于 OpenAI 支持的地区，检查 Windows 系统代理后重试。"],
  codex_device_access_denied: ["OpenAI denied device sign-in. Check that your account allows device-code sign-in, then retry.", "OpenAI 拒绝了设备登录。请确认账户允许设备代码登录后重试。"],
  codex_device_web_verification_required: ["OpenAI returned a web access or verification page. Check the network route used by OmicsOps and your Windows system proxy, then retry.", "OpenAI 返回了网页访问或验证页面。请检查 OmicsOps 使用的网络连接和 Windows 系统代理后重试。"],
  codex_device_rate_limited: ["Too many sign-in requests. Wait a little and retry later.", "登录请求过于频繁。请稍候再重试。"],
  codex_device_login_unavailable: ["OpenAI device sign-in is temporarily unavailable. Retry later.", "OpenAI 设备登录暂时不可用。请稍后重试。"],
  codex_auth_response_invalid: protocolLoginError,
  codex_auth_field_invalid: protocolLoginError,
  codex_device_interval_invalid: protocolLoginError,
  codex_device_challenge_invalid: protocolLoginError,
  codex_device_login_failed: networkLoginError,
  codex_device_authorization_failed: ["Codex authorization failed. Sign in again.", "Codex 授权失败。请重新登录。"],
  codex_token_exchange_failed: ["OpenAI could not finish authorization. Sign in again.", "OpenAI 未能完成授权。请重新登录。"],
  codex_reauthentication_required: ["Codex needs renewed authorization. Sign in again.", "Codex 需要重新授权。请重新登录。"],
  codex_login_expired: ["Codex sign-in expired. Sign in again to get a new code.", "Codex 登录已过期。请重新登录以获取新代码。"],
} as const;
type FormError = keyof typeof codexLoginErrors;
function safeCodexLoginError(error: unknown): FormError {
  const code = typeof error === "string" ? error : error instanceof Error ? error.message : null;
  return code && Object.prototype.hasOwnProperty.call(codexLoginErrors, code) ? code as FormError : "operation_failed";
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
  const [error, setError] = useState<FormError | null>(null);
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
    void api.subscriptionModelStatus(profile.id).then(value => { if (active) setStatus(value); }).catch(() => { if (active) setError("operation_failed"); });
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
        else if (value.state === "failed" || value.state === "expired") {
          setError(safeCodexLoginError(value.state === "expired" ? "codex_login_expired" : value.error_code));
        }
      } catch (failure) { if (active) { setError(safeCodexLoginError(failure)); setLoginState("failed"); } }
    }
    void poll();
    return () => { active = false; clearTimeout(timer); };
  }, [challenge]);
  async function begin() {
    if (busyRef.current) return;
    busyRef.current = true; setBusy(true); setError(null);
    const generation = lifecycle.current;
    try {
      if (currentLogin.current) cancelOnce(currentLogin.current);
      currentLogin.current = null; setChallenge(null); setLoginState(null);
      const value = await api.beginCodexLogin(profile?.id);
      if (lifecycle.current !== generation) { cancelOnce(value.login_id); return; }
      currentLogin.current = value.login_id; setChallenge(value); setLoginState("pending");
    } catch (failure) { if (lifecycle.current === generation) setError(safeCodexLoginError(failure)); }
    finally { busyRef.current = false; if (lifecycle.current === generation) setBusy(false); }
  }
  const windowValid = !windowDirty || !windowDraft.trim() || (/^[1-9]\d*$/.test(windowDraft.trim()) && Number(windowDraft) <= 4_294_967_295);
  const canSave = !busy && label.trim() && model.trim() && windowValid && (!codex || (challenge ? loginState === "authorized" : Boolean(profile?.subscription_account_ref)));
  async function save() {
    if (!canSave || busyRef.current) return;
    mutationRef.current = true;
    busyRef.current = true; setBusy(true); setError(null);
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
    } catch { if (lifecycle.current === generation) setError("operation_failed"); }
    finally { mutationRef.current = false; busyRef.current = false; if (lifecycle.current === generation) setBusy(false); }
  }
  async function disconnect() {
    if (!profile || busyRef.current) return;
    mutationRef.current = true;
    busyRef.current = true; setBusy(true); setError(null);
    const generation = lifecycle.current;
    try { const value = await api.disconnectCodex(profile.id); if (lifecycle.current === generation) onSaved(value); }
    catch { if (lifecycle.current === generation) setError("operation_failed"); }
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
    {error && <p role="alert">{codexLoginErrors[error][zh ? 1 : 0]}</p>}
    <div className="model-form-actions"><button disabled={busy && mutationRef.current} onClick={onCancel}>{zh ? "取消" : "Cancel"}</button><button className="primary" disabled={!canSave} onClick={() => void save()}>{zh ? "保存配置" : "Save profile"}</button></div>
  </section>;
}
