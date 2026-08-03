import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import * as api from "../../tauri-api";
import { V2RunView } from "./V2RunView";

describe("V2 run replay", () => {
  it("shows needs-attention reason, verification results and event hashes", async () => {
    vi.spyOn(api, "listRunsV2").mockResolvedValue([{ run_id: "run", profile_id: "p", project_id: "j", approved_plan_id: "a", state: "needs_attention", completed_steps: ["qc"], action_hashes: { qc: "action" }, attention_reason: "manifest hash mismatch" }]);
    vi.spyOn(api, "listRunEventsV2").mockResolvedValue([{ sequence: 1, timestamp: "2026-08-03T00:00:00Z", run_id: "run", kind: "needs_attention", message: "manifest hash mismatch", prev_hash: null, details: { step_id: "qc" }, event_hash: "eventhash" }]);
    vi.spyOn(api, "listStepAttemptsV2").mockResolvedValue([{ run_id: "run", step_id: "qc", attempt: 0, action_hash: "action", process_group_id: 10, started_at: "2026-08-03T00:00:00Z", finished_at: "2026-08-03T00:01:00Z", exit_code: 0, log_path: "logs/qc.0.log", manifest_path: ".omicsops/state/qc.0.manifest.json", verifications: [{ specification: { kind: "file", path: "results/qc.html", min_bytes: 10, sha256: null }, passed: false, observed: "file changed" }] }]);
    vi.spyOn(api, "listArtifactsV2").mockResolvedValue([]);
    vi.spyOn(api, "getEnvironmentLockV2").mockResolvedValue(null);

    render(<V2RunView selectedRunId="run" />);
    expect((await screen.findAllByText("manifest hash mismatch")).length).toBeGreaterThan(0);
    expect(await screen.findByText("file changed")).toBeInTheDocument();
    expect(await screen.findByText(/eventhash/)).toBeInTheDocument();
  });
});
