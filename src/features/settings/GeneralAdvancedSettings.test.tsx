import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { GeneralAdvancedSettings } from "./GeneralAdvancedSettings";

const api = vi.hoisted(() => ({
  settingsGeneralPreferences: vi.fn(),
  settingsSaveGeneralPreferences: vi.fn(),
  settingsGeneralSystemStatus: vi.fn(),
  settingsProbeSystemInterpreters: vi.fn(),
  chooseProjectDirectoryStartingAt: vi.fn(),
}));
vi.mock("../../general-settings-api", () => api);

beforeEach(() => {
  vi.clearAllMocks();
  api.settingsGeneralPreferences.mockResolvedValue({ project_directory_start: "E:/Science" });
  api.settingsGeneralSystemStatus.mockResolvedValue({ app_version: "1.2.3", app_data_directory: "C:/AppData/OmicsOps", update_status: "unconfigured", update_source_configured: false });
  api.settingsSaveGeneralPreferences.mockImplementation(async (value) => value);
  api.chooseProjectDirectoryStartingAt.mockResolvedValue({ path: null, usedFallback: false });
  api.settingsProbeSystemInterpreters.mockResolvedValue({
    python: { program: "python", status: "found" },
    r: { program: "Rscript", status: "missing", detail: "not found on system PATH" },
    checked_at: "2026-09-15T00:00:00Z",
  });
});

describe("GeneralAdvancedSettings", () => {
  it("shows the real app state without an update check or unsupported proxy controls", async () => {
    const navigate = vi.fn();
    render(<GeneralAdvancedSettings locale="en-US" onNavigate={navigate} />);
    expect(await screen.findByText("Version 1.2.3")).toBeInTheDocument();
    expect(screen.getByText("No update source is configured.")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /check for updates/i })).not.toBeInTheDocument();
    expect(screen.queryByLabelText(/proxy/i)).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Models" }));
    fireEvent.click(screen.getByRole("button", { name: "Connections" }));
    expect(navigate.mock.calls).toEqual([["models"], ["connections"]]);
  });

  it("saves and clears only the new-project picker start and reports picker fallback", async () => {
    api.chooseProjectDirectoryStartingAt.mockResolvedValue({ path: "D:/New science", usedFallback: true });
    render(<GeneralAdvancedSettings locale="en-US" onNavigate={vi.fn()} />);
    await screen.findByText("E:/Science");
    fireEvent.click(screen.getByRole("button", { name: "Choose…" }));
    await waitFor(() => expect(api.settingsSaveGeneralPreferences).toHaveBeenCalledWith({ project_directory_start: "D:/New science" }));
    expect(screen.getByRole("status")).toHaveTextContent("saved start folder was unavailable");
    fireEvent.click(screen.getByRole("button", { name: "Clear" }));
    await waitFor(() => expect(api.settingsSaveGeneralPreferences).toHaveBeenLastCalledWith({ project_directory_start: null }));
  });

  it("probes only through the fixed host command and states that dependencies remain unverified", async () => {
    render(<GeneralAdvancedSettings locale="en-US" onNavigate={vi.fn()} />);
    await screen.findByText("System Python / R");
    fireEvent.click(screen.getByRole("button", { name: "Check interpreters" }));
    expect(await screen.findByText("Executable found; project dependencies not verified")).toBeInTheDocument();
    expect(screen.getByText("Not found on the system PATH")).toBeInTheDocument();
    expect(api.settingsProbeSystemInterpreters).toHaveBeenCalledWith();
  });
});
