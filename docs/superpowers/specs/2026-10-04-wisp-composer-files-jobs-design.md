# Wisp 改进设计：草稿、表格与远端作业

日期：2026-10-04。状态：用户已确认设计及8515da8实施计划；W01/W02首次代码范围已实现并审阅通过，W03待实施。本文件仍定义首次行为边界，实际检查与未执行验收见总计划。总范围见 [总设计](2026-10-04-wisp-improvements-design.md)，来源见 [源码对照清单](2026-10-04-wisp-science-improvement-backlog.md)。

## 用户目标与方案选择

用户要求先保存原修改、创建分支，然后实现参考清单，并明确选择全部 12 项的首次范围。这里解决三件日常操作问题：切换会话后找回文字；查看 CSV/TSV 而不下载整个大文件；离开 Agent 后仍能查看已派发的 SSH 后台作业。

采用现有工作区与宿主接口上的独立能力。相比重新引入上游会话/运行管理器，这能保留已有审批和恢复身份；相比只增加按钮，这要求每个按钮都有真正的宿主行为、来源信息与失败回归。三项分别交付，不要求同时上线。

## W01：会话文字草稿

### 状态与接口

新增 `src/features/workspace/composerDrafts.ts` 的纯内存存储和相应 Hook。`DesktopApp` 用稳定实例持有存储，生命期为当前应用窗口；通过 Props 注入 `WorkspaceShell`。独立组件测试可提供自己的实例。不能使用模块全局单例、SQLite、localStorage 或日志保存草稿。

键为 `(project_id, conversation_id | null)`。值为 `{ text, revision }`，revision 在每次修改时增加，即使文字恰好再次相同。进入作用域立即读取对应文字；所有 textarea、引用插入、命令、分支起始文字等修改都经过同一 setter。不能只在离开时由 effect 保存旧 state，否则切换与延迟异步响应可能覆盖新作用域。

存储的契约为：

- `read(scope)` 返回该作用域当前 `{ text, revision }`，缺失返回空文字。
- `update(scope, updater)` 修改该作用域并返回新 revision。
- `capture(scope)` 返回不可变发送凭据 `{ scope, revision, text }`。
- `clearAccepted(receipt)` 仅在 scope 与 revision 均仍匹配时清空。
- `adoptUnassigned(projectId, conversationId)` 仅用于从无会话状态明确新建的会话；目标已有文字时不覆盖，也不丢弃无会话文字。
- `removeConversation(scope)` 仅在真实删除成功后清理；收起会话不调用它。

普通导航至已有会话不领走无会话文字。活跃会话中新建另一个会话得到空草稿。DesktopApp 的现有初次创建会话路径显式调用 adoption，不能根据第一次遇到某个 ID 猜测创建来源。

### 发送回执与其他材料

普通发送、入队、replacement、side chat、branch-send 与相关重试都捕获原 scope/revision。确定被宿主接受后，可以清理离屏原作用域的已提交版本；旧响应不能清空该会话后来编辑的版本，也不能清空当前其他会话。拒绝、失败或未知回执保留文字。

选中引用与附件不加入文字缓存。继续使用现有作用域重置、附件 receipt 校验、generation 与接受回执清理规则。恢复文字不恢复跨作用域材料或授权。现有 branch 起始消息逻辑改为写入已创建目标的草稿，不写入当时偶然显示的会话。

### 验收与文件地图

纯存储测试验证 A/B 往返、跨项目同名、无会话显式 adoption、重新编辑相同文字、离屏回执、失败保留与真实删除。RTL 集成测试在发送 Promise 未完成时切换并编辑，分别完成旧发送/side-chat/replacement/branch 回执，验证文字和材料作用域。测试还要离开项目再返回，覆盖 WorkspaceShell 卸载。

产品文件：`src/DesktopApp.tsx`、`WorkspaceShell.tsx`、新增 `composerDrafts.ts` 与 Hook；测试放在同目录并更新 `WorkspaceShell.test.tsx`、composer integration 和 DesktopApp 回归。无需 DTO、数据库迁移或桌面执行命令。

## W02：有界 CSV/TSV 表格预览

### 用户入口与范围

文件树对 `.csv`/`.tsv` 提供只读表格预览，沿用 `WorkspaceFileReference` 的 project/backend/相对路径；其他格式沿用既有入口。弹层显示来源、文件字节数、实际读取字节、展示行数、hash scope 与截断原因。表格使用 React 文本节点，不能执行 HTML、公式、脚本或自动请求外部资源。第一行作为表头显示，同时保留它是文件首行的说明。

首版支持 UTF-8（含 BOM）、逗号或制表符、LF/CRLF、双引号转义及引号内分隔符/换行。不猜测编码、分隔符或 locale；不提供编辑、筛选整文件、PDF、Office 或 Notebook。预览不将表格自动发送给模型，也不从截断单元格创建 ComposerQuote。既有完整文本 quote 命令与其完整文件摘要语义保持独立。

### 读取与 DTO

新增 `preview_project_table(request)`，request 为 `{ project_id, backend_id, relative_path }`；宿主根据扩展名选择分隔符。DTO 位于 `omicsops-dto`，宿主重导出并在 `dto_contract_tests.rs` 校验，前端类型与调用遵循现有边界。

