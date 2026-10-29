import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import * as preferencesApi from "../../conversation-preferences-api";
import * as contextApi from "../../context-usage-api";
import type { AgentRunEventV4, ContextCompactionReceiptV4, RunSummaryV4 } from "../../types";
import * as projectTemplatesApi from "../../project-templates-api";
import * as workflowApi from "../../composer-workflow-api";
import * as guidanceApi from "../../tauri-api";
import * as diagnosticsApi from "../../run-diagnostics-api";
import * as remoteJobApi from "../../remote-job-api";
import { WorkspaceShell } from "./WorkspaceShell";
import { ComposerDraftStore } from "./composerDrafts";
import * as branchApi from "../../conversation-branch-api";
import * as baselineApi from "../../workspace-navigation-api";
import * as composerFilesApi from "../../composer-file-api";
import * as referenceApi from "../../composer-reference-api";
import { SettingsPanel } from "../settings/SettingsPanel";
import { setComposerSendPreference } from "../settings/useComposerSendPreference";
import { setSelectionActionsEnabled } from "../../use-general-preferences";

const project = {
  id: "project-1",
  name: "PBMC 图谱",
  status: "running" as const,
  template: "single_cell_rna_seq" as const,
};

it("detects SSH interpreters on menu open, shows independent project prefixes and closes with immediate Escape", () => {
  const refresh = vi.fn(); const onEnvironmentChange = vi.fn();
  const selected = { environment: "envs:python311:r443", label: "Python .envs/python311 · R .envs/r443", interpreters: [
    { language: "python" as const, status: "available" as const, version: "Python 3.11.18" },
    { language: "r" as const, status: "available" as const, version: "R 4.4.3" },
  ] };
  const report = { project_id: project.id, connection_id: "lab", remote_root: "/work/project", requested_environment: selected.environment, observed_at: "2026-10-29T00:00:00Z", suggested_environment: selected.environment, truncated: false, environments: [selected] };
  const backend = { descriptor: { schema_version: 4 as const, backend_id: "ssh:lab", kind: "ssh" as const, isolation: "process" as const, available: false, supports_python: true, supports_r: true, supports_network_policy: false }, selectable: true, reason: null, python_status: "unverified" as const, r_status: "unverified" as const, resolved_image_id: null };
  render(<WorkspaceShell project={{ ...project, connection_id: "lab", remote_root: "/work/project" }} locale="zh-CN" computeBackends={[backend]} computeBackendId="ssh:lab" computeEnvironment={selected.environment} onComputeEnvironmentChange={onEnvironmentChange} sshEnvironments={{ report, selected, refresh, loading: false, error: false }} />);
  expect(screen.getByRole("button", { name: "Python 环境" })).toHaveTextContent("可用");
  fireEvent.click(screen.getByRole("button", { name: "选择计算后端" }));
  expect(refresh).toHaveBeenCalledOnce();
  expect(screen.getByRole("region", { name: "SSH 解释器检测" })).toHaveTextContent("Python 3.11.18");
  expect(screen.getByRole("region", { name: "SSH 解释器检测" })).toHaveTextContent("R 4.4.3");
  fireEvent.click(screen.getByRole("button", { name: "刷新远程环境" })); expect(refresh).toHaveBeenCalledTimes(2);
  fireEvent.keyDown(window, { key: "Escape" });
  expect(screen.queryByRole("region", { name: "V4 计算后端" })).not.toBeInTheDocument();
});

it("loads a remote Markdown image through the project preview instead of the webview origin", async () => {
  const path = "results/hcc/figures/umap_stage3_umap_final.png";
  const preview = vi.fn().mockResolvedValue({ relative_path: path, mime_type: "image/png", size_bytes: 8, sha256: "a".repeat(64), data_url: "data:image/png;base64,iVBORw0KGgo=" });
  render(<WorkspaceShell project={{ ...project, connection_id: "ssh-1", remote_root: "/research/hcc" }} locale="zh-CN" activeConversationId="chat-1" onPreviewImage={preview} messages={[{ id: "answer", role: "assistant", markdown: `![GSE149614 UMAP 最终注释](${path})` }]} />);
  await waitFor(() => expect(screen.getByRole("img", { name: "GSE149614 UMAP 最终注释" })).toHaveAttribute("src", "data:image/png;base64,iVBORw0KGgo="));
  expect(preview).toHaveBeenCalledWith(path);
});

it("visibility_catalog_refresh invalidates an open # request without clearing a selected historical chip", async () => {
  const item = { reference: { kind: "session" as const, project_id: project.id, id: "old-session" }, label: "Historical session", description: "Saved source" };
  let finishOld!: (items: typeof item[]) => void;
  const catalog = vi.spyOn(referenceApi, "composerReferenceCatalog").mockResolvedValue([item]);
  const send = vi.fn().mockResolvedValue(true);
  const view = render(<WorkspaceShell project={project} locale="en-US" activeConversationId="active" onSend={send} referenceCatalogVersion={0} />);
  const input = screen.getByRole("textbox", { name: /Describe/ });
  fireEvent.change(input, { target: { value: "#", selectionStart: 1 } });
  fireEvent.click(await screen.findByRole("option", { name: /Historical session/ }));
  await act(async () => { await new Promise((resolve) => requestAnimationFrame(resolve)); });
  expect(screen.getByRole("button", { name: "Remove reference: Historical session" })).toBeEnabled();
  catalog.mockReturnValueOnce(new Promise((resolve) => { finishOld = resolve; })).mockResolvedValue([]);
  fireEvent.change(input, { target: { value: "#", selectionStart: 1 } });
  await waitFor(() => expect(catalog).toHaveBeenCalledTimes(2));
  view.rerender(<WorkspaceShell project={project} locale="en-US" activeConversationId="active" onSend={send} referenceCatalogVersion={1} />);
  await screen.findByText("No matching references");
  await act(async () => finishOld([item]));
  expect(screen.queryByRole("option", { name: /Historical session/ })).toBeNull();
  expect(screen.getByRole("button", { name: "Remove reference: Historical session" })).toBeEnabled();
  catalog.mockResolvedValue([item]);
  view.rerender(<WorkspaceShell project={project} locale="en-US" activeConversationId="active" onSend={send} referenceCatalogVersion={2} />);
  await screen.findByRole("option", { name: /Historical session/ });
  vi.restoreAllMocks();
});

it("visibility_scope_and_escape drops a restore completion after A to B to A and keeps newer restore busy", async () => {
  let finishOld!: () => void; let finishNew!: () => void;
  const restore = vi.fn().mockImplementationOnce(() => new Promise<void>((resolve) => { finishOld = resolve; })).mockImplementationOnce(() => new Promise<void>((resolve) => { finishNew = resolve; }));
  const state = { project_id: project.id, conversation_id: "a", visibility: "shelved" as const, revision: 3, changed_at: null };
  const props = { project, locale: "en-US" as const, onSetVisibility: restore, activeConversationVisibility: state };
  const view = render(<WorkspaceShell {...props} activeConversationId="a" />);
  fireEvent.click(screen.getByRole("button", { name: "Restore conversation" }));
  view.rerender(<WorkspaceShell {...props} activeConversationId="b" />);
  view.rerender(<WorkspaceShell {...props} activeConversationId="a" />);
  fireEvent.click(screen.getByRole("button", { name: "Restore conversation" }));
  await act(async () => finishOld());
  expect(screen.getByRole("button", { name: "Restore conversation" })).toBeDisabled();
  await act(async () => finishNew());
  expect(screen.getByRole("button", { name: "Restore conversation" })).toBeEnabled();
  expect(restore).toHaveBeenNthCalledWith(2, "a", "visible", 3);
});

it("keeps a newer explicit retry busy after an old A to B to A retry fails", async () => {
  const old = deferred<void>(); const newer = deferred<void>();
  const retry = vi.fn().mockReturnValueOnce(old.promise).mockReturnValueOnce(newer.promise);
  const props = { project, locale: "en-US" as const, pendingVisibilityTarget: "shelved" as const, onRetryConversationVisibility: retry };
  const view = render(<WorkspaceShell {...props} activeConversationId="a" />);
  fireEvent.click(screen.getByRole("button", { name: "Retry original visibility action" }));
  view.rerender(<WorkspaceShell {...props} activeConversationId="b" pendingVisibilityTarget={undefined} />);
  expect(screen.queryByRole("button", { name: "Retry original visibility action" })).toBeNull();
  view.rerender(<WorkspaceShell {...props} activeConversationId="a" />);
  fireEvent.click(screen.getByRole("button", { name: "Retry original visibility action" }));
  await act(async () => old.reject(new Error("late old retry failure")));
  expect(screen.getByRole("button", { name: "Retry original visibility action" })).toBeDisabled();
  expect(screen.queryByText("Visibility action failed. Retry the same operation.")).toBeNull();
  await act(async () => newer.reject(new Error("current retry failure")));
  expect(screen.getByRole("button", { name: "Retry original visibility action" })).toBeEnabled();
  expect(screen.getByText("Visibility action failed. Retry the same operation.")).toBeVisible();
});

const fastProfile = {
  id: "fast-model",
  label: "Fast model",
  provider: "open_ai_compatible" as const,
  base_url: "https://api.openai.com/v1",
  model: "gpt-5.6-luna",
  credential_reference: null,
  supports_tools: true,
  supports_vision: false,
};

const originalRangeRect = Object.getOwnPropertyDescriptor(Range.prototype, "getBoundingClientRect");

function selectVisibleMessageText(node: Text) {
  Object.defineProperty(Range.prototype, "getBoundingClientRect", {
    configurable: true,
    value: vi.fn(() => ({ x: 20, y: 30, top: 30, left: 20, right: 120, bottom: 48, width: 100, height: 18, toJSON: () => ({}) })),
  });
  const range = document.createRange();
  range.selectNodeContents(node);
  const selection = window.getSelection()!;
  selection.removeAllRanges();
  selection.addRange(range);
  act(() => document.dispatchEvent(new Event("selectionchange")));
}

afterEach(() => {
  for (const key of Object.keys(localStorage)) {
    if (key.startsWith("omicsops.preferences.pendingSave:")) localStorage.removeItem(key);
  }
  vi.useRealTimers();
  vi.restoreAllMocks();
  setComposerSendPreference(false);
  setSelectionActionsEnabled(true);
  window.getSelection()?.removeAllRanges();
  if (originalRangeRect) Object.defineProperty(Range.prototype, "getBoundingClientRect", originalRangeRect);
  else delete (Range.prototype as Partial<Range>).getBoundingClientRect;
});

function deferred<T>() {
  let resolve!: (value: T | PromiseLike<T>) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise;
    reject = rejectPromise;
  });
  return { promise, resolve, reject };
}

async function openBaselineFiles() {
  fireEvent.click(screen.getByRole("button", { name: "Add context or choose mode" }));
  fireEvent.click(screen.getByRole("menuitem", { name: /Browse project files/ }));
  await screen.findByRole("tree", { name: "Remote project files" });
}
it("integrates explicit tree selection and same-request baseline retry across close and backend change", async () => {
  const create = vi.spyOn(baselineApi, "createWorkspaceFileBaseline").mockRejectedValueOnce({ code: "storage_failed", message: "unknown receipt" });
  vi.spyOn(baselineApi, "listWorkspaceFileBaselines").mockResolvedValue({ items: [], next_offset: null });
  const download = vi.fn();
  render(<WorkspaceShell project={{ ...project, connection_id: "c" }} locale="en-US" remoteFiles={[{ relative_path: "my script.py", directory: false, size_bytes: 12, modified_unix_seconds: 0 }]} onDownloadFile={download} />);
  await openBaselineFiles(); fireEvent.click(screen.getByRole("checkbox", { name: "Select for baseline my script.py" }));
  fireEvent.click(screen.getByRole("button", { name: "File baselines (1)" })); fireEvent.click(screen.getByRole("button", { name: "Save baseline" }));
  await screen.findByRole("button", { name: "Retry same request" }); const original = create.mock.calls[0][0];
  expect(original.files).toEqual([{ backend_id: "ssh:c", relative_path: "my script.py", expected_full_sha256: null }]); expect(original.branch_conversation_id).toBeNull(); expect(download).not.toHaveBeenCalled();
  fireEvent.keyDown(window, { key: "Escape" }); expect(screen.queryByRole("dialog", { name: "File baselines" })).not.toBeInTheDocument(); expect(screen.getByRole("complementary")).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Local" })); fireEvent.click(screen.getByRole("button", { name: "File baselines (0)" })); expect(screen.queryByRole("button", { name: "Retry same request" })).not.toBeInTheDocument();
  fireEvent.keyDown(window, { key: "Escape" }); fireEvent.click(screen.getByRole("button", { name: "SSH" })); fireEvent.click(screen.getByRole("button", { name: "File baselines (1)" }));
  fireEvent.click(await screen.findByRole("button", { name: "Retry same request" })); await waitFor(() => expect(create).toHaveBeenCalledTimes(2)); expect(create.mock.calls[1][0]).toEqual(original);
});

it("settles a closed baseline request in its original project without clearing another project's files", async () => {
  const pending = deferred<import("../../workspace-navigation-types").FileBaselineV1>(); const create = vi.spyOn(baselineApi, "createWorkspaceFileBaseline").mockReturnValue(pending.promise);
  vi.spyOn(baselineApi, "listWorkspaceFileBaselines").mockResolvedValue({ items: [], next_offset: null });
  const file = { relative_path: "x.txt", directory: false, size_bytes: 1, modified_unix_seconds: 0 };
  const view = render(<WorkspaceShell project={{ ...project, connection_id: "c" }} locale="en-US" remoteFiles={[file]} />);
  await openBaselineFiles(); fireEvent.click(screen.getByRole("checkbox", { name: "Select for baseline x.txt" })); fireEvent.click(screen.getByRole("button", { name: "File baselines (1)" })); fireEvent.click(screen.getByRole("button", { name: "Save baseline" }));
  const request = create.mock.calls[0][0]; fireEvent.keyDown(window, { key: "Escape" });
  view.rerender(<WorkspaceShell project={{ ...project, id: "other", connection_id: "c" }} locale="en-US" remoteFiles={[file]} />);
  fireEvent.click(screen.getByRole("checkbox", { name: "Select for baseline x.txt" }));
  await act(async () => pending.resolve({ schema_version: 1, id: "b", project_id: project.id, branch_conversation_id: null, title: request.title, entries: [], manifest_sha256: "a".repeat(64), captured_text_bytes: 0, created_at: "now" }));
  expect(screen.getByRole("checkbox", { name: "Select for baseline x.txt" })).toBeChecked();
  view.rerender(<WorkspaceShell project={{ ...project, connection_id: "c" }} locale="en-US" remoteFiles={[file]} />); fireEvent.click(screen.getByRole("button", { name: "File baselines (1)" })); await screen.findByText(/Saved baseline: /); expect(create).toHaveBeenCalledTimes(1);
});

it("keeps baseline text under a later real table dialog and closes only the visible top layer immediately", async () => {
  const hash = "a".repeat(64);
  const saved: import("../../workspace-navigation-types").FileBaselineV1 = { schema_version: 1, id: "b", project_id: project.id, branch_conversation_id: null, title: "Before", entries: [{ backend_id: "ssh:c", relative_path: "x.txt", content_mode: "text_snapshot", size_bytes: 1, hash: { scope: "full_file", sha256: hash }, omission: null, captured_at: "now" }], manifest_sha256: hash, captured_text_bytes: 1, created_at: "now" };
  vi.spyOn(baselineApi, "listWorkspaceFileBaselines").mockResolvedValue({ items: [saved], next_offset: null }); vi.spyOn(baselineApi, "getWorkspaceFileBaseline").mockResolvedValue(saved);
  const pending = deferred<import("../../workspace-navigation-types").BaselineTextSnapshotV1>(); vi.spyOn(baselineApi, "readWorkspaceBaselineText").mockReturnValue(pending.promise);
  vi.spyOn(composerFilesApi, "previewProjectTable").mockRejectedValue("unavailable_backend");
  render(<WorkspaceShell project={{ ...project, connection_id: "c" }} locale="en-US" remoteFiles={[{ relative_path: "x.txt", directory: false, size_bytes: 1, modified_unix_seconds: 0 }, { relative_path: "x.csv", directory: false, size_bytes: 1, modified_unix_seconds: 0 }]} />);
  await openBaselineFiles(); fireEvent.click(screen.getByRole("button", { name: "File baselines (0)" })); fireEvent.click(await screen.findByRole("button", { name: "Open baseline Before" })); fireEvent.click(await screen.findByRole("button", { name: "View saved text x.txt" }));
  fireEvent.click(screen.getByRole("button", { name: "Preview table x.csv" }));
  expect(screen.getByRole("dialog", { name: "Preview table" })).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "File baselines (0)" })).toBeDisabled();
  await act(async () => pending.resolve({ project_id: project.id, baseline_id: "b", entry_index: 0, relative_path: "x.txt", full_sha256: hash, text: "saved" }));
  expect(screen.getByRole("dialog", { name: "Saved baseline text" })).toHaveTextContent("saved");
  fireEvent.keyDown(window, { key: "Escape" }); expect(screen.queryByRole("dialog", { name: "Preview table" })).not.toBeInTheDocument(); expect(screen.getByRole("dialog", { name: "Saved baseline text" })).toBeInTheDocument();
  fireEvent.keyDown(window, { key: "Escape" }); expect(screen.queryByRole("dialog", { name: "Saved baseline text" })).not.toBeInTheDocument(); expect(screen.getByRole("dialog", { name: "File baselines" })).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Compare current files" })); expect(screen.getByRole("dialog", { name: "Baseline diff" })).toBeInTheDocument(); fireEvent.keyDown(window, { key: "Escape" }); expect(screen.queryByRole("dialog", { name: "Baseline diff" })).not.toBeInTheDocument(); expect(screen.getByRole("dialog", { name: "File baselines" })).toBeInTheDocument();
});

it("offers branch association only from the verified branch identity and preserves source navigation", async () => {
  const verified: import("../../types").ConversationBranchV4 = { request_id: "r", branch_conversation_id: "branch", project_id: project.id, source_conversation_id: "source", source_message_id: "m", source_sequence: 1, source_head_sequence: 2, checkpoint_kind: "after_response", boundary_hash: "hash", request_hash: "hash", state: "active", created_at: "now", updated_at: "now" };
  vi.spyOn(branchApi, "getConversationBranch").mockResolvedValueOnce({ ...verified, project_id: "wrong" }).mockResolvedValueOnce(verified);
  vi.spyOn(baselineApi, "listWorkspaceFileBaselines").mockResolvedValue({ items: [], next_offset: null });
  const onSelect = vi.fn(); const props = { project: { ...project, connection_id: "c" }, locale: "en-US" as const, onOpenBranch: vi.fn(), onSelectConversation: onSelect };
  const view = render(<WorkspaceShell {...props} activeConversationId="wrong" />); await act(async () => {}); expect(screen.queryByRole("button", { name: "Branch file baselines" })).not.toBeInTheDocument();
  view.rerender(<WorkspaceShell {...props} activeConversationId="branch" />); fireEvent.click(await screen.findByRole("button", { name: "Open source conversation" })); expect(onSelect).toHaveBeenCalledWith("source");
  fireEvent.click(screen.getByRole("button", { name: "Branch file baselines" })); expect(screen.getByRole("checkbox", { name: "Associate verified branch" })).not.toBeChecked();
  fireEvent.click(screen.getByRole("checkbox", { name: "Only verified branch baselines" })); await waitFor(() => expect(baselineApi.listWorkspaceFileBaselines).toHaveBeenLastCalledWith({ project_id: project.id, branch_conversation_id: "branch", offset: 0, limit: 20 }));
});

