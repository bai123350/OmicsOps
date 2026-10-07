import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { ComputeBackendAvailabilityV4, RuntimeBoundaryViewV4 } from "../../types";
import { WorkspaceShell } from "./WorkspaceShell";
import { ComposerDraftStore } from "./composerDrafts";
import * as referenceApi from "../../composer-reference-api";
import * as api from "../../tauri-api";
import * as attachmentApi from "../../composer-attachment-api";
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
const runtimeSelection = { schema_version: 4 as const, backend_id: "local", backend_kind: "local" as const, autonomy_mode: "supervised" as const, approval_policy: "risk_based" as const, environment: "system", network_policy: "host_inherited" as const, container_image: null };
const runtimeBoundary: RuntimeBoundaryViewV4 = { project_id: project.id, source: { kind: "draft_selection" }, compute_selection: runtimeSelection, execution_location: "local_host", isolation: "process", limits: ["same_user_permissions", "project_cwd_not_access_control"], interactive_lifecycle: "run_scoped_no_restart_reconnect", detached_job_lifecycle: "unsupported", verification: "not_checked_by_this_view" };

function renderShell(overrides: Partial<React.ComponentProps<typeof WorkspaceShell>> = {}) {
  return render(
    <WorkspaceShell
      project={project}
      locale="en-US"
      computeBackends={[localBackend]}
      computeBackendId="local"
      runtimeDraftSelection={runtimeSelection}
      {...overrides}
    />,
  );
}

function sideController(send = vi.fn().mockResolvedValue(true)): SideChatController {
  return { records: [], loading: false, ready: true, busy: false, pending: false, error: false, modelId: "side-model", setModelId: vi.fn(), draft: "", setDraft: vi.fn(), originalQuestion: undefined, send, retry: vi.fn().mockResolvedValue(true), refresh: vi.fn(), hasMore: false, loadingOlder: false, loadOlder: vi.fn() };
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}

type ReceiptRoute = "send" | "queue" | "replacement" | "side" | "branch" | "fork";
function receiptFixture(route: ReceiptRoute, submit: ReturnType<typeof vi.fn>, retry = vi.fn().mockResolvedValue(true)) {
  const props: Partial<React.ComponentProps<typeof WorkspaceShell>> = {
    activeConversationId: "a", messages: [{ id: "u1", role: "user", markdown: "First task" }],
  };
  if (route === "send") props.onSend = submit;
  if (route === "queue") props.onQueue = submit;
  if (route === "replacement") Object.assign(props, {
    onQueue: vi.fn(), activeRunId: "run", agentBusy: true,
    agentRunEventsV4: [{ schema_version: 4, run_id: "run", project_id: project.id, conversation_id: "a", sequence: 1, previous_hash: "", event_hash: "head", occurred_at: "", event: { kind: "run_created", mode: "execute" } }],
    replacement: { busy: false, pending: false, error: false, send: submit, retry },
  });
  if (route === "side") props.sideChat = { ...sideController(submit), retry };
  if (route === "branch" || route === "fork") Object.assign(props, { onOpenBranch: vi.fn(), onBranchSend: submit, onRetryBranchSend: retry });
  return props;
}

async function launchReceiptRoute(route: ReceiptRoute) {
  if (route === "send" || route === "queue" || route === "fork") {
    const button = screen.getByRole("button", { name: "Send" });
    await waitFor(() => expect(button).toBeEnabled());
    fireEvent.click(button);
  } else {
    fireEvent.click(screen.getByRole("button", { name: "Send options" }));
    const name = route === "replacement" ? /Interrupt and replace/ : route === "side" ? /Side chat/ : /Branch conversation/;
    const item = screen.getByRole("menuitem", { name });
    await waitFor(() => expect(item).toBeEnabled());
    fireEvent.click(item);
  }
}

