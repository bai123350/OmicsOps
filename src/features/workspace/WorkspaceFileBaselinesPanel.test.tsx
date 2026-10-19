import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, expect, it, vi } from "vitest";
import * as api from "../../workspace-navigation-api";
import type { FileBaselineV1, FileBaselineDiffV1, BaselineTextSnapshotV1 } from "../../workspace-navigation-types";
import { WorkspaceFileBaselinesPanel } from "./WorkspaceFileBaselinesPanel";

vi.mock("../../workspace-navigation-api", () => ({ createWorkspaceFileBaseline: vi.fn(), listWorkspaceFileBaselines: vi.fn(), getWorkspaceFileBaseline: vi.fn(), readWorkspaceBaselineText: vi.fn(), diffWorkspaceFileBaseline: vi.fn() }));
const hash = "a".repeat(64);
const saved: FileBaselineV1 = { schema_version: 1, id: "b", project_id: "p", branch_conversation_id: null, title: "Before", manifest_sha256: hash, captured_text_bytes: 12, created_at: "2026-10-13", entries: [
  { backend_id: "local", relative_path: "my script.py", content_mode: "text_snapshot", size_bytes: 12, hash: { scope: "full_file", sha256: hash }, omission: null, captured_at: "2026-10-13" },
  { backend_id: "ssh:c", relative_path: "data.csv", content_mode: "reference_only", size_bytes: null, hash: { scope: "recorded_file", sha256: hash }, omission: "remote_reference", captured_at: "2026-10-13" },
  { backend_id: "local", relative_path: null, content_mode: "reference_only", size_bytes: null, hash: null, omission: "sensitive_content", captured_at: "2026-10-13" },
] };
const props = { projectId: "p", branchConversationId: null, locale: "en-US" as const, selectedFiles: [{ backend_id: "local", relative_path: "my script.py", expected_full_sha256: null }], onClose: vi.fn() };
function deferred<T>() { let resolve!: (value: T) => void; const promise = new Promise<T>((r) => { resolve = r; }); return { promise, resolve }; }
const text: BaselineTextSnapshotV1 = { project_id: "p", baseline_id: "b", entry_index: 0, relative_path: "my script.py", full_sha256: hash, text: '<script>window.BAD = true</script>\nprint("saved")' };
beforeEach(() => { vi.resetAllMocks(); vi.mocked(api.listWorkspaceFileBaselines).mockResolvedValue({ items: [saved], next_offset: null }); vi.mocked(api.getWorkspaceFileBaseline).mockResolvedValue(saved); vi.mocked(api.createWorkspaceFileBaseline).mockResolvedValue(saved); vi.mocked(api.readWorkspaceBaselineText).mockResolvedValue(text); });
async function openSaved() { fireEvent.click(await screen.findByRole("button", { name: "Open baseline Before" })); await screen.findByRole("button", { name: "View saved text my script.py" }); }

it("captures selected files only and retries the immutable original request after an uncertain receipt", async () => {
  vi.mocked(api.createWorkspaceFileBaseline).mockRejectedValueOnce({ code: "storage_failed", message: "arbitrary secret" });
  const view = render(<WorkspaceFileBaselinesPanel {...props} branchConversationId="verified-branch" />);
  fireEvent.click(screen.getByRole("checkbox", { name: "Associate verified branch" }));
  fireEvent.change(screen.getByRole("textbox", { name: "Baseline title" }), { target: { value: "One file" } });
  fireEvent.click(screen.getByRole("button", { name: "Save baseline" }));
  await screen.findByRole("button", { name: "Retry same request" });
  const original = vi.mocked(api.createWorkspaceFileBaseline).mock.calls[0][0];
  expect(original).toMatchObject({ project_id: "p", branch_conversation_id: "verified-branch", title: "One file", files: props.selectedFiles });
  expect(original.request_id).toMatch(/^[0-9a-f-]{36}$/i); expect(original.files).toHaveLength(1);
  view.rerender(<WorkspaceFileBaselinesPanel {...props} selectedFiles={[]} branchConversationId="verified-branch" />);
  fireEvent.click(screen.getByRole("button", { name: "Retry same request" }));
  await waitFor(() => expect(api.createWorkspaceFileBaseline).toHaveBeenCalledTimes(2));
  expect(vi.mocked(api.createWorkspaceFileBaseline).mock.calls[1][0]).toEqual(original);
  expect(screen.queryByText(/arbitrary secret/)).not.toBeInTheDocument();
});