describe("WorkspaceShell", () => {
  it("keeps an uncancelled inspection visibly pending after reopening, then requires an explicit fresh query", async () => {
    const old = deferred<import("../../types").KernelInspectionV4>();
    const metadata = { project_id: project.id, conversation_id: "current", run_id: "old-run", language: "python" as const, session_id: "old", status: "ready" as const, objects: [], truncated: false };
    const inspect = vi.spyOn(guidanceApi, "inspectKernelV4").mockReturnValueOnce(old.promise).mockResolvedValueOnce({ ...metadata, session_id: "new" });
    render(<WorkspaceShell project={project} locale="en-US" activeConversationId="current" agentRunEventsV4={[{ schema_version: 4, run_id: "old-run", project_id: project.id, conversation_id: "current", sequence: 1, occurred_at: "2026-10-07T00:00:00Z", previous_hash: "", event_hash: "hash", event: { kind: "run_spec_frozen", approval_hash: "a", spec_hash: "b" } }]} />);
    fireEvent.click(screen.getByRole("button", { name: "Run environment" })); fireEvent.click(screen.getByRole("button", { name: "View kernel metadata" })); fireEvent.click(screen.getByRole("button", { name: "Refresh metadata" }));
    fireEvent.keyDown(window, { key: "Escape" }); fireEvent.click(screen.getByRole("button", { name: "View kernel metadata" }));
    const refresh = screen.getByRole("button", { name: "Refresh metadata" }); const disabledWhileOldPending = refresh.hasAttribute("disabled");
    const pendingText = screen.getByRole("dialog", { name: "Kernel metadata" }).textContent;
    fireEvent.click(refresh); expect(inspect).toHaveBeenCalledTimes(1);
    await act(async () => { old.resolve({ ...metadata, objects: [{ name: "old_session_object", type_label: "opaque", dimensions: null, element_count: null, summary: "opaque" }] }); });
    expect(disabledWhileOldPending).toBe(true); expect(pendingText).toMatch(/request.*pending/i);
    expect(screen.queryByText("old_session_object")).not.toBeInTheDocument(); expect(inspect).toHaveBeenCalledTimes(1); expect(refresh).toBeEnabled();
    fireEvent.click(refresh); await waitFor(() => expect(screen.getByRole("dialog", { name: "Kernel metadata" })).toHaveTextContent("new")); expect(inspect).toHaveBeenCalledTimes(2);
  });
  it("trajectory remains lower and later in DOM while Runtime and metadata own focus and Escape", async () => {
    const pending = deferred<import("../../types").KernelInspectionV4>();
    const retryBoundary = deferred<import("../../types").RuntimeBoundaryViewV4>();
    vi.spyOn(guidanceApi, "agentV4RuntimeBoundary").mockRejectedValueOnce("unavailable").mockReturnValueOnce(retryBoundary.promise);
    const inspect = vi.spyOn(guidanceApi, "inspectKernelV4").mockReturnValue(pending.promise);
    render(<WorkspaceShell project={project} locale="en-US" activeConversationId="current" agentRunEventsV4={[{ schema_version: 4, run_id: "old-run", project_id: project.id, conversation_id: "current", sequence: 1, occurred_at: "2026-10-07T00:00:00Z", previous_hash: "", event_hash: "hash", event: { kind: "run_spec_frozen", approval_hash: "a", spec_hash: "b" } }]} />);
    fireEvent.change(screen.getByRole("textbox", { name: /Describe a research goal/ }), { target: { value: "/trajectory" } });
    fireEvent.click(screen.getByRole("option", { name: /\/trajectory/ }));
    const trajectory = screen.getByRole("dialog", { name: "Run trajectory" });
    const runEntry = within(trajectory).getByRole("button", { name: "Run environment" }); runEntry.focus(); fireEvent.click(runEntry);
    const runtime = screen.getByRole("dialog", { name: "Runtimes" });
    const retry = await within(runtime).findByRole("button", { name: "Retry boundary" }); retry.focus(); fireEvent.click(retry);
    const parentClose = within(runtime).getByRole("button", { name: "Close runtimes" }); const focusedAfterControlRemoved = document.activeElement;
    parentClose.blur(); const parentTab = new KeyboardEvent("keydown", { key: "Tab", bubbles: true, cancelable: true }); window.dispatchEvent(parentTab);
    const parentConsumedBodyTab = parentTab.defaultPrevented; const parentFocusedAfterTab = document.activeElement;
    const entry = screen.getByRole("button", { name: "View kernel metadata" }); entry.focus(); fireEvent.click(entry);
    const child = screen.getByRole("dialog", { name: "Kernel metadata" }); const close = within(child).getByRole("button", { name: "Close kernel metadata" });
    expect(Array.from(document.querySelectorAll('[role="dialog"]')).at(-1)).toBe(trajectory);
    close.blur(); const tab = new KeyboardEvent("keydown", { key: "Tab", bubbles: true, cancelable: true }); window.dispatchEvent(tab);
    const consumedBodyTab = tab.defaultPrevented; const focusedAfterBodyTab = document.activeElement;
    fireEvent.click(within(child).getByRole("button", { name: "Refresh metadata" }));
    const closeFocusedWhileLoading = document.activeElement === close;
    close.blur(); fireEvent.click(screen.getByRole("tab", { name: "R" })); const recoveredAfterScopeReset = document.activeElement === close;
    await act(async () => {
      pending.resolve({ project_id: project.id, conversation_id: "current", run_id: "old-run", language: "python", session_id: "old", status: "ready", objects: [], truncated: false });
      retryBoundary.resolve({ project_id: project.id, source: { kind: "frozen_run", run_id: "old-run" }, compute_selection: { schema_version: 4, backend_id: "local", backend_kind: "local", autonomy_mode: "supervised", environment: "system", network_policy: "host_inherited" }, execution_location: "local_host", isolation: "process", limits: ["same_user_permissions"], interactive_lifecycle: "run_scoped_no_restart_reconnect", detached_job_lifecycle: "unsupported", verification: "not_checked_by_this_view" });
    });
    expect(focusedAfterControlRemoved).toBe(parentClose); expect(parentConsumedBodyTab).toBe(true); expect(parentFocusedAfterTab).toBe(parentClose);
    expect(consumedBodyTab).toBe(true); expect(focusedAfterBodyTab).toBe(close); expect(closeFocusedWhileLoading).toBe(true); expect(recoveredAfterScopeReset).toBe(true);
    close.blur(); fireEvent.keyDown(window, { key: "Tab", shiftKey: true }); expect(within(child).getByRole("button", { name: "Refresh metadata" })).toHaveFocus();
    fireEvent.keyDown(document.body, { key: "Escape" }); expect(entry).toHaveFocus(); expect(runtime).toBeVisible(); expect(trajectory).toBeVisible();
    fireEvent.keyDown(window, { key: "Escape" }); expect(screen.queryByRole("dialog", { name: "Runtimes" })).not.toBeInTheDocument(); expect(runEntry).toHaveFocus(); expect(trajectory).toBeVisible();
    expect(inspect).toHaveBeenCalledTimes(1);
  });
  it.each(["close", "project", "conversation", "language", "run"])("real live-run inspection discards late objects after %s", async (change) => {
    const pending = deferred<import("../../types").KernelInspectionV4>();
    const inspect = vi.spyOn(guidanceApi, "inspectKernelV4").mockReturnValue(pending.promise);
    const event: import("../../types").AgentRunEventV4 = { schema_version: 4, run_id: "live-run", project_id: project.id, conversation_id: "current", sequence: 1, occurred_at: "2026-10-07T00:00:00Z", previous_hash: "", event_hash: "hash", event: { kind: "run_spec_frozen", approval_hash: "a", spec_hash: "b" } };
    const props = { project, locale: "en-US" as const, activeConversationId: "current", activeRunId: "live-run", runStarted: true, agentRunEventsV4: [event] };
    const view = render(<WorkspaceShell {...props} />);
    fireEvent.click(screen.getByRole("button", { name: "Run environment" })); fireEvent.click(screen.getByRole("button", { name: "View kernel metadata" }));
    fireEvent.click(screen.getByRole("button", { name: "Refresh metadata" })); expect(inspect).toHaveBeenCalledTimes(1);
    const close = screen.getByRole("button", { name: "Close kernel metadata" }); expect(close).toHaveFocus();
    // Even if focus is outside, the top dialog owns Tab and immediately Escape.
    close.blur(); expect(document.activeElement).toBe(document.body);
    fireEvent.keyDown(window, { key: "Tab", shiftKey: true }); expect(close).toHaveFocus();
    if (change === "close") fireEvent.keyDown(document.body, { key: "Escape" });
    if (change === "language") fireEvent.click(screen.getByRole("tab", { name: "R" }));
    view.rerender(<WorkspaceShell {...props} project={change === "project" ? { ...project, id: "other" } : project} activeConversationId={change === "conversation" ? "other" : "current"} activeRunId={change === "run" ? "new-run" : "live-run"} agentRunEventsV4={change === "run" ? [{ ...event, run_id: "new-run" }] : [event]} />);
    if (change === "run") fireEvent.click(screen.getByRole("button", { name: "Run environment" }));
    await act(async () => { pending.resolve({ project_id: project.id, conversation_id: "current", run_id: "live-run", language: "python", session_id: "old", status: "ready", objects: [{ name: "old_session_object", type_label: "opaque", dimensions: null, element_count: null, summary: "opaque" }], truncated: false }); });
    expect(screen.queryByText("old_session_object")).not.toBeInTheDocument(); expect(inspect).toHaveBeenCalledTimes(1);
    view.unmount();
  });
  it("runtime_escape_closes_only_top_overlay; frozen inspection and remote jobs have independent manual lifecycles", async () => {
    const inspect = vi.spyOn(guidanceApi, "inspectKernelV4").mockResolvedValue({ project_id: project.id, conversation_id: "current", run_id: "old-run", language: "python", session_id: null, status: "not_started", objects: [], truncated: false });
    const list = vi.spyOn(remoteJobApi, "listRemoteJobsV4").mockResolvedValue({ jobs: [], limit: 100, remote_liveness_checked: false });
    const probe = vi.spyOn(guidanceApi, "agentV4ComputeBackends"); const onSend = vi.fn(); const onStartRun = vi.fn(); const onComputeBackendChange = vi.fn();
    render(<WorkspaceShell project={project} locale="en-US" activeConversationId="current" onSend={onSend} onStartRun={onStartRun} onComputeBackendChange={onComputeBackendChange} agentRunEventsV4={[{ schema_version: 4, run_id: "old-run", project_id: project.id, conversation_id: "current", sequence: 1, occurred_at: "2026-10-07T00:00:00Z", previous_hash: "", event_hash: "hash", event: { kind: "run_spec_frozen", approval_hash: "a", spec_hash: "b" } }]} />);
    fireEvent.click(screen.getByRole("button", { name: "Run environment" }));
    const entry = screen.getByRole("button", { name: "View kernel metadata" }); entry.focus(); fireEvent.click(entry);
    expect(screen.getByRole("dialog", { name: "Kernel metadata" })).toBeVisible(); expect(inspect).not.toHaveBeenCalled();
    fireEvent.keyDown(window, { key: "Escape" });
    expect(screen.queryByRole("dialog", { name: "Kernel metadata" })).not.toBeInTheDocument(); expect(entry).toHaveFocus(); expect(screen.getByRole("dialog", { name: "Runtimes" })).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: "Remote jobs" })); fireEvent.keyDown(window, { key: "Escape" }); expect(list).toHaveBeenCalledTimes(1);
    fireEvent.click(entry); fireEvent.click(screen.getByRole("button", { name: "Refresh metadata" }));
    await waitFor(() => expect(screen.getByRole("dialog", { name: "Kernel metadata" })).toHaveTextContent("Not started"));
    expect(inspect).toHaveBeenCalledTimes(1); expect(inspect).toHaveBeenCalledWith({ project_id: project.id, conversation_id: "current", run_id: "old-run", language: "python", expected_session_id: null });
    expect(probe).not.toHaveBeenCalled(); expect(onSend).not.toHaveBeenCalled(); expect(onStartRun).not.toHaveBeenCalled(); expect(onComputeBackendChange).not.toHaveBeenCalled();
    fireEvent.keyDown(document.body, { key: "Escape" }); expect(screen.getByRole("dialog", { name: "Runtimes" })).toBeVisible();
    fireEvent.keyDown(window, { key: "Escape" }); expect(screen.queryByRole("dialog", { name: "Runtimes" })).not.toBeInTheDocument();
  });
  it("opens metadata for the current owner without probing or starting compute and immediately escapes child then parent", async () => {
    const list = vi.spyOn(remoteJobApi, "listRemoteJobsV4").mockResolvedValue({ jobs: [], limit: 100, remote_liveness_checked: false });
    const refresh = vi.spyOn(remoteJobApi, "refreshRemoteJobV4");
    const probe = vi.spyOn(guidanceApi, "agentV4ComputeBackends");
    const onSend = vi.fn(); const onStartRun = vi.fn(); const onComputeBackendChange = vi.fn();
    render(<WorkspaceShell project={project} locale="en-US" activeConversationId="current" onSend={onSend} onStartRun={onStartRun} onComputeBackendChange={onComputeBackendChange} />);
    fireEvent.click(screen.getByRole("button", { name: "Python environment" }));
    const entry = screen.getByRole("button", { name: "Remote jobs" }); entry.focus();
    fireEvent.click(entry);
    expect(screen.getByRole("dialog", { name: "Remote background jobs" })).toBeVisible();
    fireEvent.keyDown(window, { key: "Escape" });
    expect(screen.queryByRole("dialog", { name: "Remote background jobs" })).not.toBeInTheDocument();
    expect(screen.getByRole("dialog", { name: "Runtimes" })).toBeVisible(); expect(entry).toHaveFocus();
    expect(list).toHaveBeenCalledWith({ project_id: project.id, conversation_id: "current" });
    expect(refresh).not.toHaveBeenCalled(); expect(probe).not.toHaveBeenCalled();
    expect(onSend).not.toHaveBeenCalled(); expect(onStartRun).not.toHaveBeenCalled(); expect(onComputeBackendChange).not.toHaveBeenCalled();
    expect(screen.queryByRole("button", { name: /Cancel remote job/ })).not.toBeInTheDocument();
    fireEvent.keyDown(window, { key: "Escape" }); expect(screen.queryByRole("dialog", { name: "Runtimes" })).not.toBeInTheDocument();
  });
  it.each(["close", "project", "conversation", "connection", "root"])("stops the real child query window when owner %s changes", async (change) => {
    vi.useFakeTimers();
    const job = { project_id: project.id, conversation_id: "current", job_id: "job", run_id: "run", call_id: "call", backend_id: "ssh:one", language: "python", environment: "system", request_sha256: "hash", recorded_state: "running" as const, query_eligibility: "needs_root_check" as const, recorded_at: null };
    vi.spyOn(remoteJobApi, "listRemoteJobsV4").mockResolvedValue({ jobs: [job], limit: 100, remote_liveness_checked: false });
    const refresh = vi.spyOn(remoteJobApi, "refreshRemoteJobV4");
    const owner = { ...project, connection_id: "one", remote_root: "/one" };
    const view = render(<WorkspaceShell project={owner} locale="en-US" activeConversationId="current" />);
    fireEvent.click(screen.getByRole("button", { name: "Python environment" })); fireEvent.click(screen.getByRole("button", { name: "Remote jobs" }));
    await act(async () => { await Promise.resolve(); });
    fireEvent.click(screen.getByRole("button", { name: "All project jobs" })); await act(async () => { await Promise.resolve(); });
    fireEvent.click(screen.getByRole("button", { name: /job · python/ })); fireEvent.click(screen.getByRole("button", { name: "Start automatic queries" }));
    if (change === "close") fireEvent.click(screen.getByRole("button", { name: "Close remote jobs" }));
    view.rerender(<WorkspaceShell project={{ ...owner, ...(change === "project" ? { id: "other" } : change === "connection" ? { connection_id: "two" } : change === "root" ? { remote_root: "/two" } : {}) }} locale="en-US" activeConversationId={change === "conversation" ? "other" : "current"} />);
    await act(async () => { await vi.advanceTimersByTimeAsync(900000); });
    expect(refresh).not.toHaveBeenCalled();
    view.unmount();
    // jsdom queues selectionchange at 0 ms when restoring parent focus.
    await act(async () => { await vi.advanceTimersByTimeAsync(0); });
    expect(vi.getTimerCount()).toBe(0);
  });
  it("opens jobs from a frozen run without changing the recorded boundary or execution controls", async () => {
    const list = vi.spyOn(remoteJobApi, "listRemoteJobsV4").mockResolvedValue({ jobs: [], limit: 100, remote_liveness_checked: false });
    const boundary = { project_id: project.id, source: { kind: "frozen_run" as const, run_id: "old-run" }, compute_selection: { schema_version: 4 as const, backend_id: "ssh:old", backend_kind: "ssh" as const, autonomy_mode: "supervised" as const, environment: "old-env", network_policy: "host_inherited" as const }, execution_location: "ssh_host" as const, isolation: "process" as const, limits: ["same_user_permissions" as const], interactive_lifecycle: "run_scoped_no_restart_reconnect" as const, detached_job_lifecycle: "ssh_linux_only" as const, verification: "not_checked_by_this_view" as const };
    const readBoundary = vi.spyOn(guidanceApi, "agentV4RuntimeBoundary").mockResolvedValue(boundary);
    const onStartRun = vi.fn(); const onSend = vi.fn(); const onComputeBackendChange = vi.fn(); const onComputeEnvironmentChange = vi.fn();
    render(<WorkspaceShell project={{ ...project, connection_id: "current" }} locale="en-US" activeConversationId="current" onStartRun={onStartRun} onSend={onSend} onComputeBackendChange={onComputeBackendChange} onComputeEnvironmentChange={onComputeEnvironmentChange} agentRunEventsV4={[{ schema_version: 4, run_id: "old-run", project_id: project.id, conversation_id: "current", sequence: 1, occurred_at: "2026-10-07T00:00:00Z", previous_hash: "", event_hash: "hash", event: { kind: "run_spec_frozen", approval_hash: "a", spec_hash: "b" } }]} />);
    fireEvent.click(screen.getByRole("button", { name: "Run environment" }));
    await waitFor(() => expect(screen.getByRole("dialog", { name: "Runtimes" })).toHaveTextContent("old-env"));
    fireEvent.click(screen.getByRole("button", { name: "Remote jobs" }));
    expect(list).toHaveBeenCalledWith({ project_id: project.id, conversation_id: "current" });
    expect(readBoundary).toHaveBeenCalledTimes(1);
    expect(readBoundary).toHaveBeenCalledWith({ source: "frozen_run", project_id: project.id, run_id: "old-run" });
    expect(onStartRun).not.toHaveBeenCalled(); expect(onSend).not.toHaveBeenCalled(); expect(onComputeBackendChange).not.toHaveBeenCalled(); expect(onComputeEnvironmentChange).not.toHaveBeenCalled();
    fireEvent.keyDown(window, { key: "Escape" });
    expect(screen.getByRole("dialog", { name: "Runtimes" })).toHaveTextContent("old-env");
    expect(screen.queryByRole("button", { name: "Ask Agent to prepare environment" })).not.toBeInTheDocument();
  });
  it("shows an empty metadata state on an unsupported local backend without querying status", async () => {
    vi.spyOn(remoteJobApi, "listRemoteJobsV4").mockResolvedValue({ jobs: [], limit: 100, remote_liveness_checked: false });
    const refresh = vi.spyOn(remoteJobApi, "refreshRemoteJobV4");
    render(<WorkspaceShell project={project} locale="en-US" />);
    fireEvent.click(screen.getByRole("button", { name: "Python environment" })); fireEvent.click(screen.getByRole("button", { name: "Remote jobs" }));
    expect(await screen.findByText("No job records in this scope.")).toBeVisible();
    expect(remoteJobApi.listRemoteJobsV4).toHaveBeenCalledWith({ project_id: project.id, conversation_id: null });
    expect(refresh).not.toHaveBeenCalled();
  });
  it.each(["empty", "edited", "edited then emptied", "existing", "not opened"] as const)("seeds branch text only into an unchanged empty target: %s", async (targetState) => {
    const store = new ComposerDraftStore();
    const source = { projectId: project.id, conversationId: "source" };
    const target = { projectId: project.id, conversationId: "branch" };
    store.update(source, "Source draft");
    if (targetState === "existing") store.update(target, "Existing target");
    const checkpoint = { source_message_id: "u1", source_sequence: 1, source_head_sequence: 1, checkpoint_kind: "before_user" as const, boundary_hash: "boundary" };
    vi.spyOn(branchApi, "conversationBranchCheckpoint").mockResolvedValue(checkpoint);
    vi.spyOn(branchApi, "createConversationBranch").mockResolvedValue({ ...checkpoint, request_id: "branch-request", branch_conversation_id: "branch", project_id: project.id, source_conversation_id: "source", request_hash: "request-hash", state: "active", created_at: "", updated_at: "" });
    const pending = deferred<boolean>();
    const onOpenBranch = vi.fn().mockReturnValue(pending.promise);
    const view = render(<WorkspaceShell project={project} locale="en-US" activeConversationId="source" composerDraftStore={store} onOpenBranch={onOpenBranch} messages={[{ id: "u1", role: "user", markdown: "Original user text" }]} />);
    fireEvent.click(screen.getByRole("button", { name: "Branch before this message" }));
    await waitFor(() => expect(onOpenBranch).toHaveBeenCalled());
    view.rerender(<WorkspaceShell project={project} locale="en-US" activeConversationId="branch" composerDraftStore={store} onOpenBranch={onOpenBranch} />);
    const input = screen.getByRole("textbox", { name: /Describe a research goal/ });
    if (targetState === "edited" || targetState === "edited then emptied") {
      fireEvent.change(input, { target: { value: "New target draft" } });
      if (targetState === "edited then emptied") fireEvent.change(input, { target: { value: "" } });
    }
    await act(async () => pending.resolve(targetState !== "not opened"));
    expect(input).toHaveValue(targetState === "empty" ? "Original user text" : targetState === "edited" ? "New target draft" : targetState === "existing" ? "Existing target" : "");
    expect(store.read(source).text).toBe("Source draft");
  });

  it("inserts a Quick Action as an existing workflow reference without sending or running", async () => {
    const workflow = { id: "workflow-1", project_id: project.id, name: "QC workflow", description: "Inspect counts", steps: ["Inspect counts"], enabled: true };
    vi.spyOn(projectTemplatesApi, "listQuickActions").mockResolvedValue([{ id: "action-1", project_id: project.id, name: "Review QC", description: "Insert workflow", workflow_id: workflow.id, enabled: true }]);
    vi.spyOn(projectTemplatesApi, "listSpecialistTemplates").mockResolvedValue([]);
    vi.spyOn(workflowApi, "listComposerWorkflows").mockResolvedValue([workflow]);
    const onSend = vi.fn();
    render(<WorkspaceShell project={project} locale="en-US" onSend={onSend} />);

    const composer = screen.getByRole("textbox", { name: /Describe a research goal/ });
    fireEvent.change(composer, { target: { value: "Keep this draft" } });
    fireEvent.click(screen.getByRole("button", { name: "Agent permissions" }));
    fireEvent.click(screen.getByRole("menuitem", { name: /Quick actions & roles/ }));
    fireEvent.click(await screen.findByRole("button", { name: "Use Review QC" }));

    expect(screen.getByText("QC workflow")).toBeInTheDocument();
    expect(composer).toHaveValue("Keep this draft");
    expect(onSend).not.toHaveBeenCalled();
  });

  it("appends Specialist instructions as visible draft text without sending or changing execution controls", async () => {
    const specialist = { id: "specialist-1", project_id: project.id, name: "Methods reviewer", description: "Review methods", instructions: "Identify unsupported claims.", enabled: true };
    vi.spyOn(projectTemplatesApi, "listQuickActions").mockResolvedValue([]);
    vi.spyOn(projectTemplatesApi, "listSpecialistTemplates").mockResolvedValue([specialist]);
    vi.spyOn(workflowApi, "listComposerWorkflows").mockResolvedValue([]);
    const onSend = vi.fn();
    render(<WorkspaceShell project={project} locale="en-US" onSend={onSend} />);

    const composer = screen.getByRole("textbox", { name: /Describe a research goal/ });
    fireEvent.change(composer, { target: { value: "Keep this draft exactly." } });
    fireEvent.click(screen.getByRole("button", { name: "Agent permissions" }));
    fireEvent.click(screen.getByRole("menuitem", { name: /Quick actions & roles/ }));
    fireEvent.click(await screen.findByRole("button", { name: "Use Methods reviewer" }));

    expect(composer).toHaveValue("Keep this draft exactly.\n\n[Specialist: Methods reviewer]\nIdentify unsupported claims.");
    expect(onSend).not.toHaveBeenCalled();
    expect(screen.queryByRole("dialog", { name: "Project template picker" })).not.toBeInTheDocument();
  });

  it("updates the visible shortcut and main composer keyboard behavior when the shared preference changes", async () => {
    const onSend = vi.fn().mockResolvedValue(true);
    render(<><WorkspaceShell project={project} locale="en-US" onSend={onSend} computeBackendId="local" computeBackends={[{ descriptor: { schema_version: 4, backend_id: "local", kind: "local", isolation: "process", available: true, supports_python: true, supports_r: false, supports_network_policy: false }, selectable: true, reason: null, python_status: "available", r_status: "unavailable", resolved_image_id: null }]} /><SettingsPanel locale="en-US" initialSection="general" onClose={() => undefined} /></>);
    const composer = screen.getByRole("textbox", { name: /Describe a research goal/ });
    fireEvent.change(composer, { target: { value: "Inspect counts" } });
    fireEvent.click(screen.getByRole("button", { name: "Agent permissions" }));
    const shortcut = screen.getByRole("menuitemcheckbox", { name: "Send with Ctrl/Cmd+Enter" });
    expect(shortcut).toHaveAttribute("aria-checked", "false");

    fireEvent.change(screen.getByRole("combobox", { name: "Send shortcut" }), { target: { value: "modifier" } });
    expect(shortcut).toHaveAttribute("aria-checked", "true");
    fireEvent.keyDown(composer, { key: "Enter", code: "Enter" });
    expect(onSend).not.toHaveBeenCalled();
    fireEvent.keyDown(composer, { key: "Enter", code: "Enter", ctrlKey: true });
    await waitFor(() => expect(onSend).toHaveBeenCalledWith("Inspect counts", "chat"));

    fireEvent.click(shortcut);
    expect(screen.getByRole("combobox", { name: "Send shortcut" })).toHaveValue("enter");
    await waitFor(() => expect(composer).toHaveValue(""));
    fireEvent.change(composer, { target: { value: "Use Enter" } });
    fireEvent.keyDown(composer, { key: "Enter", code: "Enter" });
    await waitFor(() => expect(onSend).toHaveBeenLastCalledWith("Use Enter", "chat"));
  });

  it("quotes selected persisted message text into the existing draft without sending and hides actions when disabled", () => {
    setSelectionActionsEnabled(true);
    const onSend = vi.fn();
    render(<><WorkspaceShell project={project} locale="en-US" onSend={onSend} activeConversationId="conversation-1" messages={[{ id: "assistant-1", role: "assistant", markdown: "Verified observation" }]} /><SettingsPanel locale="en-US" initialSection="general" onClose={() => undefined} /></>);
    const composer = screen.getByRole("textbox", { name: /Describe a research goal/ });
    fireEvent.change(composer, { target: { value: "Keep this draft" } });
    selectVisibleMessageText(screen.getByText("Verified observation").firstChild as Text);
    fireEvent.click(screen.getByRole("button", { name: "Quote in draft" }));

    expect(composer).toHaveValue("Keep this draft\n\n[Quoted from Agent response]\n> Verified observation");
    expect(onSend).not.toHaveBeenCalled();

    selectVisibleMessageText(screen.getByText("Verified observation").firstChild as Text);
    expect(screen.getByRole("toolbar", { name: "Message selection actions" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("checkbox", { name: "Show text selection actions" }));
    expect(screen.queryByRole("toolbar", { name: "Message selection actions" })).not.toBeInTheDocument();
    expect(window.localStorage.getItem("omicsops.general.selectionActionsEnabled")).toBe("false");
  });

  it.each([
    { label: "Shift+Enter", event: { key: "Enter", code: "Enter", shiftKey: true } },
    { label: "IME composition", event: { key: "Enter", code: "Enter", isComposing: true } },
    { label: "legacy IME keyCode", event: { key: "Enter", code: "Enter", keyCode: 229 } },
  ])("does not send the main composer for $label", ({ event }) => {
    const onSend = vi.fn();
    render(<WorkspaceShell project={project} locale="en-US" onSend={onSend} />);
    const composer = screen.getByRole("textbox", { name: /Describe a research goal/ });
    fireEvent.change(composer, { target: { value: "Keep editing" } });
    fireEvent.keyDown(composer, event);
    expect(onSend).not.toHaveBeenCalled();
  });
  it.each(["running", "waiting_for_approval", "needs_attention", "cancelled", "completed"])("does not present an ordinary %s contract as a Plan or hide its trace", (status) => {
    const plan = { schema_version: 4 as const, objective: "internal ordinary contract", steps: ["internal step"], completion_criteria: ["evidence"], requested_capabilities: [] };
    render(<WorkspaceShell project={project} locale="zh-CN" agentMode="agent"
      activeRunId="ordinary" runStarted={status === "running" || status === "waiting_for_approval"}
      v4Plan={{ run_id: "ordinary", status, plan, plan_hash: "internal", compute_selection: null, approval_hash: "internal", session_mode: "agent" }}
      agentRunEventsV4={[{ schema_version: 4, run_id: "ordinary", project_id: project.id, conversation_id: "c", sequence: 1, occurred_at: "2026-09-10T00:00:00Z", previous_hash: "", event_hash: "h", event: { kind: "model_text", text: "正在核对文献记录。" } }]} />);
    expect(screen.queryByText(/计划已生成/)).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "批准并运行" })).not.toBeInTheDocument();
    expect(screen.queryByText("internal ordinary contract")).not.toBeInTheDocument();
    expect(screen.getByText("正在核对文献记录。")).toBeVisible();
  });

  it("opens independent guidance from send options while the main composer is locked", () => {
    render(<WorkspaceShell project={project} locale="zh-CN" guidanceAvailable activeConversationId="conversation" activeRunId="run" runStarted />);
    expect(screen.queryByRole("region", { name: "运行中指导" })).not.toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: /描述研究目标/ })).toBeDisabled();
    fireEvent.click(screen.getByRole("button", { name: "发送选项" }));
    fireEvent.click(screen.getByRole("menuitem", { name: /追加指导/ }));
    expect(screen.getByRole("dialog", { name: "追加指导" })).toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: "追加指导" })).toBeEnabled();
  });

  it("keeps guidance out of Plan mode", () => {
    render(<WorkspaceShell project={project} locale="zh-CN" guidanceAvailable agentMode="plan" activeConversationId="conversation" activeRunId="run" runStarted />);
    fireEvent.click(screen.getByRole("button", { name: "发送选项" }));
    expect(screen.queryByRole("menuitem", { name: /追加指导/ })).not.toBeInTheDocument();
  });

  it("keeps uncertain guidance identities separate when visiting another conversation", async () => {
    vi.spyOn(guidanceApi, "agentV4ListGuidance").mockResolvedValue([]);
    vi.spyOn(guidanceApi, "onAgentV4Event").mockResolvedValue(() => undefined);
    const submit = vi.spyOn(guidanceApi, "agentV4SubmitGuidance").mockRejectedValue(new Error("connection lost"));
    const props = { project, locale: "zh-CN" as const, guidanceAvailable: true, runStarted: true };
    const view = render(<WorkspaceShell {...props} activeConversationId="first" activeRunId="run-first" />);
    const open = () => {
      fireEvent.click(screen.getByRole("button", { name: "发送选项" }));
      fireEvent.click(screen.getByRole("menuitem", { name: /追加指导/ }));
    };
    open();
    fireEvent.change(screen.getByRole("textbox", { name: "追加指导" }), { target: { value: "第一会话指导" } });
    fireEvent.click(screen.getByRole("button", { name: "发送指导" }));
    await screen.findByRole("alert");
    const original = submit.mock.calls[0][0];
    view.rerender(<WorkspaceShell {...props} activeConversationId="second" activeRunId="run-second" />);
    open();
    expect(screen.getByRole("textbox", { name: "追加指导" })).toHaveValue("");
    view.rerender(<WorkspaceShell {...props} activeConversationId="first" activeRunId="run-first" />);
    open();
    expect(screen.getByRole("textbox", { name: "追加指导" })).toHaveValue("第一会话指导");
    fireEvent.click(screen.getByRole("button", { name: "发送指导" }));
    await waitFor(() => expect(submit).toHaveBeenCalledTimes(2));
    expect(submit.mock.calls[1][0]).toEqual(original);
  });

  it.each([false, true, "same text"] as const)("clears only the exact composer text accepted as guidance (edited=%s)", async (edited) => {
    vi.spyOn(guidanceApi, "agentV4ListGuidance").mockResolvedValue([]);
    vi.spyOn(guidanceApi, "onAgentV4Event").mockResolvedValue(() => undefined);
    vi.spyOn(guidanceApi, "agentV4SubmitGuidance").mockImplementation(async (request) => ({ ...request, ordinal: 1, accepted_at: "2026-09-14T00:00:00Z", consumed_at: null }));
    const store = new ComposerDraftStore();
    const props = { project, locale: "zh-CN" as const, guidanceAvailable: true, activeConversationId: "conversation", composerDraftStore: store };
    const view = render(<WorkspaceShell {...props} />);
    const composer = screen.getByRole("textbox", { name: /描述研究目标/ });
    await waitFor(() => expect(composer).toBeEnabled());
    fireEvent.change(composer, { target: { value: "保留原始研究请求" } });
    view.rerender(<WorkspaceShell {...props} activeRunId="run" runStarted />);
    fireEvent.click(screen.getByRole("button", { name: "发送选项" }));
    fireEvent.click(screen.getByRole("menuitem", { name: /追加指导/ }));
    if (edited === true) fireEvent.change(screen.getByRole("textbox", { name: "追加指导" }), { target: { value: "另一个补充要求" } });
    if (edited === "same text") {
      act(() => { store.update({ projectId: project.id, conversationId: "conversation" }, "另一次编辑"); store.update({ projectId: project.id, conversationId: "conversation" }, "保留原始研究请求"); });
    }
    fireEvent.click(screen.getByRole("button", { name: "发送指导" }));
    await waitFor(() => expect(guidanceApi.agentV4SubmitGuidance).toHaveBeenCalledOnce());
    await waitFor(() => expect(screen.getByRole("textbox", { name: "追加指导" })).toHaveValue(""));
    expect(composer).toHaveValue(edited ? "保留原始研究请求" : "");
  });

  it("reopens the latest ordinary run's guidance history after the run has finished", () => {
    const plan = { schema_version: 4 as const, objective: "ordinary run", steps: ["inspect"], completion_criteria: ["report"], requested_capabilities: [] };
    render(<WorkspaceShell project={project} locale="zh-CN" guidanceAvailable activeConversationId="conversation"
      v4Plan={{ run_id: "finished-run", status: "completed", plan, plan_hash: "hash", compute_selection: null, approval_hash: null, session_mode: "agent" }} />);
    fireEvent.click(screen.getByRole("button", { name: "发送选项" }));
    fireEvent.click(screen.getByRole("menuitem", { name: /追加指导/ }));
    expect(screen.queryByRole("textbox", { name: "追加指导" })).not.toBeInTheDocument();
    expect(screen.getByText("当前运行暂不接受新的指导，可查看已接收历史。")).toBeInTheDocument();
  });

  it("pretty prints JSON results and reports displayed line counts", () => {
    const base = { schema_version: 4 as const, run_id: "json", project_id: project.id, conversation_id: "c", previous_hash: "", event_hash: "h", occurred_at: "2026-09-10T00:00:00Z" };
    render(<WorkspaceShell project={project} locale="zh-CN" runStarted activeRunId="json" agentRunEventsV4={[
      { ...base, sequence: 1, event: { kind: "tool_requested", call: { call_id: "s", tool_id: "search_skills", arguments: { query: "literature" } } } },
      { ...base, sequence: 2, event: { kind: "tool_finished", outcome: { call_id: "s", tool_id: "search_skills", succeeded: true, model_content: '{"results":["literature-review"]}', data: null, provenance: [] } } },
    ]} />);
    const trace = screen.getByText("search_skills").closest("details")!;
    fireEvent.click(trace.querySelector("summary")!);
    expect(trace.querySelector("pre")?.textContent).toBe(JSON.stringify({ results: ["literature-review"] }, null, 2));
    expect(trace.querySelector("summary")).toHaveTextContent("5 行");
  });

  it("shows the ready card only for a pending approval plan", () => {
    const plan = { schema_version: 4 as const, objective: "用户请求的计划", steps: ["检查"], completion_criteria: ["核验"], requested_capabilities: [] };
    const props = { project, locale: "zh-CN" as const, agentMode: "plan" as const };
    const summary = { run_id: "plan-run", status: "awaiting_approval", plan, plan_hash: "h", compute_selection: null, approval_hash: "a", session_mode: "plan" as const };
    const { rerender } = render(<WorkspaceShell {...props} v4Plan={summary} />);
    expect(screen.getByText(/计划已生成/)).toBeVisible();
    rerender(<WorkspaceShell {...props} v4Plan={{ ...summary, status: "cancelled" }} />);
    expect(screen.queryByText(/计划已生成/)).not.toBeInTheDocument();
  });

  it.each(["zh-CN", "en-US"] as const)("keeps bookkeeping out of the conversation and explains attention in %s", (locale) => {
    const base = { schema_version: 4 as const, run_id: "run-attention", project_id: project.id, conversation_id: "conversation-1", occurred_at: "2026-09-10T00:00:00Z", previous_hash: "", event_hash: "hash" };
    render(<WorkspaceShell project={project} locale={locale} agentRunEventsV4={[
      { ...base, sequence: 1, event: { kind: "run_created", mode: "execute" } },
      { ...base, sequence: 2, event: { kind: "context_archived", archive: { archive_id: "private-archive", through_sequence: 1, size_bytes: 100, sha256: "hash" } } },
      { ...base, sequence: 3, event: { kind: "context_checkpointed", checkpoint: { schema_version: 4, through_sequence: 2, completion_criteria: [], unresolved_errors: [], recent_steps: [], scientific_state: {} } } },
      { ...base, sequence: 4, event: { kind: "run_needs_attention", message: "请配置可用的文献检索工具。" } },
    ]} />);
    const timeline = screen.getByRole("region", { name: locale === "zh-CN" ? "工具调用详情" : "Tool call details" });
    expect(screen.getByRole("region", { name: locale === "zh-CN" ? "分析对话" : "Analysis conversation" })).toHaveTextContent(locale === "zh-CN" ? "需要处理" : "Needs attention");
    expect(screen.getByText("请配置可用的文献检索工具。")).toBeVisible();
    expect(timeline).not.toHaveTextContent(/context_archived|context_checkpointed|run_needs_attention|run_created/);
    expect(screen.getByText("context_archived")).not.toBeVisible();
    expect(screen.queryByText("private-archive")).not.toBeInTheDocument();
    const diagnostics = screen.getByText(locale === "zh-CN" ? "诊断记录" : "Diagnostic records").closest("details")!;
    const recordFold = diagnostics.parentElement!.closest("details")!;
    recordFold.open = true;
    diagnostics.open = true;
    expect(screen.getByText("context_archived")).toBeVisible();
  });

  it("collapses completed progress while keeping the final answer visible", () => {
    const base = { schema_version: 4 as const, run_id: "run-visible", project_id: project.id, conversation_id: "conversation-1", previous_hash: "", event_hash: "hash" };
    render(<WorkspaceShell project={project} locale="zh-CN"
      messages={[
        { id: "request", role: "user", markdown: "检查数据", created_at: "2026-09-10T00:00:00Z" },
        { id: "answer", role: "assistant", markdown: "已检查 **3 个样本**。", created_at: "2026-09-10T00:00:05Z" },
      ]}
      agentRunEventsV4={[
        { ...base, sequence: 1, occurred_at: "2026-09-10T00:00:01Z", event: { kind: "run_created", mode: "execute" } },
        { ...base, sequence: 2, occurred_at: "2026-09-10T00:00:02Z", event: { kind: "model_text", text: "正在核对样本。" } },
        { ...base, sequence: 3, occurred_at: "2026-09-10T00:00:03Z", event: { kind: "run_completed" } },
      ]} />);
    expect(screen.getByText("执行过程").closest("details")).not.toHaveAttribute("open");
    expect(screen.getByText("正在核对样本。")).not.toBeVisible();
    expect(screen.getByText("3 个样本")).toBeVisible();
    expect(screen.getByText("正在核对样本。").compareDocumentPosition(screen.getByText("3 个样本")) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  });

  it("cancels saved recovery exclusively and hides actions after cancellation", async () => {
    let release: (() => void) | undefined;
    const cancel = vi.fn(() => new Promise<void>((resolve) => { release = resolve; }));
    const resume = vi.fn();
    const event = { schema_version: 4 as const, run_id: "receipt-run", project_id: project.id, conversation_id: "c", sequence: 1, occurred_at: "2026-09-09T00:00:00Z", previous_hash: "", event_hash: "hash", event: { kind: "runtime_recovery_available" as const, call_ids: ["cell"] } };
    const props = { project, locale: "zh-CN" as const, runStarted: true, activeRunId: event.run_id, onResumeAgentRunV4: resume, onCancelRuntimeRecoveryV4: cancel };
    const { rerender } = render(<WorkspaceShell {...props} agentRunEventsV4={[event]} />);
    fireEvent.click(screen.getByRole("button", { name: "取消此运行" }));
    fireEvent.click(screen.getByRole("button", { name: "取消中…" }));
    fireEvent.click(screen.getByRole("button", { name: "恢复已保存结果" }));
    expect(cancel).toHaveBeenCalledTimes(1);
    expect(cancel).toHaveBeenCalledWith(event.run_id);
    expect(resume).not.toHaveBeenCalled();
    release?.();
    await waitFor(() => expect(screen.getByRole("button", { name: "取消此运行" })).toBeEnabled());
    rerender(<WorkspaceShell {...props} agentRunEventsV4={[event, { ...event, sequence: 2, event: { kind: "run_cancelled" } }]} />);
    expect(screen.queryByRole("button", { name: "取消此运行" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "恢复已保存结果" })).not.toBeInTheDocument();
  });
  it("keeps a rejected recovery cancellation retryable without transport details", async () => {
    const cancel = vi.fn().mockRejectedValue(new Error("private transport"));
    const event = { schema_version: 4 as const, run_id: "receipt-run", project_id: project.id, conversation_id: "c", sequence: 1, occurred_at: "2026-09-09T00:00:00Z", previous_hash: "", event_hash: "hash", event: { kind: "runtime_recovery_available" as const, call_ids: ["cell"] } };
    render(<WorkspaceShell project={project} locale="zh-CN" agentRunEventsV4={[event]} onCancelRuntimeRecoveryV4={cancel} />);
    fireEvent.click(screen.getByRole("button", { name: "取消此运行" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("取消失败");
    expect(screen.getByRole("button", { name: "取消此运行" })).toBeEnabled();
    expect(screen.queryByText("private transport")).not.toBeInTheDocument();
  });
  it("resumes durable computation receipts once and hides the action after results are recorded", async () => {
    let finish: (() => void) | undefined;
    const resume = vi.fn(() => new Promise<void>((resolve) => { finish = resolve; }));
    const event = { schema_version: 4 as const, run_id: "receipt-run", project_id: project.id, conversation_id: "c", sequence: 1, occurred_at: "2026-09-09T00:00:00Z", previous_hash: "", event_hash: "hash", event: { kind: "runtime_recovery_available" as const, call_ids: ["cell"] } };
    const props = { project, locale: "zh-CN" as const, runStarted: true, activeRunId: event.run_id, onResumeAgentRunV4: resume, onCancelRuntimeRecoveryV4: vi.fn() };
    const { rerender } = render(<WorkspaceShell {...props} agentRunEventsV4={[event]} />);
    fireEvent.click(screen.getByRole("button", { name: "恢复已保存结果" }));
    expect(screen.getByRole("button", { name: "恢复中…" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "取消此运行" })).toBeDisabled();
    expect(resume).toHaveBeenCalledTimes(1);
    expect(resume).toHaveBeenCalledWith(event.run_id);
    finish?.();
    await waitFor(() => expect(screen.getByRole("button", { name: "恢复已保存结果" })).toBeEnabled());
    rerender(<WorkspaceShell {...props} agentRunEventsV4={[event, { ...event, sequence: 2, event: { kind: "tool_finished", outcome: { call_id: "cell", tool_id: "runtime.execute", succeeded: true, model_content: "result", data: {}, provenance: [] } } }]} />);
    expect(screen.queryByRole("button", { name: "恢复已保存结果" })).not.toBeInTheDocument();
  });
  it("switches Send to a stop square and restores it after cancellation", () => {
    const cancel = vi.fn();
    const props = { project, locale: "zh-CN" as const, onCancelRun: cancel };
    const { rerender } = render(<WorkspaceShell {...props} />);
    expect(screen.getByRole("button", { name: "发送" })).toHaveTextContent("发送");
    rerender(<WorkspaceShell {...props} runStarted activeRunId="run-stop" composerBusy />);
    const stop = screen.getByRole("button", { name: "终止运行" });
    expect(stop.closest(".composer")).toBeInTheDocument();
    expect(stop.querySelector(".lucide-square")).toBeInTheDocument();
    expect(stop).toBeEnabled();
    expect(screen.queryByRole("region", { name: "远程 Agent 运行控制" })).not.toBeInTheDocument();
    fireEvent.click(stop);
    expect(cancel).toHaveBeenCalledTimes(1);
    rerender(<WorkspaceShell {...props} runStarted activeRunId="run-stop" runStopping />);
    const stopping = screen.getByRole("button", { name: "终止中…" });
    expect(stopping).toBeDisabled();
    fireEvent.click(stopping);
    expect(cancel).toHaveBeenCalledTimes(1);
    rerender(<WorkspaceShell {...props} runStarted activeRunId="run-stop" agentRunEventsV4={[{
      schema_version: 4, run_id: "run-stop", project_id: project.id, conversation_id: "c", sequence: 1, occurred_at: "2026-09-08T00:00:00Z", previous_hash: "", event_hash: "hash", event: { kind: "run_cancelled" },
    }]} />);
    expect(screen.queryByRole("button", { name: "终止运行" })).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "发送" })).toHaveTextContent("发送");
  });

  it("interleaves progress and individual read, write, edit calls even inside a batch", () => {
    const base = { schema_version: 4 as const, run_id: "run-compact", project_id: project.id, conversation_id: "conversation-1", previous_hash: "", event_hash: "hash" };
    const longOutput = `${"a".repeat(900)}\r\nlast line\r\n`;
    render(<WorkspaceShell project={project} locale="en-US" runStarted activeRunId={base.run_id} agentRunEventsV4={[
      { ...base, sequence: 1, occurred_at: "2026-08-17T00:00:00Z", event: { kind: "cycle_started", cycle_id: 1 } },
      { ...base, sequence: 2, occurred_at: "2026-08-17T00:00:01Z", event: { kind: "model_text", text: "Inspect the file first." } },
      { ...base, sequence: 3, occurred_at: "2026-08-17T00:00:02Z", event: { kind: "tool_batch_started", batch_id: 1, cycle_id: 1, phase: "executing", tool_names: ["project.read", "write", "edit"], call_ids: ["r", "w", "e"] } },
      { ...base, sequence: 4, occurred_at: "2026-08-17T00:00:03Z", event: { kind: "tool_requested", call: { call_id: "r", tool_id: "project.read", arguments: { path: "C:\\data\\input.txt" } } } },
      { ...base, sequence: 5, occurred_at: "2026-08-17T00:00:04Z", event: { kind: "tool_finished", outcome: { call_id: "r", tool_id: "project.read", succeeded: true, model_content: longOutput, data: null, provenance: [] } } },
      { ...base, sequence: 6, occurred_at: "2026-08-17T00:00:05Z", event: { kind: "model_text", text: "Now save and revise." } },
      { ...base, sequence: 7, occurred_at: "2026-08-17T00:00:06Z", event: { kind: "tool_requested", call: { call_id: "w", tool_id: "write", arguments: { file_path: "/Users/research/output.txt" } } } },
      { ...base, sequence: 8, occurred_at: "2026-08-17T00:00:07Z", event: { kind: "tool_requested", call: { call_id: "e", tool_id: "edit", arguments: { path: "C:\\data\\output.txt" } } } },
    ]} />);
    const timeline = screen.getByRole("region", { name: "Tool call details" });
    expect(Array.from(timeline.children).map((row) => row.querySelector(".v4-progress-preview")?.textContent ?? row.querySelector(".markdown-content")?.textContent ?? row.querySelector("strong")?.textContent ?? row.textContent)).toEqual(["Inspect the file first.", "read", "Now save and revise.", "write", "edit"]);
    expect(timeline.closest("details")).toHaveClass("agent-run-fold");
    const read = within(timeline).getByText("read").closest("details")!;
    expect(read).not.toHaveAttribute("open");
    expect(read.querySelector("summary")).toHaveTextContent("1s · 2 lines");
    fireEvent.click(read.querySelector("summary")!);
    expect(read).toHaveAttribute("open");
    expect(read.querySelector("pre")?.textContent).toBe(longOutput);
    expect(within(timeline).getByText("/Users/research/output.txt")).toBeInTheDocument();
  });

  it("preserves reading position on new output and returns to the latest on demand", async () => {
    const props = { project, locale: "en-US" as const, onSend: vi.fn() };
    const { container, rerender } = render(<WorkspaceShell {...props} streamingAssistant="First" />);
    const stream = container.querySelector(".message-stream") as HTMLElement;
    Object.defineProperties(stream, { scrollHeight: { configurable: true, value: 1200 }, clientHeight: { configurable: true, value: 400 } });
    stream.scrollTop = 100;
    fireEvent.scroll(stream);
    rerender(<WorkspaceShell {...props} streamingAssistant="First and second" />);
    await new Promise((resolve) => setTimeout(resolve, 30));
    expect(stream.scrollTop).toBe(100);
    fireEvent.click(screen.getByRole("button", { name: /Back to latest/ }));
    expect(stream.scrollTop).toBe(1200);
    expect(screen.queryByRole("button", { name: /Back to latest/ })).not.toBeInTheDocument();
  });

  it("renders public progress without the obsolete phase dashboard or private reasoning", () => {
    const base = { schema_version: 4 as const, run_id: "run-guided", project_id: project.id, conversation_id: "conversation-1", previous_hash: "", event_hash: "hash" };
    const events = [
      { ...base, sequence: 1, occurred_at: "2026-08-17T00:00:00Z", event: Object.assign({ kind: "model_text" as const, text: "公开摘要：先检查输入。" }, { raw_reasoning: "private reasoning must never render" }) },
      { ...base, sequence: 2, occurred_at: "2026-08-17T00:00:01Z", event: { kind: "task_shape_selected" as const, task_shape: "multi_step" as const, source: "host" as const, reason: "needs verification" } },
      { ...base, sequence: 3, occurred_at: "2026-08-17T00:00:02Z", event: { kind: "phase_changed" as const, phase: "organizing" as const } },
      { ...base, sequence: 4, occurred_at: "2026-08-17T00:00:03Z", event: { kind: "task_list_updated" as const, revision: 2, change_summary: "分解为可验证步骤", tasks: [
        { id: "task-1", title: "检查输入矩阵", status: "completed" as const },
        { id: "task-2", title: "评估批次效应", status: "in_progress" as const },
        { id: "task-3", title: "等待参考注释", status: "blocked" as const, blocked_reason: "缺少注释文件" },
      ] } },
      { ...base, sequence: 5, occurred_at: "2026-08-17T00:00:04Z", event: { kind: "tool_requested" as const, call: { call_id: "update-1", tool_id: "agent.update_tasks", arguments: { tasks: ["private task payload"] } } } },
      { ...base, sequence: 6, occurred_at: "2026-08-17T00:00:05Z", event: { kind: "tool_requested" as const, call: { call_id: "route-1", tool_id: "agent.route_request", arguments: { route: "adaptive" } } } },
      { ...base, sequence: 7, occurred_at: "2026-08-17T00:00:06Z", event: { kind: "tool_batch_started" as const, batch_id: 2, cycle_id: 1, phase: "organizing" as const, tool_names: ["agent.update_tasks"], call_ids: ["update-1"] } },
      { ...base, sequence: 8, occurred_at: "2026-08-17T00:00:07Z", event: { kind: "tool_batch_finished" as const, batch_id: 2, cycle_id: 1, phase: "organizing" as const, tool_names: ["agent.update_tasks"], call_ids: ["update-1"], duration_ms: 0, succeeded: 1, failed: 0 } },
    ];
    render(<WorkspaceShell project={project} locale="zh-CN" runStarted activeRunId="run-guided" agentRunEventsV4={events} />);

    expect(screen.queryByRole("region", { name: "Agent 阶段轨迹" })).not.toBeInTheDocument();
    expect(screen.queryByRole("region", { name: "任务列表" })).not.toBeInTheDocument();
    expect(screen.getByText("公开摘要：先检查输入。")).toBeVisible();
    expect(screen.getByText("PROGRESS")).toBeVisible();
    expect(screen.queryByText("private reasoning must never render")).not.toBeInTheDocument();
    expect(screen.queryByText("agent.update_tasks")).not.toBeInTheDocument();
    expect(screen.getByText("task_list_updated")).not.toBeVisible();

  });

  it("summarizes tool batches by phase and model cycle while keeping input decisions in the same run", () => {
    const onAnswer = vi.fn();
    const base = { schema_version: 4 as const, run_id: "run-batch", project_id: project.id, conversation_id: "conversation-1", previous_hash: "", event_hash: "hash" };
    render(<WorkspaceShell project={project} locale="en-US" runStarted activeRunId="run-batch" onAnswerAgentQuestionV4={onAnswer} agentRunEventsV4={[
      { ...base, sequence: 1, occurred_at: "2026-08-17T00:00:00Z", event: { kind: "task_shape_selected" as const, task_shape: "multi_step" as const, source: "model" as const, reason: "requires verification" } },
      { ...base, sequence: 2, occurred_at: "2026-08-17T00:00:01Z", event: { kind: "cycle_started" as const, cycle_id: 3 } },
      { ...base, sequence: 3, occurred_at: "2026-08-17T00:00:02Z", event: { kind: "tool_batch_started" as const, batch_id: 8, cycle_id: 3, phase: "executing" as const, tool_names: ["project.read", "runtime.execute"], call_ids: ["call-1", "call-2"] } },
      { ...base, sequence: 4, occurred_at: "2026-08-17T00:00:03Z", event: { kind: "tool_batch_finished" as const, batch_id: 8, cycle_id: 3, phase: "executing" as const, tool_names: ["project.read", "runtime.execute"], call_ids: ["call-1", "call-2"], duration_ms: 1200, succeeded: 2, failed: 0 } },
      { ...base, sequence: 5, occurred_at: "2026-08-17T00:00:04Z", event: { kind: "input_requested" as const, question_id: "species", question: "Which species?" } },
    ]} />);

    const fold = screen.getByText("Processing").closest("details")!;
    fold.open = false;
    expect(screen.getByText("Which species?")).toBeVisible();
    expect(screen.getByText("Which species?").closest(".agent-run-fold")).toBeNull();
    expect(screen.getByText("Which species?")).toBeInTheDocument();
    fireEvent.change(screen.getByRole("textbox", { name: "Answer V4 question" }), { target: { value: "human" } });
    fireEvent.click(screen.getByRole("button", { name: "Answer and resume" }));
    expect(onAnswer).toHaveBeenCalledWith("run-batch", "species", "human");
  });

  it("keeps fast-path runs compact and omits an empty task panel", () => {
    const base = { schema_version: 4 as const, run_id: "run-fast", project_id: project.id, conversation_id: "conversation-1", previous_hash: "", event_hash: "hash" };
    render(<WorkspaceShell project={project} locale="en-US" runStarted activeRunId="run-fast" agentRunEventsV4={[
      { ...base, sequence: 1, occurred_at: "2026-08-17T00:00:00Z", event: { kind: "task_shape_selected" as const, task_shape: "fast" as const, source: "host" as const, reason: "one bounded operation" } },
      { ...base, sequence: 2, occurred_at: "2026-08-17T00:00:01Z", event: { kind: "task_list_updated" as const, revision: 1, change_summary: "no tasks needed", tasks: [] } },
    ]} />);

    expect(screen.queryByRole("region", { name: "Agent guided trajectory" })).not.toBeInTheDocument();
    expect(screen.queryByRole("region", { name: "Task list" })).not.toBeInTheDocument();
  });

  it("renders the V4 hash-chained trajectory", () => {
    render(<WorkspaceShell project={project} locale="zh-CN" runStarted activeRunId="run-v4" agentRunEventsV4={[{
      schema_version: 4, run_id: "run-v4", project_id: project.id, conversation_id: "conversation-1", sequence: 1,
      occurred_at: "2026-08-17T00:00:00Z", previous_hash: "", event_hash: "a".repeat(64), event: { kind: "run_created", mode: "plan" },
    }]} />);
    expect(screen.getByText("执行过程")).toBeInTheDocument();
    expect(screen.getByText("Agent 正在处理任务")).toBeInTheDocument();
    expect(screen.queryByText("规划启动")).not.toBeInTheDocument();
    expect(screen.getByText("run_created").closest("details")).not.toHaveAttribute("open");
  });
  it("warns when an active run has been silent for 90 seconds without hiding stop", () => {
    const onCancel = vi.fn();
    const lastActivity = new Date(Date.now() - 90_001).toISOString();
    render(<WorkspaceShell project={project} locale="zh-CN" runStarted activeRunId="run-stalled" activeRunLastActivityAt={lastActivity} onCancelRun={onCancel} agentRunEventsV4={[{
      schema_version: 4, run_id: "run-stalled", project_id: project.id, conversation_id: "conversation-1", sequence: 1,
      occurred_at: lastActivity, previous_hash: "", event_hash: "a".repeat(64), event: { kind: "run_created", mode: "execute" },
    }]} />);

    expect(screen.getByRole("status")).toHaveTextContent("超过 90 秒未收到模型数据或 Agent 事件");
    expect(screen.getByRole("button", { name: "终止运行" })).toBeInTheDocument();
  });
  it("identifies a silent runtime tool as an unconfirmed execution rather than a stalled model", () => {
    const lastActivity = new Date(Date.now() - 151_000).toISOString();
    const base = { schema_version: 4 as const, run_id: "run-long-tool", project_id: project.id, conversation_id: "conversation-1", occurred_at: lastActivity, previous_hash: "", event_hash: "a".repeat(64) };
    render(<WorkspaceShell project={project} locale="zh-CN" runStarted activeRunId={base.run_id} activeRunLastActivityAt={lastActivity} onCancelRun={vi.fn()} agentRunEventsV4={[
      { ...base, sequence: 1, event: { kind: "tool_requested" as const, call: { call_id: "cell-1", tool_id: "runtime.execute", arguments: { language: "python", code: "print(1)" } } } },
      { ...base, sequence: 2, event: { kind: "tool_dispatch_started" as const, call_id: "cell-1", tool_id: "runtime.execute", effect: "runtime", idempotency_key: "cell-1" } },
    ]} />);

    expect(screen.getByText("执行过程").closest("summary")).toHaveTextContent("正在运行工具");
    expect(screen.getByRole("status", { name: "当前工具活动" })).toHaveTextContent("execute");
    expect(screen.getByText(/工具 runtime.execute 已运行超过 90 秒/)).toHaveTextContent("无法确认计算进度");
    expect(screen.getByText(/工具 runtime.execute 已运行超过 90 秒/)).toHaveTextContent("远端计算是否停止需核实");
    expect(screen.queryByText(/未收到模型数据或 Agent 事件/)).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "终止运行" })).toBeInTheDocument();
  });
  it("keeps a recent runtime dispatch in the tool phase without a silence warning", () => {
    const started = new Date(Date.now() - 30_000).toISOString();
    render(<WorkspaceShell project={project} locale="en-US" runStarted activeRunId="run-recent-tool" activeRunLastActivityAt={started} agentRunEventsV4={[
      { schema_version: 4, run_id: "run-recent-tool", project_id: project.id, conversation_id: "conversation-1", sequence: 1, occurred_at: started, previous_hash: "", event_hash: "a".repeat(64), event: { kind: "tool_dispatch_started", call_id: "cell-1", tool_id: "runtime.execute", effect: "runtime", idempotency_key: "cell-1" } },
    ]} />);

    expect(screen.getByText("Processing").closest("summary")).toHaveTextContent("Running tool");
    expect(screen.queryByText(/has run for over 90 seconds/)).not.toBeInTheDocument();
  });
  it("shows a requested tool as pending dispatch until execution starts", () => {
    const now = new Date().toISOString();
    render(<WorkspaceShell project={project} locale="zh-CN" runStarted activeRunId="run-requested-tool" activeRunLastActivityAt={now} agentRunEventsV4={[
      { schema_version: 4, run_id: "run-requested-tool", project_id: project.id, conversation_id: "conversation-1", sequence: 1, occurred_at: now, previous_hash: "", event_hash: "a".repeat(64), event: { kind: "tool_requested", call: { call_id: "cell-1", tool_id: "runtime.execute", arguments: { language: "python", code: "print(1)" } } } },
    ]} />);
    expect(screen.getByText("执行过程").closest("summary")).toHaveTextContent("准备执行工具");
    expect(screen.getByRole("status", { name: "当前工具活动" })).toHaveTextContent("准备执行工具");
  });
  it("does not claim a silent requested tool was dispatched", () => {
    const old = new Date(Date.now() - 100_000).toISOString();
    render(<WorkspaceShell project={project} locale="zh-CN" runStarted activeRunId="run-request-pending" activeRunLastActivityAt={old} agentRunEventsV4={[
      { schema_version: 4, run_id: "run-request-pending", project_id: project.id, conversation_id: "conversation-1", sequence: 1, occurred_at: old, previous_hash: "", event_hash: "a".repeat(64), event: { kind: "tool_requested", call: { call_id: "cell-1", tool_id: "runtime.execute", arguments: { language: "python", code: "print(1)" } } } },
    ]} />);
    expect(screen.getByText(/工具 runtime.execute 请求已等待超过 90 秒/)).toHaveTextContent("尚未收到派发或结果事件");
    expect(screen.queryByText(/工具 runtime.execute 已运行超过/)).not.toBeInTheDocument();
  });
  it("recognizes delegation graph start and recent node completion as active tool progress", () => {
    const requestedAt = new Date(Date.now() - 100_000).toISOString();
    const graphStartedAt = new Date(Date.now() - 99_000).toISOString();
    const nodeFinishedAt = new Date(Date.now() - 1_000).toISOString();
    const base = { schema_version: 4 as const, run_id: "run-delegation", project_id: project.id, conversation_id: "conversation-1", previous_hash: "", event_hash: "a".repeat(64) };
    render(<WorkspaceShell project={project} locale="zh-CN" runStarted activeRunId={base.run_id} activeRunLastActivityAt={nodeFinishedAt} agentRunEventsV4={[
      { ...base, sequence: 1, occurred_at: requestedAt, event: { kind: "tool_requested" as const, call: { call_id: "delegate-1", tool_id: "agent.delegate", arguments: { graph: {} } } } },
      { ...base, sequence: 2, occurred_at: graphStartedAt, event: { kind: "delegation_graph_started" as const, call_id: "delegate-1", graph: {} } },
      { ...base, sequence: 3, occurred_at: nodeFinishedAt, event: { kind: "delegation_node_finished" as const, call_id: "delegate-1", outcome: {} } },
    ]} />);

    expect(screen.getByText("执行过程").closest("summary")).toHaveTextContent("正在运行工具");
    expect(screen.getByRole("status", { name: "当前工具活动" })).toHaveTextContent("agent");
    expect(screen.queryByText(/超过 90 秒/)).not.toBeInTheDocument();
  });
  it("uses a restarted delegation graph after an older node result for silence timing", () => {
    const requestedAt = new Date(Date.now() - 200_000).toISOString();
    const firstGraphAt = new Date(Date.now() - 150_000).toISOString();
    const oldNodeAt = new Date(Date.now() - 100_000).toISOString();
    const resumedGraphAt = new Date(Date.now() - 1_000).toISOString();
    const base = { schema_version: 4 as const, run_id: "run-resumed-delegation", project_id: project.id, conversation_id: "conversation-1", previous_hash: "", event_hash: "a".repeat(64) };
    render(<WorkspaceShell project={project} locale="zh-CN" runStarted activeRunId={base.run_id} activeRunLastActivityAt={resumedGraphAt} agentRunEventsV4={[
      { ...base, sequence: 1, occurred_at: requestedAt, event: { kind: "tool_requested" as const, call: { call_id: "delegate-resumed", tool_id: "agent.delegate", arguments: { graph: {} } } } },
      { ...base, sequence: 2, occurred_at: firstGraphAt, event: { kind: "delegation_graph_started" as const, call_id: "delegate-resumed", graph: {} } },
      { ...base, sequence: 3, occurred_at: oldNodeAt, event: { kind: "delegation_node_finished" as const, call_id: "delegate-resumed", outcome: {} } },
      { ...base, sequence: 4, occurred_at: resumedGraphAt, event: { kind: "delegation_graph_started" as const, call_id: "delegate-resumed", graph: {} } },
    ]} />);

    expect(screen.getByText("执行过程").closest("summary")).toHaveTextContent("正在运行工具");
    expect(screen.queryByText(/超过 90 秒/)).not.toBeInTheDocument();
  });
  it("attributes silence to a pending model request after the tool has finished", () => {
    const old = new Date(Date.now() - 180_000).toISOString();
    const recent = new Date(Date.now() - 30_000).toISOString();
    const base = { schema_version: 4 as const, run_id: "run-phase-change", project_id: project.id, conversation_id: "conversation-1", previous_hash: "", event_hash: "a".repeat(64) };
    const events = [
      { ...base, sequence: 1, occurred_at: old, event: { kind: "tool_dispatch_started" as const, call_id: "cell-1", tool_id: "runtime.execute", effect: "runtime", idempotency_key: "cell-1" } },
      { ...base, sequence: 2, occurred_at: old, event: { kind: "tool_finished" as const, outcome: { call_id: "cell-1", tool_id: "runtime.execute", succeeded: true, model_content: "done", data: null, provenance: [] } } },
      { ...base, sequence: 3, occurred_at: recent, event: { kind: "model_request_started" as const, request: modelRequest("after-tool") } },
    ];
    const props = { project, locale: "zh-CN" as const, runStarted: true, activeRunId: base.run_id, activeRunLastActivityAt: old };
    const { rerender } = render(<WorkspaceShell {...props} agentRunEventsV4={events} />);
    expect(screen.getByRole("status", { name: "当前模型活动" })).toHaveTextContent("等待模型响应");
    expect(screen.queryByText(/超过 90 秒/)).not.toBeInTheDocument();

    const stale = new Date(Date.now() - 91_000).toISOString();
    rerender(<WorkspaceShell {...props} agentRunEventsV4={[...events.slice(0, 2), { ...events[2], occurred_at: stale }]} />);
    expect(screen.getByText(/超过 90 秒未收到模型数据/)).toBeInTheDocument();
    expect(screen.queryByText(/工具 runtime.execute 已运行超过/)).not.toBeInTheDocument();
  });
  it("does not treat a historical tool dispatch as the active run phase", () => {
    const old = new Date(Date.now() - 100_000).toISOString();
    const base = { schema_version: 4 as const, project_id: project.id, conversation_id: "conversation-1", sequence: 1, occurred_at: old, previous_hash: "", event_hash: "a".repeat(64) };
    render(<WorkspaceShell project={project} locale="en-US" runStarted activeRunId="current" activeRunLastActivityAt={old} agentRunEventsV4={[
      { ...base, run_id: "historical", event: { kind: "tool_dispatch_started", call_id: "old-cell", tool_id: "runtime.execute", effect: "runtime", idempotency_key: "old-cell" } },
      { ...base, run_id: "current", event: { kind: "model_request_started", request: modelRequest("current-model") } },
    ]} />);
    expect(screen.getByText(/No model data or Agent event/)).toBeInTheDocument();
    expect(screen.queryByText(/runtime.execute has run for over/)).not.toBeInTheDocument();
  });
  it("keeps progress and running tool details folded in chronological execution rows", () => {
    const base = { schema_version: 4 as const, run_id: "run-live-steps", project_id: project.id, conversation_id: "conversation-1", previous_hash: "", event_hash: "hash", occurred_at: "2026-08-17T00:00:00Z" };
    const progress = { ...base, sequence: 1, event: { kind: "model_text" as const, text: "## Checking input\n\n- Source verified" } };
    const requested = { ...base, sequence: 2, event: { kind: "tool_requested" as const, call: { call_id: "read-live", tool_id: "project.read", arguments: { path: "input.tsv" } } } };
    const props = { project, locale: "en-US" as const, runStarted: true, activeRunId: base.run_id };
    const { rerender } = render(<WorkspaceShell {...props} agentRunEventsV4={[progress, requested]} />);
    const timeline = screen.getByRole("region", { name: "Tool call details" });
    expect(within(timeline).getByText(/Checking input/)).toBeVisible();
    expect(within(timeline).queryByRole("heading", { name: "Checking input" })).not.toBeInTheDocument();
    const tool = within(timeline).getByText("read").closest("details")!;
    expect(tool).not.toHaveAttribute("open");
    expect(tool.querySelector("summary")).toHaveTextContent("input.tsv");
    fireEvent.click(within(timeline).getByRole("button", { name: /PROGRESS.*Checking input/ }));
    expect(within(timeline).getByRole("heading", { name: "Checking input" })).toBeVisible();
    rerender(<WorkspaceShell {...props} agentRunEventsV4={[progress, requested, { ...base, sequence: 3, event: { kind: "tool_finished", outcome: { call_id: "read-live", tool_id: "project.read", succeeded: true, model_content: "done", data: null, provenance: [] } } }]} />);
    expect(tool).not.toHaveAttribute("open");
    expect(within(timeline).getByRole("heading", { name: "Checking input" })).toBeVisible();
  });
  it("orders model requests, progress, and tool rows by their durable event sequence", () => {
    const base = { schema_version: 4 as const, run_id: "run-ordered", project_id: project.id, conversation_id: "conversation-1", previous_hash: "", event_hash: "hash", occurred_at: new Date(Date.now() - 2_000).toISOString() };
    render(<WorkspaceShell project={project} locale="en-US" runStarted activeRunId={base.run_id} agentRunEventsV4={[
      { ...base, sequence: 1, event: { kind: "model_request_started", request: modelRequest("ordered-attempt") } },
      { ...base, sequence: 2, event: { kind: "model_text", text: "Checking sources" } },
      { ...base, sequence: 3, event: { kind: "tool_requested", call: { call_id: "ordered-read", tool_id: "project.read", arguments: { path: "source.tsv" } } } },
    ]} />);
    const rows = screen.getByRole("region", { name: "Tool call details" }).querySelectorAll(".v4-timeline-row");
    expect([...rows].map((row) => row.textContent)).toEqual([
      expect.stringContaining("MODEL REQUEST"),
      expect.stringContaining("PROGRESS"),
      expect.stringContaining("read"),
    ]);
  });
  it("shows recent reasoning activity without a false silence warning", () => {
    const lastActivity = new Date(Date.now() - 90_001).toISOString();
    render(<WorkspaceShell project={project} locale="zh-CN"
      runStarted activeRunId="run-reasoning" activeRunLastActivityAt={lastActivity}
      agentModelActivity={{ run_id: "run-reasoning", attempt_id: "attempt-reasoning", phase: "reasoning", received_at: new Date().toISOString() }}
      agentRunEventsV4={[{
        schema_version: 4, run_id: "run-reasoning", project_id: project.id, conversation_id: "conversation-1", sequence: 1,
        occurred_at: lastActivity, previous_hash: "", event_hash: "a".repeat(64), event: { kind: "run_created", mode: "execute" },
      }, {
        schema_version: 4, run_id: "run-reasoning", project_id: project.id, conversation_id: "conversation-1", sequence: 2,
        occurred_at: lastActivity, previous_hash: "a".repeat(64), event_hash: "b".repeat(64), event: { kind: "model_request_started", request: modelRequest("attempt-reasoning") },
      }]} />);
    expect(screen.getByRole("status", { name: "当前模型活动" })).toHaveTextContent("模型正在推理");
    expect(screen.queryByText(/超过 90 秒未收到模型数据或 Agent 事件/)).not.toBeInTheDocument();
  });
  it("shows a real model request row and keeps live thinking folded inside execution details", () => {
    const started = new Date(Date.now() - 3_000).toISOString();
    const base = { schema_version: 4 as const, run_id: "run-thinking", project_id: project.id, conversation_id: "conversation-1", previous_hash: "", event_hash: "hash", occurred_at: started };
    const events = [
      { ...base, sequence: 1, event: { kind: "run_created" as const, mode: "execute" as const } },
      { ...base, sequence: 2, event: { kind: "model_request_started" as const, request: modelRequest("attempt-thinking") } },
    ];
    const props = { project, locale: "zh-CN" as const, runStarted: true, activeRunId: base.run_id, agentRunEventsV4: events };
    const { rerender } = render(<WorkspaceShell {...props} />);
    const waiting = screen.getByRole("status", { name: "当前模型活动" });
    expect(waiting).toHaveTextContent("等待模型响应");
    expect(waiting).toHaveTextContent(/本次请求 3/);
    expect(within(waiting).getByRole("timer")).toHaveAttribute("aria-live", "off");
    expect(waiting.closest("details")).toBeNull();
    const timeline = screen.getByRole("region", { name: "工具调用详情" });
    expect(within(timeline).getByText("MODEL REQUEST")).toBeVisible();
    expect(screen.queryByRole("article", { name: "模型思考" })).not.toBeInTheDocument();

    rerender(<WorkspaceShell {...props} agentReasoningPreview={{ run_id: base.run_id, attempt_id: "attempt-thinking", text: "先核对输入" }} />);
    const thinking = screen.getByRole("article", { name: "模型思考" });
    expect(thinking.closest(".agent-run-fold")).toBeInTheDocument();
    expect(within(thinking).getByText("THINKING")).toBeVisible();
    expect(thinking.querySelector("details")).not.toHaveAttribute("open");
    fireEvent.click(screen.getByText("执行过程"));
    expect(thinking).not.toBeVisible();
    expect(screen.getByRole("status", { name: "当前模型活动" })).toBeVisible();
    fireEvent.click(screen.getByText("执行过程"));
    fireEvent.click(within(thinking).getByText("THINKING"));
    expect(thinking.querySelector("details")).toHaveAttribute("open");
    rerender(<WorkspaceShell {...props} agentReasoningPreview={{ run_id: base.run_id, attempt_id: "attempt-thinking", text: "先核对输入\n再检查 <img src=x onerror=alert(1)>" }} />);
    expect(thinking).toHaveTextContent("再检查 <img src=x onerror=alert(1)>");
    expect(thinking.querySelector("img")).toBeNull();
    fireEvent.click(within(thinking).getByText("THINKING"));
    expect(thinking.querySelector("details")).not.toHaveAttribute("open");
    rerender(<WorkspaceShell {...props} agentReasoningPreview={{ run_id: base.run_id, attempt_id: "attempt-thinking", text: "先核对输入" }} agentModelActivity={{ run_id: base.run_id, attempt_id: "attempt-thinking", phase: "tool_call", received_at: new Date().toISOString() }} />);
    expect(thinking.querySelector("details")).not.toHaveAttribute("open");
    expect(screen.getByRole("status", { name: "当前模型活动" })).toHaveTextContent("模型正在准备工具");
    rerender(<WorkspaceShell {...props} agentReasoningPreview={{ run_id: base.run_id, attempt_id: "stale-attempt", text: "旧推理泄漏" }} />);
    expect(screen.queryByRole("article", { name: "模型思考" })).not.toBeInTheDocument();
  });
  it("shows the active tool and elapsed time outside execution details, but stops working status for approval", () => {
    const started = new Date(Date.now() - 4_000).toISOString();
    const base = { schema_version: 4 as const, run_id: "run-tool", project_id: project.id, conversation_id: "conversation-1", previous_hash: "", event_hash: "hash", occurred_at: started };
    const requested = { ...base, sequence: 1, event: { kind: "tool_requested" as const, call: { call_id: "read-1", tool_id: "project.read", arguments: { path: "input.tsv" } } } };
    const running = { ...base, sequence: 2, event: { kind: "tool_dispatch_started" as const, call_id: "read-1", tool_id: "project.read", effect: "read_only", idempotency_key: "read-1" } };
    const props = { project, locale: "zh-CN" as const, runStarted: true, activeRunId: base.run_id };
    const { rerender } = render(<WorkspaceShell {...props} agentRunEventsV4={[requested, running]} />);
    const status = screen.getByRole("status", { name: "当前工具活动" });
    expect(status).toHaveTextContent("read");
    expect(status).toHaveTextContent(/4/);
    expect(within(status).getByRole("timer")).toHaveAttribute("aria-live", "off");
    expect(status.closest("details")).toBeNull();
    rerender(<WorkspaceShell {...props} agentRunEventsV4={[requested, running, { ...base, sequence: 3, event: { kind: "tool_approval_requested", request: { approval_id: "approval-1", call: requested.event.call, effect: "read_only", reason: "Review", call_hash: "a".repeat(64) } } }]} />);
    expect(screen.queryByRole("status", { name: "当前工具活动" })).not.toBeInTheDocument();
  });
  it("coalesces character-sized V4 model deltas into one completed response", () => {
    const deltas = ["我", "先", "检查", "输入", "目录", "。"];
    render(<WorkspaceShell project={project} locale="zh-CN" runStarted activeRunId="run-v4-text" agentRunEventsV4={deltas.map((text, index) => ({
      schema_version: 4 as const, run_id: "run-v4-text", project_id: project.id, conversation_id: "conversation-1", sequence: index + 1,
      occurred_at: "2026-08-17T00:00:00Z", previous_hash: String(index), event_hash: String(index + 1), event: { kind: "model_text" as const, text },
    }))} />);
    expect(screen.getAllByRole("article", { name: "模型输出" })).toHaveLength(1);
    expect(screen.getByRole("article", { name: "模型输出" })).toHaveTextContent("我先检查输入目录。");
  });
  it("shows the V4 input question and hides the answer form after it is answered", () => {
    const onAnswer = vi.fn();
    const base = { schema_version: 4 as const, run_id: "run-question", project_id: project.id, conversation_id: "conversation-1", previous_hash: "", event_hash: "hash" };
    const requested = { ...base, sequence: 1, occurred_at: "2026-08-17T00:00:01Z", event: { kind: "input_requested" as const, question_id: "species", question: "该数据来自人还是小鼠？" } };
    const { rerender } = render(<WorkspaceShell project={project} locale="zh-CN" runStarted activeRunId="run-question" agentRunEventsV4={[requested]} onAnswerAgentQuestionV4={onAnswer} />);

    expect(screen.getByText("需要补充信息")).toBeInTheDocument();
    expect(screen.getByText("该数据来自人还是小鼠？")).toBeInTheDocument();
    fireEvent.change(screen.getByRole("textbox", { name: "回答 V4 问题" }), { target: { value: "人" } });
    fireEvent.click(screen.getByRole("button", { name: "回答并恢复" }));
    expect(onAnswer).toHaveBeenCalledWith("run-question", "species", "人");

    rerender(<WorkspaceShell project={project} locale="zh-CN" runStarted activeRunId="run-question" agentRunEventsV4={[requested,
      { ...base, sequence: 2, occurred_at: "2026-08-17T00:00:02Z", event: { kind: "user_input_answered" as const, question_id: "species", answer: "人" } },
    ]} onAnswerAgentQuestionV4={onAnswer} />);
    expect(screen.getByText("已提交回答：人")).toBeInTheDocument();
    expect(screen.queryByRole("textbox", { name: "回答 V4 问题" })).not.toBeInTheDocument();
  });
  it("restores a completed V4 run after its persisted user message", () => {
    const base = { schema_version: 4 as const, run_id: "run-history", project_id: project.id, conversation_id: "conversation-1", previous_hash: "", event_hash: "hash" };
    render(<WorkspaceShell project={project} locale="zh-CN"
      messages={[{ id: "message-1", role: "user", markdown: "检查矩阵", created_at: "2026-08-17T00:00:00Z" }]}
      agentRunEventsV4={[
        { ...base, sequence: 1, occurred_at: "2026-08-17T00:00:01Z", event: { kind: "run_created", mode: "execute" } },
        { ...base, sequence: 2, occurred_at: "2026-08-17T00:00:02Z", event: { kind: "model_text", text: "矩阵检查完成。" } },
        { ...base, sequence: 3, occurred_at: "2026-08-17T00:00:03Z", event: { kind: "run_completed" } },
      ]} />);
    expect(screen.getByText("检查矩阵")).toBeInTheDocument();
    expect(screen.getByText(/已完成 · 0 个步骤/)).toBeInTheDocument();
    fireEvent.click(screen.getByText("执行过程"));
    expect(screen.getByRole("article", { name: "模型输出" })).toHaveTextContent("矩阵检查完成。");
  });
  it("hides the failed-run resume banner and treats later events as running", () => {
    const onResume = vi.fn();
    const base = { schema_version: 4 as const, run_id: "run-recover", project_id: project.id, conversation_id: "conversation-1", previous_hash: "", event_hash: "hash" };
    const failed = [
      { ...base, sequence: 1, occurred_at: "2026-08-17T00:00:01Z", event: { kind: "run_created" as const, mode: "execute" as const } },
      { ...base, sequence: 2, occurred_at: "2026-08-17T00:00:02Z", event: { kind: "run_failed" as const, message: "system environment cannot be created or changed" } },
    ];
    const { rerender } = render(<WorkspaceShell project={project} locale="zh-CN" agentRunEventsV4={failed} onResumeAgentRunV4={onResume} />);

    fireEvent.click(screen.getByText("执行过程"));
    expect(screen.queryByRole("button", { name: "继续运行" })).not.toBeInTheDocument();
    expect(screen.queryByText("修正运行条件后可从已验证事件链继续。")).not.toBeInTheDocument();

    rerender(<WorkspaceShell project={project} locale="zh-CN" runStarted activeRunId="run-recover" agentRunEventsV4={[...failed,
      { ...base, sequence: 3, occurred_at: "2026-08-17T00:00:03Z", event: { kind: "tool_dispatch_resolved" as const, call_id: "call-1", resolution: "side_effect_not_observed" as const, evidence: "immutable system ensure" } },
    ]} onResumeAgentRunV4={onResume} />);
    expect(screen.getByText(/运行中 · 0 个步骤/)).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "继续运行" })).not.toBeInTheDocument();
  });
  it("shows all compute choices in chat and keeps full access container-only", () => {
    const onBackendChange = vi.fn();
    const onApprovalPolicyChange = vi.fn();
    render(<WorkspaceShell project={project} locale="zh-CN" onSend={() => true}
      computeBackendId="local" onComputeBackendChange={onBackendChange} onApprovalPolicyChange={onApprovalPolicyChange}
      computeBackends={[
        { descriptor: { schema_version: 4, backend_id: "local", kind: "local", isolation: "process", available: true, supports_python: true, supports_r: false, supports_network_policy: false }, selectable: true, reason: null, python_status: "available", r_status: "unavailable", resolved_image_id: null },
        { descriptor: { schema_version: 4, backend_id: "ssh:server", kind: "ssh", isolation: "process", available: true, supports_python: true, supports_r: false, supports_network_policy: false }, selectable: true, reason: null, python_status: "available", r_status: "unavailable", resolved_image_id: null },
        { descriptor: { schema_version: 4, backend_id: "docker", kind: "docker", isolation: "container", available: true, supports_python: true, supports_r: true, supports_network_policy: true }, selectable: true, reason: null, python_status: "unverified", r_status: "unverified", resolved_image_id: "sha256:abc" },
        { descriptor: { schema_version: 4, backend_id: "podman", kind: "podman", isolation: "container", available: false, supports_python: false, supports_r: false, supports_network_policy: true }, selectable: false, reason: "engine unavailable", python_status: "unavailable", r_status: "unavailable", resolved_image_id: null },
      ]} />);
    fireEvent.click(screen.getByRole("button", { name: "选择计算后端" }));
    expect(screen.getByRole("region", { name: "V4 计算后端" })).toHaveTextContent("本地与 SSH 按需连接；选择配置不代表依赖已安装");
    expect(screen.getByRole("radio", { name: /LOCAL/ })).toBeInTheDocument();
    expect(screen.getByRole("radio", { name: /SSH/ })).toBeInTheDocument();
    expect(screen.getByRole("radio", { name: /DOCKER/ })).toBeInTheDocument();
    expect(screen.getByRole("radio", { name: /PODMAN/ })).toBeDisabled();
    fireEvent.click(screen.getByRole("radio", { name: /DOCKER/ }));
    expect(onBackendChange).toHaveBeenCalledWith("docker");
    fireEvent.click(screen.getByRole("button", { name: "Agent 权限" }));
    expect(screen.getByRole("menuitemradio", { name: /^请求批准/ })).toBeInTheDocument();
    expect(screen.getByRole("menuitemradio", { name: /^帮我批准/ })).toBeInTheDocument();
    expect(screen.getByRole("menuitemradio", { name: /^完全访问权限/ })).toBeDisabled();
  });
  it("enables full access only for an available offline container", () => {
    const onApprovalPolicyChange = vi.fn();
    const onAutonomyModeChange = vi.fn();
    render(<WorkspaceShell project={project} locale="zh-CN" onSend={() => true}
      computeBackendId="docker" onApprovalPolicyChange={onApprovalPolicyChange} onAutonomyModeChange={onAutonomyModeChange}
      computeBackends={[{ descriptor: { schema_version: 4, backend_id: "docker", kind: "docker", isolation: "container", available: true, supports_python: true, supports_r: true, supports_network_policy: true }, selectable: true, reason: null, python_status: "unverified", r_status: "unverified", resolved_image_id: "sha256:abc" }]} />);
    fireEvent.click(screen.getByRole("button", { name: "Agent 权限" }));
    const fullAccess = screen.getByRole("menuitemradio", { name: /^完全访问权限/ });
    expect(fullAccess).toBeEnabled();
    fireEvent.click(fullAccess);
    expect(onApprovalPolicyChange).toHaveBeenCalledWith("full_access");
    expect(onAutonomyModeChange).toHaveBeenCalledWith("full_auto");
  });

  it("defaults the composer to local deletion approval and preserves request approval choice", () => {
    const onApprovalPolicyChange = vi.fn();
    render(<WorkspaceShell project={project} locale="zh-CN" onApprovalPolicyChange={onApprovalPolicyChange} computeBackendId="local"
      computeBackends={[{ descriptor: { schema_version: 4, backend_id: "local", kind: "local", isolation: "process", available: true, supports_python: true, supports_r: false, supports_network_policy: false }, selectable: true, reason: null, python_status: "available", r_status: "unavailable", resolved_image_id: null }]} />);
    fireEvent.click(screen.getByRole("button", { name: "Agent 权限" }));
    const assist = screen.getByRole("menuitemradio", { name: /^帮我批准/ });
    expect(assist).toHaveAttribute("aria-checked", "true");
    expect(assist).toHaveTextContent("自动批准操作，仅检测到本地删除时询问");
    expect(assist).toHaveTextContent("动态代码或第三方工具中的间接删除可能无法识别");
    fireEvent.click(screen.getByRole("menuitemradio", { name: /^请求批准/ }));
    expect(onApprovalPolicyChange).toHaveBeenCalledWith("request_approval");
    fireEvent.click(screen.getByRole("button", { name: "Agent 权限" }));
    fireEvent.click(screen.getByRole("menuitemradio", { name: /^帮我批准/ }));
    expect(onApprovalPolicyChange).toHaveBeenLastCalledWith("auto_approve_except_local_deletion");
  });

  it("labels historical risk based selection without presenting it as the new policy", () => {
    render(<WorkspaceShell project={project} locale="zh-CN" approvalPolicy="risk_based" computeBackendId="local"
      computeBackends={[{ descriptor: { schema_version: 4, backend_id: "local", kind: "local", isolation: "process", available: true, supports_python: true, supports_r: false, supports_network_policy: false }, selectable: true, reason: null, python_status: "available", r_status: "unavailable", resolved_image_id: null }]} />);
    expect(screen.getByText(/Agent 模式：LOCAL · 风险审批/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Agent 权限" }));
    expect(screen.getByRole("menuitemradio", { name: /^帮我批准/ })).toHaveAttribute("aria-checked", "false");
  });

  it("hydrates conversation preferences and sends each switch with the active scope", async () => {
    const loaded = { delegation_enabled: false, auto_review: true, memory_enabled: false };
    const saved = { delegation_enabled: true, auto_review: true, memory_enabled: false };
    const get = vi.spyOn(preferencesApi, "getConversationAgentPreferencesV4").mockResolvedValue(loaded);
    const save = vi.spyOn(preferencesApi, "saveConversationAgentPreferencesV4").mockResolvedValue(saved);
    render(<WorkspaceShell project={project} locale="en-US" activeConversationId="conversation-a" onSend={vi.fn().mockResolvedValue(true)} computeBackendId="local" computeBackends={[{ descriptor: { schema_version: 4, backend_id: "local", kind: "local", isolation: "process", available: true, supports_python: true, supports_r: false, supports_network_policy: false }, selectable: true, reason: null, python_status: "available", r_status: "unavailable", resolved_image_id: null }]} />);

    await waitFor(() => expect(get).toHaveBeenCalledWith(project.id, "conversation-a"));
    fireEvent.click(screen.getByRole("button", { name: "Agent permissions" }));
    expect(screen.getByRole("menuitemcheckbox", { name: /Delegation/ })).toHaveAttribute("aria-checked", "false");
    expect(screen.getByRole("menuitemcheckbox", { name: /Auto-review/ })).toHaveAttribute("aria-checked", "true");
    expect(screen.getByRole("menuitemcheckbox", { name: /Use memory/ })).toHaveAttribute("aria-checked", "false");
    fireEvent.click(screen.getByRole("menuitemcheckbox", { name: /Delegation/ }));

    await waitFor(() => expect(save).toHaveBeenCalledWith(project.id, "conversation-a", saved, expect.any(Number)));
    expect(screen.getByRole("menuitemcheckbox", { name: /Delegation/ })).toHaveAttribute("aria-checked", "true");
  });

  it("offers Fast mode near the model and persists menu choices including reset to inheritance", async () => {
    const get = vi.spyOn(preferencesApi, "getConversationAgentPreferencesV4").mockResolvedValue({ delegation_enabled: true, auto_review: true, memory_enabled: true, fast_mode: null });
    const save = vi.spyOn(preferencesApi, "saveConversationAgentPreferencesV4").mockImplementation(async (_projectId, _conversationId, preferences) => preferences);
    render(<WorkspaceShell project={project} locale="en-US" activeConversationId="conversation-a" activeModelProfile={fastProfile} onSend={vi.fn().mockResolvedValue(true)} computeBackendId="local" computeBackends={[{ descriptor: { schema_version: 4, backend_id: "local", kind: "local", isolation: "process", available: true, supports_python: true, supports_r: false, supports_network_policy: false }, selectable: true, reason: null, python_status: "available", r_status: "unavailable", resolved_image_id: null }]} />);

    await waitFor(() => expect(get).toHaveBeenCalledWith(project.id, "conversation-a"));
    const fastToggle = screen.getByRole("button", { name: "Fast mode" });
    expect(fastToggle).toHaveAttribute("aria-pressed", "false");
    fireEvent.click(fastToggle);
    await waitFor(() => expect(save).toHaveBeenNthCalledWith(1, project.id, "conversation-a", expect.objectContaining({ fast_mode: true }), expect.any(Number)));
    expect(fastToggle).toHaveAttribute("aria-pressed", "true");

    fireEvent.click(screen.getByRole("button", { name: "Agent permissions" }));
    const select = screen.getByRole("combobox", { name: "Fast mode" });
    expect(select).toHaveValue("fast");
    fireEvent.change(select, { target: { value: "standard" } });
    await waitFor(() => expect(save).toHaveBeenNthCalledWith(2, project.id, "conversation-a", expect.objectContaining({ fast_mode: false }), expect.any(Number)));
    fireEvent.change(select, { target: { value: "default" } });
    await waitFor(() => expect(save).toHaveBeenNthCalledWith(3, project.id, "conversation-a", expect.objectContaining({ fast_mode: null }), expect.any(Number)));
  });

  it("keeps a stale enabled Fast override available to turn off after an unsupported model switch", async () => {
    const get = vi.spyOn(preferencesApi, "getConversationAgentPreferencesV4").mockResolvedValue({ delegation_enabled: true, auto_review: true, memory_enabled: true, fast_mode: true });
    const save = vi.spyOn(preferencesApi, "saveConversationAgentPreferencesV4").mockResolvedValue({ delegation_enabled: true, auto_review: true, memory_enabled: true, fast_mode: null });
    const unsupported = { ...fastProfile, base_url: "https://gateway.example/v1" };
    render(<WorkspaceShell project={project} locale="en-US" activeConversationId="conversation-a" activeModelProfile={unsupported} />);

    await waitFor(() => expect(get).toHaveBeenCalled());
    const fastToggle = screen.getByRole("button", { name: "Fast mode" });
    expect(fastToggle).toHaveAttribute("aria-pressed", "true");
    fireEvent.click(fastToggle);
    await waitFor(() => expect(save).toHaveBeenCalledWith(project.id, "conversation-a", expect.objectContaining({ fast_mode: null }), expect.any(Number)));
    expect(screen.queryByRole("button", { name: "Fast mode" })).not.toBeInTheDocument();
  });

  it("overrides a stale profile Fast default with standard, then restores that default from the menu", async () => {
    const get = vi.spyOn(preferencesApi, "getConversationAgentPreferencesV4").mockResolvedValue({ delegation_enabled: true, auto_review: true, memory_enabled: true, fast_mode: null });
    const save = vi.spyOn(preferencesApi, "saveConversationAgentPreferencesV4").mockImplementation(async (_projectId, _conversationId, preferences) => preferences);
    const unsupportedProfile = { ...fastProfile, base_url: "https://gateway.example/v1", fast_mode: true as const };
    render(<WorkspaceShell project={project} locale="en-US" activeConversationId="conversation-a" activeModelProfile={unsupportedProfile} />);

    await waitFor(() => expect(get).toHaveBeenCalled());
    const fastToggle = screen.getByRole("button", { name: "Fast mode" });
    expect(fastToggle).toHaveAttribute("aria-pressed", "true");
    fireEvent.click(fastToggle);
    await waitFor(() => expect(save).toHaveBeenNthCalledWith(1, project.id, "conversation-a", expect.objectContaining({ fast_mode: false }), expect.any(Number)));
    expect(fastToggle).toHaveAttribute("aria-pressed", "false");

    fireEvent.click(screen.getByRole("button", { name: "Agent permissions" }));
    const select = screen.getByRole("combobox", { name: "Fast mode" });
    expect(select).toHaveValue("standard");
    fireEvent.change(select, { target: { value: "default" } });
    await waitFor(() => expect(save).toHaveBeenNthCalledWith(2, project.id, "conversation-a", expect.objectContaining({ fast_mode: null }), expect.any(Number)));
    expect(fastToggle).toHaveAttribute("aria-pressed", "true");
  });

  it("locks Fast mode and Send while its preference save is pending", async () => {
    const request = deferred<{ delegation_enabled: boolean; auto_review: boolean; memory_enabled: boolean; fast_mode: boolean }>();
    vi.spyOn(preferencesApi, "getConversationAgentPreferencesV4").mockResolvedValue({ delegation_enabled: true, auto_review: true, memory_enabled: true, fast_mode: null });
    vi.spyOn(preferencesApi, "saveConversationAgentPreferencesV4").mockReturnValue(request.promise);
    render(<WorkspaceShell project={project} locale="en-US" activeConversationId="conversation-a" activeModelProfile={fastProfile} onSend={vi.fn()} computeBackendId="local" computeBackends={[{ descriptor: { schema_version: 4, backend_id: "local", kind: "local", isolation: "process", available: true, supports_python: true, supports_r: false, supports_network_policy: false }, selectable: true, reason: null, python_status: "available", r_status: "unavailable", resolved_image_id: null }]} />);

    await waitFor(() => expect(screen.getByRole("button", { name: "Fast mode" })).toBeEnabled());
    fireEvent.click(screen.getByRole("button", { name: "Fast mode" }));
    expect(screen.getByRole("button", { name: "Fast mode" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Send" })).toBeDisabled();
    await act(async () => request.resolve({ delegation_enabled: true, auto_review: true, memory_enabled: true, fast_mode: true }));
    await waitFor(() => expect(screen.getByRole("button", { name: "Fast mode" })).toBeEnabled());
  });

  it("keeps an unconfirmed preference save blocked and offers reconciliation without repeating the write", async () => {
    const get = vi.spyOn(preferencesApi, "getConversationAgentPreferencesV4").mockResolvedValue({ delegation_enabled: true, auto_review: true, memory_enabled: true });
    const save = vi.spyOn(preferencesApi, "saveConversationAgentPreferencesV4").mockRejectedValueOnce(new Error("private transport details")).mockResolvedValueOnce({ delegation_enabled: false, auto_review: true, memory_enabled: true });
    render(<WorkspaceShell project={project} locale="en-US" activeConversationId="conversation-a" onSend={vi.fn().mockResolvedValue(true)} />);
    await waitFor(() => expect(get).toHaveBeenCalled());
    fireEvent.click(screen.getByRole("button", { name: "Agent permissions" }));
    const delegation = screen.getByRole("menuitemcheckbox", { name: /Delegation/ });
    fireEvent.click(delegation);

    expect(await screen.findByRole("alert")).toHaveTextContent("preference save is unconfirmed");
    expect(screen.queryByText("private transport details")).not.toBeInTheDocument();
    expect(screen.getByRole("menuitemcheckbox", { name: /Delegation/ })).toHaveAttribute("aria-checked", "false");
    expect(delegation).toBeDisabled();
    expect(screen.getByRole("button", { name: "Reconcile preference save" })).toBeEnabled();
    expect(save).toHaveBeenCalledTimes(1);
  });

  it("locks Send on a preference load failure and keeps a localized retry visible", async () => {
    const get = vi.spyOn(preferencesApi, "getConversationAgentPreferencesV4")
      .mockRejectedValueOnce(new Error("private load details"))
      .mockResolvedValueOnce({ delegation_enabled: true, auto_review: true, memory_enabled: true });
    const onSend = vi.fn();
    render(<WorkspaceShell project={project} locale="zh-CN" activeConversationId="conversation-a" onSend={onSend} computeBackendId="local" computeBackends={[{ descriptor: { schema_version: 4, backend_id: "local", kind: "local", isolation: "process", available: true, supports_python: true, supports_r: false, supports_network_policy: false }, selectable: true, reason: null, python_status: "available", r_status: "unavailable", resolved_image_id: null }]} />);
    fireEvent.change(screen.getByRole("textbox", { name: /描述研究目标/ }), { target: { value: "检查矩阵" } });

    expect(await screen.findByRole("alert")).toHaveTextContent("会话偏好加载失败");
    expect(screen.queryByText("private load details")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "发送" })).toBeDisabled();
    fireEvent.click(screen.getByRole("button", { name: "重试会话偏好" }));
    await waitFor(() => expect(get).toHaveBeenCalledTimes(2));
    await waitFor(() => expect(screen.getByRole("button", { name: "发送" })).toBeEnabled());
  });

  it("locks Send and preference switches during preference persistence", async () => {
    const request = deferred<{ delegation_enabled: boolean; auto_review: boolean; memory_enabled: boolean }>();
    vi.spyOn(preferencesApi, "getConversationAgentPreferencesV4").mockResolvedValue({ delegation_enabled: true, auto_review: true, memory_enabled: true });
    vi.spyOn(preferencesApi, "saveConversationAgentPreferencesV4").mockReturnValue(request.promise);
    const onSend = vi.fn();
    render(<WorkspaceShell project={project} locale="en-US" activeConversationId="conversation-a" onSend={onSend} computeBackendId="local" computeBackends={[{ descriptor: { schema_version: 4, backend_id: "local", kind: "local", isolation: "process", available: true, supports_python: true, supports_r: false, supports_network_policy: false }, selectable: true, reason: null, python_status: "available", r_status: "unavailable", resolved_image_id: null }]} />);
    fireEvent.change(screen.getByRole("textbox", { name: /Describe a research goal/ }), { target: { value: "Inspect the matrix" } });
    await waitFor(() => expect(screen.getByRole("button", { name: "Send" })).toBeEnabled());
    fireEvent.click(screen.getByRole("button", { name: "Agent permissions" }));
    const delegation = screen.getByRole("menuitemcheckbox", { name: /Delegation/ });
    fireEvent.click(delegation);
    expect(screen.getByRole("button", { name: "Send" })).toBeDisabled();
    expect(delegation).toBeDisabled();
    expect(screen.getByRole("status")).toHaveTextContent("Saving conversation preferences");
    fireEvent.click(screen.getByRole("button", { name: "Send" }));
    expect(onSend).not.toHaveBeenCalled();
    await act(async () => request.resolve({ delegation_enabled: false, auto_review: true, memory_enabled: true }));
    await waitFor(() => expect(screen.getByRole("button", { name: "Send" })).toBeEnabled());
  });
  it("keeps ordinary questions in chat until the user explicitly selects Plan mode from Agent controls", async () => {
    const onSend = vi.fn().mockResolvedValue(true);
    render(<WorkspaceShell project={project} locale="zh-CN" onSend={onSend}
      computeBackendId="local" computeBackends={[{ descriptor: { schema_version: 4, backend_id: "local", kind: "local", isolation: "process", available: true, supports_python: true, supports_r: false, supports_network_policy: false }, selectable: true, reason: null, python_status: "available", r_status: "unavailable", resolved_image_id: null }]} />);

    fireEvent.change(screen.getByRole("textbox", { name: /描述研究目标/ }), { target: { value: "这个文件是什么格式？" } });
    fireEvent.click(screen.getByRole("button", { name: "发送" }));
    await waitFor(() => expect(onSend).toHaveBeenCalledWith("这个文件是什么格式？", "chat"));
    expect(screen.queryByRole("region", { name: "V4 计算后端" })).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Agent 权限" }));
    fireEvent.click(screen.getByRole("menuitemcheckbox", { name: "先做计划" }));
    fireEvent.click(screen.getByRole("button", { name: "选择计算后端" }));
    expect(screen.getByRole("region", { name: "V4 计算后端" })).toBeInTheDocument();
    fireEvent.change(screen.getByRole("textbox", { name: /描述研究目标/ }), { target: { value: "执行完整 QC" } });
    fireEvent.click(screen.getByRole("button", { name: "发送" }));
    await waitFor(() => expect(onSend).toHaveBeenLastCalledWith("执行完整 QC", "plan"));
  });
  it("keeps projects, scientific conversation, and context visible together", () => {
    render(<WorkspaceShell project={project} locale="zh-CN" />);

    expect(screen.getByRole("navigation", { name: "项目与会话" })).toBeInTheDocument();
    expect(screen.getByRole("main", { name: "科研对话" })).toBeInTheDocument();
    expect(screen.queryByRole("complementary", { name: "项目上下文" })).not.toBeInTheDocument();
    expect(screen.getAllByText("PBMC 图谱")).toHaveLength(2);
    expect(screen.getByText("Scanpy 质量控制")).toBeInTheDocument();
  });

  it("shows an honest empty artifact catalog", () => {
    render(<WorkspaceShell project={project} locale="zh-CN" />);

    if (!screen.queryByRole("complementary")) fireEvent.click(screen.getByRole("button", { name: "展开侧栏" }));
    if (!screen.queryByRole("menu", { name: "侧栏内容" })) fireEvent.click(screen.getByRole("button", { name: "添加侧栏标签" }));
    fireEvent.click(screen.getByRole("menuitemcheckbox", { name: "Artifacts (0)" }));
    expect(screen.getByText("暂无登记产物")).toBeVisible();
    expect(screen.queryByText("UMAP 聚类概览")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "展开预览" })).not.toBeInTheDocument();
  });

  it("loads the selected remote image into the preview", async () => {
    const onPreviewImage = vi.fn().mockResolvedValue({ relative_path: "results/umap.png", mime_type: "image/png", size_bytes: 1024, sha256: "a".repeat(64), data_url: "data:image/png;base64,iVBORw0KGgo=" });
    render(<WorkspaceShell project={project} locale="en-US" remoteFiles={[{ relative_path: "results/umap.png", directory: false, size_bytes: 1024, modified_unix_seconds: 0 }, { relative_path: "results/markers.csv", directory: false, size_bytes: 20, modified_unix_seconds: 0 }]} onPreviewImage={onPreviewImage} />);

    if (!screen.queryByRole("complementary")) fireEvent.click(screen.getByRole("button", { name: "Expand sidebar" }));
    if (!screen.queryByRole("menu", { name: "Sidebar sections" })) fireEvent.click(screen.getByRole("button", { name: "Add sidebar tab" }));
    fireEvent.click(screen.getByRole("menuitemcheckbox", { name: "Artifacts (0)" }));
    expect(screen.getByRole("combobox", { name: "Select project image" })).toHaveValue("results/umap.png");
    fireEvent.click(screen.getByRole("button", { name: "Show image" }));
    expect(await screen.findByRole("img", { name: "results/umap.png" })).toHaveAttribute("src", "data:image/png;base64,iVBORw0KGgo=");
    expect(onPreviewImage).toHaveBeenCalledWith("results/umap.png");
    expect(screen.queryByRole("option", { name: "results/markers.csv" })).not.toBeInTheDocument();
  });

  it("renders English copy from the shared locale resource", () => {
    render(<WorkspaceShell project={project} locale="en-US" />);

    expect(screen.getByRole("main", { name: "Research conversation" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Expand sidebar" }));
    fireEvent.click(screen.getByRole("button", { name: "Add sidebar tab" }));
    expect(screen.getByRole("menuitemcheckbox", { name: "Notebook (0)" })).toBeInTheDocument();
  });

  it("sends an ordinary message without exposing a Plan tab", () => {
    render(<WorkspaceShell project={project} locale="zh-CN" />);
    fireEvent.change(screen.getByRole("textbox", { name: /描述研究目标/ }), { target: { value: "先检查双细胞率" } });
    fireEvent.click(screen.getByRole("button", { name: "发送" }));
    expect(screen.getByText("先检查双细胞率")).toBeInTheDocument();
    expect(screen.queryByRole("tab", { name: "Plan" })).not.toBeInTheDocument();
  });

  it.each(["local", "ssh"] as const)("sends an ordinary research request with unverified %s compute", async (kind) => {
    const onSend = vi.fn(() => true);
    const backendId = kind === "ssh" ? "ssh:offline" : "local";
    render(<WorkspaceShell project={project} locale="zh-CN" agentMode="agent" onSend={onSend}
      computeBackendId={backendId} computeBackends={[{ descriptor: { schema_version: 4, backend_id: backendId, kind, isolation: "process", available: false, supports_python: true, supports_r: true, supports_network_policy: false }, selectable: true, reason: null, python_status: "unverified", r_status: "unverified", resolved_image_id: null }]} />);
    fireEvent.change(screen.getByRole("textbox", { name: /描述研究目标/ }), { target: { value: "寻找肝癌单细胞文献" } });
    expect(screen.getByRole("button", { name: "发送" })).toBeEnabled();
    fireEvent.click(screen.getByRole("button", { name: "发送" }));
    await waitFor(() => expect(onSend).toHaveBeenCalledWith("寻找肝癌单细胞文献", "chat"));
  });

  it("does not send using a selectable backend while its catalog is refreshing", async () => {
    const onSend = vi.fn(() => true);
    const backend = { descriptor: { schema_version: 4 as const, backend_id: "local", kind: "local" as const, isolation: "process" as const, available: true, supports_python: true, supports_r: false, supports_network_policy: false }, selectable: true, reason: null, python_status: "available" as const, r_status: "unavailable" as const, resolved_image_id: null };
    const { rerender } = render(<WorkspaceShell project={project} locale="zh-CN" onSend={onSend} computeBackendId="local" computeBackends={[backend]} computeBusy />);
    const input = screen.getByRole("textbox", { name: /描述研究目标/ });
    fireEvent.change(input, { target: { value: "检查环境" } });
    expect(screen.getByRole("button", { name: "发送" })).toBeDisabled();
    fireEvent.keyDown(input, { key: "Enter" });
    expect(onSend).not.toHaveBeenCalled();
    rerender(<WorkspaceShell project={project} locale="zh-CN" onSend={onSend} computeBackendId="local" computeBackends={[backend]} />);
    fireEvent.click(screen.getByRole("button", { name: "发送" }));
    await waitFor(() => expect(onSend).toHaveBeenCalledWith("检查环境", "chat"));
  });

  it("shows the real agent state instead of a fixed remote progress value", async () => {
    let finish!: (value: boolean) => void;
    const onSend = vi.fn(() => new Promise<boolean>((resolve) => { finish = resolve; }));
    const backend = { descriptor: { schema_version: 4 as const, backend_id: "local", kind: "local" as const, isolation: "process" as const, available: true, supports_python: true, supports_r: false, supports_network_policy: false }, selectable: true, reason: null, python_status: "available" as const, r_status: "unavailable" as const, resolved_image_id: null };
    const { rerender } = render(<WorkspaceShell project={project} locale="zh-CN" onSend={onSend} computeBackendId="local" computeBackends={[backend]} />);

    expect(screen.queryByText("65%")).not.toBeInTheDocument();
    fireEvent.change(screen.getByRole("textbox", { name: /描述研究目标/ }), { target: { value: "检查 hg19 数据" } });
    fireEvent.click(screen.getByRole("button", { name: "发送" }));
    expect(screen.getByRole("textbox", { name: /描述研究目标/ })).toHaveValue("检查 hg19 数据");

    rerender(<WorkspaceShell project={project} locale="zh-CN" onSend={onSend} computeBackendId="local" computeBackends={[backend]} agentBusy agentNotice="503 model_not_found" />);
    expect(screen.getByRole("status")).toHaveTextContent("正在等待模型响应");
    expect(screen.getByRole("alert")).toHaveTextContent("503 model_not_found");
    finish(true);
    await waitFor(() => expect(onSend).toHaveBeenCalledWith("检查 hg19 数据", "chat"));
  });

  it("keeps a conversation error visible when a separate run status read recovers", () => {
    const props = { project, locale: "en-US" as const, onSend: () => true, agentNotice: "Send failed", runPollingError: "Database read unavailable" };
    const view = render(<WorkspaceShell {...props} />);
    expect(screen.getByText("Run status is temporarily unavailable")).toBeInTheDocument();
    expect(screen.getByText("Conversation did not complete")).toBeInTheDocument();
    view.rerender(<WorkspaceShell {...props} runPollingError="" />);
    expect(screen.queryByText("Run status is temporarily unavailable")).not.toBeInTheDocument();
    expect(screen.getByRole("alert")).toHaveTextContent("Send failed");
  });

  it("keeps the composer available with a long multiline assistant response", () => {
    const markdown = Array.from({ length: 80 }, (_, index) => `步骤 ${index + 1}\n\`gene_${index}\``).join("\n");
    render(<WorkspaceShell project={project} locale="zh-CN" onSend={() => true} messages={[{ id: "assistant-long", role: "assistant", markdown }]} />);

    expect(screen.getByText(/步骤 80/)).toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: /描述研究目标/ })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "发送" })).toBeInTheDocument();
  });

  it("requires an explicit decision for a pending V4 tool approval", () => {
    const decide = vi.fn();
    const request = { approval_id: "approval-1", call: { call_id: "call-1", tool_id: "runtime.execute", arguments: { code: "print(1)" } }, effect: "runtime" as const, reason: "首次代码执行需要批准", call_hash: "c".repeat(64) };
    render(<WorkspaceShell project={project} locale="zh-CN" runStarted activeRunId="run-approval" onCancelRun={vi.fn()} onDecideToolApprovalV4={decide} agentRunEventsV4={[{
      schema_version: 4, run_id: "run-approval", project_id: project.id, conversation_id: "conversation-1", sequence: 1, occurred_at: "2026-08-17T00:00:00Z", previous_hash: "", event_hash: "hash", event: { kind: "tool_approval_requested", request },
    }]} />);
    expect(screen.getByText(/等待工具审批 · 0 个步骤/)).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "终止运行" })).not.toBeInTheDocument();
    expect(screen.getByRole("region", { name: "工具审批" })).toHaveTextContent("runtime.execute");
    expect(screen.getByRole("region", { name: "工具审批" })).toHaveTextContent("首次代码执行需要批准");
    fireEvent.click(screen.getByRole("button", { name: "批准并继续" }));
    expect(decide).toHaveBeenCalledWith("run-approval", "approval-1", "c".repeat(64), "approved");
  });

  it("binds browser approval scope and resumes the same run after a successful browser call", () => {
    const decide = vi.fn();
    const resume = vi.fn();
    const base = { schema_version: 4 as const, run_id: "run-browser", project_id: project.id, conversation_id: "conversation-1", previous_hash: "", event_hash: "hash" };
    const request = { approval_id: "approval-browser", call: { call_id: "call-browser", tool_id: "web_open_tab", arguments: { session: "workspace", url: "https://example.org/paper" } }, effect: "network" as const, reason: "需要访问独立来源", call_hash: "d".repeat(64) };
    const { rerender } = render(<WorkspaceShell project={project} locale="zh-CN" runStarted activeRunId="run-browser" onDecideToolApprovalV4={decide} onResumeAgentRunV4={resume} agentRunEventsV4={[
      { ...base, sequence: 1, occurred_at: "2026-08-17T00:00:00Z", event: { kind: "tool_approval_requested" as const, request } },
    ]} />);
    fireEvent.change(screen.getByRole("combobox", { name: "浏览器授权范围" }), { target: { value: "project" } });
    fireEvent.click(screen.getByRole("button", { name: "批准并继续" }));
    expect(decide).toHaveBeenCalledWith("run-browser", "approval-browser", "d".repeat(64), "approved", "project");

    const disconnected = [
      { ...base, sequence: 2, occurred_at: "2026-08-17T00:00:01Z", event: { kind: "browser_connection_required" as const, session: "workspace" as const, protocol_version: 1, message: "connect the extension" } },
    ];
    rerender(<WorkspaceShell project={project} locale="zh-CN" runStarted activeRunId="run-browser" onResumeAgentRunV4={resume} agentRunEventsV4={disconnected} />);
    expect(screen.getByText(/等待连接浏览器 · 0 个步骤/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "已连接，继续" }));
    expect(resume).toHaveBeenCalledWith("run-browser");

    rerender(<WorkspaceShell project={project} locale="zh-CN" runStarted activeRunId="run-browser" onResumeAgentRunV4={resume} agentRunEventsV4={[...disconnected,
      { ...base, sequence: 3, occurred_at: "2026-08-17T00:00:02Z", event: { kind: "tool_finished" as const, outcome: { call_id: "call-search", tool_id: "web_search", succeeded: true, model_content: "searched", data: { tab_id: 10 }, provenance: [] } } },
    ]} />);
    expect(screen.getByText(/运行中 · 1 个步骤/)).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "已连接，继续" })).not.toBeInTheDocument();
  });

  it("keeps a terminal status when a tab-cleanup prompt follows it", () => {
    const closeTabs = vi.fn();
    const base = { schema_version: 4 as const, run_id: "run-cleanup", project_id: project.id, conversation_id: "conversation-1", previous_hash: "", event_hash: "hash" };
    render(<WorkspaceShell project={project} locale="zh-CN" runStarted activeRunId="run-cleanup" onCloseBrowserRunTabsV4={closeTabs} agentRunEventsV4={[
      { ...base, sequence: 1, occurred_at: "2026-08-17T00:00:00Z", event: { kind: "run_completed" as const } },
      { ...base, sequence: 2, occurred_at: "2026-08-17T00:00:01Z", event: { kind: "browser_tab_cleanup_required" as const, sessions: ["workspace" as const], tabs: [{ session: "workspace" as const, tab_id: 10, run_id: "run-cleanup", title: "Paper", origin: "https://example.org", created_by_run: true }], message: "close run tabs" } },
    ]} />);
    expect(screen.getByText(/已完成 · 0 个步骤/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "关闭本轮标签" }));
    expect(closeTabs).toHaveBeenCalledWith("run-cleanup", ["workspace"]);
  });

  it("pauses the same run for CAPTCHA intervention without claiming it was solved", () => {
    const resume = vi.fn();
    render(<WorkspaceShell project={project} locale="zh-CN" runStarted activeRunId="run-captcha" onResumeAgentRunV4={resume} agentRunEventsV4={[{
      schema_version: 4, run_id: "run-captcha", project_id: project.id, conversation_id: "conversation-1", sequence: 1, occurred_at: "2026-08-17T00:00:00Z", previous_hash: "", event_hash: "hash", event: { kind: "browser_human_intervention_required", session: "workspace", reason: "captcha_detected", message: "CAPTCHA detected" },
    }]} />);
    expect(screen.getByText(/等待人工处理浏览器 · 0 个步骤/)).toBeInTheDocument();
    expect(screen.getByText(/不会自动求解 CAPTCHA/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "已人工处理，继续" }));
    expect(resume).toHaveBeenCalledWith("run-captcha");
  });

  it("shows runtime uncertainty as a failure without requiring an evidence dialog", () => {
    const resolve = vi.fn();
    render(<WorkspaceShell project={project} locale="zh-CN" runStarted activeRunId="run-uncertain" onResolveUncertainV4={resolve} agentRunEventsV4={[{
      schema_version: 4, run_id: "run-uncertain", project_id: project.id, conversation_id: "conversation-1", sequence: 1, occurred_at: "2026-08-17T00:00:00Z", previous_hash: "", event_hash: "hash", event: { kind: "tool_dispatch_uncertain", call_id: "call-1", tool_id: "runtime.execute" },
    }]} />);
    expect(screen.queryByRole("textbox", { name: "核验证据" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "保存证据并继续" })).not.toBeInTheDocument();
    expect(screen.getByText("工具调用失败")).toBeInTheDocument();
    expect(screen.getByText(/失败 · 1 个步骤/)).toBeInTheDocument();
    expect(resolve).not.toHaveBeenCalled();
  });

  it("shows the actual MCP tool and query in the trace summary", () => {
    render(<WorkspaceShell project={project} locale="zh-CN" agentRunEventsV4={[{
      schema_version: 4, run_id: "mcp", project_id: project.id, conversation_id: "conversation-1", sequence: 1, occurred_at: "2026-08-17T00:00:00Z", previous_hash: "", event_hash: "hash",
      event: { kind: "tool_requested", call: { call_id: "search", tool_id: "use_mcp_tool", arguments: { server_id: "server", tool: "pubmed_search", arguments: { query: "Hi-C 3D genome" } } } },
    }]} />);
    const summary = document.querySelector(".v4-tool-trace-heading");
    expect(summary).toHaveTextContent("pubmed_search");
    expect(summary).toHaveTextContent("Hi-C 3D genome");
  });

  it("shows the iteration-limit status summary without a completion claim or resume banner", () => {
    render(<WorkspaceShell project={project} locale="zh-CN" onResumeAgentRunV4={vi.fn()} agentRunEventsV4={[{
      schema_version: 4, run_id: "limit", project_id: project.id, conversation_id: "conversation-1", sequence: 1, occurred_at: "2026-08-17T00:00:00Z", previous_hash: "", event_hash: "hash",
      event: { kind: "run_needs_attention", message: "max_iterations (100). 已获得文献列表，全文核验尚未完成。" },
    }]} />);
    expect(screen.getByText(/全文核验尚未完成/)).toBeInTheDocument();
    expect(screen.getByText(/需要处理 · 0 个步骤/)).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "继续运行" })).not.toBeInTheDocument();
  });

  it("keeps context budget diagnostics without a resume banner", () => {
    const resume = vi.fn();
    render(<WorkspaceShell project={project} locale="zh-CN" runStarted activeRunId="run-context" onResumeAgentRunV4={resume} agentRunEventsV4={[{
      schema_version: 4, run_id: "run-context", project_id: project.id, conversation_id: "conversation-1", sequence: 1, occurred_at: "2026-08-17T00:00:00Z", previous_hash: "", event_hash: "hash", event: { kind: "run_needs_attention", message: "run needs attention: model context exceeds byte budget (268851 > 262144); original run and evidence retained" },
    }]} />);
    expect(screen.queryByRole("button", { name: "继续运行" })).not.toBeInTheDocument();
    expect(screen.getByText(/model context exceeds byte budget/)).toBeInTheDocument();
  });

  it("shows legacy MCP uncertainty as failure without a side-effect form", () => {
    render(<WorkspaceShell project={project} locale="zh-CN" runStarted activeRunId="run-mcp-error" onResolveUncertainV4={vi.fn()} agentRunEventsV4={[{
      schema_version: 4, run_id: "run-mcp-error", project_id: project.id, conversation_id: "conversation-1", sequence: 1, occurred_at: "2026-08-17T00:00:00Z", previous_hash: "", event_hash: "hash", event: { kind: "tool_dispatch_uncertain", call_id: "call-1", tool_id: "use_mcp_tool" },
    }, { schema_version: 4, run_id: "run-mcp-error", project_id: project.id, conversation_id: "conversation-1", sequence: 2, occurred_at: "2026-08-17T00:00:01Z", previous_hash: "hash", event_hash: "hash2", event: { kind: "run_needs_attention", message: "MCP error: -32602: query exceeds limit" } }]} />);
    expect(screen.queryByRole("textbox", { name: "核验证据" })).not.toBeInTheDocument();
    expect(screen.queryByText("工具状态不确定")).not.toBeInTheDocument();
    expect(screen.getByText("工具调用失败")).toBeInTheDocument();
    expect(screen.getByText(/失败 · 1 个步骤/)).toBeInTheDocument();
  });

  it("renders user and assistant messages as safe GFM markdown", () => {
    render(<WorkspaceShell project={project} locale="en-US" messages={[
      { id: "user-md", role: "user", markdown: "**检查**矩阵" },
      { id: "assistant-md", role: "assistant", markdown: "## Results\n\n| gene | status |\n| --- | --- |\n| CD3D | pass |" },
    ]} />);
    expect(screen.getByText("检查")).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Results" })).toBeInTheDocument();
    expect(screen.getByRole("cell", { name: "CD3D" })).toBeInTheDocument();
  });

  it("opens the active trajectory while omitting persistence placeholder text", () => {
    const base = { schema_version: 4 as const, run_id: "run-technical", project_id: project.id, conversation_id: "conversation-1", previous_hash: "", event_hash: "hash" };
    render(<WorkspaceShell project={project} locale="zh-CN" runStarted activeRunId="run-technical" agentRunEventsV4={[
      { ...base, sequence: 1, occurred_at: "2026-08-17T00:00:00Z", event: { kind: "run_spec_frozen" as const, approval_hash: "a", spec_hash: "b" } },
      { ...base, sequence: 2, occurred_at: "2026-08-17T00:00:01Z", event: { kind: "tool_dispatch_started" as const, call_id: "call-1", tool_id: "runtime.execute", effect: "runtime", idempotency_key: "key" } },
    ]} />);
    expect(screen.getByText("执行过程").closest("details")).toHaveAttribute("open");
    expect(screen.queryByText(/状态已写入可验证事件链/)).not.toBeInTheDocument();
  });

  it("keeps agent.complete internal and shows a clean verification status", () => {
    const base = { schema_version: 4 as const, run_id: "run-completing", project_id: project.id, conversation_id: "conversation-1", previous_hash: "", event_hash: "hash" };
    render(<WorkspaceShell project={project} locale="zh-CN" runStarted activeRunId="run-completing" agentRunEventsV4={[
      { ...base, sequence: 1, occurred_at: "2026-08-17T00:00:00Z", event: { kind: "tool_requested" as const, call: { call_id: "complete-1", tool_id: "agent.complete", arguments: { schema_version: 4, summary: "done", answer_markdown: "## 不应显示在工具卡片中", criteria: [] } } } },
    ]} />);
    expect(screen.getByText("正在核验最终结果…")).toBeInTheDocument();
    expect(screen.getByText(/运行中 · 0 个步骤/)).toBeInTheDocument();
    expect(screen.queryByText("agent.complete")).not.toBeInTheDocument();
    expect(screen.queryByText("不应显示在工具卡片中")).not.toBeInTheDocument();
  });

  it("renders tool calls as human-readable collapsed steps", () => {
    const base = { schema_version: 4 as const, run_id: "run-readable-tool", project_id: project.id, conversation_id: "conversation-1", previous_hash: "", event_hash: "hash" };
    render(<WorkspaceShell project={project} locale="zh-CN" runStarted activeRunId="run-readable-tool" agentRunEventsV4={[
      { ...base, sequence: 1, occurred_at: "2026-08-17T00:00:00Z", event: { kind: "tool_requested" as const, call: { call_id: "read-1", tool_id: "project.read", arguments: { path: "results/audit.md" } } } },
      { ...base, sequence: 2, occurred_at: "2026-08-17T00:00:01Z", event: { kind: "tool_finished" as const, outcome: { call_id: "read-1", tool_id: "project.read", succeeded: true, model_content: "large internal report", data: null, provenance: [] } } },
    ]} />);
    fireEvent.click(screen.getByText("执行过程"));
    const step = screen.getByTitle("读取项目文件").closest("details");
    expect(step).not.toHaveAttribute("open");
    expect(step).toHaveTextContent("results/audit.md");
    expect(screen.getByText(/运行中 · 1 个步骤/)).toBeInTheDocument();
  });

  it("merges tool request, dispatch, finish, and reuse events into one detail", () => {
    const base = { schema_version: 4 as const, run_id: "run-tools", project_id: project.id, conversation_id: "conversation-1", previous_hash: "", event_hash: "hash" };
    render(<WorkspaceShell project={project} locale="zh-CN" runStarted activeRunId="run-tools" agentRunEventsV4={[
      { ...base, sequence: 1, occurred_at: "2026-08-17T00:00:00Z", event: { kind: "tool_requested" as const, call: { call_id: "call-1", tool_id: "runtime.execute", arguments: { code: "print(1)", secret: "hidden" } } } },
      { ...base, sequence: 2, occurred_at: "2026-08-17T00:00:01Z", event: { kind: "tool_dispatch_started" as const, call_id: "call-1", tool_id: "runtime.execute", effect: "runtime", idempotency_key: "key" } },
      { ...base, sequence: 3, occurred_at: "2026-08-17T00:00:02Z", event: { kind: "tool_finished" as const, outcome: { call_id: "call-1", tool_id: "runtime.execute", succeeded: true, model_content: "QC complete", data: null, provenance: [] } } },
      { ...base, sequence: 4, occurred_at: "2026-08-17T00:00:03Z", event: { kind: "tool_outcome_reused" as const, idempotency_key: "key", outcome: { call_id: "call-1", tool_id: "runtime.execute", succeeded: true, model_content: "QC complete", data: null, provenance: [] } } },
      { ...base, sequence: 5, occurred_at: "2026-08-17T00:00:04Z", event: { kind: "tool_requested" as const, call: { call_id: "call-2", tool_id: "runtime.execute", arguments: {} } } },
      { ...base, sequence: 6, occurred_at: "2026-08-17T00:00:05Z", event: { kind: "tool_finished" as const, outcome: { call_id: "call-2", tool_id: "runtime.execute", succeeded: true, model_content: "second result", data: null, provenance: [] } } },
      { ...base, sequence: 7, occurred_at: "2026-08-17T00:00:06Z", event: { kind: "tool_requested" as const, call: { call_id: "call-3", tool_id: "runtime.execute", arguments: {} } } },
      { ...base, sequence: 8, occurred_at: "2026-08-17T00:00:07Z", event: { kind: "tool_finished" as const, outcome: { call_id: "call-3", tool_id: "runtime.execute", succeeded: false, model_content: "failed result", data: null, provenance: [] } } },
    ]} />);
    fireEvent.click(screen.getByText("执行过程"));
    expect(screen.getAllByText(/runtime\.execute/)).toHaveLength(3);
    expect(document.querySelector(".v4-tool-mark.reused")).toBeInTheDocument();
    expect(document.querySelector(".v4-tool-mark.succeeded")).toBeInTheDocument();
    expect(document.querySelector(".v4-tool-mark.failed")).toBeInTheDocument();
    expect(document.querySelector(".v4-tool-mark.failed")?.closest("details")).not.toHaveAttribute("open");
    expect(document.querySelector(".v4-tool-mark.succeeded")?.closest("details")).not.toHaveAttribute("open");
    expect(screen.getByText("QC complete")).toBeInTheDocument();
    expect(screen.getByText(/print\(1\)/)).toBeInTheDocument();
    expect(screen.getByText(/\[REDACTED\]/)).toBeInTheDocument();
    expect(screen.queryByText(/hidden/)).not.toBeInTheDocument();
  });

  it("does not keep increasing an unfinished tool duration after cancellation", () => {
    const base = { schema_version: 4 as const, run_id: "run-cancelled-tool", project_id: project.id, conversation_id: "conversation-1", previous_hash: "", event_hash: "hash" };
    render(<WorkspaceShell project={project} locale="en-US" activeRunId="run-cancelled-tool" agentRunEventsV4={[
      { ...base, sequence: 1, occurred_at: "2026-08-17T00:00:00Z", event: { kind: "tool_requested" as const, call: { call_id: "old-call", tool_id: "project.read", arguments: { path: "old.txt" } } } },
      { ...base, sequence: 2, occurred_at: "2026-08-17T00:00:01Z", event: { kind: "run_cancelled" as const } },
    ]} />);
    fireEvent.click(screen.getByText("Processed"));
    const row = document.querySelector(".v4-tool-trace");
    expect(row?.querySelector(".v4-tool-metrics")).toHaveTextContent("");
    expect(row).not.toHaveAttribute("open");
  });

  it("auto-expands blocked runs while keeping the interaction card available", () => {
    const base = { schema_version: 4 as const, run_id: "run-blocked", project_id: project.id, conversation_id: "conversation-1", previous_hash: "", event_hash: "hash" };
    const request = { approval_id: "approval-blocked", call: { call_id: "call-1", tool_id: "runtime.execute", arguments: {} }, effect: "runtime" as const, reason: "需要批准", call_hash: "c".repeat(64) };
    render(<WorkspaceShell project={project} locale="zh-CN" runStarted activeRunId="run-blocked" onDecideToolApprovalV4={vi.fn()} agentRunEventsV4={[{ ...base, sequence: 1, occurred_at: "2026-08-17T00:00:00Z", event: { kind: "tool_approval_requested" as const, request } }]} />);
    expect(screen.getByText("执行过程").closest("details")).toHaveAttribute("open");
    expect(screen.getByRole("button", { name: "批准并继续" })).toBeEnabled();
  });

  it("disables the composer for an active run but leaves approval controls enabled", () => {
    const base = { schema_version: 4 as const, run_id: "run-composer-lock", project_id: project.id, conversation_id: "conversation-1", previous_hash: "", event_hash: "hash" };
    const request = { approval_id: "approval-composer", call: { call_id: "call-1", tool_id: "runtime.execute", arguments: {} }, effect: "runtime" as const, reason: "需要批准", call_hash: "c".repeat(64) };
    render(<WorkspaceShell project={project} locale="zh-CN" onSend={vi.fn()} runStarted activeRunId="run-composer-lock" agentRunEventsV4={[{ ...base, sequence: 1, occurred_at: "2026-08-17T00:00:00Z", event: { kind: "tool_approval_requested" as const, request } }]} onDecideToolApprovalV4={vi.fn()} />);
    expect(screen.getByRole("textbox", { name: /描述研究目标/ })).toBeDisabled();
    expect(screen.getByRole("button", { name: "执行中…" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "批准并继续" })).toBeEnabled();
  });

  it("does not submit the composer twice while the first request is pending", async () => {
    let release!: (accepted: boolean) => void;
    const onSend = vi.fn(() => new Promise<boolean>((resolve) => { release = resolve; }));
    render(<WorkspaceShell project={project} locale="zh-CN" onSend={onSend}
      computeBackendId="local" computeBackends={[{ descriptor: { schema_version: 4, backend_id: "local", kind: "local", isolation: "process", available: true, supports_python: true, supports_r: false, supports_network_policy: false }, selectable: true, reason: null, python_status: "available", r_status: "unavailable", resolved_image_id: null }]} />);
    const composer = screen.getByRole("textbox", { name: /描述研究目标/ });
    fireEvent.change(composer, { target: { value: "检查矩阵" } });
    const send = screen.getByRole("button", { name: "发送" });
    fireEvent.click(send);
    fireEvent.click(send);
    expect(onSend).toHaveBeenCalledTimes(1);
    release(true);
    await waitFor(() => expect(composer).toBeEnabled());
  });

  it("guards plan approval and resume actions against double clicks", async () => {
    let releaseApproval!: () => void;
    const onApprovePlan = vi.fn(() => new Promise<void>((resolve) => { releaseApproval = resolve; }));
    const plan = { schema_version: 4 as const, objective: "执行 QC", steps: ["检查输入"], completion_criteria: ["报告完成"], requested_capabilities: [] };
    const { rerender } = render(<WorkspaceShell project={project} locale="zh-CN"
      v4Plan={{ run_id: "run-plan-guard", status: "awaiting_approval", plan, plan_hash: "plan", compute_selection: null, approval_hash: "approval" }} onApprovePlan={onApprovePlan} />);
    const approve = screen.getByRole("button", { name: "批准并运行" });
    fireEvent.click(approve);
    fireEvent.click(approve);
    expect(onApprovePlan).toHaveBeenCalledTimes(1);
    expect(approve).toBeDisabled();
    releaseApproval();
    await waitFor(() => expect(screen.getByRole("button", { name: "批准并运行" })).toBeEnabled());

    let releaseResume!: () => void;
    const onResume = vi.fn(() => new Promise<void>((resolve) => { releaseResume = resolve; }));
    const base = { schema_version: 4 as const, run_id: "run-resume-guard", project_id: project.id, conversation_id: "conversation-1", previous_hash: "", event_hash: "hash" };
    const failed = [{ ...base, sequence: 1, occurred_at: "2026-08-17T00:00:01Z", event: { kind: "run_created" as const, mode: "execute" as const } }, { ...base, sequence: 2, occurred_at: "2026-08-17T00:00:02Z", event: { kind: "browser_connection_required" as const, session: "workspace" as const, protocol_version: 1, message: "connect the extension" } }];
    rerender(<WorkspaceShell project={project} locale="zh-CN" agentRunEventsV4={failed} onResumeAgentRunV4={onResume} />);
    const resume = screen.getByRole("button", { name: "已连接，继续" });
    fireEvent.click(resume);
    fireEvent.click(resume);
    expect(onResume).toHaveBeenCalledTimes(1);
    expect(resume).toBeDisabled();
    releaseResume();
    await waitFor(() => expect(screen.getByRole("button", { name: "已连接，继续" })).toBeEnabled());
  });

  it("locks a pending Plan conversation even before execution starts", () => {
    const plan = { schema_version: 4 as const, objective: "审核 QC", steps: ["检查输入"], completion_criteria: ["报告完成"], requested_capabilities: [] };
    const revision = { id: "revision-pending", project_id: project.id, conversation_id: "conversation-1", run_id: "run-pending", revision: 4, plan, markdown: "# 审核 QC", plan_hash: "plan-hash", status: "pending" as const, feedback: null, created_at: "2026-08-20T00:00:00Z", updated_at: "2026-08-20T00:00:00Z" };
    render(<WorkspaceShell project={project} locale="zh-CN" onSend={vi.fn()}
      agentMode="plan" conversationLocked latestPlanRevision={revision}
      v4Plan={{ run_id: revision.run_id, status: "awaiting_approval", plan, plan_hash: revision.plan_hash, compute_selection: null, approval_hash: "approval", plan_revision: 4, session_mode: "plan" }}
      onApprovePlan={vi.fn()} onRequestPlanRevision={vi.fn()} onCancelRun={vi.fn()} />);

    expect(screen.getByRole("textbox", { name: /描述研究目标/ })).toBeDisabled();
    expect(screen.getByRole("button", { name: "批准并运行" })).toBeEnabled();
    expect(screen.getByRole("button", { name: "请求修改" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "取消计划" })).toBeEnabled();
  });

  it("submits revision feedback once and disables every competing plan action", async () => {
    let release!: () => void;
    const requestRevision = vi.fn(() => new Promise<void>((resolve) => { release = resolve; }));
    const plan = { schema_version: 4 as const, objective: "修改 QC", steps: ["检查输入"], completion_criteria: ["报告完成"], requested_capabilities: [] };
    const revision = { id: "revision-feedback", project_id: project.id, conversation_id: "conversation-1", run_id: "run-feedback", revision: 5, plan, markdown: "# 修改 QC", plan_hash: "plan-hash", status: "pending" as const, feedback: null, created_at: "2026-08-20T00:00:00Z", updated_at: "2026-08-20T00:00:00Z" };
    render(<WorkspaceShell project={project} locale="zh-CN"
      agentMode="plan" conversationLocked latestPlanRevision={revision}
      v4Plan={{ run_id: revision.run_id, status: "awaiting_approval", plan, plan_hash: revision.plan_hash, compute_selection: null, approval_hash: "approval", plan_revision: 5, session_mode: "plan" }}
      onApprovePlan={vi.fn()} onRequestPlanRevision={requestRevision} onCancelRun={vi.fn()} />);
    const feedback = screen.getByRole("textbox", { name: "计划修改意见" });
    fireEvent.change(feedback, { target: { value: "补充批次效应检查" } });
    const request = screen.getByRole("button", { name: "请求修改" });
    fireEvent.click(request);
    fireEvent.click(request);

    expect(requestRevision).toHaveBeenCalledTimes(1);
    expect(requestRevision).toHaveBeenCalledWith("补充批次效应检查");
    expect(screen.getByRole("button", { name: "批准并运行" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "取消计划" })).toBeDisabled();
    release();
    await waitFor(() => expect(feedback).toHaveValue(""));
    expect(screen.getByRole("button", { name: "批准并运行" })).toBeEnabled();
    expect(screen.getByRole("button", { name: "取消计划" })).toBeEnabled();
  });

  it("guards answer recovery actions while preserving their controls during a run", async () => {
    let releaseAnswer!: () => void;
    const onAnswer = vi.fn(() => new Promise<void>((resolve) => { releaseAnswer = resolve; }));
    const base = { schema_version: 4 as const, run_id: "run-input-guard", project_id: project.id, conversation_id: "conversation-1", previous_hash: "", event_hash: "hash" };
    const requested = { ...base, sequence: 1, occurred_at: "2026-08-17T00:00:01Z", event: { kind: "input_requested" as const, question_id: "species", question: "物种？" } };
    render(<WorkspaceShell project={project} locale="zh-CN" runStarted activeRunId="run-input-guard" agentRunEventsV4={[requested]} onAnswerAgentQuestionV4={onAnswer} />);
    fireEvent.change(screen.getByRole("textbox", { name: "回答 V4 问题" }), { target: { value: "人" } });
    const answer = screen.getByRole("button", { name: "回答并恢复" });
    fireEvent.click(answer);
    fireEvent.click(answer);
    expect(onAnswer).toHaveBeenCalledTimes(1);
    expect(answer).toBeDisabled();
    releaseAnswer();
    await waitFor(() => expect(screen.getByRole("button", { name: "回答并恢复" })).toBeEnabled());

  });
});

