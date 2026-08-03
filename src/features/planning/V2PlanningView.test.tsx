import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import * as api from "../../tauri-api";
import type { AnalysisPlanV2 } from "../../types";
import { V2PlanningView } from "./V2PlanningView";

const plan: AnalysisPlanV2 = {
  schema_version: 2,
  id: "10000000-0000-4000-8000-000000000001",
  title: "Bulk RNA-seq",
  summary: "WT vs induced",
  environment: { kind: "micromamba", channels: ["conda-forge", "bioconda"], dependencies: ["salmon"] },
  stages: [{ id: "qc", goal: "QC", dependencies: [], steps: [{
    id: "fastqc", title: "FastQC", rationale: "QC", dependencies: [],
    action: { kind: "tool", tool_id: "bio.fastqc", version: "1.0.0", arguments: { input: "reads/a.fastq.gz", outdir: "results/qc" } },
    working_directory: ".", resources: { max_cpu_cores: 2, max_memory_gib: 4, max_disk_gib: 10, max_step_seconds: 600 }, risk: "low",
    verifications: [{ kind: "file", path: "results/qc/a_fastqc.html", min_bytes: 10, sha256: null }], expected_artifacts: ["results/qc/a_fastqc.html"],
  }] }],
  resource_budget: { max_cpu_cores: 4, max_memory_gib: 8, max_disk_gib: 20, max_step_seconds: 1200 },
  policy: { allowed_tools: ["bio.fastqc"], allowed_domains: [], max_risk: "low", allow_legacy_shell: false }, metadata: {},
};

describe("V2 planning workflow", () => {
  it("moves from clarification to validation and hash approval", async () => {
    vi.spyOn(api, "planningTurn")
      .mockResolvedValueOnce({ kind: "clarification", questions: ["参考基因组版本？"] })
      .mockResolvedValueOnce({ kind: "draft", plan });
    vi.spyOn(api, "validatePlanV2").mockResolvedValue({ valid: true, issues: [] });
    vi.spyOn(api, "approvePlanV2").mockResolvedValue({ id: "approved", plan_id: plan.id, plan_hash: "abc123", policy: plan.policy, approved_at: "2026-08-03T00:00:00Z" });

    render(<V2PlanningView profileId="profile" projectId="project" environmentSummary="Linux" onStarted={() => undefined} />);
    fireEvent.change(screen.getByLabelText("分析目标"), { target: { value: "比较 WT 和诱导组" } });
    fireEvent.click(screen.getByRole("button", { name: "生成计划" }));
    expect(await screen.findByText("参考基因组版本？")).toBeInTheDocument();
    fireEvent.change(screen.getByLabelText("参考基因组版本？"), { target: { value: "R64-1-1" } });
    fireEvent.click(screen.getByRole("button", { name: "提交澄清" }));
    expect(await screen.findByDisplayValue("FastQC")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "校验计划" }));
    expect(await screen.findByText("计划校验通过")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "批准冻结计划" }));
    expect(await screen.findByText(/abc123/)).toBeInTheDocument();
  });
});
