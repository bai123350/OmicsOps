import { invoke } from "@tauri-apps/api/core";
import type { UsageAggregatePage, UsageConversationPage, UsageFilter } from "./types";

function requireDesktop() {
  if (typeof window === "undefined" || !("__TAURI_INTERNALS__" in window)) {
    throw new Error("Usage settings require the desktop app.");
  }
}

export async function settingsUsagePage(filter: UsageFilter, cursor: string | null = null): Promise<UsageAggregatePage> {
  requireDesktop();
  return invoke("settings_usage_page", { filter, cursor });
}

export async function settingsUsageConversations(filter: UsageFilter, cursor: string | null = null): Promise<UsageConversationPage> {
  requireDesktop();
  return invoke("settings_usage_conversations", { filter, cursor });
}
