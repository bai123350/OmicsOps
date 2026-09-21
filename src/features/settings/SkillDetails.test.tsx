import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { useState } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import * as api from "../../skill-settings-api";
import type { SkillSettingsDetail } from "../../types";
import { useWindowEscapeLayer } from "./BrowserSettings";
import { SkillDetails } from "./SkillDetails";

vi.mock("../../skill-settings-api", () => ({
  settingsSkillDetail: vi.fn(),
  settingsReadSkillFile: vi.fn(),
  settingsRemoveSkill: vi.fn(),
}));

const detail = (): SkillSettingsDetail => ({
  skill: { id: "skill-1", name: "QC reviewer", version: "1.0.0", source_path: "C:/managed/qc/sha", sha256: "a".repeat(64), enabled: true, capabilities: ["read_project_files"], category: "analysis" },
  origin: "managed_import",
  integrity: "verified",
  files: [
    { relative_path: "SKILL.md", size_bytes: 42, previewable: true },
    { relative_path: "asset.bin", size_bytes: 12, previewable: false },
  ],
  inventory_complete: true,
  dependent_skills: [],
  can_remove_from_library: true,
  can_delete_files: true,
  blocking_reasons: [],
});

describe("SkillDetails", () => {
  beforeEach(() => {
    vi.mocked(api.settingsSkillDetail).mockReset().mockResolvedValue(detail());
    vi.mocked(api.settingsReadSkillFile).mockReset().mockResolvedValue({ relative_path: "SKILL.md", content: "# QC", redacted: false, package_sha256: "a".repeat(64) });
    vi.mocked(api.settingsRemoveSkill).mockReset().mockResolvedValue({ removed_from_library: true, files_removed: true, preserved_files: false, status: "removed", message: "Removed" });
  });

  it("loads real host details and previews only the selected file", async () => {
    render(<SkillDetails skillId="skill-1" locale="en-US" onClose={() => undefined} />);
    expect(screen.getByRole("status")).toHaveTextContent("Validating");
    expect(await screen.findByRole("heading", { name: "QC reviewer" })).toBeInTheDocument();
    expect(api.settingsSkillDetail).toHaveBeenCalledWith("skill-1");
    expect(screen.getByText("Managed import")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Not previewable" })).toBeDisabled();

    fireEvent.click(screen.getByRole("button", { name: "Preview" }));
    expect(await screen.findByRole("dialog", { name: "Skill file preview" })).toBeInTheDocument();
    expect(api.settingsReadSkillFile).toHaveBeenCalledWith("skill-1", "SKILL.md", "a".repeat(64));
    expect(screen.getByText("# QC")).toBeInTheDocument();
  });

  it("sizes details and preview dialogs inside the compensated overlay", async () => {
    document.documentElement.dataset.omicsopsScale = "1.2";
    const view = render(<SkillDetails skillId="skill-1" locale="en-US" onClose={() => undefined} />);
    await screen.findByRole("heading", { name: "QC reviewer" });

    const details = screen.getByRole("dialog", { name: "Skill details" });
    expect(details).toHaveClass("skill-details-dialog");
    expect(details).toHaveStyle({ width: "min(720px, calc(100% - 36px))", maxHeight: "calc(100% - 48px)" });

    fireEvent.click(screen.getByRole("button", { name: "Preview" }));
    const preview = await screen.findByRole("dialog", { name: "Skill file preview" });
    expect(preview).toHaveClass("skill-preview-dialog");
    expect(preview).toHaveStyle({ width: "min(780px, calc(100% - 28px))", maxHeight: "calc(100% - 32px)" });

    view.unmount();
    delete document.documentElement.dataset.omicsopsScale;
  });

  it("keeps details open and blocks duplicate preview reads after a failure", async () => {
    let reject!: (reason: unknown) => void;
    vi.mocked(api.settingsReadSkillFile).mockImplementation(() => new Promise((_, nextReject) => { reject = nextReject; }));
    render(<SkillDetails skillId="skill-1" locale="en-US" onClose={() => undefined} />);
    await screen.findByRole("heading", { name: "QC reviewer" });
    const preview = screen.getByRole("button", { name: "Preview" });
    fireEvent.click(preview);
    fireEvent.click(preview);
    expect(api.settingsReadSkillFile).toHaveBeenCalledTimes(1);
    reject(new Error("secret transport detail"));
    expect(await screen.findByRole("alert")).toHaveTextContent("Could not safely preview");
    expect(screen.queryByText("secret transport detail")).not.toBeInTheDocument();
    expect(screen.getByRole("dialog", { name: "Skill details" })).toBeInTheDocument();
  });

  it("closes the preview before details and the parent Escape layer", async () => {
    const closeDetails = vi.fn();
    const closeParent = vi.fn();
    function Harness() {
      const [open, setOpen] = useState(false);
      useWindowEscapeLayer(true, closeParent);
      return <><button onClick={() => setOpen(true)}>Open details</button>{open && <SkillDetails skillId="skill-1" locale="en-US" onClose={closeDetails} />}</>;
    }
    render(<Harness />);
    fireEvent.click(screen.getByRole("button", { name: "Open details" }));
    await screen.findByRole("heading", { name: "QC reviewer" });
    fireEvent.click(screen.getByRole("button", { name: "Preview" }));
    await screen.findByRole("dialog", { name: "Skill file preview" });

    fireEvent.keyDown(window, { key: "Escape" });
    await waitFor(() => expect(screen.queryByRole("dialog", { name: "Skill file preview" })).not.toBeInTheDocument());
    expect(closeDetails).not.toHaveBeenCalled();
    expect(closeParent).not.toHaveBeenCalled();
  });

  it("closes details before a parent layer when no file preview is open", async () => {
    const closeDetails = vi.fn();
    const closeParent = vi.fn();
    function Harness() {
      const [open, setOpen] = useState(false);
      useWindowEscapeLayer(true, closeParent);
      return <><button onClick={() => setOpen(true)}>Open details</button>{open && <SkillDetails skillId="skill-1" locale="en-US" onClose={closeDetails} />}</>;
    }
    render(<Harness />);
    fireEvent.click(screen.getByRole("button", { name: "Open details" }));
    await screen.findByRole("heading", { name: "QC reviewer" });
    fireEvent.keyDown(window, { key: "Escape" });
    expect(closeDetails).toHaveBeenCalledOnce();
    expect(closeParent).not.toHaveBeenCalled();
  });

  it("ignores a late detail response after switching skill IDs", async () => {
    let resolveFirst!: (value: SkillSettingsDetail) => void;
    vi.mocked(api.settingsSkillDetail)
      .mockImplementationOnce(() => new Promise((resolve) => { resolveFirst = resolve; }))
      .mockResolvedValueOnce({ ...detail(), skill: { ...detail().skill, id: "skill-2", name: "Second" } });
    const view = render(<SkillDetails skillId="skill-1" locale="en-US" onClose={() => undefined} />);
    view.rerender(<SkillDetails skillId="skill-2" locale="en-US" onClose={() => undefined} />);
    expect(await screen.findByRole("heading", { name: "Second" })).toBeInTheDocument();
    resolveFirst(detail());
    await waitFor(() => expect(screen.queryByRole("heading", { name: "QC reviewer" })).not.toBeInTheDocument());
  });

  it("clears a stale preview operation when switching skill IDs", async () => {
    let resolvePreview!: (value: { relative_path: string; content: string; redacted: boolean; package_sha256: string }) => void;
    vi.mocked(api.settingsReadSkillFile).mockImplementationOnce(() => new Promise((resolve) => { resolvePreview = resolve; }));
    vi.mocked(api.settingsSkillDetail)
      .mockResolvedValueOnce(detail())
      .mockResolvedValueOnce({ ...detail(), skill: { ...detail().skill, id: "skill-2", name: "Second" } });

    const view = render(<SkillDetails skillId="skill-1" locale="en-US" onClose={() => undefined} />);
    await screen.findByRole("heading", { name: "QC reviewer" });
    fireEvent.click(screen.getByRole("button", { name: "Preview" }));
    expect(screen.getByRole("button", { name: "Reading…" })).toBeDisabled();

    view.rerender(<SkillDetails skillId="skill-2" locale="en-US" onClose={() => undefined} />);
    await screen.findByRole("heading", { name: "Second" });
    expect(screen.getByRole("button", { name: "Preview" })).toBeEnabled();
    resolvePreview({ relative_path: "SKILL.md", content: "old", redacted: false, package_sha256: "a".repeat(64) });
    await waitFor(() => expect(screen.queryByText("old")).not.toBeInTheDocument());
  });

  it("uses separate library and owned-file confirmations and closes only the top Escape layer", async () => {
    const onClose = vi.fn();
    const onRemoved = vi.fn().mockResolvedValue(undefined);
    const onOperationsChanged = vi.fn().mockResolvedValue(undefined);
    render(<SkillDetails skillId="skill-1" locale="en-US" onClose={onClose} onRemoved={onRemoved} onOperationsChanged={onOperationsChanged} />);
    await screen.findByRole("heading", { name: "QC reviewer" });

    fireEvent.click(screen.getByRole("button", { name: "Remove from library (keep installed files)" }));
    expect(screen.getByRole("dialog", { name: "Confirm skill removal" })).toHaveTextContent("Installed files will remain");
    fireEvent.keyDown(window, { key: "Escape" });
    expect(screen.queryByRole("dialog", { name: "Confirm skill removal" })).not.toBeInTheDocument();
    expect(screen.getByRole("dialog", { name: "Skill details" })).toBeInTheDocument();
    expect(onClose).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole("button", { name: "Remove from library (keep installed files)" }));
    fireEvent.click(screen.getByRole("button", { name: "Confirm library removal" }));
    await waitFor(() => expect(api.settingsRemoveSkill).toHaveBeenCalledWith("skill-1", "a".repeat(64), "library_only"));
    expect(onRemoved).toHaveBeenCalledOnce();
    expect(onOperationsChanged).toHaveBeenCalledOnce();
    expect(onClose).toHaveBeenCalledOnce();
  });

  it("keeps partial owned-file cleanup visible and refreshes both catalogs", async () => {
    vi.mocked(api.settingsRemoveSkill).mockResolvedValue({ removed_from_library: true, files_removed: false, preserved_files: true, status: "needs_attention", message: "Verified cleanup is incomplete; files were preserved." });
    const onClose = vi.fn();
    const onRemoved = vi.fn().mockResolvedValue(undefined);
    const onOperationsChanged = vi.fn().mockResolvedValue(undefined);
    render(<SkillDetails skillId="skill-1" locale="en-US" onClose={onClose} onRemoved={onRemoved} onOperationsChanged={onOperationsChanged} />);
    await screen.findByRole("heading", { name: "QC reviewer" });

    fireEvent.click(screen.getByRole("button", { name: "Delete owned installation files" }));
    expect(screen.getByRole("dialog", { name: "Confirm skill removal" })).toHaveTextContent("Changed or unverified files will be preserved");
    fireEvent.click(screen.getByRole("button", { name: "Confirm owned-file deletion" }));

    expect(await screen.findByRole("status", { name: "Skill removal result" })).toHaveTextContent("files were preserved");
    expect(api.settingsRemoveSkill).toHaveBeenCalledWith("skill-1", "a".repeat(64), "owned_files");
    expect(onRemoved).toHaveBeenCalledOnce();
    expect(onOperationsChanged).toHaveBeenCalledOnce();
    expect(onClose).not.toHaveBeenCalled();
    expect(screen.getByRole("dialog", { name: "Skill details" })).toBeInTheDocument();
  });

  it("blocks duplicate removal requests and keeps a sanitized failure retryable", async () => {
    let rejectRemoval!: (reason: unknown) => void;
    vi.mocked(api.settingsRemoveSkill).mockImplementation(() => new Promise((_, reject) => { rejectRemoval = reject; }));
    render(<SkillDetails skillId="skill-1" locale="en-US" onClose={() => undefined} />);
    await screen.findByRole("heading", { name: "QC reviewer" });
    fireEvent.click(screen.getByRole("button", { name: "Delete owned installation files" }));
    fireEvent.click(screen.getByRole("button", { name: "Confirm owned-file deletion" }));
    expect(screen.getByRole("button", { name: "Working…" })).toBeDisabled();
    fireEvent.click(screen.getByRole("button", { name: "Working…" }));
    expect(api.settingsRemoveSkill).toHaveBeenCalledTimes(1);

    rejectRemoval(new Error("token=secret-sentinel"));
    expect(await screen.findByRole("alert")).toHaveTextContent("Skill removal or refresh did not finish");
    expect(screen.queryByText(/secret-sentinel/)).not.toBeInTheDocument();
    expect(screen.getByRole("dialog", { name: "Confirm skill removal" })).toBeInTheDocument();
  });

  it("routes plugin-owned skills to plugin management instead of generic deletion", async () => {
    vi.mocked(api.settingsSkillDetail).mockResolvedValue({ ...detail(), origin: "plugin_owned", can_remove_from_library: false, can_delete_files: false });
    const onNavigatePlugins = vi.fn();
    render(<SkillDetails skillId="skill-1" locale="en-US" onClose={() => undefined} onNavigatePlugins={onNavigatePlugins} />);
    await screen.findByRole("heading", { name: "QC reviewer" });
    fireEvent.click(screen.getByRole("button", { name: "Manage in Plugins" }));
    expect(onNavigatePlugins).toHaveBeenCalledOnce();
    expect(screen.queryByRole("button", { name: /remove from library/i })).not.toBeInTheDocument();
  });
});
