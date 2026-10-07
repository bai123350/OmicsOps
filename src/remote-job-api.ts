import { invoke } from "@tauri-apps/api/core";
import type { ListRemoteJobsV4Request, RefreshRemoteJobV4Request, RemoteJobObservationV4, RemoteJobsPageV4 } from "./types";

function requireDesktop() {
  if (typeof window === "undefined" || !("__TAURI_INTERNALS__" in window)) throw "remote_job_unavailable";
}

export async function listRemoteJobsV4(request: ListRemoteJobsV4Request): Promise<RemoteJobsPageV4> {
  requireDesktop();
  return invoke("list_remote_jobs_v4", { request: { project_id: request.project_id, conversation_id: request.conversation_id } });
}

export async function refreshRemoteJobV4(request: RefreshRemoteJobV4Request): Promise<RemoteJobObservationV4> {
  requireDesktop();
  return invoke("refresh_remote_job_v4", { request: { project_id: request.project_id, job_id: request.job_id } });
}