it("labels saved/reference/unknown metadata and reads inert saved text independently of current files", async () => {
  render(<WorkspaceFileBaselinesPanel {...props} selectedFiles={[]} />); await openSaved();
  expect(screen.getAllByText("Reference only")).toHaveLength(2); expect(screen.getByText(/Recorded file SHA-256/)).toBeInTheDocument();
  expect(screen.getAllByText(/Size: Unknown/)).toHaveLength(2); expect(screen.getByText(/Path withheld/)).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "View saved text my script.py" }));
  const child = await screen.findByRole("dialog", { name: "Saved baseline text" });
  await waitFor(() => expect(child.querySelector("pre")?.textContent).toBe(text.text)); expect(child.querySelector("script")).toBeNull();
  expect(api.readWorkspaceBaselineText).toHaveBeenCalledWith({ project_id: "p", baseline_id: "b", entry_index: 0, expected_manifest_sha256: hash });
  fireEvent.keyDown(window, { key: "Escape" });
  expect(screen.queryByRole("dialog", { name: "Saved baseline text" })).not.toBeInTheDocument();
  expect(screen.getByRole("dialog", { name: "File baselines" })).toBeInTheDocument(); expect(props.onClose).not.toHaveBeenCalled();
  fireEvent.keyDown(window, { key: "Escape" }); expect(props.onClose).toHaveBeenCalledTimes(1);
  for (const name of [/Restore files/i, /Execute/i, /Sync/i]) expect(screen.queryByRole("button", { name })).not.toBeInTheDocument();
});

it("shows backend diff states, unknown newline flags and bounded truncation without inventing Missing", async () => {
  const diff: FileBaselineDiffV1 = { project_id: "p", baseline_id: "b", manifest_sha256: hash, observed_at: "now", entries: [
    { baseline: saved.entries[0], state: "binding_changed", current_size_bytes: null, current_hash: null, line_endings_changed: null, final_newline_changed: null, hunks: [], hunks_truncated: false },
    { baseline: saved.entries[1], state: "unreadable", current_size_bytes: null, current_hash: null, line_endings_changed: null, final_newline_changed: null, hunks: [], hunks_truncated: false },
    { baseline: saved.entries[0], state: "changed", current_size_bytes: 13, current_hash: { scope: "full_file", sha256: hash }, line_endings_changed: true, final_newline_changed: false, hunks: [{ old_start: 1, old_lines: 1, new_start: 1, new_lines: 1, lines: [{ kind: "removed", text: "old" }, { kind: "added", text: "<img src=x onerror=BAD>" }] }], hunks_truncated: true },
  ] };
  vi.mocked(api.diffWorkspaceFileBaseline).mockResolvedValue(diff);
  render(<WorkspaceFileBaselinesPanel {...props} />); await openSaved(); fireEvent.click(screen.getByRole("button", { name: "Compare current files" }));
  const child = await screen.findByRole("dialog", { name: "Baseline diff" }); await within(child).findByText(/Root binding changed/);
  expect(child).toHaveTextContent("Unreadable"); expect(child).toHaveTextContent(/comparison is incomplete/i); expect(child).toHaveTextContent(/Final newline: Unknown/);
  expect(child).not.toHaveTextContent("Missing"); expect(child.querySelector("img")).toBeNull();
  expect(api.diffWorkspaceFileBaseline).toHaveBeenCalledWith({ project_id: "p", baseline_id: "b", expected_manifest_sha256: hash });
  fireEvent.keyDown(document.body, { key: "Escape" }); expect(screen.getByRole("dialog", { name: "File baselines" })).toBeInTheDocument(); expect(props.onClose).not.toHaveBeenCalled();
});