it("updates one live progress row then replaces it with the committed message", () => {
  const props = { project, locale: "en-US" as const, runStarted: true, activeRunId: "live" };
  const base = { schema_version: 4 as const, run_id: "live", project_id: project.id, conversation_id: "c", previous_hash: "", event_hash: "h", occurred_at: "2026-09-11T00:00:00Z" };
  const events: import("../../types").AgentRunEventV4[] = [{ ...base, sequence: 1, event: { kind: "run_created", mode: "execute" } }];
  const { rerender } = render(<WorkspaceShell {...props} agentRunEventsV4={events} agentTextPreview={{ run_id: "live", text: "Searching" }} />);
  const row = screen.getByRole("article", { name: "Live model output" });
  rerender(<WorkspaceShell {...props} agentRunEventsV4={events} agentTextPreview={{ run_id: "live", text: "Searching literature" }} />);
  expect(screen.getByRole("article", { name: "Live model output" })).toBe(row);
  expect(row).toHaveTextContent("Searching literature");
  rerender(<WorkspaceShell {...props} agentRunEventsV4={[...events, { ...base, sequence: 2, event: { kind: "model_text", text: "Searching literature" } }]} agentTextPreview={null} />);
  expect(screen.queryByRole("article", { name: "Live model output" })).not.toBeInTheDocument();
  expect(screen.getAllByRole("article", { name: "Model output" })).toHaveLength(1);
  rerender(<WorkspaceShell {...props} agentRunEventsV4={events} agentTextPreview={{ run_id: "other", text: "wrong run" }} />);
  expect(screen.queryByText("wrong run")).not.toBeInTheDocument();
});

