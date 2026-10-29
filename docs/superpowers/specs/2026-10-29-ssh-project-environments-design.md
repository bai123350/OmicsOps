# SSH 项目环境发现与执行一致性

## 问题与范围

用户已在远端项目 `.envs` 下分别安装 Python 与 R，左下角仍显示待检测。
原后端列表只是配置目录，始终返回 unverified；运行时仅支持 system 或
`.omicsops/environments/<name>`，因此只改状态也无法使用现有环境。

本次在修复分支实现有界、只读的项目环境发现，并让检测、前台内核、软件版本
读取与 SSH 后台作业采用相同的解释器绑定。自动检测只说明解释器可执行，不说明
科研依赖已安装。Local 仍只接受 system，容器行为不变。

## 冻结身份

保留旧 system 和安全 managed 名称的含义；新增明确的项目绑定 token：

- `envs:<name>`：Python/R 均使用项目 `.envs/<name>`。
- `envs:<python-name>:<r-name>`：两种语言分别使用明确的项目子目录。
  空段表示该语言没有绑定，执行报错，不回退 system 或其他环境。

名称仅允许安全、有限长度的单层目录名，拒绝点目录、路径分隔符、控制符和
shell 内容。token 进入既有 ComputeSelection、运行哈希、ExecutionContextKey
及后台任务身份，不新增可在运行中被重写的全局自动环境指针。
旧记录序列化不变，无 SQLite 模式迁移。新 token 仅在 SSH 后端有效。

## 发现与 UI

共享 DTO 和新宿主命令按当前项目、连接、已信任主机及远程根目录执行探测。
只检查 `.envs` 与 `.omicsops/environments` 第一层；候选数、输出、命令运行和
总等待均有限制，symlink/越界目录不作为可选环境。版本来自实际的隔离版本命令，
未知、超时和错误不伪装成缺失。探测不安装软件、不创建环境、不运行工作流。

设置建议由已验证候选产生：分别优先唯一的 Python/R 命名环境；歧义时保持手动
选择。首次尚未手工选择时可采用建议，并明确显示对应目录。用户显式选择的
system 或具体环境保留；不把其他候选的可用状态汇总成 system 可用。

SSH 计算/Runtime 面板打开后探测，并提供刷新。环境切换时重检；已启用检测的
项目任务结束后刷新一次。没有持续后台轮询。按 project/connection/root/backend/
environment 及请求代次隔离响应，切换后立即隐藏旧结果。失败不阻止普通文献对话。
历史运行继续显示冻结配置，不借用当前实时状态。新 UI 使用既有窗口级 Escape 栈。

## 执行与安全边界

项目绑定使用对应 prefix 下的 bin/python 或 bin/Rscript，并仅在进程环境中
前置该 bin 到 PATH；不读取 shell activation 脚本。执行前验证 prefix/bin 的
实际目录路径，拒绝环境目录或 bin 目录重定向。解释器文件允许普通 venv 指向基础
Python 的符号链接，探测与执行采用相同规则；冻结的是调用路径与环境身份，
不声称解释器二进制字节不可变或形成安全沙盒。旧 managed 环境继续使用 Micromamba。
项目环境 ensure 只检查已安装解释器，不创建/修改环境。

凭据仍只通过既有 keyring 和可信主机连接路径取得，不读取/记录用户秘密。
只读探测报告不等同于运行证据；不改变工具审批、冻结计划、科学证据或断线恢复。
停止 Agent 不取消已派发的 SSH 后台作业。

## 验证与提交

先验证 token 语法与分语言解析、目录/输出/超时边界、模拟 SSH 版本结果、自动
建议及各执行入口一致性；前端验证刷新、切项目/绑定/环境后的晚响应、显式选择
保留、探测失败仍能对话，以及立即 Escape 只关闭最顶层。

交付前运行 Web 构建、Rust workspace、前端全套与 Windows 桌面打包；使用临时
目录和 mock runner，不以真实 SSH、API key、R 或网络作为自动化测试前提。
真实 SSH 和安装 smoke 如未执行则明确记录。按环境执行、探测接口、UI 和文档
分组提交，不推送、不创建 tag/Release。

## 实际验证（2026-10-29，Windows）

- `npm run build`：通过；保留既有 bundle 大小提示。
- `cargo test --workspace`：2077 通过，21 ignored；未把 ignored 算成真实环境验收。
- `npm test`：131 个 Vitest 文件、1577 测试通过；浏览器扩展 22、发布脚本 37 通过。
  为控制本机并行负载，VITEST_MAX_FORKS/MAX_THREADS 为 2，MIN 为 1。
- bundled Python `-B -m unittest discover -s scripts -p 'test_*.py'`：114 个测试，
  112 通过、2 skipped。后台桥与 worker 验证使用临时文件和模拟进程。
- `cargo fmt --all -- --check`：最终通过；最初仅新 SSH 集成代码有格式偏差，
  运行 `cargo fmt --all` 后将纯格式差异单独提交。
- `npm run build:desktop`：通过，生成 Windows x64 EXE/MSI；资源、Web 与原生
  二进制发布隐私检查均通过。只生成本地安装包，没有安装或上传 GitHub。
- 独立审查发现并关闭三项问题：ROOT 帧占位符误替换、配置保存落定后报告失效、
  历史运行的具体目录可见性。新增回归均按失败到通过验证；执行与探测的普通
  venv 解释器链接策略也统一。

Linux 专用真实 shell fixture 已加入，用临时目录验证包含空格/单引号根路径的
命令输出；本机 Windows 未执行此条件测试。真实 SSH、模型、Python/R 科研依赖
以及原生窗口/旧安装升级验收未执行，没有读取实际 SSH 凭据或科研数据。

Windows 手工 smoke：安装本次新构建，绑定并信任一次性 Linux SSH 项目；在其
`.envs/python311`、`.envs/r443` 准备解释器，打开计算面板，确认分别显示版本，
推荐绑定指向两个目录。显式选择 system 后刷新不得被覆盖；安装新环境的任务
完成后更新一次。断网检测应显示未核实；切项目/连接时旧响应不得回来。用各语言
执行极小示例，核对其实际环境与被冻结 token 一致；历史运行展示原目录。SSH
后台作业另要求 system Python 3 控制器，不因发现项目 Python 而承诺后台支持。
