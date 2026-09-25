import { afterEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { agentV4RuntimeBoundary } from "./tauri-api";
import type { RuntimeBoundaryRequestV4 } from "./types";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

const request: RuntimeBoundaryRequestV4 = { source: "frozen_run", project_id: "project-1", run_id: "run-1" };

afterEach(() => {
  Reflect.deleteProperty(window, "__TAURI_INTERNALS__");
  vi.clearAllMocks();
});

describe("runtime boundary API", () => {
  it("passes the exact source request to the native command", async () => {
    Object.defineProperty(window, "__TAURI_INTERNALS__", { value: {}, configurable: true });
    vi.mocked(invoke).mockResolvedValueOnce({ project_id: "project-1" });
    await agentV4RuntimeBoundary(request);
    expect(invoke).toHaveBeenCalledWith("agent_v4_runtime_boundary", { request });
  });

  it("rejects outside the desktop app instead of presenting a fabricated boundary", async () => {
    await expect(agentV4RuntimeBoundary(request)).rejects.toThrow("Runtime boundaries require the desktop app");
    expect(invoke).not.toHaveBeenCalled();
  });
});
