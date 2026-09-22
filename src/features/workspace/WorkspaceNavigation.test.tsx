import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, expect, it, vi } from "vitest";
import { WorkspaceNavigation, conversationDateBucket } from "./WorkspaceNavigation";
import * as api from "../../workspace-navigation-api";
import type { WorkspaceConversation } from "../../types";
import { useWindowEscapeLayer } from "../settings/BrowserSettings";

vi.mock("../../workspace-navigation-api", () => ({ listConversationGroups: vi.fn(), saveConversationGroup: vi.fn(), deleteConversationGroup: vi.fn(), moveConversations: vi.fn() }));
const conversation = (id: string, title: string, status: WorkspaceConversation["status"] = "idle"): WorkspaceConversation => ({ id, title, status, project_id: "p", model_profile_id: null, created_at: "2026-09-20T02:00:00Z", updated_at: "2026-09-22T02:00:00Z" });
const props = { projectId: "p", projectName: "RNA", locale: "en-US" as const, conversations: [conversation("a", "Alpha"), conversation("b", "Beta", "completed")], activeConversationId: "a", collapsed: false, onToggleCollapsed: vi.fn(), onNavigate: vi.fn(), onNewConversation: vi.fn(), onSelectConversation: vi.fn(), onOpenSearch: vi.fn(), onBack: vi.fn() };
beforeEach(() => { vi.resetAllMocks(); vi.mocked(api.listConversationGroups).mockResolvedValue({ groups: [], memberships: [] }); });

it("provides all requested sidebar destinations and keeps new session/search shortcuts visible", async () => {
  render(<WorkspaceNavigation {...props} />);
  await waitFor(() => expect(api.listConversationGroups).toHaveBeenCalledWith("p"));
  for (const name of ["Files", "Research journey", "Publication", "Library"]) fireEvent.click(screen.getByRole("button", { name }));
  expect(props.onNavigate.mock.calls.map(([page]) => page)).toEqual(["files", "journey", "publication", "library"]);
  expect(screen.getByText("Ctrl+N")).toBeVisible();
  expect(screen.getByText("Ctrl+K")).toBeVisible();
});

it("creates a group with one stable request when save fails and is retried", async () => {
  vi.mocked(api.saveConversationGroup).mockRejectedValueOnce(new Error("lost reply")).mockResolvedValueOnce({ id: "g", project_id: "p", name: "RNA group", created_at: "", updated_at: "" });
  render(<WorkspaceNavigation {...props} />);
  fireEvent.click(screen.getByRole("button", { name: "New group" }));
  fireEvent.change(screen.getByRole("textbox", { name: "Group name" }), { target: { value: "RNA group" } });
  fireEvent.click(screen.getByRole("button", { name: "Save group" }));
  await screen.findByRole("alert");
  fireEvent.click(screen.getByRole("button", { name: "Save group" }));
  await waitFor(() => expect(api.saveConversationGroup).toHaveBeenCalledTimes(2));
  expect(vi.mocked(api.saveConversationGroup).mock.calls[0][0]).toEqual(vi.mocked(api.saveConversationGroup).mock.calls[1][0]);
});

it("filters sessions and moves selected conversations without deleting them", async () => {
  vi.mocked(api.listConversationGroups).mockResolvedValue({ groups: [{ id: "g", project_id: "p", name: "Group", created_at: "", updated_at: "" }], memberships: [] });
  render(<WorkspaceNavigation {...props} />);
  await screen.findByRole("button", { name: "Expand group: Group" });
  fireEvent.click(screen.getByRole("button", { name: "Filter sessions" }));
  fireEvent.change(screen.getByRole("textbox", { name: "Filter session titles" }), { target: { value: "Beta" } });
  expect(screen.queryByRole("button", { name: "Alpha" })).not.toBeInTheDocument();
  fireEvent.keyDown(window, { key: "Escape" });
  expect(screen.queryByRole("textbox", { name: "Filter session titles" })).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Select sessions" }));
  fireEvent.click(screen.getByRole("checkbox", { name: "Select: Beta" }));
  fireEvent.change(screen.getByRole("combobox", { name: "Move selected to group" }), { target: { value: "g" } });
  fireEvent.click(screen.getByRole("button", { name: "Move" }));
  await waitFor(() => expect(api.moveConversations).toHaveBeenCalledWith({ project_id: "p", group_id: "g", conversation_ids: ["b"] }));
});

