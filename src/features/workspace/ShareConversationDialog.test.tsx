import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { saveConversationExport } from "../../conversation-export-api";
import { ShareConversationDialog } from "./ShareConversationDialog";
vi.mock("../../conversation-export-api", () => ({ saveConversationExport: vi.fn() }));
const messages = [{ id: "a", role: "user", markdown: "hello" }];
describe("share dialog", () => {
  it("locks repeat export, shows errors and allows retry without losing selection", async () => {
    let reject!: (reason: Error) => void;
    vi.mocked(saveConversationExport).mockImplementationOnce(() => new Promise((_resolve, fail) => { reject = fail; }));
    render(<ShareConversationDialog messages={messages} locale="en-US" onClose={vi.fn()} />);
    fireEvent.change(screen.getByLabelText("Format"), { target: { value: "html" } });
    fireEvent.click(screen.getByRole("button", { name: "Export" }));
    expect(screen.getByRole("button", { name: "Exporting…" })).toBeDisabled();
    reject(new Error("disk full"));
    await waitFor(() => expect(screen.getByRole("alert")).toHaveTextContent("disk full"));
    expect(screen.getByRole("checkbox")).toBeChecked();
    expect(screen.getByRole("button", { name: "Export" })).toBeEnabled();
  });
  it("closes on immediate window Escape", () => {
    const close = vi.fn();
    render(<ShareConversationDialog messages={messages} locale="en-US" onClose={close} />);
    fireEvent.keyDown(window, { key: "Escape" });
    expect(close).toHaveBeenCalledOnce();
  });
  it("supports all/none and keeps preview after cancelled save", async () => {
    vi.mocked(saveConversationExport).mockResolvedValue(null);
    const close = vi.fn();
    render(<ShareConversationDialog messages={messages} locale="en-US" onClose={close} />);
    fireEvent.click(screen.getByRole("button", { name: "Select none" }));
    expect(screen.getByRole("button", { name: "Export" })).toBeDisabled();
    fireEvent.click(screen.getByRole("button", { name: "Select all" }));
    fireEvent.change(screen.getByLabelText("Format"), { target: { value: "html" } });
    fireEvent.click(screen.getByRole("button", { name: "Export" }));
    await waitFor(() => expect(screen.getByRole("button", { name: "Export" })).toBeEnabled());
    expect(close).not.toHaveBeenCalled();
    expect(screen.queryByRole("status")).toBeNull();
  });
});
