import { fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import * as api from "../../tauri-api";
import { SubscriptionResourceLink } from "./SubscriptionResourceLink";
afterEach(() => vi.restoreAllMocks());
it("opens a symbolic fixed resource instead of navigating the app webview", async () => {
  const open=vi.spyOn(api,"openSubscriptionResource").mockResolvedValue(undefined);
  render(<SubscriptionResourceLink resource="codex_login" zh={false}>Official sign-in</SubscriptionResourceLink>);
  expect(fireEvent.click(screen.getByRole("link"))).toBe(false);
  expect(open).toHaveBeenCalledWith("codex_login");
});
it("keeps a copyable official URL visible when native opening is unavailable", async () => {
  vi.spyOn(api,"openSubscriptionResource").mockRejectedValue(new Error("raw native output"));
  render(<SubscriptionResourceLink resource="go_privacy" zh={false}>Privacy</SubscriptionResourceLink>);
  fireEvent.click(screen.getByRole("link"));
  expect(await screen.findByRole("alert")).toHaveTextContent("https://opencode.ai/docs/go/#privacy");
  expect(screen.queryByText("raw native output")).not.toBeInTheDocument();
});
