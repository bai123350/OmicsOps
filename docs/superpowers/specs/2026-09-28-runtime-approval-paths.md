# 常规项目路径审批与可解释的拒绝原因

## 本次证据

审批调用 hash 前缀 `2603bd93e440` 已从本机事件数据库以 SQLite URI `mode=ro` 关联。代码为 6,381 字节，SHA-256 为 `41787bb779023e5e5144b65623c7dd3806663d8e5495716ec019c458538b99a2`。原始参数只存于 TEMP，未进入仓库，未执行 Python 研究代码、访问网络或修改事件。

当前生产 Rust 分类函数返回 false。按 Python AST 的源区间替换原文中的单个表达式、保持其余原文不变，得到：

| 静态分类输入 | 结果 |
| --- | --- |
| 原始调用 | false |
| 仅以常量替换 `os.makedirs(...)` | false |
| 替换 makedirs，并把输出文件名表达式换为固定项目相对路径 | true |
| 替换 makedirs 和 getsize 调用 | true |

因此已证实两个独立拒绝点：`os.makedirs('results/literature', exist_ok=True)` 不属于允许的方法；`out_path` 来自 `%s` 模板和日期字符串，现有仅识别字面量、简单变量与 join 的路径证明无法处理，继而拒绝 `os.path.getsize(out_path)`。日期链为 `now = datetime.datetime.now(datetime.timezone.utc)`、`today = now.strftime('%Y-%m-%d')`、`today.replace('-', '')`。这次真实原文没有其他拒绝点；不能把上一条 exists 修复当作本次已修复。

## 目标和边界

Local 与 SSH 在 RiskBased 下，对可静态识别的项目内建目录、日期命名的结果文件和元数据检查不再反复审批。未知路径来源、删除、进程启动、外部路径及不支持的能力仍审批。宿主作出判定，忽略模型 `analysis` 中的安全自述。

这是 best-effort 审批路由，不是 Python 沙箱或完整静态分析器，也不证明任意依赖包、既有内核状态或文件系统符号链接安全。不能将其描述成“所有普通 Python/R 永远免审批”。本次仍保留冻结能力、精确调用 hash、既有拒绝、Plan、MCP 和浏览器授权边界。不得自动批准旧 pending，不改数据库或事件契约，不延长执行超时。

## 小型值证明

在现有 `runtime_approval.rs` 内用有类型的已知值替代只有“安全路径名称”的集合，不新增 Python 解释器依赖，也不执行代码进行分类。不因为文件长而另拆框架。

证明至少区分：项目相对路径/已知字符串、日期对象、安全文件名片段。布尔值与文件大小不属于路径。路径拼接、日期格式化和变换可组合，但每一层必须消耗完整表达式，并沿用深度和输入长度上限。

允许本次需要的直接 `import os`、`import datetime`，包括普通逗号分隔导入。别名导入、from 导入、动态属性、裸方法引用、未知绑定仍保守审批。日期类型不能仅由变量名 `now` 或 `today` 推断。

1. 日期来源：在模块未被重新绑定或修改的前提下，识别 `datetime.datetime.now()` 与 `datetime.datetime.now(datetime.timezone.utc)`。可组合 `known_datetime.strftime(format)`；`format` 必须是字面量，只含 `%Y`、`%m`、`%d`、`%H`、`%M`、`%S`、`%f` 及 ASCII 字母、数字、`-`、`_`。未列出的日期指令、斜杠、反斜杠、冒号、点、区域化名称等不产生安全片段证明。
2. 片段变换：已证明的安全文件名片段可调用 `replace(old, new)`，仅允许两个字面量参数，old 非空，new 可空，字符仍限 ASCII 字母、数字、`-`、`_`。不能把未知字符串通过 replace “净化”为可信片段。
3. 路径模板：允许字面量 `%s` 模板与一个安全片段，或数量精确匹配的片段元组。模板中其他格式指令、宽度、映射、星号、额外参数、未知表达式不支持。以固定非路径字符占位替换各 `%s` 后，完整结果必须通过现有项目相对路径校验。模板必须提供固定相对目录前缀，不能仅由动态片段承担根路径；模板插值不能产生路径分隔符。不要特殊匹配研究名称、文件名或固定日期。
4. `os.path.join` 可消费证明成立的相对路径和安全片段；`os.path.getsize`、`os.path.exists` 恰好消费一个证明成立的路径，返回值不能登记为路径。避免把目前扁平化的 f-string token 当作已证明字符串；本次不扩大到任意 f-string、`.format` 或字符串执行。
5. `os.makedirs` 接受一个已证明的项目相对路径，可带唯一的 `exist_ok=True` 或 `exist_ok=False`；没有该参数也允许。不支持未知关键字、`mode`、额外位置参数、`*args`、`**kwargs` 或动态布尔值。它和元数据操作共用同一套路径证明；不顺带允许 chmod、删除、移动、符号链接等方法。

