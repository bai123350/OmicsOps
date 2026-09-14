import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { RemoteFileTree } from "./RemoteFileTree";

const files = [
  { relative_path: "src", directory: true, size_bytes: 0, modified_unix_seconds: 0 },
  { relative_path: "src/scanpy_workflow_v3.py", directory: false, size_bytes: 7168, modified_unix_seconds: 0 },
  { relative_path: "results/scrna_qc_annotation/summary.json", directory: false, size_bytes: 2048, modified_unix_seconds: 0 },
  { relative_path: "README.md", directory: false, size_bytes: 512, modified_unix_seconds: 0 },
];

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