it("keeps the run duration in the process header while showing a separate request wait timer", () => {
  vi.useFakeTimers();
  vi.setSystemTime(new Date("2026-09-22T12:02:13Z"));
  const clearInterval = vi.spyOn(window, "clearInterval");
  const base = { schema_version: 4 as const, run_id: "model-wait", project_id: project.id, conversation_id: "c", previous_hash: "", event_hash: "h" };
  const events: import("../../types").AgentRunEventV4[] = [
    { ...base, sequence: 1, occurred_at: "2026-09-22T11:59:00Z", event: { kind: "run_created", mode: "execute" } },
    { ...base, sequence: 2, occurred_at: "2026-09-22T12:00:00Z", event: { kind: "model_request_started", request: modelRequest("attempt-1") } },
  ];
  const props = { project, locale: "en-US" as const, runStarted: true, activeRunId: base.run_id };
  const { rerender, unmount } = render(<WorkspaceShell {...props} agentRunEventsV4={events} />);

  expect(screen.getByText(/Waiting for model · 0 steps · 3m 13s/)).toBeInTheDocument();
  expect(screen.getByRole("status", { name: "Current model activity" })).toHaveTextContent("This request 2m 13s");
  act(() => vi.advanceTimersByTime(1_000));
  expect(screen.getByText(/Waiting for model · 0 steps · 3m 14s/)).toBeInTheDocument();

  rerender(<WorkspaceShell {...props} agentRunEventsV4={events} agentTextPreview={{ run_id: base.run_id, text: "First public tokens" }} />);
  expect(screen.getByText(/Generating response · 0 steps · 3m 14s/)).toBeInTheDocument();
  unmount();
  expect(clearInterval).toHaveBeenCalled();
});

