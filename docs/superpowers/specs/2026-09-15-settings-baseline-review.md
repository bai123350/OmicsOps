# Settings 基础页调用核验

日期：2026-09-15。依据source-audit和已批准的master基础适配范围，核对Session、Models、Environments、Skills、Browser、Connections当前源码。本文是代码阅读结果，未新增执行产品测试；后续实现提交和最终验收结果以master为准。ACP、HTTP/OAuth MCP、WSL导入和自定义本地解释器不在本次缺口清单。

| 页面 | 可验证的既有调用 | 仍需修复或补齐 |
|---|---|---|
| Session | `src/features/settings/AgentSettings.tsx:11`读取/保存5项设置；`src-tauri/src/agent_settings.rs:7`读取Store并兼容缺省；`agent_v4.rs:4127`加载运行限制，`follow_up_questions.rs:106`消费建议开关 | 未发现基础范围缺失能力；最终验收待做。 |
| Models | `SettingsPanel.tsx:98/130/141/191`连接保存、探测、模型目录和配置编辑；`src-tauri/src/model_commands.rs:143/152/206/230`提供真实列表/保存/探测/发现，凭据经现有vault | 未发现基础范围缺失能力；模型预算、推理和目录精确匹配继续沿用现有契约；最终验收待做。 |
| Environments | `SettingsPanel.tsx:446`的RemoteSettings连接SSH创建/编辑、连接测试、主机指纹确认、项目绑定 | system探测及入口未接；项目绑定缺busy/错误反馈；没有新建第二条连接的表单重置入口。 |
| Skills | `SettingsPanel.tsx:442`的SkillCard呈现名称/版本/摘要片段/能力并调用启停；`src/tauri-api.ts:288`一组API连接list/import/enable | 详情、文件读取和安全删除缺少设置API/UI；启停Promise未捕获，失败无反馈、可重复点击。 |
| Browser | `BrowserSettings.tsx`连接配置加载保存、session启动及授权撤销；`src-tauri/src/browser_commands.rs::browser_save_settings`保存失败有运行配置回滚 | 首次读取失败保存风险已由 `ce37b5f` 修复，定向4 tests通过（主代理报告）；最终全套待重跑。 |
| Connections | SettingsPanel连接stdio配置保存、检查/发现、启停及逐工具授权；p1_commands保留声明变更失效授权语义 | `b5c3156`已修环境绑定编辑保留语义，`60c4b69`已修变量名逐字符失焦；定向及根键盘检查通过，最终全套待跑。 |

## 最小补齐与验证

1. **system探测必须是真实操作。** `src-tauri/src/agent_v4.rs:344`明确backend列表只检查配置；`configured_process_backend:384`的python_status/r_status固定为unverified。RuntimeDialog仅消费此状态并插入环境准备草稿，不能证明已找到解释器。新设只读system诊断命令，复用 `LocalEnvironmentPortV4::check_interpreter:5848` → `program_available:8064` 的真实检查，固定python/Rscript并用模拟命令运行器测试；General和Environments使用同一命令。显示找到/未找到/检查失败，不宣称项目依赖可运行。
2. **SSH动作完整反馈。** `SettingsPanel.tsx:489`绑定按钮直接`void onBind`，未设置busy或捕获错误；保存连接后form.id不重置且无新建按钮。加明确绑定handler及新建表单入口；测试绑定失败保留输入、禁重复、成功后能创建不同UUID的第二条连接。项目切换后的绑定草稿也应重置至新项目真实值。
3. **Skills真实详情与安全删除。** `skill_commands.rs:263/368/422`已有内部文本资源读取，但未暴露设置命令；`omicsops-store/src/lib.rs::delete_skill_package`只删记录，不能直接当安全卸载。新增按skill ID解析受管路径的有界详情/文件读取和所有权+摘要保护删除；插件拥有项交Plugins处理，不删除外部源目录或改过的文件。测试路径穿越/链接/摘要冲突/共享来源保护及立即Escape只关闭详情或确认。SkillCard启停加busy/try-catch，失败保持原状态和可重试错误。
4. **Browser首次加载失败禁止保存（已修复）。** 原实现读取失败会结束loading、保存按钮仅以loading/saving门控；`ce37b5f`已加入成功读取baseline校验，定向4 tests通过。此项不再是待实现功能，最终全套与平台验收仍待重跑。
5. **MCP环境绑定保留语义（已修复）。** 原editMcpServer将literal值置空、saveServer拒绝空值，无法仅改名称；`b5c3156`已修保留/替换/移除语义及完整页操作，`60c4b69`进一步修复变量名逐字符输入失焦。原始缺陷不再登记为待实现，测试结果与范围见下表；真实修改启动声明仍按既有规则撤销授权。

现有Session/Models/Browser/MCP组件测试覆盖多项保存、失败、目录和Escape行为，但本核验没有重跑，不能替代最终完整检查或真实Windows/SSH/model验收。上述缺口已由主代理安排实现；后续改动不得将本审计旧行号误作最新实现证明。

## 后续已提交进展与验证边界

以下测试次数来自主代理报告，记录实际批次而非声称本只读审阅亲自执行：

| 页面/批次 | 已提交行为与实际验证 |
|---|---|
| Credentials `34afb53` / `26c4438` | 真实keyring目录/受限操作及设置页已接通；原始native 11 tests、frontend/API/SettingsPanel 64 tests、Web/desktop build通过；快照串行化修复后Credentials 7 tests通过。 |
| Memory `2837101` / `1cd6088` | 待处理草稿保护和按文件名筛选已提交，最终定向11 tests通过；不是记忆正文全文检索或全局习惯系统。 |
| Browser `ce37b5f` | 首次加载失败后的保存门控已修复，4 tests通过。 |
| Templates `4051103` / `3931cad` | Quick Actions/Specialists工作区可见草稿插入及卡片间距修复已提交，整合153 tests和Web build通过；不自动发送或启动独立专家。 |
| Connections `b5c3156` / `60c4b69` | 完整页修复后native MCP 25/25、Settings+API 61/61、Web build通过；输入失焦修复后Settings 59/59、Web build通过。根Edge 720×600实际keyboard.type('NCBI_EMAIL')得到完整值且焦点仍在env name 1。 |
| Usage native `ff03b2b` / `b7aace8` | Store最终5/5、native 7/7通过；仅native完成，UI/route仍在写，不能标整页完成。 |
| Storage `901bb76` | CSS修复后定向3/3和Web build通过；有数据mock fixture在1440×1000及720×600、dark120%、OS light下无横溢，内容宽956/956及410/410；不证明真实磁盘扫描。 |

Connections基础页修复已提交；Usage仅native完成。新增提交后的最终全套确定性检查仍待统一重跑；真实SSH/model/PBMC验收没有由这些测试代替。某次Vitest阶段711项通过，但npm参数被末尾node --test解析导致整体命令失败，不能称完整npm test成功。

主代理该批dist视觉检查：headless Edge 720×600、dark、uiScale1.2且OS为light，当时18个导航（含额外Privacy，不含尚未进入该dist的Usage/Plugins）可点击，页面main clientWidth=scrollWidth=410、document width=720。该无Tauri宿主/无项目检查仅证明导航及小窗框架。随后Storage padding已修并查看有数据fixture截图 `C:/Users/jindong/.codex/visualizations/2026/09/15/01a0a4d8-424b-7122-b3e4-925d0df0a256/settings-storage-ui-fixture-verified.png`；旧 `storage-ui-fixture.png`已过时。mock图验证布局，不替代真实磁盘扫描或其他原生行为验收。
