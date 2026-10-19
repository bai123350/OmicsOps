import { act, fireEvent, render, screen, within } from "@testing-library/react";
import { beforeEach, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { WorkspacePublicationPage } from "./WorkspacePublicationPage";
import { ResearchDialog } from "./ResearchPagePrimitives";
import type { PublicationDetail, PublicationReadinessReportV1, PublicationSelectionV1 } from "../../workspace-navigation-types";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
const host = vi.mocked(invoke);
const selection: PublicationSelectionV1 = { project_id: "p1", publication_id: "pub1", revision: 2, expected_revision_sha256: "b".repeat(64) };
const detail: PublicationDetail = {
  publication: { id: "pub1", project_id: "p1", title: "Study", revision: 2, updated_at: "2026-10-13T00:00:00Z" },
  revisions: [2, 1].map((revision) => ({ id: `rev${revision}`, publication_id: "pub1", revision, title: "Study", markdown: "Saved body", references: [], sha256: (revision === 2 ? "b" : "a").repeat(64), created_at: "2026-10-13T00:00:00Z", legacy: revision === 1 })),
};
const report: PublicationReadinessReportV1 = {
  schema_version: 1, selection, revision_hash_scope: "stored_revision_content", observed_at: "2026-10-13T01:00:00Z",
  scientific_revision: 3, scientific_state_sha256: "d".repeat(64), references_checked: 1, related_records_checked: 2,
  attention_count: 1, unverified_count: 1, truncated: true, reproduction_status: "not_executed",
  findings: [
    { code: "source_changed", severity: "attention", source: null, related_record_id: null, message: "Saved source has changed" },
    { code: "inspection_limit", severity: "unverified", source: null, related_record_id: null, message: "Some related records were not inspected" },
  ],
};
function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (value: unknown) => void;
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}
const props = { page: "publication" as const, projectId: "p1", locale: "en-US" as const, zh: false, onOpenConversation: vi.fn(), onInsert: vi.fn() };
function page(projectId = "p1") { return <WorkspacePublicationPage {...props} projectId={projectId} />; }
async function open() {
  const study = await screen.findByRole("button", { name: "Study" });
  await act(async () => { fireEvent.click(study); });
  expect(await screen.findByRole("button", { name: "Check saved revision 2" })).toBeEnabled();
  expect(screen.getByRole("button", { name: "Export saved revision 2 reference package" })).toBeEnabled();
}
beforeEach(() => {
  Object.defineProperty(window, "__TAURI_INTERNALS__", { configurable: true, value: {} });
  host.mockReset();
  host.mockImplementation(async (command) => {
    if (command === "workspace_list_publications") return [detail.publication];
    if (command === "workspace_get_publication") return detail;
    if (command === "workspace_check_publication_readiness") return report;
    if (command === "workspace_export_publication_reference_package" || command === "workspace_export_publication") return null;
    throw new Error(`Unexpected command ${command}`);
  });
});

it("saved_revision_and_generation: dirty text is excluded while the saved report describes limits and hash scopes", async () => {
  render(page()); await open();
  fireEvent.change(screen.getByRole("textbox", { name: "Markdown" }), { target: { value: "unsaved markdown" } });
  fireEvent.click(screen.getByRole("button", { name: "Check saved revision 2" }));
  const dialog = await screen.findByRole("dialog", { name: "Saved revision evidence" });
  expect(host).toHaveBeenCalledWith("workspace_check_publication_readiness", { request: selection });
  expect(JSON.stringify(host.mock.calls.filter(([c]) => c === "workspace_check_publication_readiness"))).not.toContain("unsaved markdown");
  expect(host.mock.calls.some(([c]) => c === "workspace_save_publication")).toBe(false);
  expect(screen.getByRole("textbox", { name: "Markdown" })).toHaveValue("unsaved markdown");
  expect(await within(dialog).findByText("Saved source has changed")).toBeVisible();
  expect(dialog).toHaveTextContent("Attention: 1"); expect(dialog).toHaveTextContent("Unverified: 1");
  expect(dialog).toHaveTextContent(/truncated/i);
  expect(dialog).toHaveTextContent("NotChecked"); expect(dialog).toHaveTextContent("NotExecuted");
  expect(dialog).toHaveTextContent("StoredRevisionContent");
  expect(dialog).toHaveTextContent("Saved snapshot text SHA-256");
  expect(dialog).toHaveTextContent("Exported reference text SHA-256");
  expect(dialog).toHaveTextContent("Recorded file SHA-256");
  expect(dialog).toHaveTextContent(/after the save dialog/i);
  expect(screen.getByRole("button", { name: "Export Markdown" })).toBeEnabled();
});

