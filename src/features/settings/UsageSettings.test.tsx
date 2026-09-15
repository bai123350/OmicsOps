import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import type {
  ObservedCounterV4,
  UsageAggregatePage,
  UsageConversationPage,
  UsageTotalsV4,
  WorkspaceProject,
} from "../../types";
import { UsageSettings } from "./UsageSettings";

const api = vi.hoisted(() => ({
  settingsUsagePage: vi.fn(),
  settingsUsageConversations: vi.fn(),
}));
vi.mock("../../usage-settings-api", () => api);

function project(id: string, name: string): WorkspaceProject {
  return {
    id,
    name,
    description: "",
    local_root: `E:/data/${id}`,
    remote_root: null,
    connection_id: null,
    template: "blank",
    status: "ready",
    ollama_only: false,
    created_at: "2026-09-15T00:00:00Z",
    updated_at: "2026-09-15T00:00:00Z",
  };
}

function counter(known: number | null = null, incomplete_attempts = 0): ObservedCounterV4 {
  return { known, incomplete_attempts };
}

function totals(input: number | null = null, overrides: Partial<UsageTotalsV4> = {}): UsageTotalsV4 {
  return {
    input_tokens: counter(input),
    output_tokens: counter(),
    reasoning_tokens: counter(),
    cache_read_input_tokens: counter(),
    cache_creation_input_tokens: counter(),
    reported_total_tokens: counter(),
    observed_attempts: input == null ? 0 : 1,
    final_attempts: input == null ? 0 : 1,
    partial_attempts: 0,
    interrupted_attempts: 0,
    unknown_attempts: 0,
    ...overrides,
  };
}

function aggregate(input: number | null = null, overrides: Partial<UsageAggregatePage> = {}): UsageAggregatePage {
  return {
    totals: totals(input),
    projects: [],
    models: [],
    days: [],
    tools: [],
    next_cursor: null,
    scanned_runs: 1,
    omitted_runs: 0,
    unattributed_events: 0,
    snapshot_at: "2026-09-15T00:00:00Z",
    completeness: "complete",
    ...overrides,
  };
}

function conversations(overrides: Partial<UsageConversationPage> = {}): UsageConversationPage {
  return { items: [], next_cursor: null, snapshot_at: "2026-09-15T00:00:00Z", ...overrides };
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((next, fail) => { resolve = next; reject = fail; });
  return { promise, resolve, reject };
}

beforeEach(() => {
  api.settingsUsagePage.mockReset().mockResolvedValue(aggregate());
  api.settingsUsageConversations.mockReset().mockResolvedValue(conversations());
});