it("closes the group dialog on immediate window Escape", () => {
  render(<WorkspaceNavigation {...props} />);
  fireEvent.click(screen.getByRole("button", { name: "New group" }));
  expect(within(screen.getByRole("dialog")).getByRole("textbox")).toBeVisible();
  fireEvent.keyDown(window, { key: "Escape" });
  expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  expect(screen.getByRole("navigation")).toBeVisible();
});

it("buckets dates by local calendar days including invalid dates", () => {
  const now = new Date(2026, 8, 22, 10);
  expect(conversationDateBucket(new Date(2026, 8, 22, 0).toISOString(), now)).toBe("today");
  expect(conversationDateBucket(new Date(2026, 8, 21, 23).toISOString(), now)).toBe("yesterday");
  expect(conversationDateBucket(new Date(2026, 8, 17).toISOString(), now)).toBe("week");
  expect(conversationDateBucket("invalid", now)).toBe("earlier");
});

it.each([
  "database failed: error returned from database: (code: 2067) UNIQUE constraint failed: conversation_groups.project_id, conversation_groups.name",
  "invalid input: group name must contain 1..80 characters",
])("allows correcting a group name after a definitive host validation rejection: %s", async (error) => {
  vi.mocked(api.saveConversationGroup).mockRejectedValueOnce(new Error(error)).mockResolvedValueOnce({ id: "g", project_id: "p", name: "Corrected", created_at: "", updated_at: "" });
  render(<WorkspaceNavigation {...props} />);
  fireEvent.click(screen.getByRole("button", { name: "New group" }));
  fireEvent.change(screen.getByRole("textbox", { name: "Group name" }), { target: { value: "Rejected" } });
  fireEvent.click(screen.getByRole("button", { name: "Save group" }));
  await screen.findByRole("alert");
  expect(screen.getByRole("textbox", { name: "Group name" })).toBeEnabled();
  fireEvent.change(screen.getByRole("textbox", { name: "Group name" }), { target: { value: "Corrected" } });
  fireEvent.click(screen.getByRole("button", { name: "Save group" }));
  await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
  const [first, second] = vi.mocked(api.saveConversationGroup).mock.calls;
  expect(second[0].name).toBe("Corrected");
  expect(second[0].request_id).not.toBe(first[0].request_id);
});

it("keeps the earliest-created session first regardless of update buckets or groups", async () => {
  const older = { ...conversation("older", "Oldest"), created_at: "2020-01-01T00:00:00Z", updated_at: "2020-02-01T00:00:00Z" };
  const newer = { ...conversation("newer", "Newest"), created_at: "2025-01-01T00:00:00Z", updated_at: new Date().toISOString() };
  vi.mocked(api.listConversationGroups).mockResolvedValue({ groups: [{ id: "g", project_id: "p", name: "Group", created_at: "", updated_at: "" }], memberships: [{ conversation_id: "older", group_id: "g" }] });
  render(<WorkspaceNavigation {...props} conversations={[newer, older]} />);
  await screen.findByRole("button", { name: "Expand group: Group" });
  fireEvent.click(screen.getByRole("button", { name: "Filter sessions" }));
  fireEvent.change(screen.getByRole("combobox", { name: "Session order" }), { target: { value: "created" } });
  expect(screen.getAllByRole("button", { name: /^(Oldest|Newest)$/ }).map((node) => node.textContent)).toEqual(["Oldest", "Newest"]);
});

it("does not consume Escape for a hidden filter after collapsing navigation", async () => {
  const parentClose = vi.fn();
  function Parent({ collapsed }: { collapsed: boolean }) { useWindowEscapeLayer(true, parentClose); return <WorkspaceNavigation {...props} collapsed={collapsed} />; }
  const view = render(<Parent collapsed={false} />);
  fireEvent.click(screen.getByRole("button", { name: "Filter sessions" }));
  view.rerender(<Parent collapsed />);
  fireEvent.keyDown(window, { key: "Escape" });
  expect(parentClose).toHaveBeenCalledTimes(1);
});
