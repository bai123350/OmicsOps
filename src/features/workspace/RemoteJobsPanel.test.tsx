import { useState } from "react";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import * as api from "../../remote-job-api";
import type { RemoteJobSummaryV4 } from "../../types";
import { useWindowEscapeLayer } from "../settings/BrowserSettings";
import { RemoteJobsPanel } from "./RemoteJobsPanel";
vi.mock("../../remote-job-api", () => ({ listRemoteJobsV4: vi.fn(), refreshRemoteJobV4: vi.fn() }));
const job: RemoteJobSummaryV4 = { project_id: "p", conversation_id: "c", job_id: "j", run_id: "r", call_id: "call", backend_id: "ssh", language: "python", environment: "system", request_sha256: "hash", recorded_state: "running", query_eligibility: "needs_root_check", recorded_at: null };
const props = { projectId: "p", conversationId: "c", bindingVersion: "v1", locale: "en-US" as const, onClose: vi.fn() };
beforeEach(() => { vi.resetAllMocks(); vi.mocked(api.listRemoteJobsV4).mockResolvedValue({ jobs: [job], limit: 100, remote_liveness_checked: false }); vi.mocked(api.refreshRemoteJobV4).mockResolvedValue({ job, status: "unknown", observed_at: "2026-10-07T00:00:00Z", stdout_excerpt: "bounded output", stderr_excerpt: "", captures: [], resubmitted: false }); });
afterEach(cleanup);
it("shows recorded state separately from observation with truthful metadata and no execution controls", async () => {
  render(<RemoteJobsPanel {...props} />); fireEvent.click(await screen.findByRole("button", { name: /j · python/ }));
  expect(screen.getByText(/Recorded state: Running/)).toBeInTheDocument(); expect(screen.getByText(/Recorded time: Unknown/)).toBeInTheDocument(); expect(screen.getByText(/Remote liveness has not been checked/)).toBeInTheDocument();
  expect(api.refreshRemoteJobV4).not.toHaveBeenCalled(); fireEvent.click(screen.getByRole("button", { name: "Refresh status" }));
  expect(await screen.findByText(/Observation: Unknown/)).toBeInTheDocument(); expect(screen.getByText(/Recorded state: Running/)).toBeInTheDocument();
  expect(screen.getByText(/4 KiB/)).toBeInTheDocument(); expect(screen.queryByRole("button", { name: /cancel|resubmit|resume agent/i })).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "All project jobs" })); await waitFor(() => expect(api.listRemoteJobsV4).toHaveBeenLastCalledWith({ project_id: "p", conversation_id: null }));
});
it("explains eligibility and permits only local verification of a retained receipt", async () => {
  vi.mocked(api.listRemoteJobsV4).mockResolvedValue({ jobs: [{ ...job, recorded_state: "succeeded", query_eligibility: "recorded_receipt" }, { ...job, job_id: "old", query_eligibility: "binding_changed" }], limit: 100, remote_liveness_checked: false });
  render(<RemoteJobsPanel {...props} />); fireEvent.click(await screen.findByRole("button", { name: /j · python/ }));
  expect(screen.getByText(/Verify retained receipt locally/)).toBeInTheDocument(); expect(screen.getByRole("button", { name: "Start automatic queries" })).toBeDisabled(); expect(screen.getByRole("button", { name: "Verify recorded receipt" })).toBeEnabled();
  fireEvent.click(screen.getByRole("button", { name: /old · python/ })); expect(screen.getByRole("button", { name: "Refresh status" })).toBeDisabled(); expect(screen.getByText(/binding has changed/i)).toBeInTheDocument();
});
it("closes only the top layer immediately, traps Tab, and restores the connected opener even pending", async () => {
  let resolve!: (v: Awaited<ReturnType<typeof api.refreshRemoteJobV4>>) => void; vi.mocked(api.refreshRemoteJobV4).mockReturnValue(new Promise((yes) => { resolve = yes; }));
  const parentClose = vi.fn(); function Parent() { const [open, setOpen] = useState(false); useWindowEscapeLayer(true, parentClose); return <><button onClick={() => setOpen(true)}>Open jobs</button>{open && <RemoteJobsPanel {...props} onClose={() => setOpen(false)} />}</>; }
  render(<Parent />); const opener = screen.getByRole("button", { name: "Open jobs" }); opener.focus(); fireEvent.click(opener);
  const close = screen.getByRole("button", { name: "Close remote jobs" }); expect(close).toHaveFocus(); fireEvent.keyDown(close, { key: "Tab", shiftKey: true }); expect(screen.getByRole("button", { name: "All project jobs" })).toHaveFocus();
  fireEvent.click(await screen.findByRole("button", { name: /j · python/ })); fireEvent.click(screen.getByRole("button", { name: "Refresh status" })); fireEvent.keyDown(window, { key: "Escape" });
  expect(screen.queryByRole("dialog")).not.toBeInTheDocument(); expect(parentClose).not.toHaveBeenCalled(); expect(opener).toHaveFocus();
  await act(async () => resolve({ job, status: "completed", observed_at: "now", stdout_excerpt: "late", stderr_excerpt: "", captures: [], resubmitted: false })); expect(opener).toHaveFocus();
  fireEvent.click(opener); fireEvent.keyDown(window, { key: "Escape" }); expect(parentClose).not.toHaveBeenCalled(); fireEvent.keyDown(window, { key: "Escape" }); expect(parentClose).toHaveBeenCalledTimes(1);
});
it("does not overlap a pending native query after closing and reopening or revive old output/focus", async () => {
  let resolve!: (v: Awaited<ReturnType<typeof api.refreshRemoteJobV4>>) => void; vi.mocked(api.refreshRemoteJobV4).mockReturnValueOnce(new Promise((yes) => { resolve = yes; }));
  function Parent() { const [open, setOpen] = useState(false); return <><button onClick={() => setOpen(true)}>Open jobs</button>{open && <RemoteJobsPanel {...props} onClose={() => setOpen(false)} />}</>; }
  render(<Parent />); const opener = screen.getByRole("button", { name: "Open jobs" }); opener.focus(); fireEvent.click(opener); fireEvent.click(await screen.findByRole("button", { name: /j · python/ })); fireEvent.click(screen.getByRole("button", { name: "Refresh status" }));
  fireEvent.keyDown(window, { key: "Escape" }); expect(opener).toHaveFocus(); fireEvent.click(opener); fireEvent.click(await screen.findByRole("button", { name: /j · python/ }));
  expect(screen.getByRole("button", { name: "Refresh status" })).toBeDisabled(); fireEvent.click(screen.getByRole("button", { name: "Refresh status" })); expect(api.refreshRemoteJobV4).toHaveBeenCalledTimes(1);
  const close = screen.getByRole("button", { name: "Close remote jobs" }); close.focus(); await act(async () => resolve({ job, status: "completed", observed_at: "now", stdout_excerpt: "late prior output", stderr_excerpt: "", captures: [], resubmitted: false }));
  expect(close).toHaveFocus(); expect(screen.queryByText("late prior output")).not.toBeInTheDocument(); expect(screen.getByRole("button", { name: "Refresh status" })).toBeEnabled(); expect(api.refreshRemoteJobV4).toHaveBeenCalledTimes(1);
});
it("moves keyboard Retry focus to retained Close before loading removes Retry, then restores opener", async () => {
  vi.mocked(api.listRemoteJobsV4).mockRejectedValueOnce("secret raw path"); let resolve!: (v: Awaited<ReturnType<typeof api.listRemoteJobsV4>>) => void; vi.mocked(api.listRemoteJobsV4).mockReturnValueOnce(new Promise((yes) => { resolve = yes; }));
  function Parent() { const [open, setOpen] = useState(false); return <><button onClick={() => setOpen(true)}>Open jobs</button>{open && <RemoteJobsPanel {...props} onClose={() => setOpen(false)} />}</>; }
  render(<Parent />); const opener = screen.getByRole("button", { name: "Open jobs" }); opener.focus(); fireEvent.click(opener);
  const retry = await screen.findByRole("button", { name: "Retry list" }); expect(screen.queryByText("secret raw path")).not.toBeInTheDocument(); retry.focus(); fireEvent.click(retry);
  expect(screen.queryByRole("button", { name: "Retry list" })).not.toBeInTheDocument(); expect(screen.getByRole("button", { name: "Close remote jobs" })).toHaveFocus();
  await act(async () => resolve({ jobs: [job], limit: 100, remote_liveness_checked: false })); expect(screen.getByRole("button", { name: "Close remote jobs" })).toHaveFocus(); fireEvent.keyDown(window, { key: "Escape" }); expect(opener).toHaveFocus();
});
it("keeps focus in the panel when automatic querying disables a keyboard-focused refresh button", async () => {
  vi.useFakeTimers();
  let resolve!: (v: Awaited<ReturnType<typeof api.refreshRemoteJobV4>>) => void; vi.mocked(api.refreshRemoteJobV4).mockReturnValueOnce(new Promise((yes) => { resolve = yes; }));
  try {
    render(<RemoteJobsPanel {...props} />); await act(async () => { await Promise.resolve(); }); fireEvent.click(screen.getByRole("button", { name: /j · python/ })); fireEvent.click(screen.getByRole("button", { name: "Start automatic queries" }));
    screen.getByRole("button", { name: "Refresh status" }).focus(); await act(async () => { await vi.advanceTimersByTimeAsync(2000); }); expect(screen.getByRole("button", { name: "Close remote jobs" })).toHaveFocus();
    await act(async () => resolve({ job, status: "completed", observed_at: "now", stdout_excerpt: "", stderr_excerpt: "", captures: [], resubmitted: false })); expect(screen.getByRole("button", { name: "Close remote jobs" })).toHaveFocus();
  } finally { cleanup(); vi.useRealTimers(); }
});
it.each(["project", "conversation", "binding"])("invalidates a pending observation on owner %s navigation even in the all-project filter", async (change) => {
  let resolve!: (v: Awaited<ReturnType<typeof api.refreshRemoteJobV4>>) => void; vi.mocked(api.refreshRemoteJobV4).mockReturnValueOnce(new Promise((yes) => { resolve = yes; }));
  vi.mocked(api.listRemoteJobsV4).mockImplementation(async (request) => ({ jobs: [{ ...job, project_id: request.project_id, conversation_id: request.conversation_id ?? "c" }], limit: 100, remote_liveness_checked: false }));
  const nextProps = { ...props, ...(change === "project" ? { projectId: "new-project" } : change === "conversation" ? { conversationId: "new-conversation" } : { bindingVersion: "new-binding" }) };
  try {
  const panel = render(<RemoteJobsPanel {...props} />); await screen.findByRole("button", { name: /j · python/ }); fireEvent.click(screen.getByRole("button", { name: "All project jobs" })); await act(async () => { await Promise.resolve(); }); fireEvent.click(screen.getByRole("button", { name: /j · python/ })); fireEvent.click(screen.getByRole("button", { name: "Refresh status" }));
  panel.rerender(<RemoteJobsPanel {...nextProps} />); await act(async () => { await Promise.resolve(); }); expect(screen.queryByText(/Recorded state:/)).not.toBeInTheDocument(); expect(screen.getByRole("button", { name: "All project jobs" })).toHaveAttribute("aria-pressed", "true");
  fireEvent.click(screen.getByRole("button", { name: /j · python/ })); expect(screen.getByRole("button", { name: "Refresh status" })).toBeDisabled();
  await act(async () => resolve({ job, status: "completed", observed_at: "now", stdout_excerpt: "stale owner response", stderr_excerpt: "", captures: [], resubmitted: false })); expect(screen.queryByText("stale owner response")).not.toBeInTheDocument(); expect(screen.getByRole("button", { name: "Refresh status" })).toBeEnabled();
  expect(api.refreshRemoteJobV4).toHaveBeenCalledTimes(1); expect(api.listRemoteJobsV4).toHaveBeenLastCalledWith({ project_id: nextProps.projectId, conversation_id: null });
  } finally { await act(async () => resolve({ job, status: "completed", observed_at: "now", stdout_excerpt: "fixture settled", stderr_excerpt: "", captures: [], resubmitted: false })); }
});
it.each(["project", "conversation", "binding"])("stops a queued automatic timer on owner %s navigation in the all-project filter", async (change) => {
  vi.useFakeTimers();
  vi.mocked(api.listRemoteJobsV4).mockImplementation(async (request) => ({ jobs: [{ ...job, project_id: request.project_id, conversation_id: request.conversation_id ?? "c" }], limit: 100, remote_liveness_checked: false }));
  const nextProps = { ...props, ...(change === "project" ? { projectId: "new-project" } : change === "conversation" ? { conversationId: "new-conversation" } : { bindingVersion: "new-binding" }) };
  try {
    const panel = render(<RemoteJobsPanel {...props} />); await act(async () => { await Promise.resolve(); }); fireEvent.click(screen.getByRole("button", { name: "All project jobs" })); await act(async () => { await Promise.resolve(); }); fireEvent.click(screen.getByRole("button", { name: /j · python/ })); fireEvent.click(screen.getByRole("button", { name: "Start automatic queries" }));
    await act(async () => { await vi.advanceTimersByTimeAsync(1000); }); panel.rerender(<RemoteJobsPanel {...nextProps} />); await act(async () => { await Promise.resolve(); await vi.advanceTimersByTimeAsync(900000); });
    expect(api.refreshRemoteJobV4).not.toHaveBeenCalled(); expect(screen.queryByText(/Recorded state:/)).not.toBeInTheDocument(); expect(vi.getTimerCount()).toBe(0);
  } finally { cleanup(); vi.useRealTimers(); }
});
