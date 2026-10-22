# SSH 内核初始化失败与计算派发边界

## 故障证据

环境检查成功后，后续 `runtime.execute` 在取得运行时 session 前失败，错误为
`I/O failed: Failure`。持久化 job 没有 session 或 result request，远端最近的
内核驱动文件为 0 字节；先前驱动有完整长度。当前 SFTP 库会在 shutdown 时
等待 WRITE 确认，不能用“缺少 flush”解释此故障，也不能忽略写入失败。

只读检查显示项目和内核目录存在、可写，文件系统显示 100% 使用率，但仍报告
剩余空间，未获得明确配额错误。随后在两个唯一临时子目录内，对 128 字节、
8 KiB 文件和完整捆绑 Python 驱动的写入均成功，测试文件已清理。因此原始
服务端写入失败当前无法稳定复现，不能声称已修复远端存储容量或配额。

可确定的程序错误是：`runtime.acquire` 初始化失败被统一保存为 Unknown，
再升级为 ToolDispatchUncertain，中断了本可根据已有环境观察报告限制的对话。
所有 backend launch 只接收执行上下文，不接收本次用户代码；当前代码直到
acquire 成功、Running 状态保存后才传入 start_job。

## 修复边界

- 对已明确返回失败的初始化新增持久化 `initialization_failed` 终态及封闭的
  错误分类，返回失败工具结果，明确 `code_dispatched=false`。不构造虚假的
  RuntimeResult、session、request 或科研执行证据。
- SSH 驱动上传失败只记录固定宿主分类和说明，不把服务端原始输出、路径或
  凭据写入事件。提示核对远端存储空间、配额和权限。
- 初始化期间仍可能创建启动文件或空闲进程；这不是“无任何副作用”的证明，
  也不证明已有远端计算已取消。不会自动重试本次调用。
- 同一请求恢复时，只读取匹配上下文及请求哈希的已保存初始化失败结果，
  不再次启动内核。初始化中断、崩溃、未获得确定结果，以及代码提交后的连接
  失败继续保守处理；历史 Unknown 记录不会因为 session 缺失被改写。
- 新字段可选，旧记录仍可读取，无数据库表迁移。后台 detached 作业不能进入
  此初始化失败终态；其重连、查询和取消边界不变。
- 对带 analysis 声明的调用，仅凭匹配运行、请求哈希及完整内容的持久化回执，
  将尚未启动的分析记为失败。不生成执行代码哈希、运行时身份或 provenance。
  工具结果落库后科学状态尚未更新的窗口，也可通过同一验证路径幂等恢复。

## 验证

确定性回归覆盖初始化失败的保存与重启恢复、同调用不重试、请求和上下文校验、
原始错误脱敏、取消及派发后连接失败。真实 SSH 验收使用独立临时目录和捆绑
驱动，仅执行合成 Python 单元，不运行用户原代码或安装/删除用户环境。

完整 Agent 回归串联成功的只读解释器探测、真实预留/初始化失败处理和自动审核。
最终回答只能引用已成功的探测证据并说明执行限制；额外断言失败回执不能充当
检查已成功执行的证据，且没有重复执行或 ToolDispatchUncertain。

## 2026-10-22 实际验证

- `npm run build`：通过。
- `npm test`：前端 1511 项及浏览器桥接 22 项通过。
- `cargo test --workspace`：2021 项通过，21 项 ignored，0 项失败。
- 桌面科学失败契约 11 项、运行时 job 3 项、Store job 17 项均通过。
- `cargo fmt --all -- --check`、`git diff --check`：通过。格式化仅涉及本次行为
  变更文件，没有无关的纯格式改动。
- 按 `acceptance/README.md` 设置一个已有连接和临时目录父路径后，执行
  `cargo test -p omicsops-desktop --lib agent_v4::ssh_startup_acceptance_tests::live_ssh_kernel_startup_preserves_two_python_cells -- --ignored --exact --nocapture`：
  1 项通过，24.89 秒；生产驱动上传、同会话两次 Python 执行及精确清理均通过。
  不代表原任务、R 环境、真实模型组合或原生界面的完整验收。
- `npm run build:desktop`：通过，生成 Windows 可执行文件及 NSIS 安装包，
  未安装或发布。原始远端写入故障当前未复现，也未清理或更改用户存储配额。
