import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { expect, it, vi } from "vitest";
import type { SideChatTurnV4 } from "../../types";
import { SideChatPanel } from "./SideChatPanel";
const turn: SideChatTurnV4 = { id: "r", request_id: "r", project_id: "p", conversation_id: "c", model_profile_id: "m", model_label: "Side model", source_snapshot_sha256: "snapshot", source_watermark: { message_count: 0, event_count: 0, event_heads: [] }, question_markdown: "Why?", references: [], attachments: [], sources: [{ source_id: "message:u", message_id: "u", sequence: 1, role: "user", label: "Original question", excerpt: "Compare batches" }], status: "completed", answer_markdown: "The batches differ.", cited_source_ids: ["message:u"], created_at: "2026-09-14T00:00:00Z", updated_at: "2026-09-14T00:00:01Z" };
const base = { projectId: "p", conversationId: "c", locale: "en-US" as const, records: [turn], modelId: "m", models: [{ id: "m", label: "Side model" }], onModelChange: vi.fn(), onSend: vi.fn(), onRefresh: vi.fn() };
it("shows an independent answer with inspectable source excerpts", () => {
  const open = vi.fn(); render(<SideChatPanel {...base} onOpenSource={open} />);
  expect(screen.getByText("The batches differ.")).toBeInTheDocument();
  fireEvent.click(screen.getByText("Sources (1)"));
  expect(screen.getByText("Compare batches")).toBeVisible();
  fireEvent.click(screen.getByRole("button", { name: "Go to source message" }));
  expect(open).toHaveBeenCalledWith("u");
});
it("preserves a changed draft when an earlier question is accepted", async () => {
  let done!: (accepted: boolean) => void;
  const send = vi.fn(() => new Promise<boolean>((resolve) => { done = resolve; }));
  render(<SideChatPanel {...base} onSend={send} />);
  const input = screen.getByRole("textbox", { name: "Side question" });
  fireEvent.change(input, { target: { value: "First question" } });
  fireEvent.click(screen.getByRole("button", { name: "Ask" }));
  fireEvent.change(input, { target: { value: "Next question" } });
  done(true);
  await waitFor(() => expect(input).toHaveValue("Next question"));
  expect(send).toHaveBeenCalledWith("First question");
});
it("keeps unknown question text and exposes reconciliation instead of a fresh send", async () => {
  const view = render(<SideChatPanel {...base} onSend={vi.fn().mockResolvedValue(false)} />);
  const input = screen.getByRole("textbox", { name: "Side question" });
  fireEvent.change(input, { target: { value: "Pending question" } });
  fireEvent.click(screen.getByRole("button", { name: "Ask" }));
  await waitFor(() => expect(screen.getByRole("button", { name: "Ask" })).toBeEnabled());
  const retry = vi.fn().mockResolvedValue(true);
  view.rerender(<SideChatPanel {...base} pending originalQuestion="Pending question" onRetry={retry} />);
  expect(screen.getByRole("button", { name: "Ask" })).toBeDisabled();
  fireEvent.click(screen.getByRole("button", { name: "Reconcile original question" }));
  await waitFor(() => expect(input).toHaveValue(""));
  expect(retry).toHaveBeenCalledOnce();
});
it("filters foreign records and offers a fresh linked attempt only for terminal failures", () => {
  const retry = vi.fn();
  render(<SideChatPanel {...base} records={[{ ...turn, conversation_id: "foreign", answer_markdown: "Private" }, { ...turn, status: "interrupted", answer_markdown: null }]} onRetryTurn={retry} />);
  expect(screen.queryByText("Private")).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "Ask again" }));
  expect(retry).toHaveBeenCalledWith(expect.objectContaining({ request_id: "r", status: "interrupted" }));
});
