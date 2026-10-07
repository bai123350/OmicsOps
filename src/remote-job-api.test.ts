import { beforeEach, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { listRemoteJobsV4, refreshRemoteJobV4 } from "./remote-job-api";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
beforeEach(() => { vi.clearAllMocks(); Reflect.deleteProperty(window, "__TAURI_INTERNALS__"); });

it("rejects browser observation without claiming remote liveness", async () => {
  await expect(listRemoteJobsV4({ project_id: "p", conversation_id: null })).rejects.toBe("remote_job_unavailable");
  await expect(refreshRemoteJobV4({ project_id: "p", job_id: "j" })).rejects.toBe("remote_job_unavailable");
  expect(invoke).not.toHaveBeenCalled();
});

it("forwards only the nested native request and preserves the public DTO", async () => {
  Object.defineProperty(window, "__TAURI_INTERNALS__", { value: {}, configurable: true });
  const page = { jobs: [], limit: 100, remote_liveness_checked: false };
  const observation = { job: { job_id: "j", recorded_at: null, query_eligibility: "recorded_receipt" }, status: "completed", observed_at: "now", stdout_excerpt: "bounded", stderr_excerpt: "", captures: [{ relative_path: "result.csv", sha256: "hash", size_bytes: null }], resubmitted: false };
  vi.mocked(invoke).mockResolvedValueOnce(page).mockResolvedValueOnce(observation);
  expect(await listRemoteJobsV4({ project_id: "p", conversation_id: "c" })).toEqual(page);
  // Extra runtime properties must not become host authorization inputs.
  expect(await refreshRemoteJobV4({ project_id: "p", job_id: "j", bindingVersion: "secret" } as never)).toEqual(observation);
  expect(invoke).toHaveBeenNthCalledWith(1, "list_remote_jobs_v4", { request: { project_id: "p", conversation_id: "c" } });
  expect(invoke).toHaveBeenNthCalledWith(2, "refresh_remote_job_v4", { request: { project_id: "p", job_id: "j" } });
});