it("tracks only the latest model attempt and clears waiting at durable boundaries", () => {
  vi.useFakeTimers();
  vi.setSystemTime(new Date("2026-09-22T12:00:30Z"));
  const base = { schema_version: 4 as const, run_id: "attempts", project_id: project.id, conversation_id: "c", previous_hash: "", event_hash: "h" };
  const event = (sequence: number, seconds: number, value: import("../../types").AgentEventKindV4): import("../../types").AgentRunEventV4 => ({
    ...base,
    sequence,
    occurred_at: `2026-09-22T12:00:${String(seconds).padStart(2, "0")}Z`,
    event: value,
  });
  const requestOne = event(1, 0, { kind: "model_request_started", request: modelRequest("attempt-1") });
  const requestTwo = event(2, 10, { kind: "model_request_started", request: modelRequest("attempt-2") });
  const staleFinal = event(3, 11, { kind: "model_usage_observed", observation: modelUsage("attempt-1", "final") });
  const partial = event(4, 12, { kind: "model_usage_observed", observation: modelUsage("attempt-2", "partial") });
  const props = { project, locale: "en-US" as const, runStarted: true, activeRunId: base.run_id };
  const { rerender } = render(<WorkspaceShell {...props} agentRunEventsV4={[requestOne, requestTwo, staleFinal, partial]} />);

  expect(screen.getByText(/Waiting for model · 0 steps · 30s/)).toBeInTheDocument();
  expect(screen.getByRole("status", { name: "Current model activity" })).toHaveTextContent("This request 20s");

  const matchingFinal = event(5, 13, { kind: "model_usage_observed", observation: modelUsage("attempt-2", "final") });
  rerender(<WorkspaceShell {...props} agentRunEventsV4={[requestOne, requestTwo, staleFinal, partial, matchingFinal]} />);
  expect(screen.queryByText(/Waiting for model/)).not.toBeInTheDocument();

  const interruptedRequest = event(6, 14, { kind: "model_request_started", request: modelRequest("attempt-interrupted") });
  const interrupted = event(7, 15, { kind: "model_usage_observed", observation: modelUsage("attempt-interrupted", "interrupted") });
  rerender(<WorkspaceShell {...props} agentRunEventsV4={[interruptedRequest, interrupted]} />);
  expect(screen.queryByText(/Waiting for model/)).not.toBeInTheDocument();

  const requestThree = event(8, 16, { kind: "model_request_started", request: modelRequest("attempt-3") });
  const committedText = event(9, 17, { kind: "model_text", text: "Completed response" });
  rerender(<WorkspaceShell {...props} agentRunEventsV4={[requestOne, requestTwo, requestThree, committedText]} />);
  expect(screen.queryByText(/Waiting for model/)).not.toBeInTheDocument();

  const requestFour = event(10, 18, { kind: "model_request_started", request: modelRequest("attempt-4") });
  const toolBoundary = event(11, 19, { kind: "tool_requested", call: { call_id: "read", tool_id: "project.read", arguments: {} } });
  rerender(<WorkspaceShell {...props} agentRunEventsV4={[requestFour, toolBoundary]} />);
  expect(screen.queryByText(/Waiting for model/)).not.toBeInTheDocument();

  const paused = event(12, 20, { kind: "input_requested", question_id: "q", question: "Continue?" });
  rerender(<WorkspaceShell {...props} agentRunEventsV4={[requestFour, paused]} />);
  expect(screen.getByText(/Waiting for input · 0 steps/)).toBeInTheDocument();
  expect(screen.queryByText(/Waiting for model/)).not.toBeInTheDocument();

  const answered = event(13, 21, { kind: "user_input_answered", question_id: "q", answer: "Yes" });
  rerender(<WorkspaceShell {...props} agentRunEventsV4={[requestFour, paused, answered]} />);
  expect(screen.queryByText(/Waiting for model/)).not.toBeInTheDocument();

  const requestFive = event(14, 22, { kind: "model_request_started", request: modelRequest("attempt-5") });
  rerender(<WorkspaceShell {...props} agentRunEventsV4={[requestFour, paused, answered, requestFive]} />);
  expect(screen.getByText(/Waiting for model · 0 steps · 12s/)).toBeInTheDocument();
  expect(screen.getByRole("status", { name: "Current model activity" })).toHaveTextContent("This request 8s");

  const terminal = event(15, 23, { kind: "run_cancelled" });
  rerender(<WorkspaceShell {...props} agentRunEventsV4={[requestFour, terminal]} />);
  expect(screen.queryByText(/Waiting for model/)).not.toBeInTheDocument();

  rerender(<WorkspaceShell project={project} locale="en-US" agentRunEventsV4={[requestFour]} />);
  expect(screen.queryByText(/Waiting for model/)).not.toBeInTheDocument();
});

