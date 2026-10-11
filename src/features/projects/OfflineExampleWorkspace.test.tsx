import { fireEvent, render, screen, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import fixture from "../../../examples/offline/synthetic-evidence-report-v1.json";
import type { OfflineExampleWorkspaceView } from "../../types";
import { OfflineExampleWorkspace } from "./OfflineExampleWorkspace";
import * as offlineApi from "../../offline-example-api";

export function exampleView(projectId = "example-project"): OfflineExampleWorkspaceView {
  return {
    schema_version: 1, project_id: projectId, example_id: fixture.example_id,
    source_kind: "synthetic", availability: "available", unavailable_reason: null,
    title: fixture.title, disclaimer: fixture.disclaimer, source_description: fixture.source_description,
    license_spdx: fixture.license_spdx, fixture_sha256: "fixture-hash",
    entries: fixture.entries as OfflineExampleWorkspaceView["entries"],
    files: Object.entries({ ...fixture.files, "SOURCE.json": "{\"source_kind\":\"synthetic\"}", "LICENSE.txt": "AGPL-3.0-only" }).map(([relative_path, preview_text]) => ({
      relative_path, preview_text, media_type: "text/plain", availability: "available", expected_sha256: "expected", current_sha256: "expected",
    })),
  };
}

describe("OfflineExampleWorkspace", () => {
  it.each(["zh-CN", "en-US"] as const)("shows four preset sections and seven literal file previews in %s", (locale) => {
    const workspace = exampleView(); const onBack = vi.fn();
    const { container } = render(<OfflineExampleWorkspace workspace={workspace} locale={locale} onBack={onBack} />);
    expect(screen.getByRole("heading", { level: 1 })).toHaveTextContent(locale === "zh-CN" ? workspace.title.zh : workspace.title.en);
    expect(screen.getByText(locale === "zh-CN" ? workspace.disclaimer.zh : workspace.disclaimer.en)).toBeInTheDocument();
    expect(container.querySelectorAll("[data-example-entry]")).toHaveLength(4);
    const files = screen.getByRole("navigation", { name: locale === "zh-CN" ? "示例文件" : "Example files" });
    expect(within(files).getAllByRole("button")).toHaveLength(7);
    fireEvent.click(within(files).getByRole("button", { name: /results\/summary.csv/ }));
    expect(screen.getByRole("region", { name: locale === "zh-CN" ? "文本预览" : "Text preview" })).toHaveTextContent("A_mean,3 B_mean,6 B_minus_A,3");
    fireEvent.click(screen.getByRole("button", { name: locale === "zh-CN" ? "返回项目库" : "Back to projects" }));
    expect(onBack).toHaveBeenCalledOnce();
  });

  it("renders changed user text literally without remote resource elements or network", () => {
    const workspace = exampleView(); const fetch = vi.spyOn(window, "fetch").mockRejectedValue(new Error("network forbidden"));
    const markup = '<img src="https://remote.invalid/image"><script src="https://remote.invalid/script"></script><iframe src="https://remote.invalid/frame"></iframe> ![image](https://remote.invalid/md) [link](https://remote.invalid/link)';
    workspace.files[0] = { ...workspace.files[0], availability: "changed", current_sha256: "user-hash", preview_text: markup };
    workspace.files[1] = { ...workspace.files[1], availability: "missing", current_sha256: null, preview_text: null };
    workspace.files[2] = { ...workspace.files[2], availability: "unavailable", current_sha256: null, preview_text: null };
    const { container } = render(<OfflineExampleWorkspace workspace={workspace} locale="en-US" onBack={() => {}} />);
    fireEvent.click(screen.getByRole("button", { name: /data\/observations.csv/ }));
    expect(screen.getByRole("region", { name: "Text preview" })).toHaveTextContent(markup);
    expect(screen.getByText(/Current user text/)).toBeInTheDocument();
    expect(screen.getAllByText(/Changed/).length).toBeGreaterThan(0);
    expect(screen.getByRole("button", { name: /Missing/ })).toBeDisabled();
    expect(screen.getByRole("button", { name: /Unavailable/ })).toBeDisabled();
    expect(container.querySelector("img, script, iframe, a, link, style")).toBeNull();
    expect(fetch).not.toHaveBeenCalled(); fetch.mockRestore();
  });

  it.each(["receipt_missing", "receipt_invalid", "root_unavailable", "identity_mismatch"] as const)("retains synthetic identity and disables all previews for %s", (unavailable_reason) => {
    const workspace = { ...exampleView(), availability: "unavailable" as const, unavailable_reason };
    render(<OfflineExampleWorkspace workspace={workspace} locale="zh-CN" onBack={() => {}} />);
    expect(screen.getByText(workspace.disclaimer.zh)).toBeInTheDocument();
    expect(screen.getByRole("alert")).toHaveTextContent("不可用");
    expect(screen.queryByRole("region", { name: "文本预览" })).not.toBeInTheDocument();
    expect(screen.queryByRole("navigation", { name: "示例文件" })).not.toBeInTheDocument();
  });
});

describe("offline example API", () => {
  it("uses only the two registered commands and propagates marker errors without None fallback", async () => {
    const original = Object.getOwnPropertyDescriptor(window, "__TAURI_INTERNALS__");
    const invoke = vi.fn();
    Object.defineProperty(window, "__TAURI_INTERNALS__", { configurable: true, value: { invoke } });
    try {
      const workspace = exampleView(); const request = { request_id: "request-uuid", example_id: workspace.example_id, parent_directory: "E:/Examples" };
      invoke.mockResolvedValueOnce({ project_id: workspace.project_id, workspace });
      expect(await offlineApi.createOfflineExampleWorkspace(request)).toEqual({ project_id: workspace.project_id, workspace });
      expect(invoke).toHaveBeenNthCalledWith(1, "create_offline_example_workspace", { request }, undefined);
      invoke.mockResolvedValueOnce(workspace);
      expect(await offlineApi.getOfflineExampleWorkspace(workspace.project_id)).toEqual(workspace);
      expect(invoke).toHaveBeenNthCalledWith(2, "get_offline_example_workspace", { projectId: workspace.project_id }, undefined);
      invoke.mockRejectedValueOnce("offline_example_marker_invalid");
      await expect(offlineApi.getOfflineExampleWorkspace(workspace.project_id)).rejects.toBe("offline_example_marker_invalid");
      expect(invoke).toHaveBeenCalledTimes(3);
    } finally { if (original) Object.defineProperty(window, "__TAURI_INTERNALS__", original); else Reflect.deleteProperty(window, "__TAURI_INTERNALS__"); }
  });

  it("fails explicitly outside the desktop host rather than claiming a marker is absent", async () => {
    await expect(offlineApi.getOfflineExampleWorkspace("unknown")).rejects.toThrow("example_platform_unavailable");
  });
});
