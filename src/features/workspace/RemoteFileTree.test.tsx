import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { RemoteFileTree, type FileListingError } from "./RemoteFileTree";

const files = [
  { relative_path: "src", directory: true, size_bytes: 0, modified_unix_seconds: 0 },
  { relative_path: "src/scanpy_workflow_v3.py", directory: false, size_bytes: 7168, modified_unix_seconds: 0 },
  { relative_path: "results/scrna_qc_annotation/summary.json", directory: false, size_bytes: 2048, modified_unix_seconds: 0 },
  { relative_path: "README.md", directory: false, size_bytes: 512, modified_unix_seconds: 0 },
];

it("selects only explicit baseline file leaves without attachment, transfer or directory selection", () => {
  const select = vi.fn(); const attach = vi.fn(); const download = vi.fn();
  render(<RemoteFileTree locale="en-US" remoteFiles={files} busy={false} onToggleBaselineFile={select} baselineSelectedPaths={["README.md"]} onAttach={attach} onDownload={download} />);
  expect(screen.getByRole("checkbox", { name: "Select for baseline README.md" })).toBeChecked();
  expect(screen.queryByRole("checkbox", { name: "Select for baseline src" })).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole("treeitem", { name: "Expand folder：src" }));
  expect(select).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("checkbox", { name: "Select for baseline src/scanpy_workflow_v3.py" }));
  expect(select).toHaveBeenCalledWith("src/scanpy_workflow_v3.py");
  expect(attach).not.toHaveBeenCalled(); expect(download).not.toHaveBeenCalled();
});

it("routes only CSV and TSV preview actions to the table while keeping text, attachment and drag separate", () => {
  const onPreviewTable = vi.fn(); const onPreviewText = vi.fn(); const onAttach = vi.fn();
  const reference = { kind: "workspace_file" as const, project_id: "p", backend_id: "ssh:c", relative_path: "data/A.CSV" };
  render(<RemoteFileTree locale="en-US" remoteFiles={[...files, ...["data/A.CSV", "data/B.tSv", "data/C.xlsx"].map((relative_path) => ({ relative_path, directory: false, size_bytes: 9, modified_unix_seconds: 0 }))]} busy={false} onPreviewTable={onPreviewTable} onPreviewText={onPreviewText} onAttach={onAttach} dragReference={() => reference} />);
  fireEvent.click(screen.getByRole("treeitem", { name: "Expand folder：data" }));
  fireEvent.click(screen.getByRole("button", { name: "Preview table data/A.CSV" }));
  fireEvent.click(screen.getByRole("button", { name: "Preview table data/B.tSv" }));
  expect(onPreviewTable.mock.calls).toEqual([["data/A.CSV"], ["data/B.tSv"]]);
  expect(onPreviewText).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "Preview text data/C.xlsx" }));
  expect(onPreviewText).toHaveBeenCalledWith("data/C.xlsx");
  fireEvent.click(screen.getByRole("button", { name: "Attach reference data/A.CSV" }));
  expect(onAttach).toHaveBeenCalledWith("data/A.CSV");
  const setData = vi.fn();
  fireEvent.dragStart(screen.getByText("A.CSV").closest('[role="treeitem"]')!, { dataTransfer: { setData } });
  expect(setData).toHaveBeenCalledWith("application/x-omicsops-workspace-file", JSON.stringify(reference));
  expect(screen.queryByRole("button", { name: "Preview text data/A.CSV" })).not.toBeInTheDocument();
});

describe("RemoteFileTree", () => {
  it("renders remote paths as a collapsed explorer hierarchy", () => {
    render(<RemoteFileTree locale="zh-CN" remoteFiles={files} busy={false} />);

    expect(screen.getByRole("tree", { name: "远端项目文件" })).toBeInTheDocument();
    expect(screen.getByRole("treeitem", { name: "展开文件夹：src" })).toHaveAttribute("aria-expanded", "false");
    expect(screen.getByText("README.md")).toBeInTheDocument();
    expect(screen.queryByText("scanpy_workflow_v3.py")).not.toBeInTheDocument();
    expect(screen.queryByText("src/scanpy_workflow_v3.py")).not.toBeInTheDocument();
  });

  it("expands synthesized nested folders and downloads with the full relative path", () => {
    const onDownload = vi.fn();
    render(<RemoteFileTree locale="zh-CN" remoteFiles={files} busy={false} onDownload={onDownload} />);

    fireEvent.click(screen.getByRole("treeitem", { name: "展开文件夹：results" }));
    fireEvent.click(screen.getByRole("treeitem", { name: "展开文件夹：scrna_qc_annotation" }));
    expect(screen.getByText("summary.json")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "下载 results/scrna_qc_annotation/summary.json" }));
    expect(onDownload).toHaveBeenCalledWith("results/scrna_qc_annotation/summary.json");

    fireEvent.click(screen.getByRole("treeitem", { name: "折叠文件夹：results" }));
    expect(screen.queryByText("summary.json")).not.toBeInTheDocument();
  });
});

