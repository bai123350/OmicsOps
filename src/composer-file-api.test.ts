import { afterEach, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { parseClipboardFilePaths, parseWorkspaceFileDrag, resolveComposerClipboardPaths, listLocalComposerFiles, previewComposerFileText, createComposerQuote } from "./composer-file-api";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
afterEach(() => { Reflect.deleteProperty(window, "__TAURI_INTERNALS__"); vi.resetAllMocks(); });

it("sends source-bound preview and quote requests without pretending to read in a browser", async () => {
  const reference = { kind: "workspace_file" as const, project_id: "p", backend_id: "local", relative_path: "notes.md" };
  await expect(previewComposerFileText(reference)).rejects.toThrow("desktop");
  const request = { project_id: "p", conversation_id: "c", backend_id: "local", relative_path: "notes.md", sha256: "hash", text: "selection" };
  await expect(createComposerQuote(request)).rejects.toThrow("desktop");
  Object.defineProperty(window, "__TAURI_INTERNALS__", { value: {}, configurable: true });
  await previewComposerFileText(reference);
  expect(invoke).toHaveBeenCalledWith("preview_composer_file_text", { projectId: "p", backendId: "local", relativePath: "notes.md" });
  await createComposerQuote(request);
  expect(invoke).toHaveBeenCalledWith("create_composer_quote", { request });
});

it("recognizes filesystem-only clipboard text without consuming commands, URLs or ordinary prose", () => {
  expect(parseClipboardFilePaths('"C:\\research data\\counts.csv"\r\n"C:\\research data\\plot.png"')).toEqual(["C:\\research data\\counts.csv", "C:\\research data\\plot.png"]);
  expect(parseClipboardFilePaths("/tmp/results/counts.csv")).toEqual(["/tmp/results/counts.csv"]);
  expect(parseClipboardFilePaths("./results/counts.csv")).toEqual(["./results/counts.csv"]);
  for (const text of ["/plan", "See results/counts.csv", "https://example.org/results.csv", "normal text", "C:\\one.csv\nordinary text"]) expect(parseClipboardFilePaths(text)).toBeNull();
});

it("only accepts typed internal file drag records and leaves authority to the host", () => {
  const reference = { kind: "workspace_file", project_id: "project-a", backend_id: "ssh:connection-a", relative_path: "results/counts.csv" };
  expect(parseWorkspaceFileDrag(JSON.stringify(reference))).toEqual(reference);
  expect(parseWorkspaceFileDrag("not JSON")).toBeNull();
  expect(parseWorkspaceFileDrag(JSON.stringify({ ...reference, kind: "image" }))).toBeNull();
  expect(parseWorkspaceFileDrag(JSON.stringify({ ...reference, relative_path: 42 }))).toBeNull();
});

it("keeps previews empty and sends clipboard paths only to host validation", async () => {
  expect(await listLocalComposerFiles("project-a")).toEqual([]);
  await expect(resolveComposerClipboardPaths("project-a", ["C:\\one.csv"])).rejects.toThrow("desktop");
  Object.defineProperty(window, "__TAURI_INTERNALS__", { value: {}, configurable: true });
  vi.mocked(invoke).mockResolvedValueOnce([]);
  await resolveComposerClipboardPaths("project-a", ["C:\\one.csv"]);
  expect(invoke).toHaveBeenCalledWith("resolve_composer_clipboard_paths", { projectId: "project-a", paths: ["C:\\one.csv"] });
});
