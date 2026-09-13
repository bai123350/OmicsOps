# PubMed MCP 统一

旧入口通过 --omicsops-pubmed-mcp 暴露两个工具，科学预设通过
--omicsops-bio-mcp pubmed 使用编译目录。用户要求统一实现及设置入口，兼容旧配置。

新配置统一使用 Science · pubmed，复用科学 MCP 的注册、检查、授权、调用路径。
设置不再展示独立 PubMed 添加预设；统一 PubMed 卡片提供凭据配置表单，
API key 写入系统凭据库，环境配置仅保存引用。
旧 add_pubmed_mcp_server API 保留，但转到统一预设；Agent 不再为 PubMed 专门刷新目录。

启动注册后，识别确属应用旧版的单参数声明，将缺失的环境绑定补到统一预设。
只复制凭据引用，不读取或复制密钥值；统一预设已有绑定优先，邮箱别名视为同一设置。
未配置的统一预设继承旧启用选择、工作目录和超时。声明变化撤销旧授权，目录变化
不授权新工具。自定义可执行命令不被迁移覆盖。

旧记录不删除，保留 ID、工具目录和凭据引用供历史追溯，禁用并撤销授权，标为
superseded；设置列表和新任务发现排除这些旧入口。重复启动不会重复迁移。
历史运行若冻结了旧目录，不能静默改用新目录继续执行，需新运行并重新检查/授权。
旧 stdio 开关保留作为兼容代码，不作为新配置入口。

确定性测试覆盖迁移、幂等、凭据引用、自定义配置、既有禁用选择、兼容 API、
凭据轮换撤销审批，以及统一设置入口。执行 workspace 测试、前端测试、Web/桌面构建。
真实 NCBI API、已安装应用启动迁移及真实模型验收未执行。

Windows 手工检查：启动新 EXE，确认旧 PubMed 行消失、统一预设可发现完整目录；
检查环境绑定引用及邮箱，重新检查和授权后发起文献查询；重启确认不重复增加条目。

实际验证：cargo test --workspace 769 通过、11 ignored、0 失败；
npm test 前端 203 项、扩展 22 项通过；npm run build 通过；
cargo fmt --all -- --check 与 git diff --check 通过。独立代码审查通过。
npm run build:desktop 通过，生成 target/release/bundle/nsis/OmicsOps_0.1.0_x64-setup.exe。
未执行安装或真实 NCBI 网络查询。
