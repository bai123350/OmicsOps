# Wisp 首次交付设计：轨迹诊断、离线示例与 Skill helper 回归

日期：2026-10-04。状态：用户已确认设计，实施计划待审阅，尚未实现。

本文细化 [Wisp 改进 backlog](2026-10-04-wisp-science-improvement-backlog.md) 中 W06、W07 与 W08 的 Python helper/CI 部分。三个章节各自可交付；本文是行为和接口设计，不是实施计划。W08 的 V4 场景 manifest、runner 和报告契约由 [Runtime 设计](2026-10-04-wisp-runtime-design.md) 定义，本文只规定两种离线检查的 CI 边界。

上游参考固定在 `xuzhougeng/wisp-science@2c1ad2e1ee2e600be22c08f826490b1c627c71b4`。采用其问题和验收思路，不重复导入已经捆绑的 Skills，不执行上游代码。第一批范围不包含 PDF/Office 解析、远程取消或独立探索执行。实现后的默认检查仍为 `cargo test --workspace`、`npm test`、`npm run build`；桌面组合与资源改动还需 `npm run build:desktop`。真实模型、SSH、R 和 PBMC 验收仍独立遵循 [acceptance/README.md](../../../acceptance/README.md)。

## W06：只读轨迹诊断与安全 HTML/JSON 导出

### 目标与当前接缝

用户可按 run、工具和失败筛选当前对话的轨迹，定位一个精确的 `(run_id, sequence, event_hash)`，将同一份公开诊断投影保存为 HTML 或 JSON。诊断展示已有事件和观察，不新增模型用量采集或计时系统，也不改变执行、审批或恢复状态。

当前 [WorkspaceShell](../../../src/features/workspace/WorkspaceShell.tsx) 的 `V4RunTrace` 已合并工具调用、模型请求和公开进度，诊断折叠区只列事件 sequence/kind/time。`mergeV4ToolCalls` 与 `toolMetrics` 是现有显示接缝；新增筛选不得让重复 `call_id` 跨 run 合并。[Store](../../../crates/omicsops-store/src/lib.rs) 的 `agent_events_v4` / `agent_events_for_context_v4` 已通过 `deserialize_event_chain_v4` 验证整条链，[protocol](../../../crates/omicsops-protocol/src/lib.rs) 的 `validate_event_chain_v4` 验证事件哈希、sequence、前驱和终态位置。

[agent_v4.rs](../../../src-tauri/src/agent_v4.rs) 的现有事件查询命令先执行 terminal reconciliation，可能追加事件。因此本项通过新的宿主只读命令直接访问 Store，不调用这些查询命令，不触发恢复、终态修补、模型或工具。

### 推荐方案与取舍

在工作区增加一个诊断对话框，宿主返回已经过白名单投影和脱敏的行。UI 筛选和导出共用这份投影规则；既有 run 轨迹继续用于精确定位。导出由宿主重新读取受限定作用域的完整链，验证后生成文件。客户端提交查询与预览 head，不提交原始事件、HTML、JSON 或 base64 字节。

只扩充现有会话导出的文件保存叶子接缝：让 [conversation_export.rs](../../../src-tauri/src/conversation_export.rs) 的扩展名检查和同目录临时文件写入可被宿主诊断模块调用。既有 HTML/PNG 请求和解码规则保持原契约。采用单独的诊断 DTO 与格式枚举，不增加通用导出注册表、serializer 框架或任意内容导出接口。

选择安全元数据投影，代价是第一版不会包含工具输出、代码或详细 provider/SSH 错误。它足以定位失败和源事件；若需要原始证据，用户仍在原项目按权限查看。前端直接序列化完整事件虽简单，但容易包含私有 replay、工具内容和跨作用域数据，且无法证明客户端提供的是当前持久化链，故不采用。

### 读取与过滤契约

宿主先确认 project 存在、conversation 属于该 project。Store 增加专用的 `agent_events_for_context_v4_bounded(project_id, conversation_id, max_events, max_serialized_bytes)` 只读接口：在同一 SQLite 读事务中预检 event 数与 `value_json` UTF-8 总字节，读取作用域内所有 run 的完整链，复用现有反序列化/链验证，并检查索引的 run/project/conversation 与每个事件 envelope、所属 run 的持久化作用域一致。现有无限定调用方无需改变，无迁移。

固定上限为整个 conversation 的 10,000 个事件、16 MiB 原始序列化事件和 8 MiB 公开投影；先验证完整链，再过滤。超过限额整体失败，不按过滤条件绕过完整验证，不截断后宣称链完整。工具筛选输入至多 256 UTF-8 字节；自定义脱敏词至多 32 个、每个 1 KiB、总计 16 KiB，空词拒绝。

run 筛选是 UUID 精确匹配，工具筛选是原始 registry tool ID 精确匹配，不接受正则或搜索任意 payload。工具关联键始终是 `(run_id, call_id)`。失败筛选包括 `RunFailed`、`ModelRetrying`、`ToolFinished/ToolOutcomeReused` 的 `succeeded=false`，以及 `ToolDispatchUncertain`；uncertain 单独标为“结果未确定”，不标为已失败或已取消。取消单独展示，用户主动取消不算失败。工具筛选只保留能明确归属该工具的事件；run 失败若无法归属工具，不混入该工具的结果。

