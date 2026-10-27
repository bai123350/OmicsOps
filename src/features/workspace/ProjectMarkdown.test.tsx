import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { ProjectImagePreview } from "../../types";
import { ProjectMarkdown, ProjectMarkdownImages } from "./ProjectMarkdown";

const dataUrl = "data:image/png;base64,iVBORw0KGgo=";
const preview: ProjectImagePreview = { relative_path: "figures/umap.png", mime_type: "image/png", size_bytes: 8, sha256: "a".repeat(64), data_url: dataUrl };

function content(markdown: string, load?: (path: string) => Promise<ProjectImagePreview>, scopeKey = "project:chat:ssh:root", remoteRoot = "/research/hcc") {
  return <ProjectMarkdownImages scopeKey={scopeKey} remoteRoot={remoteRoot} onPreviewImage={load} zh={false}><ProjectMarkdown markdown={markdown} /></ProjectMarkdownImages>;
}

describe("project Markdown images", () => {
  it.each([
    ["figures/umap.png", "figures/umap.png"],
    ["./figures/umap.png", "figures/umap.png"],
    ["/research/hcc/figures/umap.png", "figures/umap.png"],
    ["file:///research/hcc/figures/umap.png", "figures/umap.png"],
    ["figures/UMAP%20%E5%9B%BE%23one.PNG?download=1#view", "figures/UMAP 图#one.PNG"],
  ])("resolves %s through the host preview", async (source, relativePath) => {
    const load = vi.fn().mockResolvedValue(preview);
    render(content(`![UMAP](${source} "Cell atlas")`, load));
    await waitFor(() => expect(screen.getByRole("img", { name: "UMAP" })).toHaveAttribute("src", dataUrl));
    expect(screen.getByRole("img")).toHaveAttribute("title", "Cell atlas");
    expect(load).toHaveBeenCalledTimes(1);
    expect(load).toHaveBeenCalledWith(relativePath);
  });

  it.each(["../outside.png", "%2e%2e/outside.png", "/research/hcc/../secret.png", "/research/hcc-other/umap.png", "file:///etc/umap.png", "file://server/share/umap.png", "C:/private/umap.png", "figures/%5c..%5csecret.png", "figures/%00.png", "././.omicsops/secret.png", "figures/a.svg", "javascript:alert(1)", "data:image/png;base64,AAAA", "figures/bad%ZZ.png"])("does not fetch unsafe or unsupported source %s", async (source) => {
    const load = vi.fn();
    render(content(`![UMAP](${source})`, load));
    await act(async () => {});
    expect(load).not.toHaveBeenCalled();
    expect(screen.queryByRole("img")).toBeNull();
    expect(screen.getByText(/Image unavailable/)).toBeVisible();
  });

  it("keeps web images on their original URL without sending them to SSH", async () => {
    const load = vi.fn();
    render(content("![Web](https://example.test/umap.png)", load));
    await act(async () => {});
    expect(screen.getByRole("img", { name: "Web" })).toHaveAttribute("src", "https://example.test/umap.png");
    expect(load).not.toHaveBeenCalled();
  });

  it("shows a placeholder instead of fetching a project path from the webview without a loader", () => {
    render(content("![UMAP](figures/umap.png)"));
    expect(screen.queryByRole("img")).toBeNull();
    expect(screen.getByText(/Image unavailable/)).toBeVisible();
  });

  it("shows loading, hides raw host errors, and retries only when requested", async () => {
    let reject!: (reason: unknown) => void;
    const pending = new Promise<ProjectImagePreview>((_, fail) => { reject = fail; });
    const load = vi.fn().mockReturnValueOnce(pending).mockResolvedValue(preview);
    render(content("![UMAP](figures/umap.png)", load));
    expect(screen.getByText(/Loading image/)).toBeVisible();
    expect(screen.queryByRole("img")).toBeNull();
    await act(async () => reject(new Error("private host diagnostic")));
    expect(screen.queryByText(/private host diagnostic/)).toBeNull();
    expect(screen.getByText(/Could not load image/)).toBeVisible();
    expect(load).toHaveBeenCalledTimes(1);
    fireEvent.click(screen.getByRole("button", { name: "Retry image" }));
    await waitFor(() => expect(screen.getByRole("img")).toHaveAttribute("src", dataUrl));
    expect(load).toHaveBeenCalledTimes(2);
    fireEvent.error(screen.getByRole("img"));
    expect(screen.getByText(/Could not load image/)).toBeVisible();
  });

  it("does not refetch on streaming text updates or callback identity changes", async () => {
    const load = vi.fn().mockResolvedValue(preview);
    const view = render(content("![UMAP](figures/umap.png)", (path) => load(path)));
    await screen.findByRole("img");
    view.rerender(content("![UMAP](figures/umap.png)\n\nMore text", (path) => load(path)));
    expect(screen.getByRole("img")).toHaveAttribute("src", dataUrl);
    expect(load).toHaveBeenCalledTimes(1);
  });

  it("shares an in-flight preview for repeated references to the same image", async () => {
    const load = vi.fn().mockResolvedValue(preview);
    render(content("![First](figures/umap.png)\n\n![Second](./figures/umap.png)", load));
    await waitFor(() => expect(screen.getAllByRole("img")).toHaveLength(2));
    expect(screen.getByRole("img", { name: "First" })).toHaveAttribute("src", dataUrl);
    expect(screen.getByRole("img", { name: "Second" })).toHaveAttribute("src", dataUrl);
    expect(load).toHaveBeenCalledTimes(1);
  });

  it.each(["scope", "source"])("drops late image responses after a %s change", async (change) => {
    let finish!: (value: ProjectImagePreview) => void;
    const oldLoad = vi.fn().mockReturnValue(new Promise((resolve) => { finish = resolve; }));
    const newLoad = vi.fn().mockResolvedValue({ ...preview, data_url: "data:image/png;base64,bmV3" });
    const view = render(content("![UMAP](figures/umap.png)", oldLoad));
    view.rerender(content(`![UMAP](figures/${change === "source" ? "other" : "umap"}.png)`, newLoad, change === "scope" ? "other-project:chat:ssh:root" : undefined));
    await waitFor(() => expect(screen.getByRole("img")).toHaveAttribute("src", "data:image/png;base64,bmV3"));
    await act(async () => finish(preview));
    expect(screen.getByRole("img")).toHaveAttribute("src", "data:image/png;base64,bmV3");
  });

  it("clears an already loaded image when its binding changes", async () => {
    const load = vi.fn().mockResolvedValue(preview);
    const view = render(content("![UMAP](figures/umap.png)", load));
    await screen.findByRole("img");
    view.rerender(content("![UMAP](figures/umap.png)", () => new Promise(() => {}), "project:chat:other-ssh:root"));
    expect(screen.queryByRole("img")).toBeNull();
    expect(screen.getByText(/Loading image/)).toBeVisible();
  });
});
