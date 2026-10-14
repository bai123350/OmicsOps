import { useState } from "react";
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { ShelvedConversationsDialog } from "./ShelvedConversationsDialog";
import { ResearchDialog } from "./ResearchPagePrimitives";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
const row = (id: string, revision = 1, projectId = "p") => ({ state: { project_id: projectId, conversation_id: id, visibility: "shelved", revision, changed_at: null }, title: "Same title", conversation_status: "running", conversation_created_at: "2026-10-04T00:00:00Z", conversation_updated_at: "2026-10-04T00:00:00Z" });
const page = (id: string, next: number | null = null, projectId = "p") => ({ items: [row(id, 1, projectId)], next_offset: next });
const props = () => ({ projectId: "p", locale: "en-US" as const, onClose: vi.fn(), onOpenConversation: vi.fn(), onSetVisibility: vi.fn().mockResolvedValue(undefined) });
function deferred<T>() { let resolve!: (value: T) => void; let reject!: (cause: unknown) => void; const promise = new Promise<T>((done, fail) => { resolve = done; reject = fail; }); return { promise, resolve, reject }; }
beforeEach(() => { vi.mocked(invoke).mockReset(); Object.defineProperty(window, "__TAURI_INTERNALS__", { configurable: true, value: {} }); });

it("visibility_dialog_query_and_page resets refresh and restore to page zero using row ID/revision", async () => {
  vi.mocked(invoke).mockResolvedValueOnce({ items: [row("hidden-a", 3), row("hidden-b", 7)], next_offset: 50 }).mockResolvedValueOnce(page("page-two")).mockResolvedValueOnce(page("refreshed", 50)).mockResolvedValueOnce(page("page-two-again")).mockResolvedValueOnce(page("after-restore")).mockResolvedValueOnce(page("searched"));
  const callbacks = props(); render(<ShelvedConversationsDialog {...callbacks} />);
  await screen.findByText("hidden-b");
  expect(invoke).toHaveBeenLastCalledWith("workspace_list_conversation_visibility", { request: { project_id: "p", visibility: "shelved", query: "", offset: 0, limit: 50 } });
  fireEvent.click(screen.getByRole("button", { name: "Next page" })); await screen.findByText("page-two");
  expect(invoke).toHaveBeenLastCalledWith("workspace_list_conversation_visibility", { request: { project_id: "p", visibility: "shelved", query: "", offset: 50, limit: 50 } });
  expect(screen.queryByText("hidden-a")).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Refresh" })); await screen.findByText("refreshed");
  expect(vi.mocked(invoke).mock.calls.at(-1)?.[1]).toMatchObject({ request: { offset: 0 } });
  fireEvent.click(screen.getByRole("button", { name: "Next page" })); await screen.findByText("page-two-again");
  fireEvent.click(screen.getByRole("button", { name: "Restore" })); await screen.findByText("after-restore");
  expect(callbacks.onSetVisibility).toHaveBeenCalledWith("page-two-again", "visible", 1);
  expect(vi.mocked(invoke).mock.calls.at(-1)?.[1]).toMatchObject({ request: { offset: 0 } });
  fireEvent.change(screen.getByRole("textbox", { name: "Search shelved conversations" }), { target: { value: "RNA" } }); await screen.findByText("searched");
  expect(vi.mocked(invoke).mock.calls.at(-1)?.[1]).toMatchObject({ request: { query: "RNA", offset: 0 } });
});

it("visibility_dialog_open_does_not_restore and duplicate titles keep exact identities", async () => {
  vi.mocked(invoke).mockResolvedValue({ items: [row("hidden-a", 3), row("hidden-b", 7)], next_offset: null });
  const callbacks = props(); render(<ShelvedConversationsDialog {...callbacks} />);
  const id = await screen.findByText("hidden-b"); const item = id.closest("li")!;
  fireEvent.click(within(item).getByRole("button", { name: "Open" }));
  await waitFor(() => expect(callbacks.onOpenConversation).toHaveBeenCalledWith("hidden-b"));
  expect(callbacks.onSetVisibility).not.toHaveBeenCalled();
  fireEvent.click(within(item).getByRole("button", { name: "Restore" }));
  await waitFor(() => expect(callbacks.onSetVisibility).toHaveBeenCalledWith("hidden-b", "visible", 7));
});

