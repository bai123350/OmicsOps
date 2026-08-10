import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { SettingsPanel } from "./SettingsPanel";

describe("SettingsPanel model providers", () => {
  it("collects provider configuration without rendering stored credentials", async () => {
    const onSaveModel = vi.fn().mockResolvedValue(undefined);
    render(<SettingsPanel locale="en-US" onClose={() => undefined} modelProfiles={[]} onSaveModel={onSaveModel} />);
    fireEvent.click(screen.getByRole("button", { name: "Configure Anthropic" }));
    fireEvent.change(screen.getByLabelText("Profile label"), { target: { value: "Lab Claude" } });
    fireEvent.change(screen.getByLabelText("Model"), { target: { value: "claude-science" } });
    fireEvent.change(screen.getByLabelText("API key"), { target: { value: "secret" } });
    fireEvent.click(screen.getByRole("button", { name: "Save provider" }));
    expect(onSaveModel).toHaveBeenCalledWith(expect.objectContaining({ provider: "anthropic", label: "Lab Claude", model: "claude-science", credential: "secret" }));
    await waitFor(() => expect(screen.queryByDisplayValue("secret")).not.toBeInTheDocument());
  });
});
