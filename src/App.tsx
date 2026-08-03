import { useEffect, useMemo, useRef, useState } from "react";
import {
  Activity,
  ArrowRight,
  Bot,
  Check,
  CheckCircle2,
  CircleDot,
  Database,
  Download,
  FileSearch,
  FileText,
  FolderCog,
  KeyRound,
  LoaderCircle,
  Play,
  RotateCcw,
  Server,
  Settings2,
  ShieldCheck,
  Square,
  TerminalSquare,
  X,
} from "lucide-react";

import * as api from "./tauri-api";
import { V2PlanningView } from "./features/planning/V2PlanningView";
import { V2RunView } from "./features/runs/V2RunView";
import type {
  AnalysisPlan,
  Artifact,
  AuthenticationMethod,
  ConnectionProfile,
  ProjectSpec,
  RunCheckpoint,
  RunEvent,
  ServerInspection,
} from "./types";

type View = "connection" | "project" | "plan" | "run" | "results";

const views: Array<{ id: View; label: string; icon: typeof Server }> = [
  { id: "connection", label: "服务器连接", icon: Server },
  { id: "project", label: "远端项目", icon: FolderCog },
  { id: "plan", label: "方案确认", icon: FileSearch },
  { id: "run", label: "运行监控", icon: Activity },
  { id: "results", label: "结果", icon: Database },
];

const connectionId = "11111111-1111-4111-8111-111111111111";
const projectId = "22222222-2222-4222-8222-222222222222";

