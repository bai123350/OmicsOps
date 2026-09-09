# wisp-inspired Agent Loop 实施计划

日期：2026-09-08。状态：已实现前三个切片的执行保护及切片 4 的有界委派/冻结角色 profile；设计见 [设计文档](../specs/2026-09-08-wisp-inspired-agent-loop.md)。切片 5 已接入持久化指导与模型等待切入；完整规格目录/准确图片成本、推理档位配置和切片 6 待实施，不能将六步计划整体标为完成。

## 切片 1：完整请求预算

当前实现：adapters `RequestBudget` 统一估算最终 JSON，core 调用 ModelPort 预检并在压缩后再次检查；DesktopModelPortV4 使用同一请求构造函数，发送及 fallback 再检查。已有 profile fallback 保留，未知图片成本明确拒绝。新增 context_overflow 错误类别，供应商明确溢出时最多一次缩小请求重试，未改变存储表。原计划中的 models.dev 输出目录在当前工作树不存在；不新增硬编码型号规格。目录接入及准确图片预算保持待办。

1. 在 `omicsops-agent-core` 为请求预算建立纯数据类型/纯计算函数，保留 context_max_bytes 保护。
2. 检查 `model_catalog_shared.rs`、统一模型客户端和 DesktopModelPortV4 已有规格来源，传递精确 profile 对应的窗口、最大输出与估算信息；不得在 core 硬编码模型规格。
3. `context_for` 压缩前后校验预算；provider 最终请求发送前复核 system、工具 schema、图片和适配器额外消息的开销。未知成本和压缩仍超限有明确错误。
4. 保持归档先于 checkpoint；一次请求的压缩与 overflow 重试均有上限。不得在预算失败时创建新 run 或清除历史。
5. 测试边界等于预算、schema 独自超限、超大科学状态、图片未知成本、UTF-8、归档失败、压缩仍超限以及 Plan/ordinary 回归。

完成标准：超限请求不发送，原事件可恢复，近邻及默认检查通过。此切片不增加自动模型路由、子 agent 工具或输入队列。

## 切片 2：工具结果的有界模型视图

已实现：8 KiB 工具模型视图、原始事件引用、同 run 的只读分页工具和 checkpoint 引用；没有另存重复大文件或同步远端数据。新增测试覆盖多字节数据分页重建、跨 run/hash 拒绝、越界、恢复以及 Plan 工具不可见。旧的未授予读取 capability 的 run 保留旧投影。

沿用现有 outcome/archive 接口，增加受控 artifact 引用和有界短视图；先查明所有模型上下文拼装入口，避免 data 与 model_content 重复注入。测试重新加载也有界、hash/路径校验、秘密过滤契约和截断后证据可检索。保留原始科研数据引用，不同步巨型数据。

## 切片 3：无进展与响应结束原因

已实现：按调用/结果而非参数单独检测重复后缀；完整 batch 收口才计入；用户输入/科学状态变化重置；空响应、显式截断、缺少终止事件拒绝作为成功工具轮次。未增加第一次无进展的额外修正轮次，直接使用现有 needs-attention 恢复方式。

补足 empty/truncated/overflow 等响应分类，再添加 completed-batch observation 窗口与重复后缀检测。测试固定参数但变化结果不误报、稳定时间戳噪声不逃逸、A/B 循环、guidance/任务变化重置、恢复后继续计数。失败路径不得 RunCompleted，不执行失效批次尾部调用。

## 切片 4：有界委派上下文与角色模型

已实现：子请求（包括 schema/工具描述）默认 64 KiB 且不超过父上下文字节限额；提交 JSON 默认 8 KiB；超限保留原始 outcome 并失败或要求精简，未静默截断证据。子模型/只读工具有超时和取消检查。父视图保留结构化结论与调用统计，完整 node/graph trace 沿用事件存储，通过同 run/hash 的 `data` 分页读取；checkpoint 同样保留引用。

恢复时校验完整 graph 一致，仅复用已有成功节点及成功依赖链。按 run 从 GraphStarted 事件累计预留最多 32 轮子模型和 64 次子工具；未完成图的每次重启重新预留整图上限（保守计数），全部成功复用不再收费。此额度独立于既有主模型循环限额，未宣称实现精确 token/金额计费。

