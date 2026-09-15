import { invoke } from "@tauri-apps/api/core";
import type {
  CreateManagedCredentialRequest,
  CreateCredentialResult,
  CredentialEntry,
  DeleteCredentialResult,
  ReplaceCredentialRequest,
} from "./types";

export function listCredentials(): Promise<CredentialEntry[]> {
  return invoke("settings_list_credentials");
}

export function createCredential(request: CreateManagedCredentialRequest): Promise<CreateCredentialResult> {
  return invoke("settings_create_credential", { request });
}

export function replaceCredential(request: ReplaceCredentialRequest): Promise<CredentialEntry> {
  return invoke("settings_replace_credential", { request });
}

export function deleteCredential(id: string): Promise<DeleteCredentialResult> {
  return invoke("settings_delete_credential", { id });
}
