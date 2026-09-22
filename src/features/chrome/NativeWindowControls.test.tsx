import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { NativeWindowControls } from "./NativeWindowControls";

const native = vi.hoisted(() => ({
  isTauri: vi.fn(),
  getCurrentWindow: vi.fn(),
  minimize: vi.fn(),
  toggleMaximize: vi.fn(),
  close: vi.fn(),
  isMaximized: vi.fn(),
  onResized: vi.fn(),
  unlisten: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({ isTauri: native.isTauri }));
vi.mock("@tauri-apps/api/window", () => ({ getCurrentWindow: native.getCurrentWindow }));

beforeEach(() => {
  vi.resetAllMocks();
  vi.spyOn(navigator, "userAgent", "get").mockReturnValue("Mozilla/5.0 (Windows NT 10.0; Win64; x64)");
  native.isTauri.mockReturnValue(true);
  native.getCurrentWindow.mockReturnValue(native);
  native.minimize.mockResolvedValue(undefined);
  native.toggleMaximize.mockResolvedValue(undefined);
  native.close.mockResolvedValue(undefined);
  native.isMaximized.mockResolvedValue(false);
  native.onResized.mockResolvedValue(native.unlisten);
});

afterEach(() => vi.restoreAllMocks());

describe("native Windows title bar controls", () => {
  it("minimizes, maximizes, and closes the native window from the matching controls", async () => {
    render(<NativeWindowControls locale="en-US" />);
    fireEvent.click(screen.getByRole("button", { name: "Minimize window" }));
    fireEvent.click(screen.getByRole("button", { name: "Maximize window" }));
    fireEvent.click(screen.getByRole("button", { name: "Close window" }));
    await waitFor(() => {
      expect(native.minimize).toHaveBeenCalledTimes(1);
      expect(native.toggleMaximize).toHaveBeenCalledTimes(1);
      expect(native.close).toHaveBeenCalledTimes(1);
    });
  });

  it("reflects initial maximization and external resize changes in the accessible label", async () => {
    native.isMaximized.mockResolvedValue(true);
    render(<NativeWindowControls locale="en-US" />);
    expect(await screen.findByRole("button", { name: "Restore window" })).toBeVisible();
    native.isMaximized.mockResolvedValue(false);
    await act(async () => native.onResized.mock.calls[0][0]());
    expect(screen.getByRole("button", { name: "Maximize window" })).toBeVisible();
  });

  it("refreshes maximization after the action even without a resize event", async () => {
    render(<NativeWindowControls locale="zh-CN" />);
    await waitFor(() => expect(native.isMaximized).toHaveBeenCalledTimes(1));
    native.isMaximized.mockResolvedValue(true);
    fireEvent.click(screen.getByRole("button", { name: "最大化窗口" }));
    expect(await screen.findByRole("button", { name: "还原窗口" })).toBeVisible();
  });

  it("shows a concise error when a window action fails and clears it on success", async () => {
    const onError = vi.fn();
    native.minimize.mockRejectedValueOnce(new Error("host failure"));
    render(<NativeWindowControls locale="en-US" onError={onError} />);
    fireEvent.click(screen.getByRole("button", { name: "Minimize window" }));
    expect(await screen.findByRole("status")).toHaveTextContent("Could not update the window.");
    expect(onError).toHaveBeenCalledWith("Could not update the window.");
    fireEvent.click(screen.getByRole("button", { name: "Minimize window" }));
    await waitFor(() => expect(screen.queryByRole("status")).not.toBeInTheDocument());
  });

  it.each(["isMaximized", "onResized"] as const)("reports %s failures without an unhandled rejection", async (method) => {
    native[method].mockRejectedValueOnce(new Error("host unavailable"));
    render(<NativeWindowControls locale="zh-CN" />);
    expect(await screen.findByRole("status")).toHaveTextContent("无法更新窗口状态。");
  });

  it("unsubscribes resize events on unmount", async () => {
    const view = render(<NativeWindowControls locale="en-US" />);
    await act(async () => undefined);
    view.unmount();
    expect(native.unlisten).toHaveBeenCalledTimes(1);
  });

  it("unsubscribes when event registration resolves after unmount", async () => {
    let registered!: (unlisten: () => void) => void;
    native.onResized.mockReturnValue(new Promise<() => void>((resolve) => { registered = resolve; }));
    const view = render(<NativeWindowControls locale="en-US" />);
    view.unmount();
    await act(async () => registered(native.unlisten));
    expect(native.unlisten).toHaveBeenCalledTimes(1);
  });

  it("ignores a late query failure after unmount", async () => {
    let rejectQuery!: (error: Error) => void;
    native.isMaximized.mockReturnValue(new Promise<boolean>((_, reject) => { rejectQuery = reject; }));
    const onError = vi.fn();
    const view = render(<NativeWindowControls locale="en-US" onError={onError} />);
    await waitFor(() => expect(native.isMaximized).toHaveBeenCalledTimes(1));
    view.unmount();
    await act(async () => rejectQuery(new Error("late failure")));
    expect(onError).not.toHaveBeenCalled();
  });

  it("hides native controls in a browser without calling the window API", () => {
    native.isTauri.mockReturnValue(false);
    render(<NativeWindowControls locale="en-US" />);
    expect(screen.queryByRole("button")).not.toBeInTheDocument();
    expect(native.getCurrentWindow).not.toHaveBeenCalled();
  });

  it("keeps macOS native frame controls without duplicating buttons", () => {
    vi.spyOn(navigator, "userAgent", "get").mockReturnValue("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)");
    render(<NativeWindowControls locale="en-US" />);
    expect(screen.queryByRole("button")).not.toBeInTheDocument();
    expect(native.getCurrentWindow).not.toHaveBeenCalled();
  });
});