it("drops old project list/get/create responses and keeps the new selection", async () => {
  const list = deferred<{ items: FileBaselineV1[]; next_offset: null }>(); const get = deferred<FileBaselineV1>(); const create = deferred<FileBaselineV1>();
  vi.mocked(api.listWorkspaceFileBaselines).mockReturnValueOnce(list.promise).mockResolvedValue({ items: [], next_offset: null });
  const view = render(<WorkspaceFileBaselinesPanel {...props} />);
  view.rerender(<WorkspaceFileBaselinesPanel {...props} projectId="q" selectedFiles={[{ ...props.selectedFiles[0], relative_path: "new.txt" }]} />);
  await act(async () => list.resolve({ items: [saved], next_offset: null })); expect(screen.queryByText("Before")).not.toBeInTheDocument();
  view.rerender(<WorkspaceFileBaselinesPanel {...props} />); vi.mocked(api.listWorkspaceFileBaselines).mockResolvedValue({ items: [saved], next_offset: null });
  fireEvent.click(screen.getByRole("button", { name: "Refresh baselines" })); await screen.findByRole("button", { name: "Open baseline Before" });
  vi.mocked(api.getWorkspaceFileBaseline).mockReturnValueOnce(get.promise); fireEvent.click(screen.getByRole("button", { name: "Open baseline Before" }));
  vi.mocked(api.createWorkspaceFileBaseline).mockReturnValueOnce(create.promise); fireEvent.click(screen.getByRole("button", { name: "Save baseline" }));
  view.rerender(<WorkspaceFileBaselinesPanel {...props} projectId="q" selectedFiles={[{ ...props.selectedFiles[0], relative_path: "new.txt" }]} />);
  await act(async () => { get.resolve(saved); create.resolve(saved); }); expect(screen.queryByText("Before")).not.toBeInTheDocument(); expect(screen.getByText("new.txt")).toBeInTheDocument();
});

it("drops superseded list generations and rejects wrong-owner get receipts", async () => {
  const old = deferred<{ items: FileBaselineV1[]; next_offset: null }>(); vi.mocked(api.listWorkspaceFileBaselines).mockReturnValueOnce(old.promise);
  render(<WorkspaceFileBaselinesPanel {...props} />); fireEvent.click(screen.getByRole("button", { name: "Refresh baselines" }));
  await screen.findByRole("button", { name: "Open baseline Before" }); await act(async () => old.resolve({ items: [{ ...saved, title: "Old list" }], next_offset: null })); expect(screen.queryByText("Old list")).not.toBeInTheDocument();
  vi.mocked(api.getWorkspaceFileBaseline).mockResolvedValueOnce({ ...saved, project_id: "wrong" }); fireEvent.click(screen.getByRole("button", { name: "Open baseline Before" })); await screen.findByRole("alert"); expect(screen.queryByRole("button", { name: "View saved text my script.py" })).not.toBeInTheDocument();
});

it("closes pending text immediately and discards deferred text and mismatched diff responses", async () => {
  const pending = deferred<BaselineTextSnapshotV1>(); vi.mocked(api.readWorkspaceBaselineText).mockReturnValueOnce(pending.promise);
  render(<WorkspaceFileBaselinesPanel {...props} />); await openSaved(); fireEvent.click(screen.getByRole("button", { name: "View saved text my script.py" }));
  expect(screen.getByRole("dialog", { name: "Saved baseline text" })).toBeInTheDocument(); fireEvent.keyDown(window, { key: "Escape" }); await act(async () => pending.resolve(text)); expect(screen.queryByText(/window.BAD/)).not.toBeInTheDocument();
  vi.mocked(api.diffWorkspaceFileBaseline).mockResolvedValueOnce({ project_id: "p", baseline_id: "b", manifest_sha256: "wrong", observed_at: "now", entries: [] });
  fireEvent.click(screen.getByRole("button", { name: "Compare current files" })); await screen.findByRole("alert"); expect(screen.getByRole("dialog", { name: "Baseline diff" })).not.toHaveTextContent("now");
});

