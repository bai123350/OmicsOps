import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { GuidancePanel } from "./GuidancePanel";
import * as api from "../../tauri-api";
import type { AgentRunEventV4, GuidanceRecordV4 } from "../../types";

vi.mock("../../tauri-api", () => ({ agentV4ListGuidance: vi.fn(), agentV4SubmitGuidance: vi.fn(), onAgentV4Event: vi.fn() }));
const props = { runId: "run", projectId: "project", conversationId: "conversation", enabled: true, locale: "en-US" as const };
const row: GuidanceRecordV4 = { run_id:"run",project_id:"project",conversation_id:"conversation",message_id:"message",ordinal:1,markdown:"keep controls",accepted_at:"2026-09-08T00:00:00Z",consumed_at:null };
let listener: (event: AgentRunEventV4) => void;
function changed() { listener({run_id:"run",project_id:"project",conversation_id:"conversation",event:{kind:"guidance_consumed",message_id:"message",markdown:"keep controls"}} as AgentRunEventV4); }

describe("GuidancePanel", () => {
  beforeEach(() => {
    vi.mocked(api.agentV4ListGuidance).mockReset().mockResolvedValue([]);
    vi.mocked(api.agentV4SubmitGuidance).mockReset();
    vi.mocked(api.onAgentV4Event).mockReset().mockImplementation(async (callback) => {listener=callback;return vi.fn();});
  });
  it("renders received versus applied from storage and retains history when stopped", async () => {
    vi.mocked(api.agentV4ListGuidance).mockResolvedValue([row]);
    const view=render(<GuidancePanel {...props}/>);
    expect(await screen.findByText("Received, waiting to apply")).toBeInTheDocument();
    vi.mocked(api.agentV4ListGuidance).mockResolvedValue([{...row,consumed_at:"2026-09-08T00:01:00Z"}]);
    act(changed);
    expect(await screen.findByText("Applied")).toBeInTheDocument();
    view.rerender(<GuidancePanel {...props} enabled={false}/>);
    expect(screen.queryByLabelText("Additional guidance")).not.toBeInTheDocument();
    expect(screen.getByText("keep controls")).toBeInTheDocument();
  });
  it("retries an uncertain submission with the same id and preserves new draft text", async () => {
    vi.mocked(api.agentV4SubmitGuidance).mockRejectedValueOnce(new Error("secret transport detail"));
    render(<GuidancePanel {...props}/>);
    fireEvent.change(screen.getByLabelText("Additional guidance"),{target:{value:"keep controls"}});
    fireEvent.click(screen.getByRole("button",{name:"Send guidance"}));
    expect(await screen.findByRole("alert")).not.toHaveTextContent("secret");
    const first=vi.mocked(api.agentV4SubmitGuidance).mock.calls[0][0];
    vi.mocked(api.agentV4SubmitGuidance).mockImplementation(async (request) => {
      const accepted={...row,...request};vi.mocked(api.agentV4ListGuidance).mockResolvedValue([accepted]);return accepted;
    });
    fireEvent.click(screen.getByRole("button",{name:"Send guidance"}));
    await waitFor(() => expect(screen.getByLabelText("Additional guidance")).toHaveValue(""));
    expect(vi.mocked(api.agentV4SubmitGuidance).mock.calls[1][0].message_id).toBe(first.message_id);
    expect(await screen.findByText("Received, waiting to apply")).toBeInTheDocument();
  });
  it("does not clear a newly edited draft when an older uncertain send is found", async () => {
    vi.mocked(api.agentV4SubmitGuidance).mockRejectedValue(new Error("lost response"));
    render(<GuidancePanel {...props}/>);
    fireEvent.change(screen.getByLabelText("Additional guidance"),{target:{value:"keep controls"}});
    fireEvent.click(screen.getByRole("button",{name:"Send guidance"}));
    await screen.findByRole("alert");
    const request=vi.mocked(api.agentV4SubmitGuidance).mock.calls[0][0];
    fireEvent.change(screen.getByLabelText("Additional guidance"),{target:{value:"new draft"}});
    vi.mocked(api.agentV4ListGuidance).mockResolvedValue([{...row,...request}]);
    act(changed);
    await screen.findByText("keep controls");
    expect(screen.getByLabelText("Additional guidance")).toHaveValue("new draft");
  });
  it("ignores a late response after run switch and enforces UTF-8 byte limits", async () => {
    let finish!: (value: GuidanceRecordV4) => void;
    vi.mocked(api.agentV4SubmitGuidance).mockImplementation(() => new Promise((resolve)=>{finish=resolve;}));
    const view=render(<GuidancePanel {...props}/>);
    fireEvent.change(screen.getByLabelText("Additional guidance"),{target:{value:"测".repeat(683)}});
    expect(screen.getByRole("button",{name:"Send guidance"})).toBeDisabled();
    fireEvent.change(screen.getByLabelText("Additional guidance"),{target:{value:"keep controls"}});
    fireEvent.click(screen.getByRole("button",{name:"Send guidance"}));
    const request=vi.mocked(api.agentV4SubmitGuidance).mock.calls[0][0];
    view.rerender(<GuidancePanel {...props} runId="new-run"/>);
    fireEvent.change(screen.getByLabelText("Additional guidance"),{target:{value:"new run text"}});
    await act(async()=>{finish({...row,...request});});
    expect(screen.queryByText("keep controls")).not.toBeInTheDocument();
    expect(screen.getByLabelText("Additional guidance")).toHaveValue("new run text");
  });
});