export default function App() {
  const [view, setView] = useState<View>("connection");
  const [notice, setNotice] = useState("等待建立可信连接");
  const [busy, setBusy] = useState(false);
  const [secret, setSecret] = useState("");
  const [privateKeyPath, setPrivateKeyPath] = useState("");
  const [profile, setProfile] = useState<ConnectionProfile>({
    id: connectionId,
    label: "分析服务器",
    host: "",
    port: 22,
    username: "omicsops",
    authentication: "password",
    authentication_reference: `ssh/${connectionId}`,
    host_key_fingerprint: null,
  });
  const [llm, setLlm] = useState({
    baseUrl: "https://api.openai.com/",
    model: "gpt-5.1",
    apiKey: "",
  });
  const [project, setProject] = useState<ProjectSpec>({
    id: projectId,
    connection_id: connectionId,
    remote_root: "/home/omicsops/projects/pbmc",
    plan_summary: "",
    data_sources: [],
    resource_limits: {
      max_cpu_cores: 8,
      max_memory_gib: 32,
      max_disk_gib: 200,
      max_step_seconds: 86_400,
    },
    allowed_network_domains: ["cf.10xgenomics.com"],
  });
  const [inspection, setInspection] = useState<ServerInspection | null>(null);
  const [planPath, setPlanPath] = useState("");
  const [documentText, setDocumentText] = useState("");
  const [plan, setPlan] = useState<AnalysisPlan | null>(null);
  const [runId, setRunId] = useState("RUN-2026-0001");
  const [v2RunId, setV2RunId] = useState("");
  const [runs, setRuns] = useState<RunCheckpoint[]>([]);
  const [events, setEvents] = useState<RunEvent[]>([]);
  const [artifacts, setArtifacts] = useState<Artifact[]>([]);
  const [progressDialogOpen, setProgressDialogOpen] = useState(false);
  const selectedRunId = useRef(runId);

  useEffect(() => {
    selectedRunId.current = runId;
  }, [runId]);

  useEffect(() => {
    let cleanup: () => void = () => {};
    let disposed = false;
    api.listRuns().then(async (storedRuns) => {
      if (disposed) return;
      setRuns(storedRuns);
      const selected = storedRuns.at(-1);
      if (selected) {
        selectedRunId.current = selected.run_id;
        setRunId(selected.run_id);
        const storedEvents = await api.listRunEvents(selected.run_id);
        if (!disposed) setEvents(storedEvents);
      }
    }).catch((error) => {
      if (!disposed) setNotice(error instanceof Error ? error.message : String(error));
    });
    api.listenRunEvents((event) => {
      const isInitialRun = selectedRunId.current === "RUN-2026-0001";
      const isSelectedRun = selectedRunId.current === event.run_id;
      if (isInitialRun) {
        selectedRunId.current = event.run_id;
        setRunId(event.run_id);
      }
      if (isInitialRun || isSelectedRun) {
        setEvents((current) => {
          if (current.some((item) => item.run_id === event.run_id && item.sequence === event.sequence)) {
            return current;
          }
          return [...current, event];
        });
      }
      api.listRuns().then((storedRuns) => {
        if (!disposed) setRuns(storedRuns);
      });
      if (isInitialRun || isSelectedRun) setNotice(event.reason || event.action);
    }).then((unlisten) => {
      cleanup = unlisten;
    });
    return () => {
      disposed = true;
      cleanup();
    };
  }, []);

  useEffect(() => {
    api.listRunsV2().then((storedRuns) => {
      const selected = storedRuns.at(-1);
      if (selected) {
        setV2RunId(selected.run_id);
        setRunId(selected.run_id);
      }
    }).catch(() => undefined);
  }, []);

  const activeRun = runs.find((run) => run.run_id === runId) ?? null;

  const completeViews = useMemo(
    () =>
      new Set<View>([
        ...(profile.host_key_fingerprint ? (["connection"] as View[]) : []),
        ...(inspection ? (["project"] as View[]) : []),
        ...(plan?.approved ? (["plan"] as View[]) : []),
        ...(events.some((event) => event.state === "succeeded")
          ? (["run"] as View[])
          : []),
      ]),
    [events, inspection, plan?.approved, profile.host_key_fingerprint],
  );

  const perform = async (operation: () => Promise<void>) => {
    setBusy(true);
    try {
      await operation();
    } catch (error) {
      setNotice(error instanceof Error ? error.message : String(error));
    } finally {
      setBusy(false);
    }
  };

  const saveAndTestConnection = () =>
    perform(async () => {
      const storedSecret =
        profile.authentication === "password"
          ? secret
          : JSON.stringify({ path: privateKeyPath, passphrase: secret || null });
      await api.saveConnection(profile, storedSecret);
      const result = await api.testConnection(profile.id);
      if (result.trusted) {
        setNotice("SSH 连接与 host key 已验证");
      } else {
        setNotice(`首次连接，请确认服务器指纹：${result.fingerprint}`);
        setProfile((current) => ({
          ...current,
          host_key_fingerprint: result.fingerprint,
        }));
      }
    });

  const confirmFingerprint = () =>
    perform(async () => {
      if (!profile.host_key_fingerprint) return;
      await api.confirmHostKey(profile.id, profile.host_key_fingerprint);
      setNotice("服务器指纹已写入可信配置");
    });

  const inspect = () =>
    perform(async () => {
      const result = await api.inspectProject(profile.id, project.remote_root);
      setInspection(result);
      setNotice(result.projectEmpty ? "目录可用于新项目" : "目录不为空，已阻止初始化");
    });

  const initialize = () =>
    perform(async () => {
      const result = await api.initializeProject(profile.id, project);
      setInspection(result);
      setNotice("远端控制目录、脚本与环境规范已初始化");
    });

  const selectPlan = () =>
    perform(async () => {
      const path = await api.choosePlan();
      if (!path) return;
      const extracted = await api.extractPlan(path);
      setPlanPath(path);
      setDocumentText(extracted.text);
      setProject((current) => ({ ...current, plan_summary: extracted.text.slice(0, 240) }));
      setNotice(`已提取 ${extracted.format} 方案`);
    });

  const compilePlan = () =>
    perform(async () => {
      if (!inspection) throw new Error("请先完成服务器与项目探查");
      const generated = await api.generatePlan(documentText, inspection);
      setPlan(generated);
      setNotice("Agent 已生成阶段级计划，等待批准");
    });

  const approve = () =>
    perform(async () => {
      if (!plan) return;
      const approved = await api.approvePlan(plan);
      setPlan(approved);
      setNotice("阶段目标已批准，阶段内允许自主纠错");
    });

  const start = () =>
    perform(async () => {
      if (!plan?.approved) throw new Error("分析计划尚未批准");
      const id = await api.startRun(profile.id, project, plan);
      selectedRunId.current = id;
      setRunId(id);
      setEvents([]);
      setProgressDialogOpen(true);
      const [storedRuns, storedEvents] = await Promise.all([
        api.listRuns(),
        api.listRunEvents(id),
      ]);
      setRuns(storedRuns);
      setEvents((current) => {
        const merged = [...storedEvents, ...current];
        return merged.filter((event, index) =>
          merged.findIndex((item) => item.run_id === event.run_id && item.sequence === event.sequence) === index,
        );
      });
      if (!api.isTauri()) {
        setEvents([
          demoEvent(id, 1, "run_started", "running", "开始执行远端阶段"),
          demoEvent(id, 2, "step_succeeded", "running", "公开数据下载完成"),
          demoEvent(id, 3, "step_started", "running", "Scanpy 质量控制正在运行"),
        ]);
      }
      setNotice("远端任务已启动，断开桌面不会终止运行");
      setView("run");
    });

  const selectRun = (checkpoint: RunCheckpoint) =>
    perform(async () => {
      setV2RunId("");
      selectedRunId.current = checkpoint.run_id;
      setRunId(checkpoint.run_id);
      setEvents(await api.listRunEvents(checkpoint.run_id));
      setNotice(`已载入运行 ${checkpoint.run_id}`);
    });

  const resume = () =>
    perform(async () => {
      if (!activeRun) throw new Error("没有可恢复的运行");
      setProgressDialogOpen(true);
      await api.resumeRun(activeRun.run_id);
      setNotice("已从最后一个成功步骤恢复运行");
      setRuns(await api.listRuns());
    });

  const approvePendingRun = () =>
    perform(async () => {
      const approval = activeRun?.pending_approval;
      if (!activeRun || !approval) throw new Error("没有待处理的审批请求");
      setProgressDialogOpen(true);
      await api.approveRun(activeRun.run_id, approval.id);
      setNotice("已批准当前动作并继续运行");
      setRuns(await api.listRuns());
    });

  const cancel = () =>
    perform(async () => {
      if (!activeRun) throw new Error("没有可取消的运行");
      await api.cancelRun(activeRun.run_id);
      setNotice("已请求终止当前步骤");
      setRuns(await api.listRuns());
    });

  const refreshArtifacts = () =>
    perform(async () => {
      setArtifacts(await api.listArtifacts(profile.id, project.remote_root));
      setNotice("远端产物清单和校验值已刷新");
    });

  const download = (artifact: Artifact) =>
    perform(async () => {
      const name = artifact.remote_path.split("/").at(-1) ?? "artifact";
      const path = await api.chooseDownloadPath(name);
      if (!path) return;
      await api.downloadArtifact(profile.id, artifact.remote_path, path);
      setNotice(`已校验并下载 ${name}`);
    });

  return (
    <div className="app-shell">
      <aside className="sidebar">
        <div className="brand">
          <div className="brand-mark"><Bot size={22} /></div>
          <div><strong>OmicsOps</strong><span>Autonomous Bioinformatics</span></div>
        </div>
        <nav aria-label="业务流程">
          {views.map(({ id, label, icon: Icon }, index) => (
            <button
              key={id}
              className={view === id ? "nav-item active" : "nav-item"}
              onClick={() => setView(id)}
            >
              <span className="nav-index">{completeViews.has(id) ? <Check size={14} /> : index + 1}</span>
              <Icon size={18} />
              <span>{label}</span>
            </button>
          ))}
        </nav>
        <div className="trust-box">
          <ShieldCheck size={18} />
          <div><strong>项目沙箱</strong><span>专用低权限 SSH 用户</span></div>
        </div>
      </aside>

      <main>
        <header className="topbar">
          <div>
            <span className="eyebrow">REMOTE CONTROL PLANE</span>
            <h1>{views.find((item) => item.id === view)?.label}</h1>
          </div>
          <div className="status-line"><CircleDot size={15} /><span>{notice}</span></div>
        </header>

        <section className="workspace">
          {view === "connection" && (
            <ConnectionView
              profile={profile}
              setProfile={setProfile}
              secret={secret}
              setSecret={setSecret}
              privateKeyPath={privateKeyPath}
              setPrivateKeyPath={setPrivateKeyPath}
              llm={llm}
              setLlm={setLlm}
              busy={busy}
              onTest={saveAndTestConnection}
              onConfirm={confirmFingerprint}
              onSaveLlm={() =>
                perform(async () => {
                  await api.saveLlm(llm.baseUrl, llm.model, llm.apiKey);
                  setNotice("模型端点支持流式 tool calling");
                })
              }
            />
          )}
          {view === "project" && (
            <ProjectView
              project={project}
              setProject={setProject}
              inspection={inspection}
              busy={busy}
              onInspect={inspect}
              onInitialize={initialize}
            />
          )}
          {view === "plan" && (
            <>
              <V2PlanningView
                profileId={profile.id}
                projectId={project.id}
                environmentSummary={JSON.stringify(inspection ?? {})}
                onStarted={(id) => {
                  setV2RunId(id);
                  setRunId(id);
                  selectedRunId.current = id;
                  setNotice("V2 运行已按冻结审批记录启动");
                  setView("run");
                }}
              />
              <details>
                <summary>查看旧版 Shell 计划兼容入口</summary>
                <PlanView
                  planPath={planPath}
                  documentText={documentText}
                  plan={plan}
                  busy={busy}
                  onSelect={selectPlan}
                  onCompile={compilePlan}
                  onApprove={approve}
                />
              </details>
            </>
          )}
          {view === "run" && (
            v2RunId ? <V2RunView selectedRunId={v2RunId} /> : <RunView
              runId={runId}
              runs={runs}
              activeRun={activeRun}
              events={events}
              busy={busy}
              onStart={start}
              onSelect={selectRun}
              onResume={resume}
              onApprove={approvePendingRun}
              onCancel={cancel}
              onShowProgress={() => setProgressDialogOpen(true)}
              plan={plan}
            />
          )}
          {view === "results" && (
            <ResultsView
              artifacts={artifacts}
              busy={busy}
              onRefresh={refreshArtifacts}
              onDownload={download}
            />
          )}
        </section>
      </main>
      {progressDialogOpen && (
        <ServerProgressDialog
          runId={runId}
          checkpoint={activeRun}
          plan={plan}
          events={events.filter((event) => event.run_id === runId)}
          busy={busy}
          onCancel={cancel}
          onClose={() => setProgressDialogOpen(false)}
        />
      )}
    </div>
  );
}

