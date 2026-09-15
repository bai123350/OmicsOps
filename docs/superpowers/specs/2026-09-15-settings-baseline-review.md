# Settings 基础页调用核验

日期：2026-09-15。依据source-audit和已批准的master基础适配范围，核对Session、Models、Environments、Skills、Browser、Connections当前源码。本文是代码阅读结果，未新增执行产品测试；后续实现提交和最终验收结果以master为准。ACP、HTTP/OAuth MCP、WSL导入和自定义本地解释器不在本次缺口清单。

| 页面 | 可验证的既有调用 | 仍需修复或补齐 |
|---|---|---|
| Session | `src/features/settings/AgentSettings.tsx:11`读取/保存5项设置；`src-tauri/src/agent_settings.rs:7`读取Store并兼容缺省；`agent_v4.rs:4127`加载运行限制，`follow_up_questions.rs:106`消费建议开关 | 未发现基础范围缺失能力；最终验收待做。 |
| Models | `SettingsPanel.tsx:98/130/141/191`连接保存、探测、模型目录和配置编辑；`src-tauri/src/model_commands.rs:143/152/206/230`提供真实列表/保存/探测/发现，凭据经现有vault | 未发现基础范围缺失能力；模型预算、推理和目录精确匹配继续沿用现有契约；最终验收待做。 |
| Environments | `SettingsPanel.tsx:446`的RemoteSettings连接SSH创建/编辑、连接测试、主机指纹确认、项目绑定 | system探测及入口未接；项目绑定缺busy/错误反馈；没有新建第二条连接的表单重置入口。 |
| Skills | `SettingsPanel.tsx:442`的SkillCard呈现名称/版本/摘要片段/能力并调用启停；`src/tauri-api.ts:288`一组API连接list/import/enable | 详情、文件读取和安全删除缺少设置API/UI；启停Promise未捕获，失败无反馈、可重复点击。 |
| Browser | `BrowserSettings.tsx:173/232/256/274`连接配置加载保存、session启动及授权撤销；`src-tauri/src/browser_commands.rs:46`保存失败有运行配置回滚 | 首次配置读取失败后仍可保存默认配置，需要成功baseline门控。 |
| Connections | `SettingsPanel.tsx:311`起连接stdio配置保存、检查/发现、启停及逐工具授权；`p1_commands.rs:402`起保留声明变更失效授权语义 | 编辑现有literal环境绑定会清空草稿值，保存校验拒绝空值，无法仅改名称并保留原环境值。 |

## 最小补齐与验证

1. **system探测必须是真实操作。** `src-tauri/src/agent_v4.rs:344`明确backend列表只检查配置；`configured_process_backend:384`的python_status/r_status固定为unverified。RuntimeDialog仅消费此状态并插入环境准备草稿，不能证明已找到解释器。新设只读system诊断命令，复用 `LocalEnvironmentPortV4::check_interpreter:5848` → `program_available:8064` 的真实检查，固定python/Rscript并用模拟命令运行器测试；General和Environments使用同一命令。显示找到/未找到/检查失败，不宣称项目依赖可运行。
2. **SSH动作完整反馈。** `SettingsPanel.tsx:489`绑定按钮直接`void onBind`，未设置busy或捕获错误；保存连接后form.id不重置且无新建按钮。加明确绑定handler及新建表单入口；测试绑定失败保留输入、禁重复、成功后能创建不同UUID的第二条连接。项目切换后的绑定草稿也应重置至新项目真实值。
3. **Skills真实详情与安全删除。** `skill_commands.rs:263/368/422`已有内部文本资源读取，但未暴露设置命令；`omicsops-store/src/lib.rs::delete_skill_package`只删记录，不能直接当安全卸载。新增按skill ID解析受管路径的有界详情/文件读取和所有权+摘要保护删除；插件拥有项交Plugins处理，不删除外部源目录或改过的文件。测试路径穿越/链接/摘要冲突/共享来源保护及立即Escape只关闭详情或确认。SkillCard启停加busy/try-catch，失败保持原状态和可重试错误。
4. **Browser首次加载失败禁止保存。** `BrowserSettings.tsx:173`失败后会结束loading，而`:429`保存按钮仅以loading/saving门控。加入成功读取baseline校验（保存handler也校验），测试加载失败时不能调用保存、重试读取成功后才可编辑保存。
5. **MCP环境绑定保留语义。** `SettingsPanel.tsx::editMcpServer`将literal值置空，`saveServer`拒绝空值并提交整个env_bindings；`p1_commands.rs::mcp_profile_from_request`按完整声明更新。需显式keep/replace/remove语义，宿主重新解析原绑定，不依赖回显秘密。测试只改名称仍保留literal环境值且不无故撤授权；真实修改启动声明仍按既有规则撤销授权。

现有Session/Models/Browser/MCP组件测试覆盖多项保存、失败、目录和Escape行为，但本核验没有重跑，不能替代最终完整检查或真实Windows/SSH/model验收。上述缺口已由主代理安排实现；后续改动不得将本审计旧行号误作最新实现证明。