it("does not replace a later baseline/text selection when an earlier save receipt arrives", async () => {
  const pending = deferred<FileBaselineV1>(); vi.mocked(api.createWorkspaceFileBaseline).mockReturnValueOnce(pending.promise);
  render(<WorkspaceFileBaselinesPanel {...props} />); fireEvent.click(screen.getByRole("button", { name: "Save baseline" }));
  await openSaved(); fireEvent.click(screen.getByRole("button", { name: "View saved text my script.py" }));
  await waitFor(() => expect(screen.getByRole("dialog", { name: "Saved baseline text" }).querySelector("pre")?.textContent).toBe(text.text));
  await act(async () => pending.resolve({ ...saved, id: "created", title: "Late save" }));
  expect(screen.getByRole("dialog", { name: "Saved baseline text" })).toBeInTheDocument();
  expect(screen.getByRole("region", { name: "Saved baseline details" })).toHaveTextContent("Before");
});

it.each(["source_changed", "limit_exceeded"])("allows an explicit new request after native %s rejection without automatic dispatch", async (code) => {
  vi.mocked(api.createWorkspaceFileBaseline).mockRejectedValueOnce({ code, message: "native rejection" });
  const view = render(<WorkspaceFileBaselinesPanel {...props} />); fireEvent.click(screen.getByRole("button", { name: "Save baseline" }));
  const release = await screen.findByRole("button", { name: "Edit selection / New request" });
  const original = vi.mocked(api.createWorkspaceFileBaseline).mock.calls[0][0];
  expect(screen.getByRole("button", { name: "Retry same request" })).toBeEnabled();
  const files = [{ ...props.selectedFiles[0], relative_path: "changed.txt" }];
  view.rerender(<WorkspaceFileBaselinesPanel {...props} selectedFiles={files} />); fireEvent.click(release);
  expect(api.createWorkspaceFileBaseline).toHaveBeenCalledTimes(1);
  fireEvent.change(screen.getByRole("textbox", { name: "Baseline title" }), { target: { value: "Changed selection" } }); fireEvent.click(screen.getByRole("button", { name: "Save baseline" }));
  await waitFor(() => expect(api.createWorkspaceFileBaseline).toHaveBeenCalledTimes(2));
  const next = vi.mocked(api.createWorkspaceFileBaseline).mock.calls[1][0]; expect(next.request_id).not.toBe(original.request_id); expect(next.files).toEqual(files); expect(next.title).toBe("Changed selection");
});

it.each(["unknown", "invalid_selection"])("retains the immutable original request after ambiguous %s rejection", async (code) => {
  vi.mocked(api.createWorkspaceFileBaseline).mockRejectedValueOnce({ code, message: "uncertain" });
  const view = render(<WorkspaceFileBaselinesPanel {...props} />); fireEvent.click(screen.getByRole("button", { name: "Save baseline" }));
  await screen.findByRole("button", { name: "Retry same request" }); const original = vi.mocked(api.createWorkspaceFileBaseline).mock.calls[0][0];
  expect(screen.queryByRole("button", { name: "Edit selection / New request" })).not.toBeInTheDocument();
  view.rerender(<WorkspaceFileBaselinesPanel {...props} selectedFiles={[]} />); fireEvent.click(screen.getByRole("button", { name: "Retry same request" }));
  await waitFor(() => expect(api.createWorkspaceFileBaseline).toHaveBeenCalledTimes(2)); expect(vi.mocked(api.createWorkspaceFileBaseline).mock.calls[1][0]).toEqual(original);
});

it("retains the original request after a locally detected wrong-owner create receipt", async () => {
  vi.mocked(api.createWorkspaceFileBaseline).mockResolvedValueOnce({ ...saved, project_id: "wrong" });
  render(<WorkspaceFileBaselinesPanel {...props} />); fireEvent.click(screen.getByRole("button", { name: "Save baseline" }));
  await screen.findByRole("button", { name: "Retry same request" }); const original = vi.mocked(api.createWorkspaceFileBaseline).mock.calls[0][0];
  expect(screen.queryByRole("button", { name: "Edit selection / New request" })).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Retry same request" })); await waitFor(() => expect(api.createWorkspaceFileBaseline).toHaveBeenCalledTimes(2)); expect(vi.mocked(api.createWorkspaceFileBaseline).mock.calls[1][0]).toEqual(original);
});
