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
- project.list、Memory、Skills、MCP 按需发现；项目列表路径仍受工具路径规则检查。
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