Settings 可将主 profile 绑定到另一个已保存的只读子 profile；新建 ordinary run 冻结子 profile ID 和精确配置 hash，纳入 spec_hash。执行和恢复不读主 profile 的最新角色选择覆盖冻结值；配置不存在/变化明确失败，未配置沿用主模型。approved_plan 和旧 spec 不改变；reviewer 保留原路径。ModelProfile 使用现有 JSON 存储，新增可选字段，无 SQL 表变更。共享保存 DTO 区分省略（保留）和 null（清除）。

确定性测试覆盖 schema 实际传递、超限不发送、输出修正、模型/工具超时取消、父级投影与 checkpoint、原始 trace 分页、精确成功复用、恢复预算、fake role resolver、精确配置变化、旧 spec hash 兼容、DTO 与设置表单 Escape。推理档位 requested/effective/capability source 和自动主模型路由仍未实施；不能把产品 profile 选择描述为 Luna/max 已生效。

## 切片 5：运行中指导与单 driver

已实现独立 SQLite 收件箱及 GuidanceConsumed 事件。接收不抢占事件序号；driver 在模型边界以同一事务消费并写事件。每 run 最多 16 条，每条最多 2048 UTF-8 字节，重送 message_id 幂等且校验项目/会话/run/内容。消费事件在 checkpoint 后仍完整进入 active_guidance，不修改 frozen spec 或 approved_plan。RunCompleted 与未消费收件箱在同一事务互斥，旧完成提案不能越过新指导。普通/计划启动在事务内争用同一会话；恢复复用原 run 的 driver 槽位。

UI 使用内联面板，区分已接收和已应用，失败保留原文并复用请求 ID；切换 run 丢弃迟到响应。已应用仅表示持久化进入模型上下文，不表示指导要求已完成。模型等待及主循环未完成的 ReadOnly 工具等待可被指导打断。已完成的工具结果保留；中断的读取记录 guidance_interrupted 可恢复失败，不伪造证据。副作用工具仍等待 batch 收口；不调用 runtime 全局 interrupt，不宣称远端操作已取消。approved_plan 不受影响，无新增覆盖层。

## 切片 6：结构化等待的闭环

前置调查：当前 KernelProcessV4::execute 仍为等待最终结果的同步异步调用，session_id 是解释器会话而非可恢复的计算任务句柄。已补充独立任务 ID、持久化启动占用和 reserved/running/unknown/succeeded/failed 状态；下一增量接入后台等待与恢复；不能把会话 ID 当作任务 ID，也不新建通用 shell 长超时。fake backend 覆盖启动成功、运行中、断线未知、完成产物、取消未确认；等待期间不重复启动任务，也不以固定频率调用模型。真实 SSH/GPU 等另行一次性验收。

## 每个切片的验证和回滚

- 先运行变更包的近邻测试，再运行仓库要求的 `cargo test --workspace`、`npm test`、`npm run build`；桌面组合/配置/打包变化再做 `npm run build:desktop`。
- 依赖锁变化使用 `npm ci`。仅格式偏差按 AGENTS.md 独立处理，不混入功能提交。
- 本计划未执行真实模型、SSH、浏览器或跨平台安装验收。后续记录实际命令、结果、ignored 数和可复核产物，不把 fake/构建描述为端到端验证。
- 每个切片使用现有配置开关或兼容默认控制启用；回滚停止新行为但保留已记录事件、引用和 uncertain 状态。协议/迁移需提前定义向前兼容读取策略；旧二进制不能理解新事件时应明确拒绝恢复，不能跳过未知事件继续副作用。

## 本次设计交付的实际验证

2026-09-08，Windows 工作树，仅新增两份 Markdown 文档：

| 命令 | 结果 |
| --- | --- |
| `cargo test --workspace` | 通过，exit 0；真实环境相关 ignored 测试未执行 |
| `npm test` | 通过：110 个前端测试、22 个浏览器扩展测试 |
| `npm run build` | 通过；Vite 提示存在超过 500 kB 的 chunk |
| `git diff --check` | 通过；新增未跟踪文档另作内容/引用检查 |
| `npm run build:desktop` | 未执行，本次没有桌面组合、配置或打包变更 |
| 真实模型/SSH/R/PBMC、macOS smoke | 未执行 |

