import { StrictMode } from "react";
import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, expect, it, vi } from "vitest";

import type { AgentRunEventV4 } from "./types";
import { useRunNotifications } from "./use-run-notifications";

const api = vi.hoisted(() => ({
  notifyRunEvent: vi.fn(),
  onRunNotificationCandidate: vi.fn(),
}));
vi.mock("./notification-settings-api", () => api);

function event(hash: string, kind: AgentRunEventV4["event"]): AgentRunEventV4 {
  return {
    schema_version: 4,
    run_id: "run-background",
    project_id: "project-background",
    conversation_id: "conversation-background",
    sequence: 7,
    occurred_at: "2026-09-15T00:00:00Z",
    previous_hash: "",
    event_hash: hash,
    event: kind,
  };
}

beforeEach(() => {
  vi.clearAllMocks();
  api.notifyRunEvent.mockResolvedValue({ outcome: "sent" });
});

it("does not hydrate history and sends one candidate across StrictMode duplicate listeners", async () => {
  const listeners: Array<(value: AgentRunEventV4) => void> = [];
  api.onRunNotificationCandidate.mockImplementation(async (listener) => {
    listeners.push(listener);
    return vi.fn();
  });
  renderHook(() => useRunNotifications(vi.fn()), {
    wrapper: ({ children }) => <StrictMode>{children}</StrictMode>,
  });
  await waitFor(() => expect(listeners.length).toBeGreaterThan(0));
  expect(api.notifyRunEvent).not.toHaveBeenCalled();

  const completed = event("1".repeat(64), { kind: "run_completed" });
  await act(async () => listeners.forEach((listener) => listener(completed)));
  await waitFor(() => expect(api.notifyRunEvent).toHaveBeenCalledTimes(1));
  expect(api.notifyRunEvent).toHaveBeenCalledWith("run-background", "1".repeat(64));
});

it("ignores non-notifiable live events and reports native command failures", async () => {
  let listener!: (value: AgentRunEventV4) => void;
  api.onRunNotificationCandidate.mockImplementation(async (next) => {
    listener = next;
    return vi.fn();
  });
  const onFailure = vi.fn();
  renderHook(() => useRunNotifications(onFailure));
  await waitFor(() => expect(listener).toBeDefined());
  act(() => listener(event("2".repeat(64), { kind: "model_text", text: "history-like text" })));
  expect(api.notifyRunEvent).not.toHaveBeenCalled();

  api.notifyRunEvent.mockRejectedValueOnce(new Error("notification bridge unavailable"));
  act(() => listener(event("3".repeat(64), { kind: "run_failed", message: "private" })));
  await waitFor(() => expect(onFailure).toHaveBeenCalledWith("notification bridge unavailable"));
});

it("surfaces an at-most-once native delivery failure result", async () => {
  let listener!: (value: AgentRunEventV4) => void;
  api.onRunNotificationCandidate.mockImplementation(async (next) => {
    listener = next;
    return vi.fn();
  });
  api.notifyRunEvent.mockResolvedValueOnce({ outcome: "failed" });
  const onFailure = vi.fn();
  renderHook(() => useRunNotifications(onFailure));
  await waitFor(() => expect(listener).toBeDefined());
  act(() => listener(event("4".repeat(64), { kind: "run_needs_attention", message: "private" })));
  await waitFor(() => expect(onFailure).toHaveBeenCalledWith("系统通知发送失败"));
});

it("cleans up the independent live subscription", async () => {
  const unlisten = vi.fn();
  api.onRunNotificationCandidate.mockResolvedValue(unlisten);
  const { unmount } = renderHook(() => useRunNotifications(vi.fn()));
  await waitFor(() => expect(api.onRunNotificationCandidate).toHaveBeenCalled());
  unmount();
  await waitFor(() => expect(unlisten).toHaveBeenCalled());
});