`ProjectTablePreview` 包含：来源身份；`size_bytes`（远端不可确认时 nullable）；`read_bytes`；`parsed_bytes`；`sha256`；`hash_scope: full | prefix`；`headers`；`rows`；`displayed_rows`；`truncation_reasons`；`source_stability: checked | unknown`。truncation reason 使用固定枚举，不把异常、服务器输出或绝对路径放进 DTO。

预算由宿主固定：读取前缀最多 256 KiB，额外探测 1 字节判断 EOF；展示最多 200 个数据记录，最多 64 列；单元格展示最多 4 KiB UTF-8。列数超限返回明确限制错误，超长单元格只截断显示并标记。sha256 覆盖实际返回的原始前缀字节，不包含额外探测字节；仅确认 EOF 且读取稳定时标为 full。BOM、未显示记录和 UTF-8 半字符仍计入原始字节摘要。

新增纯解析器，以有界原始输入为参数，不读文件。它按字段状态解析完整逻辑记录，不按物理行拆分。达到字节上限时丢弃不完整末尾记录；缺失的 UTF-8 尾部字节可以作为前缀截断处理，输入中间非法编码必须报错。完整文件中未闭合引号、闭合引号后非法字符或不一致列数报固定格式错误。前缀末尾未闭合字段只算截断；已观察到的非法语法仍报错。行数限制可停止呈现，不能声称未解析尾部的格式已验证。

### 本地共享文件检查接缝

W02/W10/W11 都需要打开用户显式选择的项目文件。新增聚焦的 `src-tauri/src/project_file_read.rs`；不拆分无关模块，也不给纯 DTO 或 core 引入 Win32。

`ValidatedProjectFile::open(project, relative_path)` 验证路径并打开普通文件；`metadata()` 返回有界公共元数据；`read_prefix(max_bytes)` 与 `read_complete(max_bytes)` 在同一已验证 File handle 上读取。宿主内部 `BoundedFileRead` 保存 bytes、size/read bytes、EOF、sha256/hash scope；raw bytes 不直接成为公共 DTO。

拒绝绝对路径、驱动器、UNC、设备路径、ADS、遍历、Windows 保留名、结尾空格/点、内部目录及非普通文件。复用现有相对路径规则，并检查全部祖先和目标的 symlink，以及 Windows `FILE_ATTRIBUTE_REPARSE_POINT`。Windows 使用现有 `windows-sys` 文件系统能力核对已打开 handle 的最终路径与文件身份、项目根 containment；读取前后核对身份、长度与修改元数据，检测到变化时拒绝结果。这里提供受控文件读取，不宣称敌意并发写入下的原子快照或 OS 沙盒。

W10 完整小文件读取受同一预算控制。W11 大文件只读取 metadata 与已有可信摘要，不为了迁移预览扫描巨大文件；缺少摘要明确显示 unknown。

### SSH 读取

验证当前项目连接、可信 fingerprint 和 canonical remote root/path，拒绝远端 symlink 祖先及逃逸。在 adapters SSH 中新增有界前缀读取能力，复用 SFTP，不能把现有 `read_file_limited` 的完整文本读取改为悄悄截断。远端 open 后读取最多预算+1；尽可能用同一远端 handle 的 metadata 在前后核对 size/mtime，不足以确认时报告 stability unknown。即使 EOF 已观察到，无法确认稳定性时也不能把摘要宣传为已校验的完整源文件摘要。

提供可注入的 table source seam，模拟本地/SSH 读取和 metadata；自动测试不连接真实主机。整个远端预览路径（连接、根/文件检查、metadata、读取）固定最长等待 20 秒，关闭弹层或切作用域使响应失效；正常disconnect计入同一20秒deadline；超时返回后只做一次有界session disconnect，不延长公开等待，失败时真实关闭状态未知，不表示远端计算取消。不产生同步任务。

### 验收与文件地图

解析测试覆盖 BOM、中文、CRLF、引号内换行/分隔符、转义、空字段、空文件、末行无换行、非法 UTF-8、半字符/半记录前缀、列数和行/单元格限制。临时目录及注入路径检查覆盖 Windows reparse/junction、symlink、内部目录、ADS/保留名、文件变更和源范围逃逸。SSH fake 验证读取预算、full/prefix 区分和源变化。

UI 测试覆盖 loading/error/retry、身份显示、截断提示、XSS 文本、切来源过期响应、打开后立即 Escape，以及嵌套弹层一次只关顶层。需要文件：DTO 新表格模块；`composer-file-api.ts`/`types.ts`；`project_file_read.rs`；宿主 table command/parser；`omicsops-adapters/src/ssh.rs`；文件树/独立 TablePreviewDialog 与测试；`src-tauri/src/lib.rs` 注册命令。不新增持久化表。

## W03：SSH 后台作业列表与查询

### 列表与身份

新增独立 RemoteJobsPanel，入口位于工作区执行环境/RuntimeDialog，支持项目列表和当前会话筛选。列表只读取 SQLite，最多返回最近 100 项 detached SSH 作业；显示 job/run/call ID、语言/环境、持久化状态、可查询性和限制。不能把持久化 running/reserved 当作远端进程正在存活。

