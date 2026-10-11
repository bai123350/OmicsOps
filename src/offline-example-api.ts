import { invoke } from "@tauri-apps/api/core";
import type { CreateOfflineExampleWorkspaceRequest, CreateOfflineExampleWorkspaceResult, OfflineExampleWorkspaceView } from "./types";

function requireDesktop() {
  if (typeof window === "undefined" || !("__TAURI_INTERNALS__" in window)) throw new Error("example_platform_unavailable");
}

export async function createOfflineExampleWorkspace(request: CreateOfflineExampleWorkspaceRequest): Promise<CreateOfflineExampleWorkspaceResult> {
  requireDesktop();
  return invoke("create_offline_example_workspace", { request });
}

export async function getOfflineExampleWorkspace(projectId: string): Promise<OfflineExampleWorkspaceView | null> {
  requireDesktop();
  return invoke("get_offline_example_workspace", { projectId });
}