it("visibility_dialog_scope_and_escape discards old project/query/page responses and closes only the top layer", async () => {
  const oldProject = deferred<ReturnType<typeof page>>(); const oldQuery = deferred<ReturnType<typeof page>>(); const oldPage = deferred<ReturnType<typeof page>>();
  vi.mocked(invoke).mockReturnValueOnce(oldProject.promise).mockReturnValueOnce(oldQuery.promise).mockResolvedValueOnce(page("current", 50, "q")).mockReturnValueOnce(oldPage.promise).mockResolvedValueOnce(page("refresh-current", null, "q"));
  const callbacks = props(); const parentClose = vi.fn();
  function View({ projectId }: { projectId: string }) { const [open, setOpen] = useState(true); return <ResearchDialog title="Parent" zh={false} onClose={parentClose}>{open && <ShelvedConversationsDialog {...callbacks} projectId={projectId} onClose={() => { callbacks.onClose(); setOpen(false); }} />}</ResearchDialog>; }
  const view = render(<View projectId="p" />); view.rerender(<View projectId="q" />);
  fireEvent.change(screen.getByRole("textbox", { name: "Search shelved conversations" }), { target: { value: "new" } }); await screen.findByText("current");
  await act(async () => { oldProject.resolve(page("stale-project")); oldQuery.resolve(page("stale-query", null, "q")); });
  expect(screen.queryByText("stale-project")).not.toBeInTheDocument(); expect(screen.queryByText("stale-query")).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Next page" })); fireEvent.click(screen.getByRole("button", { name: "Refresh" })); await screen.findByText("refresh-current");
  await act(async () => oldPage.resolve(page("stale-page", null, "q"))); expect(screen.queryByText("stale-page")).not.toBeInTheDocument();
  fireEvent.keyDown(window, { key: "Escape" });
  expect(callbacks.onClose).toHaveBeenCalledTimes(1); expect(parentClose).not.toHaveBeenCalled(); expect(screen.getByRole("dialog", { name: "Parent" })).toBeVisible();
  expect(callbacks.onSetVisibility).not.toHaveBeenCalled();
});

it("visibility_dialog_scope_and_escape works immediately without focus or a list response", () => {
  vi.mocked(invoke).mockReturnValue(new Promise(() => {})); const callbacks = props(); const parentClose = vi.fn();
  function Parent() { const [open, setOpen] = useState(false); return <ResearchDialog title="Parent" zh={false} onClose={parentClose}><button onClick={() => setOpen(true)}>Show collection</button>{open && <ShelvedConversationsDialog {...callbacks} />}</ResearchDialog>; }
  render(<Parent />); fireEvent.click(screen.getByRole("button", { name: "Show collection" }));
  fireEvent.keyDown(window, { key: "Escape" }); expect(callbacks.onClose).toHaveBeenCalledTimes(1); expect(parentClose).not.toHaveBeenCalled(); expect(callbacks.onSetVisibility).not.toHaveBeenCalled();
});

it("stops pagination at the native offset bound and requests narrower search", async () => {
  vi.mocked(invoke).mockResolvedValue(page("last", 10001)); render(<ShelvedConversationsDialog {...props()} />);
  await screen.findByText("Pagination limit reached. Narrow the search.");
  expect(screen.queryByRole("button", { name: "Next page" })).not.toBeInTheDocument(); expect(invoke).toHaveBeenCalledTimes(1);
});

