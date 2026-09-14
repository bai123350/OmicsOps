import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { expect, it, vi } from "vitest";
import type { ComposerQueueItemV4 } from "../../types";
import { WorkspaceShell } from "./WorkspaceShell";
import type { SideChatController } from "./useSideChat";
const base = { project: { id: "p", name: "test", status: "ready" as const, template: "blank" as const }, locale: "en-US" as const, onLocaleChange: vi.fn(), activeConversationId: "c" };
function controller(): SideChatController {
  return { records: [], loading: false, ready: true, busy: false, pending: false, error: false, modelId: "model", setModelId: vi.fn(), draft: "", setDraft: vi.fn(), originalQuestion: undefined, send: vi.fn().mockResolvedValue(true), retry: vi.fn().mockResolvedValue(true), refresh: vi.fn(), hasMore: false, loadingOlder: false, loadOlder: vi.fn() };
}
function openSideChat() { fireEvent.click(screen.getByRole("button", { name: "Send options" })); fireEvent.click(screen.getByRole("menuitem", { name: /Side chat/ })); }
it("opens an empty side chat during the main run and immediately closes it with Escape", () => {
  const side = controller(); render(<WorkspaceShell {...base} agentBusy onQueue={vi.fn()} sideChat={side} />);
  openSideChat();
  expect(screen.getByRole("region", { name: "Side chat" })).toBeInTheDocument();
  expect(side.send).not.toHaveBeenCalled();
  fireEvent.keyDown(window, { key: "Escape" });
  expect(screen.queryByRole("region", { name: "Side chat" })).toBeNull();
  expect(screen.getByRole("textbox", { name: /Describe/ })).toBeInTheDocument();
});
it("passes all restored material to side chat without sending a primary turn", async () => {
  const side = controller(); const main = vi.fn();
  const item: ComposerQueueItemV4 = {
    request_id: "q", message_id: "m", run_id: "r", project_id: "p", conversation_id: "c", mode: "agent", message_markdown: "Explain these counts", position: 1, revision: 1, status: "cancelled", created_at: "now", updated_at: "now",
    frozen: { model_profile_id: "model", model_configuration_hash: "hash", compute_selection: { schema_version: 4, backend_id: "local", backend_kind: "local", autonomy_mode: "supervised", approval_policy: "risk_based", environment: "system", network_policy: "host_inherited", container_image: null }, conversation_preferences: { delegation_enabled: true, auto_review: true, memory_enabled: true }, service_tier: {}, delegated_model: null, reviewer_model: null },
    references: [{ kind: "artifact", project_id: "p", id: "artifact" }], attachments: ["file"], attachment_receipts: [{ id: "file", project_id: "p", conversation_id: "c", name: "counts.csv", size_bytes: 4, media_type: "text/csv", relative_path: ".omicsops/attachments/file/counts.csv", sha256: "hash" }],
  };
  render(<WorkspaceShell {...base} onQueue={main} sideChat={side} queueItems={[item]} onQueueUpdate={vi.fn()} onQueueAction={vi.fn()} />);
  fireEvent.click(screen.getByRole("button", { name: "Restore to composer" }));
  await waitFor(() => expect(screen.getByRole("textbox", { name: /Describe/ })).toHaveValue(item.message_markdown));
  openSideChat();
  await waitFor(() => expect(side.send).toHaveBeenCalledWith({ question_markdown: item.message_markdown, references: item.references, attachments: item.attachments }));
  expect(main).not.toHaveBeenCalled();
  expect(screen.getByRole("textbox", { name: /Describe/ })).toHaveValue("");
  expect(screen.queryByRole("button", { name: "Remove reference: artifact" })).toBeNull();
});
it("does not clear an identically worded draft in another conversation after a late receipt", async () => {
  const side = controller(); let finish!: (accepted: boolean) => void;
  side.send = vi.fn(() => new Promise<boolean>((resolve) => { finish = resolve; }));
  const view = render(<WorkspaceShell {...base} onQueue={vi.fn()} sideChat={side} />);
  fireEvent.change(screen.getByRole("textbox", { name: /Describe/ }), { target: { value: "Same words" } }); openSideChat();
  view.rerender(<WorkspaceShell {...base} activeConversationId="other" onQueue={vi.fn()} sideChat={controller()} />);
  fireEvent.change(screen.getByRole("textbox", { name: /Describe/ }), { target: { value: "Same words" } });
  await act(async () => { finish(true); });
  expect(screen.getByRole("textbox", { name: /Describe/ })).toHaveValue("Same words");
});
it("preserves a reference removed and re-added while side chat is accepting", async () => {
  const side = controller(); let finish!: (accepted: boolean) => void;
  side.send = vi.fn(() => new Promise<boolean>((resolve) => { finish = resolve; }));
  const item = { reference: { kind: "artifact" as const, project_id: "p", id: "a" }, label: "Counts", description: "" };
  const props = { ...base, onQueue: vi.fn(), sideChat: side };
  const view = render(<WorkspaceShell {...props} searchRequest={{ key: "first", kind: "attach", projectId: "p", conversationId: "c", item }} />);
  fireEvent.change(screen.getByRole("textbox", { name: /Describe/ }), { target: { value: "Explain" } });
  openSideChat();
  fireEvent.click(screen.getByRole("button", { name: "Remove reference: Counts" }));
  view.rerender(<WorkspaceShell {...props} searchRequest={{ key: "again", kind: "attach", projectId: "p", conversationId: "c", item }} />);
  await act(async () => { finish(true); });
  expect(screen.getByRole("button", { name: "Remove reference: Counts" })).toBeInTheDocument();
});
