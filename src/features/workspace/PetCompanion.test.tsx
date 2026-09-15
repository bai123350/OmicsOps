import { act, fireEvent, render, screen } from "@testing-library/react";
import { useState } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import {
  PET_ENABLED_STORAGE_KEY,
  PetPreferencesProvider,
} from "../../use-pet-preferences";
import { useWindowEscapeLayer } from "../settings/BrowserSettings";
import { PetCompanion, type PetCompanionStatus } from "./PetCompanion";

function installReducedMotion(initial = false) {
  let matches = initial;
  const listeners = new Set<(event: MediaQueryListEvent) => void>();
  const addEventListener = vi.fn((_type: string, listener: (event: MediaQueryListEvent) => void) => listeners.add(listener));
  const removeEventListener = vi.fn((_type: string, listener: (event: MediaQueryListEvent) => void) => listeners.delete(listener));
  Object.defineProperty(window, "matchMedia", {
    configurable: true,
    value: vi.fn((query: string) => ({
      get matches() { return query === "(prefers-reduced-motion: reduce)" ? matches : false; },
      media: query,
      onchange: null,
      addEventListener,
      removeEventListener,
      addListener: vi.fn(),
      removeListener: vi.fn(),
      dispatchEvent: vi.fn(),
    })),
  });
  return {
    addEventListener,
    removeEventListener,
    emit(next: boolean) {
      matches = next;
      const event = { matches: next, media: "(prefers-reduced-motion: reduce)" } as MediaQueryListEvent;
      listeners.forEach((listener) => listener(event));
    },
  };
}

function renderEnabled(ui: React.ReactNode) {
  window.localStorage.setItem(PET_ENABLED_STORAGE_KEY, "true");
  return render(<PetPreferencesProvider>{ui}</PetPreferencesProvider>);
}

beforeEach(() => {
  window.localStorage.clear();
  installReducedMotion(false);
});

describe("PetCompanion", () => {
  it("does not render while disabled", () => {
    render(<PetPreferencesProvider><PetCompanion /></PetPreferencesProvider>);
    expect(screen.queryByLabelText("Open Pip companion")).not.toBeInTheDocument();
  });

  it("does not invent a run state when the host provides none", () => {
    renderEnabled(<PetCompanion />);
    expect(screen.getByLabelText("Open Pip companion")).toBeInTheDocument();
    expect(screen.queryByText("Idle")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "View current run" })).not.toBeInTheDocument();
  });

  it("updates from the explicit host status and opens the run only on command", () => {
    const onOpenCurrentRun = vi.fn();
    function Harness() {
      const [status, setStatus] = useState<PetCompanionStatus>("idle");
      return <>
        <button onClick={() => setStatus("running")}>Set running</button>
        <PetCompanion status={status} onOpenCurrentRun={onOpenCurrentRun} />
      </>;
    }
    renderEnabled(<Harness />);

    fireEvent.click(screen.getByLabelText("Open Pip companion"));
    expect(screen.getByText("Idle")).toBeInTheDocument();
    expect(onOpenCurrentRun).not.toHaveBeenCalled();

    fireEvent.click(screen.getByText("Set running"));
    expect(screen.getByText("Running")).toBeInTheDocument();
    expect(onOpenCurrentRun).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole("button", { name: "View current run" }));
    expect(onOpenCurrentRun).toHaveBeenCalledTimes(1);
  });

  it("honors reduced motion changes and cleans up the media listener", () => {
    const media = installReducedMotion(true);
    const view = renderEnabled(<PetCompanion />);
    const companion = screen.getByLabelText("Open Pip companion").closest(".pet-companion");
    expect(companion).toHaveAttribute("data-motion", "reduced");
    expect(media.addEventListener).toHaveBeenCalledTimes(1);

    act(() => media.emit(false));
    expect(companion).toHaveAttribute("data-motion", "full");
    view.unmount();
    expect(media.removeEventListener).toHaveBeenCalledTimes(1);
  });

  it("closes its details before the parent Escape layer", () => {
    const parentClose = vi.fn();
    function Harness() {
      useWindowEscapeLayer(true, parentClose);
      return <PetCompanion />;
    }
    renderEnabled(<Harness />);

    fireEvent.click(screen.getByLabelText("Open Pip companion"));
    expect(screen.getByRole("dialog", { name: "Pip details" })).toBeInTheDocument();
    fireEvent.keyDown(window, { key: "Escape" });
    expect(screen.queryByRole("dialog", { name: "Pip details" })).not.toBeInTheDocument();
    expect(parentClose).not.toHaveBeenCalled();

    fireEvent.keyDown(window, { key: "Escape" });
    expect(parentClose).toHaveBeenCalledTimes(1);
  });

  it("can be hidden without calling a run callback", () => {
    const onOpenCurrentRun = vi.fn();
    renderEnabled(<PetCompanion status="needs_attention" onOpenCurrentRun={onOpenCurrentRun} />);

    fireEvent.click(screen.getByRole("button", { name: "Hide Pip" }));
    expect(screen.queryByLabelText("Open Pip companion")).not.toBeInTheDocument();
    expect(onOpenCurrentRun).not.toHaveBeenCalled();
  });
});