it("renders safe action failures without restoring when open is unavailable", async () => {
  vi.mocked(invoke).mockResolvedValue(page("unavailable")); const callbacks = props(); callbacks.onOpenConversation.mockRejectedValue({ code: "not_found", message: "SECRET host message" });
  callbacks.onSetVisibility.mockRejectedValue({ code: "revision_conflict", message: "SECRET conflict" });
  render(<ShelvedConversationsDialog {...callbacks} />); await screen.findByText("unavailable"); fireEvent.click(screen.getByRole("button", { name: "Open" }));
  expect(await screen.findByRole("alert")).toHaveTextContent("Conversation unavailable"); expect(callbacks.onSetVisibility).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "Restore" })); await waitFor(() => expect(screen.getByRole("alert")).toHaveTextContent("visibility changed"));
  expect(screen.queryByText(/SECRET/)).not.toBeInTheDocument(); expect(invoke).toHaveBeenCalledTimes(1);
});

it("visibility_safe_error shows fixed not-found/unknown errors and visible query length validation", async () => {
  vi.mocked(invoke).mockRejectedValueOnce({ code: "not_found", message: "SECRET backend details" }).mockRejectedValueOnce(new Error("SECRET raw failure"));
  render(<ShelvedConversationsDialog {...props()} />);
  expect(await screen.findByRole("alert")).toHaveTextContent("Conversation unavailable"); expect(screen.queryByText(/SECRET/)).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Refresh" })); await waitFor(() => expect(screen.getByRole("alert")).toHaveTextContent("Could not complete"));
  fireEvent.change(screen.getByRole("textbox", { name: "Search shelved conversations" }), { target: { value: "a".repeat(201) } });
  expect(await screen.findByRole("alert")).toHaveTextContent("200 characters"); expect(invoke).toHaveBeenCalledTimes(2);
});

it("shows empty state and safely rejects a foreign page", async () => {
  vi.mocked(invoke).mockResolvedValueOnce({ items: [], next_offset: null }).mockResolvedValueOnce(page("foreign", null, "other"));
  render(<ShelvedConversationsDialog {...props()} />); await screen.findByText("No shelved conversations.");
  fireEvent.click(screen.getByRole("button", { name: "Refresh" })); expect(await screen.findByRole("alert")).toHaveTextContent("identity mismatch"); expect(screen.queryByText("foreign")).not.toBeInTheDocument();
});

it("does not refresh a new query when an old restore finishes", async () => {
  vi.mocked(invoke).mockResolvedValueOnce(page("restore-old")).mockResolvedValueOnce(page("new-query"));
  const pending = deferred<void>(); const callbacks = { ...props(), onSetVisibility: vi.fn(() => pending.promise) }; render(<ShelvedConversationsDialog {...callbacks} />);
  await screen.findByText("restore-old"); fireEvent.click(screen.getByRole("button", { name: "Restore" }));
  fireEvent.change(screen.getByRole("textbox", { name: "Search shelved conversations" }), { target: { value: "new" } }); await screen.findByText("new-query");
  await act(async () => pending.resolve()); expect(invoke).toHaveBeenCalledTimes(2); expect(screen.getByText("new-query")).toBeVisible();
});

async function changeRecurrenceScope(mode: "query" | "page", away: boolean, expectedId: string) {
  if (mode === "query") fireEvent.change(screen.getByRole("textbox", { name: "Search shelved conversations" }), { target: { value: away ? "B" : "" } });
  else fireEvent.click(screen.getByRole("button", { name: away ? "Next page" : "Previous page" }));
  await screen.findByText(expectedId);
}