function ConnectionView(props: {
  profile: ConnectionProfile;
  setProfile: React.Dispatch<React.SetStateAction<ConnectionProfile>>;
  secret: string;
  setSecret: (value: string) => void;
  privateKeyPath: string;
  setPrivateKeyPath: (value: string) => void;
  llm: { baseUrl: string; model: string; apiKey: string };
  setLlm: React.Dispatch<React.SetStateAction<{ baseUrl: string; model: string; apiKey: string }>>;
  busy: boolean;
  onTest: () => void;
  onConfirm: () => void;
  onSaveLlm: () => void;
}) {
  const update = (field: keyof ConnectionProfile, value: string | number) =>
    props.setProfile((current) => ({ ...current, [field]: value }));
  return (
    <div className="two-column">
      <Panel title="SSH 连接" icon={TerminalSquare} subtitle="凭据只进入 Windows Credential Manager">
        <div className="form-grid">
          <Field label="配置名称"><input value={props.profile.label} onChange={(e) => update("label", e.target.value)} /></Field>
          <Field label="主机"><input value={props.profile.host} placeholder="server.example.org" onChange={(e) => update("host", e.target.value)} /></Field>
          <Field label="端口"><input type="number" value={props.profile.port} onChange={(e) => update("port", Number(e.target.value))} /></Field>
          <Field label="专用用户名"><input value={props.profile.username} onChange={(e) => update("username", e.target.value)} /></Field>
          <Field label="认证方式">
            <select value={props.profile.authentication} onChange={(e) => update("authentication", e.target.value as AuthenticationMethod)}>
              <option value="password">密码</option>
              <option value="private_key">SSH 私钥</option>
            </select>
          </Field>
          {props.profile.authentication === "private_key" && (
            <Field label="私钥路径"><input value={props.privateKeyPath} onChange={(e) => props.setPrivateKeyPath(e.target.value)} placeholder="C:\keys\id_ed25519" /></Field>
          )}
          <Field label={props.profile.authentication === "password" ? "密码" : "私钥口令"}>
            <input type="password" value={props.secret} onChange={(e) => props.setSecret(e.target.value)} />
          </Field>
        </div>
        <div className="actions">
          <button className="primary" disabled={props.busy} onClick={props.onTest}><KeyRound size={16} />保存并测试</button>
          {props.profile.host_key_fingerprint && (
            <button className="secondary" onClick={props.onConfirm}><ShieldCheck size={16} />确认指纹</button>
          )}
        </div>
        {props.profile.host_key_fingerprint && <code className="fingerprint">{props.profile.host_key_fingerprint}</code>}
      </Panel>
      <Panel title="模型端点" icon={Bot} subtitle="要求流式 Chat Completions 与 tool calling">
        <div className="form-stack">
          <Field label="Base URL"><input value={props.llm.baseUrl} onChange={(e) => props.setLlm((v) => ({ ...v, baseUrl: e.target.value }))} /></Field>
          <Field label="Model"><input value={props.llm.model} onChange={(e) => props.setLlm((v) => ({ ...v, model: e.target.value }))} /></Field>
          <Field label="API Key"><input type="password" value={props.llm.apiKey} onChange={(e) => props.setLlm((v) => ({ ...v, apiKey: e.target.value }))} /></Field>
        </div>
        <button className="secondary full" disabled={props.busy} onClick={props.onSaveLlm}><Settings2 size={16} />保存并探测能力</button>
      </Panel>
    </div>
  );
}