it.each(["revision", "publication", "project", "reload", "new", "hash", "unmount"])("discards pending old reports on %s changes and releases the next selection", async (change) => {
  const late = deferred<PublicationReadinessReportV1>();
  host.mockImplementation(async (command, args: any) => {
    if (command === "workspace_list_publications") return [detail.publication, { ...detail.publication, id: "pub2", title: "Other" }];
    if (command === "workspace_get_publication") return args.publicationId === "pub2" ? { ...detail, publication: { ...detail.publication, id: "pub2", title: "Other" } } : detail;
    if (command === "workspace_check_publication_readiness") return late.promise;
  });
  const view = render(page()); await open();
  fireEvent.click(screen.getByRole("button", { name: "Check saved revision 2" }));
  if (change === "revision") fireEvent.change(screen.getByRole("combobox", { name: "Version history" }), { target: { value: "1" } });
  if (change === "publication") fireEvent.click(screen.getByRole("button", { name: "Other" }));
  if (change === "project") view.rerender(page("p2"));
  if (change === "reload" || change === "hash") {
    if (change === "hash") host.mockImplementation(async (command) => command === "workspace_get_publication" ? { ...detail, revisions: detail.revisions.map((r) => ({ ...r, sha256: "c".repeat(64) })) } : [detail.publication]);
    fireEvent.click(screen.getByRole("button", { name: "Reload latest revision" }));
    await screen.findByRole("button", { name: "Check saved revision 2" });
  }
  if (change === "new") fireEvent.click(screen.getByRole("button", { name: "New manuscript" }));
  if (change === "unmount") view.unmount();
  await act(async () => late.resolve({ ...report, findings: [{ ...report.findings[0], message: "old report" }] }));
  expect(screen.queryByText("old report")).not.toBeInTheDocument();
  expect(screen.queryByRole("dialog", { name: "Saved revision evidence" })).not.toBeInTheDocument();
  if (["revision", "publication", "reload", "hash"].includes(change)) expect(screen.getByRole("button", { name: /Check saved revision/ })).toBeEnabled();
});

it("shows the historical legacy hash scope and exact saved tuple", async () => {
  host.mockImplementation(async (command) => command === "workspace_list_publications" ? [detail.publication] : command === "workspace_get_publication" ? detail : { ...report, selection: { ...selection, revision: 1, expected_revision_sha256: "a".repeat(64) }, revision_hash_scope: "public_legacy_markdown", truncated: false });
  render(page()); await open();
  fireEvent.change(screen.getByRole("combobox", { name: "Version history" }), { target: { value: "1" } });
  fireEvent.click(screen.getByRole("button", { name: "Check saved revision 1" }));
  expect(await screen.findByText(/PublicLegacyMarkdown/)).toBeVisible();
  expect(host).toHaveBeenCalledWith("workspace_check_publication_readiness", { request: { ...selection, revision: 1, expected_revision_sha256: "a".repeat(64) } });
});