describe("UsageSettings", () => {
  it("keeps not reported, real zero, and observed subtotal visibly distinct", async () => {
    api.settingsUsagePage.mockResolvedValue(aggregate(null, {
      totals: totals(null, {
        output_tokens: counter(0),
        reasoning_tokens: counter(12, 1),
        observed_attempts: 1,
        final_attempts: 1,
      }),
      completeness: "partial",
    }));
    render(<UsageSettings locale="en-US" projects={[]} />);

    const cards = await screen.findByLabelText("Token totals");
    expect(within(cards).getAllByText("Not reported")).toHaveLength(4);
    expect(within(cards).getByText("0")).toBeInTheDocument();
    expect(within(cards).getByText("12 observed subtotal")).toBeInTheDocument();
    expect(screen.getByText(/Observed subtotal: 0 runs omitted/)).toBeInTheDocument();
  });

  it("merges fixed-snapshot pages sequentially and marks in-progress cards as subtotals", async () => {
    const second = deferred<UsageAggregatePage>();
    api.settingsUsagePage
      .mockResolvedValueOnce(aggregate(10, { next_cursor: "page-2", scanned_runs: 1 }))
      .mockReturnValueOnce(second.promise);
    render(<UsageSettings locale="en-US" projects={[]} />);

    expect(await screen.findByText("10 observed subtotal")).toBeInTheDocument();
    expect(screen.getByText(/More fixed-snapshot pages are being merged/)).toBeInTheDocument();
    expect(api.settingsUsagePage).toHaveBeenNthCalledWith(2, expect.any(Object), "page-2");
    second.resolve(aggregate(20, { scanned_runs: 2 }));

    expect(await screen.findByText("30")).toBeInTheDocument();
    expect(screen.getByText(/3 runs scanned/)).toBeInTheDocument();
    expect(screen.queryByText(/More fixed-snapshot pages are being merged/)).not.toBeInTheDocument();
  });

  it("retains completed pages on a transient error and restarts safely after a repeated cursor", async () => {
    api.settingsUsagePage
      .mockResolvedValueOnce(aggregate(10, { next_cursor: "page-2" }))
      .mockResolvedValueOnce(aggregate(20, { next_cursor: "page-2" }))
      .mockResolvedValueOnce(aggregate(5));
    render(<UsageSettings locale="en-US" projects={[]} />);

    expect(await screen.findByRole("alert")).toHaveTextContent("Completed pages are retained");
    expect(screen.getByText("30 observed subtotal")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Retry" }));

    expect(await screen.findByText("5")).toBeInTheDocument();
    expect(api.settingsUsagePage).toHaveBeenNthCalledWith(3, expect.any(Object), null);
  });

  it("catches a changed snapshot outside React state updates and can recalculate", async () => {
    api.settingsUsagePage
      .mockResolvedValueOnce(aggregate(10, { next_cursor: "page-2", snapshot_at: "snapshot-a" }))
      .mockResolvedValueOnce(aggregate(20, { snapshot_at: "snapshot-b" }))
      .mockResolvedValueOnce(aggregate(7, { snapshot_at: "snapshot-c" }));
    render(<UsageSettings locale="en-US" projects={[]} />);

    expect(await screen.findByRole("alert")).toHaveTextContent("Usage history changed");
    expect(screen.getByText("10 observed subtotal")).toBeInTheDocument();
    fireEvent.click(within(screen.getByRole("alert")).getByRole("button", { name: "Recalculate" }));
    expect(await screen.findByText("7")).toBeInTheDocument();
  });

  it("follows a changed parent project and ignores the previous project's late response", async () => {
    const old = deferred<UsageAggregatePage>();
    api.settingsUsagePage.mockImplementation((filter: { project_id?: string }) => {
      if (filter.project_id === "project-old") return old.promise;
      return Promise.resolve(aggregate(22, {
        projects: [{ key: "project-new", label: "New project usage", totals: totals(22) }],
      }));
    });
    const view = render(<UsageSettings locale="en-US" projects={[project("project-old", "Old"), project("project-new", "New")]} selectedProjectId="project-old" />);
    view.rerender(<UsageSettings locale="en-US" projects={[project("project-old", "Old"), project("project-new", "New")]} selectedProjectId="project-new" />);

    expect(await screen.findByText("New project usage")).toBeInTheDocument();
    old.resolve(aggregate(99, {
      projects: [{ key: "project-old", label: "Late old usage", totals: totals(99) }],
    }));
    await Promise.resolve();
    expect(screen.queryByText("Late old usage")).not.toBeInTheDocument();
    expect(screen.getByRole("combobox", { name: "Project" })).toHaveValue("project-new");
  });

  it("appends conversation pages and opens the exact project and conversation", async () => {
    const row = (project_id: string, conversation_id: string, label: string) => ({
      project_id,
      conversation_id,
      label,
      latest_activity: "2026-09-15T00:00:00Z",
      totals: totals(4),
      incomplete: false,
    });
    api.settingsUsageConversations
      .mockResolvedValueOnce(conversations({ items: [row("project-a", "conversation-a", "First conversation")], next_cursor: "more" }))
      .mockResolvedValueOnce(conversations({ items: [row("project-b", "conversation-b", "Second conversation")] }));
    const onOpenConversation = vi.fn();
    render(<UsageSettings locale="en-US" projects={[]} onOpenConversation={onOpenConversation} />);

    await screen.findByText("First conversation");
    fireEvent.click(screen.getByRole("button", { name: "More" }));
    await screen.findByText("Second conversation");
    fireEvent.click(screen.getAllByRole("button", { name: "Open" })[1]);
    expect(onOpenConversation).toHaveBeenCalledWith("project-b", "conversation-b");
  });

  it("serializes conversation navigation and reports a rejected open inside Usage", async () => {
    const pending = deferred<void>();
    api.settingsUsageConversations.mockResolvedValue(conversations({
      items: [
        { project_id: "project-a", conversation_id: "conversation-a", label: "First conversation", latest_activity: "2026-09-15T00:00:00Z", totals: totals(1), incomplete: false },
        { project_id: "project-b", conversation_id: "conversation-b", label: "Second conversation", latest_activity: "2026-09-15T00:00:00Z", totals: totals(2), incomplete: false },
      ],
    }));
    const onOpenConversation = vi.fn().mockReturnValue(pending.promise);
    render(<UsageSettings locale="en-US" projects={[]} onOpenConversation={onOpenConversation} />);
    await screen.findByText("First conversation");
    const openButtons = screen.getAllByRole("button", { name: "Open" });

    fireEvent.click(openButtons[0]);
    expect(screen.getAllByRole("button", { name: "Opening…" })).toHaveLength(2);
    fireEvent.click(screen.getAllByRole("button", { name: "Opening…" })[1]);
    expect(onOpenConversation).toHaveBeenCalledTimes(1);
    pending.reject(new Error("private navigation detail"));

    expect(await screen.findByRole("alert")).toHaveTextContent("Could not open this conversation");
    expect(screen.queryByText("private navigation detail")).not.toBeInTheDocument();
  });

  it("shows a read error without presenting an empty successful state", async () => {
    api.settingsUsagePage.mockRejectedValue(new Error("offline"));
    render(<UsageSettings locale="en-US" projects={[]} />);

    expect(await screen.findByRole("alert")).toHaveTextContent("Usage loading stopped");
    expect(screen.queryByText("No Agent V4 usage is available.")).not.toBeInTheDocument();
  });

  it("recalculates an all-time filter even though its optional dates remain absent", async () => {
    render(<UsageSettings locale="en-US" projects={[]} />);
    await screen.findByLabelText("Token totals");
    fireEvent.change(screen.getByRole("combobox", { name: "UTC range" }), { target: { value: "all" } });
    await waitFor(() => expect(api.settingsUsagePage).toHaveBeenCalledTimes(2));

    fireEvent.click(screen.getByRole("button", { name: "Recalculate" }));
    await waitFor(() => expect(api.settingsUsagePage).toHaveBeenCalledTimes(3));
    expect(api.settingsUsagePage).toHaveBeenLastCalledWith({}, null);
  });
});