function ProjectView(props: {
  project: ProjectSpec;
  setProject: React.Dispatch<React.SetStateAction<ProjectSpec>>;
  inspection: ServerInspection | null;
  busy: boolean;
  onInspect: () => void;
  onInitialize: () => void;
}) {
  return (
    <div className="content-stack">
      <Panel title="项目根目录" icon={FolderCog} subtitle="必须为空，并由专用 SSH 用户拥有">
        <Field label="远端绝对路径">
          <input value={props.project.remote_root} onChange={(e) => props.setProject((v) => ({ ...v, remote_root: e.target.value }))} />
        </Field>
        <Field label="允许下载域名">
          <input value={props.project.allowed_network_domains.join(", ")} onChange={(e) => props.setProject((v) => ({ ...v, allowed_network_domains: e.target.value.split(",").map((item) => item.trim()).filter(Boolean) }))} />
        </Field>
        <div className="actions">
          <button className="secondary" disabled={props.busy} onClick={props.onInspect}><FileSearch size={16} />只读探查</button>
          <button className="primary" disabled={props.busy || !props.inspection?.projectEmpty} onClick={props.onInitialize}><FolderCog size={16} />初始化项目</button>
        </div>
      </Panel>
      {props.inspection && (
        <div className="metric-grid">
          <Metric label="系统" value={props.inspection.os} />
          <Metric label="CPU" value={`${props.inspection.cpuCores} cores`} />
          <Metric label="内存" value={`${Math.round(props.inspection.memoryKib / 1024 / 1024)} GiB`} />
          <Metric label="可用磁盘" value={`${Math.round(props.inspection.diskAvailableKib / 1024 / 1024)} GiB`} />
          <Metric label="环境管理器" value={props.inspection.micromamba ?? "待安装"} />
          <Metric label="目录状态" value={props.inspection.projectEmpty ? "空目录，可用" : "非空，阻止"} />
        </div>
      )}
    </div>
  );
}

