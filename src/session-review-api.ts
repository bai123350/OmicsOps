import { invoke } from "@tauri-apps/api/core";

import type {
  ReviewerSettingsV4,
  SessionReviewRecordV4,
  SessionReviewRequestV4,
  SessionReviewStartErrorV4,
} from "./types";

export const DEFAULT_REVIEWER_SETTINGS: ReviewerSettingsV4 = {
  backend: { kind: "follow_session" },
  default_http_profile_id: null,
};

export function isDesktopReviewHost(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

export async function getReviewerSettingsV4(): Promise<ReviewerSettingsV4> {
  if (!isDesktopReviewHost()) return { ...DEFAULT_REVIEWER_SETTINGS, backend: { ...DEFAULT_REVIEWER_SETTINGS.backend } };
  return invoke<ReviewerSettingsV4>("reviewer_get_settings_v4");
}

export async function saveReviewerSettingsV4(settings: ReviewerSettingsV4): Promise<ReviewerSettingsV4> {
  if (!isDesktopReviewHost()) throw new Error("Reviewer settings require the desktop app.");
  return invoke<ReviewerSettingsV4>("reviewer_save_settings_v4", { settings });
}

export async function startSessionReviewV4(request: SessionReviewRequestV4): Promise<SessionReviewRecordV4> {
  if (!isDesktopReviewHost()) throw { kind: "rejected", message: "Session reviews require the desktop app." } satisfies SessionReviewStartErrorV4;
  return invoke<SessionReviewRecordV4>("session_start_review_v4", { request });
}

export async function listSessionReviewsV4(projectId: string, conversationId: string): Promise<SessionReviewRecordV4[]> {
  if (!isDesktopReviewHost()) return [];
  return invoke<SessionReviewRecordV4[]>("session_list_reviews_v4", { projectId, conversationId });
}

export async function getSessionReviewV4(projectId: string, conversationId: string, requestId: string): Promise<SessionReviewRecordV4 | null> {
  if (!isDesktopReviewHost()) return null;
  return invoke<SessionReviewRecordV4 | null>("session_get_review_v4", { projectId, conversationId, requestId });
}

export function isRejectedReviewStart(error: unknown): error is SessionReviewStartErrorV4 {
  return Boolean(error && typeof error === "object" && "kind" in error && error.kind === "rejected"
    && "message" in error && typeof error.message === "string");
}
