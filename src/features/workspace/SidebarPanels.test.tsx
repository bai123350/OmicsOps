import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { ArtifactCatalog, CodeNotebook, ProvenancePanel } from "./SidebarPanels";
import type { ProjectArtifact } from "../../types";

describe("sidebar evidence views", () => {
  it("only previews formats accepted by the host image loader", () => {
    const artifact: ProjectArtifact = { id: "a", project_id: "p", run_id: "r", relative_path: "plot.tiff", remote_path: null, media_type: "image/tiff", size_bytes: 4, sha256: "hash", verified: false, created_at: "" };
    const select = vi.fn();
    const { rerender } = render(<ArtifactCatalog artifacts={[artifact]} locale="en-US" onSelect={select} />);
    expect(screen.queryByRole("button", { name: "Preview" })).not.toBeInTheDocument();
    rerender(<ArtifactCatalog artifacts={[{ ...artifact, relative_path: "plot.png", media_type: "image/png" }]} locale="en-US" onSelect={select} />);
    fireEvent.click(screen.getByRole("button", { name: "Preview" }));
    expect(select).toHaveBeenCalledWith("plot.png");
  });
  it("preserves structured tool data separately from response text", () => {
    render(<ProvenancePanel locale="en-US" rows={[{ id: "e", runId: "r", sequence: 1, occurredAt: "", eventHash: "hash", callId: "c", toolId: "runtime.execute", succeeded: true, output: "model excerpt", data: { software_versions: { python: "3.12" } }, sources: ["kernel:one"], reused: false }]} />);
    fireEvent.click(screen.getByText("Structured tool response"));
    expect(screen.getByText(/software_versions/)).toBeVisible();
    expect(screen.getByText("Tool response text and event hash")).toBeVisible();
  });
  it("reports unavailable clipboard instead of throwing", async () => {
    render(<CodeNotebook locale="en-US" cells={[{ id: "c", language: "python", source: "print(42)", output: "", origin: "assistant", status: "source" }]} />);
    fireEvent.click(screen.getByRole("button", { name: "Copy code 0" }));
    await waitFor(() => expect(screen.getByRole("alert")).toHaveTextContent("Copy failed"));
  });
});
