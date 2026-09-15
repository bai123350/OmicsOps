import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { BrowserSettings } from "./BrowserSettings";
import { SettingsPanel } from "./SettingsPanel";
import * as browserApi from "../../tauri-api";

vi.mock("../../tauri-api", () => ({
  browserGetSettings: vi.fn(),
  browserListAuthorizations: vi.fn(),
  browserRevokeAuthorization: vi.fn(),
  browserSaveSettings: vi.fn(),
  browserSetup: vi.fn(),
}));

const status = (session: "shared" | "workspace", connected: boolean) => ({
  session,
  port: session === "shared" ? 18_775 : 18_776,
  listening: connected,
  connected,
  protocol_version: 1,
  extension_id: "joifljknpalpoppceknociillogolbnb",
  capabilities: ["tabs", "search"],
  tab_summaries: [],
});

const settingsResponse = () => ({
  config: {
    auto_launch: false,
    auto_close_turn_tabs: true,
    browser_path: "C:/Program Files/Google/Chrome/Application/chrome.exe",
    default_search_provider: "google" as const,
    disabled_domains: ["blocked.example"],
    preferred_domains: ["preferred.example"],
  },
  shared: status("shared", true),
  workspace: status("workspace", false),
  extension_path: "E:/Project/OmicsOps/browser-extension",
});

const authorization = {
  id: "authorization-1",
  scope: "project" as const,
  binding: {
    capability: "web_open_tab",
    target_host: "pubmed.ncbi.nlm.nih.gov",
    session: "workspace" as const,
    protocol_version: 1,
  },
  project_id: "project-1",
  created_at_ms: 1_700_000_000_000,
};

function mockedApi() {
  return browserApi as unknown as {
    browserGetSettings: ReturnType<typeof vi.fn>;
    browserListAuthorizations: ReturnType<typeof vi.fn>;
    browserRevokeAuthorization: ReturnType<typeof vi.fn>;
    browserSaveSettings: ReturnType<typeof vi.fn>;
    browserSetup: ReturnType<typeof vi.fn>;
  };
}

beforeEach(() => {
  vi.clearAllMocks();
  mockedApi().browserGetSettings.mockResolvedValue(settingsResponse());
  mockedApi().browserListAuthorizations.mockResolvedValue([authorization]);
  mockedApi().browserSaveSettings.mockImplementation(async (config) => ({ ...settingsResponse(), config }));
  mockedApi().browserSetup.mockImplementation(async (session: "shared" | "workspace") => status(session, true));
  mockedApi().browserRevokeAuthorization.mockResolvedValue(true);
});

describe("BrowserSettings", () => {
  it("loads snake_case browser configuration, saves edits, starts both sessions, and revokes authorization", async () => {
    render(<BrowserSettings locale="en-US" />);

    expect(await screen.findByDisplayValue("C:/Program Files/Google/Chrome/Application/chrome.exe")).toBeInTheDocument();
    expect(screen.getByText("Connected")).toBeInTheDocument();
    expect(screen.getByText("pubmed.ncbi.nlm.nih.gov")).toBeInTheDocument();
    expect(screen.getByDisplayValue("E:/Project/OmicsOps/browser-extension")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("checkbox", { name: "Start browser automatically" }));
    fireEvent.change(screen.getByRole("combobox", { name: "Default search provider" }), { target: { value: "duckduckgo" } });
    fireEvent.change(screen.getByRole("textbox", { name: "Add blocked domain" }), { target: { value: "new.example" } });
    fireEvent.click(screen.getByRole("button", { name: "Add blocked domain submit" }));
    fireEvent.click(screen.getByRole("button", { name: "Save browser settings" }));

    await waitFor(() => expect(mockedApi().browserSaveSettings).toHaveBeenCalledWith(expect.objectContaining({
      auto_launch: true,
      default_search_provider: "duckduckgo",
      disabled_domains: ["blocked.example", "new.example"],
      preferred_domains: ["preferred.example"],
    })));

    fireEvent.click(screen.getByRole("button", { name: "Start shared session" }));
    fireEvent.click(screen.getByRole("button", { name: "Start workspace session" }));
    await waitFor(() => expect(mockedApi().browserSetup).toHaveBeenNthCalledWith(1, "shared", true));
    expect(mockedApi().browserSetup).toHaveBeenNthCalledWith(2, "workspace", true);

    fireEvent.click(screen.getByRole("button", { name: /Revoke authorization pubmed/ }));
    expect(screen.getByRole("dialog", { name: "Revoke browser authorization?" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Revoke authorization" }));
    await waitFor(() => expect(mockedApi().browserRevokeAuthorization).toHaveBeenCalledWith("authorization-1"));
    expect(screen.queryByText("pubmed.ncbi.nlm.nih.gov")).not.toBeInTheDocument();
  });

  it("keeps API failures visible", async () => {
    mockedApi().browserGetSettings.mockRejectedValueOnce(new Error("browser bridge offline"));
    mockedApi().browserListAuthorizations.mockRejectedValueOnce(new Error("authorization store unavailable"));

    render(<BrowserSettings locale="en-US" />);

    expect(await screen.findByRole("alert")).toHaveTextContent("browser bridge offline");
    expect(screen.getByRole("alert")).toHaveTextContent("authorization store unavailable");
  });

  it("does not save defaults when the initial settings baseline could not be loaded", async () => {
    mockedApi().browserGetSettings.mockRejectedValueOnce(new Error("browser settings unavailable"));
    mockedApi().browserListAuthorizations.mockResolvedValueOnce([]);
    render(<BrowserSettings locale="en-US" />);

    expect(await screen.findByRole("alert")).toHaveTextContent("browser settings unavailable");
    const save = screen.getByRole("button", { name: "Save browser settings" });
    expect(save).toBeDisabled();
    fireEvent.click(save);
    expect(mockedApi().browserSaveSettings).not.toHaveBeenCalled();
  });

  it("closes only the revoke confirmation on Escape while the parent Settings dialog stays open", async () => {
    render(<SettingsPanel locale="en-US" onClose={() => undefined} />);
    fireEvent.click(screen.getByRole("button", { name: "Browser" }));
    expect(await screen.findByRole("heading", { name: "Connection status" })).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: /Revoke authorization pubmed/ }));
    expect(screen.getByRole("dialog", { name: "Revoke browser authorization?" })).toBeInTheDocument();

    fireEvent.keyDown(window, { key: "Escape" });

    expect(screen.queryByRole("dialog", { name: "Revoke browser authorization?" })).not.toBeInTheDocument();
    expect(screen.getByRole("dialog", { name: "Workspace settings" })).toBeInTheDocument();
  });
});
