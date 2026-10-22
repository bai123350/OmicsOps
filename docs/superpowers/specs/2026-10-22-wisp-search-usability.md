# Wisp 参考：检索可发现性与逐项目结果

日期：2026-10-22。

## 对照与选择

本轮参考固定为 Wisp Science 提交
[`4f2957a309377bd605f9509b87361b6e10617575`](https://github.com/xuzhougeng/wisp-science/tree/4f2957a309377bd605f9509b87361b6e10617575)。
阅读源码与文档，没有运行上游应用或测试。

- 上游 [Skills 搜索](https://github.com/xuzhougeng/wisp-science/blob/4f2957a309377bd605f9509b87361b6e10617575/crates/wisp-skills/src/tool.rs)
  统一大小写、常见分隔符，支持连续中文查询。OmicsOps 已具备中文二元／三元
  检索，但将连字符、下划线留在英文词项中，导致自然语言无法命中复合名称。
- 上游 [项目会话搜索](https://github.com/xuzhougeng/wisp-science/blob/4f2957a309377bd605f9509b87361b6e10617575/ui/src/project_chat_search.rs)
  在项目作用域内用 generation 和存活状态保护异步响应。OmicsOps 已有相同
  保护，其跨项目目录检索却等待所有请求结束才发布结果。本轮逐项目发布是
  针对 OmicsOps 的改进，并非声称上游已有相同跨项目实现。

前一轮 W01–W12 的首次范围已实现，本轮只改善现有检索流程，不重做其子系统。

## 行为与边界

### Agent Skills 和记忆检索

`omicsops-knowledge::lexical_terms` 将连字符、下划线与空格一样视为词边界。
查询和被检索文本采用同一规则，名称及章节中的复合词均可发现。
保留整词重叠评分、中文连续词项、结果数量上限、停用过滤和原始来源信息。
`rnaseq` 不等同于 `rna seq`，不引入子串匹配、语义翻译或远程索引服务。

共享该函数的知识库 MCP 搜索 helper 同步获得一致行为，但桌面 Agent 的 MCP
发现实际通过 `context_views` 的目录过滤；本轮没有改变该桌面 MCP 路径。
不会改变模型目录中的精确协议、host、端口与完整 model ID 匹配。

### 工作区全局搜索

打开搜索时项目行立即可见，每个项目的会话／产物／技能目录返回后立即显示。
一个项目尚未完成或失败，不阻塞其他项目的结果。每次发布不可变目录快照，
按现有项目顺序组装结果；全部请求结束后才退出加载状态，失败数量逐项更新。

沿用每批请求的 generation 和清理标记。关闭搜索、重试、项目集合变化后，
旧请求的成功与失败均不可更新新状态。重试仍重新加载当前全部项目，不增加
后台查询或自动重试。

键盘选择绑定结果 key，新结果插入或组顺序移动时，Enter 仍打开用户选中的
条目。修改查询或所选条目消失时回到首个匹配项；无结果时无选中项。
沿用现有窗口级 Escape 堆栈、焦点恢复和 IME 处理。

## 架构与数据影响

修改限定于 knowledge 的既有词法处理和前端全局搜索 hook／对话框。
没有新增依赖、DTO、数据库模式、持久化记录、权限或外部请求。
Skill 冻结、MCP 授权与凭据 keyring 路径保持现有契约。
搜索结果是发现入口，不代表代码已经执行、依赖可用或结论已验证。

## 验证

回归测试覆盖分隔符变体、完整词边界、中文、停用过滤、结果限额、来源保持，
以及未批准 MCP helper 命中仍不能执行；前端使用模拟 Tauri 目录请求验证
逐项目成功／失败、重试／关闭／切换后的陈旧响应和稳定键盘选择。

实际执行记录：

- `cargo test -p omicsops-knowledge`：先以 3 个新增测试复现漏检，修改后 11 项通过。
- `npx vitest run src/use-workspace-search.test.tsx`：新增的 5 项先失败，修复后通过。
- `npx vitest run src/features/workspace/WorkspaceSearchDialog.test.tsx`：新增的
  结果到达后选中项保持测试先失败，修复后通过。
- 联合运行 hook、搜索索引、搜索对话框测试：27 项通过。
- `npm run build`：通过；仍有 Vite 单 chunk 超过 500 KiB 的体积提示。
- `npm test`：129 个前端测试文件、1,518 项测试和 22 项浏览器扩展测试通过。
- `cargo test --workspace`：沙箱外复验退出码 0；2,024 项通过、0 失败、21 项
  ignored（未显式执行）。仍有增量缓存硬链接回退复制及 Windows 链接器信息提示。
- `cargo fmt --all -- --check` 与 `git diff --check`：通过。
- 独立代码审查：未发现可操作问题或阻断项。

首次沙箱内 `cargo test --workspace` 未完成，因既有本机模拟 HTTP 测试长时间
等待而中断。分别复跑确认
`installation_reports_only_the_rename_that_created_the_managed_directory`
在临时目录重命名时报 Windows OS 5 PermissionDenied，
`local_lm_studio_client_does_not_follow_redirects` 连接本机代理时报 OS 10013
PermissionDenied。随后通过自动审批在沙箱外重跑默认 workspace 测试；仅为
本次测试进程把 `127.0.0.1,localhost,::1` 加入 `NO_PROXY`，避免本机模拟 HTTP
经过系统代理。没有修改用户的持久代理设置或测试／产品代码来绕过断言。

手工 smoke：在有多个项目的 Windows 工作区打开 Ctrl+K，加载期间用方向键
选中一个会话或产物，确认新结果到达后 Enter 仍打开同一条目；关闭后重新打开、
切换项目与重试不会显示旧结果。打开搜索后立即按 Escape，确认只关闭顶层且
焦点恢复。真实 Windows GUI、模型与 SSH 验收未执行，离线测试不代替这些验收。

## 后续独立切片

审查还发现 `LocalProjectFilesystemV4::read` 先整文件读取后取 400 行，SSH 对应
路径只有行数限制；超长单行仍缺少字节边界与截断标记。该问题涉及本地／SSH
一致性和宿主组合，应作为独立的有界文件读取变更验证，本轮未修改。
