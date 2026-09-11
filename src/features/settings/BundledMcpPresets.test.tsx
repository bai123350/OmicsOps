import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { SettingsPanel } from "./SettingsPanel";

const presets = [
  { id: "uniprot", name: "UniProt", description: "Protein sequences", description_zh: "蛋白质序列", tool_count: 6 },
  { id: "ensembl", name: "Ensembl", description: "Genome annotation", description_zh: "基因组注释", tool_count: 4 },
];

describe("bundled MCP presets", () => {
  it("loads and searches the host catalog and registers the chosen preset without inspecting", async () => {
    const list = vi.fn().mockResolvedValue(presets);
    const add = vi.fn().mockResolvedValue({});
    const inspect = vi.fn();
    const approve = vi.fn();
    render(<SettingsPanel locale="zh-CN" initialSection="skills" onClose={() => undefined} onListBundledMcpPresets={list} onAddBundledMcp={add} onInspectMcpServer={inspect} onSetMcpToolApproval={approve} />);
    expect(await screen.findByRole("option", { name: "UniProt · 6 个工具" })).toBeInTheDocument();
    fireEvent.change(screen.getByLabelText("搜索内置 MCP"), { target: { value: "基因组" } });
    expect(screen.queryByRole("option", { name: /UniProt/ })).not.toBeInTheDocument();
    fireEvent.change(screen.getByLabelText("选择内置 MCP"), { target: { value: "ensembl" } });
    expect(screen.getByText("基因组注释")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "添加内置 MCP" }));
    await waitFor(() => expect(add).toHaveBeenCalledWith({ preset_id: "ensembl" }));
    expect(await screen.findByRole("status")).toHaveTextContent("已添加 Ensembl");
    expect(inspect).not.toHaveBeenCalled();
    expect(approve).not.toHaveBeenCalled();
  });

  it("surfaces load and add failures and allows retry", async () => {
    const list = vi.fn().mockRejectedValueOnce(new Error("catalog unavailable")).mockResolvedValue(presets);
    const add = vi.fn().mockRejectedValueOnce(new Error("registration failed")).mockResolvedValue({});
    render(<SettingsPanel locale="en-US" initialSection="skills" onClose={() => undefined} onListBundledMcpPresets={list} onAddBundledMcp={add} />);
    expect(await screen.findByRole("alert")).toHaveTextContent("catalog unavailable");
    fireEvent.click(screen.getByRole("button", { name: "Retry catalog" }));
    await screen.findByRole("option", { name: "UniProt · 6 tools" });
    fireEvent.change(screen.getByLabelText("Select bundled MCP"), { target: { value: "uniprot" } });
    fireEvent.click(screen.getByRole("button", { name: "Add bundled MCP" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("registration failed");
    fireEvent.click(screen.getByRole("button", { name: "Add bundled MCP" }));
    expect(await screen.findByRole("status")).toHaveTextContent("Added UniProt");
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });
});
