import { act, fireEvent, render, renderHook, screen, waitFor } from "@testing-library/react";
import { createElement } from "react";
import { beforeEach, expect, it, vi } from "vitest";
import * as api from "../../conversation-branch-api";
import { useConversationBranchSend } from "./useConversationBranchSend";
import { WorkspaceShell } from "./WorkspaceShell";
import { ComposerDraftStore } from "./composerDrafts";
import type { CreateConversationBranchAndSendRequestV4 } from "../../types";
vi.mock("../../conversation-branch-api", () => ({ conversationBranchCheckpoint: vi.fn(), createConversationBranchAndSend: vi.fn(), getConversationBranch: vi.fn().mockResolvedValue(null) }));
const input = { sourceMessageId: "u", message_markdown: "branch task", mode: "agent" as const, model_profile_id: "model", compute_selection: { schema_version: 4 as const, backend_id: "local", backend_kind: "local" as const, autonomy_mode: "supervised" as const, approval_policy: "risk_based" as const, environment: "system", network_policy: "host_inherited" as const, container_image: null }, references: [{ kind: "artifact" as const, project_id: "p", id: "a" }], attachments: ["file"] };
function receipt(request: CreateConversationBranchAndSendRequestV4) { return { branch: { ...request.branch, branch_conversation_id: "branch", source_sequence: 1, source_head_sequence: 2, boundary_hash: "hash", request_hash: "hash", state: "active" as const, created_at: "now", updated_at: "now" }, queue: { message_markdown: request.message_markdown, mode: request.mode, references: request.references, attachments: request.attachments, attachment_receipts: [], position: 1, revision: 1, status: "pending", created_at: "now", updated_at: "now", frozen: { model_profile_id: request.model_profile_id, model_configuration_hash: "hash", compute_selection: request.compute_selection, conversation_preferences: { delegation_enabled: true, auto_review: true, memory_enabled: true }, service_tier: {}, delegated_model: null, reviewer_model: null }, project_id: request.branch.project_id, message_id: request.queue_message_id, run_id: request.queue_run_id, request_id: request.queue_request_id, conversation_id: "branch" } } as Awaited<ReturnType<typeof api.createConversationBranchAndSend>>; }
beforeEach(() => { vi.clearAllMocks(); vi.mocked(api.conversationBranchCheckpoint).mockResolvedValue({ source_message_id: "u", source_sequence: 1, source_head_sequence: 2, checkpoint_kind: "after_response", boundary_hash: "hash" }); vi.mocked(api.createConversationBranchAndSend).mockImplementation(async (request) => receipt(request)); });
it("retries unknown branch-send outcomes with identical IDs and the complete original payload", async () => {
  vi.mocked(api.createConversationBranchAndSend).mockRejectedValueOnce(new Error("response lost"));
  const opened = vi.fn().mockResolvedValue(true); const { result } = renderHook(() => useConversationBranchSend("p", "c", opened));
  await act(async () => expect(await result.current.submit(input)).toBe(false));
  expect(result.current.pending).toBe(true);
  expect(result.current.error).toBe(true);
  const first = vi.mocked(api.createConversationBranchAndSend).mock.calls[0][0];
  await act(async () => expect(await result.current.retry()).toBe(true));
  expect(vi.mocked(api.createConversationBranchAndSend).mock.calls[1][0]).toEqual(first);
  expect(first.attachments).toEqual(["file"]); expect(first.references).toEqual(input.references);
  expect(api.conversationBranchCheckpoint).toHaveBeenCalledTimes(1);
});
it("does not enqueue again after the committed branch could not be opened", async () => {
  const opened = vi.fn().mockRejectedValueOnce(new Error("list failed")).mockResolvedValue(true);
  const { result } = renderHook(() => useConversationBranchSend("p", "c", opened));
  await act(async () => expect(await result.current.submit(input)).toBe(true));
  expect(result.current.pending).toBe(true);
  expect(result.current.error).toBe(true);
  await act(async () => expect(await result.current.retry()).toBe(true));
  expect(api.createConversationBranchAndSend).toHaveBeenCalledTimes(1);
});
it("keeps long markdown intact while bounding only the title", async () => {
  const { result } = renderHook(() => useConversationBranchSend("p", "c", vi.fn().mockResolvedValue(true)));
  const markdown = "分析".repeat(1000);
  await act(async () => { await result.current.submit({ ...input, message_markdown: markdown }); });
  const request = vi.mocked(api.createConversationBranchAndSend).mock.calls[0][0];
  expect(request.message_markdown).toBe(markdown);
  expect(new TextEncoder().encode(request.branch.title).length).toBeLessThanOrEqual(240);
});

it("retains a committed receipt across scope switches without navigating or sending twice", async () => {
  let resolve!: (value: Awaited<ReturnType<typeof api.createConversationBranchAndSend>>) => void;
  vi.mocked(api.createConversationBranchAndSend).mockImplementation(() => new Promise((yes) => { resolve = yes; }));
  const opened = vi.fn().mockResolvedValue(true);
  const { result, rerender } = renderHook(({ id }) => useConversationBranchSend("p", id, opened), { initialProps: { id: "c" } });
  let work!: Promise<boolean>;
  act(() => { work = result.current.submit(input); });
  await waitFor(() => expect(api.createConversationBranchAndSend).toHaveBeenCalledTimes(1));
  rerender({ id: "other" });
  await act(async () => { resolve(receipt(vi.mocked(api.createConversationBranchAndSend).mock.calls[0][0])); expect(await work).toBe(true); });
  expect(opened).not.toHaveBeenCalled();
  rerender({ id: "c" });
  await act(async () => expect(await result.current.retry()).toBe(true));
  expect(opened).toHaveBeenCalledTimes(1); expect(api.createConversationBranchAndSend).toHaveBeenCalledTimes(1);
});