function PlanView(props: {
  planPath: string;
  documentText: string;
  plan: AnalysisPlan | null;
  busy: boolean;
  onSelect: () => void;
  onCompile: () => void;
  onApprove: () => void;
}) {
  return (
    <div className="two-column plan-columns">
      <Panel title="上传方案" icon={FileText} subtitle="Markdown、DOCX 或文本型 PDF">
        <button className="drop-zone" onClick={props.onSelect}>
          <FileText size={30} />
          <strong>{props.planPath || "选择分析方案"}</strong>
          <span>扫描 PDF 暂不支持 OCR</span>
        </button>
        {props.documentText && <pre className="document-preview">{props.documentText}</pre>}
        <button className="primary full" disabled={props.busy || !props.documentText} onClick={props.onCompile}><Bot size={16} />生成阶段计划</button>
      </Panel>
      <Panel title="阶段级契约" icon={ShieldCheck} subtitle="批准后，阶段内允许 Agent 自主纠错">
        {!props.plan ? <Empty text="等待 Agent 编译方案" /> : (
          <>
            <h2 className="plan-title">{props.plan.title}</h2>
            <p className="muted">{props.plan.summary}</p>
            <ol className="stage-list">
              {props.plan.stages.map((stage, index) => (
                <li key={stage.id}><span>{index + 1}</span><div><strong>{stage.goal}</strong><small>{stage.expected_artifacts.length} 个预期产物</small></div></li>
              ))}
            </ol>
            <button className={props.plan.approved ? "approved full" : "primary full"} disabled={props.plan.approved || props.busy} onClick={props.onApprove}>
              <CheckCircle2 size={17} />{props.plan.approved ? "计划已批准" : "批准阶段目标"}
            </button>
          </>
        )}
      </Panel>
    </div>
  );
}

