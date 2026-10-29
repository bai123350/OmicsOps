import type { SshInterpreterObservationV4 } from "../../types";
import type { useSshProjectEnvironments } from "./useSshProjectEnvironments";

export type SshEnvironmentProbe = ReturnType<typeof useSshProjectEnvironments>;

export function sshEnvironmentDisplayLabel(environment: string | undefined, zh: boolean) {
  if (!environment?.startsWith("envs:")) return environment;
  const parts = environment.split(":");
  if (parts.length < 2 || parts.length > 3 || !parts.slice(1).some(Boolean)
    || parts.slice(1).some((name) => name && !/^[A-Za-z0-9][A-Za-z0-9_.-]{0,63}$/.test(name))) return environment;
  const python = parts[1]; const r = parts.length === 2 ? python : parts[2];
  const label = (name: string) => name ? `.envs/${name}` : (zh ? "未绑定" : "not bound");
  return `Python ${label(python)} · R ${label(r)}`;
}

export function sshInterpreterAvailability(probe: SshEnvironmentProbe, environment: string, language: "python" | "r", zh: boolean) {
  if (probe.loading) return zh ? "检测中" : "CHECKING";
  if (probe.error) return zh ? "检测失败" : "CHECK FAILED";
  const observation = probe.selected?.environment === environment ? probe.selected.interpreters.find((item) => item.language === language) : undefined;
  return observation?.status === "available" ? (zh ? "可用" : "AVAILABLE") : observation?.status === "not_found" ? (zh ? "不可用" : "UNAVAILABLE") : observation ? (zh ? "检测失败" : "CHECK FAILED") : (zh ? "待检测" : "UNVERIFIED");
}

export function sshInterpreterStatus(observation: SshInterpreterObservationV4 | undefined, zh: boolean) {
  if (!observation) return zh ? "待检测" : "Unverified";
  if (observation.status === "available") return observation.version || (zh ? "已找到解释器" : "Interpreter found");
  if (observation.status === "not_found") return zh ? "未找到解释器" : "Interpreter not found";
  if (observation.status === "timeout") return zh ? "检测超时" : "Detection timed out";
  return zh ? "检测失败" : "Detection failed";
}

export function SshEnvironmentObservations({ probe, environment, zh, onEnvironmentChange }: { probe: SshEnvironmentProbe; environment: string; zh: boolean; onEnvironmentChange?: (value: string) => void }) {
  const selected = probe.selected?.environment === environment ? probe.selected : null;
  const recommendation = probe.report?.truncated ? null : probe.report?.suggested_environment;
  const interpreterPath = (observation: SshInterpreterObservationV4 | undefined) => {
    const executable = observation?.executable;
    if (!executable) return null;
    const root = probe.report?.remote_root.replace(/\/+$/, "");
    return root && executable.startsWith(`${root}/`) ? executable.slice(root.length + 1) : executable;
  };
  return <section className="ssh-environment-observations" aria-label={zh ? "SSH 解释器检测" : "SSH interpreter detection"}>
    <button type="button" disabled={probe.loading} onClick={probe.refresh}>{probe.loading ? (zh ? "检测中…" : "Detecting…") : probe.report ? (zh ? "刷新远程环境" : "Refresh remote environments") : (zh ? "检测远程环境" : "Detect remote environments")}</button>
    {probe.loading && <p role="status">{zh ? "正在检查远端项目的解释器…" : "Checking remote project interpreters…"}</p>}
    {probe.error && <p role="alert">{zh ? "远端环境检测失败，请检查 SSH 连接后重试。" : "Remote detection failed. Check the SSH connection and retry."}</p>}
    <small>{zh ? "优先使用项目目录中的 Python / R；system 可按需选择。" : "Prefer project Python / R environments; system is optional."}</small>
    {probe.report && <>
      {onEnvironmentChange && <label>{zh ? "已发现的 SSH 环境" : "Discovered SSH environments"}<select aria-label={zh ? "已发现的 SSH 环境" : "Discovered SSH environments"} value={environment} onChange={(event) => onEnvironmentChange(event.target.value)}>
        {!probe.report.environments.some((option) => option.environment === environment) && <option value={environment}>{environment}</option>}
        {probe.report.environments.map((option) => <option key={option.environment} value={option.environment}>{option.environment === recommendation && option.environment.startsWith("envs:") ? `${zh ? "项目推荐" : "Project recommended"} · ` : ""}{option.environment === "system" ? (zh ? "system（可选）" : "system (optional)") : option.label}</option>)}
      </select></label>}
      {selected && <><small>{selected.label}</small><ul>{(["python", "r"] as const).map((language) => {
        const observation = selected.interpreters.find((item) => item.language === language);
        const path = interpreterPath(observation);
        return <li key={language}><b>{language === "python" ? "Python" : "R"}</b>: {sshInterpreterStatus(observation, zh)}{path && <> · <code>{path}</code></>}</li>;
      })}</ul></>}
      <small>{zh ? "解释器观察时间" : "Interpreters observed"}: <time>{probe.report.observed_at}</time></small>
      {probe.report.truncated && <p>{zh ? "环境列表已达检测上限；可返回左下角计算配置菜单，手动指定环境后刷新。" : "Environment discovery reached its limit; return to the compute configuration menu at the lower left to enter an environment and refresh."}</p>}
    </>}
    <small>{zh ? "仅检查解释器，不验证科研依赖是否齐全。" : "Checks interpreters; research dependencies are not verified."}</small>
  </section>;
}
