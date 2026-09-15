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
});