function RunView(props: {
  runId: string;
  runs: RunCheckpoint[];
  activeRun: RunCheckpoint | null;
  events: RunEvent[];
  busy: boolean;
  onStart: () => void;
  onSelect: (run: RunCheckpoint) => void;
  onResume: () => void;
  onApprove: () => void;
  onCancel: () => void;
  onShowProgress: () => void;
  plan: AnalysisPlan | null;
}) {
  const terminal = props.activeRun
    ? ["succeeded", "canceled"].includes(props.activeRun.state)
    : true;
  const pendingApproval = props.activeRun?.pending_approval;
  return (
    <div className="content-stack">
      <div className="run-header">
        <div>
          <span className="eyebrow">ACTIVE RUN</span>
          <h2>{props.runId}</h2>
          {props.activeRun && <span className="kind">{props.activeRun.state}</span>}
        </div>
        <div className="actions">
          <button className="primary" disabled={props.busy || !props.plan?.approved} onClick={props.onStart}><Play size={16} />启动新分析</button>
          {props.activeRun && (
            <button className="secondary" onClick={props.onShowProgress}><Activity size={16} />查看进度</button>
          )}
          <button className="secondary" disabled={props.busy || !props.activeRun || terminal || Boolean(pendingApproval)} onClick={props.onResume}><RotateCcw size={16} />恢复运行</button>
          <button className="secondary" disabled={props.busy || !props.activeRun || terminal} onClick={props.onCancel}><Square size={15} />取消运行</button>
        </div>
      </div>
      {props.runs.length > 0 && (
        <div className="run-list" aria-label="已保存运行">
          {props.runs.map((run) => (
            <button
              key={run.run_id}
              className={run.run_id === props.runId ? "secondary active" : "secondary"}
              onClick={() => props.onSelect(run)}
            >
              <code>{run.run_id.slice(0, 8)}</code>
              <span>{run.state}</span>
            </button>
          ))}
        </div>
      )}
      {pendingApproval && (
        <Panel title="需要审批" icon={ShieldCheck} subtitle="此动作超出阶段内已批准的安全边界">
          <div className="approval-card">
            <strong>{pendingApproval.reason}</strong>
            <code>{pendingApproval.proposed_action}</code>
            <p>{pendingApproval.impact}</p>
            {pendingApproval.alternatives.length > 0 && (
              <small>替代方案：{pendingApproval.alternatives.join("；")}</small>
            )}
            <div className="actions">
              <button className="primary" disabled={props.busy} onClick={props.onApprove}>
                <ShieldCheck size={16} />批准并继续
              </button>
              <button className="secondary" disabled={props.busy} onClick={props.onCancel}>
                <Square size={15} />拒绝并取消
              </button>
            </div>
          </div>
        </Panel>
      )}
      <Panel title="实时审计流" icon={Activity} subtitle="SSH 断线后从远端 PID、状态与日志继续">
        {props.events.length === 0 ? <Empty text="任务尚未启动" /> : (
          <div className="event-stream">
            {props.events.map((event) => (
              <div className="event-row" key={`${event.run_id}-${event.sequence}`}>
                <span className={`event-state ${event.state}`} />
                <code>{String(event.sequence).padStart(3, "0")}</code>
                <div><strong>{event.action}</strong><span>{event.reason}</span></div>
                <small>{event.stage_id ?? "run"}{event.attempt ? ` · repair ${event.attempt}` : ""}</small>
              </div>
            ))}
          </div>
        )}
      </Panel>
    </div>
  );
}

