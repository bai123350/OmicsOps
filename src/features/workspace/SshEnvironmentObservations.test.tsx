import { fireEvent, render, screen, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { SshEnvironmentObservations } from "./SshEnvironmentObservations";

describe("project-first SSH environment choices", () => {
  it("distinguishes the recommended per-language binding from optional system and shows each interpreter path", () => {
    const selected = { environment: "envs:python311:r443", label: "Python .envs/python311 · R .envs/r443", interpreters: [
      { language: "python" as const, status: "available" as const, version: "Python 3.11.18", executable: "/work/project/.envs/python311/bin/python" },
      { language: "r" as const, status: "available" as const, version: "R 4.4.3", executable: "/work/project/.envs/r443/bin/Rscript" },
    ] };
    const system = { environment: "system", label: "system", interpreters: [] };
    const change = vi.fn();
    render(<SshEnvironmentObservations zh environment={selected.environment} onEnvironmentChange={change} probe={{ selected, loading: false, error: false, refresh: vi.fn(), report: { project_id: "p", connection_id: "lab", remote_root: "/work/project", requested_environment: selected.environment, observed_at: "now", suggested_environment: selected.environment, truncated: false, environments: [selected, system] } }} />);
    const choices = screen.getByRole("combobox", { name: "已发现的 SSH 环境" });
    expect(within(choices).getByRole("option", { name: /项目推荐.*Python \.envs\/python311.*R \.envs\/r443/ })).toBeInTheDocument();
    expect(within(choices).getByRole("option", { name: /system.*可选/ })).toBeInTheDocument();
    const observations = screen.getByRole("region", { name: "SSH 解释器检测" });
    expect(observations).toHaveTextContent(".envs/python311/bin/python");
    expect(observations).toHaveTextContent(".envs/r443/bin/Rscript");
    fireEvent.change(choices, { target: { value: "system" } });
    expect(change).toHaveBeenCalledWith("system");
  });
});