事件排序先按该 run 首事件的 `occurred_at`，再按 run UUID、sequence；不依赖数据库返回或渲染顺序。每行通过白名单生成固定事件 kind、状态、可定位 ID、UTC 时间和可用的工具标识。缺少关联 start、负时间差、重复/歧义 dispatch 或复用旧 outcome 时，工具耗时为 `null`，显示 unknown，不用零补齐。用量复用 `UsageTotalsV4::from_observations`；额外注明已持久化 request attempt 中没有 observation 的数量，已有 known subtotal 保留 incomplete 信息。没有 observation 显示 unknown，不把本地估计当作 provider 实测值。

### DTO 与持久化

Rust 类型加入 `crates/omicsops-dto/src/run_diagnostics.rs` 并由 crate 导出；通过现有 `src-tauri/src/dto.rs` re-export。`src/types.ts` 与 `dto_contract_tests.rs` 同步字段/枚举，跨边界不复制另一套 Rust DTO。请求使用 `deny_unknown_fields`，序列化枚举使用 snake_case。

| 类型 | 字段与语义 |
| --- | --- |
| `RunDiagnosticsFilterV4` | `run_id: Option<Uuid>`、`tool_id: Option<String>`、`failures_only: bool`。只用于宿主匹配。 |
| `RunDiagnosticsRequestV4` | `project_id`、`conversation_id`、`filter`、`redaction_keywords: Vec<String>`。脱敏词仅存在于本次请求内存。 |
| `RunDiagnosticsFilterSummaryV4` | `run_id`、`tool_label: Option<String>`、`failures_only`。tool_label 为脱敏后显示值，不能用于再次查询。 |
| `RunDiagnosticsSourceHeadV4` | `run_id`、`event_count: u64`、`last_sequence: u64`、`event_hash: String`。每个实际有事件的 run 一个 head。 |
| `RunDiagnosticEventV4` | `run_id`、`sequence`、`event_hash`、`previous_hash`、`occurred_at`、`event_kind: String`、`status: Option<RunDiagnosticStatusV4>`、`tool_label: Option<String>`、`call_ref: Option<String>`、`duration_ms: Option<u64>`。没有原始 event/payload 字段。 |
| `RunDiagnosticStatusV4` | `succeeded`、`failed`、`uncertain`、`cancelled`；无可解释状态的事件为 `null`。 |
| `RunDiagnosticUsageV4` | `run_id`、`totals: UsageTotalsV4`、`missing_observation_attempts: u32`。只含数值与完整性限定，不含 provider 请求。 |
| `RunDiagnosticsSnapshotV4` | `schema_version: 1`、project/conversation UUID、`filter_summary`、`source_heads`、`source_chain_validated: true`、`total_event_count`、`selected_event_count`、`filtered: bool`、`events`、`usage`。usage 是选中 run 的整 run 小计，明确不随工具/失败筛选重算。 |
| `RunDiagnosticsExportFormatV4` | `html`、`json`。 |
| `SaveRunDiagnosticsRequestV4` | `query: RunDiagnosticsRequestV4`、`expected_heads: Vec<RunDiagnosticsSourceHeadV4>`、`format`。不接受任意输出字节。 |

命令为 `get_run_diagnostics_v4(request) -> Result<RunDiagnosticsSnapshotV4, String>` 与 `save_run_diagnostics_v4(request) -> Result<Option<String>, String>`。后者返回实际成功保存的路径；取消是 `Ok(None)`。固定错误码由 UI 本地化，不将 SQL、provider、SSH 或文件系统原始错误传回诊断内容。

不新增 SQLite 表或事件。筛选、临时预览和脱敏词不写入 SQLite、日志、导出或项目 manifest。`source_heads` 包含该对话读取快照的全部有事件 run，按 run UUID 排序；导出重新读取的 heads 必须与预览完全一致，否则返回 `diagnostics_snapshot_changed`，保留预览并要求刷新。过滤/脱敏词改变时 UI 丢弃旧预览；无法以旧 heads 和新 UI 参数静默导出。

保存使用一次经过验证的不可变快照；native dialog 等待期间新追加的事件不使该快照伪装成“最新”，文件明确写出 source head/截点。这是对所选快照的导出，不承诺点击保存到最终落盘之间的全局数据库锁。

### 安全投影与文件格式

只读源在宿主内验证后，逐字段构建输出。字符串复用 [composer_references::public_text](../../../src-tauri/src/composer_references.rs)，它在 core API key/Bearer 规则外还处理 token/password/Authorization 赋值和完整或不完整 PEM 私钥块。诊断自定义关键词仅针对 `public_text` 的原文本做非递归 literal 匹配，相交命中合并遮盖；新增的 `[REDACTED]` 不再作为下一轮输入。完整命中判断先于 UTF-8 安全限长，tool_label 最多 256 字节，不能先截断原秘密再遮盖。`public_text` 接缝也供 publication report 使用，双方不用各自实现更弱的凭据规则；诊断自定义关键词规则仍仅属本次请求。

任意自由文本不进入第一版投影：排除 `ModelText` 内容、`ModelReplayRecorded` 内容、continuation/thinking、工具 arguments、`model_content`、`data`、provenance 字符串、plan 代码、用户输入、公开进度正文以及原始异常。上述事件可以有仅含固定 kind 和定位引用的行。`call_ref` 为对 `(run UUID, 原 call_id)` 的定长 SHA-256 引用，原 call_id 不导出。tool_label 经 public_text 和自定义词处理且限长；hash 字段必须是经链验证的 64 位十六进制值，首事件的 previous_hash 可以为空。无任意原始 JSON 透传字段。