it("attaches and drags a file reference without downloading or exposing directory actions", () => {
  const onAttach = vi.fn(); const onDownload = vi.fn();
  const reference = { kind: "workspace_file" as const, project_id: "project-a", backend_id: "local", relative_path: "README.md" };
  render(<RemoteFileTree locale="en-US" source="local" remoteFiles={files} busy={false} onAttach={onAttach} onDownload={onDownload} dragReference={() => reference} />);
  fireEvent.click(screen.getByRole("button", { name: "Attach reference README.md" }));
  expect(onAttach).toHaveBeenCalledWith("README.md");
  expect(onDownload).not.toHaveBeenCalled();
  expect(screen.queryByRole("button", { name: "Attach reference src" })).not.toBeInTheDocument();
  const setData = vi.fn();
  fireEvent.dragStart(screen.getByText("README.md").closest('[role="treeitem"]')!, { dataTransfer: { setData } });
  expect(setData).toHaveBeenCalledWith("application/x-omicsops-workspace-file", JSON.stringify(reference));
});

it("does not invent file entries when no listing is provided", () => {
  render(<RemoteFileTree locale="en-US" busy={false} />);
  expect(screen.queryByRole("tree")).not.toBeInTheDocument();
  expect(screen.queryByText("The remote directory is empty")).not.toBeInTheDocument();
  expect(screen.getByRole("status")).toHaveTextContent("Directory not loaded");
});

it.each([
  { source: "local", locale: "en-US", loading: "Loading local directory", error: "Could not load the local directory. Refresh to retry.", empty: "The local directory is empty", refresh: "Refresh local directory" },
  { source: "remote", locale: "en-US", loading: "Loading remote directory", error: "Could not load the remote directory. Refresh to retry.", empty: "The remote directory is empty", refresh: "Refresh remote directory" },
  { source: "local", locale: "zh-CN", loading: "正在加载本地目录", error: "本地目录加载失败，请刷新重试。", empty: "本地目录为空", refresh: "刷新本地目录" },
  { source: "remote", locale: "zh-CN", loading: "正在加载远端目录", error: "远端目录加载失败，请刷新重试。", empty: "远端目录为空", refresh: "刷新远端目录" },
] as const)("shows an empty $source directory only after successful loading in $locale", ({ source, locale, loading, error, empty, refresh }) => {
  const props = { source, locale, remoteFiles: [], busy: false, onRefresh: vi.fn() };
  const { rerender } = render(<RemoteFileTree {...props} listingStatus="loading" />);
  expect(screen.getByRole("status")).toHaveTextContent(loading);
  expect(screen.queryByText(empty)).not.toBeInTheDocument();

  rerender(<RemoteFileTree {...props} listingStatus="error" />);
  expect(screen.getByRole("alert")).toHaveTextContent(error);
  expect(screen.queryByText(empty)).not.toBeInTheDocument();
  expect(screen.getByRole("button", { name: refresh })).toBeEnabled();

  rerender(<RemoteFileTree {...props} listingStatus="ready" />);
  expect(screen.getByText(empty)).toBeInTheDocument();
  expect(screen.queryByRole("alert")).not.toBeInTheDocument();

  rerender(<RemoteFileTree {...props} listingStatus="idle" />);
  expect(screen.queryByText(empty)).not.toBeInTheDocument();
});

it("treats a supplied empty listing as ready without interpreting transfer notices as listing errors", () => {
  render(<RemoteFileTree locale="en-US" remoteFiles={[]} busy={false} notice="Upload failed. Retry the transfer." />);
  expect(screen.getByText("The remote directory is empty")).toBeInTheDocument();
  expect(screen.getByRole("status")).toHaveTextContent("Upload failed. Retry the transfer.");
  expect(screen.queryByRole("alert")).not.toBeInTheDocument();
});

