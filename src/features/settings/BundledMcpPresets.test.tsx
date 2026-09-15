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
    render(<SettingsPanel locale="zh-CN" initialSection="connections" onClose={() => undefined} onListBundledMcpPresets={list} onAddBundledMcp={add} onInspectMcpServer={inspect} onSetMcpToolApproval={approve} />);
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
    render(<SettingsPanel locale="en-US" initialSection="connections" onClose={() => undefined} onListBundledMcpPresets={list} onAddBundledMcp={add} />);
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

it("configures PubMed credentials inside the unified preset and clears the secret", async () => {
  const configure = vi.fn().mockResolvedValue({});
  const add = vi.fn();
  const save = vi.fn();
  render(<SettingsPanel locale="en-US" initialSection="connections" onClose={vi.fn()} onListBundledMcpPresets={async () => [{ id: "pubmed", name: "PubMed", description: "Literature", description_zh: "文献", tool_count: 3 }]} onAddBundledMcp={add} onConfigurePubMedMcp={configure} onSaveMcpServer={save} />);
  await screen.findByRole("option", { name: "PubMed · 3 tools" });
  fireEvent.change(screen.getByLabelText("Select bundled MCP"), { target: { value: "pubmed" } });
  fireEvent.click(screen.getByText("Configure credentials"));
  const key = screen.getByLabelText("NCBI API key");
  expect(key).toHaveAttribute("type", "password");
  fireEvent.change(key, { target: { value: "ncbi-secret" } });
  fireEvent.change(screen.getByLabelText("NCBI admin email"), { target: { value: "scientist@example.org" } });
  fireEvent.click(screen.getByRole("button", { name: "Save PubMed credentials" }));
  await waitFor(() => expect(configure).toHaveBeenCalledWith({ api_key: "ncbi-secret", admin_email: "scientist@example.org" }));
  await waitFor(() => expect(key).toHaveValue(""));
  expect(add).not.toHaveBeenCalled();
  expect(save).not.toHaveBeenCalled();
  expect(screen.queryByRole("button", { name: "Add PubMed MCP" })).not.toBeInTheDocument();
});

it("keeps a failed credential edit private and clears it when leaving PubMed", async () => {
  const configure = vi.fn().mockRejectedValue(new Error("credential vault unavailable"));
  const log = vi.spyOn(console, "log").mockImplementation(() => undefined);
  const warn = vi.spyOn(console, "warn").mockImplementation(() => undefined);
  const error = vi.spyOn(console, "error").mockImplementation(() => undefined);
  try {
    render(<SettingsPanel locale="en-US" initialSection="connections" onClose={vi.fn()} onListBundledMcpPresets={async () => [{ id: "pubmed", name: "PubMed", description: "Literature", description_zh: "文献", tool_count: 3 }, ...presets]} onConfigurePubMedMcp={configure} />);
    await screen.findByRole("option", { name: "PubMed · 3 tools" });
    fireEvent.change(screen.getByLabelText("Select bundled MCP"), { target: { value: "pubmed" } });
    fireEvent.click(screen.getByText("Configure credentials"));
    fireEvent.change(screen.getByLabelText("NCBI API key"), { target: { value: "retry-secret" } });
    fireEvent.click(screen.getByRole("button", { name: "Save PubMed credentials" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("credential vault unavailable");
    expect(screen.getByLabelText("NCBI API key")).toHaveValue("retry-secret");
    expect(screen.getByRole("alert")).not.toHaveTextContent("retry-secret");
    expect(JSON.stringify([...log.mock.calls, ...warn.mock.calls, ...error.mock.calls])).not.toContain("retry-secret");
    fireEvent.change(screen.getByLabelText("Select bundled MCP"), { target: { value: "uniprot" } });
    expect(screen.queryByLabelText("NCBI API key")).not.toBeInTheDocument();
    fireEvent.change(screen.getByLabelText("Select bundled MCP"), { target: { value: "pubmed" } });
    expect(screen.getByLabelText("NCBI API key")).toHaveValue("");
  } finally { log.mockRestore(); warn.mockRestore(); error.mockRestore(); }
});
