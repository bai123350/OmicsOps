# 帮我批准：默认自动批准，本地删除询问

## 用户要求

用户明确要求：未知或危险操作也自动批准，只有删除本地文件需要询问。这是对之前风险审批策略的主动修改，不再通过扩展“低风险代码白名单”满足要求。文件夹删除同样询问。只改变用户选择“帮我批准”后的新调用策略，不改变 Codex 开发工具自身的权限。

## 可观察行为

“帮我批准”采用 `auto_approve_except_local_deletion`。宿主检测到本地文件或目录删除时请求本次调用批准，其余未知或危险操作默认自动批准。普通写入、覆盖、截断、进程启动、网络及远端修改不因风险等级单独询问。SSH runtime 的直接文件删除属于远端操作；Local 和映射本机工作区的 Docker/Podman 删除属于本地删除。

这是有限删除识别，不是文件保护沙箱。任意动态代码、导入模块副作用、混淆、反射、间接子进程、MCP 内部实现或远端反向访问本机可能隐藏删除；未知仍按用户的新授权自动批准。普通覆盖写入也可能损坏旧内容。界面应说明这项限制，不能声称所有删除均被阻止。

“请求批准”保留现有手动策略，“先做计划”保留计划批准步骤。禁用工具、未冻结能力、参数错误、MCP 未配置/未启用/未批准启动、失效目录或 schema、浏览器目标及会话不合法等仍不能执行；自动批准不等于启用未授权能力。当前浏览器功能开关保持原样。

## 兼容性与审批次序

新增 `ApprovalPolicyV4::AutoApproveExceptLocalDeletion`，wire value 为 `auto_approve_except_local_deletion`。保留 `RiskBased` 的 serde 默认、默认省略规则与旧判定，避免改变历史冻结配置和批准哈希；新值必须显式持久化。只有新 composer 配置构造时把遗留 `risk_based` 选择升级为新值，不能升级已冻结运行、恢复请求、已排队冻结配置或已批准计划。

新增默认返回 None 的宿主接口 `local_deletion_approval_reason(&self, call: &ToolCallV4) -> Option<String>`，由 ToolExecutorV4 经授权过滤后的 ToolRegistry 转发至 ToolPortV4。Registry 先验证工具及参数。新策略先保留已有精确调用的 pending/denied/approved 决定，再依据删除检测选择是否询问；检测排在只读标签、会话授权和浏览器/MCP 普遍授权复用之前。本地删除批准只用于绑定 hash 的那次调用，不能生成永久删除授权。

Desktop 执行器的 MCP/浏览器授权接入同一新策略，避免核心自动批准后执行器仍以旧风险规则拒绝。仅对当前合法调用提供批准，不写入新的持久授权，不覆盖启用、启动批准、目录/schema、目标与会话检查。旧策略、历史审批和精确调用绑定保持原样。

## 有限删除检测

独立宿主模块负责新策略的正向删除检测，不改写旧 `runtime_approval` 的低风险语义。不执行 Python/R 或命令来分类，复用现有 RustPython 解析依赖。分析预算有限，无法解析或超限视为未知并按新策略自动批准，不能偷偷恢复“未知要求审批”。原因使用固定文字，不回显路径、源码或凭据。

- Python：直接及常见 import alias 的 `os.remove/unlink/rmdir/removedirs`、`shutil.rmtree`、`pathlib.Path.unlink/rmdir`。
- R：`unlink`、`file.remove`。
- 在执行入口可见的字面命令：Python `subprocess`/`os.system` 与 R `system/system2/shell` 中的 rm、rmdir、unlink、cmd del/erase/rd、PowerShell Remove-Item 及常见别名。识别命令边界，不能把日志字符串、注释、HTTP DELETE 或 pandas.drop 当文件删除。
- MCP：显式文件/目录删除工具名，或明确 delete/remove/trash 动作及文件路径参数。MCP stdio 在本机，不随 SSH compute 变为远端；无法确认目标本地性的明确文件删除用“可能删除本地文件或目录”询问。未知或一般危险 MCP 调用自动批准，readOnlyHint 不能豁免明确删除。

## 界面和验证

“帮我批准”说明为“自动批准操作，仅检测到本地删除时询问”；补充“动态代码或第三方工具中的间接删除可能无法识别”。新默认、退出完全访问权限后的 fallback、菜单回调及新 composer 配置使用新策略。保留旧值的类型与历史展示；不将历史风险审批误标为新策略。

回归覆盖：未知和危险但非删除自动派发；本地删除等待审批；SSH 远端删除自动派发；容器本机映射与 MCP 不被 SSH 配置误判；普通日志/列表操作不误报；精确批准/拒绝/恢复不变；已存在授权不能绕过删除；MCP 核心与执行器一致；非法能力不被自动批准放行；历史缺省策略与哈希不变；UI 新选择进入新运行。

按 AGENTS.md 执行定向测试、完整 Rust/前端测试、Web/桌面构建及适用的独立真实模型验收。测试不执行真实删除、不修改用户数据库、不恢复或重跑原研究任务。每项功能完成后提交，纯格式改动单独提交，保留用户的未跟踪 `website/`。
