# Settings 基础页调用核验

日期：2026-09-15。依据source-audit和已批准的master基础适配范围，核对Session、Models、Environments、Skills、Browser、Connections当前源码。本文是代码阅读结果，未新增执行产品测试；后续实现提交和最终验收结果以master为准。ACP、HTTP/OAuth MCP、WSL导入和自定义本地解释器不在本次缺口清单。

| 页面 | 可验证的既有调用 | 仍需修复或补齐 |
|---|---|---|
| Session | `src/features/settings/AgentSettings.tsx:11`读取/保存5项设置；`src-tauri/src/agent_settings.rs:7`读取Store并兼容缺省；`agent_v4.rs:4127`加载运行限制，`follow_up_questions.rs:106`消费建议开关 | 未发现基础范围缺失能力；最终验收待做。 |
| Models | `SettingsPanel.tsx:98/130/141/191`连接保存、探测、模型目录和配置编辑；`src-tauri/src/model_commands.rs:143/152/206/230`提供真实列表/保存/探测/发现，凭据经现有vault | 未发现基础范围缺失能力；模型预算、推理和目录精确匹配继续沿用现有契约；最终验收待做。 |
| Environments | `SettingsPanel.tsx:446`的RemoteSettings连接SSH创建/编辑、连接测试、主机指纹确认、项目绑定 | `c306e0a`已补system探测、新SSH入口、字段busy、绑定反馈及切项目竞态；定向通过，最终全套待跑。 |
| Skills | `SettingsPanel.tsx:442`的SkillCard呈现名称/版本/摘要片段/能力并调用启停；`src/tauri-api.ts:288`一组API连接list/import/enable | S1 `d7ba5e9`与父junction修复 `f18f45b`、S2 native `831a659`已提交（native 11 tests通过）；UI/S3管理仍进行，整页未完成。 |
| Browser | `BrowserSettings.tsx`连接配置加载保存、session启动及授权撤销；`src-tauri/src/browser_commands.rs::browser_save_settings`保存失败有运行配置回滚 | 首次读取失败保存风险已由 `ce37b5f` 修复，定向4 tests通过（主代理报告）；最终全套待重跑。 |
| Connections | SettingsPanel连接stdio配置保存、检查/发现、启停及逐工具授权；p1_commands保留声明变更失效授权语义 | `b5c3156`已修环境绑定编辑保留语义，`60c4b69`已修变量名逐字符失焦；定向及根键盘检查通过，最终全套待跑。 |

## 最小补齐与验证

1. **system探测必须是真实操作（已接入）。** 原backend目录只报告配置/unverified，不能证明解释器存在。General G1 native `b67f2c2`新增真实system探测，UI `75bdf7e`与Environments `c306e0a`接入；固定python/Rscript，显示找到/未找到/检查失败，不宣称项目依赖可运行。定向验证见下表，真实解释器验收未由mock替代。
2. **SSH动作完整反馈（已修复）。** `c306e0a`补新建连接入口、字段busy、绑定错误反馈与项目切换保护，避免旧绑定结果拉回旧项目。基础缺口已提交，最终全套待跑。
3. **Skills真实详情与安全删除（部分完成）。** S2 native `831a659`已新增按skill ID解析受管路径的有界详情/文件读取；UI/S3管理仍待交付。`omicsops-store/src/lib.rs::delete_skill_package`只删记录，不能直接当安全卸载，仍需所有权+摘要保护删除；插件拥有项交Plugins处理，不删除外部源目录或改过的文件。测试路径穿越/链接/摘要冲突/共享来源保护及立即Escape只关闭详情或确认。SkillCard启停加busy/try-catch，失败保持原状态和可重试错误。
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
| Usage `ff03b2b` / `b7aace8` / `57dfbc4` | Store最终5/5、native 7/7；整页API 2 + Usage 9 + Settings 60 + Desktop 77 = 148/148及Web build通过。 |
| Storage `901bb76` | CSS修复后定向3/3和Web build通过；有数据mock fixture在1440×1000及720×600、dark120%、OS light下无横溢，内容宽956/956及410/410；不证明真实磁盘扫描。 |

Connections、Usage、Environments基础页已提交；General G3通知、Skills UI/S3管理、Plugins仍未完成。`13d9c57`时Rust workspace全套通过，后续新增之后仍需重跑。新增提交后的最终全套确定性检查仍待统一重跑；真实SSH/model/PBMC验收没有由这些测试代替。某次Vitest阶段711项通过，但npm参数被末尾node --test解析导致整体命令失败，不能称完整npm test成功。

主代理该批dist视觉检查：headless Edge 720×600、dark、uiScale1.2且OS为light，当时18个导航（含额外Privacy，不含尚未进入该dist的Usage/Plugins）可点击，页面main clientWidth=scrollWidth=410、document width=720。该无Tauri宿主/无项目检查仅证明导航及小窗框架。随后Storage padding已修并查看有数据fixture截图 `C:/Users/jindong/.codex/visualizations/2026/09/15/01a0a4d8-424b-7122-b3e4-925d0df0a256/settings-storage-ui-fixture-verified.png`；旧 `storage-ui-fixture.png`已过时。mock图验证布局，不替代真实磁盘扫描或其他原生行为验收。

本次新增进度（主代理报告）：

| 批次 | 实际验证与剩余边界 |
|---|---|
| 长内容布局 `13d9c57` | Usage 9 + Storage 3 + Settings 61 = 73/73、Web build及Rust workspace全套通过；后续新增后最终全套仍需重跑。 |
| General G1 `b67f2c2` / `75bdf7e` | native 7/7及desktop build、UI 72/72及Web build通过；目录失效宿主预检/ProjectLibrary notice由 `8db2719`补齐（native 8、frontend 12及Web build通过）；G3通知待做。 |
| Environments `c306e0a` | system probe、新SSH入口、字段busy、绑定反馈及防旧绑定拉回项目已接入；Settings 64、Settings + Desktop 142/142及Web build通过。 |
| Skills S1 `d7ba5e9` | 来源receipt adapter 2/2、native 16/16、fmt通过；父junction修复 `f18f45b`已提交；S2 native `831a659`有界详情/预览已提交，native 11 tests通过；UI/S3管理仍进行，不能标整页完成。 |

根actual dist、Edge 720×600、dark120%长内容fixture验证：Storage 15分类main clientHeight/scrollHeight为436/1830，各卡clientWidth=scrollWidth；Usage main高度436/1424，6统计section均clientWidth=scrollWidth，均已滚至底部。上述截图目录中的 `settings-storage-long-content-verified.png`、`settings-usage-long-content-verified.png`为本轮证据，旧 `settings-usage-ui-fixture.png`是缺陷图。General mock native fixture正确呈现found/missing，main高度436/1100、宽410/410，图为同目录 `settings-general-native-fixture.png`。这些检查证明UI布局/模拟状态呈现，不是真磁盘扫描、真实用量原生端到端或真解释器验收。

General G2 `a3e4c7b`已全部接入，164 tests及Web build通过（主代理报告）。根实际Edge dist + mock Tauri消息UI检查：旧草稿保留、引用追加、未调用发送mutation、立即window Escape关闭选区toolbar、textarea.setSelectionRange不触发toolbar，全部通过。这是UI mock验证，不是模型/SSH端到端验收；后续新增提交后的最终全套仍待重跑。
