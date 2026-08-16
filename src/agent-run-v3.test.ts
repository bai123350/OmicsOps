import { describe, expect, it } from "vitest";

import { mergeAgentRunEventsV3 } from "./DesktopApp";
import type { AgentRunEventV3 } from "./types";

const event = (sequence: number, occurredAt: string): AgentRunEventV3 => ({
  run_id: "run-v3",
  project_id: "project-1",
  conversation_id: "conversation-1",
  sequence,
  previous_hash: sequence === 1 ? "0" : "a",
  event_hash: String(sequence),
  occurred_at: occurredAt,
  event: { kind: sequence === 3 ? "run_completed" : "run_started" },
});

describe("Harness v3 event replay", () => {
  it("deduplicates replay and live delivery by run and sequence", () => {
    expect(mergeAgentRunEventsV3([event(2, "2026-08-16T00:00:02Z")], [
      event(1, "2026-08-16T00:00:01Z"),
      event(2, "2026-08-16T00:00:02Z"),
      event(3, "2026-08-16T00:00:03Z"),
    ]).map((item) => item.sequence)).toEqual([1, 2, 3]);
  });
});