以上只说明现有代码基线的检查结果，不验证尚未实现的设计行为。

## 前三个切片的实现验证

2026-09-08，Windows 工作树，完成上述前三个切片的执行保护后：

| 命令 | 结果 |
| --- | --- |
| `cargo test --workspace` | 通过，exit 0；包含 core 的 69 个测试和模型适配器契约的 16 个测试；真实环境相关 ignored 测试未执行 |
| `npm test` | 通过：110 个前端测试、22 个浏览器扩展测试 |
| `npm run build` | 通过；Vite 仍提示存在超过 500 kB 的 chunk |
| `npm run build:desktop` | 通过，exit 0；Windows release 和 NSIS 安装包构建成功，仅用于构建验证，未发布或分发 |
| 修改的 Rust 文件的 `rustfmt --edition 2024 --check --config skip_children=true` | 通过 |
| `git diff --check` | 通过 |
| 真实模型/SSH/R/PBMC、桌面交互及 macOS smoke | 未执行；步骤见 `acceptance/README.md` |

新增确定性覆盖包括最终请求预算与 fallback、一次 overflow 恢复、工具原始结果分页与访问范围、结果变化和重复循环、空响应及截断流。上述测试和构建不代表真实模型或远端科研工作流端到端验收。依赖锁文件和数据库表未改变。

在前三个切片交付时，完整模型规格目录、准确图片成本及切片 4–6 尚未完成。开发委派按用户要求使用 Luna/max；此处是该阶段的历史记录，最新角色配置进度见上面的切片 4。

## 切片 4 的实现验证

2026-09-08，Windows 工作树：

| 命令 | 结果 |
| --- | --- |
| `cargo test -p omicsops-agent-core delegation --lib` | 通过：11 项委派测试 |
| `cargo test -p omicsops-desktop delegated_profile_freezes_exact_configuration_and_missing_profiles_fail --lib` | 通过：冻结配置、缺失 profile、子 provider 独立预算 |
| `npx vitest run src/features/settings/SettingsPanel.test.tsx` | 通过：12 项设置测试 |
| `cargo test --workspace` | 通过，exit 0；core 共 76 项测试；真实环境 ignored 测试未执行 |
| `npm test` | 通过：112 项前端测试、22 项浏览器扩展测试 |
| `npm run build` | 通过（桌面构建的 beforeBuildCommand）；Vite 仍提示超过 500 kB 的 chunk |
| 修改的 Rust 文件的 `rustfmt --edition 2024 --check --config skip_children=true` | 通过 |
| `git diff --check` | 通过 |

`npm run build:desktop` 通过，exit 0；Windows release 和 NSIS 安装包构建成功。真实模型、SSH、macOS 与桌面交互 smoke 未执行；测试不代表真实供应商端到端验收。没有新增依赖或 SQL 表，没有提交、发布或分发安装包。

## 功能提交整理

2026-09-08，按用户要求在上述验证完成后创建本地提交：

- `ad7d43b`：供应商最终请求预算及流结束校验。
- `5e5e893`：Agent 上下文、无进展检测、委派限额、恢复与核心角色接口。
- `c08afcd`：桌面预算接入、冻结子模型 profile、设置界面和 DTO。

设计、README 与验收步骤另作文档提交。未推送远端，未发布或分发安装包。前面各阶段的“未提交”描述是当时交付状态。

## 切片 5 当前增量的实现验证

2026-09-08，Windows 工作树：

- `cargo test -p omicsops-store --test guidance`：7 项通过，覆盖幂等、作用域、限额、重开、故障回滚、消费/完成及普通/计划启动竞争。
- `cargo test -p omicsops-agent-core guidance --lib`：2 项通过，覆盖模型等待切入、消费一次、checkpoint、完成保护及 approved_plan 隔离。
- `cargo test --workspace`：通过，exit 0；真实环境 ignored 测试未执行。
- `npm test`：116 项前端测试、22 项扩展测试通过，其中新增指导面板 4 项。
- `npm run build`（桌面 beforeBuildCommand）：通过，保留 Vite 大 chunk 提示。
- `npm run build:desktop`：通过，Windows release/NSIS 构建成功，仅用于验证，未分发。
- `cargo fmt --all -- --check` 初次发现偏差，已运行 `cargo fmt --all`，复查通过；格式修改单独提交。

