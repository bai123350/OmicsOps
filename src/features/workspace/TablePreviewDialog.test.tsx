import { useState } from "react";
import { act, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import * as fileApi from "../../composer-file-api";
import type { ProjectTablePreview } from "../../types";
import { useWindowEscapeLayer } from "../settings/BrowserSettings";
import { TablePreviewDialog } from "./TablePreviewDialog";

afterEach(() => vi.restoreAllMocks());
const reference = { kind: "workspace_file" as const, project_id: "p", backend_id: "local", relative_path: "data.csv" };
const fixture = (overrides: Partial<ProjectTablePreview> = {}): ProjectTablePreview => ({ project_id: "p", backend_id: "local", relative_path: "data.csv", size_bytes: 500000, read_bytes: 262144, parsed_bytes: 250000, sha256: "raw-source-hash", hash_scope: "prefix", headers: ["gene", "value"], rows: [["A", "=1+1"]], displayed_rows: 1, source_stability: "unknown", truncation_reasons: ["byte_limit", "row_limit", "cell_limit", "incomplete_record", "incomplete_utf8"], ...overrides });
function pending<T>() { let resolve!: (value: T) => void; let reject!: (error: unknown) => void; const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; }); return { promise, resolve, reject }; }

it.each(["en-US", "zh-CN"] as const)("shows literal cells, bounded source accounting and every limit in %s", async (locale) => {
  const html = '<script data-cell>alert(1)</script><img src="https://invalid.test/x">';
  vi.spyOn(fileApi, "previewProjectTable").mockResolvedValue(fixture({ rows: [[html, "=1+1"]] }));
  const quote = vi.spyOn(fileApi, "createComposerQuote");
  const text = vi.spyOn(fileApi, "previewComposerFileText");
  render(<TablePreviewDialog reference={reference} locale={locale} onClose={vi.fn()} />);
  const table = await screen.findByRole("table");
  expect(within(table).getByRole("columnheader", { name: "gene" })).toBeInTheDocument();
  expect(within(table).getByRole("cell", { name: html })).toBeInTheDocument();
  expect(within(table).getByRole("cell", { name: "=1+1" })).toBeInTheDocument();
  expect(document.querySelector("script[data-cell], img, iframe")).toBeNull();
  const dialog = screen.getByRole("dialog");
  for (const value of ["500000", "262144", "250000", "raw-source-hash", "data.csv"]) expect(dialog).toHaveTextContent(value);
  for (const label of locale === "zh-CN" ? ["仅前缀", "稳定性未知", "字节上限", "行数上限", "单元格显示截断", "不完整记录", "不完整 UTF-8"] : ["Prefix only", "Stability unknown", "Byte limit", "Row limit", "Cell display truncated", "Incomplete record", "Incomplete UTF-8"]) expect(dialog).toHaveTextContent(label);
  expect(dialog).toHaveTextContent(locale === "zh-CN" ? "已显示行数：1" : "Displayed rows: 1");
  expect(quote).not.toHaveBeenCalled(); expect(text).not.toHaveBeenCalled();
  expect(screen.queryByRole("textbox")).not.toBeInTheDocument();
});

it.each([{ size: 0, label: "0 bytes" }, { size: null, label: "Unknown" }])("distinguishes empty size $size from unavailable metadata", async ({ size, label }) => {
  vi.spyOn(fileApi, "previewProjectTable").mockResolvedValue(fixture({ size_bytes: size, read_bytes: 0, parsed_bytes: 0, hash_scope: "full", headers: [], rows: [], displayed_rows: 0, source_stability: "checked", truncation_reasons: [] }));
  render(<TablePreviewDialog reference={reference} locale="en-US" onClose={vi.fn()} />);
  expect(await screen.findByText("Empty table")).toBeInTheDocument();
  expect(screen.getByLabelText("Source size")).toHaveTextContent(label);
  expect(screen.getByRole("dialog")).toHaveTextContent("Full source");
  expect(screen.getByRole("dialog")).toHaveTextContent("Stability checked");
});

it("keeps header-only tables visible with no data rows", async () => {
  vi.spyOn(fileApi, "previewProjectTable").mockResolvedValue(fixture({ rows: [], displayed_rows: 0, truncation_reasons: [] }));
  render(<TablePreviewDialog reference={reference} locale="en-US" onClose={vi.fn()} />);
  expect(await screen.findByRole("columnheader", { name: "gene" })).toBeInTheDocument();
  expect(screen.getByText("No data rows")).toBeInTheDocument();
});

it.each(["timeout", "invalid_path", "unsupported_format", "invalid_utf8", "malformed_table", "column_limit", "source_changed", "read_failed", "unavailable_backend", "PRIVATE /server/key secret", new Error("PRIVATE secret")])("shows safe errors and retries: %s", async (error) => {
  const preview = vi.spyOn(fileApi, "previewProjectTable").mockRejectedValueOnce(error).mockResolvedValueOnce(fixture());
  render(<TablePreviewDialog reference={reference} locale="en-US" onClose={vi.fn()} />);
  expect(screen.getByRole("status")).toHaveTextContent("Loading table preview");
  expect(await screen.findByRole("alert")).not.toHaveTextContent("PRIVATE");
  fireEvent.click(screen.getByRole("button", { name: "Retry preview" }));
  await screen.findByRole("table");
  expect(preview).toHaveBeenCalledTimes(2);
});