顶层完整简单赋值才能建立证明。重新赋值、增强赋值、解构、循环变量、with/as、def/lambda 参数、class、match/case、del、别名与导入等复杂绑定必须撤销涉及的证明；条件或缩进赋值不建立新证明。日期模块及属性发生赋值、遮蔽或不明传递时不能继续声称其产生可信日期。保留现有绑定反例，不能因类型增加而绕过它们。实现可保守撤销更多证明，不得通过忽略复杂语法扩大权限。

## 有限原因

保留 `ordinary_runtime_call_is_low_risk(&Value) -> bool` 供现有调用者使用，由统一分析结果派生。增加 crate 内分析结果与固定原因枚举，至少区分：输入/语言不支持、后台或 capture 路径、代码无法解析、明确危险操作、外部/受保护路径、不支持的 OS 操作、项目路径或绑定无法证明。结果不能包含源码片段、路径、URL、参数值或模型推断文字。

一次调用可有多个原因；至多收集四个去重固定原因。实现不必为了收集原因恢复不明确语法的含义，解析失败可以单独结束。允许的新模式真正通过分类，不可仅替换审批文案。

在 `ToolPortV4` 增加默认返回 None 的同步宿主 hook：`fn risk_based_approval_reason(&self, call: &ToolCallV4) -> Option<String>`。Desktop 仅对 `runtime.execute` 返回固定原因的用户可读组合。core `approval_request` 只在 RiskBased 的普通运行时请求中使用该原因，继续通过已有 `ToolApprovalRequestV4.reason` 保存和显示；Request、Plan、浏览器、MCP 原有文案与优先级不改变。低风险调用因其他已有门禁要求审批时，None 回落原效果原因，不能伪称路径分类失败。原因不能改变授权判定。

## 测试与交付

加入脱敏但完整结构的 fixture：逗号导入、公开 GET、UTC now、strftime、replace、%s 文件名、makedirs、open/write 和 getsize 同时存在；在修改前分类 false，修改后 true。保留旧 exists/join/getsize 测试。

正例覆盖不同变量名、目录和日期格式、默认与显式 exist_ok、嵌套 join、单片段与片段元组。负例覆盖危险 OS、删除、绝对/父级/受保护路径、远端响应/配置/环境等未知片段来源、日期格式含分隔符、参数数量和未知关键字、日期与模块重新绑定、条件/def/循环等复杂绑定、布尔/大小伪装成路径、扁平 f-string 假证明。有限原因通过 classifier、desktop hook、core 请求三个层次验证，不泄露参数内容，Local/SSH 一致。

最终以完整真实原始参数再次调用生产 Rust 分类函数确认 true，并生成同形危险版本确认仍 false；此过程仅分类，不能执行原脚本。记录通过/失败，不把它当成真实科研或 SSH 端到端验收。执行 workspace Rust 测试、前端测试、Web build；涉及 core/desktop 接线后执行 desktop build。按 AGENTS.md 单独提交纯格式变化。各项完成后单独 commit。