没有依赖锁变化。新增收件箱表由现有幂等迁移加载；未引入凭据字段、远端同步或新审批路径。旧程序无法识别 GuidanceConsumed 事件时不可跳过事件恢复。真实模型、SSH、Windows 桌面交互和 macOS smoke 未执行。工具等待即时切入、结构化计算等待、完整模型规格/图片预算与推理档位仍待后续增量。

## 切片 5：只读工具等待切入补充

主执行循环按单个工具 future 检查指导收件箱：仅 ordinary Agent 的 ReadOnly 等待可提前返回 guidance_interrupted；join_all 保留已完成结果，主循环按既有 ToolFinished 路径收口后消费指导。网络、runtime、写入和委派操作不丢弃 future；已批准计划不受影响。停止等待不等于终止底层远端操作，指导不会触发全 run 的 runtime interrupt。此能力尚未扩展到委派子循环。

2026-09-08 实际验证：`cargo test --workspace` 通过（core 80 项，真实环境 ignored 未执行）；`npm test` 通过（116 项前端、22 项扩展）；`npm run build` 通过（已有大 chunk 提示）；`cargo fmt --all` 后格式复查通过，格式独立提交。新增测试覆盖已完成读取证据保留、未完成读取切入、消费与恢复不重放、所有副作用分类与 approved_plan 等待真实结果。`npm run build:desktop` 未执行，本次未改桌面组合/配置/打包；真实模型、SSH 和 Windows/macOS 交互 smoke 未执行。

## 切片 6：持久化计算任务基础

2026-09-09，新增 RuntimeJobV4 与 runtime_jobs_v4 表。job_id 区分单次调用与可复用 session_id；同 run/call_id 只有一次初始启动资格，项目、执行上下文、冻结 compute selection、已记录请求 hash 和未解决派发均须匹配。已有记录（包括未知、失败或成功）不会重新授予启动资格，旧已完成/不确定派发也不能借缺失任务记录重放。

桌面 runtime.execute 在启动解释器前持久化 reserved，取得会话后记录 running，收到结果并补齐软件版本后记录 succeeded/failed 及结果 request_id/SHA-256。任务表不复制代码、stdout/stderr、进程字符串、凭据或产物内容；原始 ToolFinished/归档仍负责证据，provenance 增加 runtime-job 引用。表由现有幂等迁移创建，无依赖变更。

运行失败、取消及 ToolDispatchUncertain 与未完成任务转 unknown 在同一事件事务提交；并发状态更新使用 CAS，迟到完成不能覆盖 unknown。reserved/running 只反映最后持久化阶段，不证明远端仍存活；崩溃恢复沿用原 uncertain dispatch 核验流程。任务摘要成功不等于 Agent 完成，也不能取代完整工具证据。

本增量不启动后台 worker、不添加模型轮询、不重跑未知计算，不宣称自动重连已完成。后台 start/status/wait、可验证的远端终态与产物重新挂接、取消确认、等待 UI，以及子委派等待切入仍待后续开发。Windows/macOS/SSH 使用各自现有 RuntimeManager 后端，无新增 Unix-only 命令。

本增量实际验证（2026-09-09）：

- `cargo test -p omicsops-store --test runtime_jobs` 初次 5 项通过，新增事务故障测试后在完整工作区测试中共 6 项通过。
- `cargo test -p omicsops-desktop runtime_jobs_v4 --lib` 通过，模拟成功/断线、并发重复请求、重建管理器均只启动一次。
- `cargo test --workspace` 通过，真实环境 ignored 未执行。
- `npm test` 通过：116 项前端测试、22 项扩展测试。
- `npm run build` 和 `npm run build:desktop` 通过；已有 Vite 大 chunk 与 Windows linker 信息提示，未发布或分发构建产物。
- `cargo fmt --all -- --check` 初次发现偏差，执行 `cargo fmt --all` 后复查通过；纯格式修改单独提交。`git diff --check` 通过。

未执行真实模型、SSH、Windows/macOS 交互 smoke。以上验证不代表后台计算等待/重连端到端完成。