it.each([
  ["query", "success"], ["query", "error"], ["page", "success"], ["page", "error"],
] as const)("discards stale %s recurrence restore %s", async (mode, outcome) => {
  vi.mocked(invoke).mockResolvedValueOnce(page("first-A", 50)).mockResolvedValueOnce(page("B", 100)).mockResolvedValueOnce(page("returned-A", 50)).mockResolvedValue(page("unexpected-refresh"));
  const old = deferred<void>(); const callbacks = { ...props(), onSetVisibility: vi.fn(() => old.promise) };
  render(<ShelvedConversationsDialog {...callbacks} />); await screen.findByText("first-A");
  fireEvent.click(screen.getByRole("button", { name: "Restore" })); expect(callbacks.onSetVisibility).toHaveBeenCalledWith("first-A", "visible", 1);
  await changeRecurrenceScope(mode, true, "B"); await changeRecurrenceScope(mode, false, "returned-A");
  expect(invoke).toHaveBeenCalledTimes(3);
  await act(async () => { if (outcome === "success") old.resolve(); else old.reject({ code: "revision_conflict" }); });
  expect(invoke).toHaveBeenCalledTimes(3); expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  expect(screen.getByText("returned-A")).toBeVisible(); expect(screen.getByRole("button", { name: "Restore" })).toBeEnabled();
});

it.each(["query", "page"] as const)("allows a new restore after %s recurrence while the old restore is pending", async (mode) => {
  vi.mocked(invoke).mockResolvedValueOnce(page("first-A", 50)).mockResolvedValueOnce(page("B", 100)).mockResolvedValueOnce(page("returned-A", 50));
  const old = deferred<void>(); const newer = deferred<void>(); const callbacks = { ...props(), onSetVisibility: vi.fn().mockReturnValueOnce(old.promise).mockReturnValueOnce(newer.promise) };
  render(<ShelvedConversationsDialog {...callbacks} />); await screen.findByText("first-A"); fireEvent.click(screen.getByRole("button", { name: "Restore" }));
  await changeRecurrenceScope(mode, true, "B"); await changeRecurrenceScope(mode, false, "returned-A");
  fireEvent.click(screen.getByRole("button", { name: "Restore" }));
  expect(callbacks.onSetVisibility).toHaveBeenCalledTimes(2); expect(callbacks.onSetVisibility).toHaveBeenLastCalledWith("returned-A", "visible", 1);
  expect(screen.getByRole("button", { name: "Open" })).toBeDisabled();
});

it.each(["query", "page"] as const)("old %s recurrence completion cannot clear a newer pending restore", async (mode) => {
  vi.mocked(invoke).mockResolvedValueOnce(page("first-A", 50)).mockResolvedValueOnce(page("B", 100)).mockResolvedValueOnce(page("returned-A", 50)).mockResolvedValueOnce(page("confirmed-current"));
  const old = deferred<void>(); const middle = deferred<void>(); const newer = deferred<void>();
  const callbacks = { ...props(), onSetVisibility: vi.fn().mockReturnValueOnce(old.promise).mockReturnValueOnce(middle.promise).mockReturnValueOnce(newer.promise) };
  render(<ShelvedConversationsDialog {...callbacks} />); await screen.findByText("first-A"); fireEvent.click(screen.getByRole("button", { name: "Restore" }));
  await changeRecurrenceScope(mode, true, "B"); fireEvent.click(screen.getByRole("button", { name: "Restore" }));
  await changeRecurrenceScope(mode, false, "returned-A"); fireEvent.click(screen.getByRole("button", { name: "Restore" }));
  expect(callbacks.onSetVisibility).toHaveBeenCalledTimes(3); expect(screen.getByRole("button", { name: "Open" })).toBeDisabled();
  await act(async () => old.resolve());
  expect(invoke).toHaveBeenCalledTimes(3); expect(screen.getByRole("button", { name: "Open" })).toBeDisabled(); expect(screen.getByRole("button", { name: "Working…" })).toBeDisabled();
  await act(async () => middle.reject({ code: "not_found" }));
  expect(screen.queryByRole("alert")).not.toBeInTheDocument(); expect(screen.getByRole("button", { name: "Open" })).toBeDisabled();
  fireEvent.click(screen.getByRole("button", { name: "Working…" })); expect(callbacks.onSetVisibility).toHaveBeenCalledTimes(3);
  await act(async () => newer.resolve()); await screen.findByText("confirmed-current");
  expect(invoke).toHaveBeenCalledTimes(4); expect(screen.getByRole("button", { name: "Restore" })).toBeEnabled();
});
