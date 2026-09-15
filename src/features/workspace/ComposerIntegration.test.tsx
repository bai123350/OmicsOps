import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { ComputeBackendAvailabilityV4 } from "../../types";
import { WorkspaceShell } from "./WorkspaceShell";
import * as referenceApi from "../../composer-reference-api";
import type { SideChatController } from "./useSideChat";

afterEach(() => vi.restoreAllMocks());

const project = {
  id: "project-composer",
  name: "Composer test project",
  status: "ready" as const,
  template: "blank" as const,
};

const localBackend: ComputeBackendAvailabilityV4 = {
  descriptor: {
    schema_version: 4,
    backend_id: "local",
    kind: "local",
    isolation: "process",
    available: true,
    supports_python: true,
    supports_r: false,
    supports_network_policy: false,
  },
  selectable: true,
  reason: null,
  python_status: "available",
  r_status: "unavailable",
  resolved_image_id: null,
};

function renderShell(overrides: Partial<React.ComponentProps<typeof WorkspaceShell>> = {}) {
  return render(
    <WorkspaceShell
      project={project}
      locale="en-US"
      onLocaleChange={vi.fn()}
      computeBackends={[localBackend]}
      computeBackendId="local"
      {...overrides}
    />,
  );
}

function sideController(send = vi.fn().mockResolvedValue(true)): SideChatController {
  return { records: [], loading: false, ready: true, busy: false, pending: false, error: false, modelId: "side-model", setModelId: vi.fn(), draft: "", setDraft: vi.fn(), originalQuestion: undefined, send, retry: vi.fn().mockResolvedValue(true), refresh: vi.fn(), hasMore: false, loadingOlder: false, loadOlder: vi.fn() };
}

