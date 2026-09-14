import { afterEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { composerReferenceCatalog, validateComposerReferences } from "./composer-reference-api";
import type { ComposerReference } from "./types";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
afterEach(() => {
  Reflect.deleteProperty(window, "__TAURI_INTERNALS__");
  vi.clearAllMocks();
});
describe("composerReferenceCatalog", () => {
  it("returns no invented references in browser preview", async () => {
    expect(await composerReferenceCatalog("project")).toEqual([]);
    expect(invoke).not.toHaveBeenCalled();
  });
  it("requests a host-owned project catalog and propagates failure for retry", async () => {
    Object.defineProperty(window, "__TAURI_INTERNALS__", { value: {}, configurable: true });
    vi.mocked(invoke).mockRejectedValueOnce(new Error("unavailable")).mockResolvedValueOnce([]);
    await expect(composerReferenceCatalog("project")).rejects.toThrow("unavailable");
    expect(await composerReferenceCatalog("project")).toEqual([]);
    expect(invoke).toHaveBeenLastCalledWith("composer_reference_catalog", { projectId: "project" });
  });
});

describe("validateComposerReferences", () => {
  const references: ComposerReference[] = [
    { kind: "artifact", project_id: "project", id: "artifact" },
    { kind: "skill", id: "skill" },
  ];

  it("is a no-op in browser preview", async () => {
    await expect(validateComposerReferences("project", "conversation", references)).resolves.toBeUndefined();
    expect(invoke).not.toHaveBeenCalled();
  });

  it("requests host validation with project, conversation, and references", async () => {
    Object.defineProperty(window, "__TAURI_INTERNALS__", { value: {}, configurable: true });
    vi.mocked(invoke).mockResolvedValueOnce(undefined);

    await expect(validateComposerReferences("project", "conversation", references)).resolves.toBeUndefined();
    expect(invoke).toHaveBeenLastCalledWith("validate_composer_references", {
      projectId: "project",
      conversationId: "conversation",
      references,
    });
  });

  it("propagates host validation failures", async () => {
    Object.defineProperty(window, "__TAURI_INTERNALS__", { value: {}, configurable: true });
    vi.mocked(invoke).mockRejectedValueOnce(new Error("reference disappeared"));

    await expect(validateComposerReferences("project", "conversation", references)).rejects.toThrow("reference disappeared");
  });
});
