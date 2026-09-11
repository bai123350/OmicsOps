import { useEffect, useState, type ReactNode } from "react";
import { agentGetIterationSettings, agentSaveIterationSettings } from "../../tauri-api";
import type { AgentIterationSettingsV4 } from "../../types";
import type { Locale } from "../workspace/copy";
import "./session-settings.css";

const defaults: AgentIterationSettingsV4 = { max_iterations: 100, auto_continue: false, auto_continue_limit: 10, auto_compact: true, follow_up_questions: true };
const normalize = (settings: AgentIterationSettingsV4) => ({ ...defaults, ...settings });
const validLimit = (value: string) => /^\d+$/.test(value) && Number(value) <= 4294967295;

export function AgentSettings({ locale }: { locale: Locale }) {
  const zh = locale === "zh-CN";
  const [settings, setSettings] = useState(defaults);
  const [baseline, setBaseline] = useState<AgentIterationSettingsV4 | null>(null);
  const [limit, setLimit] = useState("100");
  const [continuations, setContinuations] = useState("10");
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState("");
  const [saved, setSaved] = useState(false);
  function restore(value: AgentIterationSettingsV4) {
    setSettings(value); setLimit(String(value.max_iterations)); setContinuations(String(value.auto_continue_limit));
  }
  useEffect(() => {
    let active = true;
    agentGetIterationSettings().then(value => {
      if (active) { const loaded = normalize(value); restore(loaded); setBaseline(loaded); }
    }).catch((error: unknown) => { if (active) setError(error instanceof Error ? error.message : String(error)); });
    return () => { active = false; };
  }, []);
  const valid = validLimit(limit) && validLimit(continuations);
  const disabled = !baseline || saving;
  function toggle(key: "auto_continue" | "auto_compact" | "follow_up_questions") {
    if (key === "auto_continue" && settings.auto_continue && !validLimit(continuations)) {
      setContinuations(String(settings.auto_continue_limit));
    }
    setSettings(current => ({ ...current, [key]: !current[key] })); setSaved(false);
  }
  async function save() {
    if (disabled || !valid) return;
    setSaving(true); setError(""); setSaved(false);
    try {
      const value = normalize(await agentSaveIterationSettings({ ...settings, max_iterations: Number(limit), auto_continue_limit: Number(continuations) }));
      restore(value); setBaseline(value); setSaved(true);
    } catch (error) { setError(error instanceof Error ? error.message : String(error)); }
    finally { setSaving(false); }
  }
  function row(id: string, title: string, description: string, control: ReactNode, dependent = false) {
    return <div className={`session-setting-row${dependent ? " session-setting-dependent" : ""}`}>
      <div className="session-setting-copy"><label id={`${id}-label`} htmlFor={id}>{title}</label><p id={`${id}-description`}>{description}</p></div>{control}
    </div>;
  }
  function switchControl(id: "auto_continue" | "auto_compact" | "follow_up_questions") {
    return <button id={id} type="button" role="switch" aria-labelledby={`${id}-label`} aria-describedby={`${id}-description`} aria-checked={settings[id]} className="session-toggle" disabled={disabled} onClick={() => toggle(id)}><span /></button>;
  }
  return <main className="session-settings">
    <div className="session-heading"><h3>Session</h3></div>
    <section className="session-card">
      <section className="session-group" aria-labelledby="session-run-limits"><h4 id="session-run-limits">{zh ? "运行限制" : "Run limits"}</h4>
        {row("max_iterations", zh ? "每轮对话的最大 Agent 迭代次数" : "Maximum agent iterations per turn", zh ? "限制一次对话中的模型/工具轮数，到限后进行一次无工具总结。默认 100；0 表示不限。" : "Limits model/tool rounds in one turn, followed by one tool-free summary. Default: 100; 0 means unlimited.", <input id="max_iterations" aria-describedby="max_iterations-description" type="number" min="0" max="4294967295" step="1" value={limit} disabled={disabled} onChange={event => { setLimit(event.target.value); setSaved(false); }} />)}
        {row("auto_continue", zh ? "自动继续被截断的输出" : "Auto-continue truncated output", zh ? "当模型达到输出 token 上限时，自动继续当前对话。" : "When a model reaches its output-token limit, continue the current turn automatically.", switchControl("auto_continue"))}
        {row("auto_continue_limit", zh ? "每轮对话最多自动继续次数" : "Maximum automatic continuations per turn", zh ? "默认 10；0 表示不自动继续。达到上限后停止自动续写。" : "Default: 10; 0 means no automatic continuations. Automatic continuation stops at this limit.", <input id="auto_continue_limit" aria-describedby="auto_continue_limit-description" type="number" min="0" max="4294967295" step="1" value={continuations} disabled={disabled || !settings.auto_continue} onChange={event => { setContinuations(event.target.value); setSaved(false); }} />, true)}
      </section>
      <section className="session-group" aria-labelledby="session-context"><h4 id="session-context">{zh ? "上下文管理" : "Context management"}</h4>
        {row("auto_compact", zh ? "自动压缩长对话" : "Automatically compact long conversations", zh ? "默认开启。在模型请求前，当上下文接近容量时自动压缩对话。" : "Enabled by default. Automatically compact the conversation before model requests as the context fills.", switchControl("auto_compact"))}
      </section>
      <section className="session-group" aria-labelledby="session-follow-up"><h4 id="session-follow-up">{zh ? "后续互动" : "Follow-up interaction"}</h4>
        {row("follow_up_questions", zh ? "建议后续问题" : "Suggest follow-up questions", zh ? "使用当前对话模型，在完成回复后建议三个后续问题。" : "Use the current conversation model to suggest three next questions after a completed reply.", switchControl("follow_up_questions"))}
      </section>
      {baseline && !valid && <p role="alert">{zh ? "请输入 0 到 4294967295 之间的整数。" : "Enter an integer from 0 to 4294967295."}</p>}
      {error && <p role="alert">{error}</p>}
      {saved && <p role="status">{zh ? "已保存，下次运行生效。" : "Saved for the next run."}</p>}
      <div className="session-actions"><button disabled={disabled} onClick={() => { if (baseline) restore(baseline); setSaved(false); setError(""); }}>{zh ? "取消" : "Cancel"}</button><button className="primary" disabled={disabled || !valid} onClick={() => void save()}>{zh ? "保存" : "Save"}</button></div>
    </section>
  </main>;
}
