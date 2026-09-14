import { beforeEach, describe, expect, it, vi } from "vitest";

import { invoke } from "@tauri-apps/api/core";
import {
  DEFAULT_REVIEWER_SETTINGS,
  getReviewerSettingsV4,
  getSessionReviewV4,
  listSessionReviewsV4,
  saveReviewerSettingsV4,
  startSessionReviewV4,
} from "./session-review-api";
import type { ReviewerSettingsV4, SessionReviewRecordV4 } from "./types";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

const invokeMock = vi.mocked(invoke);

const record: SessionReviewRecordV4 = {
  id: "review-1",
  project_id: "project-a",
  conversation_id: "conversation-a",
  reviewer_profile_id: "profile-a",
  reviewer_configuration_hash: "config-hash",
  source_snapshot_sha256: "source-hash",
  source_message_count: 1,
  sources: [{ message_id: "message-1", sequence: 1, role: "user", text: "Evidence" }],
  status: "running",
  report: null,
  error: null,
  service_tier: { fast_mode: null },
  created_at: "2026-09-14T00:00:00.000Z",
  updated_at: "2026-09-14T00:00:00.000Z",
};

beforeEach(() => {
  vi.clearAllMocks();
  delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
});

describe("session review API", () => {
  it("uses safe browser fallbacks without inventing reviews or pretending to save settings", async () => {
    await expect(getReviewerSettingsV4()).resolves.toEqual(DEFAULT_REVIEWER_SETTINGS);
    await expect(listSessionReviewsV4("project-a", "conversation-a")).resolves.toEqual([]);
    await expect(getSessionReviewV4("project-a", "conversation-a", "review-1")).resolves.toBeNull();
    await expect(saveReviewerSettingsV4(DEFAULT_REVIEWER_SETTINGS)).rejects.toThrow("desktop app");
    await expect(startSessionReviewV4({ request_id: "request-1", project_id: "project-a", conversation_id: "conversation-a", model_profile_id: "profile-a" })).rejects.toThrow("desktop app");
    expect(invokeMock).not.toHaveBeenCalled();
  });

  it("sends the native DTO payloads with stable command names and scope", async () => {
    (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {};
    const settings: ReviewerSettingsV4 = { backend: { kind: "http_profile", profile_id: "profile-a" }, default_http_profile_id: "profile-b" };
    invokeMock
      .mockResolvedValueOnce(settings)
      .mockResolvedValueOnce(settings)
      .mockResolvedValueOnce(record)
      .mockResolvedValueOnce([record])
      .mockResolvedValueOnce(record);

    await expect(getReviewerSettingsV4()).resolves.toEqual(settings);
    await expect(saveReviewerSettingsV4(settings)).resolves.toEqual(settings);
    await expect(startSessionReviewV4({ request_id: "request-1", project_id: "project-a", conversation_id: "conversation-a", model_profile_id: "profile-a" })).resolves.toEqual(record);
    await expect(listSessionReviewsV4("project-a", "conversation-a")).resolves.toEqual([record]);
    await expect(getSessionReviewV4("project-a", "conversation-a", "request-1")).resolves.toEqual(record);

    expect(invokeMock).toHaveBeenNthCalledWith(1, "reviewer_get_settings_v4");
    expect(invokeMock).toHaveBeenNthCalledWith(2, "reviewer_save_settings_v4", { settings });
    expect(invokeMock).toHaveBeenNthCalledWith(3, "session_start_review_v4", { request: { request_id: "request-1", project_id: "project-a", conversation_id: "conversation-a", model_profile_id: "profile-a" } });
    expect(invokeMock).toHaveBeenNthCalledWith(4, "session_list_reviews_v4", { projectId: "project-a", conversationId: "conversation-a" });
    expect(invokeMock).toHaveBeenNthCalledWith(5, "session_get_review_v4", { projectId: "project-a", conversationId: "conversation-a", requestId: "request-1" });
  });
});
