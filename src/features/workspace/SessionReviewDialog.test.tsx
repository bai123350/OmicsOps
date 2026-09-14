import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { useState } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import type { ModelProfile, SessionReviewRecordV4 } from "../../types";
import { SessionReviewDialog } from "./SessionReviewDialog";

const profile: ModelProfile = {
  id: "profile-a",
  label: "Review model",
  provider: "open_ai_compatible",
  base_url: "https://api.openai.com/v1",
  model: "gpt-6-astra",
  credential_reference: "credential:profile-a",
  supports_tools: true,
  supports_vision: true,
};

function review(overrides: Partial<SessionReviewRecordV4> = {}): SessionReviewRecordV4 {
  return {
    id: "review-1",
    project_id: "project-a",
    conversation_id: "conversation-a",
    reviewer_profile_id: "profile-a",
    reviewer_configuration_hash: "config-hash",
    source_snapshot_sha256: "source-hash",
    source_message_count: 3,
    sources: [
      { message_id: "message-1", sequence: 1, role: "user", text: "The first method" },
      { message_id: "message-2", sequence: 2, role: "assistant", text: "The first result" },
    ],
    status: "completed",
    report: { summary: "The evidence is partial.", findings: [{ severity: "warn", code: "missing-control", message: "A control is not described.", source_ids: ["message-1"] }] },
    error: null,
    created_at: "2026-09-14T00:00:00.000Z",
    updated_at: "2026-09-14T00:00:02.000Z",
    ...overrides,
  };
}

function renderDialog(overrides: Partial<React.ComponentProps<typeof SessionReviewDialog>> = {}) {
  return render(<SessionReviewDialog locale="en-US" modelProfiles={[profile]} records={[]} onStartReview={vi.fn()} onClose={vi.fn()} {...overrides} />);
}

beforeEach(() => vi.clearAllMocks());

describe("SessionReviewDialog", () => {
  it("renders the newest stored report with frozen IDs, hashes, source coverage, and linked evidence", () => {
    const older = review({ id: "older", updated_at: "2026-09-14T00:00:01.000Z", report: { summary: "Older report", findings: [] } });
    const newer = review({ id: "newer", updated_at: "2026-09-14T00:00:03.000Z", report: { summary: "Newest report", findings: [{ severity: "warn", code: "missing-control", message: "A control is not described.", source_ids: ["message-1"] }] } });
    renderDialog({ records: [newer, older] });

    expect(screen.getByText("Newest report")).toBeInTheDocument();
    expect(screen.queryByText("Older report")).not.toBeInTheDocument();
    expect(screen.getAllByText("2 / 3 messages excerpted").length).toBeGreaterThan(0);
    expect(screen.getByText("config-hash")).toBeInTheDocument();
    expect(screen.getByText("source-hash")).toBeInTheDocument();
    expect(screen.getByText("Review model")).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "message-1" })).toHaveAttribute("href", "#session-review-source-message-1");
    expect(screen.getByText("The first method")).toBeInTheDocument();
    expect(screen.queryByText(/scientifically verified/i)).not.toBeInTheDocument();
  });

  it("keeps a running review readable while locking Request review", () => {
    renderDialog({ records: [review({ status: "running", report: null })], busy: true });
    expect(screen.getByRole("button", { name: "Reviewing…" })).toBeDisabled();
    expect(screen.getByRole("status")).toHaveTextContent("continues in the background");
  });

  it("prevents duplicate starts and reports a safe local error", async () => {
    const onStartReview = vi.fn().mockRejectedValue(new Error("private provider response"));
    renderDialog({ onStartReview });
    const button = screen.getByRole("button", { name: "Request review" });
    fireEvent.click(button);
    fireEvent.click(button);
    expect(onStartReview).toHaveBeenCalledOnce();
    expect(await screen.findByRole("alert")).toHaveTextContent("Could not start the review");
    expect(screen.queryByText("private provider response")).not.toBeInTheDocument();
  });

  it("localizes review errors in Chinese and keeps retry available without exposing provider details", () => {
    const onRetry = vi.fn();
    const { rerender } = renderDialog({
      locale: "zh-CN",
      error: "Could not refresh the review status. Please retry.",
      onRetry,
    });
    expect(screen.getByRole("alert")).toHaveTextContent("无法刷新审核状态，请重试。");
    expect(screen.getByRole("button", { name: "重试" })).toBeEnabled();

    rerender(<SessionReviewDialog locale="zh-CN" records={[]} onStartReview={vi.fn()} onClose={vi.fn()} error="private provider response" onRetry={onRetry} />);
    expect(screen.getByRole("alert")).toHaveTextContent("审核操作未完成，请重试。");
    expect(screen.getByRole("alert")).not.toHaveTextContent("private provider response");
  });

  it("closes on immediate window Escape, traps focus, and restores the launch button", async () => {
    function Host() {
      const [open, setOpen] = useState(false);
      return <><button type="button" onClick={() => setOpen(true)}>Launch review</button>{open && <SessionReviewDialog locale="en-US" records={[]} onStartReview={vi.fn()} onClose={() => setOpen(false)} />}</>;
    }

    render(<Host />);
    const launch = screen.getByRole("button", { name: "Launch review" });
    launch.focus();
    fireEvent.click(launch);
    const dialog = await screen.findByRole("dialog", { name: "Session review" });
    const close = within(dialog).getByRole("button", { name: "Close session review" });
    expect(document.activeElement).toBe(close);
    fireEvent.keyDown(close, { key: "Tab" });
    expect(document.activeElement).toBe(within(dialog).getByRole("button", { name: "Request review" }));
    fireEvent.keyDown(document.activeElement!, { key: "Tab", shiftKey: true });
    expect(document.activeElement).toBe(close);
    fireEvent.keyDown(window, { key: "Escape" });
    await waitFor(() => expect(screen.queryByRole("dialog", { name: "Session review" })).not.toBeInTheDocument());
    expect(document.activeElement).toBe(launch);
  });

  it("keeps Request review disabled while an ordinary Agent run is active without presenting it as review work", () => {
    renderDialog({ startDisabled: true });
    const button = screen.getByRole("button", { name: "Request review" });
    expect(button).toBeDisabled();
    expect(screen.queryByText("Reviewing…")).not.toBeInTheDocument();
  });
});
