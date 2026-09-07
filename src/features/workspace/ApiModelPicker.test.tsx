import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, beforeEach, vi } from "vitest";

import { listModelProfileModels } from "../../tauri-api";
import type { ModelProfile } from "../../types";
import { ApiModelPicker } from "./ApiModelPicker";

vi.mock("../../tauri-api", () => ({
  listModelProfileModels: vi.fn(),
}));

const profile = (overrides: Partial<ModelProfile> = {}): ModelProfile => ({
  id: "profile-a",
  label: "Research API",
  provider: "open_ai_compatible",
  base_url: "https://api.example.test/v1",
  model: "current-model",
  credential_reference: "credential-ref",
  supports_tools: true,
  supports_vision: false,
  ...overrides,
});

function renderPicker(overrides: Partial<React.ComponentProps<typeof ApiModelPicker>> = {}) {
  const props: React.ComponentProps<typeof ApiModelPicker> = {
    zh: false,
    profiles: [profile()],
    activeProfileId: "profile-a",
    disabled: false,
    onProfileChange: vi.fn(),
    onModelSelect: vi.fn().mockResolvedValue(undefined),
    onManage: vi.fn(),
    ...overrides,
  };
  return render(<ApiModelPicker {...props} />);
}

function openPicker() {
  fireEvent.click(screen.getByRole("button", { name: "Choose model" }));
}

function deferred<T>() {
  let resolve!: (value: T | PromiseLike<T>) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

describe("ApiModelPicker", () => {
  beforeEach(() => {
    vi.mocked(listModelProfileModels).mockReset();
  });

  it("renders every API model and filters the complete list", async () => {
    const apiModels = Array.from({ length: 25 }, (_, index) => `model-${String(index + 1).padStart(2, "0")}`);
    vi.mocked(listModelProfileModels).mockResolvedValue(apiModels);

    renderPicker();
    openPicker();

    await waitFor(() => expect(screen.getByRole("menuitemradio", { name: "model-25" })).toBeInTheDocument());
    expect(screen.getAllByRole("menuitemradio")).toHaveLength(26);
    expect(screen.getByRole("menuitemradio", { name: "model-01" })).toBeInTheDocument();
    expect(screen.getByRole("menuitemradio", { name: "model-25" })).toBeInTheDocument();

    const search = screen.getByRole("textbox", { name: "Search API models" });
    fireEvent.change(search, { target: { value: "model-25" } });

    expect(screen.getAllByRole("menuitemradio")).toHaveLength(1);
    expect(screen.getByRole("menuitemradio", { name: "model-25" })).toBeInTheDocument();
    expect(screen.queryByRole("menuitemradio", { name: "model-24" })).not.toBeInTheDocument();
  });

  it("keeps the selected model visible after a failed load and retries with a fresh list", async () => {
    vi.mocked(listModelProfileModels)
      .mockRejectedValueOnce(new Error("secret endpoint response"))
      .mockResolvedValueOnce(["new-model"]);

    renderPicker();
    openPicker();

    await waitFor(() => expect(screen.getByRole("alert")).toHaveTextContent("Could not load models"));
    expect(screen.getByRole("menuitemradio", { name: "current-model" })).toHaveAttribute("aria-checked", "true");
    expect(screen.queryByText("secret endpoint response")).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Refresh models" }));

    await waitFor(() => expect(screen.getByRole("menuitemradio", { name: "new-model" })).toBeInTheDocument());
    expect(screen.getByRole("menuitemradio", { name: "current-model" })).toHaveAttribute("aria-checked", "true");
    expect(listModelProfileModels).toHaveBeenCalledTimes(2);
  });

  it("ignores a stale result from API A after switching to API B", async () => {
    const requestA = deferred<string[]>();
    const requestB = deferred<string[]>();
    vi.mocked(listModelProfileModels).mockImplementation((id) => id === "profile-a" ? requestA.promise : requestB.promise);

    const profileB = profile({ id: "profile-b", label: "Second API", model: "b-current", base_url: "https://second.example.test/v1" });
    const onProfileChange = vi.fn();
    const { rerender } = renderPicker({ profiles: [profile(), profileB], onProfileChange });
    openPicker();

    const apiSelect = screen.getByRole("combobox", { name: "Choose API" });
    fireEvent.change(apiSelect, { target: { value: "profile-b" } });
    expect(onProfileChange).toHaveBeenCalledWith("profile-b");
    rerender(<ApiModelPicker
      zh={false}
      profiles={[profile(), profileB]}
      activeProfileId="profile-b"
      disabled={false}
      onProfileChange={onProfileChange}
      onModelSelect={vi.fn().mockResolvedValue(undefined)}
      onManage={vi.fn()}
    />);

    requestB.resolve(["b-live"]);
    await waitFor(() => expect(screen.getByRole("menuitemradio", { name: "b-live" })).toBeInTheDocument());
    requestA.resolve(["a-stale"]);
    await waitFor(() => expect(screen.queryByRole("menuitemradio", { name: "a-stale" })).not.toBeInTheDocument());
    expect(screen.queryByRole("menuitemradio", { name: "a-stale" })).not.toBeInTheDocument();
  });

  it("waits for a successful save before closing and keeps the old selection after a failed save", async () => {
    vi.mocked(listModelProfileModels).mockResolvedValue(["new-model"]);
    const save = deferred<void>();
    const onModelSelect = vi.fn().mockReturnValue(save.promise);

    const first = renderPicker({ onModelSelect });
    openPicker();
    await waitFor(() => expect(screen.getByRole("menuitemradio", { name: "new-model" })).toBeInTheDocument());

    fireEvent.click(screen.getByRole("menuitemradio", { name: "new-model" }));
    expect(onModelSelect).toHaveBeenCalledWith(expect.objectContaining({ id: "profile-a" }), "new-model");
    expect(screen.getByRole("button", { name: "Choose model" })).toHaveAttribute("aria-expanded", "true");
    expect(screen.getByRole("menuitemradio", { name: "new-model" })).toBeDisabled();

    save.resolve();
    await waitFor(() => expect(screen.getByRole("button", { name: "Choose model" })).toHaveAttribute("aria-expanded", "false"));
    first.unmount();

    const rejected = vi.fn().mockRejectedValue(new Error("secret save response"));
    const { unmount } = renderPicker({ onModelSelect: rejected });
    openPicker();
    await waitFor(() => expect(screen.getByRole("menuitemradio", { name: "new-model" })).toBeInTheDocument());
    fireEvent.click(screen.getByRole("menuitemradio", { name: "new-model" }));

    await waitFor(() => expect(screen.getByRole("alert")).toHaveTextContent("Could not switch models"));
    expect(screen.getByRole("button", { name: "Choose model" })).toHaveAttribute("aria-expanded", "true");
    expect(screen.getByRole("menuitemradio", { name: "current-model" })).toHaveAttribute("aria-checked", "true");
    expect(screen.queryByText("secret save response")).not.toBeInTheDocument();
    unmount();
  });

  it("closes on Escape immediately after opening", async () => {
    vi.mocked(listModelProfileModels).mockResolvedValue(["current-model"]);

    renderPicker();
    openPicker();
    expect(screen.getByRole("button", { name: "Choose model" })).toHaveAttribute("aria-expanded", "true");

    fireEvent.keyDown(window, { key: "Escape" });

    await waitFor(() => expect(screen.getByRole("button", { name: "Choose model" })).toHaveAttribute("aria-expanded", "false"));
    expect(screen.queryByRole("menu", { name: "API models" })).not.toBeInTheDocument();
  });
});
