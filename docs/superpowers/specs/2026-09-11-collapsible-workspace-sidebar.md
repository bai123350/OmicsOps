# 可折叠工作区侧栏：Wisp 设计对照

参考仓库：<https://github.com/xuzhougeng/wisp-science>，读取版本 `3628a4209e494ba6fbef1095bb964782f7d2c430`。本次按其已核实的交互与内容语义适配 OmicsOps；不将截图文字当作功能实现。

## 来源与修正

- [main.rs，右面板标签与加号菜单](https://github.com/xuzhougeng/wisp-science/blob/3628a4209e494ba6fbef1095bb964782f7d2c430/ui/src/main.rs#L14374)：菜单勾选表示该标签已经打开，可以同时勾选多个。点击项确保标签打开并激活；关闭由标签上的叉号完成。标签支持拖动排序、横向滚动。旧实现的单选菜单已移除。
- [notebook.rs](https://github.com/xuzhougeng/wisp-science/blob/3628a4209e494ba6fbef1095bb964782f7d2c430/ui/src/notebook.rs)：Notebook 是对话代码及运行结果的只读投影。跳过 CSV/TSV/FASTA 数据围栏，已经提交工具的同语言同代码合并为工具单元；保留每次真实调用，生成源代码明确标记未执行。
- [main.rs，各内容视图](https://github.com/xuzhougeng/wisp-science/blob/3628a4209e494ba6fbef1095bb964782f7d2c430/ui/src/main.rs#L14800)：Agents 显示委派任务，Highlights 来自收藏的对话摘录，Files 为文件浏览，Environment 为执行上下文与运行时，Side chat 为独立且带证据引用的问答。
- [right-pane.css](https://github.com/xuzhougeng/wisp-science/blob/3628a4209e494ba6fbef1095bb964782f7d2c430/ui/src/styles/right-pane.css)：紧凑标签、加号入口、圆角纵向菜单、卡片和代码单元的视觉层次。

## OmicsOps 数据映射

| 入口 | 当前真实数据与交互 |
| --- | --- |
| Artifacts | 登记产物列表/网格、类型、大小、核验状态、run 与 checksum；通过现有图片接口预览。无数据时不展示固定 UMAP 示例。 |
| Agents | 当前会话的 delegation_graph_started / node_finished / graph_finished 事件，展示任务目标、依赖、状态、错误及返回值。Plan 不冒充 Agents。 |
| Notebook | assistant 代码围栏以及 runtime.execute/runtime.python/runtime.r/shell 调用和返回；支持复制代码、展开输出。正式研究记录保留独立入口。 |
| Highlights | 当前没有收藏摘录的持久化 API，菜单禁用并提供原因；不伪造空集合或用 Memory 代替摘录。 |
| Files | 保留真实远端文件树、刷新、上传、下载与同步操作；当前 API 不支持 Wisp 的本地/远端文件源切换，因此不提供虚假选择器。 |
| Provenance | 工具返回事件的 run、call、sequence、时间、来源引用、模型侧返回文本、结构化工具返回和事件哈希；支持筛选。不再只统计 Notebook 中的证据 ID。 |
| Environment | 现有计算后端的 local/SSH 类型、隔离模式、解释器探测、当前环境及交互内核。可找到解释器不代表依赖已经安装。 |
| Side chat | 当前没有独立的带证据问答 API，菜单禁用并提供原因；不使用主 Agent 请求冒充独立侧聊。 |

普通会话默认收起；初始已打开的标签为 Artifacts、Agents、Files、Environment，与用户截图的勾选组合一致。右上图标仅负责展开/收起；展开后加号显示菜单。进入 Plan 模式自动打开独立 Plan 标签；关闭后可以通过“查看 Plan”重新打开。权限菜单中的“记忆与研究记录”打开独立研究记录标签。

关闭最后一个标签会收起侧栏，再次展开恢复 Artifacts。关闭活动标签选择相邻标签。窗口级 Escape 依次关闭菜单、预览覆盖层、侧栏，单次仅关闭顶层。窄窗口用覆盖式侧栏。该变更不增加数据库、DTO、审批或执行接口。

## 验证

自动化覆盖：多标签勾选、去重打开、关闭与重新打开、Plan 恢复、Escape 分层、Notebook 代码投影和执行区分、跨 Run 的 call ID 隔离、不确定派发、后台作业未验证完成、委派结果、原始工具来源，以及原有文件/图片/会话隔离回归。

执行默认完整检查 `cargo test --workspace`、`npm test`、`npm run build`；并通过 `npm run build:desktop` 生成当前修改的 exe。浏览器演示环境用于实际布局与菜单 smoke；不等同于真实模型、SSH、安装或生产端到端验收。

手工 Windows smoke：展开/收起右侧面板，添加 Notebook、关闭活动与最后一个标签；查看多项勾选；拖动标签；在 Plan 审核中关闭后重新打开；验证图片预览；在窄窗口检查菜单滚动和 Escape。真实模型、SSH 与安装验收本次未执行。

实际结果：Rust workspace 测试通过（真实环境 ignored 测试未执行）；Vitest 166 项与扩展 22 项通过；Web 与 NSIS 桌面构建通过。独立审查提出的不确定后台派发、结构化来源遗漏和不支持图片预览问题已修复并增加回归测试。普通窗口菜单及 Notebook 空状态完成浏览器 smoke；最终窄窗口复核因本地预览连接中断未完成，仍需 Windows 手工确认。
