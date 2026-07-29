import { useEffect, useMemo, useState } from "react";
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
  Play,
  Server,
  Settings2,
  ShieldCheck,
  TerminalSquare,
} from "lucide-react";

import * as api from "./tauri-api";
import type {
  AnalysisPlan,
  Artifact,
  AuthenticationMethod,
  ConnectionProfile,
  ProjectSpec,
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
  const [events, setEvents] = useState<RunEvent[]>([]);
  const [artifacts, setArtifacts] = useState<Artifact[]>([]);

  useEffect(() => {
    let cleanup: () => void = () => {};
    api.listenRunEvents((event) => {
      setEvents((current) => [...current, event]);
      setNotice(event.reason || event.action);
    }).then((unlisten) => {
      cleanup = unlisten;
    });
    return () => cleanup();
  }, []);

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
      setRunId(id);
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
            <PlanView
              planPath={planPath}
              documentText={documentText}
              plan={plan}
              busy={busy}
              onSelect={selectPlan}
              onCompile={compilePlan}
              onApprove={approve}
            />
          )}
          {view === "run" && (
            <RunView runId={runId} events={events} busy={busy} onStart={start} plan={plan} />
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

function RunView(props: { runId: string; events: RunEvent[]; busy: boolean; onStart: () => void; plan: AnalysisPlan | null }) {
  return (
    <div className="content-stack">
      <div className="run-header">
        <div><span className="eyebrow">ACTIVE RUN</span><h2>{props.runId}</h2></div>
        <button className="primary" disabled={props.busy || !props.plan?.approved} onClick={props.onStart}><Play size={16} />启动分析</button>
      </div>
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