新增 `list_remote_jobs_v4({ project_id, conversation_id? })` 和 `refresh_remote_job_v4({ project_id, job_id })`。Store 列表接缝读取该项目 jobs 并校验 job/context/run 的所有权；完整 frozen root/fingerprint 留在宿主用于校验，公共 DTO 只提供可安全显示的来源身份、原 request hash 与固定可查询性原因，不返回代码、凭据或原始 RuntimeResult JSON。无时间戳的旧 job 不推断时间。

列表显示旧连接/旧根的作业，但标记不能在当前绑定查询。单项刷新必须先从 Store 找到原记录和 frozen run：不能接收任意 backend/root/fingerprint，也不能由 UI 提供要执行的代码或 SSH 命令。

### 宿主刷新边界

刷新为明确用户触发的有界只读状态检查。宿主核对项目当前 connection/backend、已确认 host key、原 frozen job/run/context/request hash 与连接实际 fingerprint；连接后 canonicalize 当前项目 root，必须精确匹配原 frozen root。缺失/变更绑定时拒绝，不替用户重新绑定，也不使用旧凭据自动连接其他主机。

复用 `remote_jobs_v4::query` 和既有 `Transport` seam，仅发送 `action: status` 与原 identity。保留原 receipt/result digest 校验、CAS 终态记录和 `resubmitted: false`。不调用 submit，不释放 job reservation，不因未知响应重跑，不直接恢复 Agent，不把观察结果升级为科研 evidence。

共享completed-result校验须在Store CAS前拒绝敏感样式文本及不安全引用；现有query会先保存typed RuntimeResult，再返回给公开投影，不能仅脱敏UI。检查全部将持久化的字符串/结构敏感标签（不只4KiB片段），仍使用原始可接受结果及摘要；拒绝时保持旧状态/receipt，固定安全错误，不把改写后的结果挂在旧stream/result hash下。模式检测不声称能识别任意无标签秘密。

整个远端刷新路径（SSH 连接、身份检查、根路径解析、状态查询）固定最长等待 20 秒，不能只给 `transport.invoke` 加 timeout；response 沿用已有字节/产物数预算。输出投影只返回脱敏 stdout/stderr 各最多 4 KiB 的观察片段、固定 status/error category、host observed_at 和有界捕获引用。断连、未知 receipt 或超时不改记为 failed/cancelled；保留旧记录并说明远端情况未知。已经终态且有已校验 result 的作业可从 ledger 查看，无需再次联系 SSH。

不增加远端取消命令。停止查询只停止本地等待，不确认远端计算终止。UI 要分别显示 recorded_state 与 remote_observation，避免混淆。当前运行的冻结审批不因面板操作改变，已派发作业始终沿用原身份。

### 受控退避

手动刷新始终是默认。用户在面板内明确启用某一 job 的临时自动查询后，采用串行 2/4/8/16/30 秒退避；一次窗口最多 10 次查询，无并发请求，也不扫全项目。失败保持退避，不无限重连。终态、次数用尽、面板关闭、离开工作区/切项目或会话、绑定变化均停止调度；应用重启不自动恢复查询。可见性 shelving 不会取消远端作业。

再次手动刷新或重新启用可以建立新窗口；同一刷新按钮不会叠加 scheduler。UI generation 拦截晚到响应和 timer，fake timers 验证退避与停止。远端 status 只读请求即使已发出，也不能被描述为已取消远端 job。

### 验收与文件地图

Store/host 测试使用原 job fixture 与 FakeTransport：项目/会话越权拒绝、metadata list 不调用 transport、绑定/fingerprint/root/request 不符拒绝、丢失派发 ACK 后只 status、unknown 不重跑、终态 receipt 不一致拒绝、并发查询 CAS、重启后原 job 查询、timeout/错误脱敏和 bounded logs。UI 覆盖立即 Escape、记录状态与观察状态区分、手动刷新、opt-in 退避、关闭/切作用域/终态停止与过期响应。

文件：`omicsops-store/src/runtime_jobs.rs` 聚焦列表/查找；DTO 新 remote jobs 模块及合同测试；`src-tauri/src/remote_job_commands.rs`；复用 `remote_jobs_v4.rs`；`src-tauri/src/lib.rs`；前端调用和独立面板/Hook；RuntimeDialog/WorkspaceShell 接入。同步更新 README、执行边界说明和手工 smoke 文档中的“无自动轮询”：只提供面板内用户启用的有限查询，仍无远端取消/跨重启自动轮询。

## 共同验证与限制

逐项先运行对应纯函数、Store/宿主与 RTL 回归，再运行项目规定的完整 Rust、前端和 Web build。W02/W03 的桌面命令组合与 Windows API 改动还需 desktop build。真实 SSH、真实科学数据、模型 key 不作为自动测试前提；真实 SSH 验收另按 acceptance 文档显式执行并记录，未执行就不能声称通过。三项不改变模型目录匹配、本地 system PATH 解释器、凭据存储与冻结审批规则。
