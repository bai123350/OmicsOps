import { act, fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { WorkspaceResearchPages } from "./WorkspaceResearchPages";
import { WorkspaceSearchDialog } from "./WorkspaceSearchDialog";

const { readFileSync } = await vi.importActual<{ readFileSync: (path: string, encoding: "utf8") => string }>("node:fs");
const researchStyles = readFileSync("src/features/workspace/workspace-research-pages.css", "utf8");
const searchStyles = readFileSync("src/features/workspace/workspace-search.css", "utf8");

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
const source = { project_id: "p1", kind: "artifact", id: "a1", conversation_id: "c1", run_id: "r1", sequence: null, event_hash: null, content_sha256: null, start: null, end: null };
const props = { projectId: "p1", locale: "en-US", onOpenConversation: vi.fn(), onInsert: vi.fn() };
function zIndex(dialog: HTMLElement) { return Number(getComputedStyle(dialog.parentElement!).zIndex); }

beforeEach(() => {
  Object.defineProperty(window, "__TAURI_INTERNALS__", { configurable: true, value: {} });
  vi.mocked(invoke).mockReset();
  vi.mocked(invoke).mockImplementation(async (command) => {
    if (command === "workspace_journey") return { entries: [{ source, title: "QC report", status: "unverified", occurred_at: "2026-09-22T01:00:00Z", summary: "Quality control" }], next_offset: null };
    if (command === "workspace_source_detail") return { source, title: "QC report", text: "Metadata", sha256: "hash", status: "unverified", metadata: {}, availability: "available" };
    if (command === "workspace_list_publications") return [];
  });
});

it("shows a later search above source details and immediately escapes only search", async () => {
  const closeSearch = vi.fn();
  function View({ search = false }) {
    return <><style>{researchStyles}{searchStyles}</style><WorkspaceResearchPages {...props} page="journey" />{search && <WorkspaceSearchDialog entries={[]} zh={false} onRetry={vi.fn()} onClose={closeSearch} onOpen={vi.fn()} />}</>;
  }
  const view = render(<View />);
  fireEvent.click(await screen.findByRole("button", { name: /QC report/ }));
  const details = await screen.findByRole("dialog", { name: "Source details" });
  view.rerender(<View search />);
  const search = screen.getByRole("dialog", { name: "Search workspace" });
  expect(zIndex(search)).toBeGreaterThan(zIndex(details));
  fireEvent.keyDown(window, { key: "Escape" });
  expect(closeSearch).toHaveBeenCalledTimes(1);
  expect(details).toBeInTheDocument();
});

it("shows a navigation decision above an existing search and immediately escapes only the decision", () => {
  let guard!: (next: () => void) => void;
  const closeSearch = vi.fn();
  const next = vi.fn();
  render(<><style>{researchStyles}{searchStyles}</style><WorkspaceResearchPages {...props} page="publication" registerBeforeLeave={(value) => { guard = value; }} /><WorkspaceSearchDialog entries={[]} zh={false} onRetry={vi.fn()} onClose={closeSearch} onOpen={vi.fn()} /></>);
  fireEvent.click(screen.getByRole("button", { name: "New manuscript" }));
  fireEvent.change(screen.getByRole("textbox", { name: "Markdown" }), { target: { value: "Unsaved" } });
  act(() => guard(next));
  const decision = screen.getByRole("dialog", { name: "Unsaved manuscript" });
  const search = screen.getByRole("dialog", { name: "Search workspace" });
  expect(zIndex(decision)).toBeGreaterThan(zIndex(search));
  fireEvent.keyDown(window, { key: "Escape" });
  expect(decision).not.toBeInTheDocument();
  expect(search).toBeInTheDocument();
  expect(closeSearch).not.toHaveBeenCalled();
  expect(next).not.toHaveBeenCalled();
});
