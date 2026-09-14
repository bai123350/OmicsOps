import { describe, expect, it } from "vitest";
import { buildWorkspaceSearchEntries, canAttachSearchEntry, filterWorkspaceSearchEntries } from "./workspace-search";

const projects = [{ id: "p1", name: "Cells", description: "RNA analysis" }, { id: "p2", name: "Papers", description: "Literature" }];
describe("workspace search index", () => {
  it("keeps source projects and deduplicates globally enabled skills", () => {
    const entries = buildWorkspaceSearchEntries(projects, new Map([
      ["p1", [
        { reference: { kind: "artifact" as const, project_id: "p1", id: "a" }, label: "QC report", description: "Counts" },
        { reference: { kind: "skill" as const, id: "s" }, label: "QC", description: "Quality checks" },
      ]],
      ["p2", [{ reference: { kind: "skill" as const, id: "s" }, label: "QC", description: "Quality checks" }]],
    ]));
    expect(entries.filter((entry) => entry.kind === "project")).toHaveLength(2);
    expect(entries.filter((entry) => entry.kind === "skill")).toHaveLength(1);
    expect(entries.find((entry) => entry.kind === "artifact")?.projectId).toBe("p1");
    expect(filterWorkspaceSearchEntries(entries, "cells")).toHaveLength(2);
    expect(filterWorkspaceSearchEntries(entries, "QUALITY")).toHaveLength(1);
  });
  it("rejects stale catalogs for removed projects and mismatched ownership", () => {
    const entries = buildWorkspaceSearchEntries(projects.slice(0, 1), new Map([
      ["p1", [{ reference: { kind: "session" as const, project_id: "p2", id: "foreign" }, label: "Wrong owner", description: "" }]],
      ["deleted", [{ reference: { kind: "artifact" as const, project_id: "deleted", id: "a" }, label: "Removed", description: "" }]],
    ]));
    expect(entries.map((entry) => entry.kind)).toEqual(["project"]);
  });
  it("separates opening a result from references supported in the current conversation", () => {
    const entries = buildWorkspaceSearchEntries(projects, new Map([["p1", [
      { reference: { kind: "session" as const, project_id: "p1", id: "current" }, label: "Current", description: "" },
      { reference: { kind: "artifact" as const, project_id: "p1", id: "a" }, label: "Report", description: "" },
    ]]]));
    expect(canAttachSearchEntry(entries[0], "p1", "current")).toBe(true);
    expect(canAttachSearchEntry(entries[0], "p2", "current")).toBe(false);
    const session = entries.find((entry) => entry.kind === "session")!;
    const artifact = entries.find((entry) => entry.kind === "artifact")!;
    expect(canAttachSearchEntry(session, "p1", "current")).toBe(false);
    expect(canAttachSearchEntry(artifact, "p1", "current")).toBe(true);
    expect(canAttachSearchEntry(artifact, "p2", "another")).toBe(false);
    expect(canAttachSearchEntry(artifact, undefined, undefined)).toBe(false);
  });

  it("keeps compute references out of the global search index", () => {
    const entries = buildWorkspaceSearchEntries(projects.slice(0, 1), new Map([[
      "p1",
      [
        { reference: { kind: "execution_context" as const, project_id: "p1", backend_id: "local" }, label: "Local context", description: "unverified" },
        { reference: { kind: "runtime" as const, project_id: "p1", backend_id: "local", language: "python" as const }, label: "Python", description: "unverified" },
        { reference: { kind: "project" as const, project_id: "p1", id: "p1" }, label: "Cells", description: "Current" },
      ],
    ]]));
    expect(entries.map((entry) => entry.kind)).toEqual(["project"]);
    expect(entries[0].item?.reference).toEqual({ kind: "project", project_id: "p1", id: "p1" });
  });
});
