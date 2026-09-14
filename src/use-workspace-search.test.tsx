import { act, renderHook, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import * as referenceApi from "./composer-reference-api";
import { useWorkspaceSearch } from "./use-workspace-search";
import type { ComposerCatalogItem } from "./types";

const projects = [
  { id: "p1", name: "Cells", description: "RNA analysis" },
  { id: "p2", name: "Papers", description: "Literature" },
];

const artifact = (projectId: string, id: string, label = "Report"): ComposerCatalogItem => ({
  reference: { kind: "artifact", project_id: projectId, id },
  label,
  description: "Saved output",
});

const session = (projectId: string, id: string): ComposerCatalogItem => ({
  reference: { kind: "session", project_id: projectId, id },
  label: "Earlier discussion",
  description: "Saved transcript",
});

afterEach(() => vi.restoreAllMocks());

describe("useWorkspaceSearch", () => {
  it("indexes projects immediately and lazily loads catalogs only when open", async () => {
    let resolveP1!: (items: ComposerCatalogItem[]) => void;
    let resolveP2!: (items: ComposerCatalogItem[]) => void;
    const request = vi.spyOn(referenceApi, "composerReferenceCatalog")
      .mockImplementationOnce(() => new Promise((resolve) => { resolveP1 = resolve; }))
      .mockImplementationOnce(() => new Promise((resolve) => { resolveP2 = resolve; }));

    const { result, rerender } = renderHook(
      ({ open, currentProjects }: { open: boolean; currentProjects: typeof projects }) => useWorkspaceSearch(open, currentProjects),
      { initialProps: { open: false, currentProjects: projects } },
    );

    expect(request).not.toHaveBeenCalled();
    expect(result.current.entries.map((entry) => entry.kind)).toEqual(["project", "project"]);

    rerender({ open: true, currentProjects: projects });
    expect(result.current.entries.filter((entry) => entry.kind === "project")).toHaveLength(2);
    await waitFor(() => expect(result.current.loading).toBe(true));
    expect(request).toHaveBeenCalledTimes(2);
    expect(request).toHaveBeenNthCalledWith(1, "p1");
    expect(request).toHaveBeenNthCalledWith(2, "p2");

    resolveP1([artifact("p1", "a1")]);
    resolveP2([session("p2", "s2")]);
    await waitFor(() => expect(result.current.loading).toBe(false));
    expect(result.current.entries.map((entry) => entry.kind)).toEqual(["project", "artifact", "project", "session"]);
  });

  it("keeps successful project catalogs and project rows when another fetch fails", async () => {
    const request = vi.spyOn(referenceApi, "composerReferenceCatalog")
      .mockResolvedValueOnce([artifact("p1", "a1")])
      .mockRejectedValueOnce(new Error("p2 unavailable"));
    const { result } = renderHook(() => useWorkspaceSearch(true, projects));

    await waitFor(() => expect(result.current.loading).toBe(false));
    expect(result.current.failedProjects).toBe(1);
    expect(result.current.entries.filter((entry) => entry.kind === "project")).toHaveLength(2);
    expect(result.current.entries.some((entry) => entry.key === "artifact:p1:a1")).toBe(true);
    expect(result.current.entries.some((entry) => entry.key.startsWith("artifact:p2:"))).toBe(false);
    expect(request).toHaveBeenCalledTimes(2);
  });

  it("refetches every current project on retry and publishes only the retry result", async () => {
    let resolveOld!: (items: ComposerCatalogItem[]) => void;
    let resolveFresh!: (items: ComposerCatalogItem[]) => void;
    const request = vi.spyOn(referenceApi, "composerReferenceCatalog")
      .mockImplementationOnce(() => new Promise((resolve) => { resolveOld = resolve; }))
      .mockImplementationOnce(() => Promise.resolve([]))
      .mockImplementationOnce(() => new Promise((resolve) => { resolveFresh = resolve; }))
      .mockImplementationOnce(() => Promise.resolve([]));
    const { result } = renderHook(() => useWorkspaceSearch(true, projects));
    await waitFor(() => expect(request).toHaveBeenCalledTimes(2));

    act(() => result.current.retry());
    await waitFor(() => expect(request).toHaveBeenCalledTimes(4));
    expect(result.current.loading).toBe(true);

    resolveOld([artifact("p1", "stale")]);
    await act(async () => {});
    expect(result.current.entries.some((entry) => entry.key === "artifact:p1:stale")).toBe(false);

    resolveFresh([artifact("p1", "fresh")]);
    // The other retry request resolves with the browser-safe empty catalog.
    await waitFor(() => expect(result.current.loading).toBe(false));
    expect(result.current.entries.some((entry) => entry.key === "artifact:p1:fresh")).toBe(true);
    expect(result.current.entries.some((entry) => entry.key === "artifact:p1:stale")).toBe(false);
  });

  it("ignores a response that arrives after the search closes", async () => {
    let resolve!: (items: ComposerCatalogItem[]) => void;
    const request = vi.spyOn(referenceApi, "composerReferenceCatalog")
      .mockImplementation(() => new Promise((complete) => { resolve = complete; }));
    const { result, rerender } = renderHook(
      ({ open }: { open: boolean }) => useWorkspaceSearch(open, projects),
      { initialProps: { open: true } },
    );
    await waitFor(() => expect(request).toHaveBeenCalledTimes(2));

    rerender({ open: false });
    resolve([artifact("p1", "late")]);
    await act(async () => {});
    expect(result.current.loading).toBe(false);
    expect(result.current.failedProjects).toBe(0);
    expect(result.current.entries.map((entry) => entry.kind)).toEqual(["project", "project"]);
  });

  it("ignores responses for a switched project set", async () => {
    let resolveOld!: (items: ComposerCatalogItem[]) => void;
    let resolveNew!: (items: ComposerCatalogItem[]) => void;
    const request = vi.spyOn(referenceApi, "composerReferenceCatalog")
      .mockImplementationOnce(() => new Promise((resolve) => { resolveOld = resolve; }))
      .mockImplementationOnce(() => new Promise((resolve) => { resolveNew = resolve; }));
    const { result, rerender } = renderHook(
      ({ currentProjects }: { currentProjects: Array<{ id: string; name: string; description?: string }> }) => useWorkspaceSearch(true, currentProjects),
      { initialProps: { currentProjects: [projects[0]] } },
    );
    await waitFor(() => expect(request).toHaveBeenCalledWith("p1"));

    rerender({ currentProjects: [projects[1]] });
    await waitFor(() => expect(request).toHaveBeenCalledWith("p2"));
    resolveOld([artifact("p1", "old")]);
    await act(async () => {});
    expect(result.current.entries.some((entry) => entry.key === "artifact:p1:old")).toBe(false);
    expect(result.current.entries.map((entry) => entry.projectId)).toEqual(["p2"]);

    resolveNew([artifact("p2", "new")]);
    await waitFor(() => expect(result.current.loading).toBe(false));
    expect(result.current.entries.some((entry) => entry.key === "artifact:p2:new")).toBe(true);
  });

  it("does not refetch solely because the caller creates a new equivalent project array", async () => {
    const request = vi.spyOn(referenceApi, "composerReferenceCatalog").mockResolvedValue([]);
    const { result, rerender } = renderHook(
      ({ currentProjects }: { currentProjects: typeof projects }) => useWorkspaceSearch(true, currentProjects),
      { initialProps: { currentProjects: projects } },
    );
    await waitFor(() => expect(result.current.loading).toBe(false));

    rerender({ currentProjects: projects.map((project) => ({ ...project })) });
    await act(async () => {});
    expect(request).toHaveBeenCalledTimes(2);
  });
});
