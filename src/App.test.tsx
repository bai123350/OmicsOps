import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import App from "./App";

describe("OmicsOps desktop workflow", () => {
  it("presents the five business stages", () => {
    render(<App />);

    expect(screen.getByRole("button", { name: /服务器连接/ })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /远端项目/ })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /方案确认/ })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /运行监控/ })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /结果/ })).toBeInTheDocument();
  });

  it("moves between project and run views without losing task identity", () => {
    render(<App />);

    fireEvent.click(screen.getByRole("button", { name: /运行监控/ }));
    expect(screen.getByText("RUN-2026-0001")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: /结果/ }));
    expect(screen.getByText(/远端产物/)).toBeInTheDocument();
  });
});