JSON 文件保留该 DTO 的全部版本化诊断投影字段与 `schema_version`，另有固定文件专用 `export_notes`，不改变查询/预览 DTO；HTML 由宿主对所有可变文本进行转义，使用静态 CSS 和当前会话导出的严格 CSP：`default-src 'none'; style-src 'unsafe-inline'; base-uri 'none'; form-action 'none'`。不嵌入脚本、外部链接、图片、字体、工具文件或可执行 Markdown。两种格式均显示筛选条件、原/选中事件数、源 heads 截点、用量 unknown/incomplete 和“公开诊断投影，非完整科研证据”；文件专用说明也明确投影不能重算原事件哈希或作为可 replay 原始审计链。

`event_hash` 是已验证原始事件的定位引用；删除敏感 payload 后的投影不能重算原事件哈希，文件不声称自身是可 replay 的原始审计链。自定义脱敏词列表不导出。生成 bytes 在 native Save dialog 前完成，限定 8 MiB；路径扩展名匹配格式，同目录临时文件写入并 sync 后原子替换，失败不返回成功。保存路径仅供本地确认，不作为诊断 manifest 内容。

### UI、定位与失败语义

保存只接受宿主选择器返回的原路径：扩展名按格式大小写不敏感匹配 `.json` 或
`.html`，缺失或错误时在写入前返回固定安全错误，不追加或重写扩展名。选择
无扩展名路径不会覆盖旁边同名的带后缀文件；取消不写文件。既有会话 HTML/PNG
保存采用相同边界，保持原序列化和显式合法目标（2026-10-19 最终审阅修正）。

运行诊断从运行轨迹对话框中的入口打开；会话标题栏不显示该按钮（2026-10-21 界面调整）。

`RunDiagnosticsDialog` 通过 `src/run-diagnostics-api.ts` 调宿主命令；browser 开发态用受控测试 mock，不把 browser 下载视为宿主已校验导出。工作区作用域变化使未完成查询过期；晚响应不得填入另一 project/conversation。保存进行时禁止重复提交，取消或失败保留筛选/预览并显示对应状态。

点击定位向 WorkspaceShell 发出 `{run_id, sequence, event_hash}`，展开对应 run 与诊断折叠区，滚动并 focus 精确行。该区覆盖所有事件 kind，包括只有元数据的 ModelText 行，不能因现有 UI 排除 ModelText 而丢失定位目标。行标记用这三个字段，不只按 call_id、index 或 sequence 查找。源行暂时不在当前缓存时使用新的只读诊断命令刷新后再查，不调用带 reconciliation 的旧命令；hash 不一致或仍缺失则明确“源事件已不可定位”，不得选择相邻行替代。`ActivityWindow` 的裁剪不用于这个定位目标。对话框进入窗口级 Escape 堆栈，打开后立即 Escape 可关闭顶层，一次按键不关闭父级覆盖层。

| 失败条件 | 用户可观察行为 |
| --- | --- |
| project/conversation 不存在或归属不符 | 整体拒绝，不泄露另一作用域是否有事件。 |
| envelope/索引归属错误、断链、hash 或终态位置错误 | 不显示/导出部分链，不继续过滤；固定 `diagnostics_source_invalid`。 |
| 上限、请求字段或过滤值不合法 | 固定错误，不将截断结果标为完整。 |
| 源 heads 与预览不同 | 刷新提示；不弹保存成功。 |
| 保存取消、扩展名错误或写入失败 | 分别为取消或失败，均无成功 toast。 |
| 无事件或筛选无匹配 | 正常空状态，保留源/筛选计数，不推断执行成功。 |

### 测试验收与文件地图

用有效 hash 链 fixtures 测 Store bound/scope 验证、归属 payload 注入、断链、hash 修改和终态异常；过滤前仍必须验证未选中 run。纯投影测试覆盖两个 run 的同名 call_id、失败/uncertain/取消区别、稳定排序、无 start/反向时间/复用结果的 null 耗时、缺失 observation 和 incomplete subtotal。

安全测试在所有被排除 payload 及 tool 字符串中放置 API key、password、token、Authorization、私钥、关键词、HTML 属性/脚本、provider/SSH 错误与私有推理哨兵，确认 JSON/HTML 均不出现哨兵、脱敏词列表或原始 payload。快照 head 变化测试必须失败；空快照须正确限定语义。保存叶子使用临时目录和 fake chooser 覆盖取消、扩展名、写入失败与 exact bytes。UI mock 覆盖 run/tool/失败筛选、精确三元组定位、作用域晚响应、立即 Escape、保存取消和重复提交。无真实模型/SSH/网络。

| 文件 | 职责 |
| --- | --- |
| `crates/omicsops-dto/src/run_diagnostics.rs`、`src/lib.rs` | 新请求/公开投影/格式 DTO。 |
| `crates/omicsops-store/src/lib.rs` | bounded 只读读取与索引/envelope/run 归属校验，复用完整链验证，无迁移。 |
| `src-tauri/src/run_diagnostics.rs` | 作用域校验、过滤、白名单投影、source head guard、HTML/JSON 生成和命令。 |
| `src-tauri/src/conversation_export.rs` | 复用小型保存叶子函数；原会话格式契约维持。 |
| `src-tauri/src/lib.rs`、`dto_contract_tests.rs` | 命令注册与严格序列化契约。 |
| `src/types.ts`、`src/run-diagnostics-api.ts` | TS 类型与宿主调用。 |
| `src/features/workspace/RunDiagnosticsDialog.tsx` 及测试、`WorkspaceShell.tsx` 及测试 | 筛选、预览、保存状态与精确行定位，复用 Escape 接缝。 |