function modelRequest(attemptId: string): import("../../types").ModelRequestStartedV4 {
  return {
    logical_request_id: "logical-request",
    attempt_id: attemptId,
    model_profile_id: "profile",
    context_limit_source: { kind: "unknown" },
  };
}

function modelUsage(attemptId: string, state: "partial" | "final" | "interrupted"): import("../../types").ModelUsageObservationV4 {
  return {
    ...modelRequest(attemptId),
    sample_index: 0,
    state,
    aggregation: "unknown",
  };
}

describe("Session follow-up wiring", () => {
  const base = { schema_version: 4 as const, run_id: "completed-session", project_id: project.id, conversation_id: "session-1", previous_hash: "", event_hash: "hash", occurred_at: "2026-09-12T00:00:01Z" };

  it("shows three questions for a completed run and fills the composer without sending", async () => {
    const questions = ["How should I validate these results?", "Which dataset should I compare next?", "Can you explain the remaining limitations?"];
    const suggest = vi.fn().mockResolvedValue(questions);
    const send = vi.fn();
    render(<WorkspaceShell project={project} locale="en-US"
      activeConversationId="session-1" activeRunId={base.run_id} onSend={send} onSuggestFollowUps={suggest}
      agentRunEventsV4={[{ ...base, sequence: 1, event: { kind: "run_completed" } }]} />);
    const region = await screen.findByRole("region", { name: "Suggested follow-up questions" });
    expect(within(region).getAllByRole("button")).toHaveLength(3);
    expect(suggest).toHaveBeenCalledTimes(1);
    expect(suggest).toHaveBeenCalledWith(base.run_id);
    fireEvent.click(within(region).getByRole("button", { name: questions[1] }));
    expect(screen.getByRole("textbox")).toHaveValue(questions[1]);
    expect(send).not.toHaveBeenCalled();
  });

  it.each(["run_needs_attention", "run_failed"] as const)("does not generate suggestions after %s", async (kind) => {
    const suggest = vi.fn().mockResolvedValue(["First?", "Second?", "Third?"]);
    render(<WorkspaceShell project={project} locale="en-US"
      activeConversationId="session-1" activeRunId={base.run_id} onSend={vi.fn()} onSuggestFollowUps={suggest}
      agentRunEventsV4={[{ ...base, sequence: 1, event: { kind, message: "Review required" } }]} />);
    expect(screen.queryByRole("region", { name: "Suggested follow-up questions" })).not.toBeInTheDocument();
    expect(suggest).not.toHaveBeenCalled();
  });
});

