import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { WorkspaceShell } from "./WorkspaceShell";
import * as fileApi from "../../composer-file-api";
import * as attachmentApi from "../../composer-attachment-api";
import type { ComposerCatalogItem, ComputeBackendAvailabilityV4 } from "../../types";

afterEach(() => vi.restoreAllMocks());
const project = { id: "file-project", name: "File project", status: "ready" as const, template: "blank" as const, connection_id: "connection-a" };
const backend: ComputeBackendAvailabilityV4 = { descriptor: { schema_version: 4, backend_id: "local", kind: "local", isolation: "process", available: true, supports_python: true, supports_r: false, supports_network_policy: false }, selectable: true, reason: null, python_status: "available", r_status: "unavailable", resolved_image_id: null };
const props = { project, activeConversationId: "conversation", locale: "en-US" as const, computeBackends: [backend], computeBackendId: "local" };
const entry = { relative_path: "counts.csv", directory: false, size_bytes: 12, modified_unix_seconds: 0 };
const fileReference = { kind: "workspace_file" as const, project_id: project.id, backend_id: "ssh:connection-a", relative_path: entry.relative_path };

function input() { return screen.getByRole("textbox", { name: /Describe a research goal/ }); }
function openFiles() {
  fireEvent.click(screen.getByRole("button", { name: "Add context or choose mode" }));
  fireEvent.click(screen.getByRole("menuitem", { name: /Your files/ }));
}

it("prioritizes an internal source reference over OS upload data and preserves its backend", async () => {
  const stage = vi.spyOn(attachmentApi, "stageComposerAttachment");
  const onSend = vi.fn().mockResolvedValue(true);
  render(<WorkspaceShell {...props} onSend={onSend} />);
  fireEvent.change(input(), { target: { value: "Inspect counts" } });
  fireEvent.drop(input(), { dataTransfer: { types: [fileApi.WORKSPACE_FILE_DRAG_TYPE, "Files"], files: [new File(["data"], "counts.csv")], getData: () => JSON.stringify(fileReference) } });
  expect(stage).not.toHaveBeenCalled();
  await waitFor(() => expect(screen.getByRole("button", { name: "Send" })).toBeEnabled());
  fireEvent.click(screen.getByRole("button", { name: "Send" }));
  await waitFor(() => expect(onSend).toHaveBeenCalledWith("Inspect counts", "chat", [fileReference]));
});

it("attaches local and remote file leaves without downloading and refreshes on Your files", async () => {
  const list = vi.spyOn(fileApi, "listLocalComposerFiles").mockResolvedValue([entry]);
  const refresh = vi.fn(); const download = vi.fn();
  render(<WorkspaceShell {...props} remoteFiles={[entry]} onRefreshFiles={refresh} onDownloadFile={download} />);
  openFiles();
  await waitFor(() => expect(refresh).toHaveBeenCalled());
  fireEvent.click(screen.getByRole("button", { name: "Attach reference counts.csv" }));
  expect(download).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "Local" }));
  await waitFor(() => expect(list).toHaveBeenCalledWith(project.id));
  fireEvent.click(await screen.findByRole("button", { name: "Attach reference counts.csv" }));
  expect(screen.getAllByRole("button", { name: "Remove reference: counts.csv" })).toHaveLength(2);
  const calls = list.mock.calls.length;
  openFiles();
  await waitFor(() => expect(list.mock.calls.length).toBe(calls + 1));
});

it("shows local listing loading and failure without claiming the directory is empty, then retries", async () => {
  let fail!: (reason: unknown) => void;
  let finish!: (files: typeof entry[]) => void;
  vi.spyOn(fileApi, "listLocalComposerFiles")
    .mockReturnValueOnce(new Promise((_resolve, reject) => { fail = reject; }))
    .mockReturnValueOnce(new Promise((resolve) => { finish = resolve; }));
  render(<WorkspaceShell {...props} project={{ ...project, connection_id: null }} />);
  openFiles();
  expect(screen.getByText("Loading local directory")).toBeInTheDocument();
  expect(screen.queryByText("The local directory is empty")).not.toBeInTheDocument();
  await act(async () => fail(new Error("PRIVATE_LOCAL_PATH")));
  expect(screen.getByRole("alert")).toHaveTextContent("Could not load the local directory. Refresh to retry.");
  expect(screen.queryByText(/PRIVATE_LOCAL_PATH|The local directory is empty/)).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Refresh local directory" }));
  expect(screen.getByText("Loading local directory")).toBeInTheDocument();
  expect(screen.queryByText("Could not load the local directory. Refresh to retry.")).not.toBeInTheDocument();
  await act(async () => finish([]));
  expect(screen.getByText("The local directory is empty")).toBeInTheDocument();
});

