import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { useState } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { getReviewerSettingsV4, saveReviewerSettingsV4 } from "../../session-review-api";
import type { ModelProfile, ReviewerSettingsV4 } from "../../types";
import { ReviewerSettingsDialog } from "./ReviewerSettingsDialog";

vi.mock("../../session-review-api", () => ({
  DEFAULT_REVIEWER_SETTINGS: { backend: { kind: "follow_session" }, default_http_profile_id: null },
  getReviewerSettingsV4: vi.fn(),
  saveReviewerSettingsV4: vi.fn(),
}));

const getSettings = vi.mocked(getReviewerSettingsV4);
const saveSettings = vi.mocked(saveReviewerSettingsV4);

const profiles: ModelProfile[] = [
  { id: "profile-openai", label: "OpenAI", provider: "open_ai_compatible", base_url: "https://api.openai.com/v1", model: "gpt-6-astra", credential_reference: "credential:openai", supports_tools: true, supports_vision: true },
  { id: "profile-ollama", label: "Local Ollama", provider: "ollama", base_url: "http://127.0.0.1:11434", model: "llama3", credential_reference: null, supports_tools: true, supports_vision: false },
];
const defaults: ReviewerSettingsV4 = { backend: { kind: "follow_session" }, default_http_profile_id: null };

beforeEach(() => {
  vi.clearAllMocks();
  getSettings.mockResolvedValue(defaults);
  saveSettings.mockImplementation(async (settings) => settings);
});

function renderDialog(overrides: Partial<React.ComponentProps<typeof ReviewerSettingsDialog>> = {}) {
  return render(<ReviewerSettingsDialog zh={false} modelProfiles={profiles} onClose={vi.fn()} {...overrides} />);
}

describe("ReviewerSettingsDialog", () => {
  it("loads the real backend choices and exposes all saved HTTP profiles including Ollama", async () => {
    renderDialog();
    const dialog = await screen.findByRole("dialog", { name: "Reviewer model" });
    const backend = within(dialog).getByRole("combobox", { name: "Reviewer backend" });
    expect(within(backend).getByRole("option", { name: "Follow active session" })).toBeInTheDocument();
    fireEvent.change(backend, { target: { value: "default_http" } });
    const defaultProfile = within(dialog).getByRole("combobox", { name: "Default HTTP profile" });
    expect(within(defaultProfile).getByRole("option", { name: /Local Ollama · llama3/ })).toBeInTheDocument();
    fireEvent.change(defaultProfile, { target: { value: "profile-ollama" } });
    fireEvent.click(within(dialog).getByRole("button", { name: "Save reviewer settings" }));
    await waitFor(() => expect(saveSettings).toHaveBeenCalledWith({ backend: { kind: "default_http" }, default_http_profile_id: "profile-ollama" }));
  });

  it("shows a safe save error and keeps the selected setting available for retry", async () => {
    saveSettings.mockRejectedValue(new Error("private native detail"));
    renderDialog();
    const dialog = await screen.findByRole("dialog", { name: "Reviewer model" });
    fireEvent.change(within(dialog).getByRole("combobox", { name: "Reviewer backend" }), { target: { value: "http_profile" } });
    fireEvent.change(within(dialog).getByRole("combobox", { name: "Reviewer HTTP profile" }), { target: { value: "profile-ollama" } });
    fireEvent.click(within(dialog).getByRole("button", { name: "Save reviewer settings" }));
    expect(await within(dialog).findByRole("alert")).toHaveTextContent("Could not save reviewer settings");
    expect(screen.queryByText("private native detail")).not.toBeInTheDocument();
    expect(within(dialog).getByRole("combobox", { name: "Reviewer backend" })).toHaveValue("follow_session");
  });

  it("does not save defaults when the initial settings load fails", async () => {
    getSettings.mockRejectedValueOnce(new Error("private load detail"));
    renderDialog();
    const dialog = await screen.findByRole("dialog", { name: "Reviewer model" });
    await within(dialog).findByRole("alert");
    expect(within(dialog).getByRole("button", { name: "Save reviewer settings" })).toBeDisabled();
    expect(saveSettings).not.toHaveBeenCalled();
  });

  it("autofocuses, traps Tab, closes immediately on window Escape, and restores launch focus", async () => {
    function Host() {
      const [open, setOpen] = useState(false);
      return <><button type="button" onClick={() => setOpen(true)}>Launch reviewer settings</button>{open && <ReviewerSettingsDialog zh={false} modelProfiles={profiles} onClose={() => setOpen(false)} />}</>;
    }

    render(<Host />);
    const launch = screen.getByRole("button", { name: "Launch reviewer settings" });
    launch.focus();
    fireEvent.click(launch);
    const dialog = await screen.findByRole("dialog", { name: "Reviewer model" });
    const close = within(dialog).getByRole("button", { name: "Close reviewer model settings" });
    expect(document.activeElement).toBe(close);
    fireEvent.keyDown(close, { key: "Tab" });
    expect(document.activeElement).toBe(within(dialog).getByRole("combobox", { name: "Reviewer backend" }));
    fireEvent.keyDown(document.activeElement!, { key: "Tab", shiftKey: true });
    expect(document.activeElement).toBe(close);
    fireEvent.keyDown(window, { key: "Escape" });
    await waitFor(() => expect(screen.queryByRole("dialog", { name: "Reviewer model" })).not.toBeInTheDocument());
    expect(document.activeElement).toBe(launch);
  });
});