function ServerProgressDialog(props: {
  runId: string;
  checkpoint: RunCheckpoint | null;
  plan: AnalysisPlan | null;
  events: RunEvent[];
  busy: boolean;
  onCancel: () => void;
  onClose: () => void;
}) {
  const latestEvent = props.events.at(-1) ?? null;
  const state = latestEvent?.state ?? props.checkpoint?.state ?? "preparing";
  const terminal = ["succeeded", "failed", "canceled"].includes(state);
  const paused = state === "paused_for_approval";
  const steps = props.plan?.stages.flatMap((stage) =>
    stage.steps.map((step) => ({ stage, step })),
  ) ?? [];
  const completedSteps = new Set(
    props.events
      .filter((event) => event.action === "step_succeeded" && event.step_id)
      .map((event) => `${event.stage_id ?? ""}/${event.step_id}`),
  );
  const completedCount = steps.filter(({ stage, step }) =>
    completedSteps.has(`${stage.id}/${step.id}`),
  ).length;
  const progress = state === "succeeded"
    ? 100
    : steps.length > 0
      ? Math.round((completedCount / steps.length) * 100)
      : null;
  const currentStage = props.plan?.stages.find(
    (stage) => stage.id === latestEvent?.stage_id,
  ) ?? (props.checkpoint ? props.plan?.stages[props.checkpoint.stage_index] : undefined);
  const currentStep = currentStage?.steps.find(
    (step) => step.id === latestEvent?.step_id,
  ) ?? (currentStage && props.checkpoint ? currentStage.steps[props.checkpoint.step_index] : undefined);

  useEffect(() => {
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key === "Escape") props.onClose();
    };
    window.addEventListener("keydown", closeOnEscape);
    return () => window.removeEventListener("keydown", closeOnEscape);
  }, [props.onClose]);

  return (
    <div className="progress-dialog-backdrop" role="presentation" onMouseDown={props.onClose}>
      <section
        className="progress-dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby="server-progress-title"
        onMouseDown={(event) => event.stopPropagation()}
      >
        <header className="progress-dialog-header">
          <div className={`progress-dialog-icon ${terminal ? state : "active"}`}>
            {state === "succeeded" ? <CheckCircle2 size={22} /> : terminal ? <Square size={18} /> : <LoaderCircle size={22} />}
          </div>
          <div>
            <span className="eyebrow">REMOTE SERVER</span>
            <h2 id="server-progress-title">服务器任务进度</h2>
          </div>
          <span className={`run-state-badge ${state}`}>{runStateLabel(state)}</span>
          <button className="dialog-close" aria-label="关闭进度对话框" onClick={props.onClose}><X size={19} /></button>
        </header>

        <div className="progress-dialog-body">
          <div className="progress-summary">
            <div>
              <span>运行编号</span>
              <code>{props.runId}</code>
            </div>
            <div>
              <span>当前阶段</span>
              <strong>{currentStage?.goal ?? (terminal ? "任务已结束" : "连接服务器并准备任务")}</strong>
            </div>
            <div>
              <span>当前步骤</span>
              <strong>{currentStep?.title ?? latestEvent?.action ?? "等待服务器响应"}</strong>
            </div>
          </div>

          <div className="progress-track-heading">
            <span>{progress === null ? "服务器正在执行" : `已完成 ${completedCount} / ${steps.length} 个步骤`}</span>
            <strong>{progress === null ? (terminal ? runStateLabel(state) : "进行中") : `${progress}%`}</strong>
          </div>
          <div
            className={`progress-track ${progress === null && !terminal ? "indeterminate" : ""}`}
            role="progressbar"
            aria-label="服务器任务完成进度"
            aria-valuemin={0}
            aria-valuemax={100}
            aria-valuenow={progress ?? undefined}
          >
            <span style={progress !== null ? { width: `${progress}%` } : undefined} />
          </div>

          <div className="dialog-event-heading">
            <strong>服务器实时消息</strong>
            <span>{props.events.length} 条</span>
          </div>
          <div className="dialog-event-stream" aria-live="polite">
            {props.events.length === 0 ? (
              <div className="dialog-waiting"><LoaderCircle size={17} />正在建立 SSH 会话，等待服务器返回进度…</div>
            ) : props.events.map((event) => (
              <div className="dialog-event" key={`${event.run_id}-${event.sequence}`}>
                <span className={`event-state ${event.state}`} />
                <time>{formatEventTime(event.timestamp)}</time>
                <div><strong>{event.action}</strong><span>{event.reason}</span></div>
              </div>
            ))}
          </div>
        </div>

        <footer className="progress-dialog-footer">
          <p>{terminal ? "服务器任务已结束，可以关闭此窗口。" : paused ? "任务正在等待审批，关闭窗口不会取消服务器任务。" : "关闭窗口后任务仍会在服务器上继续运行。"}</p>
          <div className="actions">
            {!terminal && !paused && (
              <button className="secondary danger" disabled={props.busy || !props.checkpoint} onClick={props.onCancel}><Square size={15} />取消任务</button>
            )}
            <button className="primary" onClick={props.onClose}>{terminal ? "关闭" : "后台运行"}</button>
          </div>
        </footer>
      </section>
    </div>
  );
}