it("cancel_and_safe_error: export cancellation is neutral and a successful receipt has separate ZIP and manifest hashes", async () => {
  render(page()); await open();
  fireEvent.change(screen.getByRole("textbox", { name: "Markdown" }), { target: { value: "unsaved markdown" } });
  fireEvent.click(screen.getByRole("button", { name: "Export saved revision 2 reference package" }));
  expect(await screen.findByText("Reference package export cancelled. Nothing was saved.")).toBeVisible();
  expect(host).toHaveBeenCalledWith("workspace_export_publication_reference_package", { request: selection });
  expect(screen.queryByText(/Package saved to/)).not.toBeInTheDocument();
  expect(screen.getByRole("textbox", { name: "Markdown" })).toHaveValue("unsaved markdown");
  host.mockResolvedValueOnce({ path: "C:/synthetic study/references.zip", package_sha256: "e".repeat(64), manifest_sha256: "f".repeat(64), size_bytes: 1024, selection });
  fireEvent.click(screen.getByRole("button", { name: "Export saved revision 2 reference package" }));
  expect(await screen.findByText(/Package saved to C:\/synthetic study\/references.zip/)).toBeVisible();
  const dialog = screen.getByRole("dialog", { name: "Saved revision evidence" });
  expect(dialog).toHaveTextContent("ZIP bytes SHA-256"); expect(dialog).toHaveTextContent("Manifest bytes SHA-256");
  expect(dialog).toHaveTextContent("1024"); expect(dialog).toHaveTextContent("NotExecuted"); expect(dialog).toHaveTextContent("NotChecked");
});

it.each(["invalid_selection", "not_found", "revision_changed", "limit_exceeded", "unsafe_public_content", "storage_read_failed", "export_write_failed", "unknown", "raw"])("uses fixed safe messages for %s without exposing thrown content", async (code) => {
  render(page()); await open();
  host.mockRejectedValueOnce(code === "raw" ? new Error("SECRET_SENTINEL password=hidden") : { code, message: "SECRET_SENTINEL password=hidden" });
  fireEvent.click(screen.getByRole("button", { name: "Export saved revision 2 reference package" }));
  expect(await screen.findByRole("alert")).not.toHaveTextContent("SECRET_SENTINEL");
  expect(screen.getByRole("alert")).toHaveTextContent(/saved revision|package|inspection|public content/i);
  expect(screen.queryByText(/Package saved to/)).not.toBeInTheDocument();
});

it.each(["receipt", "cancel", "error"])("ignores a late export %s after reload without leaking pending state", async (outcome) => {
  const late = deferred<any>();
  render(page()); await open();
  host.mockImplementationOnce(async () => late.promise);
  fireEvent.click(screen.getByRole("button", { name: "Export saved revision 2 reference package" }));
  fireEvent.click(screen.getByRole("button", { name: "Reload latest revision" }));
  await screen.findByRole("button", { name: "Check saved revision 2" });
  await act(async () => {
    if (outcome === "error") late.reject({ code: "export_write_failed", message: "SECRET_SENTINEL" });
    else late.resolve(outcome === "cancel" ? null : { path: "OLD_PACKAGE", package_sha256: "e".repeat(64), manifest_sha256: "f".repeat(64), size_bytes: 1024, selection });
  });
  expect(screen.queryByRole("dialog", { name: "Saved revision evidence" })).not.toBeInTheDocument();
  expect(screen.queryByText(/OLD_PACKAGE|cancelled|SECRET_SENTINEL/)).not.toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Export saved revision 2 reference package" })).toBeEnabled();
});

it("escape_top_layer: immediate window Escape closes the real report while the real parent remains open", async () => {
  const closeParent = vi.fn();
  render(<ResearchDialog title="Parent research overlay" zh={false} onClose={closeParent}>{page()}</ResearchDialog>);
  await open();
  fireEvent.click(screen.getByRole("button", { name: "Check saved revision 2" }));
  await screen.findByRole("dialog", { name: "Saved revision evidence" });
  fireEvent.keyDown(window, { key: "Escape" });
  expect(screen.queryByRole("dialog", { name: "Saved revision evidence" })).not.toBeInTheDocument();
  expect(screen.getByRole("dialog", { name: "Parent research overlay" })).toBeVisible();
  expect(closeParent).not.toHaveBeenCalled();
  fireEvent.keyDown(window, { key: "Escape" });
  expect(closeParent).toHaveBeenCalledTimes(1);
});

