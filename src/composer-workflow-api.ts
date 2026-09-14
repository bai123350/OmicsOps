import { invoke } from "@tauri-apps/api/core";
import type { ComposerWorkflowTemplate, SaveComposerWorkflowRequest } from "./types";

export async function listComposerWorkflows(projectId: string): Promise<ComposerWorkflowTemplate[]> {
  if (!("__TAURI_INTERNALS__" in window)) return [];
  return invoke("list_composer_workflows", { projectId });
}

export async function saveComposerWorkflow(request: SaveComposerWorkflowRequest): Promise<ComposerWorkflowTemplate> {
  if (!("__TAURI_INTERNALS__" in window)) throw new Error("Workflow editing requires the desktop app.");
  return invoke("save_composer_workflow", { request });
}