it("preserves listed files but disables file actions while refreshing", () => {
  const action = vi.fn();
  const reference = { kind: "workspace_file" as const, project_id: "p", backend_id: "ssh:c", relative_path: "README.md" };
  render(<RemoteFileTree locale="en-US" remoteFiles={files} listingStatus="loading" busy={false} onRefresh={action} onUpload={action} onDownload={action} onPreviewText={action} onAttach={action} onToggleBaselineFile={action} dragReference={() => reference} />);

  expect(screen.getByText("README.md")).toBeInTheDocument();
  for (const name of ["Refresh remote directory", "Choose files to upload", "Download README.md", "Preview text README.md", "Attach reference README.md"]) {
    expect(screen.getByRole("button", { name })).toBeDisabled();
  }
  expect(screen.getByRole("checkbox", { name: "Select for baseline README.md" })).toBeDisabled();
  const file = screen.getByText("README.md").closest('[role="treeitem"]')!;
  expect(file).toHaveAttribute("draggable", "false");
  const setData = vi.fn();
  fireEvent.dragStart(file, { dataTransfer: { setData } });
  expect(setData).not.toHaveBeenCalled();
});

it("keeps existing files and transfer notice separate from a listing failure, then clears it when ready", () => {
  const props = { locale: "en-US" as const, remoteFiles: files, busy: false, notice: "Upload completed", onRefresh: vi.fn(), listingError: "remote_root_unavailable" as const };
  const { rerender } = render(<RemoteFileTree {...props} listingStatus="error" />);

  expect(screen.getByText("README.md")).toBeInTheDocument();
  expect(screen.getByRole("status")).toHaveTextContent("Upload completed");
  expect(screen.getByRole("alert")).toHaveTextContent("The bound remote project directory is inaccessible. Check the project's SSH directory and permissions.");
  expect(screen.queryByText("The remote directory is empty")).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Refresh remote directory" }));
  expect(props.onRefresh).toHaveBeenCalledOnce();

  rerender(<RemoteFileTree {...props} listingStatus="ready" />);
  expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  expect(screen.getByRole("status")).toHaveTextContent("Upload completed");
});

it.each([
  { locale: "zh-CN", listingError: "remote_root_unavailable", expected: "绑定的远端项目目录不可访问，请检查项目的 SSH 目录和权限。" },
  { locale: "en-US", listingError: "remote_root_unavailable", expected: "The bound remote project directory is inaccessible. Check the project's SSH directory and permissions." },
  { locale: "zh-CN", listingError: "remote_root_resolution_failed", expected: "无法解析远端项目目录，请检查目录配置和服务器工具。" },
  { locale: "en-US", listingError: "remote_root_resolution_failed", expected: "Could not resolve the remote project directory. Check the directory configuration and server tools." },
  { locale: "zh-CN", listingError: "remote_listing_failed", expected: "远端目录加载失败，请刷新重试。" },
  { locale: "en-US", listingError: "remote_listing_failed", expected: "Could not load the remote directory. Refresh to retry." },
] as const)("shows safe guidance for $listingError in $locale", ({ locale, listingError, expected }) => {
  render(<RemoteFileTree locale={locale} source="remote" remoteFiles={[]} busy={false} listingStatus="error" listingError={listingError} />);
  expect(screen.getByRole("alert")).toHaveTextContent(expected);
  expect(screen.queryByText(listingError)).not.toBeInTheDocument();
});

it.each([
  { locale: "zh-CN", expected: "本地目录加载失败，请刷新重试。" },
  { locale: "en-US", expected: "Could not load the local directory. Refresh to retry." },
] as const)("uses local retry guidance even when passed a remote error classification in $locale", ({ locale, expected }) => {
  render(<RemoteFileTree locale={locale} source="local" remoteFiles={[]} busy={false} listingStatus="error" listingError="remote_root_unavailable" />);
  expect(screen.getByRole("alert")).toHaveTextContent(expected);
});

it("never renders an unexpected raw listing error received at runtime", () => {
  const unexpectedError = "ssh stderr: private-server-path" as FileListingError;
  render(<RemoteFileTree locale="en-US" source="remote" remoteFiles={[]} busy={false} listingStatus="error" listingError={unexpectedError} />);
  expect(screen.getByRole("alert")).toHaveTextContent("Could not load the remote directory. Refresh to retry.");
  expect(screen.queryByText(unexpectedError)).not.toBeInTheDocument();
});

it("reads text only through the explicit preview action", () => {
  const onPreviewText = vi.fn(); const onAttach = vi.fn();
  render(<RemoteFileTree locale="en-US" source="local" remoteFiles={files} busy={false} onPreviewText={onPreviewText} onAttach={onAttach} />);
  fireEvent.click(screen.getByRole("button", { name: "Attach reference README.md" }));
  expect(onPreviewText).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "Preview text README.md" }));
  expect(onPreviewText).toHaveBeenCalledWith("README.md");
  expect(screen.queryByRole("button", { name: "Preview text src" })).not.toBeInTheDocument();
});
