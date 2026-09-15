import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  listCredentials: vi.fn(),
  createCredential: vi.fn(),
  replaceCredential: vi.fn(),
  deleteCredential: vi.fn(),
}));
vi.mock("../../credentials-settings-api", () => ({
  listCredentials: mocks.listCredentials,
  createCredential: mocks.createCredential,
  replaceCredential: mocks.replaceCredential,
  deleteCredential: mocks.deleteCredential,
}));

import type { CredentialEntry } from "../../types";
import { useWindowEscapeLayer } from "./BrowserSettings";
import { CredentialsSettings } from "./CredentialsSettings";

const managed: CredentialEntry = {
  target: { kind: "managed", id: "managed-1" },
  reference: "settings/managed-1",
  label: "NCBI token",
  presence: "missing",
  value_kind: "api_key",
  consumers: [],
  can_replace: true,
  can_delete: true,
};

const ssh: CredentialEntry = {
  target: { kind: "ssh", id: "ssh-1" },
  reference: "ssh/ssh-1",
  label: "Cluster",
  presence: "unavailable",
  value_kind: "ssh_private_key",
  consumers: [{ kind: "ssh", id: "ssh-1", label: "Cluster", binding_name: null }],
  can_replace: true,
  can_delete: false,
};

describe("CredentialsSettings", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.listCredentials.mockResolvedValue([managed, ssh]);
  });

  it("loads actual entries and distinguishes missing from unavailable", async () => {
    render(<CredentialsSettings locale="en-US" onOpenOwner={vi.fn()} />);
    expect(await screen.findByText("NCBI token")).toBeInTheDocument();
    expect(screen.getByText("Missing")).toBeInTheDocument();
    expect(screen.getByText("Unavailable")).toBeInTheDocument();
    expect(screen.getByText(/does not mean the secret is missing/i)).toBeInTheDocument();
  });

  it("does not cross a list snapshot with create, replace, or editor refresh", async () => {
    let finishInitial!: (entries: CredentialEntry[]) => void;
    mocks.listCredentials.mockReturnValueOnce(new Promise((resolve) => { finishInitial = resolve; }));
    render(<CredentialsSettings locale="en-US" onOpenOwner={vi.fn()} />);
    expect(screen.getByRole("button", { name: "New credential" })).toBeDisabled();
    finishInitial([managed]);
    expect(await screen.findByText("NCBI token")).toBeInTheDocument();

    let finishRefresh!: (entries: CredentialEntry[]) => void;
    mocks.listCredentials.mockReturnValueOnce(new Promise((resolve) => { finishRefresh = resolve; }));
    fireEvent.click(screen.getByRole("button", { name: "Refresh" }));
    expect(screen.getByRole("button", { name: "Replace" })).toBeDisabled();
    finishRefresh([managed]);
    await waitFor(() => expect(screen.getByRole("button", { name: "Replace" })).toBeEnabled());
    fireEvent.click(screen.getByRole("button", { name: "Replace" }));
    expect(screen.getByRole("button", { name: "Refresh" })).toBeDisabled();
  });

  it("retains replacement input after a safe failure and sends entity identity plus type", async () => {
    mocks.replaceCredential.mockRejectedValue(new Error("secret-sentinel raw error"));
    render(<CredentialsSettings locale="en-US" onOpenOwner={vi.fn()} />);
    await screen.findByText("NCBI token");
    fireEvent.click(screen.getAllByRole("button", { name: "Replace" })[0]);
    const input = screen.getByLabelText("New credential value");
    fireEvent.change(input, { target: { value: "secret-sentinel" } });
    fireEvent.click(within(screen.getByRole("dialog")).getByRole("button", { name: "Replace" }));
    await screen.findByRole("alert");
    expect(input).toHaveValue("secret-sentinel");
    expect(screen.queryByText(/secret-sentinel raw error/)).not.toBeInTheDocument();
    expect(mocks.replaceCredential).toHaveBeenCalledWith({
      target: managed.target,
      expected_reference: managed.reference,
      expected_value_kind: "api_key",
      secret: "secret-sentinel",
    });
  });

  it("turns an unconfirmed create into replacement of the same managed id", async () => {
    mocks.createCredential.mockResolvedValue({ kind: "secret_save_not_confirmed", entry: managed });
    mocks.replaceCredential.mockResolvedValue({ ...managed, presence: "present" });
    render(<CredentialsSettings locale="en-US" onOpenOwner={vi.fn()} />);
    await screen.findByText("NCBI token");
    fireEvent.click(screen.getByRole("button", { name: "New credential" }));
    fireEvent.change(screen.getByLabelText("Credential label"), { target: { value: "Retry item" } });
    fireEvent.change(screen.getByLabelText("Secret value"), { target: { value: "retry-secret" } });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    expect(await screen.findByText(/could not confirm this save/i)).toBeInTheDocument();
    expect(screen.getByLabelText("New credential value")).toHaveValue("retry-secret");
    expect(mocks.createCredential).toHaveBeenCalledTimes(1);
    fireEvent.click(within(screen.getByRole("dialog")).getByRole("button", { name: "Replace" }));
    await waitFor(() => expect(mocks.replaceCredential).toHaveBeenCalledWith(expect.objectContaining({
      target: managed.target,
      expected_reference: managed.reference,
      secret: "retry-secret",
    })));
    expect(mocks.createCredential).toHaveBeenCalledTimes(1);
  });

  it("disables editable fields while a mutation is pending", async () => {
    let finish!: (value: CredentialEntry) => void;
    mocks.replaceCredential.mockReturnValue(new Promise((resolve) => { finish = resolve; }));
    render(<CredentialsSettings locale="en-US" onOpenOwner={vi.fn()} />);
    await screen.findByText("NCBI token");
    fireEvent.click(screen.getAllByRole("button", { name: "Replace" })[0]);
    const input = screen.getByLabelText("New credential value");
    fireEvent.change(input, { target: { value: "next" } });
    fireEvent.click(within(screen.getByRole("dialog")).getByRole("button", { name: "Replace" }));
    expect(input).toBeDisabled();
    await act(async () => finish({ ...managed, presence: "present" }));
    await waitFor(() => expect(screen.queryByLabelText("New credential value")).not.toBeInTheDocument());
  });

  it("closes only the confirmation on the first immediate Escape", async () => {
    const closeParent = vi.fn();
    function Host() {
      useWindowEscapeLayer(true, closeParent);
      return <CredentialsSettings locale="en-US" onOpenOwner={vi.fn()} />;
    }
    render(<Host />);
    await screen.findByText("NCBI token");
    fireEvent.click(screen.getByRole("button", { name: "Delete" }));
    expect(screen.getByRole("alertdialog")).toBeInTheDocument();
    fireEvent.keyDown(window, { key: "Escape" });
    expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument();
    expect(closeParent).not.toHaveBeenCalled();
    fireEvent.keyDown(window, { key: "Escape" });
    expect(closeParent).toHaveBeenCalledTimes(1);
  });

  it("keeps an entry when deletion discovers a new consumer", async () => {
    mocks.deleteCredential.mockResolvedValue({ kind: "in_use", consumers: [{ kind: "mcp", id: "server-1", label: "Papers", binding_name: "TOKEN" }] });
    render(<CredentialsSettings locale="en-US" onOpenOwner={vi.fn()} />);
    await screen.findByText("NCBI token");
    fireEvent.click(screen.getByRole("button", { name: "Delete" }));
    fireEvent.click(within(screen.getByRole("alertdialog")).getByRole("button", { name: "Delete" }));
    expect(await screen.findByText(/configuration now uses this credential/i)).toBeInTheDocument();
    expect(screen.getByText("NCBI token")).toBeInTheDocument();
    expect(screen.getByText("Papers · TOKEN")).toBeInTheDocument();
  });
});