it("collapses a finished failure while keeping its reason and final answer visible", () => {
  const base = { schema_version: 4 as const, run_id: "failed-compact", project_id: project.id, conversation_id: "conversation-1", previous_hash: "", event_hash: "h", occurred_at: "2026-09-14T00:00:00Z" };
  render(<WorkspaceShell project={project} locale="zh-CN" runStarted activeRunId={base.run_id} messages={[{ id: "answer", role: "assistant", markdown: "已核验的部分结果" }]} agentRunEventsV4={[
    { ...base, sequence: 1, event: { kind: "model_text", text: "正在检索。\n\n更详细的检索范围。" } },
    { ...base, sequence: 2, event: { kind: "run_failed", message: "检索服务暂不可用" } },
  ]} />);
  expect(screen.getByText("执行过程").closest("details")).not.toHaveAttribute("open");
  expect(screen.getByText("检索服务暂不可用")).toBeVisible();
  expect(screen.getByText("已核验的部分结果")).toBeVisible();
});

it("shows a first-line progress snippet by default and preserves manual expansion across streamed updates", () => {
  const base = { schema_version: 4 as const, run_id: "progress-compact", project_id: project.id, conversation_id: "conversation-1", previous_hash: "", event_hash: "h", occurred_at: "2026-09-14T00:00:00Z" };
  const props = { project, locale: "zh-CN" as const, runStarted: true, activeRunId: base.run_id, agentRunEventsV4: [{ ...base, sequence: 1, event: { kind: "model_text" as const, text: "正在检索。\n\n## 检索范围\n\nHi-C 与染色质结构。" } }] };
  const { rerender } = render(<WorkspaceShell {...props} />);
  expect(screen.queryByRole("heading", { name: "检索范围" })).not.toBeInTheDocument();
  expect(screen.getByRole("button", { name: /PROGRESS.*正在检索/ })).toHaveAttribute("aria-expanded", "false");
  fireEvent.click(screen.getByRole("button", { name: /PROGRESS.*正在检索/ }));
  expect(screen.getByRole("heading", { name: "检索范围" })).toBeVisible();
  rerender(<WorkspaceShell {...props} agentRunEventsV4={[...props.agentRunEventsV4, { ...base, sequence: 2, event: { kind: "model_text", text: "\n继续核对引用。" } }]} />);
  expect(screen.getByRole("heading", { name: "检索范围" })).toBeVisible();
  fireEvent.click(screen.getByRole("button", { name: "PROGRESS" }));
  expect(screen.queryByRole("heading", { name: "检索范围" })).not.toBeInTheDocument();
  rerender(<WorkspaceShell {...props} agentTextPreview={{ run_id: base.run_id, text: "继续核对引用" }} />);
  expect(screen.queryByRole("heading", { name: "检索范围" })).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: /PROGRESS.*正在检索/ }));
  expect(screen.getByRole("heading", { name: "检索范围" })).toBeVisible();
});