it.each(["check", "export"])("compares the returned %s selection by fields rather than JSON object order", async (operation) => {
  render(page()); await open();
  const reordered = { expected_revision_sha256: "b".repeat(64), revision: 2, publication_id: "pub1", project_id: "p1" };
  host.mockResolvedValueOnce(operation === "check" ? { ...report, selection: reordered } : { selection: reordered, path: "REORDERED_PACKAGE", package_sha256: "e".repeat(64), manifest_sha256: "f".repeat(64), size_bytes: 1024 });
  fireEvent.click(screen.getByRole("button", { name: operation === "check" ? "Check saved revision 2" : "Export saved revision 2 reference package" }));
  expect(await screen.findByText(operation === "check" ? "Saved source has changed" : /Package saved to REORDERED_PACKAGE/)).toBeVisible();
});

it("keeps the dirty leave decision above an open report and closes only that visible top layer", async () => {
  render(page()); await open();
  fireEvent.change(screen.getByRole("textbox", { name: "Markdown" }), { target: { value: "unsaved markdown" } });
  fireEvent.click(screen.getByRole("button", { name: "Check saved revision 2" }));
  await screen.findByText("Saved source has changed");
  fireEvent.click(screen.getByRole("button", { name: "New manuscript" }));
  expect(screen.getByRole("dialog", { name: "Unsaved manuscript" })).toBeVisible();
  fireEvent.keyDown(window, { key: "Escape" });
  expect(screen.queryByRole("dialog", { name: "Unsaved manuscript" })).not.toBeInTheDocument();
  expect(screen.getByRole("dialog", { name: "Saved revision evidence" })).toBeVisible();
  expect(screen.getByRole("textbox", { name: "Markdown" })).toHaveValue("unsaved markdown");
});

it("does not start evidence actions behind an existing dirty leave decision", async () => {
  render(page()); await open();
  fireEvent.change(screen.getByRole("textbox", { name: "Markdown" }), { target: { value: "unsaved markdown" } });
  fireEvent.click(screen.getByRole("button", { name: "New manuscript" }));
  const leaveDialog = screen.getByRole("dialog", { name: "Unsaved manuscript" });
  const check = screen.getByRole("button", { name: "Check saved revision 2" });
  expect(check).toBeDisabled();
  expect(screen.getByRole("button", { name: "Export saved revision 2 reference package" })).toBeDisabled();
  fireEvent.click(check);
  expect(host.mock.calls.some(([command]) => command === "workspace_check_publication_readiness")).toBe(false);
  fireEvent.keyDown(window, { key: "Escape" });
  expect(leaveDialog).not.toBeInTheDocument();
  expect(screen.getByRole("textbox", { name: "Markdown" })).toHaveValue("unsaved markdown");
});

it("renders a later local evidence picker above the mounted report and immediate Escape closes only the picker", async () => {
  render(page()); await open();
  fireEvent.click(screen.getByRole("button", { name: "Check saved revision 2" }));
  await screen.findByText("Saved source has changed");
  const reportDialog = screen.getByRole("dialog", { name: "Saved revision evidence" });
  host.mockResolvedValueOnce({ entries: [], next_offset: null });
  fireEvent.click(screen.getByRole("button", { name: "Choose evidence" }));
  const picker = await screen.findByRole("dialog", { name: "Choose evidence" });
  // These local backdrops share z-index: DOM order determines the visible top layer.
  expect(reportDialog.compareDocumentPosition(picker) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  fireEvent.keyDown(window, { key: "Escape" });
  expect(picker).not.toBeInTheDocument();
  expect(reportDialog).toBeVisible();
  expect(reportDialog).toHaveTextContent("Saved source has changed");
});
