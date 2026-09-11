import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { SettingsPanel } from "./SettingsPanel";
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
const mockedInvoke = vi.mocked(invoke);
const defaults = { max_iterations: 100, auto_continue: false, auto_continue_limit: 10, auto_compact: true, follow_up_questions: true };
beforeEach(() => { mockedInvoke.mockReset(); mockedInvoke.mockResolvedValue({ max_iterations: 100 }); });
function open(onClose = vi.fn()) { render(<SettingsPanel locale="en-US" initialSection="agent" onClose={onClose} />); return onClose; }
it("loads legacy settings into Session grouped rows with defaults", async () => {
  open();
  expect(screen.getByRole("button", { name: "Session" })).toBeInTheDocument();
  expect(screen.getByRole("heading", { name: "Session" })).toBeInTheDocument();
  await waitFor(() => expect(screen.getByLabelText("Maximum agent iterations per turn")).toHaveValue(100));
  expect(screen.getByLabelText("Maximum agent iterations per turn").closest(".session-setting-row")).not.toBeNull();
  expect(screen.getByRole("switch", { name: "Auto-continue truncated output" })).not.toBeChecked();
  expect(screen.getByLabelText("Maximum automatic continuations per turn")).toBeDisabled();
  expect(screen.getByLabelText("Maximum automatic continuations per turn")).toHaveValue(10);
  expect(screen.getByRole("switch", { name: "Automatically compact long conversations" })).toBeChecked();
  expect(screen.getByRole("switch", { name: "Suggest follow-up questions" })).toBeChecked();
});
it("saves every Session control including zero limits", async () => {
  open(); await waitFor(() => expect(screen.getByRole("button", { name: "Save" })).toBeEnabled());
  fireEvent.change(screen.getByLabelText("Maximum agent iterations per turn"), { target: { value: "0" } });
  fireEvent.click(screen.getByRole("switch", { name: "Auto-continue truncated output" }));
  expect(screen.getByLabelText("Maximum automatic continuations per turn")).toBeEnabled();
  fireEvent.change(screen.getByLabelText("Maximum automatic continuations per turn"), { target: { value: "0" } });
  fireEvent.click(screen.getByRole("switch", { name: "Automatically compact long conversations" }));
  fireEvent.click(screen.getByRole("switch", { name: "Suggest follow-up questions" }));
  mockedInvoke.mockResolvedValueOnce({ ...defaults, max_iterations: 0, auto_continue: true, auto_continue_limit: 0, auto_compact: false, follow_up_questions: false });
  fireEvent.click(screen.getByRole("button", { name: "Save" }));
  await waitFor(() => expect(mockedInvoke).toHaveBeenCalledWith("agent_save_iteration_settings", { settings: { max_iterations: 0, auto_continue: true, auto_continue_limit: 0, auto_compact: false, follow_up_questions: false } }));
});
it("Cancel restores saved settings without closing the panel", async () => {
  const close = open(); await waitFor(() => expect(screen.getByRole("button", { name: "Save" })).toBeEnabled());
  fireEvent.change(screen.getByLabelText("Maximum agent iterations per turn"), { target: { value: "7" } });
  fireEvent.click(screen.getByRole("switch", { name: "Auto-continue truncated output" }));
  fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
  expect(screen.getByLabelText("Maximum agent iterations per turn")).toHaveValue(100);
  expect(screen.getByRole("switch", { name: "Auto-continue truncated output" })).not.toBeChecked();
  expect(close).not.toHaveBeenCalled();
});
it.each(["", "-1", "1.5", "4294967296"])("rejects invalid iteration limit %s", async value => {
  open(); await waitFor(() => expect(screen.getByRole("button", { name: "Save" })).toBeEnabled());
  fireEvent.change(screen.getByLabelText("Maximum agent iterations per turn"), { target: { value } });
  expect(screen.getByRole("button", { name: "Save" })).toBeDisabled();
});
it("rejects invalid continuation limit", async () => {
  open(); await waitFor(() => expect(screen.getByRole("button", { name: "Save" })).toBeEnabled());
  fireEvent.click(screen.getByRole("switch", { name: "Auto-continue truncated output" }));
  fireEvent.change(screen.getByLabelText("Maximum automatic continuations per turn"), { target: { value: "-1" } });
  expect(screen.getByRole("button", { name: "Save" })).toBeDisabled();
});
it("reports save failures", async () => {
  open(); await waitFor(() => expect(screen.getByRole("button", { name: "Save" })).toBeEnabled());
  mockedInvoke.mockRejectedValueOnce(new Error("database unavailable"));
  fireEvent.click(screen.getByRole("button", { name: "Save" }));
  expect(await screen.findByRole("alert")).toHaveTextContent("database unavailable");
});
it("disabling continuation restores an invalid dependent limit so other settings can save", async () => {
  open(); await waitFor(() => expect(screen.getByRole("button", { name: "Save" })).toBeEnabled());
  fireEvent.click(screen.getByRole("switch", { name: "Auto-continue truncated output" }));
  fireEvent.change(screen.getByLabelText("Maximum automatic continuations per turn"), { target: { value: "" } });
  expect(screen.getByRole("button", { name: "Save" })).toBeDisabled();
  fireEvent.click(screen.getByRole("switch", { name: "Auto-continue truncated output" }));
  expect(screen.getByRole("button", { name: "Save" })).toBeEnabled();
  expect(screen.getByLabelText("Maximum automatic continuations per turn")).toHaveValue(10);
});
it("closes settings on immediate Escape without moving focus", () => {
  const close = open(); fireEvent.keyDown(window, { key: "Escape" }); expect(close).toHaveBeenCalledTimes(1);
});
it("keeps save disabled when loading settings fails", async () => {
  mockedInvoke.mockRejectedValueOnce(new Error("cannot load settings")); open();
  expect(await screen.findByRole("alert")).toHaveTextContent("cannot load settings");
  expect(screen.getByRole("button", { name: "Save" })).toBeDisabled();
});
it("Cancel restores the latest successful save", async () => {
  open(); await waitFor(() => expect(screen.getByRole("button", { name: "Save" })).toBeEnabled());
  fireEvent.change(screen.getByLabelText("Maximum agent iterations per turn"), { target: { value: "42" } });
  mockedInvoke.mockResolvedValueOnce({ ...defaults, max_iterations: 42 });
  fireEvent.click(screen.getByRole("button", { name: "Save" }));
  await screen.findByRole("status");
  fireEvent.change(screen.getByLabelText("Maximum agent iterations per turn"), { target: { value: "5" } });
  fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
  expect(screen.getByLabelText("Maximum agent iterations per turn")).toHaveValue(42);
});
