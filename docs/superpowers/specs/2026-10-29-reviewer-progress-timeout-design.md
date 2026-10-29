# 评审多轮进度与超时边界修复

## 问题与证据

最新失败为 `reviewer produced no completed report within 180 seconds`。
只读、脱敏的本机事件元数据显示：确定性验证结束后，两次评审各在约 180 秒后
超时；没有 `ReviewerFinished`。持久化未记录评审内部轮次，不能据此判定某个
provider 请求卡住、具体读了多少页或用户运行了哪个安装版本。

代码根因为超时作用域与实现不一致：`review_with_retry` 把最多 8 次串行
`model.stream` 的整次独立评审包在一个 180 秒 timeout 中，而 V4 provider
stream 已采用 900 秒硬上限。评审内部丢弃了流式进度，core 无法区分持续生成
与无输出等待；超时后会从同一冻结快照重新评审，先前页工作集不会被复用。

## 修复边界

为 ModelPortV4 增加向后兼容的 `review_with_progress` 默认方法，旧端口仍委托
原 `review`。桌面端在冻结 reviewer 选择后逐层转发该方法与模型流事件。core
仅用非空文本/推理、非零内容字节和工具调用生成活动重置 idle 计时，不将独立
评审的原文或推理写入预览、日志、事件或主执行上下文。重试、usage、普通活动
和空内容不延长 idle；评审的只读、来源绑定和提交校验保持不变。

整次 `review_with_retry`（含格式重生成、传输重试与退避）共享已有 V4 的
900 秒绝对硬上限。默认 180 秒作为无有效输出的 idle 上限；持续进度允许
正常分页超过旧 180 秒总限制。硬上限不重置、不再重试；idle/传输错误仍受原
重试额度限制。正在评审及退避时按现有取消策略及时响应 Stop，丢弃未完成报告，
不写入 ReviewerFinished/RunCompleted，也不重新派发任何执行工具。

不修改模型目录、profile 的输出窗口、上下文限额、8 轮/16 次读取额度；不跳过
独立评审或自动生成通过报告。真正无响应及持续生成超过硬上限仍明确失败。
没有新增 DTO、数据库迁移、凭据路径或外部调用。

## 验证

使用 Tokio 虚拟时间和模拟模型传输复现多轮健康生成累计超过 180 秒；验证
idle、空心跳、重试噪声、绝对上限、跨重试共享预算、取消与完成记录。桌面
测试验证内部流事件经过冻结 reviewer 端口传到 core，且不包含外部工具调用。
默认检查及 Windows 打包均需完成；真实模型/SSH 验收单独标记。

## 实际验证记录

核心先用 Tokio 虚拟时间复现 11 项失败（22 项既有回归通过），其中健康生成
累计 211 秒的例子明确产生与截图相同的旧 180 秒错误。另用手动 poll 后暂停
宿主 Future、外部 advance 181 秒再 poll 的测试复现迟到结果抢在 idle 分支前
被接受的竞态。修复后 callback 仅接受上次有效进度 idle 内的更新，报告完成
分支再核验有效进度和 hard 截止；watch 可合并合法更新，不复活迟到结果。

`cargo test -p omicsops-agent-core --lib reviewer_` 为 34 通过，涵盖持续生成、
静默、空内容、usage/retry 噪声、共享硬截止、同刻完成、格式再生、退避及停止
边界。`cargo test -p omicsops-desktop --lib reviewer_` 为 34 通过，保留原有
32 项来源/预算回归，并证明冻结的独立 Reviewer 在分页和最终提交前实时转发
Responses 字节进度及 HTTP `reasoning_content`，执行端没有被误调用。新增
Responses 测试按现有隐私契约验证内容字节，未要求适配器公开 reasoning 文本。
HTTP 测试只连接自身绑定的 loopback 模拟服务；受限沙箱会以 10013 拒绝连接，
在主机测试权限下通过，没有真实 provider 或外部网络前提。

最终 `npm run build`、`npm test` 通过：132 个 Vitest 文件、1588 项，扩展
22 项、发布脚本 37 项。最终 `cargo test --workspace` 为 2128 通过、22 ignored、
零失败。全局 `cargo fmt --all -- --check` 和差异空白检查通过。两名独立复核者
分别确认 core 时间/来源边界及 UI→Stop→运行 token 链无阻塞项。

本轮没有读取真实凭据或调用真实 API/SSH；没有执行 ignored 验收、安装升级验收
或 macOS/Linux 原生验收。用户已有执行结果及失败历史不修改，GitHub Release
不更新。

`npm run build:desktop` 通过，配置资源、Web 与原生二进制隐私检查通过。
Windows x64 EXE 生成于 2026-10-29 21:45:26 +08:00，MSI 生成于 21:45:38。
版本保持 0.1.0，文件名与之前包相同；这些是本轮本地构建产物，未上传或安装。
打包仅有既有 Vite 大 chunk 与 Windows linker 信息警告，没有构建失败。
