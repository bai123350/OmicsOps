import type { AgentRunEventV4 } from "../../types";

// Read-only projections of the host's existing transcript/events. These views
// never dispatch work or promote generated source to verified execution.
export interface NotebookCell {
  id: string;
  language: string;
  source: string;
  output: string;
  origin: "assistant" | "runtime" | "shell";
  status: "source" | "requested" | "returned" | "failed" | "uncertain" | "dispatched";
  runId?: string;
}
type Message = { id: string; role: string; markdown: string };
const record = (value: unknown): Record<string, unknown> => value !== null && typeof value === "object" && !Array.isArray(value) ? value as Record<string, unknown> : {};
const string = (value: unknown) => typeof value === "string" ? value : "";
const callKey = (run: string, call: string) => JSON.stringify([run, call]);
export const isSidebarPreviewImage = (path: string) => /\.(png|jpe?g|gif|webp|bmp)$/i.test(path);

export function collectNotebookCells(messages: Message[], events: AgentRunEventV4[]): NotebookCell[] {
  const executed = new Map<string, NotebookCell>();
  const background = new Set<string>();
  for (const { run_id, event } of events) {
    if (event.kind === "tool_requested") {
      const { call } = event;
      const args = record(call.arguments);
      const runtime = ["runtime.execute", "runtime.python", "runtime.r"].includes(call.tool_id);
      const shell = call.tool_id === "shell";
      if (!runtime && !shell) continue;
      const source = string(shell ? args.command : args.code);
      if (!source.trim()) continue;
      const id = callKey(run_id, call.call_id);
      if (args.background === true) background.add(id);
      executed.set(id, { id, language: shell ? "shell" : string(args.language) || (call.tool_id === "runtime.r" ? "r" : "python"), source, output: "", origin: shell ? "shell" : "runtime", status: "requested", runId: run_id });
    } else if (event.kind === "tool_finished" || event.kind === "tool_outcome_reused") {
      const id = callKey(run_id, event.outcome.call_id);
      const cell = executed.get(id);
      if (cell) {
        cell.output = event.outcome.model_content;
        const data = record(event.outcome.data);
        const confirmedSubmission = data.status === "submitted" && data.submission_observed === true;
        cell.status = !event.outcome.succeeded ? "failed" : background.has(id) ? (confirmedSubmission ? "dispatched" : "uncertain") : "returned";
      }
    } else if (event.kind === "tool_dispatch_uncertain") {
      const cell = executed.get(callKey(run_id, event.call_id));
      if (cell) cell.status = "uncertain";
    }
  }
  const cells: NotebookCell[] = [];
  const normalizeLanguage = (language: string) => ["bash", "sh", "powershell", "pwsh", "shell"].includes(language.toLowerCase()) ? "shell" : language.toLowerCase();
  const executedSource = new Set([...executed.values()].map((cell) => `${normalizeLanguage(cell.language)}\n${cell.source.trim()}`));
  for (const message of messages) {
    if (message.role !== "assistant") continue;
    const lines = message.markdown.split(/\r?\n/);
    for (let i = 0; i < lines.length; i++) {
      const start = /^ {0,3}(`{3,}|~{3,})([^\s`]*)[^\r\n]*$/.exec(lines[i]);
      if (!start) continue;
      const language = start[2].toLowerCase() || "text";
      const end = new RegExp(`^ {0,3}${start[1][0]}{${start[1].length},}\\s*$`);
      const source: string[] = [];
      while (++i < lines.length && !end.test(lines[i])) source.push(lines[i]);
      if (i === lines.length) break; // Streaming, incomplete fences are not cells yet.
      const code = source.join("\n");
      if (["csv", "tsv", "fasta", "fa"].includes(language) || !code.trim() || executedSource.has(`${normalizeLanguage(language)}\n${code.trim()}`)) continue;
      cells.push({ id: `${message.id}:${i}`, language, source: code, output: "", origin: "assistant", status: "source" });
    }
  }
  return [...cells, ...executed.values()];
}

export interface DelegatedTask { id: string; nodeId: string; runId: string; objective: string; dependencies: string[]; status: string; error: string; output: string }
export function collectDelegatedTasks(events: AgentRunEventV4[]): DelegatedTask[] {
  const tasks = new Map<string, DelegatedTask>();
  for (const { run_id, event } of events) {
    if (event.kind === "delegation_graph_started") {
      const nodes = record(event.graph).nodes;
      if (!Array.isArray(nodes)) continue;
      for (const raw of nodes) {
        const node = record(raw);
        if (!string(node.id)) continue;
        const id = JSON.stringify([run_id, event.call_id, node.id]);
        tasks.set(id, { id, nodeId: string(node.id), runId: run_id, objective: string(node.objective), dependencies: Array.isArray(node.dependencies) ? node.dependencies.filter((v): v is string => typeof v === "string") : [], status: "pending", error: "", output: "" });
      }
    } else if (event.kind === "delegation_node_finished" || event.kind === "delegation_graph_finished") {
      const outcomes = event.kind === "delegation_node_finished" ? [event.outcome] : Object.values(record(record(event.outcome).nodes));
      for (const raw of outcomes) {
        const outcome = record(raw);
        const task = tasks.get(JSON.stringify([run_id, event.call_id, outcome.node_id]));
        if (!task) continue;
        task.status = string(outcome.status) || "unknown";
        task.error = string(outcome.error);
        task.output = outcome.output == null ? "" : typeof outcome.output === "string" ? outcome.output : JSON.stringify(outcome.output, null, 2);
      }
    }
  }
  return [...tasks.values()];
}

export function collectProvenance(events: AgentRunEventV4[]) {
  return events.flatMap(({ run_id, sequence, occurred_at, event, event_hash }) => event.kind === "tool_finished" || event.kind === "tool_outcome_reused" ? [{
    id: `${run_id}:${sequence}`, runId: run_id, sequence, occurredAt: occurred_at, eventHash: event_hash,
    callId: event.outcome.call_id, toolId: event.outcome.tool_id, succeeded: event.outcome.succeeded,
    output: event.outcome.model_content, data: event.outcome.data, sources: event.outcome.provenance, reused: event.kind === "tool_outcome_reused",
  }] : []);
}
