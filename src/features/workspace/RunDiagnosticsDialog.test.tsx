import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { RunDiagnosticsDialog } from "./RunDiagnosticsDialog";
import { getRunDiagnostics, saveRunDiagnostics } from "../../run-diagnostics-api";
import type { RunDiagnosticsSnapshotV4 } from "../../types";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
const call = vi.mocked(invoke);
const row = { run_id: "run-a", sequence: 3, event_hash: "hash-a", previous_hash: "hash-before", occurred_at: "2026-10-08T00:00:00Z", event_kind: "model_text", status: null, tool_label: null, call_ref: null, duration_ms: null };
const snapshot: RunDiagnosticsSnapshotV4 = { schema_version: 1, project_id: "p", conversation_id: "c", filter_summary: { run_id: null, tool_label: null, failures_only: false }, source_heads: [{ run_id: "run-a", event_count: 3, last_sequence: 3, event_hash: "hash-a" }], source_chain_validated: true, total_event_count: 3, selected_event_count: 1, filtered: false, events: [row], usage: [] };
const query = { project_id: "p", conversation_id: "c", filter: { run_id: null, tool_id: null, failures_only: false }, redaction_keywords: [] };
function deferred<T>() { let resolve!: (value: T) => void; const promise = new Promise<T>((r) => { resolve = r; }); return { promise, resolve }; }
function show() { const onClose = vi.fn(); const onLocate = vi.fn(); const view = render(<RunDiagnosticsDialog projectId="p" conversationId="c" locale="en-US" onLocate={onLocate} onClose={onClose} />); return { ...view, onClose, onLocate }; }
async function preview() { fireEvent.click(screen.getByRole("button", { name: "Refresh diagnostics" })); await screen.findByText("model_text"); }

beforeEach(() => { Object.defineProperty(window, "__TAURI_INTERNALS__", { value: {}, configurable: true }); call.mockReset(); call.mockResolvedValue(snapshot); });
afterEach(() => { Reflect.deleteProperty(window, "__TAURI_INTERNALS__"); });

