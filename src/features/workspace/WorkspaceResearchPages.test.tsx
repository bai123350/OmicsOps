import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { useWindowEscapeLayer } from "../settings/BrowserSettings";
import { WorkspaceResearchPages, type WorkspaceResearchPagesProps } from "./WorkspaceResearchPages";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
const host = vi.mocked(invoke);
const source = { project_id: "p1", kind: "artifact", id: "a1", conversation_id: "c1", run_id: "r1", sequence: null, event_hash: null, content_sha256: null, start: null, end: null };
const snapshot = { source, title: "QC report", text: "Artifact metadata only", sha256: "saved-hash", status: "unverified", metadata: { location: "ssh", path: "/study/qc.pdf" }, availability: "available" };
const entry = { source, title: "QC report", status: "unverified", occurred_at: "2026-09-22T01:00:00Z", summary: "Quality control output" };
const publication = { id: "pub1", project_id: "p1", title: "Study", revision: 2, updated_at: "2026-09-22T01:00:00Z" };
const oldRevision = { id: "rev1", publication_id: "pub1", revision: 1, title: "First title", markdown: "First body", references: [], sha256: "one", created_at: "2026-09-21T01:00:00Z", legacy: false };
const detail = { publication, revisions: [{ ...oldRevision, id: "rev2", revision: 2, title: "Study", markdown: "Saved body", sha256: "two" }, oldRevision] };
const libraryItem = { id: "lib1", kind: "excerpt", title: "Methods", source_project_id: "p1", source_project_name: "Cells", source_conversation_id: "c1", source_conversation_title: "Discussion", text_preview: "Saved excerpt", created_at: "2026-09-22T01:00:00Z" };
const libraryDetail = { item: libraryItem, snapshot: { ...snapshot, source: { ...source, kind: "message" }, text: "Saved excerpt", availability: "missing" } };

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((yes) => { resolve = yes; });
  return { promise, resolve };
}
function renderPage(overrides: Partial<WorkspaceResearchPagesProps> = {}) {
  const props: WorkspaceResearchPagesProps = { page: "journey", projectId: "p1", locale: "en-US", onOpenConversation: vi.fn(), onInsert: vi.fn(), ...overrides };
  return { ...render(<WorkspaceResearchPages {...props} />), props };
}
beforeEach(() => {
  Object.defineProperty(window, "__TAURI_INTERNALS__", { configurable: true, value: {} });
  host.mockReset();
  host.mockImplementation(async (command) => {
    switch (command) {
      case "workspace_journey": return { entries: [entry], next_offset: null };
      case "workspace_source_detail": return snapshot;
      case "workspace_list_publications": return [publication];
      case "workspace_get_publication": return detail;
      case "workspace_list_library": return { items: [libraryItem], next_offset: null };
      case "workspace_get_library_item": return libraryDetail;
      case "workspace_export_publication": return null;
      case "workspace_save_library_item": return libraryDetail;
      default: return undefined;
    }
  });
});

