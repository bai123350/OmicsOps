# 普通 Agent 自适应循环

日期：2026-09-10。用户已确认实施范围；确定性和真实验收结果分别记录。

## 范围

保留 V4 基础，把普通 Agent 从固定发现流程改为目标驱动的模型—工具—结果循环。
本设计取代 ordinary-agent-guided-loop.md 中固定 discovery、强制任务清单与
MCP/浏览器顺序要求。复杂 Workflow、ACP、计算资源延迟初始化不属于第一阶段切片。
第二阶段按需资源初始化见 [设计](2026-09-10-lazy-agent-execution-resources.md)。

## 行为

- agent.route_request 是可选进度元数据；首次直接工具调用由 Host 记录 task shape。
- fast/multi_step、phase、cycle、batch 继续记录，不能授予权限，也不要求返回 discovery。
- project.list、Memory、Skills、MCP 按需发现；列表与读取路径仍受工具路径规则检查。
  列出项目根目录时优先省略 path 或使用空字符串、`.`，并使用列表返回的相对路径
  继续 project.read。project.list 仅把完整的 `/` 或单个 `\` 视为项目根目录别名，
  不能访问宿主磁盘根目录，也不把 `/subdir` 转换为项目相对路径。
  其余路径必须保持项目相对路径；绝对路径、父级遍历与规范化后越出项目根目录
  的路径仍被拒绝。project.read 不接受这些根目录别名来访问宿主路径。
- project.list/project.read/artifact.verify 的文件系统调用已返回失败时，作为
  succeeded=false 的可恢复工具结果反馈模型，使用固定的项目相对路径说明，
  不将原始 OS/SSH 错误或失败 provenance 传入结果。模型可以更正路径继续同一次
  运行。通用执行器错误与 runtime/远端计算的传输或派发不确定性仍保留原有
  契约，不据此重试副作用操作。
- 专业来源已足够时可以交付，不强制浏览器补跑；按 URL 直接浏览也不要求先搜索 MCP。
- Skill/MCP 的实际调用保留启用状态、schema/catalog、能力与审批检查。
- 浏览器保留连接、作用域、目标、CAPTCHA 和禁止网页 AI 提示词等检查。
  页面内容变化后先重新扫描再依赖该内容是方法要求，不再用全局发现顺序限制全部工具。
- agent.update_tasks 可选；使用时仍验证 2–12 项、revision、状态及完成项不回退。
  未完成清单不得直接完成；可恢复错误反馈模型继续处理。
- agent.complete 保留非空答案、完成标准、证据引用、确定性验证及独立 Reviewer。
  本次没有放宽 evidence schema 或增加无证据的纯文本完成分支。

## Plan 与运行状态

Ordinary run 内部继续存储冻结 ExecutionPlanV4，保持 spec/hash 和旧数据兼容。
桌面 summary 投影按 spec.execution_kind 隐藏 ordinary plan、plan_hash、approval_hash
和 plan_revision；不改磁盘记录。缺少 execution_kind 的旧 spec 仍按 approved_plan。

通用 conversation lock、工具 waiting_for_approval 不能单独证明存在 Plan。
计划就绪卡片只在真实待批准计划时出现，普通运行的工具轨迹不得因内部 plan 被隐藏。
已有批准计划的 revision/hash、批准、修订、取消和只读门禁保留。
恢复普通旧 run 不修改冻结契约；系统指导说明旧自动发现步骤不再作为必需前置条件。

## 运行与数据边界

继续使用 ModelPortV4、ToolPortV4、EventStoreV4、ScientificStateStoreV4 和现有运行循环。
无新增 crate、数据库迁移、权限默认值、外部消息或自动同步。
保留取消、预算、压缩、无进展检测、同 run 恢复和不确定派发人工核验。
本地与 SSH 计算能力不变，未改成后台作业自动轮询或跨重启交互内核恢复。

## 验证

确定性回归覆盖：无分类/基线发现/MCP 后浏览器补跑也能检索并修复后完成；
未授权调用仍暂停且无 dispatch；可选 task list 阻止虚假完成；summary 不改写冻结记录；
普通运行在运行、工具审批、失败待处理、取消和完成状态不显示 Plan；真正待批计划仍可见。

交付运行 cargo test --workspace、npm test、npm run build、npm run build:desktop。
真实模型/SSH 测试必须按 acceptance/README.md 的一次性环境要求执行；缺少配置时
明确记录未执行，不能把上述模拟回归或构建作为真实科研验收。

手工 smoke：Windows/macOS 新建普通会话检索文献，观察按需调用、实际记录和答案；
触发工具审批并切换会话后返回，确认无 Plan 卡片、审批仍可操作；取消/重开后状态准确。
另开 Plan 会话，生成、修订和批准计划，确认它仍走原有审批契约。

## 本次实际验证结果（Windows，2026-09-10）

- `cargo test --workspace`：通过，398 项通过，11 项 ignored。
- `npm test`：通过，143 项前端测试和 22 项浏览器扩展测试。
- `npm run build`：通过；保留 Vite 大于 500 kB chunk 提示。
- `npm run build:desktop`：通过，完成本地 NSIS 构建；未安装或分发。
- `cargo fmt --all -- --check`、`git diff --check`：通过。
- 真实模型/SSH/浏览器与 macOS 手工 smoke：未执行。验收要求的
  OMICSOPS_LIVE_SSH_*、OMICSOPS_LIVE_PBMC_ROOT 和模型环境配置缺失；
  ignored 测试与模拟回归不等于真实端到端通过。

## 项目根目录路径与失败分类回归（2026-10-01）

用户提供的 Windows OpenCode deepseek-v4.1-flash 运行截图中，project.list 的
path 为 `/`，失败说明为 `project path escaped the canonical project root`；
同一次运行随后因副作用派发不确定而终止。Windows 的 rooted 路径不一定是
is_absolute 路径，原有检查允许 `/` 进入本地 join 后才被根目录边界拒绝。
桌面执行器又把已明确返回的文件系统失败作为 Err 传给核心，使其走保守的派发
不确定分支，模型无法更正路径继续。

本次修复把 project.list 的完整空字符串、`.`、`/` 和单个 `\` 统一解释为当前
项目根目录；不扩展其他 rooted 或绝对路径。工具描述和 path schema 明确空路径
默认值、根目录别名及后续读取的相对路径规则。桌面执行器将 list/read/artifact.verify
已返回的文件系统失败转换为使用固定安全说明的可恢复失败结果；不修改核心的
通用 Err 契约、MCP 审批或计算派发与传输错误的恢复语义。没有新增持久化模式，
也不扩大项目文件访问范围。

确定性回归覆盖根目录别名、本地与 SSH 路径边界、列表返回的相对路径，以及
list/read/artifact.verify 失败后更正路径继续；仍保留 runtime/通用执行器错误
不盲目重放的回归。
本次 Windows 实际验证结果：

- 修复前 `cargo test -p omicsops-desktop --lib project_filesystem_ -- --nocapture`：
  原有 1 项通过，新增 4 项失败，复现根目录别名错误和只读错误被记为不确定派发。
- 修复后同一定向命令：6 项通过，包括新增的原始后端错误脱敏回归。
- `cargo test --workspace`：1,494 项通过，16 项 ignored；ignored 不算真实验收通过。
- `npm test`：992 项前端测试、22 项浏览器扩展测试通过。
- `npm run build`：通过；保留既有 Vite chunk 大小提示。
- `npm run build:desktop`：通过，完成本地 Windows release 与 NSIS 构建；未安装或分发。
  Rust linker 的建库信息以 warning 输出，未导致构建失败。
- `cargo fmt --all -- --check`、`git diff --check`：通过；无依赖锁文件或数据库模式变化。

当前未设置一次性验收所需的 `OMICSOPS_LIVE_MODEL_PROFILE_ID`、
`OMICSOPS_LIVE_SSH_HOST` 和 `OMICSOPS_LIVE_PBMC_ROOT`，未执行对应 ignored 测试。

手工 Windows Local smoke：在一次性项目中创建一个合成文本文件，分别用空 path、
`.`、`/` 和单个 `\` 列根目录，确认输出保持项目相对路径；按返回路径读取文件。
再尝试不存在的路径、`/subdir`、绝对路径和父级遍历，确认显示失败工具结果，
更正为有效相对路径后同一次运行可继续，且项目外文件不出现在结果中。
Windows SSH smoke：按 acceptance/README.md 配置一次性远端项目，重复这些调用，
确认根目录指向绑定的远端项目，失败后仍能更正路径，且不会切换为本地根目录。

真实 OpenCode 模型驱动运行、真实 SSH 验收和 macOS smoke 当前未执行；上述
确定性回归与待执行的手工步骤不证明真实端到端科研流程或 macOS 路径行为通过。
