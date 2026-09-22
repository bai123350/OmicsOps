import { useState } from "react";
import { fireEvent, render, screen, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { AppearanceProvider, APPEARANCE_STORAGE_KEY } from "../../use-appearance";
import { useWindowEscapeLayer } from "../settings/BrowserSettings";
import { ApplicationMenuBar, type ApplicationMenuBarProps } from "./ApplicationMenuBar";

const defaults = (): ApplicationMenuBarProps => ({
  locale: "en-US",
  hasProject: true,
  onNewProject: vi.fn(),
  onNewConversation: vi.fn(),
  onProjects: vi.fn(),
  onSearch: vi.fn(),
  onSettings: vi.fn(),
  onFiles: vi.fn(),
});

function renderMenu(overrides: Partial<ApplicationMenuBarProps> = {}) {
  const props = { ...defaults(), ...overrides };
  render(<AppearanceProvider><ApplicationMenuBar {...props} /></AppearanceProvider>);
  return props;
}

beforeEach(() => window.localStorage.clear());

describe("ApplicationMenuBar", () => {
  it("opens one of four menus, switches menus and closes a repeated selection", () => {
    renderMenu();
    const bar = screen.getByRole("menubar");
    expect(within(bar).getAllByRole("menuitem").map((item) => item.textContent)).toEqual(["File", "Edit", "View", "Help"]);
    fireEvent.click(within(bar).getByRole("menuitem", { name: "File" }));
    expect(screen.getByRole("menu", { name: "File" })).toBeInTheDocument();
    fireEvent.click(within(bar).getByRole("menuitem", { name: "Edit" }));
    expect(screen.queryByRole("menu", { name: "File" })).not.toBeInTheDocument();
    expect(screen.getByRole("menuitem", { name: /Search/ })).toBeInTheDocument();
    fireEvent.click(within(bar).getByRole("menuitem", { name: "Edit" }));
    expect(screen.queryByRole("menu")).not.toBeInTheDocument();
  });

  it("routes file commands and closes their menu", () => {
    const props = renderMenu();
    for (const label of ["New project", "New conversation", "Projects", "Settings"]) {
      fireEvent.click(screen.getByRole("menuitem", { name: "File" }));
      fireEvent.click(screen.getByRole("menuitem", { name: label }));
      expect(screen.queryByRole("menu")).not.toBeInTheDocument();
    }
    expect(props.onNewProject).toHaveBeenCalledOnce();
    expect(props.onNewConversation).toHaveBeenCalledOnce();
    expect(props.onProjects).toHaveBeenCalledOnce();
    expect(props.onSettings).toHaveBeenCalledWith("general");
  });

  it("routes edit and view commands to the existing destinations", () => {
    const props = renderMenu();
    for (const label of ["Search", "Models", "Skills", "MCP connections"]) {
      fireEvent.click(screen.getByRole("menuitem", { name: "Edit" }));
      fireEvent.click(screen.getByRole("menuitem", { name: new RegExp(`^${label}`) }));
    }
    fireEvent.click(screen.getByRole("menuitem", { name: "View" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "Project files" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "View" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "Appearance" }));
    expect(props.onSearch).toHaveBeenCalledOnce();
    expect(props.onSettings).toHaveBeenNthCalledWith(1, "models");
    expect(props.onSettings).toHaveBeenNthCalledWith(2, "skills");
    expect(props.onSettings).toHaveBeenNthCalledWith(3, "connections");
    expect(props.onSettings).toHaveBeenNthCalledWith(4, "appearance");
    expect(props.onFiles).toHaveBeenCalledOnce();
  });

  it("disables project actions without a project and skips them during keyboard navigation", () => {
    const props = renderMenu({ hasProject: false });
    fireEvent.keyDown(screen.getByRole("menuitem", { name: "File" }), { key: "ArrowDown" });
    expect(screen.getByRole("menuitem", { name: "New conversation" })).toBeDisabled();
    fireEvent.keyDown(screen.getByRole("menuitem", { name: "New project" }), { key: "ArrowDown" });
    expect(screen.getByRole("menuitem", { name: "Projects" })).toHaveFocus();
    fireEvent.click(screen.getByRole("menuitem", { name: "New conversation" }));
    expect(props.onNewConversation).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("menuitem", { name: "View" }));
    expect(screen.getByRole("menuitem", { name: "Project files" })).toBeDisabled();
  });

  it("disables new conversations while the selected project is busy", () => {
    renderMenu({ newConversationDisabled: true });
    fireEvent.click(screen.getByRole("menuitem", { name: "File" }));
    expect(screen.getByRole("menuitem", { name: "New conversation" })).toBeDisabled();
  });

  it("navigates menus with arrow, Home and End keys and exits with Tab", () => {
    renderMenu();
    const file = screen.getByRole("menuitem", { name: "File" });
    file.focus();
    fireEvent.keyDown(file, { key: "ArrowRight" });
    const edit = screen.getByRole("menuitem", { name: "Edit" });
    expect(edit).toHaveFocus();
    fireEvent.keyDown(edit, { key: "ArrowUp" });
    expect(screen.getByRole("menuitem", { name: "MCP connections" })).toHaveFocus();
    fireEvent.keyDown(document.activeElement!, { key: "Home" });
    expect(screen.getByRole("menuitem", { name: /Search/ })).toHaveFocus();
    fireEvent.keyDown(document.activeElement!, { key: "End" });
    expect(screen.getByRole("menuitem", { name: "MCP connections" })).toHaveFocus();
    fireEvent.keyDown(document.activeElement!, { key: "ArrowRight" });
    expect(screen.getByRole("menuitem", { name: "Project files" })).toHaveFocus();
    fireEvent.keyDown(document.activeElement!, { key: "ArrowLeft" });
    expect(screen.getByRole("menuitem", { name: /Search/ })).toHaveFocus();
    fireEvent.keyDown(document.activeElement!, { key: "Tab" });
    expect(screen.queryByRole("menu")).not.toBeInTheDocument();
  });

  it("closes on an outside pointer press and restores trigger focus on window Escape", () => {
    renderMenu();
    const file = screen.getByRole("menuitem", { name: "File" });
    fireEvent.click(file);
    fireEvent.pointerDown(document.body);
    expect(screen.queryByRole("menu")).not.toBeInTheDocument();
    fireEvent.click(file);
    fireEvent.keyDown(window, { key: "Escape" });
    expect(screen.queryByRole("menu")).not.toBeInTheDocument();
    expect(file).toHaveFocus();
  });

  it("closes the menu when pressing the title area or window controls", () => {
    renderMenu({ children: <button type="button">Minimize window</button> });
    fireEvent.click(screen.getByRole("menuitem", { name: "File" }));
    fireEvent.pointerDown(screen.getByText("OmicsOps"));
    expect(screen.queryByRole("menu")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("menuitem", { name: "Edit" }));
    fireEvent.pointerDown(screen.getByRole("button", { name: "Minimize window" }));
    expect(screen.queryByRole("menu")).not.toBeInTheDocument();
  });

  it("consumes only the top Escape layer even before menu focus moves", () => {
    function Parent() {
      const [open, setOpen] = useState(true);
      useWindowEscapeLayer(open, () => setOpen(false));
      return <>{open && <div role="dialog" aria-label="Parent" />}<ApplicationMenuBar {...defaults()} /></>;
    }
    render(<AppearanceProvider><Parent /></AppearanceProvider>);
    fireEvent.click(screen.getByRole("menuitem", { name: "File" }));
    document.body.focus();
    fireEvent.keyDown(window, { key: "Escape" });
    expect(screen.queryByRole("menu")).not.toBeInTheDocument();
    expect(screen.getByRole("dialog", { name: "Parent" })).toBeInTheDocument();
    fireEvent.keyDown(window, { key: "Escape" });
    expect(screen.queryByRole("dialog", { name: "Parent" })).not.toBeInTheDocument();
  });

  it("updates and persists theme with radio menu semantics", () => {
    renderMenu();
    fireEvent.click(screen.getByRole("menuitem", { name: "View" }));
    expect(screen.getByRole("menuitemradio", { name: "System" })).toHaveAttribute("aria-checked", "true");
    fireEvent.click(screen.getByRole("menuitemradio", { name: "Dark" }));
    expect(document.documentElement.dataset.omicsopsTheme).toBe("dark");
    expect(JSON.parse(window.localStorage.getItem(APPEARANCE_STORAGE_KEY)!)).toMatchObject({ theme: "dark" });
    fireEvent.click(screen.getByRole("menuitem", { name: "View" }));
    expect(screen.getByRole("menuitemradio", { name: "Dark" })).toHaveAttribute("aria-checked", "true");
  });

  it("opens localized help and about dialogs with their own Escape layer", () => {
    renderMenu({ locale: "zh-CN" });
    fireEvent.click(screen.getByRole("menuitem", { name: "Help" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "快速入门" }));
    expect(screen.getByRole("dialog", { name: "快速入门" })).toHaveTextContent("项目");
    fireEvent.click(screen.getByRole("menuitem", { name: "Help" }));
    fireEvent.keyDown(window, { key: "Escape" });
    expect(screen.getByRole("dialog", { name: "快速入门" })).toBeInTheDocument();
    fireEvent.keyDown(window, { key: "Escape" });
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(screen.getByRole("menuitem", { name: "Help" })).toHaveFocus();
    fireEvent.click(screen.getByRole("menuitem", { name: "Help" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "关于 OmicsOps" }));
    expect(screen.getByRole("dialog", { name: "关于 OmicsOps" })).toHaveTextContent("OmicsOps");
    fireEvent.click(screen.getByRole("button", { name: "关闭" }));
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("reports rejected asynchronous menu commands", async () => {
    const onError = vi.fn();
    renderMenu({ onNewConversation: async () => { throw new Error("Conversation unavailable"); }, onError });
    fireEvent.click(screen.getByRole("menuitem", { name: "File" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "New conversation" }));
    await vi.waitFor(() => expect(onError).toHaveBeenCalledWith("Conversation unavailable"));
  });

  it("limits draggable regions to branding and empty title space", () => {
    renderMenu({ children: <button type="button">Close window</button> });
    expect(screen.getByText("OmicsOps").closest("[data-tauri-drag-region]")).not.toBeNull();
    expect(screen.getByRole("menubar").closest("[data-tauri-drag-region]")).toBeNull();
    expect(screen.getByRole("button", { name: "Close window" }).closest("[data-tauri-drag-region]")).toBeNull();
  });
});