it("uses the frozen skill name while keeping its ID in the tool evidence", () => {
  const base = { schema_version: 4 as const, run_id: "skill-readable", project_id: project.id, conversation_id: "conversation-1", previous_hash: "", event_hash: "h", occurred_at: "2026-09-14T00:00:00Z" };
  const skillId = "f99c620a-e92e-470c-b52d-aa64734665e4";
  render(<WorkspaceShell project={project} locale="zh-CN" runStarted activeRunId={base.run_id} agentRunEventsV4={[
    { ...base, sequence: 1, event: { kind: "tool_requested", call: { call_id: "skill", tool_id: "use_skill", arguments: { skill_id: skillId } } } },
    { ...base, sequence: 2, event: { kind: "tool_finished", outcome: { call_id: "skill", tool_id: "use_skill", succeeded: true, model_content: JSON.stringify({ skill_id: skillId, name: "文献综述" }), data: { skill_id: skillId, name: "文献综述" }, provenance: [] } } },
  ]} />);
  const step = screen.getByText("SKILL").closest("details")!;
  expect(step.querySelector("summary")).toHaveTextContent("文献综述");
  expect(step.querySelector("summary")).not.toHaveTextContent(skillId);
  expect(step.querySelector("code")).toHaveTextContent(skillId);
});

it("keeps unnamed skills readable without substituting an unrelated result name", () => {
  const base = { schema_version: 4 as const, run_id: "skill-legacy", project_id: project.id, conversation_id: "conversation-1", previous_hash: "", event_hash: "h", occurred_at: "2026-09-14T00:00:00Z" };
  render(<WorkspaceShell project={project} locale="en-US" runStarted activeRunId={base.run_id} agentRunEventsV4={[
    { ...base, sequence: 1, event: { kind: "tool_requested", call: { call_id: "skill", tool_id: "use_skill", arguments: { skill_id: "original-id" } } } },
    { ...base, sequence: 2, event: { kind: "tool_finished", outcome: { call_id: "skill", tool_id: "use_skill", succeeded: true, model_content: JSON.stringify({ skill_id: "different-id", name: "Wrong skill" }), data: null, provenance: [] } } },
  ]} />);
  const heading = screen.getByText("SKILL").closest("summary")!;
  expect(heading).toHaveTextContent("Load skill");
  expect(heading).not.toHaveTextContent("Wrong skill");
  expect(heading).not.toHaveTextContent("original-id");
});

it("provides a keyboard accessible scroll region for wide answer tables", () => {
  render(<WorkspaceShell project={project} locale="zh-CN" messages={[{ id: "wide", role: "assistant", markdown: "## 文献比较\n\n| 论文 | 结论 |\n| --- | --- |\n| Hi-C | 染色质结构 |\n\n[原文](https://example.org/paper)\n\n<script>alert(1)</script>" }]} />);
  const table = screen.getByRole("table");
  expect(table.parentElement).toHaveAttribute("tabindex", "0");
  expect(table.parentElement).toHaveAttribute("role", "region");
  expect(screen.getByRole("link", { name: "原文" })).toHaveAttribute("href", "https://example.org/paper");
  expect(document.querySelector(".markdown-content script")).toBeNull();
});

it("labels the final response with only its persisted timestamp and preserves the answer content", () => {
  const createdAt = "2026-09-25T09:41:00Z";
  render(<WorkspaceShell project={project} locale="zh-CN" messages={[
    { id: "dated-answer", role: "assistant", markdown: "> 证据摘要\n\n1. **核对** `counts.tsv`\n\n[查看来源](https://example.org/source)", created_at: createdAt },
    { id: "undated-answer", role: "assistant", markdown: "仍需核验的结果" },
  ]} />);
  const responses = screen.getAllByRole("article", { name: "Agent 回复" });
  expect(within(responses[0]).getByText("OMICSOPS")).toBeVisible();
  expect(within(responses[0]).getByRole("time")).toHaveAttribute("datetime", createdAt);
  expect(within(responses[0]).getByRole("link", { name: "查看来源" })).toHaveAttribute("href", "https://example.org/source");
  expect(within(responses[1]).queryByRole("time")).not.toBeInTheDocument();
});

it("keeps live Markdown visible when execution details are collapsed", () => {
  const base = { schema_version: 4 as const, run_id: "live-markdown", project_id: project.id, conversation_id: "c", previous_hash: "", event_hash: "h", occurred_at: "2026-09-15T00:00:00Z" };
  render(<WorkspaceShell project={project} locale="zh-CN" runStarted activeRunId={base.run_id} agentRunEventsV4={[{ ...base, sequence: 1, event: { kind: "run_created", mode: "execute" } }]} agentTextPreview={{ run_id: base.run_id, text: "## 正在核对文献\n\n- **检查来源**\n- 对比研究方法" }} />);
  const live = screen.getByRole("article", { name: "模型实时输出" });
  expect(live.closest("details")).toBeNull();
  expect(within(live).getByRole("heading", { name: "正在核对文献" })).toBeVisible();
  expect(within(live).getAllByRole("listitem")).toHaveLength(2);
  fireEvent.click(screen.getByText("执行过程"));
  expect(live).toBeVisible();
});

it("folds earlier activity in a long run without discarding its evidence", () => {
  const base = { schema_version: 4 as const, run_id: "long-activity", project_id: project.id, conversation_id: "c", previous_hash: "", event_hash: "h", occurred_at: "2026-09-15T00:00:00Z" };
  const events = Array.from({ length: 9 }, (_, i) => ({ ...base, sequence: i + 1, event: { kind: "tool_requested" as const, call: { call_id: `call-${i}`, tool_id: "project.read", arguments: { path: `file-${i}.md` } } } }));
  const props = { project, locale: "zh-CN" as const, runStarted: true, activeRunId: base.run_id };
  const { rerender } = render(<WorkspaceShell {...props} agentRunEventsV4={events} />);
  expect(screen.getByText("file-0.md")).not.toBeVisible();
  expect(screen.getByText("file-8.md")).toBeVisible();
  fireEvent.click(screen.getByText(/较早活动/));
  expect(screen.getByText("file-0.md")).toBeVisible();
  rerender(<WorkspaceShell {...props} agentRunEventsV4={[...events, { ...base, sequence: 10, event: { kind: "tool_requested", call: { call_id: "last", tool_id: "project.read", arguments: { path: "last.md" } } } }]} />);
  expect(screen.getByText("file-0.md")).toBeVisible();
  expect(screen.getByText("last.md")).toBeVisible();
});

it("omits empty assistant replies and copies the original answer Markdown", async () => {
  const writeText = vi.fn().mockResolvedValue(undefined);
  const originalClipboard = Object.getOwnPropertyDescriptor(navigator, "clipboard");
  Object.defineProperty(navigator, "clipboard", { configurable: true, value: { writeText } });
  try {
    render(<WorkspaceShell project={project} locale="zh-CN" onSend={vi.fn()} messages={[{ id: "empty", role: "assistant", markdown: "  \n " }, { id: "answer", role: "assistant", markdown: "## 结果\n\n**已核验**的内容" }]} />);
    expect(document.querySelector('[data-message-id="empty"]')).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "复制回复" }));
    await waitFor(() => expect(screen.getByRole("button", { name: "已复制" })).toBeVisible());
    expect(writeText).toHaveBeenCalledWith("## 结果\n\n**已核验**的内容");
  } finally {
    if (originalClipboard) Object.defineProperty(navigator, "clipboard", originalClipboard);
    else Reflect.deleteProperty(navigator, "clipboard");
  }
});

it("reports a clipboard failure without removing the answer", async () => {
  const originalClipboard = Object.getOwnPropertyDescriptor(navigator, "clipboard");
  Object.defineProperty(navigator, "clipboard", { configurable: true, value: { writeText: vi.fn().mockRejectedValue(new Error("denied")) } });
  try {
    render(<WorkspaceShell project={project} locale="en-US" onSend={vi.fn()} messages={[{ id: "answer", role: "assistant", markdown: "Verified answer" }]} />);
    fireEvent.click(screen.getByRole("button", { name: "Copy response" }));
    expect(await screen.findByRole("status")).toHaveTextContent("Copy failed");
    expect(screen.getByText("Verified answer")).toBeVisible();
  } finally {
    if (originalClipboard) Object.defineProperty(navigator, "clipboard", originalClipboard);
    else Reflect.deleteProperty(navigator, "clipboard");
  }
});
const compactQuestion: AgentRunEventV4 = { schema_version: 4, project_id: project.id, conversation_id: "compact-c", run_id: "compact-r", sequence: 1, occurred_at: "now", previous_hash: "", event_hash: "h", event: { kind: "input_requested", question_id: "sample", question: "Which sample should continue?" } };
const compactPlan: RunSummaryV4 = { run_id: "compact-r", status: "waiting_for_input", session_mode: "agent", plan: null, plan_hash: null, approval_hash: null, compute_selection: null };
const compactProps = { project, locale: "en-US" as const, activeConversationId: "compact-c", activeRunId: "compact-r", v4Plan: compactPlan, agentRunEventsV4: [compactQuestion], runStarted: true };
function openCompaction() { fireEvent.click(screen.getByRole("button", { name: "Inspect context usage" })); return screen.getByRole("button", { name: "Compact context" }); }
it("manual_compact_button_requires_ordinary_pause", () => {
  const view = render(<WorkspaceShell {...compactProps} />);
  expect(openCompaction()).toBeEnabled();
  for (const patch of [{ status: "running" }, { status: "completed" }, { session_mode: "plan" as const }, { approval_hash: "approved" }]) {
    view.rerender(<WorkspaceShell {...compactProps} v4Plan={{ ...compactPlan, ...patch }} />);
    expect(screen.getByRole("button", { name: "Compact context" })).toBeDisabled();
  }
  view.rerender(<WorkspaceShell {...compactProps} runStopping />); expect(screen.getByRole("button", { name: "Compact context" })).toBeDisabled();
  view.rerender(<WorkspaceShell {...compactProps} agentRunEventsV4={[compactQuestion, { ...compactQuestion, sequence: 2, event: { kind: "user_input_answered", question_id: "sample", answer: "A" } }]} />); expect(screen.getByRole("button", { name: "Compact context" })).toBeDisabled();
});
it("manual_compact_discards_late_scope_and_retains_question", async () => {
  const pending = deferred<ContextCompactionReceiptV4>(); const compact = vi.spyOn(contextApi, "compactContextV4").mockReturnValue(pending.promise);
  const answer = vi.fn(); const resume = vi.fn(); const refresh = vi.fn().mockResolvedValue(undefined);
  const props = { ...compactProps, onAnswerAgentQuestionV4: answer, onResumeAgentRunV4: resume, onRefreshContextUsage: refresh };
  const view = render(<WorkspaceShell {...props} />);
  fireEvent.change(screen.getByRole("textbox", { name: "Answer V4 question" }), { target: { value: "draft answer" } });
  fireEvent.click(openCompaction()); fireEvent.click(screen.getByRole("button", { name: /Compacting/ }));
  expect(compact).toHaveBeenCalledTimes(1); expect(screen.getByText("Which sample should continue?")).toBeVisible();
  expect(screen.getByRole("textbox", { name: "Answer V4 question" })).toHaveValue("draft answer");
  view.rerender(<WorkspaceShell {...props} activeConversationId="new-c" />);
  await act(async () => pending.resolve({ ...compact.mock.calls[0][0], status: "completed", before_bytes: 9000, after_bytes: 1000, source_through_sequence: 1, source_head_hash: "h", created_at: "now", updated_at: "now" }));
  openCompaction(); expect(screen.queryByText(/Context compacted/)).toBeNull();
  expect(answer).not.toHaveBeenCalled(); expect(resume).not.toHaveBeenCalled(); expect(refresh).not.toHaveBeenCalled();
});
it("manual_compact_error_does_not_resume", async () => {
  const compact = vi.spyOn(contextApi, "compactContextV4").mockRejectedValue(new Error("SECRET raw exception"));
  const answer = vi.fn(); const resume = vi.fn();
  render(<WorkspaceShell {...compactProps} onAnswerAgentQuestionV4={answer} onResumeAgentRunV4={resume} />);
  fireEvent.click(openCompaction());
  expect(await screen.findByText(/Compaction could not be confirmed/)).toBeVisible();
  expect(screen.queryByText(/SECRET/)).toBeNull(); expect(screen.getByText("Which sample should continue?")).toBeVisible();
  fireEvent.click(screen.getByRole("button", { name: "Compact context" }));
  await waitFor(() => expect(compact).toHaveBeenCalledTimes(2));
  expect(compact.mock.calls[0][0].request_id).toBe(compact.mock.calls[1][0].request_id);
  expect(answer).not.toHaveBeenCalled(); expect(resume).not.toHaveBeenCalled();
});
it("keeps a confirmed receipt when usage refresh fails and refreshes only once", async () => {
  const compact = vi.spyOn(contextApi, "compactContextV4").mockImplementation(async (request) => ({ ...request, status: "completed", before_bytes: 9000, after_bytes: 1000, source_through_sequence: 1, source_head_hash: "h", created_at: "now", updated_at: "now" }));
  const refresh = vi.fn().mockRejectedValue(new Error("usage unavailable"));
  const answer = vi.fn(); const resume = vi.fn();
  render(<WorkspaceShell {...compactProps} onRefreshContextUsage={refresh} onAnswerAgentQuestionV4={answer} onResumeAgentRunV4={resume} messages={[{ id: "old", role: "assistant", markdown: "Original evidence stays visible" }]} />);
  fireEvent.change(screen.getByRole("textbox", { name: "Answer V4 question" }), { target: { value: "draft retained" } });
  fireEvent.click(openCompaction());
  expect(await screen.findByText(/Context compacted/)).toBeVisible();
  expect(refresh).toHaveBeenCalledTimes(1); expect(compact).toHaveBeenCalledTimes(1);
  expect(screen.queryByText(/Compaction could not be confirmed/)).toBeNull();
  expect(screen.getByText("Original evidence stays visible")).toBeVisible();
  expect(screen.getByRole("textbox", { name: "Answer V4 question" })).toHaveValue("draft retained");
  expect(answer).not.toHaveBeenCalled(); expect(resume).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "Compact context" }));
  await waitFor(() => expect(refresh).toHaveBeenCalledTimes(2));
  expect(compact.mock.calls[1][0].request_id).not.toBe(compact.mock.calls[0][0].request_id);
});
it("blocks compaction for pending approval, unresolved dispatch, multiple questions and driver activity", () => {
  const view = render(<WorkspaceShell {...compactProps} />); openCompaction();
  const blocked: AgentRunEventV4["event"][] = [
    { kind: "tool_approval_requested", request: { approval_id: "approval", call: { call_id: "call", tool_id: "runtime.execute", arguments: {} }, effect: "mutating", reason: "approval", call_hash: "hash" } },
    { kind: "input_requested", question_id: "second", question: "Another?" },
    { kind: "tool_dispatch_started", call_id: "call", tool_id: "runtime.execute", effect: "side_effect", idempotency_key: "key" },
    { kind: "tool_dispatch_uncertain", call_id: "call", tool_id: "runtime.execute" },
  ];
  for (const event of blocked) {
    view.rerender(<WorkspaceShell {...compactProps} agentRunEventsV4={[compactQuestion, { ...compactQuestion, sequence: 2, event }]} />);
    expect(screen.getByRole("button", { name: "Compact context" })).toBeDisabled();
  }
  view.rerender(<WorkspaceShell {...compactProps} agentBusy />); expect(screen.getByRole("button", { name: "Compact context" })).toBeDisabled();
  view.rerender(<WorkspaceShell {...compactProps} conversationHydrating />); expect(screen.getByRole("button", { name: "Compact context" })).toBeDisabled();
});

// Each test represents a distinct durable run; the operation ledger now survives
// component unmounts for the lifetime of this window, as real project navigation does.
beforeEach(() => {
  const runId = crypto.randomUUID();
  compactQuestion.run_id = runId; compactPlan.run_id = runId; compactProps.activeRunId = runId;
});


describe("run diagnostics entry", () => {
  it("opens diagnostics above trajectory and immediately Escape closes only the top layer", () => {
    render(<WorkspaceShell project={project} locale="en-US" activeConversationId="current" />);
    expect(screen.queryByRole("button", { name: "Run diagnostics" })).not.toBeInTheDocument();
    fireEvent.change(screen.getByRole("textbox", { name: /Describe a research goal/ }), { target: { value: "/trajectory" } });
    fireEvent.click(screen.getByRole("option", { name: /\/trajectory/ }));
    const trajectory = screen.getByRole("dialog", { name: "Run trajectory" });
    fireEvent.click(within(trajectory).getByRole("button", { name: "Run diagnostics" }));
    expect(screen.getByRole("dialog", { name: "Run diagnostics" })).toBeVisible();
    fireEvent.keyDown(window, { key: "Escape" });
    expect(screen.queryByRole("dialog", { name: "Run diagnostics" })).not.toBeInTheDocument();
    expect(trajectory).toBeVisible();
  });
});


describe("exact diagnostic source location", () => {
  const source: AgentRunEventV4 = { schema_version: 4, project_id: project.id, conversation_id: "current", run_id: "run-a", sequence: 3, event_hash: "hash-a", previous_hash: "hash-before", occurred_at: "2026-10-08T00:00:00Z", event: { kind: "model_text", text: "PRIVATE_MODEL_TEXT" } };
  const row = { run_id: source.run_id, sequence: source.sequence, event_hash: source.event_hash, previous_hash: source.previous_hash, occurred_at: source.occurred_at, event_kind: "model_text", status: null, tool_label: null, call_ref: null, duration_ms: null };
  const snapshot: import("../../types").RunDiagnosticsSnapshotV4 = { schema_version: 1, project_id: project.id, conversation_id: "current", source_heads: [{ run_id: "run-a", event_count: 3, last_sequence: 3, event_hash: "hash-a" }], filter_summary: { run_id: null, tool_label: null, failures_only: false }, source_chain_validated: true, filtered: false, total_event_count: 3, selected_event_count: 1, events: [row], usage: [] };
  async function openAndLocate() {
    fireEvent.change(screen.getByRole("textbox", { name: /Describe a research goal/ }), { target: { value: "/trajectory" } });
    fireEvent.click(screen.getByRole("option", { name: /\/trajectory/ }));
    fireEvent.click(within(screen.getByRole("dialog", { name: "Run trajectory" })).getByRole("button", { name: "Run diagnostics" }));
    fireEvent.click(screen.getByRole("button", { name: "Refresh diagnostics" }));
    fireEvent.click(await screen.findByRole("button", { name: "Locate event 3" }));
  }
  beforeEach(() => { Object.defineProperty(window, "__TAURI_INTERNALS__", { value: {}, configurable: true }); });
  afterEach(() => { Reflect.deleteProperty(window, "__TAURI_INTERNALS__"); });
  it("focuses the exact cached ModelText metadata outside ActivityWindow and expands ancestors", async () => {
    const get = vi.spyOn(diagnosticsApi, "getRunDiagnostics").mockResolvedValue(snapshot);
    const onResume = vi.fn(); const onAnswer = vi.fn(); const onApproval = vi.fn();
    render(<WorkspaceShell project={project} locale="en-US" activeConversationId="current" agentRunEventsV4={[source, ...Array.from({ length: 16 }, (_, index): AgentRunEventV4 => ({ ...source, sequence: 4 + index, event_hash: `later-${index}`, event: index % 2 ? { kind: "run_spec_frozen", approval_hash: "a", spec_hash: "b" } : { kind: "model_text", text: `Later progress ${index}` } })), { ...source, sequence: 20, event_hash: "last", event: { kind: "model_text", text: "Last progress" } }]} onResumeAgentRunV4={onResume} onAnswerAgentQuestionV4={onAnswer} onDecideToolApprovalV4={onApproval} />);
    await openAndLocate();
    const trajectory = screen.getByRole("dialog", { name: "Run trajectory" });
    const target = trajectory.querySelector('[data-diagnostic-run-id="run-a"][data-diagnostic-sequence="3"][data-diagnostic-event-hash="hash-a"]');
    expect(target).not.toBeNull(); expect(target).toHaveFocus(); expect(target).toHaveTextContent("model_text"); expect(target).not.toHaveTextContent("PRIVATE_MODEL_TEXT");
    expect(trajectory.querySelector(".v4-earlier-activity")).not.toBeNull(); expect(target?.closest(".v4-process-timeline")).toBeNull(); let ancestor = target?.parentElement; while (ancestor) { if (ancestor instanceof HTMLDetailsElement) expect(ancestor.open).toBe(true); ancestor = ancestor.parentElement; }
    expect(get).toHaveBeenCalledOnce(); expect(onResume).not.toHaveBeenCalled(); expect(onAnswer).not.toHaveBeenCalled(); expect(onApproval).not.toHaveBeenCalled();
  });
  it.each(["missing", "hash mismatch"])("refreshes only readonly metadata for %s cache without choosing adjacent sequence", async (mode) => {
    const get = vi.spyOn(diagnosticsApi, "getRunDiagnostics").mockResolvedValue(snapshot);
    const old = vi.spyOn(guidanceApi, "agentV4Events"); const oldContext = vi.spyOn(guidanceApi, "agentV4EventsForConversation");
    render(<WorkspaceShell project={project} locale="en-US" activeConversationId="current" agentRunEventsV4={mode === "missing" ? [] : [{ ...source, event_hash: "wrong-hash" }, { ...source, sequence: 4 }]} />);
    await openAndLocate(); const trajectory = screen.getByRole("dialog", { name: "Run trajectory" });
    await waitFor(() => expect(trajectory.querySelector('[data-diagnostic-event-hash="hash-a"][data-diagnostic-sequence="3"]')).toHaveFocus());
    expect(get).toHaveBeenCalledTimes(2); expect(get).toHaveBeenLastCalledWith({ project_id: project.id, conversation_id: "current", filter: { run_id: null, tool_id: null, failures_only: false }, redaction_keywords: [] });
    expect(old).not.toHaveBeenCalled(); expect(oldContext).not.toHaveBeenCalled(); expect(within(trajectory).getByText(/metadata refreshed/i)).toBeVisible();
  });
  it("reports unavailable when readonly refresh cannot find the exact hash", async () => {
    vi.spyOn(diagnosticsApi, "getRunDiagnostics").mockResolvedValueOnce(snapshot).mockResolvedValueOnce({ ...snapshot, events: [{ ...row, event_hash: "wrong-hash" }, { ...row, sequence: 4 }] });
    render(<WorkspaceShell project={project} locale="en-US" activeConversationId="current" agentRunEventsV4={[]} />); await openAndLocate();
    const trajectory = screen.getByRole("dialog", { name: "Run trajectory" }); await within(trajectory).findByText(/exact source event is unavailable/i);
    expect(trajectory.querySelector('[data-diagnostic-event-hash="hash-a"]')).toBeNull();
  });
  it.each(["project", "conversation", "close", "reopen"])("discards late source refresh after %s", async (change) => {
    let resolve!: (value: typeof snapshot) => void; const pending = new Promise<typeof snapshot>((r) => { resolve = r; });
    vi.spyOn(diagnosticsApi, "getRunDiagnostics").mockResolvedValueOnce(snapshot).mockReturnValueOnce(pending);
    const view = render(<WorkspaceShell project={project} locale="en-US" activeConversationId="current" />); await openAndLocate();
    if (change === "close") fireEvent.keyDown(window, { key: "Escape" });
    else if (change === "reopen") fireEvent.click(within(screen.getByRole("dialog", { name: "Run trajectory" })).getByRole("button", { name: "Run diagnostics" }));
    else view.rerender(<WorkspaceShell project={change === "project" ? { ...project, id: "other" } : project} locale="en-US" activeConversationId={change === "conversation" ? "other" : "current"} />);
    await act(async () => resolve(snapshot));
    expect(document.querySelector('[data-diagnostic-event-hash="hash-a"]')).toBeNull(); expect(screen.queryByText(/metadata refreshed/i)).not.toBeInTheDocument();
  });
});