function ResultsView(props: { artifacts: Artifact[]; busy: boolean; onRefresh: () => void; onDownload: (artifact: Artifact) => void }) {
  return (
    <div className="content-stack">
      <div className="run-header">
        <div><span className="eyebrow">REMOTE ARTIFACT INDEX</span><h2>远端产物</h2></div>
        <button className="secondary" disabled={props.busy} onClick={props.onRefresh}><Database size={16} />刷新清单</button>
      </div>
      <Panel title="校验后的产物" icon={Database} subtitle="大文件保留远端，仅按需下载">
        {props.artifacts.length === 0 ? <Empty text="刷新以读取 results/ 产物" /> : (
          <div className="artifact-table">
            {props.artifacts.map((artifact) => (
              <div className="artifact-row" key={artifact.remote_path}>
                <FileText size={20} />
                <div><strong>{artifact.remote_path.split("/").at(-1)}</strong><span>{artifact.remote_path}</span></div>
                <code>{formatBytes(artifact.size_bytes)}</code>
                <span className="kind">{artifact.kind}</span>
                <button aria-label={`下载 ${artifact.remote_path}`} onClick={() => props.onDownload(artifact)}><Download size={16} /></button>
              </div>
            ))}
          </div>
        )}
      </Panel>
    </div>
  );
}

function Panel({ title, subtitle, icon: Icon, children }: { title: string; subtitle: string; icon: typeof Server; children: React.ReactNode }) {
  return <article className="panel"><div className="panel-heading"><Icon size={20} /><div><h2>{title}</h2><p>{subtitle}</p></div></div>{children}</article>;
}

function Field({ label, children }: { label: string; children: React.ReactNode }) {
  return <label className="field"><span>{label}</span>{children}</label>;
}

function Metric({ label, value }: { label: string; value: string }) {
  return <div className="metric"><span>{label}</span><strong>{value}</strong></div>;
}

function Empty({ text }: { text: string }) {
  return <div className="empty"><ArrowRight size={20} /><span>{text}</span></div>;
}

function demoEvent(runId: string, sequence: number, action: string, state: RunEvent["state"], reason: string): RunEvent {
  return { sequence, timestamp: new Date().toISOString(), run_id: runId, stage_id: "analysis", step_id: null, attempt: 0, action, state, log_reference: null, reason };
}

function formatBytes(value: number) {
  if (value < 1024) return `${value} B`;
  if (value < 1024 ** 2) return `${(value / 1024).toFixed(1)} KiB`;
  return `${(value / 1024 ** 2).toFixed(1)} MiB`;
}

function runStateLabel(state: RunEvent["state"]) {
  return {
    draft: "草稿",
    inspecting: "检查中",
    awaiting_plan_approval: "等待计划审批",
    preparing: "准备中",
    running: "运行中",
    paused_for_approval: "等待审批",
    succeeded: "已完成",
    failed: "失败",
    canceled: "已取消",
  }[state];
}

function formatEventTime(timestamp: string) {
  const date = new Date(timestamp);
  if (Number.isNaN(date.getTime())) return "--:--:--";
  return new Intl.DateTimeFormat("zh-CN", {
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
    hour12: false,
  }).format(date);
}
