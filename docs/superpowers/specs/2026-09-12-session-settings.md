# Session 设置与运行行为

依据用户提供的 Wisp Session 截图和 wisp-science 提交
`a705cc2846ed1b01a9c93bbefbd037dac4063700` 的 `src-tauri/src/lib.rs`、
`crates/wisp-core/src/agent.rs`，将设置导航和标题由 Agent 改为 Session。
保留 OmicsOps 设置外壳，使用白色分组卡片、左侧说明、右侧输入/青色开关，
以及底部 Cancel/Save。取消恢复最近一次保存值；Escape 仍关闭最上层设置。

## 参数与兼容性

沿用既有 settings 表键及 AgentIterationSettingsV4 DTO/API，无新增表。
旧的只有 max_iterations 的设置自动补齐以下默认值：

| 参数 | 默认值 | 行为 |
| --- | --- | --- |
| max_iterations | 100 | 普通对话的模型/工具轮数；0 不限；到限无工具总结 |
| auto_continue | false | 输出被明确标记为截断时自动再次请求完整回复 |
| auto_continue_limit | 10 | 一次启动/恢复最多自动续写次数；0 不续写 |
| auto_compact | true | 请求容量预检或显式上下文溢出时允许自动压缩 |
| follow_up_questions | true | 完成后使用该 run 的模型生成三个可选后续问题 |

运行限制和压缩选项在启动/恢复时读取，不改变运行中的冻结计划和审批。
已批准计划继续使用原有预算。普通对话的迭代限制与 96 次工具硬上限的区别
见 `2026-09-12-agent-iteration-summary.md`。

## 隐私与权限说明

设置中的“隐私与权限”页面集中说明现有数据与执行边界，并链接到模型、
Skills/MCP、浏览器和远端计算设置。模型、MCP 或外部服务可能接收提示词、
结果或元数据；浏览器访问的网站会接收正常网页请求。凭据仍只进入现有
Windows Credential Manager/keyring 路径，SQLite 和导出仅保存引用或脱敏信息。

大型远端数据不默认完整同步，项目可保存远端引用、校验和与元数据，文件传输
需要显式操作或选择同步范围。宿主继续负责能力与审批裁决；停止 Agent 不代表
已经派发的远端计算已取消。页面只说明并链接现有能力，不提供未由运行时执行的
全局审批或离线开关。

## 续写、压缩与建议问题

截断响应中的工具参数不执行、不拼接。下一次请求带有有界的不可信部分文本，
要求重新输出完整、自包含的回复；共享续写次数预算。取消、指导、请求容量
检查仍有效；到限总结不自动续写。其他格式错误的独立修复次数不增加。

压缩沿用 OmicsOps 的实际模型上下文预算检查，不宣称 Wisp 的 80% 触发阈值。
关闭开关禁用容量预检和供应商上下文溢出的自动压缩；硬容量检查仍保留。
原始事件保留，压缩不替代科研证据。

后续问题只对完成事件生成，输入为有界研究目标与完成提案答案；请求不包含
工具，超时 30 秒，响应仅接受三个不同的非空短字符串。此辅助请求会使用模型，
不改变 run 状态、事件或科研完成结论。关闭时不请求；失败静默隐藏。
同一个 run 的成功建议在有界内存缓存中复用，并合并进行中请求，避免组件
重挂载重复请求。建议不持久化到数据库；点击仅填入输入框，由用户发送。

## 验证与手工检查

自动测试覆盖旧设置默认值/完整往返、控件联动、取消和 Escape、截断预算、
关闭压缩、建议格式/输入边界/关闭开关/重复请求，以及切换 run 后忽略旧响应。
浏览器使用模拟 Tauri 设置验证实际布局、开关和取消；不将该预览视作模型验收。

完整检查运行 cargo test --workspace、npm test、npm run build 和
npm run build:desktop。真实模型、MCP、SSH、安装后的验收须按 acceptance/README.md
在一次性环境执行并单独记录，不能将构建或模拟测试当作真实验收。

本次实际结果：cargo test --workspace 763 通过、11 ignored、0 失败；
npm test 前端 200 项及扩展 22 项通过；npm run build 通过；
cargo fmt --all -- --check、git diff --check 通过。
独立审查发现的关闭压缩、重复建议请求、完整替代回复与表单联动问题已修复并复核。
真实模型/MCP、SSH 和安装后的验收未执行。

npm run build:desktop 通过，生成 Windows x64 安装包
`target/release/bundle/nsis/OmicsOps_0.1.0_x64-setup.exe`，未执行安装。
