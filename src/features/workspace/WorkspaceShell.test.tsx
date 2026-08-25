import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { WorkspaceShell } from "./WorkspaceShell";

const project = {
  id: "project-1",
  name: "PBMC 图谱",
  status: "running" as const,
  template: "single_cell_rna_seq" as const,
};

describe("WorkspaceShell", () => {
  it("renders the V4 hash-chained trajectory", () => {
    render(<WorkspaceShell project={project} locale="zh-CN" onLocaleChange={() => undefined} runStarted activeRunId="run-v4" agentRunEventsV4={[{
      schema_version: 4, run_id: "run-v4", project_id: project.id, conversation_id: "conversation-1", sequence: 1,
      occurred_at: "2026-08-17T00:00:00Z", previous_hash: "", event_hash: "a".repeat(64), event: { kind: "run_created", mode: "plan" },
    }]} />);
    expect(screen.getByText("执行过程")).toBeInTheDocument();
    expect(screen.getByText("Agent 正在处理任务")).toBeInTheDocument();
    expect(screen.getByText("规划启动")).toBeInTheDocument();
  });
  it("warns when an active run has been silent for 90 seconds without hiding stop", () => {
    const onCancel = vi.fn();
    const lastActivity = new Date(Date.now() - 90_001).toISOString();
    render(<WorkspaceShell project={project} locale="zh-CN" onLocaleChange={() => undefined} runStarted activeRunId="run-stalled" activeRunLastActivityAt={lastActivity} onCancelRun={onCancel} agentRunEventsV4={[{
      schema_version: 4, run_id: "run-stalled", project_id: project.id, conversation_id: "conversation-1", sequence: 1,
      occurred_at: lastActivity, previous_hash: "", event_hash: "a".repeat(64), event: { kind: "run_created", mode: "execute" },
    }]} />);

    expect(screen.getByRole("status")).toHaveTextContent("超过 90 秒未收到新的 Agent 事件");
    expect(screen.getByRole("button", { name: "终止运行" })).toBeInTheDocument();
  });
  it("coalesces character-sized V4 model deltas into one completed response", () => {
    const deltas = ["我", "先", "检查", "输入", "目录", "。"];
    render(<WorkspaceShell project={project} locale="zh-CN" onLocaleChange={() => undefined} runStarted activeRunId="run-v4-text" agentRunEventsV4={deltas.map((text, index) => ({
      schema_version: 4 as const, run_id: "run-v4-text", project_id: project.id, conversation_id: "conversation-1", sequence: index + 1,
      occurred_at: "2026-08-17T00:00:00Z", previous_hash: String(index), event_hash: String(index + 1), event: { kind: "model_text" as const, text },
    }))} />);
    expect(screen.getAllByRole("article", { name: "模型输出" })).toHaveLength(1);
    expect(screen.getByRole("article", { name: "模型输出" })).toHaveTextContent("我先检查输入目录。");
  });
  it("shows the V4 input question and hides the answer form after it is answered", () => {
    const onAnswer = vi.fn();
    const base = { schema_version: 4 as const, run_id: "run-question", project_id: project.id, conversation_id: "conversation-1", previous_hash: "", event_hash: "hash" };
    const requested = { ...base, sequence: 1, occurred_at: "2026-08-17T00:00:01Z", event: { kind: "input_requested" as const, question_id: "species", question: "该数据来自人还是小鼠？" } };
    const { rerender } = render(<WorkspaceShell project={project} locale="zh-CN" onLocaleChange={() => undefined} runStarted activeRunId="run-question" agentRunEventsV4={[requested]} onAnswerAgentQuestionV4={onAnswer} />);

    expect(screen.getByText("需要补充信息")).toBeInTheDocument();
    expect(screen.getByText("该数据来自人还是小鼠？")).toBeInTheDocument();
    fireEvent.change(screen.getByRole("textbox", { name: "回答 V4 问题" }), { target: { value: "人" } });
    fireEvent.click(screen.getByRole("button", { name: "回答并恢复" }));
    expect(onAnswer).toHaveBeenCalledWith("run-question", "species", "人");

    rerender(<WorkspaceShell project={project} locale="zh-CN" onLocaleChange={() => undefined} runStarted activeRunId="run-question" agentRunEventsV4={[requested,
      { ...base, sequence: 2, occurred_at: "2026-08-17T00:00:02Z", event: { kind: "user_input_answered" as const, question_id: "species", answer: "人" } },
    ]} onAnswerAgentQuestionV4={onAnswer} />);
    expect(screen.getByText("已提交回答：人")).toBeInTheDocument();
    expect(screen.queryByRole("textbox", { name: "回答 V4 问题" })).not.toBeInTheDocument();
  });
  it("restores a completed V4 run after its persisted user message", () => {
    const base = { schema_version: 4 as const, run_id: "run-history", project_id: project.id, conversation_id: "conversation-1", previous_hash: "", event_hash: "hash" };
    render(<WorkspaceShell project={project} locale="zh-CN" onLocaleChange={() => undefined}
      messages={[{ id: "message-1", role: "user", markdown: "检查矩阵", created_at: "2026-08-17T00:00:00Z" }]}
      agentRunEventsV4={[
        { ...base, sequence: 1, occurred_at: "2026-08-17T00:00:01Z", event: { kind: "run_created", mode: "execute" } },
        { ...base, sequence: 2, occurred_at: "2026-08-17T00:00:02Z", event: { kind: "model_text", text: "矩阵检查完成。" } },
        { ...base, sequence: 3, occurred_at: "2026-08-17T00:00:03Z", event: { kind: "run_completed" } },
      ]} />);
    expect(screen.getByText("检查矩阵")).toBeInTheDocument();
    expect(screen.getByText(/已完成 · 0 个步骤/)).toBeInTheDocument();
    fireEvent.click(screen.getByText("执行过程"));
    expect(screen.getByRole("article", { name: "模型输出" })).toHaveTextContent("矩阵检查完成。");
  });
  it("offers to resume a failed V4 run and treats later events as running", () => {
    const onResume = vi.fn();
    const base = { schema_version: 4 as const, run_id: "run-recover", project_id: project.id, conversation_id: "conversation-1", previous_hash: "", event_hash: "hash" };
    const failed = [
      { ...base, sequence: 1, occurred_at: "2026-08-17T00:00:01Z", event: { kind: "run_created" as const, mode: "execute" as const } },
      { ...base, sequence: 2, occurred_at: "2026-08-17T00:00:02Z", event: { kind: "run_failed" as const, message: "system environment cannot be created or changed" } },
    ];
    const { rerender } = render(<WorkspaceShell project={project} locale="zh-CN" onLocaleChange={() => undefined} agentRunEventsV4={failed} onResumeAgentRunV4={onResume} />);

    fireEvent.click(screen.getByText("执行过程"));
    fireEvent.click(screen.getByRole("button", { name: "继续运行" }));
    expect(onResume).toHaveBeenCalledWith("run-recover");

    rerender(<WorkspaceShell project={project} locale="zh-CN" onLocaleChange={() => undefined} runStarted activeRunId="run-recover" agentRunEventsV4={[...failed,
      { ...base, sequence: 3, occurred_at: "2026-08-17T00:00:03Z", event: { kind: "tool_dispatch_resolved" as const, call_id: "call-1", resolution: "side_effect_not_observed" as const, evidence: "immutable system ensure" } },
    ]} onResumeAgentRunV4={onResume} />);
    expect(screen.getByText(/运行中 · 0 个步骤/)).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "继续运行" })).not.toBeInTheDocument();
  });
  it("shows all compute choices in chat and keeps full access container-only", () => {
    const onBackendChange = vi.fn();
    const onApprovalPolicyChange = vi.fn();
    render(<WorkspaceShell project={project} locale="zh-CN" onLocaleChange={() => undefined} onSend={() => true}
      computeBackendId="local" onComputeBackendChange={onBackendChange} onApprovalPolicyChange={onApprovalPolicyChange}
      computeBackends={[
        { descriptor: { schema_version: 4, backend_id: "local", kind: "local", isolation: "process", available: true, supports_python: true, supports_r: false, supports_network_policy: false }, selectable: true, reason: null, python_status: "available", r_status: "unavailable", resolved_image_id: null },
        { descriptor: { schema_version: 4, backend_id: "ssh:server", kind: "ssh", isolation: "process", available: true, supports_python: true, supports_r: false, supports_network_policy: false }, selectable: true, reason: null, python_status: "available", r_status: "unavailable", resolved_image_id: null },
        { descriptor: { schema_version: 4, backend_id: "docker", kind: "docker", isolation: "container", available: true, supports_python: true, supports_r: true, supports_network_policy: true }, selectable: true, reason: null, python_status: "unverified", r_status: "unverified", resolved_image_id: "sha256:abc" },
        { descriptor: { schema_version: 4, backend_id: "podman", kind: "podman", isolation: "container", available: false, supports_python: false, supports_r: false, supports_network_policy: true }, selectable: false, reason: "engine unavailable", python_status: "unavailable", r_status: "unavailable", resolved_image_id: null },
      ]} />);
    fireEvent.click(screen.getByRole("button", { name: "选择计算后端" }));
    expect(screen.getByRole("region", { name: "V4 计算后端" })).toHaveTextContent("探测不会拉取镜像或启动容器");
    expect(screen.getByRole("radio", { name: /LOCAL/ })).toBeInTheDocument();
    expect(screen.getByRole("radio", { name: /SSH/ })).toBeInTheDocument();
    expect(screen.getByRole("radio", { name: /DOCKER/ })).toBeInTheDocument();
    expect(screen.getByRole("radio", { name: /PODMAN/ })).toBeDisabled();
    fireEvent.click(screen.getByRole("radio", { name: /DOCKER/ }));
    expect(onBackendChange).toHaveBeenCalledWith("docker");
    fireEvent.click(screen.getByRole("button", { name: "Agent 权限" }));
    expect(screen.getByRole("menuitemradio", { name: /^请求批准/ })).toBeInTheDocument();
    expect(screen.getByRole("menuitemradio", { name: /^帮我批准/ })).toBeInTheDocument();
    expect(screen.getByRole("menuitemradio", { name: /^完全访问权限/ })).toBeDisabled();
  });
  it("enables full access only for an available offline container", () => {
    const onApprovalPolicyChange = vi.fn();
    const onAutonomyModeChange = vi.fn();
    render(<WorkspaceShell project={project} locale="zh-CN" onLocaleChange={() => undefined} onSend={() => true}
      computeBackendId="docker" onApprovalPolicyChange={onApprovalPolicyChange} onAutonomyModeChange={onAutonomyModeChange}
      computeBackends={[{ descriptor: { schema_version: 4, backend_id: "docker", kind: "docker", isolation: "container", available: true, supports_python: true, supports_r: true, supports_network_policy: true }, selectable: true, reason: null, python_status: "unverified", r_status: "unverified", resolved_image_id: "sha256:abc" }]} />);
    fireEvent.click(screen.getByRole("button", { name: "Agent 权限" }));
    const fullAccess = screen.getByRole("menuitemradio", { name: /^完全访问权限/ });
    expect(fullAccess).toBeEnabled();
    fireEvent.click(fullAccess);
    expect(onApprovalPolicyChange).toHaveBeenCalledWith("full_access");
    expect(onAutonomyModeChange).toHaveBeenCalledWith("full_auto");
  });
  it("keeps ordinary questions in chat until the user explicitly selects Plan mode from the plus menu", async () => {
    const onSend = vi.fn().mockResolvedValue(true);
    render(<WorkspaceShell project={project} locale="zh-CN" onLocaleChange={() => undefined} onSend={onSend}
      computeBackendId="local" computeBackends={[{ descriptor: { schema_version: 4, backend_id: "local", kind: "local", isolation: "process", available: true, supports_python: true, supports_r: false, supports_network_policy: false }, selectable: true, reason: null, python_status: "available", r_status: "unavailable", resolved_image_id: null }]} />);

    fireEvent.change(screen.getByRole("textbox", { name: /描述研究目标/ }), { target: { value: "这个文件是什么格式？" } });
    fireEvent.click(screen.getByRole("button", { name: "发送" }));
    await waitFor(() => expect(onSend).toHaveBeenCalledWith("这个文件是什么格式？", "chat"));
    expect(screen.queryByRole("region", { name: "V4 计算后端" })).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "添加上下文或选择模式" }));
    fireEvent.click(screen.getByRole("menuitem", { name: /Plan 模式/ }));
    fireEvent.click(screen.getByRole("button", { name: "选择计算后端" }));
    expect(screen.getByRole("region", { name: "V4 计算后端" })).toBeInTheDocument();
    fireEvent.change(screen.getByRole("textbox", { name: /描述研究目标/ }), { target: { value: "执行完整 QC" } });
    fireEvent.click(screen.getByRole("button", { name: "发送" }));
    await waitFor(() => expect(onSend).toHaveBeenLastCalledWith("执行完整 QC", "plan"));
  });
  it("keeps projects, scientific conversation, and context visible together", () => {
    render(<WorkspaceShell project={project} locale="zh-CN" onLocaleChange={() => undefined} />);

    expect(screen.getByRole("navigation", { name: "项目与会话" })).toBeInTheDocument();
    expect(screen.getByRole("main", { name: "科研对话" })).toBeInTheDocument();
    expect(screen.getByRole("complementary", { name: "项目上下文" })).toBeInTheDocument();
    expect(screen.getAllByText("PBMC 图谱")).toHaveLength(2);
    expect(screen.getByText("Scanpy 质量控制")).toBeInTheDocument();
  });

  it("switches context tabs and expands artifact preview", () => {
    render(<WorkspaceShell project={project} locale="zh-CN" onLocaleChange={() => undefined} />);

    fireEvent.click(screen.getByRole("tab", { name: "预览" }));
    expect(screen.getAllByText("UMAP 聚类概览")).toHaveLength(2);
    fireEvent.click(screen.getByRole("button", { name: "展开预览" }));
    expect(screen.getByRole("dialog", { name: "产物预览" })).toBeInTheDocument();
  });

  it("loads the selected remote image into the preview", async () => {
    const onPreviewImage = vi.fn().mockResolvedValue({ relative_path: "results/umap.png", mime_type: "image/png", size_bytes: 1024, sha256: "a".repeat(64), data_url: "data:image/png;base64,iVBORw0KGgo=" });
    render(<WorkspaceShell project={project} locale="en-US" onLocaleChange={() => undefined} remoteFiles={[{ relative_path: "results/umap.png", directory: false, size_bytes: 1024, modified_unix_seconds: 0 }, { relative_path: "results/markers.csv", directory: false, size_bytes: 20, modified_unix_seconds: 0 }]} onPreviewImage={onPreviewImage} />);

    fireEvent.click(screen.getByRole("tab", { name: "Preview" }));
    expect(screen.getByRole("combobox", { name: "Select project image" })).toHaveValue("results/umap.png");
    fireEvent.click(screen.getByRole("button", { name: "Show image" }));
    expect(await screen.findByRole("img", { name: "results/umap.png" })).toHaveAttribute("src", "data:image/png;base64,iVBORw0KGgo=");
    expect(onPreviewImage).toHaveBeenCalledWith("results/umap.png");
    expect(screen.queryByRole("option", { name: "results/markers.csv" })).not.toBeInTheDocument();
  });

  it("renders English copy from the shared locale resource", () => {
    render(<WorkspaceShell project={project} locale="en-US" onLocaleChange={() => undefined} />);

    expect(screen.getByRole("main", { name: "Research conversation" })).toBeInTheDocument();
    expect(screen.getByRole("tab", { name: "Lab notebook" })).toBeInTheDocument();
  });

  it("sends a demo research message and exposes the dedicated Plan tab", () => {
    render(<WorkspaceShell project={project} locale="zh-CN" onLocaleChange={() => undefined} />);
    fireEvent.change(screen.getByRole("textbox", { name: /描述研究目标/ }), { target: { value: "先检查双细胞率" } });
    fireEvent.click(screen.getByRole("button", { name: "发送" }));
    expect(screen.getByText("先检查双细胞率")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("tab", { name: "Plan" }));
    expect(screen.getByText("尚未进入 Plan 模式")).toBeInTheDocument();
  });

  it("shows the real agent state instead of a fixed remote progress value", async () => {
    let finish!: (value: boolean) => void;
    const onSend = vi.fn(() => new Promise<boolean>((resolve) => { finish = resolve; }));
    const backend = { descriptor: { schema_version: 4 as const, backend_id: "local", kind: "local" as const, isolation: "process" as const, available: true, supports_python: true, supports_r: false, supports_network_policy: false }, selectable: true, reason: null, python_status: "available" as const, r_status: "unavailable" as const, resolved_image_id: null };
    const { rerender } = render(<WorkspaceShell project={project} locale="zh-CN" onLocaleChange={() => undefined} onSend={onSend} computeBackendId="local" computeBackends={[backend]} />);

    expect(screen.queryByText("65%")).not.toBeInTheDocument();
    fireEvent.change(screen.getByRole("textbox", { name: /描述研究目标/ }), { target: { value: "检查 hg19 数据" } });
    fireEvent.click(screen.getByRole("button", { name: "发送" }));
    expect(screen.getByRole("textbox", { name: /描述研究目标/ })).toHaveValue("");

    rerender(<WorkspaceShell project={project} locale="zh-CN" onLocaleChange={() => undefined} onSend={onSend} computeBackendId="local" computeBackends={[backend]} agentBusy agentNotice="503 model_not_found" />);
    expect(screen.getByRole("status")).toHaveTextContent("正在等待模型响应");
    expect(screen.getByRole("alert")).toHaveTextContent("503 model_not_found");
    finish(true);
    await waitFor(() => expect(onSend).toHaveBeenCalledWith("检查 hg19 数据", "chat"));
  });

  it("keeps the composer available with a long multiline assistant response", () => {
    const markdown = Array.from({ length: 80 }, (_, index) => `步骤 ${index + 1}\n\`gene_${index}\``).join("\n");
    render(<WorkspaceShell project={project} locale="zh-CN" onLocaleChange={() => undefined} onSend={() => true} messages={[{ id: "assistant-long", role: "assistant", markdown }]} />);

    expect(screen.getByText(/步骤 80/)).toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: /描述研究目标/ })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "发送" })).toBeInTheDocument();
  });

  it("requires an explicit decision for a pending V4 tool approval", () => {
    const decide = vi.fn();
    const request = { approval_id: "approval-1", call: { call_id: "call-1", tool_id: "runtime.execute", arguments: { code: "print(1)" } }, effect: "runtime" as const, reason: "首次代码执行需要批准", call_hash: "c".repeat(64) };
    render(<WorkspaceShell project={project} locale="zh-CN" onLocaleChange={() => undefined} runStarted activeRunId="run-approval" onCancelRun={vi.fn()} onDecideToolApprovalV4={decide} agentRunEventsV4={[{
      schema_version: 4, run_id: "run-approval", project_id: project.id, conversation_id: "conversation-1", sequence: 1, occurred_at: "2026-08-17T00:00:00Z", previous_hash: "", event_hash: "hash", event: { kind: "tool_approval_requested", request },
    }]} />);
    expect(screen.getByText(/等待工具审批 · 0 个步骤/)).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "终止运行" })).not.toBeInTheDocument();
    expect(screen.getByRole("region", { name: "工具审批" })).toHaveTextContent("runtime.execute");
    fireEvent.click(screen.getByRole("button", { name: "批准并继续" }));
    expect(decide).toHaveBeenCalledWith("run-approval", "approval-1", "c".repeat(64), "approved");
  });

  it("requires evidence before resolving an uncertain V4 dispatch", () => {
    const resolve = vi.fn();
    render(<WorkspaceShell project={project} locale="zh-CN" onLocaleChange={() => undefined} runStarted activeRunId="run-uncertain" onResolveUncertainV4={resolve} agentRunEventsV4={[{
      schema_version: 4, run_id: "run-uncertain", project_id: project.id, conversation_id: "conversation-1", sequence: 1, occurred_at: "2026-08-17T00:00:00Z", previous_hash: "", event_hash: "hash", event: { kind: "tool_dispatch_uncertain", call_id: "call-1", tool_id: "runtime.execute" },
    }]} />);
    const save = screen.getByRole("button", { name: "保存证据并继续" });
    expect(save).toBeDisabled();
    fireEvent.change(screen.getByRole("textbox", { name: "核验证据" }), { target: { value: "远端输出文件不存在" } });
    fireEvent.click(save);
    expect(resolve).toHaveBeenCalledWith("run-uncertain", "call-1", "side_effect_not_observed", "远端输出文件不存在");
  });

  it("renders user and assistant messages as safe GFM markdown", () => {
    render(<WorkspaceShell project={project} locale="en-US" onLocaleChange={() => undefined} messages={[
      { id: "user-md", role: "user", markdown: "**检查**矩阵" },
      { id: "assistant-md", role: "assistant", markdown: "## Results\n\n| gene | status |\n| --- | --- |\n| CD3D | pass |" },
    ]} />);
    expect(screen.getByText("检查")).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Results" })).toBeInTheDocument();
    expect(screen.getByRole("cell", { name: "CD3D" })).toBeInTheDocument();
  });

  it("keeps technical trajectory collapsed and omits persistence placeholder text", () => {
    const base = { schema_version: 4 as const, run_id: "run-technical", project_id: project.id, conversation_id: "conversation-1", previous_hash: "", event_hash: "hash" };
    render(<WorkspaceShell project={project} locale="zh-CN" runStarted activeRunId="run-technical" onLocaleChange={() => undefined} agentRunEventsV4={[
      { ...base, sequence: 1, occurred_at: "2026-08-17T00:00:00Z", event: { kind: "run_spec_frozen" as const, approval_hash: "a", spec_hash: "b" } },
      { ...base, sequence: 2, occurred_at: "2026-08-17T00:00:01Z", event: { kind: "tool_dispatch_started" as const, call_id: "call-1", tool_id: "runtime.execute", effect: "runtime", idempotency_key: "key" } },
    ]} />);
    expect(screen.getByText("执行过程").closest("details")).not.toHaveAttribute("open");
    expect(screen.queryByText(/状态已写入可验证事件链/)).not.toBeInTheDocument();
  });

  it("keeps agent.complete internal and shows a clean verification status", () => {
    const base = { schema_version: 4 as const, run_id: "run-completing", project_id: project.id, conversation_id: "conversation-1", previous_hash: "", event_hash: "hash" };
    render(<WorkspaceShell project={project} locale="zh-CN" runStarted activeRunId="run-completing" onLocaleChange={() => undefined} agentRunEventsV4={[
      { ...base, sequence: 1, occurred_at: "2026-08-17T00:00:00Z", event: { kind: "tool_requested" as const, call: { call_id: "complete-1", tool_id: "agent.complete", arguments: { schema_version: 4, summary: "done", answer_markdown: "## 不应显示在工具卡片中", criteria: [] } } } },
    ]} />);
    expect(screen.getByText("正在核验最终结果…")).toBeInTheDocument();
    expect(screen.getByText(/运行中 · 0 个步骤/)).toBeInTheDocument();
    expect(screen.queryByText("agent.complete")).not.toBeInTheDocument();
    expect(screen.queryByText("不应显示在工具卡片中")).not.toBeInTheDocument();
  });

  it("renders tool calls as human-readable collapsed steps", () => {
    const base = { schema_version: 4 as const, run_id: "run-readable-tool", project_id: project.id, conversation_id: "conversation-1", previous_hash: "", event_hash: "hash" };
    render(<WorkspaceShell project={project} locale="zh-CN" runStarted activeRunId="run-readable-tool" onLocaleChange={() => undefined} agentRunEventsV4={[
      { ...base, sequence: 1, occurred_at: "2026-08-17T00:00:00Z", event: { kind: "tool_requested" as const, call: { call_id: "read-1", tool_id: "project.read", arguments: { path: "results/audit.md" } } } },
      { ...base, sequence: 2, occurred_at: "2026-08-17T00:00:01Z", event: { kind: "tool_finished" as const, outcome: { call_id: "read-1", tool_id: "project.read", succeeded: true, model_content: "large internal report", data: null, provenance: [] } } },
    ]} />);
    fireEvent.click(screen.getByText("执行过程"));
    const step = screen.getByText("读取项目文件").closest("details");
    expect(step).not.toHaveAttribute("open");
    expect(step).toHaveTextContent("results/audit.md");
    expect(screen.getByText(/运行中 · 1 个步骤/)).toBeInTheDocument();
  });

  it("merges tool request, dispatch, finish, and reuse events into one detail", () => {
    const base = { schema_version: 4 as const, run_id: "run-tools", project_id: project.id, conversation_id: "conversation-1", previous_hash: "", event_hash: "hash" };
    render(<WorkspaceShell project={project} locale="zh-CN" runStarted activeRunId="run-tools" onLocaleChange={() => undefined} agentRunEventsV4={[
      { ...base, sequence: 1, occurred_at: "2026-08-17T00:00:00Z", event: { kind: "tool_requested" as const, call: { call_id: "call-1", tool_id: "runtime.execute", arguments: { code: "print(1)", secret: "hidden" } } } },
      { ...base, sequence: 2, occurred_at: "2026-08-17T00:00:01Z", event: { kind: "tool_dispatch_started" as const, call_id: "call-1", tool_id: "runtime.execute", effect: "runtime", idempotency_key: "key" } },
      { ...base, sequence: 3, occurred_at: "2026-08-17T00:00:02Z", event: { kind: "tool_finished" as const, outcome: { call_id: "call-1", tool_id: "runtime.execute", succeeded: true, model_content: "QC complete", data: null, provenance: [] } } },
      { ...base, sequence: 4, occurred_at: "2026-08-17T00:00:03Z", event: { kind: "tool_outcome_reused" as const, idempotency_key: "key", outcome: { call_id: "call-1", tool_id: "runtime.execute", succeeded: true, model_content: "QC complete", data: null, provenance: [] } } },
      { ...base, sequence: 5, occurred_at: "2026-08-17T00:00:04Z", event: { kind: "tool_requested" as const, call: { call_id: "call-2", tool_id: "runtime.execute", arguments: {} } } },
      { ...base, sequence: 6, occurred_at: "2026-08-17T00:00:05Z", event: { kind: "tool_finished" as const, outcome: { call_id: "call-2", tool_id: "runtime.execute", succeeded: true, model_content: "second result", data: null, provenance: [] } } },
      { ...base, sequence: 7, occurred_at: "2026-08-17T00:00:06Z", event: { kind: "tool_requested" as const, call: { call_id: "call-3", tool_id: "runtime.execute", arguments: {} } } },
      { ...base, sequence: 8, occurred_at: "2026-08-17T00:00:07Z", event: { kind: "tool_finished" as const, outcome: { call_id: "call-3", tool_id: "runtime.execute", succeeded: false, model_content: "failed result", data: null, provenance: [] } } },
    ]} />);
    fireEvent.click(screen.getByText("执行过程"));
    expect(screen.getAllByText(/runtime\.execute/)).toHaveLength(3);
    expect(document.querySelector(".v4-tool-status.reused")).toBeInTheDocument();
    expect(document.querySelector(".v4-tool-status.succeeded")).toBeInTheDocument();
    expect(document.querySelector(".v4-tool-status.failed")).toBeInTheDocument();
    expect(document.querySelector(".v4-tool-status.succeeded")).toBeInTheDocument();
    expect(screen.getByText("QC complete")).toBeInTheDocument();
    expect(screen.getByText(/print\(1\)/)).toBeInTheDocument();
    expect(screen.getByText(/\[REDACTED\]/)).toBeInTheDocument();
    expect(screen.queryByText(/hidden/)).not.toBeInTheDocument();
  });

  it("auto-expands blocked runs while keeping the interaction card available", () => {
    const base = { schema_version: 4 as const, run_id: "run-blocked", project_id: project.id, conversation_id: "conversation-1", previous_hash: "", event_hash: "hash" };
    const request = { approval_id: "approval-blocked", call: { call_id: "call-1", tool_id: "runtime.execute", arguments: {} }, effect: "runtime" as const, reason: "需要批准", call_hash: "c".repeat(64) };
    render(<WorkspaceShell project={project} locale="zh-CN" runStarted activeRunId="run-blocked" onLocaleChange={() => undefined} onDecideToolApprovalV4={vi.fn()} agentRunEventsV4={[{ ...base, sequence: 1, occurred_at: "2026-08-17T00:00:00Z", event: { kind: "tool_approval_requested" as const, request } }]} />);
    expect(screen.getByText("执行过程").closest("details")).toHaveAttribute("open");
    expect(screen.getByRole("button", { name: "批准并继续" })).toBeEnabled();
  });

  it("disables the composer for an active run but leaves approval controls enabled", () => {
    const base = { schema_version: 4 as const, run_id: "run-composer-lock", project_id: project.id, conversation_id: "conversation-1", previous_hash: "", event_hash: "hash" };
    const request = { approval_id: "approval-composer", call: { call_id: "call-1", tool_id: "runtime.execute", arguments: {} }, effect: "runtime" as const, reason: "需要批准", call_hash: "c".repeat(64) };
    render(<WorkspaceShell project={project} locale="zh-CN" onLocaleChange={() => undefined} onSend={vi.fn()} runStarted activeRunId="run-composer-lock" agentRunEventsV4={[{ ...base, sequence: 1, occurred_at: "2026-08-17T00:00:00Z", event: { kind: "tool_approval_requested" as const, request } }]} onDecideToolApprovalV4={vi.fn()} />);
    expect(screen.getByRole("textbox", { name: /描述研究目标/ })).toBeDisabled();
    expect(screen.getByRole("button", { name: "执行中…" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "批准并继续" })).toBeEnabled();
  });

  it("does not submit the composer twice while the first request is pending", async () => {
    let release!: (accepted: boolean) => void;
    const onSend = vi.fn(() => new Promise<boolean>((resolve) => { release = resolve; }));
    render(<WorkspaceShell project={project} locale="zh-CN" onLocaleChange={() => undefined} onSend={onSend}
      computeBackendId="local" computeBackends={[{ descriptor: { schema_version: 4, backend_id: "local", kind: "local", isolation: "process", available: true, supports_python: true, supports_r: false, supports_network_policy: false }, selectable: true, reason: null, python_status: "available", r_status: "unavailable", resolved_image_id: null }]} />);
    const composer = screen.getByRole("textbox", { name: /描述研究目标/ });
    fireEvent.change(composer, { target: { value: "检查矩阵" } });
    const send = screen.getByRole("button", { name: "发送" });
    fireEvent.click(send);
    fireEvent.click(send);
    expect(onSend).toHaveBeenCalledTimes(1);
    release(true);
    await waitFor(() => expect(composer).toBeEnabled());
  });

  it("guards plan approval and resume actions against double clicks", async () => {
    let releaseApproval!: () => void;
    const onApprovePlan = vi.fn(() => new Promise<void>((resolve) => { releaseApproval = resolve; }));
    const plan = { schema_version: 4 as const, objective: "执行 QC", steps: ["检查输入"], completion_criteria: ["报告完成"], requested_capabilities: [] };
    const { rerender } = render(<WorkspaceShell project={project} locale="zh-CN" onLocaleChange={() => undefined}
      v4Plan={{ run_id: "run-plan-guard", status: "awaiting_approval", plan, plan_hash: "plan", compute_selection: null, approval_hash: "approval" }} onApprovePlan={onApprovePlan} />);
    const approve = screen.getByRole("button", { name: "批准并运行" });
    fireEvent.click(approve);
    fireEvent.click(approve);
    expect(onApprovePlan).toHaveBeenCalledTimes(1);
    expect(approve).toBeDisabled();
    releaseApproval();
    await waitFor(() => expect(screen.getByRole("button", { name: "批准并运行" })).toBeEnabled());

    let releaseResume!: () => void;
    const onResume = vi.fn(() => new Promise<void>((resolve) => { releaseResume = resolve; }));
    const base = { schema_version: 4 as const, run_id: "run-resume-guard", project_id: project.id, conversation_id: "conversation-1", previous_hash: "", event_hash: "hash" };
    const failed = [{ ...base, sequence: 1, occurred_at: "2026-08-17T00:00:01Z", event: { kind: "run_created" as const, mode: "execute" as const } }, { ...base, sequence: 2, occurred_at: "2026-08-17T00:00:02Z", event: { kind: "run_failed" as const, message: "temporary failure" } }];
    rerender(<WorkspaceShell project={project} locale="zh-CN" onLocaleChange={() => undefined} agentRunEventsV4={failed} onResumeAgentRunV4={onResume} />);
    const resume = screen.getByRole("button", { name: "继续运行" });
    fireEvent.click(resume);
    fireEvent.click(resume);
    expect(onResume).toHaveBeenCalledTimes(1);
    expect(resume).toBeDisabled();
    releaseResume();
    await waitFor(() => expect(screen.getByRole("button", { name: "继续运行" })).toBeEnabled());
  });

  it("guards answer and uncertain-dispatch recovery actions while preserving their controls during a run", async () => {
    let releaseAnswer!: () => void;
    const onAnswer = vi.fn(() => new Promise<void>((resolve) => { releaseAnswer = resolve; }));
    const base = { schema_version: 4 as const, run_id: "run-input-guard", project_id: project.id, conversation_id: "conversation-1", previous_hash: "", event_hash: "hash" };
    const requested = { ...base, sequence: 1, occurred_at: "2026-08-17T00:00:01Z", event: { kind: "input_requested" as const, question_id: "species", question: "物种？" } };
    const { rerender } = render(<WorkspaceShell project={project} locale="zh-CN" onLocaleChange={() => undefined} runStarted activeRunId="run-input-guard" agentRunEventsV4={[requested]} onAnswerAgentQuestionV4={onAnswer} />);
    fireEvent.change(screen.getByRole("textbox", { name: "回答 V4 问题" }), { target: { value: "人" } });
    const answer = screen.getByRole("button", { name: "回答并恢复" });
    fireEvent.click(answer);
    fireEvent.click(answer);
    expect(onAnswer).toHaveBeenCalledTimes(1);
    expect(answer).toBeDisabled();
    releaseAnswer();
    await waitFor(() => expect(screen.getByRole("button", { name: "回答并恢复" })).toBeEnabled());

    let releaseResolve!: () => void;
    const onResolve = vi.fn(() => new Promise<void>((resolve) => { releaseResolve = resolve; }));
    const uncertain = { ...base, run_id: "run-uncertain-guard", sequence: 1, occurred_at: "2026-08-17T00:00:01Z", event: { kind: "tool_dispatch_uncertain" as const, call_id: "call-1", tool_id: "runtime.execute" } };
    rerender(<WorkspaceShell project={project} locale="zh-CN" onLocaleChange={() => undefined} runStarted activeRunId="run-uncertain-guard" agentRunEventsV4={[uncertain]} onResolveUncertainV4={onResolve} />);
    fireEvent.click(screen.getByText("执行过程"));
    fireEvent.change(screen.getByRole("textbox", { name: "核验证据" }), { target: { value: "已检查" } });
    const resolve = screen.getByRole("button", { name: "保存证据并继续" });
    fireEvent.click(resolve);
    fireEvent.click(resolve);
    expect(onResolve).toHaveBeenCalledTimes(1);
    expect(resolve).toBeDisabled();
    releaseResolve();
    await waitFor(() => expect(screen.getByRole("button", { name: "保存证据并继续" })).toBeEnabled());
  });
});
