import { describe, expect, it } from "vitest";
import type { AgentEventKindV4, AgentRunEventV4 } from "../../types";
import { collectNotebookCells, collectDelegatedTasks, collectProvenance } from "./sidebarData";

const event = (sequence: number, payload: AgentEventKindV4, run_id = "run"): AgentRunEventV4 => ({ schema_version: 4, project_id: "p", conversation_id: "c", run_id, sequence, event: payload, occurred_at: "2026-09-11T00:00:00Z", previous_hash: "", event_hash: `hash-${sequence}` });
describe("reference sidebar data projections", () => {
  it("keeps exact CRLF message ranges and persisted tool identities for collection", () => {
    const markdown = "说明\r\n```python\r\nprint('😀')\r\n```";
    const cells = collectNotebookCells([{ id: "m", role: "assistant", markdown }], [event(9, { kind: "tool_requested", call: { call_id: "call", tool_id: "runtime.python", arguments: { code: "print(2)" } } })]);
    expect(markdown.slice(cells[0].messageRange!.start, cells[0].messageRange!.end)).toBe("print('😀')\r\n");
    expect(cells[0].messageRange?.messageId).toBe("m");
    expect(cells[1].toolSource).toMatchObject({ id: "call", kind: "tool", run_id: "run", sequence: 9, event_hash: "hash-9", conversation_id: "c" });
  });
  it("deduplicates executed assistant code but preserves repeated executions and skips data fences", () => {
    const cells = collectNotebookCells([{ id: "m", role: "assistant", markdown: "```python\nprint(1)\n```\n```csv\na,b\n```\n```r\nsummary(x)\n```" }], [
      event(1, { kind: "tool_requested", call: { call_id: "a", tool_id: "runtime.execute", arguments: { language: "python", code: "print(1)" } } }),
      event(2, { kind: "tool_finished", outcome: { call_id: "a", tool_id: "runtime.execute", succeeded: true, model_content: "1", data: null, provenance: [] } }),
      event(3, { kind: "tool_requested", call: { call_id: "b", tool_id: "runtime.execute", arguments: { language: "python", code: "print(1)" } } }),
    ]);
    expect(cells.map((cell) => cell.source)).toEqual(["summary(x)", "print(1)", "print(1)"]);
    expect(cells.map((cell) => cell.status)).toEqual(["source", "returned", "requested"]);
    expect(cells[1].output).toBe("1");
  });
  it("keeps matching call IDs isolated by run and surfaces uncertain dispatch", () => {
    const request = { kind: "tool_requested", call: { call_id: "same", tool_id: "shell", arguments: { command: "echo hi" } } } as const;
    const cells = collectNotebookCells([], [event(1, request, "a"), event(1, request, "b"), event(2, { kind: "tool_finished", outcome: { call_id: "same", tool_id: "shell", succeeded: true, model_content: "hi", data: null, provenance: [] } }, "b"), event(2, { kind: "tool_dispatch_uncertain", call_id: "same", tool_id: "shell" }, "a")]);
    expect(cells.map((cell) => cell.status)).toEqual(["uncertain", "returned"]);
  });
  it("does not treat dispatched background work as completed execution", () => {
    const cells = collectNotebookCells([], [event(1, { kind: "tool_requested", call: { call_id: "a", tool_id: "runtime.execute", arguments: { language: "python", code: "slow()", background: true } } }), event(2, { kind: "tool_finished", outcome: { call_id: "a", tool_id: "runtime.execute", succeeded: true, model_content: "submitted", data: { status: "submitted", submission_observed: true }, provenance: [] } })]);
    expect(cells[0].status).toBe("dispatched");
  });
  it.each(["unknown", "already_reserved"])("preserves %s background submission uncertainty despite a successful tool response", (status) => {
    const cells = collectNotebookCells([], [event(1, { kind: "tool_requested", call: { call_id: "a", tool_id: "runtime.execute", arguments: { language: "python", code: "slow()", background: true } } }), event(2, { kind: "tool_finished", outcome: { call_id: "a", tool_id: "runtime.execute", succeeded: true, model_content: status, data: { status, submission_observed: false }, provenance: [] } })]);
    expect(cells[0].status).toBe("uncertain");
  });
  it("projects delegation objectives and actual node outcomes", () => {
    const tasks = collectDelegatedTasks([event(1, { kind: "delegation_graph_started", call_id: "d", graph: { nodes: [{ id: "n", objective: "Check evidence", dependencies: [] }] } }), event(2, { kind: "delegation_node_finished", call_id: "d", outcome: { node_id: "n", status: "failed", error: "Missing source", output: null } })]);
    expect(tasks).toMatchObject([{ objective: "Check evidence", status: "failed", error: "Missing source" }]);
  });
  it("retains tool provenance, run, event locator and failed results", () => {
    const rows = collectProvenance([event(8, { kind: "tool_finished", outcome: { call_id: "a", tool_id: "artifact.verify", succeeded: false, model_content: "Mismatch", data: { sha256: "original-hash" }, provenance: ["artifact:one"] } })]);
    expect(rows).toMatchObject([{ runId: "run", sequence: 8, toolId: "artifact.verify", sources: ["artifact:one"], succeeded: false, data: { sha256: "original-hash" } }]);
  });
});
