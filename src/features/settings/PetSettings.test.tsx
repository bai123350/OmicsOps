import { fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";

import { PetPreferencesProvider } from "../../use-pet-preferences";
import { PetCompanion } from "../workspace/PetCompanion";
import { PetSettings } from "./PetSettings";

beforeEach(() => window.localStorage.clear());

function renderSettings() {
  return render(
    <PetPreferencesProvider>
      <PetSettings locale="en-US" />
      <PetCompanion />
    </PetPreferencesProvider>,
  );
}

describe("PetSettings", () => {
  it("shows a real preview while the disabled workspace companion stays hidden", () => {
    renderSettings();

    expect(screen.getByLabelText("Pet preview")).toBeInTheDocument();
    expect(screen.queryByLabelText("Open Pip companion")).not.toBeInTheDocument();
  });

  it("synchronizes enabled, name, built-in style, and size with the companion", () => {
    renderSettings();

    fireEvent.click(screen.getByRole("checkbox", { name: "Show research companion" }));
    fireEvent.change(screen.getByRole("textbox", { name: "Companion name" }), { target: { value: "Nova" } });
    fireEvent.change(screen.getByRole("combobox", { name: "Built-in style" }), { target: { value: "cell" } });
    fireEvent.change(screen.getByRole("combobox", { name: "Size" }), { target: { value: "large" } });

    const companion = screen.getByLabelText("Open Nova companion");
    expect(companion.closest(".pet-companion")).toHaveAttribute("data-style", "cell");
    expect(companion.closest(".pet-companion")).toHaveAttribute("data-size", "large");
    expect(screen.getByLabelText("Pet preview").closest(".pet-companion")).toHaveAttribute("data-style", "cell");
  });

  it("restores settings after the provider is remounted", () => {
    const first = renderSettings();
    fireEvent.click(screen.getByRole("checkbox", { name: "Show research companion" }));
    fireEvent.change(screen.getByRole("textbox", { name: "Companion name" }), { target: { value: "Nova" } });
    first.unmount();

    renderSettings();
    expect(screen.getByRole("checkbox", { name: "Show research companion" })).toBeChecked();
    expect(screen.getByRole("textbox", { name: "Companion name" })).toHaveValue("Nova");
  });
});
