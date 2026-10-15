import { useState } from "react";
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, expect, it, vi } from "vitest";
import { ProjectMigrationPreviewDialog } from "./ProjectMigrationPreviewDialog";
import { useWindowEscapeLayer } from "../settings/BrowserSettings";
import * as api from "../../tauri-api";
import * as files from "../../composer-file-api";
import * as navigation from "../../workspace-navigation-api";
import { workspaceSource } from "../../workspace-navigation-types";
import type { ProjectMigrationPreviewV1, WorkspaceProject } from "../../types";

const connection = "11111111-1111-4111-8111-111111111111";
const project: WorkspaceProject = { id: "project-a", name: "INTERNAL name", description: "INTERNAL description", local_root: "C:/PRIVATE", remote_root: "/PRIVATE", connection_id: connection, template: "blank", status: "ready", ollama_only: false, created_at: "now", updated_at: "now" };
const props = { project, locale: "en-US" as const, onClose: vi.fn() };
function report(name = "Public project"): ProjectMigrationPreviewV1 {
  return { schema_version: 1, project: { source_project_id: project.id, name, description: "Public description", template: "blank", created_at: "2026-10-15T00:00:00Z" }, observed_at: "2026-10-15T00:00:00Z", scientific_revision: null, categories: [{ category: "scientific_metadata", selected: true, record_count: 3, kind_counts: ["dataset", "analysis", "artifact", "evidence", "provenance"].map((kind, index) => ({ kind, record_count: index === 0 ? 3 : 0 })), examples: [{ source_id: "record", kind: "dataset", label: "Safe example", status: "active", revision: 1 }], examples_truncated: true }], files: [{ backend_id: "local", relative_path: "script.py", size_bytes: 4, hash: { sha256: "a".repeat(64), scope: "full_file" }, state: "small_text_candidate", reason: "Safe text" }, { backend_id: `ssh:${connection}`, relative_path: "saved.csv", size_bytes: null, hash: { sha256: "b".repeat(64), scope: "recorded_file" }, state: "reference_only", reason: "Stored reference" }, { backend_id: `ssh:${connection}`, relative_path: "unknown.csv", size_bytes: null, hash: null, state: "unreadable", reason: "Unknown file" }], selected_known_file_bytes: 4, unknown_size_file_count: 2, rebind: [{ kind: "mcp_server", count: 2, message: "Application-global registered configurations; not project grants or enabled/authorized capabilities." }, { kind: "skill_grant", count: 1, message: "Application-global registered packages; not project grants." }], exclusions: ["Preview only; no export or import performed."], preview_sha256: "c".repeat(64) };
}
function deferred<T>() { let resolve!: (value: T) => void; let reject!: (value: unknown) => void; const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; }); return { promise, resolve, reject }; }
beforeEach(() => {
  vi.restoreAllMocks(); props.onClose.mockClear();
  vi.spyOn(api, "previewProjectMigration").mockResolvedValue(report());
  vi.spyOn(files, "listLocalComposerFiles").mockResolvedValue([{ relative_path: "script.py", directory: false, size_bytes: 4, modified_unix_seconds: 0 }, { relative_path: "second.R", directory: false, size_bytes: 8, modified_unix_seconds: 0 }]);
  vi.spyOn(navigation, "getWorkspaceJourney").mockResolvedValue({ entries: [], next_offset: null });
  vi.spyOn(navigation, "getWorkspaceSourceDetail").mockRejectedValue(new Error("unexpected detail"));
});
it("migration_preview_defaults_empty and renders only native public fields, counts and hash scopes", async () => {
  render(<ProjectMigrationPreviewDialog {...props} />);
  expect(api.previewProjectMigration).not.toHaveBeenCalled(); expect(files.listLocalComposerFiles).not.toHaveBeenCalled(); expect(navigation.getWorkspaceJourney).not.toHaveBeenCalled();
  expect(screen.getAllByRole("checkbox").every((item) => !(item as HTMLInputElement).checked)).toBe(true);
  expect(screen.queryByText(/INTERNAL|PRIVATE/)).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "Generate preview" }));
  await screen.findByText("Public project");
  expect(api.previewProjectMigration).toHaveBeenCalledTimes(1); expect(api.previewProjectMigration).toHaveBeenCalledWith({ project_id: project.id, categories: [], files: [] });
  for (const kind of ["dataset", "analysis", "artifact", "evidence", "provenance"]) expect(screen.getByText(`${kind}: ${kind === "dataset" ? 3 : 0}`)).toBeVisible();
  expect(screen.getByText("Known selected bytes: 4; unknown size files: 2")).toBeVisible();
  expect(screen.getByText(/full_file/)).toBeVisible(); expect(screen.getByText(/recorded_file/)).toBeVisible(); expect(screen.getByText("Hash: unknown")).toBeVisible();
  expect(screen.getByText(/not project grants or enabled/)).toBeVisible();
  expect(screen.getByText(/fingerprint.*not approval/i)).toBeVisible();
  expect(screen.queryByRole("button", { name: /Export|Import/ })).toBeNull();
  expect(screen.queryByText(connection, { exact: false })).toBeNull();
});
it("selects local metadata explicitly and marks a previous result stale until generated again", async () => {
  render(<ProjectMigrationPreviewDialog {...props} />);
  fireEvent.click(screen.getByRole("button", { name: "Generate preview" })); await screen.findByText("Public project");
  fireEvent.click(screen.getByRole("button", { name: "Load local file metadata" }));
  fireEvent.click(await screen.findByRole("checkbox", { name: /script.py/ }));
  fireEvent.click(screen.getByRole("checkbox", { name: /second.R/ }));
  fireEvent.click(screen.getByRole("checkbox", { name: "Conversations" }));
  expect(screen.getByText(/Selection changed.*stale/)).toBeVisible(); expect(api.previewProjectMigration).toHaveBeenCalledTimes(1);
  fireEvent.click(screen.getByRole("button", { name: "Generate preview" }));
  await waitFor(() => expect(api.previewProjectMigration).toHaveBeenCalledTimes(2));
  expect(vi.mocked(api.previewProjectMigration).mock.calls[1][0]).toEqual({ project_id: project.id, categories: ["conversations"], files: [{ backend_id: "local", relative_path: "script.py", expected_full_sha256: null }, { backend_id: "local", relative_path: "second.R", expected_full_sha256: null }] });
});
it("migration_preview_no_ssh_refresh selects only exact owned compatible saved artifact metadata", async () => {
  const remote = vi.spyOn(api, "listRemoteFiles"); const runtime = vi.spyOn(api, "agentV4ComputeBackends");
  const source = workspaceSource(project.id, "artifact", "good");
  const ids = ["good", "legacy", "wrong-owner", "wrong-source", "wrong-binding", "unsafe-path"];
  vi.mocked(navigation.getWorkspaceJourney).mockResolvedValue({ entries: ids.map((id) => ({ source: { ...source, id }, title: "DO NOT SHOW internal metadata", status: "valid", occurred_at: "now", summary: "PRIVATE" })), next_offset: 20 });
  vi.mocked(navigation.getWorkspaceSourceDetail).mockImplementation(async (s) => ({ source: { ...s, project_id: s.id === "wrong-owner" ? "foreign" : s.project_id, id: s.id === "wrong-source" ? "foreign" : s.id }, title: "PRIVATE", text: "PRIVATE", sha256: "hash", status: "valid", availability: "available", metadata: { content_mode: "reference_only", backend_id: s.id === "legacy" ? "" : s.id === "wrong-binding" ? "ssh:22222222-2222-4222-8222-222222222222" : `ssh:${connection}`, relative_path: s.id === "unsafe-path" ? "../secret.csv" : `${s.id}.csv` } }));
  render(<ProjectMigrationPreviewDialog {...props} />);
  fireEvent.click(screen.getByRole("button", { name: "Load saved SSH references" }));
  const selectable = await screen.findByRole("checkbox", { name: /good.csv/ });
  await waitFor(() => expect(screen.getAllByRole("checkbox").filter((item) => (item as HTMLInputElement).disabled)).toHaveLength(5));
  fireEvent.click(selectable); fireEvent.click(screen.getByRole("button", { name: "Generate preview" }));
  await waitFor(() => expect(api.previewProjectMigration).toHaveBeenCalledWith({ project_id: project.id, categories: [], files: [{ backend_id: `ssh:${connection}`, relative_path: "good.csv", expected_full_sha256: null }] }));
  expect(navigation.getWorkspaceJourney).toHaveBeenCalledWith({ project_id: project.id, query: "", kind: "artifact", offset: 0, limit: 20 });
  expect(screen.getByRole("button", { name: "Next saved references" })).toBeVisible();
  expect(screen.queryByText(/DO NOT SHOW|PRIVATE/)).toBeNull(); expect(remote).not.toHaveBeenCalled(); expect(runtime).not.toHaveBeenCalled();
});
it.each(["success", "error"])("discards old %s and finally after selection A to B to A while a new generation is pending", async (outcome) => {
  const old = deferred<ProjectMigrationPreviewV1>(); const current = deferred<ProjectMigrationPreviewV1>();
  vi.mocked(api.previewProjectMigration).mockReturnValueOnce(old.promise).mockReturnValueOnce(current.promise);
  render(<ProjectMigrationPreviewDialog {...props} />);
  fireEvent.click(screen.getByRole("button", { name: "Generate preview" }));
  fireEvent.click(screen.getByRole("checkbox", { name: "Conversations" })); fireEvent.click(screen.getByRole("checkbox", { name: "Conversations" }));
  fireEvent.click(screen.getByRole("button", { name: "Generate preview" }));
  await act(async () => { if (outcome === "success") old.resolve(report("OLD")); else old.reject({ message: "PRIVATE" }); });
  expect(screen.queryByText("OLD")).toBeNull(); expect(screen.queryByRole("alert")).toBeNull(); expect(screen.getByRole("button", { name: "Generating preview…" })).toBeDisabled();
  await act(async () => current.resolve(report("NEW"))); expect(screen.getByText("NEW")).toBeVisible();
});
it("rejects old project responses through A to B to A recurrence", async () => {
  const old = deferred<ProjectMigrationPreviewV1>(); vi.mocked(api.previewProjectMigration).mockReturnValueOnce(old.promise);
  const { rerender } = render(<ProjectMigrationPreviewDialog {...props} />);
  fireEvent.click(screen.getByRole("button", { name: "Generate preview" }));
  rerender(<ProjectMigrationPreviewDialog {...props} project={{ ...project, id: "project-b" }} />);
  rerender(<ProjectMigrationPreviewDialog {...props} />);
  await act(async () => old.resolve(report("OLD"))); expect(screen.queryByText("OLD")).toBeNull(); expect(screen.getByRole("button", { name: "Generate preview" })).toBeEnabled();
});
it.each(["success", "error"])("old project %s cannot clear a new pending preview after project recurrence", async (outcome) => {
  const old = deferred<ProjectMigrationPreviewV1>(); const current = deferred<ProjectMigrationPreviewV1>();
  vi.mocked(api.previewProjectMigration).mockReturnValueOnce(old.promise).mockReturnValueOnce(current.promise);
  const { rerender } = render(<ProjectMigrationPreviewDialog {...props} />);
  fireEvent.click(screen.getByRole("button", { name: "Generate preview" }));
  rerender(<ProjectMigrationPreviewDialog {...props} project={{ ...project, id: "project-b" }} />); rerender(<ProjectMigrationPreviewDialog {...props} />);
  fireEvent.click(screen.getByRole("button", { name: "Generate preview" }));
  await act(async () => { if (outcome === "success") old.resolve(report("OLD")); else old.reject({ message: "PRIVATE" }); });
  expect(screen.queryByText("OLD")).toBeNull(); expect(screen.queryByRole("alert")).toBeNull(); expect(screen.getByRole("button", { name: "Generating preview…" })).toBeDisabled();
  await act(async () => current.resolve(report("NEW"))); expect(screen.getByText("NEW")).toBeVisible();
});
it.each([["local", "success"], ["local", "error"], ["saved", "success"], ["saved", "error"]] as const)("late %s catalog %s/finally cannot replace a new project's pending catalog", async (kind, outcome) => {
  const oldLocal = deferred<Awaited<ReturnType<typeof files.listLocalComposerFiles>>>(); const newLocal = deferred<Awaited<ReturnType<typeof files.listLocalComposerFiles>>>();
  const oldSaved = deferred<Awaited<ReturnType<typeof navigation.getWorkspaceJourney>>>(); const newSaved = deferred<Awaited<ReturnType<typeof navigation.getWorkspaceJourney>>>();
  vi.mocked(files.listLocalComposerFiles).mockReturnValueOnce(oldLocal.promise).mockReturnValueOnce(newLocal.promise);
  vi.mocked(navigation.getWorkspaceJourney).mockReturnValueOnce(oldSaved.promise).mockReturnValueOnce(newSaved.promise);
  const label = kind === "local" ? "Load local file metadata" : "Load saved SSH references";
  const { rerender } = render(<ProjectMigrationPreviewDialog {...props} />); fireEvent.click(screen.getByRole("button", { name: label }));
  rerender(<ProjectMigrationPreviewDialog {...props} project={{ ...project, id: "project-b" }} />); rerender(<ProjectMigrationPreviewDialog {...props} />);
  fireEvent.click(screen.getByRole("button", { name: label }));
  await act(async () => {
    if (kind === "local") { if (outcome === "success") oldLocal.resolve([{ relative_path: "OLD.py", directory: false, size_bytes: 1, modified_unix_seconds: 0 }]); else oldLocal.reject({ message: "PRIVATE" }); }
    else { if (outcome === "success") oldSaved.resolve({ entries: [], next_offset: 20 }); else oldSaved.reject({ message: "PRIVATE" }); }
  });
  expect(screen.queryByText("OLD.py")).toBeNull(); expect(screen.queryByRole("alert")).toBeNull(); expect(screen.getByRole("button", { name: label })).toBeDisabled();
  await act(async () => { if (kind === "local") newLocal.resolve([]); else newSaved.resolve({ entries: [], next_offset: null }); });
  expect(screen.getByRole("button", { name: label })).toBeEnabled();
});
it("ignores in-flight saved detail after rebinding and never selects a stale binding", async () => {
  const old = deferred<Awaited<ReturnType<typeof navigation.getWorkspaceSourceDetail>>>();
  const source = workspaceSource(project.id, "artifact", "artifact");
  vi.mocked(navigation.getWorkspaceJourney).mockResolvedValue({ entries: [{ source, title: "OLD", status: "valid", occurred_at: "now", summary: "" }], next_offset: null });
  vi.mocked(navigation.getWorkspaceSourceDetail).mockReturnValueOnce(old.promise);
  const { rerender } = render(<ProjectMigrationPreviewDialog {...props} />);
  fireEvent.click(screen.getByRole("button", { name: "Load saved SSH references" }));
  await waitFor(() => expect(navigation.getWorkspaceSourceDetail).toHaveBeenCalledOnce());
  rerender(<ProjectMigrationPreviewDialog {...props} project={{ ...project, connection_id: "22222222-2222-4222-8222-222222222222" }} />);
  await act(async () => old.resolve({ source, title: "OLD", text: "PRIVATE", sha256: "", status: "valid", availability: "available", metadata: { content_mode: "reference_only", relative_path: "OLD.csv", backend_id: `ssh:${connection}` } }));
  expect(screen.queryByRole("checkbox", { name: /OLD.csv/ })).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "Generate preview" }));
  await waitFor(() => expect(api.previewProjectMigration).toHaveBeenCalledWith({ project_id: project.id, categories: [], files: [] }));
});
it("does not offer Windows-unsafe or internal saved paths as selectable references", async () => {
  const source = workspaceSource(project.id, "artifact", "unsafe");
  const paths = ["file?.csv", "file*.csv", "file\".csv", ".omicsops/data.csv", "data/CON.txt", "C:/PRIVATE/file.csv", "file.csv "];
  vi.mocked(navigation.getWorkspaceJourney).mockResolvedValue({ entries: paths.map((_, index) => ({ source: { ...source, id: String(index) }, title: "ignored", status: "valid", occurred_at: "now", summary: "" })), next_offset: null });
  vi.mocked(navigation.getWorkspaceSourceDetail).mockImplementation(async (s) => ({ source: s, title: "ignored", text: "PRIVATE", sha256: "", status: "valid", availability: "available", metadata: { content_mode: "reference_only", backend_id: `ssh:${connection}`, relative_path: paths[Number(s.id)] } }));
  render(<ProjectMigrationPreviewDialog {...props} />); fireEvent.click(screen.getByRole("button", { name: "Load saved SSH references" }));
  await waitFor(() => expect(screen.getAllByRole("checkbox").filter((item) => (item as HTMLInputElement).disabled)).toHaveLength(7));
});
it("caps explicit file selection at 32, rejects unsafe metadata and never sends directory entries", async () => {
  vi.mocked(files.listLocalComposerFiles).mockResolvedValue([...Array.from({ length: 33 }, (_, i) => ({ relative_path: `file${i}.py`, directory: false, size_bytes: 1, modified_unix_seconds: 0 })), { relative_path: "directory", directory: true, size_bytes: 0, modified_unix_seconds: 0 }, { relative_path: "../PRIVATE", directory: false, size_bytes: 1, modified_unix_seconds: 0 }]);
  render(<ProjectMigrationPreviewDialog {...props} />); fireEvent.click(screen.getByRole("button", { name: "Load local file metadata" }));
  await screen.findByRole("checkbox", { name: "file0.py" });
  for (let i = 0; i < 32; i++) fireEvent.click(screen.getByRole("checkbox", { name: `file${i}.py` }));
  expect(screen.getByRole("checkbox", { name: "file32.py" })).toBeDisabled(); expect(screen.queryByRole("checkbox", { name: /directory|PRIVATE/ })).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "Generate preview" }));
  await waitFor(() => expect(api.previewProjectMigration).toHaveBeenCalledOnce()); expect(vi.mocked(api.previewProjectMigration).mock.calls[0][0].files).toHaveLength(32);
});
it("uses bounded saved pages without clearing earlier explicit selections or trusting foreign journey entries", async () => {
  const source = workspaceSource(project.id, "artifact", "one");
  vi.mocked(navigation.getWorkspaceJourney).mockResolvedValueOnce({ entries: [{ source, title: "ignored", status: "valid", occurred_at: "now", summary: "" }], next_offset: 20 }).mockResolvedValueOnce({ entries: [{ source: { ...source, project_id: "foreign" }, title: "PRIVATE", status: "valid", occurred_at: "now", summary: "" }], next_offset: null });
  vi.mocked(navigation.getWorkspaceSourceDetail).mockResolvedValue({ source, title: "ignored", text: "PRIVATE", sha256: "", status: "valid", availability: "available", metadata: { content_mode: "reference_only", backend_id: `ssh:${connection}`, relative_path: "one.csv" } });
  render(<ProjectMigrationPreviewDialog {...props} />); fireEvent.click(screen.getByRole("button", { name: "Load saved SSH references" }));
  fireEvent.click(await screen.findByRole("checkbox", { name: "one.csv" })); fireEvent.click(screen.getByRole("button", { name: "Next saved references" }));
  await waitFor(() => expect(screen.queryByRole("button", { name: "Next saved references" })).toBeNull());
  expect(navigation.getWorkspaceSourceDetail).toHaveBeenCalledOnce(); expect(navigation.getWorkspaceJourney).toHaveBeenLastCalledWith({ project_id: project.id, query: "", kind: "artifact", offset: 20, limit: 20 });
  fireEvent.click(screen.getByRole("button", { name: "Generate preview" })); await waitFor(() => expect(api.previewProjectMigration).toHaveBeenCalledOnce());
  expect(vi.mocked(api.previewProjectMigration).mock.calls[0][0].files).toEqual([{ backend_id: `ssh:${connection}`, relative_path: "one.csv", expected_full_sha256: null }]);
});
it("refuses a foreign preview response with a fixed safe error", async () => {
  vi.mocked(api.previewProjectMigration).mockResolvedValue({ ...report("PRIVATE"), project: { ...report().project, source_project_id: "foreign", name: "PRIVATE" } });
  render(<ProjectMigrationPreviewDialog {...props} />); fireEvent.click(screen.getByRole("button", { name: "Generate preview" }));
  expect(await screen.findByRole("alert")).toHaveTextContent("Could not generate preview"); expect(screen.queryByText("PRIVATE")).toBeNull();
});
it("migration_preview_scope_and_escape closes only the top layer immediately and invalidates close/reopen work", async () => {
  const parentClose = vi.fn(); const old = deferred<ProjectMigrationPreviewV1>(); vi.mocked(api.previewProjectMigration).mockReturnValueOnce(old.promise);
  function Parent() { const [open, setOpen] = useState(false); useWindowEscapeLayer(true, parentClose); return <><button onClick={() => setOpen(true)}>Preview</button>{open && <ProjectMigrationPreviewDialog {...props} onClose={() => setOpen(false)} />}</>; }
  render(<Parent />); fireEvent.click(screen.getByRole("button", { name: "Preview" }));
  fireEvent.keyDown(window, { key: "Escape" }); expect(screen.queryByRole("dialog")).toBeNull(); expect(parentClose).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "Preview" })); fireEvent.click(screen.getByRole("button", { name: "Generate preview" }));
  fireEvent.keyDown(window, { key: "Escape" }); fireEvent.click(screen.getByRole("button", { name: "Preview" }));
  await act(async () => old.resolve(report("OLD"))); expect(screen.queryByText("OLD")).toBeNull(); expect(within(screen.getByRole("dialog")).getByRole("button", { name: "Generate preview" })).toBeEnabled();
});