it.each(["identity", "empty", "null"] as const)("does not report acceptance for an invalid native branch ACK: %s", async (invalid) => {
  vi.mocked(api.createConversationBranchAndSend).mockImplementationOnce(async (request) => {
    if (invalid === "empty") return {} as Awaited<ReturnType<typeof api.createConversationBranchAndSend>>;
    if (invalid === "null") return null as unknown as Awaited<ReturnType<typeof api.createConversationBranchAndSend>>;
    const result = receipt(request); result.queue.request_id = "wrong-request"; return result;
  });
  const opened = vi.fn().mockResolvedValue(true);
  const { result } = renderHook(() => useConversationBranchSend("p", "c", opened));
  await act(async () => expect(await result.current.submit(input)).toBe(false));
  expect(opened).not.toHaveBeenCalled();
  expect(result.current.pending).toBe(true);
  await act(async () => expect(await result.current.retry()).toBe(true));
  expect(api.createConversationBranchAndSend).toHaveBeenCalledTimes(2);
  expect(vi.mocked(api.createConversationBranchAndSend).mock.calls[1][0]).toEqual(vi.mocked(api.createConversationBranchAndSend).mock.calls[0][0]);
});

const project = { id: "p", name: "Branch receipt project", status: "ready" as const, template: "blank" as const };
const backend = { descriptor: { schema_version: 4 as const, backend_id: "local", kind: "local" as const, isolation: "process" as const, available: true, supports_python: true, supports_r: false, supports_network_policy: false }, selectable: true, reason: null, python_status: "available" as const, r_status: "unavailable" as const, resolved_image_id: null };

for (const route of ["branch", "fork"] as const) {
  it.each(["accepted", "same text", "unknown", "invalid", "unmounted"] as const)(`cleans only known accepted original revisions through the real ${route} controller: %s`, async (outcome) => {
    let resolve!: (value: Awaited<ReturnType<typeof api.createConversationBranchAndSend>>) => void;
    let reject!: (reason: unknown) => void;
    vi.mocked(api.createConversationBranchAndSend).mockImplementation(() => new Promise((yes, no) => { resolve = yes; reject = no; }));
    const opened = vi.fn().mockResolvedValue(true);
    const store = new ComposerDraftStore();
    const a = { projectId: "p", conversationId: "c" };
    const b = { projectId: "p", conversationId: "other" };
    store.update(a, route === "fork" ? "/fork Branch notes" : "Branch notes");
    function Harness({ id }: { id: string }) {
      const flow = useConversationBranchSend("p", id, opened);
      return createElement(WorkspaceShell, { project, locale: "en-US", activeConversationId: id, composerDraftStore: store, computeBackends: [backend], computeBackendId: "local", runtimeDraftSelection: input.compute_selection, messages: [{ id: "u", role: "user", markdown: "Original turn" }], onOpenBranch: opened, onBranchSend: (sourceMessageId, markdown) => flow.submit({ ...input, sourceMessageId, message_markdown: markdown, references: [], attachments: [] }), branchSendBusy: flow.busy, branchSendPending: flow.pending, branchSendError: flow.error, onRetryBranchSend: flow.retry });
    }
    const view = render(createElement(Harness, { id: "c" }));
    if (route === "fork") {
      await waitFor(() => expect(screen.getByRole("button", { name: "Send" })).toBeEnabled());
      fireEvent.click(screen.getByRole("button", { name: "Send" }));
    } else {
      fireEvent.click(screen.getByRole("button", { name: "Send options" }));
      fireEvent.click(screen.getByRole("menuitem", { name: /Branch conversation/ }));
    }
    await waitFor(() => expect(api.createConversationBranchAndSend).toHaveBeenCalledOnce());
    if (outcome === "unmounted") { view.unmount(); render(createElement(Harness, { id: "other" })); }
    else view.rerender(createElement(Harness, { id: "other" }));
    fireEvent.change(screen.getByRole("textbox", { name: /Describe/ }), { target: { value: "Destination notes" } });
    if (outcome === "same text") act(() => { store.update(a, "Changed notes"); store.update(a, route === "fork" ? "/fork Branch notes" : "Branch notes"); });
    await act(async () => {
      if (outcome === "unknown") reject(new Error("lost native ACK"));
      else {
        const accepted = receipt(vi.mocked(api.createConversationBranchAndSend).mock.calls[0][0]);
        if (outcome === "invalid") accepted.queue.conversation_id = "wrong-branch";
        resolve(accepted);
      }
    });
    expect(store.read(a).text).toBe(outcome === "accepted" || outcome === "unmounted" ? "" : route === "fork" ? "/fork Branch notes" : "Branch notes");
    expect(store.read(b).text).toBe("Destination notes");
    expect(screen.getByRole("textbox", { name: /Describe/ })).toHaveValue("Destination notes");
    expect(opened).not.toHaveBeenCalled();
    if (outcome === "accepted" || outcome === "same text") {
      view.rerender(createElement(Harness, { id: "c" }));
      fireEvent.click(screen.getByRole("button", { name: "Retry original branch send" }));
      await waitFor(() => expect(opened).toHaveBeenCalledOnce());
      expect(store.read(a).text).toBe(outcome === "accepted" ? "" : route === "fork" ? "/fork Branch notes" : "Branch notes");
    }
    expect(api.createConversationBranchAndSend).toHaveBeenCalledOnce();
  });
}
