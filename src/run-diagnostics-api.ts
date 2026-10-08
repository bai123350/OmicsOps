import { invoke } from "@tauri-apps/api/core";
import type { RunDiagnosticsRequestV4, RunDiagnosticsSnapshotV4, SaveRunDiagnosticsRequestV4 } from "./types";

function requireDesktop() {
  if (typeof window === "undefined" || !("__TAURI_INTERNALS__" in window)) throw "diagnostics_unavailable";
}

export async function getRunDiagnostics(request: RunDiagnosticsRequestV4): Promise<RunDiagnosticsSnapshotV4> {
  requireDesktop();
  return invoke("get_run_diagnostics_v4", { request });
}

export async function saveRunDiagnostics(request: SaveRunDiagnosticsRequestV4): Promise<string | null> {
  requireDesktop();
  return invoke("save_run_diagnostics_v4", { request });
}