describe("composer receipt routing", () => {
  const routes: ReceiptRoute[] = ["send", "queue", "replacement", "side", "branch", "fork"];
  for (const route of routes) {
    it.each(["offscreen", "unmounted", "same text", "return and edit", "unchanged", "rejected", "thrown"] as const)(`${route} protects the submitted revision: %s`, async (scenario) => {
      const pending = deferred<boolean>();
      const submit = vi.fn().mockReturnValue(pending.promise);
      const store = new ComposerDraftStore();
      const a = { projectId: project.id, conversationId: "a" };
      const b = { ...a, conversationId: "b" };
      const text = route === "fork" ? "/fork A notes" : "A notes";
      store.update(a, text);
      const props = { ...receiptFixture(route, submit), composerDraftStore: store };
      const view = renderShell(props);
      let input = screen.getByRole("textbox", { name: /Describe a research goal/ });
      await launchReceiptRoute(route);
      expect(submit).toHaveBeenCalledOnce();
      expect(store.read(a).text).toBe(text); // Pending is never acceptance.
      if (scenario === "unmounted") {
        view.unmount();
        renderShell({ ...props, activeConversationId: "b" });
        input = screen.getByRole("textbox", { name: /Describe a research goal/ });
        fireEvent.change(input, { target: { value: "B notes" } });
      } else if (scenario === "offscreen") {
        view.rerender(<WorkspaceShell project={project} locale="en-US" computeBackends={[localBackend]} computeBackendId="local" runtimeDraftSelection={runtimeSelection} {...props} activeConversationId="b" />);
        fireEvent.change(input, { target: { value: "B notes" } });
      } else if (scenario === "return and edit") {
        view.rerender(<WorkspaceShell project={project} locale="en-US" {...props} activeConversationId="b" />);
        view.rerender(<WorkspaceShell project={project} locale="en-US" {...props} activeConversationId="a" />);
        fireEvent.change(input, { target: { value: "New edit" } });
        fireEvent.change(input, { target: { value: text } });
      } else if (scenario === "same text") {
        fireEvent.change(input, { target: { value: "New edit" } });
        fireEvent.change(input, { target: { value: text } });
      }
      await act(async () => { if (scenario === "thrown") pending.reject(new Error("unknown receipt")); else pending.resolve(scenario !== "rejected"); });
      const offscreen = scenario === "offscreen" || scenario === "unmounted";
      expect(store.read(a).text).toBe(offscreen || scenario === "unchanged" ? "" : text);
      expect(input).toHaveValue(offscreen ? "B notes" : scenario === "unchanged" ? "" : text);
      if (offscreen) expect(store.read(b).text).toBe("B notes");
    });
  }

  for (const route of ["replacement", "side", "branch"] as const) {
    it.each(["unchanged", "same text", "offscreen", "rejected", "thrown"] as const)(`${route} retry retains the original receipt: %s`, async (scenario) => {
      const pending = deferred<boolean>();
      const retry = vi.fn().mockReturnValue(pending.promise);
      const store = new ComposerDraftStore();
      const a = { projectId: project.id, conversationId: "a" };
      store.update(a, "A notes");
      let props = { ...receiptFixture(route, vi.fn().mockResolvedValue(false), retry), composerDraftStore: store };
      const view = renderShell(props);
      const input = screen.getByRole("textbox", { name: /Describe a research goal/ });
      await launchReceiptRoute(route);
      await act(async () => {});
      if (route === "replacement") props = { ...props, replacement: { ...props.replacement!, pending: true, error: true, originalMarkdown: "A notes" } };
      if (route === "side") props = { ...props, sideChat: { ...props.sideChat!, pending: true, error: true, originalQuestion: "A notes" } };
      if (route === "branch") props = { ...props, branchSendPending: true, branchSendError: true, branchSendOriginalMarkdown: "A notes" };
      const rerender = (activeConversationId = "a") => view.rerender(<WorkspaceShell project={project} locale="en-US" computeBackends={[localBackend]} computeBackendId="local" runtimeDraftSelection={runtimeSelection} {...props} activeConversationId={activeConversationId} />);
      rerender();
      if (scenario === "same text") {
        fireEvent.change(input, { target: { value: "New edit" } });
        fireEvent.change(input, { target: { value: "A notes" } });
      }
      const retryName = route === "replacement" ? "Reconcile original replacement" : route === "side" ? "Reconcile original question" : "Retry original branch send";
      fireEvent.click(screen.getByRole("button", { name: retryName }));
      expect(retry).toHaveBeenCalledOnce();
      if (scenario === "offscreen") { rerender("b"); fireEvent.change(input, { target: { value: "B notes" } }); }
      expect(store.read(a).text).toBe("A notes");
      await act(async () => { if (scenario === "thrown") pending.reject(new Error("unknown retry")); else pending.resolve(scenario !== "rejected"); });
      expect(store.read(a).text).toBe(scenario === "unchanged" || scenario === "offscreen" ? "" : "A notes");
      expect(input).toHaveValue(scenario === "offscreen" ? "B notes" : scenario === "unchanged" ? "" : "A notes");
    });
  }

  for (const route of ["replacement", "side", "branch"] as const) {
    it(`${route} late acceptance leaves a newer attempt's retry receipt intact`, async () => {
      const first = deferred<boolean>();
      const second = deferred<boolean>();
      const submit = vi.fn().mockReturnValueOnce(first.promise).mockReturnValueOnce(second.promise);
      const retry = vi.fn().mockResolvedValue(true);
      const store = new ComposerDraftStore();
      const scope = { projectId: project.id, conversationId: "a" };
      store.update(scope, "First notes");
      let props = { ...receiptFixture(route, submit, retry), composerDraftStore: store };
      const view = renderShell(props);
      const input = screen.getByRole("textbox", { name: /Describe a research goal/ });
      await launchReceiptRoute(route);
      fireEvent.change(input, { target: { value: "Second notes" } });
      await launchReceiptRoute(route);
      await act(async () => { first.resolve(true); second.resolve(false); });
      expect(input).toHaveValue("Second notes");
      if (route === "replacement") props = { ...props, replacement: { ...props.replacement!, pending: true, error: true, originalMarkdown: "Second notes" } };
      if (route === "side") props = { ...props, sideChat: { ...props.sideChat!, pending: true, error: true, originalQuestion: "Second notes" } };
      if (route === "branch") props = { ...props, branchSendPending: true, branchSendOriginalMarkdown: "Second notes" };
      view.rerender(<WorkspaceShell project={project} locale="en-US" computeBackends={[localBackend]} computeBackendId="local" runtimeDraftSelection={runtimeSelection} {...props} />);
      fireEvent.click(screen.getByRole("button", { name: route === "replacement" ? "Reconcile original replacement" : route === "side" ? "Reconcile original question" : "Retry original branch send" }));
      await waitFor(() => expect(input).toHaveValue(""));
      expect(retry).toHaveBeenCalledOnce();
      expect(store.read(scope).text).toBe("");
    });
  }

  for (const route of routes) {
    it.each(["accepted", "rejected", "offscreen"] as const)(`${route} respects material receipt and scope guards: %s`, async (scenario) => {
      const pending = deferred<boolean>();
      const submit = vi.fn().mockReturnValue(pending.promise);
      const item = { reference: { kind: "artifact" as const, project_id: project.id, id: "qc" }, label: "QC", description: "" };
      const props = receiptFixture(route, submit);
      const view = renderShell({ ...props, searchRequest: { key: "a-ref", kind: "attach", projectId: project.id, conversationId: "a", item } });
      const input = screen.getByRole("textbox", { name: /Describe a research goal/ });
      await waitFor(() => expect(input).toBeEnabled());
      const receipt = { id: "a-image", project_id: project.id, conversation_id: "a", name: "a.png", relative_path: ".omicsops/attachments/a-image/a.png", size_bytes: 5, sha256: "hash", media_type: "image/png" };
      const stage = vi.spyOn(attachmentApi, "stageComposerAttachment").mockResolvedValueOnce(receipt);
      const pasteImage = (name: string) => fireEvent.paste(input, { clipboardData: { files: [new File(["bytes"], name, { type: "image/png" })] } });
      pasteImage("a.png");
      await screen.findByRole("button", { name: "Remove a.png" });
      fireEvent.change(input, { target: { value: route === "fork" ? "/fork A notes" : "A notes" } });
      await launchReceiptRoute(route);
      if (scenario === "offscreen") {
        stage.mockResolvedValueOnce({ ...receipt, id: "b-image", conversation_id: "b", name: "b.png", relative_path: ".omicsops/attachments/b-image/b.png" });
        view.rerender(<WorkspaceShell project={project} locale="en-US" computeBackends={[localBackend]} computeBackendId="local" runtimeDraftSelection={runtimeSelection} {...props} activeConversationId="b" />);
        view.rerender(<WorkspaceShell project={project} locale="en-US" computeBackends={[localBackend]} computeBackendId="local" runtimeDraftSelection={runtimeSelection} {...props} activeConversationId="b" searchRequest={{ key: "b-ref", kind: "attach", projectId: project.id, conversationId: "b", item }} />);
        expect(screen.getByRole("button", { name: "Remove reference: QC" })).toBeInTheDocument();
        await waitFor(() => expect(input).toBeEnabled());
        pasteImage("b.png");
        await screen.findByRole("button", { name: "Remove b.png" });
      }
      await act(async () => pending.resolve(scenario !== "rejected"));
      expect(screen.queryByRole("button", { name: "Remove reference: QC" }) !== null).toBe(scenario !== "accepted");
      expect(screen.queryByRole("button", { name: scenario === "offscreen" ? "Remove b.png" : "Remove a.png" }) !== null).toBe(scenario !== "accepted");
    });
  }

  it.each(["Ask", "Ask again"] as const)("keeps main composer text when reconciling a later panel-only side question from %s", async (action) => {
    const send = vi.fn().mockResolvedValue(false);
    const retry = vi.fn().mockResolvedValue(true);
    const store = new ComposerDraftStore();
    const scope = { projectId: project.id, conversationId: "a" };
    store.update(scope, "Composer notes");
    let props = { ...receiptFixture("side", send, retry), composerDraftStore: store };
    const view = renderShell(props);
    await launchReceiptRoute("side");
    await act(async () => {});
    props = { ...props, sideChat: { ...props.sideChat!, draft: "Panel question", records: action === "Ask" ? [] : [{ id: "failed", request_id: "failed", project_id: project.id, conversation_id: "a", model_profile_id: "side-model", model_label: "Side model", source_snapshot_sha256: "snapshot", source_watermark: { message_count: 0, event_count: 0, event_heads: [] }, question_markdown: "Panel question", references: [], attachments: [], sources: [], status: "failed", answer_markdown: null, cited_source_ids: [], created_at: "", updated_at: "" }] } };
    view.rerender(<WorkspaceShell project={project} locale="en-US" {...props} />);
    fireEvent.click(screen.getByRole("button", { name: action }));
    await waitFor(() => expect(send).toHaveBeenCalledTimes(2));
    await act(async () => {});
    props = { ...props, sideChat: { ...props.sideChat!, pending: true, originalQuestion: "Panel question" } };
    view.rerender(<WorkspaceShell project={project} locale="en-US" {...props} />);
    fireEvent.click(screen.getByRole("button", { name: "Reconcile original question" }));
    await waitFor(() => expect(retry).toHaveBeenCalledOnce());
    await act(async () => {});
    expect(screen.getByRole("textbox", { name: /Describe a research goal/ })).toHaveValue("Composer notes");
    expect(store.read(scope).text).toBe("Composer notes");
  });

  it.each(["ready", "model", "busy", "pending"] as const)("keeps composer retry ownership if panel send is blocked by %s", async (gate) => {
    const send = vi.fn().mockResolvedValue(false);
    const retry = vi.fn().mockResolvedValue(true);
    const store = new ComposerDraftStore();
    store.update({ projectId: project.id, conversationId: "a" }, "Composer notes");
    let props = { ...receiptFixture("side", send, retry), composerDraftStore: store };
    const view = renderShell(props);
    await launchReceiptRoute("side");
    await act(async () => {});
    props = { ...props, sideChat: { ...props.sideChat!, draft: "Panel question" } };
    view.rerender(<WorkspaceShell project={project} locale="en-US" {...props} />);
    // Simulate an admission change before the rendered panel button catches up.
    if (gate === "ready") props.sideChat!.ready = false;
    if (gate === "model") props.sideChat!.modelId = undefined;
    if (gate === "busy") props.sideChat!.busy = true;
    if (gate === "pending") props.sideChat!.pending = true;
    fireEvent.click(screen.getByRole("button", { name: "Ask" }));
    await act(async () => {});
    expect(send).toHaveBeenCalledOnce();
    props = { ...props, sideChat: { ...props.sideChat!, ready: true, modelId: "side-model", busy: false, pending: true, originalQuestion: "Composer notes" } };
    view.rerender(<WorkspaceShell project={project} locale="en-US" {...props} />);
    fireEvent.click(screen.getByRole("button", { name: "Reconcile original question" }));
    await waitFor(() => expect(screen.getByRole("textbox", { name: /Describe a research goal/ })).toHaveValue(""));
    expect(retry).toHaveBeenCalledOnce();
  });
});