it.each(["file", "backend", "project"])("drops late responses after changing %s", async (change) => {
  const old = pending<ProjectTablePreview>(); const next = pending<ProjectTablePreview>();
  vi.spyOn(fileApi, "previewProjectTable").mockReturnValueOnce(old.promise).mockReturnValueOnce(next.promise);
  const view = render(<TablePreviewDialog reference={reference} locale="en-US" onClose={vi.fn()} />);
  const changed = { ...reference, ...(change === "file" ? { relative_path: "next.tsv" } : change === "backend" ? { backend_id: "ssh:c" } : { project_id: "p2" }) };
  view.rerender(<TablePreviewDialog reference={changed} locale="en-US" onClose={vi.fn()} />);
  await act(async () => old.resolve(fixture({ rows: [["OLD", "old"]] })));
  expect(screen.queryByRole("table")).not.toBeInTheDocument();
  await act(async () => next.resolve(fixture({ ...changed, rows: [["NEW", "new"]] })));
  expect(screen.getByRole("cell", { name: "NEW" })).toBeInTheDocument();
  expect(screen.queryByText("OLD")).not.toBeInTheDocument();
});

it("rejects a response bound to another source", async () => {
  vi.spyOn(fileApi, "previewProjectTable").mockResolvedValue(fixture({ backend_id: "ssh:other" }));
  render(<TablePreviewDialog reference={reference} locale="en-US" onClose={vi.fn()} />);
  await screen.findByRole("alert"); expect(screen.queryByRole("table")).not.toBeInTheDocument();
});

it("does not revive an earlier request when switching away and back to the same source", async () => {
  const first = pending<ProjectTablePreview>(); const second = pending<ProjectTablePreview>(); const third = pending<ProjectTablePreview>();
  vi.spyOn(fileApi, "previewProjectTable").mockReturnValueOnce(first.promise).mockReturnValueOnce(second.promise).mockReturnValueOnce(third.promise);
  const view = render(<TablePreviewDialog reference={reference} locale="en-US" onClose={vi.fn()} />);
  view.rerender(<TablePreviewDialog reference={{ ...reference, relative_path: "other.csv" }} locale="en-US" onClose={vi.fn()} />);
  view.rerender(<TablePreviewDialog reference={reference} locale="en-US" onClose={vi.fn()} />);
  await act(async () => first.resolve(fixture({ rows: [["OLD", "old"]] })));
  expect(screen.queryByRole("table")).not.toBeInTheDocument();
  await act(async () => second.reject("timeout"));
  expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  await act(async () => third.resolve(fixture({ rows: [["NEW", "new"]] })));
  expect(screen.getByRole("cell", { name: "NEW" })).toBeInTheDocument();
});

it.each(["en-US", "zh-CN"] as const)("localizes the known timeout without leaking an unknown host failure in %s", async (locale) => {
  vi.spyOn(fileApi, "previewProjectTable").mockRejectedValueOnce("timeout").mockRejectedValueOnce("timeout PRIVATE /server/key");
  render(<TablePreviewDialog reference={reference} locale={locale} onClose={vi.fn()} />);
  expect(await screen.findByRole("alert")).toHaveTextContent(locale === "zh-CN" ? "表格预览超时" : "The table preview timed out");
  fireEvent.click(screen.getByRole("button", { name: locale === "zh-CN" ? "重试预览" : "Retry preview" }));
  const alert = await screen.findByRole("alert");
  expect(alert).toHaveTextContent(locale === "zh-CN" ? "无法加载表格预览" : "Could not load the table preview");
  expect(alert).not.toHaveTextContent("PRIVATE");
});

it("closes only the top layer immediately on window Escape and restores focus", () => {
  vi.spyOn(fileApi, "previewProjectTable").mockReturnValue(pending<ProjectTablePreview>().promise);
  const onClose = vi.fn(); const closeParent = vi.fn();
  function Parent() { const [open, setOpen] = useState(false); useWindowEscapeLayer(true, closeParent); return <div role="dialog" aria-label="Parent"><button onClick={() => setOpen(true)}>Open table</button>{open && <TablePreviewDialog reference={reference} locale="en-US" onClose={() => { onClose(); setOpen(false); }} />}</div>; }
  render(<Parent />); const opener = screen.getByRole("button", { name: "Open table" }); opener.focus(); fireEvent.click(opener);
  const close = screen.getByRole("button", { name: "Close table preview" }); expect(close).toHaveFocus();
  fireEvent.keyDown(close, { key: "Tab" }); expect(close).toHaveFocus();
  fireEvent.keyDown(window, { key: "Escape" }); expect(onClose).toHaveBeenCalledTimes(1); expect(closeParent).not.toHaveBeenCalled(); expect(opener).toHaveFocus();
  expect(screen.getByRole("dialog", { name: "Parent" })).toBeInTheDocument();
  fireEvent.keyDown(window, { key: "Escape" }); expect(closeParent).toHaveBeenCalledTimes(1);
});
