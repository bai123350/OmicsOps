# Local 计算失败、科研状态与 SSH 专用工具边界

## 故障链

用户第二次截图报告 Local 环境调用 `runtime.remote_job_status` 后，运行以
“remote jobs require an SSH Linux backend”及不确定副作用错误结束。只读检查
运行事件的白名单元数据确认：一次 `runtime.execute` 已返回失败结果，后续
计算返回成功；提交完成时确定性验证仍报告 `analysis_still_running`，模型随后
调用远程作业查询和内核 interrupt。只读查询抛出错误，核心把它标为
`ToolDispatchUncertain`，中断了这一批结果的记录。

这次问题与此前 Reviewer 的 JSON 纠错不同，涉及同一条运行失败恢复链：

1. 科研状态的 after-tool hook 仅在成功时调用，已结束的失败计算永久留在
   `Running`，而新的成功计算使用另一 source call，不能结束旧记录。
2. 桌面组合给 Local/Container 暴露了仅支持 Linux SSH 的后台作业能力。
3. 核心把宿主确认的静态 ReadOnly 工具执行异常也归入不确定副作用。

## 修复边界

- Local/Container 的工具目录隐藏 `runtime.remote_job_status`，并将
  `runtime.execute.background` 约束为 false。保留 registry 中的 effect，
  使历史或强制调用在派发前可得到可恢复的拒绝结果。宿主 preflight 和直接
  executor 路径都拒绝不支持的操作，不能仅依赖提示或 schema。
- SSH 保留远程作业目录与条件说明；目录可见不代表已验证远端 Linux、依赖或
  作业可用。保留惰性 SSH 连接、冻结环境和审批，禁止回退为本地执行。
- 静态 ReadOnly 的执行异常记录失败 `ToolFinished`，模型可据此调整下一步，
  已完成的同批其他结果继续记录。诊断使用固定宿主文字，不把原始异常中的
  凭据或服务器输出写入事件。Runtime、Mutating、Network 以及仅有 MCP
  read-only hint 的不确定派发仍被隔离，不自动重跑。
- 成功结果和包含完整宿主 `RuntimeResultV4` 的确定失败结果都进入科研状态
  after-tool hook。确定失败关闭为 `Failed`，保留真实 provenance；不生成
  成功产物或假成功。未派发、后台提交、部分结果和原始派发异常不算计算完成。
- 对历史遗留的 `Running`，只从同项目、run、backend、source call 的持久
  `ToolRequested` 与确定失败 `ToolFinished` 核对结果，幂等补齐科研终态。
  不重执行代码、不通过网络探测、不把未知派发或停止等待当作已取消计算。

无 schema/SQL 迁移、依赖或凭据变更。既有 Reviewer JSON 修复与用户的其他
未提交文档保持。此修复不会自动取消 SSH 后台任务，也不声明原研究目标已完成。

## 验证要求

用临时 Store、模拟模型/工具/资源工厂与完整 runtime 结果复现截图的失败链。
覆盖 Local/Container 工具目录和强制调用、SSH 目录、静态只读错误恢复及同批
结果、Runtime/MCP 不确定边界、失败与后续成功的科研终态、持久结果恢复和
历史状态幂等修复。反例覆盖未派发、未知、部分结果、错误项目/run/backend。

执行相近回归后运行 `cargo test --workspace`、`npm test`、`npm run build`、
`cargo fmt --all -- --check` 与 `npm run build:desktop`。格式仅偏差时运行
`cargo fmt --all`，纯格式修改另行处理。

Windows 手工 smoke：启动新本机构建，在 Local 一次性项目中让计算确定失败，
修正后再计算并提交；旧失败显示为 Failed，模型工具目录无远程作业查询，强制
远程请求返回未派发错误，失败只读请求不显示副作用核实。SSH、真实模型及原
文献科研流程验收须单独记录；本次不重发此前自动审批拒绝的外部模型请求。

## 实际验证（2026-10-06，Windows）

- `cargo test -p omicsops-agent-core read_only_ -- --nocapture`：新增回归先有
  5 个预期失败，修复后目标筛选 6/6 通过。`cargo test -p omicsops-agent-core`
  200/200 通过，包括 Runtime、Mutating 与 Network MCP 的不确定派发反例。
- `cargo test -p omicsops-desktop detached_remote_jobs -- --nocapture`：工具
  目录及 prepare/direct executor 的 3 个新增回归先全部失败；修复后
  `cargo test -p omicsops-desktop --lib detached_remote_jobs -- --nocapture`
  3/3 通过。首次沙箱构建的路径 canonicalize/OUT_DIR 失败属于环境阻挡，
  在正常 Windows 用户权限下正确观察红态和绿态。
- `cargo test -p omicsops-desktop scientific_failure_contract_tests -- --nocapture`：
  先复现 3 个 `Running != Failed` 失败；修复后带 `--lib` 的同一测试筛选
  7/7 通过。除状态断言外也核对新计算的 `ScientificStateChanged`，避免历史
  快照修复掩盖实时 after-tool hook 的缺失。
- `cargo test --workspace`：1533 通过、0 失败、17 ignored，退出 0。
- `npm test`：992 个前端和 22 个扩展测试通过，退出 0。
- `npm run build`：通过，退出 0；保留已有的大 chunk 提示。
- 格式检查只发现新增代码的排版偏差，已执行 `cargo fmt --all`；相关文件
  `git diff --check` 和完整 `cargo fmt --all -- --check` 复查通过。
- 独立审查未发现 P1/P2 正确性、授权或恢复问题。
- `npm run build:desktop`：通过，退出 0；release 编译耗时 2 分 40 秒。
  `target/release/omicsops-desktop.exe` 已更新。仅生成本机验证构建，未安装或发布。

已知 P3 限制：历史补记复用既有 finish hook，`analysis.ended_at` 取补记时刻，
原 `ToolFinished.occurred_at` 保留真实回执时间；精确历史耗时应依据原事件。
provenance 的记录时刻也为补记时刻。历史终态事件链不追加或重写。

真实模型、SSH、原生 GUI 与原文献工作流验收未执行；17 个 ignored 测试不算
验收通过。此轮没有重试此前被自动审批拒绝的外部模型数据传输。