describe("run diagnostics", () => {
  it("uses host request envelopes and honestly rejects browser-only get/save", async () => {
    await getRunDiagnostics(query);
    expect(call).toHaveBeenLastCalledWith("get_run_diagnostics_v4", { request: query });
    call.mockResolvedValue(null); const request = { query, expected_heads: snapshot.source_heads, format: "json" as const };
    expect(await saveRunDiagnostics(request)).toBeNull();
    expect(call).toHaveBeenLastCalledWith("save_run_diagnostics_v4", { request });
    Reflect.deleteProperty(window, "__TAURI_INTERNALS__"); call.mockClear();
    await expect(getRunDiagnostics(query)).rejects.toBe("diagnostics_unavailable");
    await expect(saveRunDiagnostics(request)).rejects.toBe("diagnostics_unavailable"); expect(call).not.toHaveBeenCalled();
    show(); expect(screen.getByText(/desktop host is required/i)).toBeVisible(); expect(screen.getByRole("button", { name: "Refresh diagnostics" })).toBeDisabled();
  });
  it.each(["Run ID", "Tool ID", "Redaction keywords", "Failures only"])("invalidates preview and pending replies when %s changes", async (label) => {
    show(); await preview();
    const pending = deferred<RunDiagnosticsSnapshotV4>(); call.mockReturnValueOnce(pending.promise);
    fireEvent.click(screen.getByRole("button", { name: "Refresh diagnostics" }));
    if (label === "Failures only") fireEvent.click(screen.getByLabelText(label)); else fireEvent.change(screen.getByLabelText(label), { target: { value: "new" } });
    await act(async () => pending.resolve(snapshot));
    expect(screen.queryByText("model_text")).not.toBeInTheDocument(); expect(screen.getByRole("button", { name: "Save JSON" })).toBeDisabled();
  });
  it.each(["project", "conversation"])("drops late %s responses", async (scope) => {
    const pending = deferred<RunDiagnosticsSnapshotV4>(); call.mockReturnValueOnce(pending.promise); const view = show();
    fireEvent.click(screen.getByRole("button", { name: "Refresh diagnostics" }));
    view.rerender(<RunDiagnosticsDialog projectId={scope === "project" ? "other" : "p"} conversationId={scope === "conversation" ? "other" : "c"} locale="en-US" onLocate={view.onLocate} onClose={view.onClose} />);
    await act(async () => pending.resolve(snapshot)); expect(screen.queryByText("model_text")).not.toBeInTheDocument(); expect(screen.getByRole("button", { name: "Save JSON" })).toBeDisabled();
  });
  it("passes the exact metadata-only ModelText locator and closes on immediate Escape", async () => {
    const view = show(); fireEvent.keyDown(window, { key: "Escape" }); expect(view.onClose).toHaveBeenCalledOnce();
    await preview(); fireEvent.click(screen.getByRole("button", { name: "Locate event 3" }));
    expect(view.onLocate).toHaveBeenCalledWith({ run_id: "run-a", sequence: 3, event_hash: "hash-a" });
    expect(screen.getByText(/public diagnostic projection/i)).toBeVisible();
  });
  it("deduplicates pending saves, and cancellation and failure never report success", async () => {
    show(); await preview(); const pending = deferred<string | null>(); call.mockReturnValueOnce(pending.promise);
    fireEvent.click(screen.getByRole("button", { name: "Save JSON" })); fireEvent.click(screen.getByRole("button", { name: "Save JSON" }));
    expect(call.mock.calls.filter(([command]) => command === "save_run_diagnostics_v4")).toHaveLength(1);
    expect(screen.getByRole("button", { name: "Save HTML" })).toBeDisabled();
    await act(async () => pending.resolve(null)); expect(screen.queryByText(/saved successfully/i)).not.toBeInTheDocument();
    call.mockRejectedValueOnce("PRIVATE_NATIVE_ERROR"); fireEvent.click(screen.getByRole("button", { name: "Save HTML" }));
    await screen.findByRole("alert"); expect(screen.queryByText(/saved successfully|PRIVATE_NATIVE_ERROR/i)).not.toBeInTheDocument();
    call.mockResolvedValueOnce("C:/diagnostics.json"); fireEvent.click(screen.getByRole("button", { name: "Save JSON" }));
    await screen.findByText(/saved successfully/i);
    expect(call).toHaveBeenLastCalledWith("save_run_diagnostics_v4", { request: { query, expected_heads: snapshot.source_heads, format: "json" } });
  });
  it("invalidates stale heads on save rejection and suppresses late save results after edits", async () => {
    show(); await preview(); call.mockRejectedValueOnce("diagnostics_snapshot_changed"); fireEvent.click(screen.getByRole("button", { name: "Save JSON" }));
    await screen.findByRole("alert"); expect(screen.getByRole("button", { name: "Save JSON" })).toBeDisabled();
    call.mockResolvedValueOnce(snapshot); await preview(); const pending = deferred<string | null>(); call.mockReturnValueOnce(pending.promise);
    fireEvent.click(screen.getByRole("button", { name: "Save HTML" })); fireEvent.change(screen.getByLabelText("Tool ID"), { target: { value: "shell" } });
    await act(async () => pending.resolve("C:/old.html")); expect(screen.queryByText(/saved successfully/i)).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Save HTML" })).toBeDisabled();
  });
  it("keeps keywords only in the current query and refuses a mismatched host scope", async () => {
    show(); fireEvent.change(screen.getByLabelText("Redaction keywords"), { target: { value: "alpha\nbeta" } });
    call.mockResolvedValueOnce({ ...snapshot, project_id: "other" }); fireEvent.click(screen.getByRole("button", { name: "Refresh diagnostics" }));
    await waitFor(() => expect(screen.getByRole("alert")).toBeVisible()); expect(screen.queryByText("model_text")).not.toBeInTheDocument();
    expect(call).toHaveBeenCalledWith("get_run_diagnostics_v4", { request: { ...query, redaction_keywords: ["alpha", "beta"] } });
  });
});

it.each([undefined, ""])("does not claim success for an invalid native receipt %s", async (receipt) => {
  show(); await preview(); call.mockResolvedValueOnce(receipt); fireEvent.click(screen.getByRole("button", { name: "Save JSON" }));
  await waitFor(() => expect(screen.getByRole("button", { name: "Save JSON" })).toBeEnabled());
  expect(screen.queryByText(/saved successfully/i)).not.toBeInTheDocument();
});

it("keeps source cutoff disclosure in the window keyboard focus cycle", async () => {
  show(); await preview(); screen.getByText("Source cutoff").focus();
  fireEvent.keyDown(window, { key: "Tab", shiftKey: true }); expect(screen.getByRole("button", { name: "Save JSON" })).toHaveFocus();
});

it.each(["project", "conversation"])("suppresses a saved receipt from the prior %s scope", async (scope) => {
  const view = show(); await preview(); const pending = deferred<string | null>(); call.mockReturnValueOnce(pending.promise);
  fireEvent.click(screen.getByRole("button", { name: "Save HTML" }));
  view.rerender(<RunDiagnosticsDialog projectId={scope === "project" ? "other" : "p"} conversationId={scope === "conversation" ? "other" : "c"} locale="en-US" onLocate={view.onLocate} onClose={view.onClose} />);
  await act(async () => pending.resolve("C:/old.html")); expect(screen.queryByText(/saved successfully/i)).not.toBeInTheDocument(); expect(screen.getByRole("button", { name: "Save HTML" })).toBeDisabled();
});