describe("WorkspaceShell composer integration", () => {
  it("attaches a search result to the current draft without switching sessions", () => {
    const onSelectConversation = vi.fn();
    const view = renderShell({ activeConversationId: "current", onSelectConversation });
    const input = screen.getByRole("textbox", { name: /Describe a research goal/ });
    fireEvent.change(input, { target: { value: "Keep this goal" } });
    const item = { reference: { kind: "session" as const, project_id: project.id, id: "saved" }, label: "Saved evidence", description: "" };
    view.rerender(<WorkspaceShell project={project} locale="en-US" onLocaleChange={vi.fn()} activeConversationId="current" onSelectConversation={onSelectConversation} searchRequest={{ key: "attach-1", kind: "attach", projectId: project.id, conversationId: "current", item }} />);
    expect(screen.getByRole("button", { name: /Remove reference: Saved evidence/ })).toBeInTheDocument();
    expect(input).toHaveValue("Keep this goal");
    expect(onSelectConversation).not.toHaveBeenCalled();
    view.rerender(<WorkspaceShell project={project} locale="en-US" onLocaleChange={vi.fn()} activeConversationId="different" searchRequest={{ key: "attach-1", kind: "attach", projectId: project.id, conversationId: "current", item }} />);
    expect(screen.queryByRole("button", { name: /Remove reference: Saved evidence/ })).not.toBeInTheDocument();
  });

  it("opens the selected artifact's actual catalog metadata from search", () => {
    renderShell({ searchRequest: { key: "artifact-open", kind: "artifact", projectId: project.id, item: { reference: { kind: "artifact", project_id: project.id, id: "output-42" }, label: "reports/qc.tsv", description: "unverified · 42 bytes" } } });
    expect(screen.getByRole("region", { name: "Selected artifact" })).toHaveTextContent("reports/qc.tsv");
    expect(screen.getByRole("region", { name: "Selected artifact" })).toHaveTextContent("output-42");
    expect(screen.getByRole("region", { name: "Selected artifact" })).toHaveTextContent("unverified");
  });

  it("keeps normal text keyboard behavior when a reference query has no results", async () => {
    const onSend = vi.fn().mockResolvedValue(true);
    renderShell({ onSend, referenceCatalog: [] });
    const input = screen.getByRole("textbox", { name: /Describe a research goal/ });
    fireEvent.change(input, { target: { value: "Discuss @missing", selectionStart: 16 } });
    expect(fireEvent.keyDown(input, { key: "ArrowUp" })).toBe(true);
    fireEvent.keyDown(input, { key: "Enter" });
    await waitFor(() => expect(onSend).toHaveBeenCalledWith("Discuss @missing", "chat"));
  });

  it("offers real slash actions beside skills without sending the command to the model", async () => {
    const onSend = vi.fn();
    const onOpenSettings = vi.fn();
    renderShell({ onSend, onOpenSettings, referenceCatalog: [{ reference: { kind: "skill", id: "skill-1" }, label: "QC", description: "Check counts" }] });
    const input = screen.getByRole("textbox", { name: /Describe a research goal/ });
    fireEvent.change(input, { target: { value: "/", selectionStart: 1 } });
    expect(await screen.findByRole("option", { name: /\/skills/ })).toBeInTheDocument();
    expect(screen.getByRole("option", { name: /QC/ })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("option", { name: /\/skills/ }));
    expect(onOpenSettings).toHaveBeenCalledWith("skills");
    expect(onSend).not.toHaveBeenCalled();
    expect(input).toHaveValue("");
  });

  it("runs /btw with explicit question text and preserves selected references", async () => {
    const send = vi.fn();
    const sideSend = vi.fn().mockResolvedValue(true);
    const reference = { kind: "artifact" as const, project_id: project.id, id: "artifact-btw" };
    renderShell({ onSend: send, onQueue: vi.fn(), agentBusy: true, sideChat: sideController(sideSend), activeConversationId: "current", referenceCatalog: [{ reference, label: "QC report", description: "Counts" }] });
    const input = screen.getByRole("textbox", { name: /Describe a research goal/ });
    fireEvent.change(input, { target: { value: "Use @QC", selectionStart: 7 } });
    fireEvent.click(await screen.findByRole("option", { name: /QC report/ }));
    fireEvent.change(input, { target: { value: "/btw Check these counts", selectionStart: 23 } });
    fireEvent.keyDown(input, { key: "Enter" });
    await waitFor(() => expect(sideSend).toHaveBeenCalledWith({ question_markdown: "Check these counts", references: [reference], attachments: [] }));
    expect(send).not.toHaveBeenCalled();
    expect(input).toHaveValue("");
  });

  it("uses an empty /btw command only to open side chat", () => {
    const side = sideController();
    renderShell({ onSend: vi.fn(), sideChat: side, activeConversationId: "current" });
    const input = screen.getByRole("textbox", { name: /Describe a research goal/ });
    fireEvent.change(input, { target: { value: "/btw", selectionStart: 4 } });
    fireEvent.keyDown(input, { key: "Enter" });
    expect(screen.getByRole("region", { name: "Side chat" })).toBeInTheDocument();
    expect(side.send).not.toHaveBeenCalled();
    expect(input).toHaveValue("/btw");
  });

  it("keeps a known command out of the main model when its native handler is unavailable", () => {
    const send = vi.fn();
    renderShell({ onSend: send });
    const input = screen.getByRole("textbox", { name: /Describe a research goal/ });
    fireEvent.change(input, { target: { value: "/btw" } });
    fireEvent.keyDown(input, { key: "Enter" });
    expect(send).not.toHaveBeenCalled();
    expect(screen.getByRole("alert")).toHaveTextContent("Side chat requires the desktop host");
    expect(input).toHaveValue("/btw");
  });

  it("uses /fork text as the branch request and clears only after acceptance", async () => {
    const branchSend = vi.fn().mockResolvedValue(true);
    const send = vi.fn();
    renderShell({ onSend: send, onOpenBranch: vi.fn(), onBranchSend: branchSend, activeConversationId: "current", messages: [{ id: "u1", role: "user", markdown: "First task" }] });
    const input = screen.getByRole("textbox", { name: /Describe a research goal/ });
    fireEvent.change(input, { target: { value: "/fork Try a stricter cutoff" } });
    fireEvent.keyDown(input, { key: "Enter" });
    await waitFor(() => expect(branchSend).toHaveBeenCalledWith("u1", "Try a stricter cutoff", "chat", [], []));
    await waitFor(() => expect(input).toHaveValue(""));
    expect(send).not.toHaveBeenCalled();
  });

  it("shows /fork usage without creating a branch when no request is supplied", () => {
    const branchSend = vi.fn();
    renderShell({ onOpenBranch: vi.fn(), onBranchSend: branchSend, activeConversationId: "current", messages: [{ id: "u1", role: "user", markdown: "First task" }] });
    const input = screen.getByRole("textbox", { name: /Describe a research goal/ });
    fireEvent.change(input, { target: { value: "/fork" } });
    fireEvent.keyDown(input, { key: "Enter" });
    expect(branchSend).not.toHaveBeenCalled();
    expect(screen.getByRole("alert")).toHaveTextContent("Usage: /fork <request>");
    expect(input).toHaveValue("/fork");
  });

  it("does not downgrade /fork to snapshot-only branching when native sending is absent", () => {
    const send = vi.fn(); const open = vi.fn();
    renderShell({ onSend: send, onOpenBranch: open, activeConversationId: "current", messages: [{ id: "u1", role: "user", markdown: "First task" }] });
    const input = screen.getByRole("textbox", { name: /Describe a research goal/ });
    fireEvent.change(input, { target: { value: "/fork Try another method" } });
    fireEvent.keyDown(input, { key: "Enter" });
    expect(screen.getByRole("alert")).toHaveTextContent("Branch sending requires the desktop host");
    expect(input).toHaveValue("/fork Try another method");
    expect(open).not.toHaveBeenCalled(); expect(send).not.toHaveBeenCalled();
  });

  it("opens only the current conversation's recorded trajectory and closes it with Escape", () => {
    const send = vi.fn();
    const base = { schema_version: 4 as const, run_id: "run-current", project_id: project.id, conversation_id: "current", previous_hash: "", event_hash: "hash", occurred_at: "2026-09-14T00:00:00Z" };
    const events: import("../../types").AgentRunEventV4[] = [
      { ...base, sequence: 1, event: { kind: "run_created", mode: "execute" } },
      { ...base, sequence: 2, event: { kind: "model_text", text: "Recorded trajectory" } },
      { ...base, run_id: "run-other", conversation_id: "other", sequence: 1, event: { kind: "model_text", text: "Wrong scope" } },
    ];
    renderShell({ onSend: send, activeConversationId: "current", agentRunEventsV4: events });
    const input = screen.getByRole("textbox", { name: /Describe a research goal/ });
    fireEvent.change(input, { target: { value: "/trajectory" } });
    fireEvent.keyDown(input, { key: "Enter" });
    const dialog = screen.getByRole("dialog", { name: "Run trajectory" });
    expect(dialog).toHaveTextContent("Recorded trajectory");
    expect(dialog).not.toHaveTextContent("Wrong scope");
    expect(within(dialog).queryByRole("button", { name: /Answer/ })).not.toBeInTheDocument();
    fireEvent.keyDown(window, { key: "Escape" });
    expect(screen.queryByRole("dialog", { name: "Run trajectory" })).not.toBeInTheDocument();
    expect(send).not.toHaveBeenCalled();
  });

  it("executes a typed slash command from Send and preserves surrounding draft on selection", async () => {
    const onSend = vi.fn();
    renderShell({ onSend, referenceCatalog: [] });
    const input = screen.getByRole("textbox", { name: /Describe a research goal/ });
    fireEvent.change(input, { target: { value: "/plan", selectionStart: 5 } });
    fireEvent.click(screen.getByRole("button", { name: /^Send$/ }));
    expect(screen.getByRole("button", { name: "Plan" })).toBeInTheDocument();
    expect(input).toHaveValue("");
    fireEvent.change(input, { target: { value: "Keep notes /save-as", selectionStart: 19 } });
    fireEvent.click(await screen.findByRole("option", { name: /\/save-as-skill/ }));
    expect((input as HTMLTextAreaElement).value).toContain("Keep notes ");
    expect((input as HTMLTextAreaElement).value).toContain("SKILL.md");
    expect(onSend).not.toHaveBeenCalled();
  });

  it("sends stable reference IDs, preserving chips on rejection and clearing on acceptance", async () => {
    const onSend = vi.fn().mockResolvedValueOnce(false).mockResolvedValue(true);
    const reference = { kind: "artifact" as const, project_id: project.id, id: "artifact-1" };
    renderShell({ onSend, referenceCatalog: [{ reference, label: "QC report", description: "Verified QC" }] });
    const input = screen.getByRole("textbox", { name: /Describe a research goal/ });
    fireEvent.change(input, { target: { value: "Summarize @QC", selectionStart: 13 } });
    fireEvent.click(await screen.findByRole("option", { name: /QC report/ }));
    expect(input).toHaveValue("Summarize ");
    fireEvent.click(screen.getByRole("button", { name: /^Send$/ }));
    await waitFor(() => expect(onSend).toHaveBeenCalledWith("Summarize", "chat", [reference]));
    await screen.findByRole("alert");
    expect(screen.getByRole("button", { name: /Remove.*QC report/ })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: /^Send$/ }));
    await waitFor(() => expect(input).toHaveValue(""));
    expect(screen.queryByRole("button", { name: /Remove.*QC report/ })).not.toBeInTheDocument();
  });

  it("uses Enter to select a reference without sending and Escape closes immediately", async () => {
    const onSend = vi.fn();
    renderShell({ onSend, referenceCatalog: [{ reference: { kind: "skill", id: "qc-skill" }, label: "Quality check", description: "QC" }] });
    const input = screen.getByRole("textbox", { name: /Describe a research goal/ });
    fireEvent.change(input, { target: { value: "Use /Quality", selectionStart: 12 } });
    await screen.findByRole("option", { name: /Quality check/ });
    fireEvent.keyDown(input, { key: "Enter" });
    expect(onSend).not.toHaveBeenCalled();
    expect(input).toHaveValue("Use ");
    fireEvent.change(input, { target: { value: "Use /", selectionStart: 5 } });
    expect(screen.getByRole("listbox")).toBeInTheDocument();
    fireEvent.keyDown(window, { key: "Escape" });
    expect(screen.queryByRole("listbox")).not.toBeInTheDocument();
    expect(input).toHaveValue("Use /");
  });

  it("closes only the reference picker when a sidebar is already open", () => {
    renderShell({ referenceCatalog: [] });
    fireEvent.click(screen.getByRole("button", { name: "Expand sidebar" }));
    const input = screen.getByRole("textbox", { name: /Describe a research goal/ });
    fireEvent.change(input, { target: { value: "@", selectionStart: 1 } });
    fireEvent.keyDown(window, { key: "Escape" });
    expect(screen.queryByRole("listbox")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Collapse sidebar" })).toBeInTheDocument();
  });

  it("ignores catalog results arriving after switching projects", async () => {
    let resolveOld!: (items: Awaited<ReturnType<typeof referenceApi.composerReferenceCatalog>>) => void;
    const catalog = vi.spyOn(referenceApi, "composerReferenceCatalog")
      .mockImplementationOnce(() => new Promise((resolve) => { resolveOld = resolve; }))
      .mockResolvedValue([{ reference: { kind: "artifact", project_id: "new-project", id: "new" }, label: "New result", description: "" }]);
    const view = renderShell({ onSend: vi.fn() });
    fireEvent.change(screen.getByRole("textbox", { name: /Describe a research goal/ }), { target: { value: "@", selectionStart: 1 } });
    await waitFor(() => expect(catalog).toHaveBeenCalledTimes(1));
    view.rerender(<WorkspaceShell project={{ ...project, id: "new-project" }} locale="en-US" onLocaleChange={vi.fn()} onSend={vi.fn()} />);
    const input = screen.getByRole("textbox", { name: /Describe a research goal/ });
    fireEvent.change(input, { target: { value: "@New", selectionStart: 4 } });
    expect(await screen.findByRole("option", { name: /New result/ })).toBeInTheDocument();
    resolveOld([{ reference: { kind: "artifact", project_id: project.id, id: "old" }, label: "Old result", description: "" }]);
    await waitFor(() => expect(screen.queryByRole("option", { name: /Old result/ })).not.toBeInTheDocument());
    catalog.mockRestore();
  });

  it("prepares a skill request without sending or discarding the draft and restores focus", () => {
    const onSend = vi.fn();
    renderShell({ onSend });
    const input = screen.getByRole("textbox", { name: /Describe a research goal/ });
    fireEvent.change(input, { target: { value: "Keep the QC thresholds" } });
    fireEvent.click(screen.getByRole("button", { name: "Add context or choose mode" }));
    fireEvent.click(screen.getByRole("menuitem", { name: /Save as skill/ }));
    expect((input as HTMLTextAreaElement).value).toContain("Keep the QC thresholds");
    expect((input as HTMLTextAreaElement).value).toContain("SKILL.md");
    expect(input).toHaveFocus();
    expect(screen.queryByRole("menu", { name: "Compose actions" })).not.toBeInTheDocument();
    expect(onSend).not.toHaveBeenCalled();
  });

  it("opens the share preview from the plus menu and returns focus on Escape", () => {
    renderShell({ messages: [{ id: "answer", role: "assistant", markdown: "Verified result" }] });
    fireEvent.click(screen.getByRole("button", { name: "Add context or choose mode" }));
    fireEvent.click(screen.getByRole("menuitem", { name: /Share conversation/ }));
    expect(screen.getByRole("dialog", { name: "Share conversation" })).toBeInTheDocument();
    expect(screen.queryByRole("menu", { name: "Compose actions" })).not.toBeInTheDocument();
    fireEvent.keyDown(window, { key: "Escape" });
    expect(screen.queryByRole("dialog", { name: "Share conversation" })).not.toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: /Describe a research goal/ })).toHaveFocus();
  });

  it("sends with Enter but leaves Shift+Enter and IME confirmation alone", async () => {
    const onSend = vi.fn().mockResolvedValue(true);
    renderShell({ onSend });
    const input = screen.getByRole("textbox", { name: /Describe a research goal/ });
    fireEvent.change(input, { target: { value: "Analyze cells" } });
    fireEvent.keyDown(input, { key: "Enter", shiftKey: true });
    fireEvent.keyDown(input, { key: "Enter", isComposing: true });
    fireEvent.keyDown(input, { key: "Enter", keyCode: 229 });
    expect(onSend).not.toHaveBeenCalled();
    fireEvent.keyDown(input, { key: "Enter" });
    await waitFor(() => expect(onSend).toHaveBeenCalledWith("Analyze cells", "chat"));
  });

  it("preserves the draft when dispatch throws and allows a retry", async () => {
    const onSend = vi.fn().mockRejectedValueOnce(new Error("internal detail")).mockResolvedValue(true);
    renderShell({ onSend });
    const input = screen.getByRole("textbox", { name: /Describe a research goal/ });
    fireEvent.change(input, { target: { value: "Keep this request" } });
    fireEvent.click(screen.getByRole("button", { name: /^Send$/ }));
    await waitFor(() => expect(screen.getByRole("alert")).toHaveTextContent("could not be sent"));
    expect(input).toHaveValue("Keep this request");
    expect(screen.queryByText("internal detail")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: /^Send$/ }));
    await waitFor(() => expect(input).toHaveValue(""));
    expect(onSend).toHaveBeenCalledTimes(2);
  });

  it("keeps the draft while opening a real compose action during dispatch", async () => {
    let accept!: (value: boolean) => void;
    renderShell({ messages: [{ id: "answer", role: "assistant", markdown: "Saved result" }], onSend: () => new Promise<boolean>((resolve) => { accept = resolve; }) });
    const input = screen.getByRole("textbox", { name: /Describe a research goal/ });
    fireEvent.change(input, { target: { value: "Initial request" } });
    fireEvent.click(screen.getByRole("button", { name: /^Send$/ }));
    fireEvent.click(screen.getByRole("button", { name: "Add context or choose mode" }));
    fireEvent.click(screen.getByRole("menuitem", { name: /Share conversation/ }));
    expect(screen.getByRole("dialog", { name: "Share conversation" })).toBeInTheDocument();
    const draft = (input as HTMLTextAreaElement).value;
    expect(draft).toBe("Initial request");
    accept(true);
    await waitFor(() => expect(input).toBeEnabled());
    expect(input).toHaveValue("");
  });

  it("does not dispatch from Enter when the compute backend is unavailable", () => {
    const onSend = vi.fn();
    renderShell({ onSend, computeBackends: [] });
    const input = screen.getByRole("textbox", { name: /Describe a research goal/ });
    fireEvent.change(input, { target: { value: "Run" } });
    fireEvent.keyDown(input, { key: "Enter" });
    expect(onSend).not.toHaveBeenCalled();
    expect(input).toHaveValue("Run");
  });

  it("grows for longer drafts while keeping a manually resized input until the draft is cleared", () => {
    renderShell();
    const input = screen.getByRole("textbox", { name: /Describe a research goal/ }) as HTMLTextAreaElement;
    Object.defineProperty(input, "scrollHeight", { configurable: true, value: 180 });
    fireEvent.change(input, { target: { value: "Long draft" } });
    expect(input.style.height).toBe("180px");
    const rect = vi.spyOn(input, "getBoundingClientRect");
    rect.mockReturnValue({ height: 180 } as DOMRect);
    fireEvent.pointerDown(input);
    input.style.height = "260px";
    rect.mockReturnValue({ height: 260 } as DOMRect);
    fireEvent.pointerUp(input);
    fireEvent.change(input, { target: { value: "Longer draft" } });
    expect(input.style.height).toBe("260px");
    Object.defineProperty(input, "scrollHeight", { configurable: true, value: 76 });
    fireEvent.change(input, { target: { value: "" } });
    expect(input.style.height).toBe("76px");
  });

  it("persists modifier-to-send preference and requires Ctrl or Cmd while preserving Shift+Enter", async () => {
    const onSend = vi.fn().mockResolvedValue(true);
    const view = renderShell({ onSend });
    fireEvent.click(screen.getByRole("button", { name: "Agent permissions" }));
    fireEvent.click(screen.getByRole("menuitemcheckbox", { name: "Send with Ctrl/Cmd+Enter" }));
    view.unmount();
    const next = renderShell({ onSend });
    const input = screen.getByRole("textbox", { name: /Describe a research goal/ });
    fireEvent.change(input, { target: { value: "Modifier send" } });
    fireEvent.keyDown(input, { key: "Enter" });
    fireEvent.keyDown(input, { key: "Enter", ctrlKey: true, shiftKey: true });
    expect(onSend).not.toHaveBeenCalled();
    fireEvent.keyDown(input, { key: "Enter", ctrlKey: true });
    await waitFor(() => expect(onSend).toHaveBeenCalledWith("Modifier send", "chat"));
    next.unmount();
    localStorage.removeItem("omicsops.composer.modifierSend");
  });

  it("closes the plus menu immediately on Escape", () => {
    renderShell();

    const plus = screen.getByRole("button", { name: "Add context or choose mode" });
    fireEvent.click(plus);
    expect(screen.getByRole("menu", { name: "Compose actions" })).toBeInTheDocument();

    fireEvent.keyDown(window, { key: "Escape" });

    expect(screen.queryByRole("menu", { name: "Compose actions" })).not.toBeInTheDocument();
    expect(plus).toHaveAttribute("aria-expanded", "false");
  });

  it("closes nested compute before its orbit permissions parent", () => {
    renderShell();

    fireEvent.click(screen.getByRole("button", { name: "Agent permissions" }));
    const permissions = screen.getByRole("menu", { name: "Agent permission options" });
    fireEvent.click(screen.getByRole("menuitem", { name: /^Compute/ }));
    expect(screen.getByRole("region", { name: "V4 compute backend" })).toBeInTheDocument();

    fireEvent.keyDown(window, { key: "Escape" });
    expect(screen.queryByRole("region", { name: "V4 compute backend" })).not.toBeInTheDocument();
    expect(permissions).toBeInTheDocument();

    fireEvent.keyDown(window, { key: "Escape" });
    expect(screen.queryByRole("menu", { name: "Agent permission options" })).not.toBeInTheDocument();
  });

  it("reports truthful Python and R availability and closes runtime dialogs on Escape", () => {
    renderShell();

    const python = screen.getByRole("button", { name: "Python environment" });
    const r = screen.getByRole("button", { name: "R environment" });
    expect(python).toHaveTextContent("AVAILABLE");
    expect(r).toHaveTextContent("UNAVAILABLE");

    fireEvent.click(python);
    expect(screen.getByRole("dialog", { name: "Runtimes" })).toHaveTextContent("Available");
    fireEvent.keyDown(window, { key: "Escape" });
    expect(screen.queryByRole("dialog", { name: "Runtimes" })).not.toBeInTheDocument();

    fireEvent.click(r);
    expect(screen.getByRole("dialog", { name: "Runtimes" })).toHaveTextContent("Unavailable");
    fireEvent.keyDown(window, { key: "Escape" });
    expect(screen.queryByRole("dialog", { name: "Runtimes" })).not.toBeInTheDocument();
  });

  it("appends the prepare-environment request to the draft without sending it", () => {
    const onSend = vi.fn();
    renderShell({ onSend });

    fireEvent.click(screen.getByRole("button", { name: "Python environment" }));
    fireEvent.click(screen.getByRole("button", { name: "Ask Agent to prepare environment" }));

    expect(onSend).not.toHaveBeenCalled();
    expect((screen.getByRole("textbox", { name: /Describe a research goal/ }) as HTMLTextAreaElement).value).toContain("Check the Python interpreter");
    expect(screen.queryByRole("dialog", { name: "Runtimes" })).not.toBeInTheDocument();
  });

  it("routes model selection to onModelChange and closes the model menu", () => {
    const onModelChange = vi.fn();
    renderShell({
      modelLabel: "Current model",
      modelOptions: [
        { id: "model-a", label: "Model A" },
        { id: "model-b", label: "Model B" },
      ],
      modelId: "model-a",
      onModelChange,
    });

    const modelButton = screen.getByRole("button", { name: "Choose model" });
    fireEvent.click(modelButton);
    fireEvent.click(screen.getByRole("menuitemradio", { name: "Model B" }));
    expect(onModelChange).toHaveBeenCalledWith("model-b");
    expect(screen.queryByRole("menu")).not.toBeInTheDocument();

    fireEvent.click(modelButton);
    expect(screen.getByRole("menu")).toBeInTheDocument();
    fireEvent.keyDown(window, { key: "Escape" });
    expect(screen.queryByRole("menu")).not.toBeInTheDocument();
  });

  it("routes plan mode through the durable callback and disables mode changes while locked", () => {
    const onAgentModeChange = vi.fn();
    const view = renderShell({ agentMode: "agent", onAgentModeChange });

    fireEvent.click(screen.getByRole("button", { name: "Agent permissions" }));
    const orbitPlan = screen.getByRole("menuitemcheckbox", { name: "Plan first" });
    expect(orbitPlan).toBeEnabled();
    fireEvent.click(orbitPlan);
    expect(onAgentModeChange).toHaveBeenCalledWith("plan");

    fireEvent.click(screen.getByRole("button", { name: "Send options" }));
    const sendPlan = screen.getByRole("menuitemradio", { name: "Plan first" });
    expect(sendPlan).toBeEnabled();
    fireEvent.click(sendPlan);
    expect(onAgentModeChange).toHaveBeenLastCalledWith("plan");
    expect(screen.queryByRole("menu")).not.toBeInTheDocument();

    view.rerender(
      <WorkspaceShell
        project={project}
        locale="en-US"
        onLocaleChange={vi.fn()}
        computeBackends={[localBackend]}
        computeBackendId="local"
        agentMode="agent"
        conversationLocked
        onAgentModeChange={onAgentModeChange}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: "Agent permissions" }));
    expect(screen.getByRole("menuitemcheckbox", { name: "Plan first" })).toBeDisabled();
    fireEvent.click(screen.getByRole("button", { name: "Send options" }));
    expect(screen.getByRole("menuitemradio", { name: "Plan first" })).toBeDisabled();
  });

  it("attaches a persisted workflow by ID without sending the slash literal", async () => {
    const onSend = vi.fn().mockResolvedValue(true);
    const reference = { kind: "workflow" as const, project_id: project.id, id: "workflow-review" };
    renderShell({ onSend, referenceCatalog: [{ reference, label: "Evidence review", description: "Collect and verify" }] });
    const input = screen.getByRole("textbox", { name: /Describe a research goal/ });
    fireEvent.change(input, { target: { value: "Review /Evidence", selectionStart: 16 } });
    fireEvent.click(await screen.findByRole("option", { name: /Evidence review/ }));
    expect(input).toHaveValue("Review ");
    fireEvent.click(screen.getByRole("button", { name: "Send" }));
    await waitFor(() => expect(onSend).toHaveBeenCalledWith("Review", "chat", [reference]));
  });

  it("refreshes an open composer workflow picker after Settings saves the catalog", async () => {
    const oldWorkflow = { reference: { kind: "workflow" as const, project_id: project.id, id: "workflow-old" }, label: "Old recipe", description: "Before save" };
    const savedWorkflow = { reference: { kind: "workflow" as const, project_id: project.id, id: "workflow-new" }, label: "Saved recipe", description: "From Settings" };
    const catalog = vi.spyOn(referenceApi, "composerReferenceCatalog")
      .mockResolvedValueOnce([oldWorkflow])
      .mockResolvedValueOnce([savedWorkflow]);
    const view = renderShell({ workflowCatalogVersion: 0 });
    const input = screen.getByRole("textbox", { name: /Describe a research goal/ });
    fireEvent.change(input, { target: { value: "/", selectionStart: 1 } });
    expect(await screen.findByRole("option", { name: /Old recipe/ })).toBeInTheDocument();

    view.rerender(<WorkspaceShell project={project} locale="en-US" onLocaleChange={vi.fn()} computeBackends={[localBackend]} computeBackendId="local" workflowCatalogVersion={1} />);

    expect(await screen.findByRole("option", { name: /Saved recipe/ })).toBeInTheDocument();
    expect(screen.queryByRole("option", { name: /Old recipe/ })).not.toBeInTheDocument();
    expect(catalog).toHaveBeenCalledTimes(2);
  });

  it("opens workflow management from the menu and preserves the draft on close", async () => {
    renderShell();
    const input = screen.getByRole("textbox", { name: /Describe a research goal/ });
    fireEvent.change(input, { target: { value: "Existing goal" } });
    fireEvent.click(screen.getByRole("button", { name: "Add context or choose mode" }));
    fireEvent.click(screen.getByRole("menuitem", { name: /Manage workflows/ }));
    await screen.findByRole("dialog");
    fireEvent.keyDown(window, { key: "Escape" });
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(input).toHaveValue("Existing goal");
  });

  it("opens Skills settings from the compose menu", () => {
    const onOpenSettings = vi.fn();
    renderShell({ onOpenSettings });

    const plus = screen.getByRole("button", { name: "Add context or choose mode" });
    fireEvent.click(plus);
    fireEvent.click(screen.getByRole("menuitem", { name: /Manage skills/ }));

    expect(onOpenSettings).toHaveBeenCalledWith("skills");
    expect(plus).toHaveAttribute("aria-expanded", "false");
  });
});