it("passes a safe remote listing error to the file tree separately from a transfer notice", () => {
  render(<WorkspaceShell {...props} locale="zh-CN" remoteFiles={[]} remoteFilesStatus="error" remoteFilesError="remote_root_unavailable" fileNotice="已校验上传 1 个文件" />);
  fireEvent.click(screen.getByRole("button", { name: "添加上下文或选择模式" }));
  fireEvent.click(screen.getByRole("menuitem", { name: /你的文件/ }));
  expect(screen.getByRole("alert")).toHaveTextContent("绑定的远端项目目录不可访问，请检查项目的 SSH 目录和权限。");
  expect(screen.getByText("已校验上传 1 个文件")).toBeInTheDocument();
  expect(screen.queryByText("远端目录为空")).not.toBeInTheDocument();
});

it("ignores a local listing that finishes after changing projects", async () => {
  let finishOld!: (files: typeof entry[]) => void;
  vi.spyOn(fileApi, "listLocalComposerFiles")
    .mockReturnValueOnce(new Promise((resolve) => { finishOld = resolve; }))
    .mockResolvedValueOnce([]);
  const view = render(<WorkspaceShell {...props} project={{ ...project, connection_id: null }} />);
  openFiles();
  view.rerender(<WorkspaceShell {...props} project={{ ...project, id: "other-project", connection_id: null }} />);
  await screen.findByText("The local directory is empty");
  await act(async () => finishOld([entry]));
  expect(screen.queryByText("counts.csv")).not.toBeInTheDocument();
  expect(screen.getByText("The local directory is empty")).toBeInTheDocument();
});

it("blocks send during clipboard path validation and ignores a result from the previous conversation", async () => {
  let finish!: (items: ComposerCatalogItem[]) => void;
  vi.spyOn(fileApi, "resolveComposerClipboardPaths").mockReturnValue(new Promise((resolve) => { finish = resolve; }));
  const onSend = vi.fn();
  const view = render(<WorkspaceShell {...props} onSend={onSend} />);
  fireEvent.change(input(), { target: { value: "Inspect" } });
  fireEvent.paste(input(), { clipboardData: { files: [], getData: () => '"C:\\project\\counts.csv"' } });
  expect(screen.getByRole("button", { name: "Checking paths…" })).toBeDisabled();
  view.rerender(<WorkspaceShell {...props} activeConversationId="another-conversation" onSend={onSend} />);
  await act(async () => finish([{ reference: { ...fileReference, backend_id: "local" }, label: "counts.csv", description: "Local" }]));
  expect(screen.queryByRole("button", { name: "Remove reference: counts.csv" })).not.toBeInTheDocument();
  expect(onSend).not.toHaveBeenCalled();
});

it("rejects cross-project internal drops without clearing the draft", () => {
  render(<WorkspaceShell {...props} />);
  fireEvent.change(input(), { target: { value: "Keep goal" } });
  fireEvent.drop(input(), { dataTransfer: { types: [fileApi.WORKSPACE_FILE_DRAG_TYPE], files: [], getData: () => JSON.stringify({ ...fileReference, project_id: "other-project" }) } });
  expect(input()).toHaveValue("Keep goal");
  expect(screen.queryByRole("button", { name: "Remove reference: counts.csv" })).not.toBeInTheDocument();
  expect(screen.getByRole("alert")).toHaveTextContent("Only files from this project");
});

it("previews explicitly and submits a stable quote while preserving the research draft", async () => {
  const textEntry = { ...entry, relative_path: "notes.md" };
  const textReference = { ...fileReference, relative_path: textEntry.relative_path };
  const preview = vi.spyOn(fileApi, "previewComposerFileText").mockResolvedValue({ project_id: project.id, backend_id: textReference.backend_id, relative_path: textEntry.relative_path, sha256: "a".repeat(64), text: "Selected evidence\nOther text" });
  const quote: ComposerCatalogItem = { reference: { kind: "quote", project_id: project.id, id: "quote-a" }, label: "SSH notes.md", description: "Selected evidence" };
  const create = vi.spyOn(fileApi, "createComposerQuote").mockResolvedValue(quote);
  const onSend = vi.fn().mockResolvedValue(true);
  render(<WorkspaceShell {...props} remoteFiles={[textEntry]} onSend={onSend} />);
  fireEvent.change(input(), { target: { value: "Assess evidence" } });
  openFiles();
  expect(preview).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "Preview text notes.md" }));
  const text = await screen.findByRole("textbox", { name: "File text" });
  (text as HTMLTextAreaElement).setSelectionRange(0, 17);
  fireEvent.select(text);
  fireEvent.click(screen.getByRole("button", { name: "Quote selected text" }));
  await waitFor(() => expect(create).toHaveBeenCalledWith({ project_id: project.id, conversation_id: props.activeConversationId, backend_id: textReference.backend_id, relative_path: textEntry.relative_path, sha256: "a".repeat(64), text: "Selected evidence" }));
  await screen.findByRole("button", { name: "Remove reference: SSH notes.md" });
  expect(input()).toHaveValue("Assess evidence");
  fireEvent.click(screen.getByRole("button", { name: "Send" }));
  await waitFor(() => expect(onSend).toHaveBeenCalledWith("Assess evidence", "chat", [quote.reference]));
});