describe("research journey", () => {
  it("requests exact status filtering across the full journey and resets pagination", async () => {
    host.mockImplementation(async (command, args: any) => {
      if (command !== "workspace_journey") return snapshot;
      if (args.request.status === "failed") return { entries: [{ ...entry, source: { ...source, id: "older-failure" }, title: "Older failed analysis", status: "failed" }], next_offset: null };
      return { entries: [{ ...entry, status: "completed" }], next_offset: 25 };
    });
    renderPage();
    await screen.findByRole("button", { name: /QC report/ });
    fireEvent.change(screen.getByRole("textbox", { name: "Record status" }), { target: { value: "failed" } });
    expect(await screen.findByRole("button", { name: /Older failed analysis/ })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /QC report/ })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Load more" })).not.toBeInTheDocument();
    expect(host).toHaveBeenLastCalledWith("workspace_journey", { request: { project_id: "p1", query: "", kind: null, status: "failed", offset: 0, limit: 25 } });
  });
  it("loads real entries, filters, pages and retries a failed request", async () => {
    let attempts = 0;
    host.mockImplementation(async (command, args: any) => {
      if (command !== "workspace_journey") return snapshot;
      if (++attempts === 1) throw new Error("offline");
      if (args.request.offset === 25) return { entries: [{ ...entry, source: { ...source, id: "a2" }, title: "Second report" }], next_offset: null };
      return { entries: args.request.query === "none" ? [] : [entry], next_offset: 25 };
    });
    renderPage();
    expect(await screen.findByRole("alert")).toHaveTextContent("offline");
    fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    expect(await screen.findByRole("button", { name: /QC report/ })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Load more" }));
    expect(await screen.findByText("Second report")).toBeInTheDocument();
    fireEvent.change(screen.getByRole("searchbox", { name: "Search journey" }), { target: { value: "none" } });
    expect(await screen.findByText("No research records match these filters.")).toBeInTheDocument();
    expect(screen.queryByText("Second report")).not.toBeInTheDocument();
  });
  it("discards old project requests and immediately closes only source detail on Escape", async () => {
    const late = deferred<any>();
    host.mockImplementation(async (command, args: any) => command === "workspace_journey" ? (args.request.project_id === "p1" ? late.promise : { entries: [{ ...entry, title: "New project record" }], next_offset: null }) : snapshot);
    const parentClose = vi.fn();
    function Parent({ projectId }: { projectId: string }) {
      useWindowEscapeLayer(true, parentClose);
      return <WorkspaceResearchPages page="journey" projectId={projectId} locale="en-US" onInsert={vi.fn()} onOpenConversation={vi.fn()} />;
    }
    const view = render(<Parent projectId="p1" />);
    view.rerender(<Parent projectId="p2" />);
    fireEvent.click(await screen.findByRole("button", { name: /New project record/ }));
    expect(await screen.findByRole("dialog", { name: "Source details" })).toHaveTextContent("Artifact reference");
    fireEvent.keyDown(window, { key: "Escape" });
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(parentClose).not.toHaveBeenCalled();
    await act(async () => late.resolve({ entries: [entry], next_offset: null }));
    expect(screen.queryByRole("button", { name: /QC report/ })).not.toBeInTheDocument();
  });
  it("keeps collection request identity on an ambiguous failure", async () => {
    let request: any;
    let attempts = 0;
    host.mockImplementation(async (command, args: any) => {
      if (command === "workspace_journey") return { entries: [entry], next_offset: null };
      if (command === "workspace_source_detail") return snapshot;
      if (command === "workspace_save_library_item") {
        if (++attempts === 1) { request = args.request; throw new Error("response lost"); }
        expect(args.request).toEqual(request);
        return libraryDetail;
      }
    });
    renderPage();
    fireEvent.click(await screen.findByRole("button", { name: /QC report/ }));
    fireEvent.click(await screen.findByRole("button", { name: "Add to library" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("response lost");
    fireEvent.click(screen.getByRole("button", { name: "Retry collection" }));
    expect(await screen.findByRole("status")).toHaveTextContent("Added to library");
    expect(request.kind).toBe("artifact");
    expect(request.source).toEqual(source);
  });
});

describe("publication workspace", () => {
  it.each([
    "Source no longer exists",
    "Source content or identity has changed",
    "Invalid source: snapshot exceeds 256 KiB",
    "At most 32 publication references may be saved",
    "Publication reference belongs to another project",
    "invalid input: publication title must contain 1..200 characters",
    "invalid input: publication title exceeds 1824 bytes",
    "invalid input: publication text exceeds 1048576 bytes",
  ])("preserves an editable draft after a definite prewrite rejection: %s", async (rejection) => {
    const requests: any[] = [];
    host.mockImplementation(async (command, args: any) => {
      if (command === "workspace_list_publications") return [];
      if (command === "workspace_journey") return { entries: [entry], next_offset: null };
      if (command === "workspace_save_publication") {
        requests.push(args.request);
        if (requests.length === 1) throw new Error(rejection);
        return { publication, revisions: [{ ...oldRevision, revision: 2, title: args.request.title, markdown: args.request.markdown }] };
      }
    });
    renderPage({ page: "publication" });
    fireEvent.click(screen.getByRole("button", { name: "New manuscript" }));
    fireEvent.change(screen.getByRole("textbox", { name: "Title" }), { target: { value: "My manuscript" } });
    fireEvent.change(screen.getByRole("textbox", { name: "Markdown" }), { target: { value: "Keep this body" } });
    fireEvent.click(screen.getByRole("button", { name: "Choose evidence" }));
    fireEvent.click(await screen.findByRole("button", { name: "Add reference" }));
    fireEvent.keyDown(window, { key: "Escape" });
    fireEvent.click(screen.getByRole("button", { name: "Save revision" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(rejection);
    expect(screen.getByRole("textbox", { name: "Markdown" })).not.toHaveAttribute("readonly");
    expect(screen.getByRole("textbox", { name: "Markdown" })).toHaveValue("Keep this body");
    fireEvent.click(screen.getByRole("button", { name: "Remove reference a1" }));
    fireEvent.click(screen.getByRole("button", { name: "Save revision" }));
    expect(await screen.findByText("Revision 2 saved.")).toBeInTheDocument();
    expect(requests[1].request_id).not.toBe(requests[0].request_id);
    expect(requests[1]).toMatchObject({ title: "My manuscript", markdown: "Keep this body", sources: [] });
  });
  it.each(["revision conflict", "response lost", "database failed: unavailable"])("preserves unsaved edits and retries the exact payload after %s", async (rejection) => {
    let request: any;
    let attempts = 0;
    host.mockImplementation(async (command, args: any) => {
      if (command === "workspace_list_publications") return [publication];
      if (command === "workspace_get_publication") return detail;
      if (command === "workspace_save_publication") {
        if (++attempts === 1) { request = args.request; throw new Error(rejection); }
        expect(args.request).toEqual(request);
        return { publication: { ...publication, revision: 3 }, revisions: [{ ...oldRevision, revision: 3, title: request.title, markdown: request.markdown }] };
      }
    });
    renderPage({ page: "publication" });
    fireEvent.click(await screen.findByRole("button", { name: "Study" }));
    fireEvent.change(await screen.findByRole("textbox", { name: "Markdown" }), { target: { value: "My draft" } });
    fireEvent.click(screen.getByRole("button", { name: "Save revision" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(rejection);
    expect(screen.getByRole("textbox", { name: "Markdown" })).toHaveValue("My draft");
    expect(screen.getByRole("textbox", { name: "Markdown" })).toHaveAttribute("readonly");
    fireEvent.click(screen.getByRole("button", { name: "Retry save" }));
    expect(await screen.findByText("Revision 3 saved.")).toBeInTheDocument();
    expect(request.expected_revision).toBe(2);
  });
  it("keeps historical title and body read-only, restores as a new revision and treats cancelled export as cancellation", async () => {
    host.mockImplementation(async (command, args: any) => {
      if (command === "workspace_list_publications") return [publication];
      if (command === "workspace_get_publication") return detail;
      if (command === "workspace_export_publication") return null;
      if (command === "workspace_restore_publication") {
        expect(args.request).toMatchObject({ expected_revision: 2, revision: 1 });
        return { publication: { ...publication, revision: 3, title: "First title" }, revisions: [{ ...oldRevision, id: "rev3", revision: 3 }, ...detail.revisions] };
      }
    });
    renderPage({ page: "publication" });
    fireEvent.click(await screen.findByRole("button", { name: "Study" }));
    fireEvent.change(await screen.findByRole("combobox", { name: "Version history" }), { target: { value: "1" } });
    expect(screen.getByRole("textbox", { name: "Title" })).toHaveValue("First title");
    expect(screen.getByRole("textbox", { name: "Markdown" })).toHaveAttribute("readonly");
    fireEvent.click(screen.getByRole("button", { name: "Export Markdown" }));
    expect(await screen.findByText("Export cancelled.")).toBeInTheDocument();
    expect(screen.queryByText(/Exported to/)).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Restore this revision" }));
    expect(await screen.findByText("Revision 3 saved.")).toBeInTheDocument();
  });
  it("guards unsaved navigation and Escape keeps editing without navigating", async () => {
    let guard!: (next: () => void) => void;
    const next = vi.fn();
    renderPage({ page: "publication", registerBeforeLeave: (value) => { guard = value; } });
    fireEvent.click(await screen.findByRole("button", { name: "New manuscript" }));
    fireEvent.change(screen.getByRole("textbox", { name: "Markdown" }), { target: { value: "Unsaved" } });
    act(() => guard(next));
    expect(screen.getByRole("dialog", { name: "Unsaved manuscript" })).toBeInTheDocument();
    fireEvent.keyDown(window, { key: "Escape" });
    expect(next).not.toHaveBeenCalled();
    expect(screen.getByRole("textbox", { name: "Markdown" })).toHaveValue("Unsaved");
    act(() => guard(next));
    fireEvent.click(screen.getByRole("button", { name: "Discard and leave" }));
    expect(next).toHaveBeenCalledTimes(1);
  });
});

describe("personal library", () => {
  it("offers every saved source project before its items have been loaded", async () => {
    const projects = [{ id: "p1", name: "Cells" }, { id: "older-project", name: "Older study" }];
    host.mockImplementation(async (command, args: any) => {
      if (command !== "workspace_list_library") return libraryDetail;
      if (args.request.project_id === "older-project") return { items: [{ ...libraryItem, id: "older-item", title: "Older methods", source_project_id: "older-project", source_project_name: "Older study" }], next_offset: null, source_projects: projects };
      return { items: [libraryItem], next_offset: 25, source_projects: projects };
    });
    renderPage({ page: "library" });
    fireEvent.change(screen.getByRole("searchbox", { name: "Search library" }), { target: { value: "methods" } });
    expect(await screen.findByRole("option", { name: "Older study" })).toBeInTheDocument();
    fireEvent.change(screen.getByRole("combobox", { name: "Source project" }), { target: { value: "older-project" } });
    expect(await screen.findByRole("button", { name: /Older methods/ })).toBeInTheDocument();
    expect(host).toHaveBeenLastCalledWith("workspace_list_library", { request: { query: "methods", kind: null, project_id: "older-project", offset: 0, limit: 25 } });
  });
  it("ignores stale project directories when a library query changes", async () => {
    const late = deferred<any>();
    host.mockImplementation(async (command, args: any) => {
      if (command !== "workspace_list_library") return libraryDetail;
      if (!args.request.query) return late.promise;
      return { items: [], next_offset: null, source_projects: [{ id: "current-project", name: "Current saved project" }] };
    });
    renderPage({ page: "library" });
    fireEvent.change(screen.getByRole("searchbox", { name: "Search library" }), { target: { value: "new query" } });
    await screen.findByRole("option", { name: "Current saved project" });
    await act(async () => late.resolve({ items: [libraryItem], next_offset: 25, source_projects: [{ id: "stale-project", name: "Stale saved project" }] }));
    expect(screen.queryByRole("option", { name: "Stale saved project" })).not.toBeInTheDocument();
    expect(screen.getByRole("option", { name: "Current saved project" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /Methods/ })).not.toBeInTheDocument();
  });
  it("retains a missing-source snapshot, disables source navigation, inserts only the snapshot and removes only the collection", async () => {
    const insert = vi.fn();
    let removed = false;
    host.mockImplementation(async (command) => {
      if (command === "workspace_list_library") return { items: removed ? [] : [libraryItem], next_offset: null };
      if (command === "workspace_get_library_item") return libraryDetail;
      if (command === "workspace_delete_library_item") { removed = true; return; }
    });
    renderPage({ page: "library", onInsert: insert });
    fireEvent.click(await screen.findByRole("button", { name: /Methods/ }));
    const dialog = await screen.findByRole("dialog", { name: "Library details" });
    expect(within(dialog).getByText("Source missing")).toBeInTheDocument();
    expect(within(dialog).getByRole("button", { name: "Open source conversation" })).toBeDisabled();
    fireEvent.click(within(dialog).getByRole("button", { name: "Insert into conversation draft" }));
    expect(insert).toHaveBeenCalledTimes(1);
    expect(insert).toHaveBeenCalledWith("Saved excerpt");
    fireEvent.click(within(dialog).getByRole("button", { name: "Remove from library" }));
    expect(await screen.findByText("No saved items match these filters.")).toBeInTheDocument();
    expect(host.mock.calls.filter(([command]) => /delete/.test(command)).map(([command]) => command)).toEqual(["workspace_delete_library_item"]);
  });
});

describe("research page boundaries", () => {
  it("guards dirty edits when retrying another manuscript that failed to load", async () => {
    let failed = false;
    host.mockImplementation(async (command, args: any) => {
      if (command === "workspace_list_publications") return [publication, { ...publication, id: "pub2", title: "Other study" }];
      if (command === "workspace_get_publication") {
        if (args.publicationId === "pub1") return detail;
        if (!failed) { failed = true; throw new Error("detail unavailable"); }
        return { publication: { ...publication, id: "pub2", title: "Other study" }, revisions: [{ ...detail.revisions[0], publication_id: "pub2", title: "Other study", markdown: "Other body" }] };
      }
    });
    renderPage({ page: "publication" });
    fireEvent.click(await screen.findByRole("button", { name: "Study" }));
    await screen.findByRole("textbox", { name: "Markdown" });
    fireEvent.click(screen.getByRole("button", { name: "Other study" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("detail unavailable");
    fireEvent.change(screen.getByRole("textbox", { name: "Markdown" }), { target: { value: "Keep this draft" } });
    fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    expect(screen.getByRole("dialog", { name: "Unsaved manuscript" })).toBeInTheDocument();
    fireEvent.keyDown(window, { key: "Escape" });
    expect(screen.getByRole("textbox", { name: "Markdown" })).toHaveValue("Keep this draft");
    fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    fireEvent.click(screen.getByRole("button", { name: "Discard and leave" }));
    expect(await screen.findByRole("textbox", { name: "Markdown" })).toHaveValue("Other body");
  });
  it("shows a retry when opening manuscript details fails", async () => {
    let failed = false;
    host.mockImplementation(async (command) => {
      if (command === "workspace_list_publications") return [publication];
      if (command === "workspace_get_publication") {
        if (!failed) { failed = true; throw new Error("detail unavailable"); }
        return detail;
      }
    });
    renderPage({ page: "publication" });
    fireEvent.click(await screen.findByRole("button", { name: "Study" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("detail unavailable");
    fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    expect(await screen.findByRole("textbox", { name: "Markdown" })).toHaveValue("Saved body");
  });
  it("keeps an evidence picker open when Escape closes its nested source detail, then saves exact selected identities", async () => {
    let saved: any;
    host.mockImplementation(async (command, args: any) => {
      if (command === "workspace_list_publications") return [];
      if (command === "workspace_journey") return { entries: [entry], next_offset: null };
      if (command === "workspace_source_detail") return snapshot;
      if (command === "workspace_save_publication") {
        saved = args.request;
        return { publication, revisions: [{ ...oldRevision, revision: 2, title: saved.title, markdown: saved.markdown, references: [snapshot] }] };
      }
    });
    renderPage({ page: "publication" });
    fireEvent.click(screen.getByRole("button", { name: "New manuscript" }));
    fireEvent.change(screen.getByRole("textbox", { name: "Title" }), { target: { value: "New study" } });
    fireEvent.click(screen.getByRole("button", { name: "Choose evidence" }));
    fireEvent.click(await screen.findByRole("button", { name: /QC report/ }));
    expect(await screen.findByRole("dialog", { name: "Source details" })).toBeInTheDocument();
    fireEvent.keyDown(window, { key: "Escape" });
    expect(screen.queryByRole("dialog", { name: "Source details" })).not.toBeInTheDocument();
    const picker = screen.getByRole("dialog", { name: "Choose evidence" });
    fireEvent.click(within(picker).getByRole("button", { name: "Add reference" }));
    expect(within(picker).getByRole("button", { name: "Selected" })).toBeDisabled();
    fireEvent.keyDown(window, { key: "Escape" });
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Save revision" }));
    expect(await screen.findByText("Revision 2 saved.")).toBeInTheDocument();
    expect(saved).toMatchObject({ publication_id: null, expected_revision: 0, title: "New study", sources: [source] });
  });
  it("selects the requested history revision after discarding dirty edits", async () => {
    renderPage({ page: "publication" });
    fireEvent.click(await screen.findByRole("button", { name: "Study" }));
    fireEvent.change(await screen.findByRole("textbox", { name: "Markdown" }), { target: { value: "Unsaved edits" } });
    fireEvent.change(screen.getByRole("combobox", { name: "Version history" }), { target: { value: "1" } });
    fireEvent.click(screen.getByRole("button", { name: "Discard and leave" }));
    expect(screen.getByRole("textbox", { name: "Markdown" })).toHaveValue("First body");
    expect(screen.getByRole("textbox", { name: "Title" })).toHaveValue("First title");
  });
  it("keeps collection request identity after closing and reopening a failed collection", async () => {
    const requests: unknown[] = [];
    host.mockImplementation(async (command, args: any) => {
      if (command === "workspace_journey") return { entries: [entry], next_offset: null };
      if (command === "workspace_source_detail") return snapshot;
      if (command === "workspace_save_library_item") { requests.push(args.request); if (requests.length === 1) throw new Error("lost response"); return libraryDetail; }
    });
    renderPage();
    fireEvent.click(await screen.findByRole("button", { name: /QC report/ }));
    fireEvent.click(await screen.findByRole("button", { name: "Add to library" }));
    await screen.findByRole("alert");
    fireEvent.keyDown(window, { key: "Escape" });
    fireEvent.click(screen.getByRole("button", { name: /QC report/ }));
    fireEvent.click(await screen.findByRole("button", { name: /Add to library|Retry collection/ }));
    await screen.findByText("Added to library", { selector: "p" });
    expect(requests[1]).toEqual(requests[0]);
  });
  it("filters library by search, kind and source project, and exposes changed source state", async () => {
    host.mockImplementation(async (command, args: any) => {
      if (command === "workspace_list_library") return { items: args.request.query === "empty" ? [] : [libraryItem], next_offset: null };
      if (command === "workspace_get_library_item") return { ...libraryDetail, snapshot: { ...libraryDetail.snapshot, availability: "changed" } };
    });
    renderPage({ page: "library" });
    fireEvent.change(screen.getByRole("combobox", { name: "Library type" }), { target: { value: "excerpt" } });
    fireEvent.change(screen.getByRole("combobox", { name: "Source project" }), { target: { value: "p1" } });
    fireEvent.click(await screen.findByRole("button", { name: /Methods/ }));
    expect(await screen.findByText("Source changed")).toBeInTheDocument();
    fireEvent.keyDown(window, { key: "Escape" });
    fireEvent.change(screen.getByRole("searchbox", { name: "Search library" }), { target: { value: "empty" } });
    expect(await screen.findByText("No saved items match these filters.")).toBeInTheDocument();
    expect(host).toHaveBeenCalledWith("workspace_list_library", { request: { query: "empty", kind: "excerpt", project_id: "p1", offset: 0, limit: 25 } });
  });
  it("does not ask twice when an approved leave callback passes through the parent navigation guard", async () => {
    let guard!: (next: () => void) => void;
    const next = vi.fn();
    renderPage({ page: "publication", registerBeforeLeave: (value) => { guard = value; } });
    fireEvent.click(screen.getByRole("button", { name: "New manuscript" }));
    fireEvent.change(screen.getByRole("textbox", { name: "Markdown" }), { target: { value: "Unsaved" } });
    act(() => guard(() => guard(next)));
    fireEvent.click(screen.getByRole("button", { name: "Discard and leave" }));
    expect(next).toHaveBeenCalledTimes(1);
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });
  it("retries an uncertain collection even if its source becomes unavailable", async () => {
    let detailReads = 0;
    const requests: unknown[] = [];
    host.mockImplementation(async (command, args: any) => {
      if (command === "workspace_journey") return { entries: [{ ...entry, source: { ...source, id: "disappearing" } }], next_offset: null };
      if (command === "workspace_source_detail") { if (++detailReads > 1) throw new Error("source deleted"); return snapshot; }
      if (command === "workspace_save_library_item") { requests.push(args.request); if (requests.length === 1) throw new Error("response lost"); return libraryDetail; }
    });
    renderPage();
    fireEvent.click(await screen.findByRole("button", { name: /QC report/ }));
    fireEvent.click(await screen.findByRole("button", { name: "Add to library" }));
    await screen.findByRole("alert");
    fireEvent.keyDown(window, { key: "Escape" });
    fireEvent.click(screen.getByRole("button", { name: /QC report/ }));
    expect(await screen.findByRole("alert")).toHaveTextContent("source deleted");
    fireEvent.click(screen.getByRole("button", { name: "Retry collection" }));
    expect(await screen.findByText("Added to library", { selector: "p" })).toBeInTheDocument();
    expect(requests[1]).toEqual(requests[0]);
  });
  it("completes cancelled navigation callbacks once on Escape, keep editing and supersession", () => {
    let guard!: (next: () => void, onCancel?: () => void) => void;
    const next = vi.fn();
    const firstCancel = vi.fn();
    const secondCancel = vi.fn();
    const escapeCancel = vi.fn();
    renderPage({ page: "publication", registerBeforeLeave: (value) => { guard = value; } });
    fireEvent.click(screen.getByRole("button", { name: "New manuscript" }));
    fireEvent.change(screen.getByRole("textbox", { name: "Markdown" }), { target: { value: "Unsaved" } });
    act(() => guard(next, firstCancel));
    act(() => guard(next, secondCancel));
    expect(firstCancel).toHaveBeenCalledTimes(1);
    fireEvent.click(screen.getByRole("button", { name: "Keep editing" }));
    expect(secondCancel).toHaveBeenCalledTimes(1);
    act(() => guard(next, escapeCancel));
    fireEvent.keyDown(window, { key: "Escape" });
    expect(escapeCancel).toHaveBeenCalledTimes(1);
    expect(next).not.toHaveBeenCalled();
  });
});
