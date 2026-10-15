# Wisp Science 可借鉴改进清单

日期：2026-10-04。状态：用户已选择全部12项首次范围、确认具体设计及[实施总计划](../plans/2026-10-04-wisp-improvements.md)，并选择子Agent逐项实施与审阅。W01–W10及W12全部源码已实现并审阅通过（47/50任务），其余项继续实施；W07原生手工验收未执行，W06隔离旧HEAD原生验收仅部分通过；状态及实际检查见总计划。全部12项首次范围尚未交付。本文件保留源码对照，具体边界见[总设计](2026-10-04-wisp-improvements-design.md)。

本轮建议先补会话草稿隔离，再分别交付科学产物预览和 SSH 后台作业面板。这三项直接改善日常研究操作，并能复用现有组件与运行身份。其余候选按依赖分批推进，优先保留 OmicsOps 已有的科研证据、审批与恢复契约。

## 核对基线和范围

上游固定为 [wisp-science 提交 2c1ad2e](https://github.com/xuzhougeng/wisp-science/commit/2c1ad2e1ee2e600be22c08f826490b1c627c71b4)，于本次读取时确认；提交时间为 2026-10-04 16:50:29 Asia/Shanghai。下文源码链接均固定到该提交，不随 main 后续更新变化。

OmicsOps 的原有修改已先在 main 提交为 `e3b20a5`，再创建 `codex/wisp-science-improvements`。独立网站仓库已在自己的 `3ba4693` 提交中保存且工作区干净；主仓库忽略该目录，保留网站历史和部署文件。

本次阅读了上游实现与测试源码，并核对本地代码。没有运行上游应用或测试；“上游有实现”仅表示源码存在，不表示完成实际平台验收。以下优先级、实施切片与适配取舍是针对 OmicsOps 的建议。

已有 Wisp 参考文档继续保留：[Agent 循环](2026-09-08-wisp-inspired-agent-loop.md)、[输入区对照](2026-09-13-wisp-composer-parity-inventory.md)、[设置中心](2026-09-15-wisp-settings-source-audit.md)、[运行时源码对照](2026-09-26-wisp-agent-runtime-source-audit.md)。本轮不重启这些已交付项目，也不把旧文档中的待办当作当前实现状态。

## 已具备的能力

| 能力 | 当前实现与本轮判断 |
| --- | --- |
| Agent 循环与恢复 | 已有工具结果反馈、无进展检测、检查点、一次上下文溢出恢复、指导、持久队列、只读委派及不确定派发处理。继续复用。 |
| Skills 与科学 MCP | 已捆绑 26 个 Wisp Skills 和 23 个原生科学工具域，按需读取资源、冻结哈希与逐工具授权。见[内置科学工具说明](../../bundled-science.md)。 |
| 设置与输入区 | 已有分组设置、工作流库、记忆管理、附件引用、旁聊、会话分支、会话分享及用量页面。不要再次列为“待引入”。 |
| 运行边界与 SSH 身份 | 已有草稿/冻结运行边界视图、交互 job 账本、保存结果回执和 Linux SSH 独立后台作业查询。缺口集中在观察与操作入口。 |
| 轨迹与用量 | 已有工具与模型等待耗时、可展开轨迹、持久请求/用量事件及稳定快照分页聚合。清单补筛选、定位和诊断，不再新建用量存储。 |
| 科学状态与稿件 | 已有 Dataset/Analysis/Artifact/Evidence/Provenance、证据失效、稿件版本、来源快照和 Markdown 导出。完整复现与证据包仍未接入现有稿件流程。 |

运行时 inspect、远端身份核验、上下文压缩、探索文件快照与轨迹导出的相关上游源文件，与此前核对的 `b242fcbd` 基线内容相同。这些属于可继续补齐的既有机制。本轮新增的近期参考包括会话草稿隔离、可恢复的会话收起和可关闭使用提示。

## 优先级总表

P0 表示优先解决可能影响操作正确性的问题；P1 表示直接改善科研操作和可验证性；P2 表示依赖前序抽象、适合后续独立交付。每一项均有现成基础，仍需独立设计、测试和提交。

| 编号 | 优先级 | 改进项 | 首次可交付范围 | 涉及边界 |
| --- | --- | --- | --- | --- |
| W01 | P0 | 会话草稿隔离 | 按项目/会话保存并恢复内存中的文字草稿 | React 状态与发送回执 |
| W02 | P1 | 科学产物预览 | 有界 CSV/TSV 表格；PDF 等格式另做切片 | DTO、项目文件读取、WebView |
| W03 | P1 | SSH 后台作业面板 | 作业列表、手动刷新、受控退避查询 | 原 job 身份、SSH、UI |
| W04 | P1 | 内核对象观察 | 已启动 Python/R 内核的有界对象元数据 | Kernel 协议、session 身份 |
| W05 | P1 | 手动上下文压缩 | 暂停 ordinary run 的完整请求准入与原子提交 | provider 预算、checkpoint、恢复 |
| W06 | P1 | 轨迹诊断与导出 | run/tool/失败筛选、事件定位、脱敏导出 | 事件链、宿主导出 |
| W07 | P1 | 离线示例工作区 | 一个合成或录制示例，无需模型密钥 | 模板、示例资源、来源标记 |
| W08 | P1 | 场景评测与 Skill 行为回归 | 离线 V4 报告；已捆绑 helper 的离线回归 | 测试接缝、CI、受控 Python |
| W09 | P2 | 稿件准备度与证据包 | 先只读报告，再结构化 reference-only 导出 | 科学状态、稿件 revision、快照 |
| W10 | P2 | 探索文件基线与差异 | 显式选中小文件的快照和差异 | 项目路径、内容哈希、分支 |
| W11 | P2 | 可选择的项目迁移 | 只读迁移清单预览；下一切片才导出/导入新项目 | Store、文件选择、能力重绑定 |
| W12 | P2 | 收起和恢复会话 | 可逆可见性偏好与独立检索入口 | 幂等迁移、搜索与引用 |

## W01 会话草稿隔离

**参考事实：**上游[草稿切换实现][w-drafts]按会话缓存文字，离开时保存、进入时恢复，缓存只存在于内存；无会话时输入的文字可交给随后创建的会话。

**本地差距：**[WorkspaceShell](../../../src/features/workspace/WorkspaceShell.tsx) 第 276 行持有一个 draft 状态，第 674 行起的作用域切换只清理部分界面与引用；[DesktopApp](../../../src/DesktopApp.tsx) 第 1433 行复用同一 WorkspaceShell。按源码判断，普通切换未建立按会话保存和恢复文字的机制。这是操作正确性候选，尚未执行专门 GUI 复现。

**建议切片：**以 project_id/conversation_id 为键缓存文字；先不落库或承诺跨重启恢复。引用和附件继续遵循原作用域，不能因恢复文字而复用其他会话的附件回执。延迟发送成功只清理其原作用域及已提交版本的草稿。

**验收：**A/B 会话往返恢复各自文字；切项目不串草稿；新会话按明确定义接受无会话草稿；旧请求回执不清空新草稿；发送失败保留文字；附件和引用不跨作用域授权。

## W02 科学产物预览

**实施状态：**共享本地 handle 读取、纯 CSV/TSV parser/DTO、本地/SSH source/command 和真实只读界面已实现，四任务独立审阅及必要复审通过（7258149/7439276、69886e4、bec43f1、a8fa4a4/153a0ea）。最终前端1171+22/Web通过；Rust/native为焦点fix前证据，最终全分支native仍待执行；GUI/liveSSH/模型验收未执行，不能冒称生产端到端通过。实际确定性检查见[W02计划](../plans/2026-10-04-wisp-w02-table-preview.md)。

**参考事实：**上游[格式路由与显示边界][w-previews]支持表格、Notebook、PDF 和 Office；[PDF 前端回归][w-pdf-tests]覆盖翻页、缩放和损坏文件。OmicsOps 的[文字预览](../../../src/features/workspace/ComposerFilePreviewDialog.tsx)和[SSH 图片预览](../../../src-tauri/src/sync_commands.rs)已绑定来源、大小及校验信息，通用表格/PDF 预览尚缺。

**建议切片：**先实现 CSV/TSV 的只读、有界表格视图，沿用项目/backend/相对路径身份。区分读取的前缀、展示行数和完整文件；SSH 大文件保持远端，显式读取有界内容。PDF、静态 Notebook/HTML、Office 分别后续交付，不一次引入所有解析器。

**验收：**UTF-8、CRLF、引号内换行/分隔符、损坏格式、超大文件、模拟 SSH、过期响应以及立即 Escape。前缀哈希与完整文件哈希分开，截断有明确提示。

**适配要求：**上游表格实现按物理行读取，不能直接当完整 CSV 解析器。普通 HTML iframe 同时允许 scripts 与 same-origin；OmicsOps 应采用[静态 Notebook 的禁网和无权限 sandbox 思路][w-preview-sandbox]，并核验远端资源、脚本和导航限制。Office 后续另加解压大小、压缩比、宏与外部资源约束。

## W03 SSH 后台作业面板

**实施状态：**metadata列表、原身份只读status、明确启用的有限10次退避与真实执行环境入口已实现，四任务独立审阅及必要复审通过（9d415c6/bad6feb、6e7519d、1aaafca、0c4be6e/e8fe4ec）。2026-10-08最终Rust1613通过/17 ignored、frontend1220+22、Web/native/fmt/diff通过；存储临时不可用继续有限窗口，结构损坏永久拒绝，均有真实RED/GREEN。GUI/安装/liveSSH/模型/Node22 CI及最终clean HEAD验证未执行，不冒称生产端到端通过。实际证据与保守腐败记录拒绝代价见[W03计划](../plans/2026-10-04-wisp-w03-remote-jobs.md)。

**对照基线事实：**上游[作业卡片][w-jobs-ui]与[退避查询][w-jobs-poll]展示日志、状态和作业操作。原OmicsOps的[remote_jobs_v4.rs](../../../src-tauri/src/remote_jobs_v4.rs)已有按原 job 身份重连查询，RuntimeDialog当时无自动轮询和取消；当前新增手动默认/明确开启的有限窗口，仍无远端取消。

**建议切片：**先复用现有 Store 和查询契约提供项目作业列表、手动刷新、有界日志、完成产物引用。自动查询作为后续受控开关，仅查询已授权且身份核验通过的绑定；切项目、解绑、终态或关闭面板停止查询，失败退避。

**验收：**重启后只查询原 job；不会重新提交代码；主机指纹/root/request 不匹配被拒绝；断线保留 unknown；旧响应不覆盖新项目；日志和查询错误脱敏；一次 Escape 只关顶层。

**独立后续：**取消必须单独设计审批与原进程身份核验。[上游取消核验][w-jobs-cancel]验证 token、PGID 和进程起始时间，并确认终止；不沿用二次取消直接收口本地 Cancelled 的语义。停止 Agent、停止轮询和远端终止分别记录。

## W04 内核对象观察

**参考事实：**上游[inspect 与 generation 检查][w-inspect]观察已有内核且不隐式启动，执行可拒绝陈旧状态。OmicsOps 的[KernelProcessV4 与 RuntimeManagerV4](../../../crates/omicsops-runtime/src/lib.rs)有 session_id、健康检查和现有作业串行执行基础，没有对象观察接口。

**建议切片：**先提供名称、内置类型、shape/尺寸和有界摘要，绑定现有 session_id；未启动返回“未启动”。观察通过同一协议队列，不能与执行争抢协议帧，不能任意调用用户对象方法。后续再为依赖内存对象的执行增加可选预期 session 与对象约束，不直接移植另一套 manager。

**验收：**观察不启动解释器、不安装依赖；重建内核后旧 session 请求拒绝；Python/R 使用共享 DTO；模拟 Local/SSH；对象和字节预算有界；敏感名称/错误不进入日志或模型上下文。观察结果不作为科研结论证据。

**实施状态：**五任务均已独立审阅及必要复审通过（a1a7467、45dd6db/91964e3、f644d0c/ef7da02、a0fbedc、db174a6/6609846）。最终UI修复后最近测试174、完整前端1251+扩展22、Web构建通过；此前完整Rust1665通过/18 ignored、Python48、Windows NSIS构建通过，但这些原生构建先于UI修复，最终全分支原生检查仍待执行。真实R/SSH/模型、Windows GUI/安装和干净提交HEAD验收未执行；测试替身与构建不能替代生产端到端验收。详见[W04计划](../plans/2026-10-04-wisp-w04-kernel-inspection.md)。

## W05 手动上下文压缩

**实施状态：**五任务源码已独立审阅，暂停 ordinary run 的完整请求准入、原子Store提交、宿主租约和恢复UI已接入；旧hash/来源兼容，binding/raw-run CAS与自动压缩非原子残余边界已记录。功能验证与真实模型/SSH未验收区别见[W05计划](../plans/2026-10-04-wisp-w05-context-compaction.md)；最终clean-HEAD和全分支检查仍待执行。

**参考事实：**上游[压缩实现][w-compact]先归档、失败回滚，并将模型工作集与可视历史分离。OmicsOps 已有 archive/checkpoint/receipt 基础，但[宿主入口](../../../src-tauri/src/context_compaction.rs)仅对 completed 返回 NotNeeded；WorkspaceShell 未传 onCompact，按钮仍禁用。

**建议切片：**只针对无活动 driver、无待消费 guidance 的暂停 ordinary run，复用现有身份、请求 ID 和冻结配置，构建压缩候选并证明完整 provider 请求可准入，成功后才原子提交与接通按钮。摘要不足或预算失败保留原上下文。

**验收：**候选和最终请求均计入 system、工具 schema 与 provider 格式；证据分页引用、原始事件链和批准计划不被丢弃；重复请求幂等；重启使用同一 checkpoint；锁冲突与压缩失败不改变原记录。

这项需补全准入与恢复设计，不能仅启用按钮，也不能以压缩为理由改变模型强度、关闭审查或扩大能力。

## W06 轨迹诊断与安全导出

**实施状态：**四任务源码独立审阅通过，validated只读链、脱敏投影、安全HTML/JSON保存及筛选定位UI已实现，确定性/桌面构建通过。2026-10-11隔离旧HEAD测试副本中真实过滤、原生保存/取消、来源定位和Escape部分通过；静态HTML浏览未完成，用户随后停止GUI。此受限副本证据不是最终HEAD或生产端到端，详见[W06计划](../plans/2026-10-04-wisp-w06-run-diagnostics.md)与最终交付记录。

**参考事实：**上游[轨迹重建][w-trace]与[宿主导出][w-trace-export]将工具、turn 和用量重建为可定位视图。OmicsOps 的 WorkspaceShell 第 1232 行起已有 V4RunTrace、模型/工具计时与折叠展示；Usage 已读取持久化观察。

**建议切片：**新增 run/tool/失败筛选、跳到精确 event sequence/hash，以及脱敏 HTML/JSON 诊断导出。导出由宿主重新读取、核验作用域和事件链；复用现有存储，先不增加新的 timing 或 usage 系统。

**验收：**重复 call_id 不跨 run 混淆；缺失耗时和用量显示 unknown；排序稳定；敏感哨兵、私有推理、原始 provider/SSH 错误不出现在包中；取消保存不报成功；筛选后的临时 UI 不能冒充完整证据。

此项便于继续验证既有[性能优化](2026-10-04-agent-performance-followup.md)，实际耗时结论仍需同任务、同模型配置的受控测量。

## W07 离线示例工作区

**实施状态：**四任务源码独立审阅通过；marker事务、固定双语合成fixture、保留handle身份的no-replace发布/清理、只读view及所有项目选择入口已接入。最终`a7e05c9`；近端157、Rust1777通过/18ignored、前端1316+扩展22、Web/native/fmt/diff通过。原生创建/重启/改删文件/丢收据smoke未执行（用户停止GUI），真实科学工作流未执行；不能把预置算术与hash当执行证据。详见[W07计划](../plans/2026-10-04-wisp-w07-offline-example.md)。

**参考事实：**上游[seed 实现及测试][w-seed]导入预录转录与配套文件，不需要模型密钥。OmicsOps 当前项目模板和工作流文本配方没有配套录制示例，捆绑文档明确 ESR1 种子未导入。

**建议切片：**用小体积合成数据与示例结果建立一个双语项目，演示“来源 → 结果表 → 报告”的路径；由显式入口创建独立项目。演示内容使用单独来源标记，不伪造当前宿主的 Run、审批、执行或验证事件；不引入真实样本或上游 thinking。

**验收：**无模型、网络或 SSH 仍可打开；示例文件可预览；重复打开不覆盖用户文件；临时目录测试拒绝越界、符号链接和冲突；明确显示录制/合成来源，查看不派发计算。资源许可与来源随包保存。

## W08 场景评测和 Skill 行为回归

**实施状态：**helper子计划三任务已实现并独立审阅通过（74e8fa4、d6a71bd、edfb23f）；捆绑runtime/SOURCE/hash/freeze未改动。2026-10-08本地helper29/完整Python38、Rust1613通过/17 ignored、frontend1220+22/Web/fmt通过，CI已配置Python3.12标准discovery但未执行（本地Python3.10/Node24）。实际fixture RED、正确旧helper baseline PASS及权限失败重跑均保留；不冒称科学端到端验收。2026-10-12场景manifest/runner两任务已独立审阅通过（a8edbb9、a500df7、9711ef6），最新scenario 9/core 232、fmt/diff通过；报告写入/非零/CI Task3也已独立审阅通过（1081b28），W08两部分源码完成；实际故障非零与完整五项JSON已验证，CI配置未运行。缺target根修订前workspace1791通过18ignored，之后core238/scenario15/Python48/前端1316+22/Web/fmt/diff通过；最后测试断言单独1/1。固定fixture身份及Cargo/cache Minor留最终审阅，shared working source不等于clean-HEAD或真实验收。证据与外部共享文件保留证明见[helper计划](../plans/2026-10-04-wisp-w08-skill-helpers.md)。

**参考事实：**上游[离线 suite][w-eval-suite]声明输入、脚本模型和工具断言，[runner 与报告][w-eval]在临时工作区运行；[Skill helper 测试][w-skill-tests]通过替身验证 OpenAlex 错误分类、claim/图路径绑定和加载无副作用。

**本地基础：**Agent core 已有 ScriptedModel、恢复/审批/预算回归；Skill 测试已有资源完整性、哈希和启停覆盖。本轮核对的 analysis-workflow 指南、literature-review/runtime.py 和 paper-narrative/runtime.py，与固定上游内容在 CRLF 归一化后相同，应补测试覆盖而非重复导入。

**两个独立切片：**先将 3–5 个现有 V4 测试接缝整理为可统一执行的离线场景与稳定报告；读取证据、拒绝审批、恢复不重派、MCP 参数失败更正和自动压缩保留来源作为首组。另用受控 Python unittest 覆盖已经捆绑的 helper 行为，模拟 HTTP、等待及 matplotlib/Pillow，不安装科研库。

**验收：**报告包含 suite hash、状态、调用数量和脱敏原因，失败退出非零；Windows CI 无真实模型、SSH、GPU、R 或网络即可运行；测试读取本地捆绑包；错误注入能让关键断言失败。真实端到端验收继续独立按 acceptance/README.md 执行。

不新增旧布局中的 cli crate，不搬上游 debug 凭据文件，不用容易抖动的机器耗时阈值代替行为断言。

## W09 稿件准备度和证据包

**实施进度（2026-10-13，最新）：**Tasks1–4源码与独立任务审阅完成，保存revision检查和reference-only包入口已接入；最终focused32/regression32、Vitest1345+Node22、Web/native构建通过，未变Rust1831/18ignored/Python48。原始文件NotChecked、独立复现NotExecuted；native手工在用户STOP下未执行，最终clean-HEAD与整分支审阅仍待后续。下述分阶段记录保留其当时范围。

**实施进度（2026-10-13）：**Task3引用包及两项边界修复经完整+定向独立审阅完成（89ae5cc、dcdd8c6）；最终package10/evidence23/fmt/diff通过，DTO2为未变合同先前通过。固定包只有公开引用，原始文件NotChecked、独立复现NotExecuted；Task4保存版本UI及全量/原生验收仍未完成。

**实施进度（2026-10-13）：**Task2报告与原始source安全扫描经完整+定向独立审阅完成（e684516、24073b4）；最终readiness13/fmt/diff通过。包/UI仍待Tasks3–4，不能称W09完整交付；Store只读验证与外部能力源码边界不等同真实复现/native验收。

**实施进度（2026-10-12）：**Task1共享DTO与单事务有界精确revision context已完成独立审阅（9cfa558、537ab71、ef6fb9c、15d727a）；最终context13/navigation18/fmt/diff通过，DTO合同1/1先前通过。报告分类、reference-only包及UI仍待Tasks2–4，不能称W09已交付；源码替身不证明真实复现或native验收。

**参考事实：**上游[准备度检查][w-readiness]不冻结草稿，freeze 前重新检查证据；[证据包导出][w-capsule]使用已冻结 revision 和 canonical manifest 校验。OmicsOps 已有[科学状态](../../../crates/omicsops-science/src/lib.rs)与[稿件 revision/来源快照](../../../src-tauri/src/workspace_navigation.rs)，现有用户导出仍是 Markdown。

**建议切片：**先为某个保存 revision 生成只读准备度报告：来源 Available/Changed/Missing/Unavailable、Evidence.valid、Provenance.complete、缺失 seed/版本/样本以及原文件位置能否核验。之后再独立导出 publication.md、版本化 manifest.json 与 checksums.sha256；第一版允许全部 reference-only，清楚记录 hash scope、原始数据位置与 omissions。

**验收：**检查不改变稿件或启动计算；输入变化使证据降级；旧来源删除仍保留历史快照；完整 provenance 不自动等于复现通过。导出重新核验 revision，保存失败无可误认的完整包；缺失、受限和 SSH 大数据保持引用。

后续若包含原始字节，先建立已校验内容快照，禁止 live-file fallback。上游[干净复现仅支持 local Runs][w-reproduction]，其文本网络/路径规则不能作为 OS 隔离保证；OmicsOps 的真正复现执行仍须经过本地/SSH 审批和 Run 生命周期。

## W10 探索文件基线和差异

**实施进度（2026-10-14）：**2026-10-14：W10 Task4 `5b4cc5a..19ac40d` 文件显式选择、可选已验证branch关联、窗口内稳定请求、保存正文和有界diff界面完成独立完整审阅，累计43/50 SOURCE。最终focused175、全量Vitest1363+Node22、Web通过；新原生恢复构建显式exit0/release5m27/NSIS1，前次相同最终源码构建成功输出但终态exit遗失，未冒充exit0。Rust1889通过/18ignored、Python48、fmt为相关未变字节的先前实际通过；候选/逆向/重放/提交后RAW11、外部7文件与两个兼容残留保留证明通过。三项非阻断Minor（JSX可读性、延迟diff/分页覆盖、构建警告）交最终全分支审阅。Windows手工smoke受用户STOP未执行，CI/真实SSH/模型/R/Micromamba/PBMC及最终clean-HEAD/整分支交付检查仍非本任务通过项。

**实施进度（2026-10-13）：**W10 Task3 已完成独立审阅：source 783ee03、fix1 e82aed0；FULL审阅指出失败读取未计预算，fresh scoped复核确认diff I1及同类capture C1均已解决且无新增阻断。两个消费者按opened size在读取前预留并传递精确payload额度，失败不返还，W02 EOF probe单独保留；额外保守省略/回归成本已记录。最新6项预算测试包含于baseline38项通过，fmt/diff通过；初始baseline32与DTO59为此前字节证据，DTO未在本轮重跑。实际RED宽filter为14通过/7失败，其中6预算因果失败、1既有毫秒排序fixture精度错误（仅修测试）。全请求work/序列化hunk预算、Unreadable余量不足与顺序非原子限制已同步；Task4 UI/完整交付检查尚待完成，GUI smoke仍未执行。累计42/50源码任务已独立审阅，不等于完整产品/真实环境验收。

**实施进度（2026-10-13）：**2026-10-13：W10 Task2 `f07d9a3` 安全捕获/blob/保存正文/删除清理及 `4a6eb14` 三项修复完成完整与定向独立审阅，累计41/50 SOURCE。敏感文件名在读取前降引用，V4历史引用严格校验类型/schema/有效性/producer状态，open-time SourceChanged整次拒绝；既有Unavailable仍为Unreadable。修复因果RED为纯策略0/1、capture15/2、Store0/2；最新capture17/pure5/saved4/Store11共37通过，fmt/diff及精确候选/逆向/replay/提交审计通过。原62是修复前历史检查，未变W02 handle16/table11/delete2沿用此前结果；完整/clean-HEAD/真实验收未完成。7外部RAW保留；六处working initializer中五处提交，一处外部测试内W10兼容initializer明确未暂存，与既有W04残留分开。diff/UI仍待Tasks3–4，不能称全部文件基线功能已交付。

**实施进度（2026-10-13）：**Task1 `5085908` 完整独立审阅通过，Store9/branch9/native DTO1/fmt/diff通过。保存的是有界metadata、原始immutable manifest/hash及请求identity；分支删除只清空当前关联，不改原manifest。宿主正文捕获/blob生命周期、diff和UI尚待Tasks2–4，不能称文件基线已完整交付。

**参考事实：**上游[探索工作区][w-exploration]对小文件做内容快照、大文件保留引用且不跟随 symlink。OmicsOps 的[会话分支](../../../src-tauri/src/conversation_branches.rs)复制的是有来源的会话材料，未隔离项目文件。

**建议切片：**仅为用户显式选中的脚本/小产物记录基线哈希、恢复快照和变化比较。大型数据与 SSH 文件优先保留引用，不自动同步；此能力可作为 W09 后续包含字节的基础。

**验收：**Windows junction/reparse point、符号链接、遍历和源目录逃逸被拒绝；快照预算有界；源文件变化不会冒充原基线；删除分支不会删除主项目文件；恢复写入需明确目标及现有审批。

未来独立执行另建 ExecutionContext 与授权，不从会话分支继承副作用权限。文件复制和路径范围检查不等于 OS 沙盒。

## W11 可选择的项目迁移

**参考事实：**上游[迁移 manifest][w-transfer]明确 remote references 保留但不重连、机器状态排除；[Store 迁移实现][w-transfer-store]排除全局设置、凭据、execution contexts 与 runtime bindings。OmicsOps 已有[显式同步选择](../../../crates/omicsops-core/src/sync.rs)和 SSH 文件传输，没有整项目便携导入入口。

**建议切片：**先提供只读迁移预览：选定项目的安全 metadata、显式选中文件、大小/哈希、排除理由、远端引用及需要重新配置的能力。下一切片才导出并导入全新项目，保留 ID 映射与审计，避免复制整个共享 SQLite。

**验收：**只处理选中项目；默认不带大型原始数据；拒绝 Windows 保留名、驱动器、UNC、遍历、junction 和覆盖冲突；按预算列出 omissions；新项目不自动连接 SSH、不复用审批/job handle、不重跑不确定派发。

keyring 引用不等于可移植授权；资料中的提示词与路径也须按现有脱敏边界处理。云中继、常驻同步和跨设备活跃运行恢复另行评估。

## W12 收起和恢复会话

**实施进度：**2026-10-15：W12 Task4 source67257de、fix4e5db85已完成独立FULL及R1定向复审，I1–I3全部关闭，无新增问题，累计47/50 SOURCE。精确打开Shelved会话、原request窗口内重试、receipt后当前状态重读、默认Visible导航和搜索/#刷新、运行中保留transcript/draft与实际窗口Escape集成完成。FULL发现旧exact-open覆盖Restore、非active操作压掉active getter、async选择异常未处理；真实递延/拒绝RED后，修复将列表与active读所有权分离，保持branch精确打开并安全处理当前scope错误。最终closest374/Web(typecheck+Vite)/diff通过；修复前完整Vitest1409项+Node桥接22项（本地Node24.16）/native release+NSIS1构建通过，Rust1914通过/18ignored、Python48、fmt为相关未变后端阶段，不冒充最终fix字节全量验收。最初Cargo终态遗失UNKNOWN、后续阻塞mock的已确认进程Ctrl-C中断与正常访问重跑、Python WinError5恢复、两个设置Escape失败后独立/全量恢复但根因UNKNOWN、测试fixture修正均保留。原生E0432后仅备份并清理验证过的target/release core包21文件/13,703,382字节后普通构建通过；旧smoke路径dep-info支持缓存问题但未证明原rustc extern/根因，没有源码/mtime或应用控制绕过。五项正式裁决保留原理由和成本：默认resume空态区别于禁用恢复的新建偏好；独立不可变重试和额外显式操作；仅中断已确认阻塞验证的重跑成本；更新事件完整Visible列表查询及旧标题修复开销；仅core编译缓存重建成本。source11/fix2候选、逆向、重放、提交后RAW/protected7/两个工作区兼容残留/外部差异/空index证明通过。M1 native cursor、M2构建警告、M3设置Escape UNKNOWN留待最终整分支审阅。用户物理Escape STOP下Windows手工及真实SSH/模型/R/Micromamba/PBMC未执行，组合Step4保持未勾选；后续all50工作树与干净归档全量、CI和最终整分支验收不属于本任务通过项。

**实施进度：**2026-10-15：W12 Task3 类型API/收起集合 source54a3318、fix4f788b1 已完成FULL及定向I1独立审阅，累计46/50。四个API保持精确载荷和历史receipt语义；集合按Shelved/default50分页，打开与恢复分离，使用行ID/revision，查询/刷新/恢复回首页与立即window Escape。FULL发现重复query/page A→B→A可复活旧action，八个真实递延回归先失败后修复，现以单调generation+唯一operation ID保护success/error/finally及新pending。最终API7+dialog17+overlays3共27项通过，noEmit/fmt/diff通过；先前19项是较早阶段，重复运行不累加。初始缺API/组件RED、Escape静态fixture13/14后改真实父层先开子层、locale TS2322九项修正均分别记录。source7/fix2候选/逆向/重放/提交后及protected7/两个兼容残留/外部差异/空index证明通过。UI拒绝超界next_offset并提示缩小搜索不等于修复native cursor Minor；应用整合、全量/clean HEAD与人工真实验收仍待Task4/最终阶段。

**实施进度：**2026-10-15：W12 Task2 展示过滤/精确来源命令源码已独立审阅通过（source 8f20cc5 + separate fmt bd6a4ec；FULL spec compliant / quality Approved），累计45/50。四个宿主命令保持 Store-only，Visible 展示/用户消息时间排序与 ALL 精确引用分离，直接读取只投影安全标题而不写回；历史 receipt 与当前 getter 分离。最终新测试5、composer19、navigation11（35项独立覆盖），fmt/diff 通过。导航临时文件 persist 在 sandbox 内 OS5 失败，经同操作诊断与相同测试正常提权1/1、完整最近导航11/11通过；原失败保留，无导航代码修改。立即格式化前 RAW4 已捕获，feature4/format3 分别提交，protected7/外部差异/空 index 与 candidate/inverse/replay/postproof 通过。继承 SQL/DTO/CAS/脱敏约束由 Task1 FULL+scoped 闭合；M1缓存 hard-link 警告保留最终审阅，UI Tasks3–4、最终全量/clean HEAD及人工/真实验收尚未执行。

**实施进度：**2026-10-15：W12 Task1 feature3809c25、独立格式提交8a47454及I1修复e4faae6完成完整与定向独立审阅，累计44/50 SOURCE。独立visibility表/DTO、exact owner getter、CAS和历史回执重放、分页前public-title查询及Visible latest已实现；旧ALL/内部latest不变，新默认latest按user时间的差异与O(全部合格标题字节)每页投影成本明确记录。I1发现core窄脱敏不等于native完整public_text，修复将原赋值/PEM纯算法共享到core并保留native原路径；实际修复RED2，最终Store14/core4/native19共37通过，fmt/diff通过。最初Store12/DTO2/latest6/branches9为此前阶段；compile RED67/8错误零执行、后置mutation1失败与真实修复RED分别保留，不混称。预格式候选仅按留存diff重建、不冒充曾捕获RAW；source7/fmt1/fix4及protected7/两个兼容残留审计通过。next_offset越界与缓存警告Minor交最终审阅；宿主/UI、全量/clean-HEAD/native手工及真实验收仍待后续。

**参考事实：**上游[会话可见性实现][w-shelve]和[持久化测试][w-shelve-tests]支持可逆收起，默认列表、搜索与 # 候选过滤；历史、已有引用与导出仍保留。本地当前分支 archived 状态是分支关系语义，没有相同的普通会话可见性入口。

**建议切片：**新增独立可见性偏好和收起集合，复用原项目作用域；过滤在分页前完成，恢复操作幂等，不混用 run 终态、分支归档或科研冻结。

**验收：**迁移可重复且旧记录默认可见；收起不删消息、证据、引用或作业；重启可恢复；跨项目操作被拒绝；活跃任务状态仍可查；立即 Escape 只关顶层集合/菜单。新的搜索与 # 候选规则同步测试。

## 建议交付顺序

第一批仅做 W01 文字草稿隔离，完成回归后单独提交。下一批从 W02 表格预览或 W03 作业列表中选择一个交付；它们没有必要绑定成同一 PR。

W08 的 helper 回归可以独立补齐；场景报告随后验证 W04/W05/W06 的行为。W09 先做只读准备度报告，W10 的文件快照验证完成后再考虑包含原始字节的证据包。W11 先预览、后迁移，W12 独立进行。

可选的小型 UX 改进是[空会话使用提示][w-tips]及[关闭测试][w-tips-tests]：只介绍 OmicsOps 已支持的操作，支持本次启动隐藏与永久关闭。它不占主要科研待办，也不能带入 Wisp 的 WSL/多资源支持承诺。

暂不整体引入 WinUI 前端、全局 research assistant、微信/外部消息通道、桌面 autopilot、任意命令 hooks、云同步和服务端 Agent。它们需要新的宿主、权限或运行边界。现有精确模型目录匹配、system PATH 本地解释器规则、Credential Manager、冻结审批与 unknown 不自动重跑的约束继续适用。

## 本轮实际验证和限制

以下检查针对原有修复提交与当前 OmicsOps 工作区，不能证明上表候选已经实施。

| 命令 | 实际结果 |
| --- | --- |
| cargo test -p omicsops-desktop --lib project_filesystem_ -- --nocapture | 6 项通过 |
| cargo test --workspace | 最终完整运行 1,514 项通过、0 失败、16 项 ignored，退出码 0 |
| npm test | 992 项前端测试与 22 项浏览器扩展测试通过 |
| npm run build | 通过；保留既有 Vite chunk 大小提示 |
| npm run build:desktop | 通过，完成 Windows x64 release 与 NSIS 构建；未安装或分发 |
| cargo fmt --all -- --check | 通过，无纯格式修改 |
| git diff --check | 通过 |

完整 Rust 检查先后遇到两个旧编译产物路径问题：http_feature_contract 使用了已不存在的旧 worktree manifest；Store 的源码边界检查同样读取旧编译时 manifest 路径。核对当前 manifest 离线解析成功后，触发相关测试重新编译，两个定向测试均通过；再完整运行得到上述最终结果。只更新了编译输入的文件时间，没有修改这两个测试的源码，也没有把失败结果记作通过。

当前未配置一次性模型/SSH/PBMC/订阅验收参数，因此未执行对应 ignored 测试。上游测试、真实模型/SSH/R/Micromamba/PBMC 流程、原生 Windows GUI smoke 和 macOS smoke 均未执行。构建、模拟工具与确定性测试不等于生产科研端到端验收。

未来每个实现切片都需更新相近测试，运行仓库默认完整检查；DTO、桌面组合或打包资源变化还需桌面构建。新增覆盖层必须测试打开后立即 Escape，一次只关闭视觉最顶层。新增协议/持久化需独立说明向后兼容与幂等迁移；导出/恢复要核验来源、权限和状态。依赖锁文件若变化，额外执行 npm ci。

[w-drafts]: https://github.com/xuzhougeng/wisp-science/blob/2c1ad2e1ee2e600be22c08f826490b1c627c71b4/ui/src/main.rs#L2317-L2346
[w-previews]: https://github.com/xuzhougeng/wisp-science/blob/2c1ad2e1ee2e600be22c08f826490b1c627c71b4/ui/src/app_support/previews.rs#L524-L545
[w-pdf-tests]: https://github.com/xuzhougeng/wisp-science/blob/2c1ad2e1ee2e600be22c08f826490b1c627c71b4/ui-tests/tests/native-rich-preview.spec.ts#L21-L59
[w-preview-sandbox]: https://github.com/xuzhougeng/wisp-science/blob/2c1ad2e1ee2e600be22c08f826490b1c627c71b4/ui/src/api.js#L3284-L3330
[w-jobs-ui]: https://github.com/xuzhougeng/wisp-science/blob/2c1ad2e1ee2e600be22c08f826490b1c627c71b4/ui/src/app_support/runtime.rs#L3298-L3360
[w-jobs-poll]: https://github.com/xuzhougeng/wisp-science/blob/2c1ad2e1ee2e600be22c08f826490b1c627c71b4/crates/wisp-runs/src/run_context/remote.rs#L1008-L1044
[w-jobs-cancel]: https://github.com/xuzhougeng/wisp-science/blob/2c1ad2e1ee2e600be22c08f826490b1c627c71b4/crates/wisp-runs/src/run_context/remote.rs#L894-L956
[w-inspect]: https://github.com/xuzhougeng/wisp-science/blob/2c1ad2e1ee2e600be22c08f826490b1c627c71b4/crates/wisp-runtime/src/manager.rs#L434-L516
[w-compact]: https://github.com/xuzhougeng/wisp-science/blob/2c1ad2e1ee2e600be22c08f826490b1c627c71b4/crates/wisp-core/src/context.rs#L1793-L1905
[w-trace]: https://github.com/xuzhougeng/wisp-science/blob/2c1ad2e1ee2e600be22c08f826490b1c627c71b4/src-tauri/src/trajectory.rs#L177-L379
[w-trace-export]: https://github.com/xuzhougeng/wisp-science/blob/2c1ad2e1ee2e600be22c08f826490b1c627c71b4/src-tauri/src/trajectory_export.rs#L555-L603
[w-seed]: https://github.com/xuzhougeng/wisp-science/blob/2c1ad2e1ee2e600be22c08f826490b1c627c71b4/src-tauri/src/seed.rs#L376-L491
[w-eval-suite]: https://github.com/xuzhougeng/wisp-science/blob/2c1ad2e1ee2e600be22c08f826490b1c627c71b4/crates/wisp-cli/eval-suites/offline-v1.yaml#L1-L34
[w-eval]: https://github.com/xuzhougeng/wisp-science/blob/2c1ad2e1ee2e600be22c08f826490b1c627c71b4/crates/wisp-cli/src/eval.rs#L992-L1040
[w-skill-tests]: https://github.com/xuzhougeng/wisp-science/blob/2c1ad2e1ee2e600be22c08f826490b1c627c71b4/python/test_skill_sidecars.py#L34-L265
[w-readiness]: https://github.com/xuzhougeng/wisp-science/blob/2c1ad2e1ee2e600be22c08f826490b1c627c71b4/src-tauri/src/publication_freeze.rs#L2724-L2780
[w-capsule]: https://github.com/xuzhougeng/wisp-science/blob/2c1ad2e1ee2e600be22c08f826490b1c627c71b4/src-tauri/src/publication_capsule.rs#L362-L406
[w-reproduction]: https://github.com/xuzhougeng/wisp-science/blob/2c1ad2e1ee2e600be22c08f826490b1c627c71b4/src-tauri/src/publication_reproduction.rs#L680-L710
[w-exploration]: https://github.com/xuzhougeng/wisp-science/blob/2c1ad2e1ee2e600be22c08f826490b1c627c71b4/src-tauri/src/exploration_workspace.rs#L474-L608
[w-transfer]: https://github.com/xuzhougeng/wisp-science/blob/2c1ad2e1ee2e600be22c08f826490b1c627c71b4/src-tauri/src/project_transfer.rs#L349-L377
[w-transfer-store]: https://github.com/xuzhougeng/wisp-science/blob/2c1ad2e1ee2e600be22c08f826490b1c627c71b4/crates/wisp-store/src/project_transfer.rs#L22-L25
[w-shelve]: https://github.com/xuzhougeng/wisp-science/blob/2c1ad2e1ee2e600be22c08f826490b1c627c71b4/crates/wisp-store/src/sessions.rs#L27-L68
[w-shelve-tests]: https://github.com/xuzhougeng/wisp-science/blob/2c1ad2e1ee2e600be22c08f826490b1c627c71b4/crates/wisp-store/tests/shelved_sessions.rs#L14-L95
[w-tips]: https://github.com/xuzhougeng/wisp-science/blob/2c1ad2e1ee2e600be22c08f826490b1c627c71b4/ui/src/main.rs#L13058-L13073
[w-tips-tests]: https://github.com/xuzhougeng/wisp-science/blob/2c1ad2e1ee2e600be22c08f826490b1c627c71b4/ui-tests/tests/ui.spec.ts#L15087-L15110
