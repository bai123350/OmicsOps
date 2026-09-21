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
const notificationApi = vi.hoisted(() => ({
  settingsNotificationStatus: vi.fn(),
  settingsSetNotificationsEnabled: vi.fn(),
  settingsSendTestNotification: vi.fn(),
}));
vi.mock("../../general-settings-api", () => api);
vi.mock("../../notification-settings-api", () => notificationApi);

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
  notificationApi.settingsNotificationStatus.mockResolvedValue({ preference_enabled: false, permission: "prompt", platform: "windows", last_failure: null });
  notificationApi.settingsSetNotificationsEnabled.mockResolvedValue({ preference_enabled: true, permission: "denied", platform: "windows", last_failure: null });
  notificationApi.settingsSendTestNotification.mockResolvedValue({ preference_enabled: true, permission: "granted", platform: "windows", last_failure: null });
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

  it("keeps notifications off by default and exposes the real permission result", async () => {
    render(<GeneralAdvancedSettings locale="en-US" onNavigate={vi.fn()} />);
    expect(await screen.findByText("System notifications")).toBeInTheDocument();
    expect(screen.getByText(/Permission not requested/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Enable notifications" }));
    await waitFor(() => expect(notificationApi.settingsSetNotificationsEnabled).toHaveBeenCalledWith(true));
    expect(await screen.findByText(/Permission denied by the system/)).toBeInTheDocument();
  });

  it("keeps general settings usable when notification status cannot be read", async () => {
    notificationApi.settingsNotificationStatus.mockRejectedValueOnce(new Error("Notification status unavailable"));
    render(<GeneralAdvancedSettings locale="en-US" onNavigate={vi.fn()} />);

    expect(await screen.findByText("Version 1.2.3")).toBeInTheDocument();
    expect(screen.getByText("Notification status unavailable")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Retry notification status" }));
    expect(await screen.findByText(/Permission not requested/)).toBeInTheDocument();
    expect(notificationApi.settingsNotificationStatus).toHaveBeenCalledTimes(2);
  });

  it("sends an explicit test and keeps a visible native failure", async () => {
    notificationApi.settingsNotificationStatus.mockResolvedValue({
      preference_enabled: true,
      permission: "granted",
      platform: "windows",
      last_failure: { message: "System notification delivery failed", occurred_at: "2026-09-15T00:00:00Z" },
    });
    notificationApi.settingsSendTestNotification.mockRejectedValueOnce(new Error("System notification delivery failed"));
    render(<GeneralAdvancedSettings locale="en-US" onNavigate={vi.fn()} />);
    expect(await screen.findByText("System notification delivery failed")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Send test notification" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("System notification delivery failed");
  });
});