## W07：一个双语、可离线打开的合成示例工作区

### 目标与推荐方案

给首次使用者一个不需要 key、网络、SSH、Python 或 R 的明确入口，展示“来源 → 结果表 → 报告”。示例是随包附带的合成内容，首次创建时生成新的本地目录和普通 Project 记录；查看由独立示例数据模型/视图承载。它不进入生产消息、Run、审批、Evidence、Verification 或事件表。

当前 [ProjectTemplate](../../../crates/omicsops-core/src/workspace.rs) 已有四种模板，工作流配方已有科学说明；本项保持 enum 和项目类型不变。新 Project 使用 `Blank`、`Ready`，`connection_id/remote_root=None`。[ProjectLibrary](../../../src/features/projects/ProjectLibrary.tsx) 的普通创建入口可能默认远端，因此新增独立 local-only 示例入口，不复用该默认选择。[DesktopApp](../../../src/DesktopApp.tsx) 对普通 selected project 会初始化对话与运行上下文，示例必须在设置普通 selected project 前识别并路由到独立视图。

上游 [seed.rs](https://github.com/xuzhougeng/wisp-science/blob/2c1ad2e1ee2e600be22c08f826490b1c627c71b4/src-tauri/src/seed.rs#L584-L640) 提供独立项目和文件的示例，但 OmicsOps 第一版只维护一个固定、原创的小型合成样例，不导入上游 transcript、thinking 或科学样本，也不提供 tar 解包和通用示例导入。使用编译期 `include_str!` 的受控 fixture，避免首次打开依赖下载或可缺失的资源目录。

### 示例内容与来源语义

固定 `example_id="synthetic-evidence-report-v1"`。原始表有 A 组 2、4 和 B 组 5、7，结果表写 A 均值 3、B 均值 6、差值 3。两份报告只是已写好的算术演示，不宣称统计显著性、真实实验或当前宿主已执行计算。

生成文件为 `data/observations.csv`、`results/summary.csv`、`report.zh.md`、`report.en.md`、`README.md`、`SOURCE.json`、`LICENSE.txt`。README 与两份报告给出中英文“合成数据，预置结果，未在本机执行”说明。SOURCE 记录本仓库原创来源、example ID、fixture schema/version 和文件 hash 范围（列其余文件的 hash，不包含 SOURCE 自身，避免循环）；使用仓库 `Cargo.toml` 声明的 `AGPL-3.0-only`，复制现有 LICENSE 正文，不更改许可文本。收据再记录包括 SOURCE 在内的全部固定文件 hash。源码中的例子 manifest 与许可说明随打包资源一并可用。

独立视图包含目标、合成表、预置结果和报告四段，以及可点击的文本文件预览。记录的叙述项使用 `question | narration | result`，不使用生产 `WorkspaceMessage` 的角色。所有时间和来源描述指向“预置示例”，没有 run ID、宿主审批、execution receipt、verified 标记或当前执行耗时。哈希仅证明示例文件与 fixture 是否相同，不证明科学结论或复现成功。

### DTO、持久化与创建边界

新 DTO 在 `crates/omicsops-dto/src/offline_example_workspace.rs`，仅含 serde 数据，不依赖 core Project、Tauri 或数据库。创建返回项目 ID 与独立 view；随后通过现有 listProjects 更新项目列表，不在 DTO crate 手工复制 Project。

| 类型 | 字段与语义 |
| --- | --- |
| `CreateOfflineExampleWorkspaceRequest` | `request_id: Uuid`、`example_id: String`、`parent_directory: String`。只接受固定 example ID。 |
| `OfflineExampleText` | `zh: String`、`en: String`，两种语言都有内容，无在线翻译。 |
| `OfflineExampleEntry` | `id: String`、`kind: question/narration/result`、`text: OfflineExampleText`；不具有执行语义。 |
| `OfflineExampleFileView` | `relative_path`、`media_type: text/plain或text/csv或text/markdown或application/json`、`expected_sha256`、`current_sha256: Option<String>`、`availability: available/changed/missing/unavailable`、`preview_text: Option<String>`。预览是有限、已脱敏的当前文本。 |
| `OfflineExampleWorkspaceView` | `schema_version: 1`、`project_id`、`example_id`、`source_kind: synthetic`、`availability: available/unavailable`、`unavailable_reason: Option<OfflineExampleUnavailableReason>`、`title`、`disclaimer`、`source_description`（均双语）、`license_spdx`、`fixture_sha256`、`entries`、`files`。没有生产 run/evidence 字段。 |
| `OfflineExampleUnavailableReason` | `receipt_missing`、`receipt_invalid`、`root_unavailable`、`identity_mismatch`；固定枚举，无原始路径/错误。 |
| `CreateOfflineExampleWorkspaceResult` | `project_id`、`workspace: OfflineExampleWorkspaceView`。 |

命令为 `create_offline_example_workspace(request) -> Result<CreateOfflineExampleWorkspaceResult, String>`，以及只读 `get_offline_example_workspace(project_id) -> Result<Option<OfflineExampleWorkspaceView>, String>`。请求拒绝未知字段。识别首先查 Store 安全身份标记，而不是依赖可删除的磁盘收据：无标记的普通项目返回 `None`，不因缺文件而判为示例；有标记则只返回 `Some(view)` 或明确错误，永不返回 None。收据缺失/损坏、根不可访问、项目/根归属不符时返回 `Some` 的 unavailable 来源视图，entries/files 为空且无内容读取。标记格式损坏或未知 example ID 则固定错误并阻断普通初始化。普通项目中用户自行放入同名收据不会自动获得示例身份。

持久化复用现有 Project/`project.json`，额外写该新目录内 `.omicsops/offline-example.json` 收据，字段为 schema_version、project_id、request_id、example_id、fixture_sha256、固定文件清单及各 expected_sha256。收据描述内容来源，不授权执行。既有 SQLite `app_objects` 保存一个独立安全身份记录：`kind="offline_example_workspace_v1"`、`id=project UUID`，值为内部 `OfflineExampleProjectMarkerV1 {schema_version:1, project_id, request_id, example_id, fixture_sha256, root_binding_sha256}`。root_binding 是创建时 project UUID 与规范化最终 local_root 字符串的 SHA-256；用于检测绑定变化，不作为权限或科学证明。没有文件内容、凭据、运行或科研结论。不新增表、迁移、conversation 或 ProjectTemplate。

Store 提供 `save_offline_example_project(&Project, &OfflineExampleProjectMarkerV1)`，在同一个 `BEGIN IMMEDIATE` 中复用 `validate_project_input`/`insert_project` 并保存固定 kind/id 标记；不能先 `save_project` 再单独 `put_json`。内部 marker 类型可定义于 Store，不跨 UI，无需产品 DTO。新项目/marker 必须同成同败；已有普通 ID/root 冲突拒绝，已有完全相同 marker/Project 返回原记录，不重置用户 metadata。`get_offline_example_project_marker(project_id)` 通过现有 get_json 机制只读；marker project ID 与查询 ID 精确相等。`delete_project` 同事务清理该固定 kind/id，仍不删除用户文件。只有显式创建/相同请求恢复登记能写标记，普通 save_project 或文件命名不能清除身份，get 时的 root 不符只产生 unavailable。

用户选择的是已有本地父目录；宿主 canonicalize 并验证后生成唯一直接子目录 `omicsops-example-{request_id}`。parent 可以是本地盘路径；第一版拒绝 UNC、设备命名空间与任何 symlink/junction/reparse component，Windows 检查不能仅依赖 Unix symlink 判定。child、staging、`.omicsops` 和所有文件的最终绝对路径必须处于该已验证 parent 下。固定 fixture 只允许声明的相对路径，拒绝绝对路径、盘符、UNC、`..`、空 segment、非规范分隔和额外文件；不解包外来 archive。

fixture manifest 上限 256 KiB，每个输出文件最多 64 KiB，总计 512 KiB、最多 12 个。LICENSE.txt 也计入这些限制。预览限于固定清单中的 UTF-8 文本，通过 W02 的 `ValidatedProjectFile::open` / `read_complete(64 KiB)` 读取并验证同一 handle 的最终路径、身份和前后元数据，不复制 Win32 读取实现，不接受客户端任意路径。此复用只依赖本地文件叶子接口，不依赖表格 UI 或 SSH。内部收据由本模块针对固定 `.omicsops/offline-example.json` 做同等受限定的 handle 检查与 64 KiB 完整读取；不能放宽共享接缝对任意内部目录的拒绝。变更过大的文件标为 unavailable，不能静默截断后标为 available。多文件顺序读取不声称同一时刻的原子快照。

创建先在已验证 parent 的唯一临时子目录用 `create_new` 写入所有文件、project manifest 与收据，再发布为唯一 child，最后在 Store 原子登记 Project 与 marker。目标已存在且不含完全一致的有效收据/manifest 时拒绝，不合并、不覆盖用户目录。Windows 发布必须保持“不覆盖既存目标”语义，并在最终发布前重查目标路径类型与 parent；不调用能覆盖既存目录的通用移动命令。

相同 request_id 的重试只有在已发布目录的 project/receipt 归属、fixture hash、文件 bytes 与 expected hashes 都相同，且 Store 中没有冲突 ID/root/marker 时才能原子恢复登记或返回同一项目。创建成功后再次打开只读现有文件；新 request_id 明确创建另一个项目。若用户已修改示例文件，重试创建返回冲突，正常打开则保留修改并标 changed。marker 和收据的来源身份均不得以自动修复名义重写。

### 离线 UI 与失败语义

ProjectLibrary 新增 `onCreateOfflineExample` 与独立“离线示例 / Offline example”动作，创建覆盖层采用窗口 Escape 堆栈。DesktopApp 统一异步 `openProjectWorkspace(project, navigation?)` 选择入口：启动首项目/上次恢复、ProjectLibrary 打开、普通/示例创建后、精确来源跳转和跨项目导航都先调用只读识别命令。`Some(view)` 清除普通 selected project 并进入 `offlineExampleView`；只有 `None` 的普通分支可 setSelected 并应用 requestedConversation/run/source。识别失败保持受控错误，不 fallback。不能保留启动 `setSelected(items[0])` 或来源路径里的直接新项目赋值。示例 navigation 不创建或打开生产 conversation，目标不可用明确提示。

请求 token/generation 用于抑制晚响应和启动默认选择与显式导航竞态，只有最后有效的打开请求提交状态。loading 状态在身份识别完成后结束；等待期间 selected 为空，不能先选择项目后补查来源。创建成功同样走统一入口或使用其同一已核验身份分支。示例状态不得触发原 selected-project effect 创建 conversation、选择 model/compute、连接 SSH 或发送 Agent 请求；后续当前项目元数据刷新也不能绕过标记检查切换到生产视图。

2026-10-11 实施边界澄清：上述等待期间 selected 为空适用于启动、项目库及没有已识别当前项目的入口。可取消的来源/用量导航可以在候选 marker 查询期间保留先前已由 None 确认的普通当前项目，显示“正在识别项目来源”，不能提前选择或初始化候选。None 才提交普通目标；Some 清除旧 selected 并打开独立示例；错误不回退到候选。取消、后续导航及 generation 失效保留原视图、草稿和已有记录，晚响应无效；尚未识别的项目不能作为保留当前项目。代价是原工作区仍可见，其先前授权工作可继续，因此只保证候选零初始化，不声明全应用零网络。全局设置目录的本地 model/Skills/MCP/连接元数据读取可保留，不启动 provider、MCP、SSH 或 compute。

ProjectLibrary 可提供窄生命周期回调 `onBeginOfflineExample(): void`，每次显式打开示例创建面板重置 DesktopApp 的创建尝试。必须保持 `onCreateOfflineExample(parentDirectory)` 签名；同一次打开内失败/重试保持 request UUID，关闭后重开获得新的 UUID，选择器取消不创建。此回调仅区分 UI 创建尝试，不改变宿主 DTO、授权或恢复登记规则。

`OfflineExampleWorkspace` 只提供本地预览与返回项目库。源/结果/报告都有明显 synthetic banner，changed/missing/unavailable 与 available 分别显示；changed 文件预览的是用户当前文本，不能继续将其称为随包原始结果。整个 view 为 unavailable 时保留合成身份/banner 和固定原因，禁止文件预览与生产会话初始化。Markdown 预览不加载图片、远程链接、脚本、iframe 或外部字体，文本经过 host public_text；无需 Markdown 打开网络才能看完示例。

| 失败条件 | 用户可观察行为与留存 |
| --- | --- |
| parent/fixture 路径不安全、目标冲突或 fixture 越限 | 不创建项目，不修改既有文件。 |
| staging 写入失败 | 不返回 Project；仅清理本请求已验证、拥有的 staging。 |
| final 已发布而 Store 原子登记失败 | 明确 `example_registration_failed`，Project 与 marker 均未新增或改写；保留带有效收据的新目录，相同 request_id 可恢复登记，不自动删除用户可能开始编辑的目录。 |
| 已登记 marker 存在，收据缺失/损坏、根不可访问或 project 归属不符 | 保持示例身份，返回 unavailable 独立视图，禁止生产初始化。 |
| 文件 changed/missing/unavailable | 正常打开来源视图并标明该状态，不覆盖、补写或宣称示例完整。 |
| 未知 example ID | 明确拒绝；不加载服务器或用户动态模板。 |

创建成功只表示示例文件已落盘且 Project 已登记。查看与重开不执行计算，不自动修复内容。文件路径和 OS 原始错误不写入可分享的示例源说明；UI 用固定错误码本地化必要的操作反馈。

### 测试验收与文件地图

宿主临时目录/临时 Store 测试覆盖完整创建、相同请求幂等、不同请求独立目录、重复打开零写入、路径越界、预存目标/额外文件冲突、符号链接与 Windows reparse/junction、fixture 大小/路径错误、staging 写入失败、登记失败后恢复和用户修改后的冲突。事务任一步错误须证明 Project/marker 零半成品；marker 冲突不覆盖，删除 Project 同事务清理 marker。重开 DB 后仍识别示例；删除/损坏收据或改 root 时仍为 Some(unavailable)；无 marker 的普通项目即使缺文件也为 None。测试检查无 conversation、Run、Agent event、approval、Evidence 或执行记录新增。重开 changed/missing 文件必须保留用户修改并返回正确 availability。

UI 使用 mock Tauri 命令验证中文/英文都有预置内容、四段来源链、明确 synthetic 标记、文件选择与安全文本预览、立即 Escape、scope 晚响应和创建取消。启动第一项目为示例、精确来源跳转、恢复导航、删除收据后重启与身份识别失败分别验证零 createConversation/Agent/SSH 调用；所有新项目选择必须经统一入口，显式导航不能被晚到启动默认选择覆盖。对 model、Agent send、SSH 和 network 接缝设置会失败的 mock，查看与创建不得调用。算术表与报告的固定数值使用 fixture 内容断言，不依赖 Python/R/scientific libraries。桌面构建检查 fixture 与 LICENSE 在发行构建中可读，但构建成功不视为真实科学端到端验收。

| 文件 | 职责 |
| --- | --- |
| `examples/offline/synthetic-evidence-report-v1.json`、`examples/offline/README.md` | 单个固定 fixture、双语内容、来源/许可和 hash 约定；不保存真实样本。 |
| `crates/omicsops-dto/src/offline_example_workspace.rs`、`src/lib.rs` | 独立示例 DTO。 |
| `src-tauri/src/offline_example_workspace.rs`、`project_file_read.rs`（W02） | 编译 fixture、受限定创建/收据/打开与恢复登记；预览复用有界本地读取。 |
| `crates/omicsops-store/src/lib.rs` | 专用 Project/marker 原子登记、只读身份查询与项目删除清理，复用既有 app_objects，无迁移。 |
| `src-tauri/src/lib.rs`、`dto_contract_tests.rs` | 命令注册与 source_kind/availability/严格请求契约。 |
| `src/types.ts`、`src/offline-example-api.ts` | 对应 TS 数据与 invoke 包装。 |
| `src/features/projects/OfflineExampleWorkspace.tsx` 及测试 | 专用来源/文件预览视图，无生产运行入口。 |
| `ProjectLibrary.tsx`、`DesktopApp.tsx` 及测试 | local-only 显式入口；启动/恢复/创建/精确来源导航统一身份识别路由。 |
| `docs/bundled-science.md`、`acceptance/README.md` | 示例来源与离线 smoke 记录；helper/example 不等于实工作流验收。 |

## W08：已导入 Skill helper 的行为回归与共同 CI 边界

### 目标、现状与推荐方案

第一版让 Windows CI 自动验证已经捆绑的 `paper-narrative/runtime.py`、`literature-review/runtime.py` 与 `figure-style/runtime.py` 的关键 helper 行为。只用 Python 标准库 `unittest` 和替身，不安装 matplotlib/Pillow 或科学依赖，不调用真实 HTTP、模型、SSH、GPU 或 R。

[skill_commands.rs](../../../src-tauri/src/skill_commands.rs) 已检查 26 个 Wisp 包的资源完整性、hash 和启停/冻结；这些检查不证明 Python helper 的行为。当前 [CI](../../../.github/workflows/ci.yml) 运行 Windows、Node 22、npm ci、Rust workspace 测试、前端测试和 Web build；[scripts/README.md](../../../scripts/README.md) 已记录 Python unittest discovery，但 CI 尚未执行它。分析工作流指南、文献/论文 helper 已与固定上游归一化核对一致，本项不修改 Skills、SOURCE pin 或冻结内容。

新增 `scripts/test_bundled_skill_helpers.py`，从仓库根下三个固定 allowlisted 路径读取源，使用 `exec(compile(source, path, "exec"), namespace)` 载入隔离测试 namespace，匹配 helper 被显式加载的形态。每个 fixture 使用新 namespace，临时目录与 mock 生命周期严格清理；测试不执行用户安装或动态选择的 Skill。缺失/损坏源直接失败，不 skip。复用 stdlib unittest discovery，不增加 CLI crate、npm script 依赖或第二套科学测试基础设施。

上游 [test_skill_sidecars.py](https://github.com/xuzhougeng/wisp-science/blob/2c1ad2e1ee2e600be22c08f826490b1c627c71b4/python/test_skill_sidecars.py#L34-L265) 提供边界例子。采用本地实际 helper 签名与断言，不直接把上游整套测试/依赖搬入仓库。将加载无副作用、检索失败和数据绑定问题作为验收，不以机器耗时阈值代替行为检查。

### 独立测试契约

| helper | 固定行为验收 |
| --- | --- |
| `paper-narrative` | `_paper_figure_claims_json` 与 `paper_brief_task` 保留 claim 和同条 `composite_path` 的配对；中文、引号、Windows 路径可 JSON round-trip；显式 null 保留，缺失 path 不补造；输入对象不被改写。 |
| `literature-review` 检索 | OpenAlex 成功零结果返回空列表，成功有结果正常映射；HTTP 401/404/429/500、超时/URL error、坏 JSON、错误 results 类型不能冒充零结果。429 只重试一次，sleep 被 mock。向 API URL 放置假 key/contact 哨兵，RuntimeError 及完整 traceback 不泄露 URL、原异常或哨兵。 |
| `literature-review` 引文/DOI | backward 与 forward 扩展所需检索都成功后才允许空结果；任一方向失败整体失败，两种方向不能交换。CrossRef/doi HEAD 的连接失败保留 unknown/未核验，不能将注册不确定映射为 verified。仅显式 OPENALEX fixture key 可进入 OpenAlex 请求，不进入其他服务。 |
| `figure-style` 加载 | 没有 matplotlib/Pillow 仍可定义 helper；load 不 import heavy library、不修改 backend/rcParams、不出 stdout、不联网、不 sleep、不写文件。已存在 matplotlib stub 时也不隐式修改状态。 |
| `figure-style` 显式调用 | `figure_style_self_check`/apply 接缝仅在明确调用时应用样式，字体查找用 stub；`panel_crops` 用 fake matplotlib Bbox、axes 和 renderer 验证像素坐标、上下翻转、同排/同列共享轴合并与图像边界 clamp。该 helper 只返回坐标，不 import PIL、不裁切或写图片，测试同样不得写图像文件。 |

对 load 测试全局拦截 urllib 网络入口、sleep、文件写入与可选依赖导入；对显式调用测试只为该调用放开所需 fake，未 mock 的网络调用立即失败。使用 `patch.dict(os.environ, ..., clear=True)` 或等效 fixture，测试进程不读取开发者实际 key/contact；敏感哨兵仅是假字符串。所有 mock/环境/sys.modules 修改在测试结束恢复。失败信息只包含固定 fixture 名和断言，不回显真实环境、URL 或私有源文件。

这些测试证明受控输入下的 helper 行为与已捆绑源匹配，不证明所有 26 个 Skills 可在本机执行、依赖已经安装、scientific validity 或真实端到端工作流通过。原有 bundle/hash 测试继续保留；Python fixture mock 不替代它们。

### 与 V4 runner 的报告和 CI 边界

[Runtime 设计](2026-10-04-wisp-runtime-design.md) 规定 V4 runner 复用 `omicsops-agent-core` 现有 private `ScriptedModel/MemoryStore` 等 unit test fixtures：在 `lib.rs` 的 `#[cfg(test)] mod tests` 下声明 `mod scenario_evaluation;`，文件为 `crates/omicsops-agent-core/src/tests/scenario_evaluation.rs`，manifest 为同目录 `data/v4-scenarios.json` 并 `include_str!` 固定读取。它不新增 production API、Tauri UI 或 CLI crate，默认 `cargo test --workspace` 即执行。

CI 在保留现有 Windows/Node 22 和所有必需检查的基础上增加 `actions/setup-python@v5` / Python 3.12 与 `python -B -m unittest discover -s scripts -p "test_*.py"`。该命令包含已有 model catalog / remote bridge 的受控测试及新 helper 测试；不运行 ignored live acceptance、不 pip install、不调用 scientific libraries。npm test 保持自身职责，不因本项要求开发者在前端测试中预先配置科学 Python 环境。

V4 定向报告命令为 `cargo test -p omicsops-agent-core scenario_evaluation -- --nocapture`，显式设置 `OMICSOPS_EVAL_REPORT_PATH=target/evaluation/v4-scenarios.json`。报告由 runtime runner 管理，内容仅为 schema_version、suite_hash、runner_contract_version、每场景 pass/fail、模型/工具尝试数与实际 dispatch 数、白名单失败原因；没有原始模型内容、私有推理、密钥或 provider/SSH 错误。失败退出非零，完整 workspace 检查仍保留。CI 可用 `actions/upload-artifact@v4` 在 `if: always()` 下上传这个明确路径；目标缺失时不生成假的成功报告，`target/` 不纳入 Git。

helper suite 使用 unittest 正常 pass/fail 输出和非零退出码，不定义第二个模型场景报告 schema，不汇入 V4 call count。两个 suite 共同受无凭据/无外部运行的约束，但报告 provenance 独立：V4 suite hash 只覆盖 manifest/runner 契约，不能冒称覆盖 Skill 文件。上传 artifact 不包含环境快照、user paths、stdout 原始检索 URL、真实 provider/SSH 错误或 helper source。整个 job 的必需检查保持依赖失败传播；`if: always()` 只用于报告上传，不把测试失败改成成功。

本地最接近的 helper 检查为 `python -B -m unittest discover -s scripts -p "test_bundled_skill_helpers.py"`，之后按 AGENTS.md 执行完整默认检查。可用 system Python 或 Codex 已提供的 Python runtime 执行标准库检查；仓库文档不固定开发者个人 Python 路径，找到解释器不表示科学依赖存在。helpers/runner 通过与 live acceptance “未执行/被忽略”必须分别报告。

### 测试验收与文件地图

新 suite 在只有 Python 标准库的环境可发现并执行，所有失败注入都有对应非零测试结果。评审通过 assertion 对关键 helper 分支的直接依赖确认不是只 import 后报通过：空结果/失败分开、一次重试、secret traceback、claim/path 绑定、加载副作用与限定 crop 输出均需断言。临时文件位于临时目录，不生成 checked-in `.pyc`，无 skip 来掩盖缺少 matplotlib/Pillow。

| 文件 | 职责 |
| --- | --- |
| `scripts/test_bundled_skill_helpers.py` | 三个 allowlisted bundled helper 的 stdlib mocks/行为断言。 |
| `scripts/README.md` | 近端 helper 命令、discovery 和离线覆盖边界。 |
| `.github/workflows/ci.yml` | Python 3.12/discovery；与 runtime owner 共用一次 V4 定向报告/upload 配置，不重复 runner。 |
| `docs/bundled-science.md`、`acceptance/README.md` | bundle/hash、helper、scripted V4 和真实工作流的不同证据级别。 |
| `crates/omicsops-agent-core/src/tests/scenario_evaluation.rs`、`data/v4-scenarios.json`、`src/lib.rs` 测试模块 | Runtime 设计拥有，本文只引用共同 CI/报告边界。 |

## 交付独立性与评审核对

W06 不依赖 W07 或 runner，复用 host public_text、Store 链验证、会话保存和 Escape 接缝；publication report 只与它共用安全文本投影，诊断 head 不作为 publication 证据。W07 不依赖生产 Agent/科学内核，以固定编译 fixture 和收据独立交付；本地预览接入前需已有 W02 的文件读取叶子，不依赖 W02 的表格功能。W08 helper suite 可先独立接入 Python CI，runner 报告按照 Runtime 设计接入同一 workflow；不产生两个报告 owner。

共享编辑边界为 `src-tauri/src/lib.rs`、`crates/omicsops-dto/src/lib.rs`、`src/types.ts`、DTO 契约测试、ProjectLibrary/DesktopApp 与 CI。实现时只在这些入口接入各模块，不按文件长度重构、不改锁文件或 SOURCE pin。本文所列 DTO、上限、scope/read-only 规则、错误/取消语义、许可与测试边界构成首次交付的评审契约。本轮仅落设计文档，未执行产品代码、自动测试或真实端到端验收。