describe("WorkspaceShell composer integration", () => {
  it("restores a project draft after the workspace unmounts", () => {
    const store = new ComposerDraftStore();
    store.update({ projectId: project.id, conversationId: "a" }, "A notes");
    const view = renderShell({ activeConversationId: "a", composerDraftStore: store });
    view.unmount();
    renderShell({ activeConversationId: "a", composerDraftStore: store });
    expect(screen.getByRole("textbox", { name: /Describe a research goal/ })).toHaveValue("A notes");
  });

  it("restores only text after a conversation round trip", () => {
    const item = { reference: { kind: "artifact" as const, project_id: project.id, id: "qc" }, label: "QC", description: "" };
    const view = renderShell({ activeConversationId: "a", searchRequest: { key: "draft-ref", kind: "attach", projectId: project.id, conversationId: "a", item } });
    const input = screen.getByRole("textbox", { name: /Describe a research goal/ });
    fireEvent.change(input, { target: { value: "A notes" } });
    expect(screen.getByRole("button", { name: "Remove reference: QC" })).toBeInTheDocument();
    view.rerender(<WorkspaceShell project={project} locale="en-US" activeConversationId="b" />);
    expect(input).toHaveValue("");
    fireEvent.change(input, { target: { value: "B notes" } });
    view.rerender(<WorkspaceShell project={project} locale="en-US" activeConversationId="a" />);
    expect(input).toHaveValue("A notes");
    expect(screen.queryByRole("button", { name: "Remove reference: QC" })).not.toBeInTheDocument();
    view.rerender(<WorkspaceShell project={project} locale="en-US" activeConversationId="b" />);
    expect(input).toHaveValue("B notes");
  });

  it("keeps same-named conversations in different projects separate", () => {
    const view = renderShell({ activeConversationId: "same" });
    const input = screen.getByRole("textbox", { name: /Describe a research goal/ });
    fireEvent.change(input, { target: { value: "First project" } });
    view.rerender(<WorkspaceShell project={{ ...project, id: "other" }} locale="en-US" activeConversationId="same" />);
    expect(input).toHaveValue("");
    fireEvent.change(input, { target: { value: "Other project" } });
    view.rerender(<WorkspaceShell project={project} locale="en-US" activeConversationId="same" />);
    expect(input).toHaveValue("First project");
  });

  it("does not adopt unassigned text during ordinary conversation navigation", () => {
    const view = renderShell();
    const input = screen.getByRole("textbox", { name: /Describe a research goal/ });
    fireEvent.change(input, { target: { value: "Unassigned notes" } });
    view.rerender(<WorkspaceShell project={project} locale="en-US" activeConversationId="saved" />);
    expect(input).toHaveValue("");
    view.rerender(<WorkspaceShell project={project} locale="en-US" />);
    expect(input).toHaveValue("Unassigned notes");
  });

  it("keeps text when the workspace navigation is collapsed", () => {
    renderShell({ activeConversationId: "a" });
    const input = screen.getByRole("textbox", { name: /Describe a research goal/ });
    fireEvent.change(input, { target: { value: "Keep while collapsed" } });
    fireEvent.click(screen.getByRole("button", { name: "Collapse workspace navigation" }));
    expect(input).toHaveValue("Keep while collapsed");
    fireEvent.click(screen.getByRole("button", { name: "Expand workspace navigation" }));
    expect(input).toHaveValue("Keep while collapsed");
  });

  it("attaches a search result to the current draft without switching sessions", () => {
    const onSelectConversation = vi.fn();
    const view = renderShell({ activeConversationId: "current", onSelectConversation });
    const input = screen.getByRole("textbox", { name: /Describe a research goal/ });
    fireEvent.change(input, { target: { value: "Keep this goal" } });
    const item = { reference: { kind: "session" as const, project_id: project.id, id: "saved" }, label: "Saved evidence", description: "" };
    view.rerender(<WorkspaceShell project={project} locale="en-US" activeConversationId="current" onSelectConversation={onSelectConversation} searchRequest={{ key: "attach-1", kind: "attach", projectId: project.id, conversationId: "current", item }} />);
    expect(screen.getByRole("button", { name: /Remove reference: Saved evidence/ })).toBeInTheDocument();
    expect(input).toHaveValue("Keep this goal");
    expect(onSelectConversation).not.toHaveBeenCalled();
    view.rerender(<WorkspaceShell project={project} locale="en-US" activeConversationId="different" searchRequest={{ key: "attach-1", kind: "attach", projectId: project.id, conversationId: "current", item }} />);
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

  it("opens the run-owned environment above trajectory and Escape closes one layer without actions", () => {
    const query = vi.spyOn(api, "agentV4RuntimeBoundary").mockImplementation(() => new Promise(() => undefined));
    const onCancelRun = vi.fn();
    const onInterruptKernel = vi.fn();
    const base = { schema_version: 4 as const, run_id: "run-recorded", project_id: project.id, conversation_id: "current", previous_hash: "", event_hash: "hash", occurred_at: "2026-09-14T00:00:00Z" };
    renderShell({ activeConversationId: "current", onCancelRun, onInterruptKernel, agentRunEventsV4: [{ ...base, sequence: 1, event: { kind: "run_created", mode: "execute" } }] });
    const input = screen.getByRole("textbox", { name: /Describe a research goal/ });
    fireEvent.change(input, { target: { value: "/trajectory" } });
    fireEvent.keyDown(input, { key: "Enter" });
    const trajectory = screen.getByRole("dialog", { name: "Run trajectory" });
    fireEvent.click(within(trajectory).getByRole("button", { name: "Run environment" }));
    expect(query).toHaveBeenCalledWith({ source: "frozen_run", project_id: project.id, run_id: "run-recorded" });
    expect(screen.getByRole("dialog", { name: "Runtimes" })).toBeInTheDocument();
    fireEvent.keyDown(window, { key: "Escape" });
    expect(screen.queryByRole("dialog", { name: "Runtimes" })).not.toBeInTheDocument();
    expect(trajectory).toBeInTheDocument();
    fireEvent.keyDown(window, { key: "Escape" });
    expect(screen.queryByRole("dialog", { name: "Run trajectory" })).not.toBeInTheDocument();
    expect(onCancelRun).not.toHaveBeenCalled();
    expect(onInterruptKernel).not.toHaveBeenCalled();
  });

  it("keeps a frozen run's boundary when the complete next-run selection changes", async () => {
    const runId = "run-frozen-selection";
    const events: import("../../types").AgentRunEventV4[] = [{
      schema_version: 4, run_id: runId, project_id: project.id, conversation_id: "current",
      sequence: 1, previous_hash: "", event_hash: "hash", occurred_at: "2026-09-14T00:00:00Z",
      event: { kind: "run_created", mode: "execute" },
    }];
    const frozenBoundary: RuntimeBoundaryViewV4 = {
      ...runtimeBoundary,
      source: { kind: "frozen_run", run_id: runId },
      compute_selection: { ...runtimeSelection, backend_id: "ssh:lab", backend_kind: "ssh", environment: "micromamba-lab" },
      execution_location: "ssh_host",
      detached_job_lifecycle: "ssh_linux_only",
    };
    const query = vi.spyOn(api, "agentV4RuntimeBoundary").mockResolvedValue(frozenBoundary);
    const onOpenSettings = vi.fn();
    const onSend = vi.fn();
    const props = { project, locale: "en-US" as const, activeConversationId: "current", agentRunEventsV4: events, computeBackends: [localBackend], computeBackendId: "local", onOpenSettings, onSend };
    const view = render(<WorkspaceShell {...props} runtimeDraftSelection={runtimeSelection} />);
    fireEvent.click(screen.getByRole("button", { name: "Run environment" }));
    const dialog = screen.getByRole("dialog", { name: "Runtimes" });
    await waitFor(() => expect(dialog).toHaveTextContent("SSH host process"));
    expect(query).toHaveBeenCalledTimes(1);
    expect(query).toHaveBeenCalledWith({ source: "frozen_run", project_id: project.id, run_id: runId });

    const nextRunSelection = { ...runtimeSelection, backend_id: "docker", backend_kind: "docker" as const, network_policy: "none" as const, container_image: { reference: "python:3.12", image_id: "sha256:new" } };
    view.rerender(<WorkspaceShell {...props} runtimeDraftSelection={nextRunSelection} computeBackendId="docker" />);
    expect(dialog).toHaveTextContent("SSH host process");
    expect(dialog).toHaveTextContent("micromamba-lab");
    expect(dialog).not.toHaveTextContent("Local compute container");
    expect(query).toHaveBeenCalledTimes(1);
    expect(query).toHaveBeenCalledWith({ source: "frozen_run", project_id: project.id, run_id: runId });
    expect(within(dialog).queryByRole("button", { name: "Ask Agent to prepare environment" })).not.toBeInTheDocument();
    expect(within(dialog).queryByRole("button", { name: "Configure SSH" })).not.toBeInTheDocument();
    expect(onOpenSettings).not.toHaveBeenCalled();
    expect(onSend).not.toHaveBeenCalled();
    expect(screen.getByRole("textbox", { name: /Describe a research goal/ })).toHaveValue("");
  });

  it("does not reopen a previous conversation's runtime dialog when switching back", () => {
    const props = { project, locale: "en-US" as const, computeBackends: [localBackend], computeBackendId: "local", runtimeDraftSelection: runtimeSelection };
    const view = render(<WorkspaceShell {...props} activeConversationId="first" />);
    fireEvent.click(screen.getByRole("button", { name: "Python environment" }));
    expect(screen.getByRole("dialog", { name: "Runtimes" })).toBeInTheDocument();
    view.rerender(<WorkspaceShell {...props} activeConversationId="second" />);
    expect(screen.queryByRole("dialog", { name: "Runtimes" })).not.toBeInTheDocument();
    view.rerender(<WorkspaceShell {...props} activeConversationId="first" />);
    expect(screen.queryByRole("dialog", { name: "Runtimes" })).not.toBeInTheDocument();
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
    view.rerender(<WorkspaceShell project={{ ...project, id: "new-project" }} locale="en-US" onSend={vi.fn()} />);
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

  it("reports truthful Python and R availability and closes runtime dialogs on Escape", async () => {
    vi.spyOn(api, "agentV4RuntimeBoundary").mockResolvedValue(runtimeBoundary);
    renderShell();

    const python = screen.getByRole("button", { name: "Python environment" });
    const r = screen.getByRole("button", { name: "R environment" });
    expect(python).toHaveTextContent("AVAILABLE");
    expect(r).toHaveTextContent("UNAVAILABLE");

    fireEvent.click(python);
    await waitFor(() => expect(screen.getByRole("dialog", { name: "Runtimes" })).toHaveTextContent("Available"));
    fireEvent.keyDown(window, { key: "Escape" });
    expect(screen.queryByRole("dialog", { name: "Runtimes" })).not.toBeInTheDocument();

    fireEvent.click(r);
    await waitFor(() => expect(screen.getByRole("dialog", { name: "Runtimes" })).toHaveTextContent("Unavailable"));
    fireEvent.keyDown(window, { key: "Escape" });
    expect(screen.queryByRole("dialog", { name: "Runtimes" })).not.toBeInTheDocument();
  });

  it("appends the prepare-environment request to the draft without sending it", async () => {
    vi.spyOn(api, "agentV4RuntimeBoundary").mockResolvedValue(runtimeBoundary);
    const onSend = vi.fn();
    renderShell({ onSend });

    fireEvent.click(screen.getByRole("button", { name: "Python environment" }));
    fireEvent.click(await screen.findByRole("button", { name: "Ask Agent to prepare environment" }));

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

    view.rerender(<WorkspaceShell project={project} locale="en-US" computeBackends={[localBackend]} computeBackendId="local" workflowCatalogVersion={1} />);

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
