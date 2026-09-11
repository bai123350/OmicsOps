import { describe, expect, it } from "vitest";
import config from "../src-tauri/tauri.conf.json";

describe("compact desktop window", () => {
  it("opens at a compact logical size while allowing a smaller window", () => {
    const window = config.app.windows[0];
    expect(window.width).toBeLessThanOrEqual(1100);
    expect(window.height).toBeLessThanOrEqual(680);
    expect(window.minWidth).toBeLessThanOrEqual(900);
    expect(window.minHeight).toBeLessThanOrEqual(560);
    expect(window.width).toBeGreaterThanOrEqual(window.minWidth);
    expect(window.height).toBeGreaterThanOrEqual(window.minHeight);
  });
});
