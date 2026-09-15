import { fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";

import { AppearanceProvider } from "../../use-appearance";
import { AppearanceSettings } from "./AppearanceSettings";

beforeEach(() => {
  window.localStorage.clear();
  Object.defineProperty(window, "matchMedia", {
    configurable: true,
    value: () => ({
      matches: false,
      media: "(prefers-color-scheme: dark)",
      onchange: null,
      addEventListener() {},
      removeEventListener() {},
      addListener() {},
      removeListener() {},
      dispatchEvent: () => true,
    }),
  });
});

function renderPage(locale: "zh-CN" | "en-US" = "en-US") {
  return render(
    <AppearanceProvider>
      <AppearanceSettings locale={locale} />
    </AppearanceProvider>,
  );
}

describe("AppearanceSettings", () => {
  it("updates theme, UI font, code font, and interface scale through one provider", () => {
    renderPage();

    fireEvent.change(screen.getByLabelText("Theme"), { target: { value: "dark" } });
    fireEvent.change(screen.getByLabelText("Interface font"), { target: { value: "sans" } });
    fireEvent.change(screen.getByLabelText("Code font"), { target: { value: "mono" } });
    fireEvent.change(screen.getByRole("combobox", { name: "Interface scale" }), { target: { value: "1.2" } });

    expect(document.documentElement).toHaveAttribute("data-omicsops-theme", "dark");
    expect(document.documentElement).toHaveAttribute("data-omicsops-ui-font", "sans");
    expect(document.documentElement).toHaveAttribute("data-omicsops-code-font", "mono");
    expect(document.documentElement).toHaveAttribute("data-omicsops-scale", "1.2");
    expect(screen.getByText("120% — Largest")).toBeInTheDocument();
  });

  it("restores saved choices after the provider is remounted", () => {
    const first = renderPage();
    fireEvent.change(screen.getByLabelText("Theme"), { target: { value: "light" } });
    fireEvent.change(screen.getByRole("combobox", { name: "Interface scale" }), { target: { value: "0.9" } });
    first.unmount();

    renderPage();

    expect(screen.getByLabelText("Theme")).toHaveValue("light");
    expect(screen.getByRole("combobox", { name: "Interface scale" })).toHaveValue("0.9");
  });

  it("renders localized labels and describes scale as the whole interface", () => {
    renderPage("zh-CN");

    expect(screen.getByRole("heading", { name: "外观" })).toBeInTheDocument();
    expect(screen.getByRole("combobox", { name: "界面缩放" })).toBeInTheDocument();
    expect(screen.getByText("同时缩放导航、正文、输入框和对话框，而不只是文字。")).toBeInTheDocument();
  });

  it("fails clearly when rendered outside AppearanceProvider", () => {
    expect(() => render(<AppearanceSettings locale="en-US" />)).toThrow(
      "useAppearance must be used within AppearanceProvider",
    );
  });
});
