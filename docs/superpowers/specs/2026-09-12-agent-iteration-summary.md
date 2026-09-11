# 普通 Agent 迭代预算与到限总结

用户已要求参考 Wisp 的 100 轮、可配置上限、无工具收尾设计。参考源码为
xuzhougeng/wisp-science 提交 a705cc2846ed1b01a9c93bbefbd037dac4063700，
crates/wisp-core/src/agent.rs 和 src-tauri/src/lib.rs；只参考机制，不复制实现。

## 行为与边界

普通 Agent 每次启动/恢复采用全局 max_iterations 设置（默认 100，u32，
0 不限制轮数）。设置保存于既有 settings 表，下一次启动/恢复读取，
运行中的循环不受设置变化影响。一轮可以含多个工具调用。普通 Agent 取消
固定 96 次工具硬上限；计划执行保留默认 32 轮/96 次调用，子 Agent 的
独立预算、冻结能力与审批裁决均不变。普通 Agent 无进展检测阈值采用 5，
继续比较实际调用/结果，0 轮次限制不关闭取消、上下文和无进展保护。

普通 Agent 到限后，已接受批次先完成，再额外生成一个无工具状态总结。
总结的模型请求完全移除工具，要求列明已完成、未验证、剩余工作及下一步。
宿主拒绝空总结或任何工具调用，绝不派发总结中的调用，也不运行完成核验。
成功总结作为 RunNeedsAttention 的内容持久化，含明确的 max_iterations
诊断；不记录 RunCompleted。失败保留原因与既有事件。取消仍为取消。
正常完成仍必须通过 agent.complete 和原有证据核验，审批/提问暂停不算完成。

不增加事件类型或迁移，不恢复已删除的失败红框。设置界面复用窗口 Escape
堆栈。测试用模拟模型、内存/临时数据库与模拟 Tauri 调用，覆盖预算、超过
96 次工具调用、0 不限、到限总结及失败、取消、设置持久化与校验。
交付运行 workspace 测试、前端测试、Web 和桌面构建；真实模型验收单独说明。

## 实际验证

- cargo test --workspace：750 passed、0 failed、11 ignored。
- npm test：前端 190、浏览器扩展 22 通过。
- npm run build：通过，保留现有的大 chunk 提示。
- cargo fmt --all -- --check 与 git diff --check：通过。
- 独立审查：无阻断问题。按审查补充了无效总结的流式预览隔离测试，
  总结仅在检查后显示；旧预算文档已标明被本设计替代。

真实模型/MCP、SSH、安装后的手工验收未执行。Windows smoke 步骤：
在设置 → Agent 保存小轮次值，运行一次需要多步的任务，确认上限后显示
已完成/未验证/待做总结且为需要处理；保存 0 后重新运行，确认不再受轮数
和 96 次工具上限限制，停止按钮仍能取消本地等待。到限总结不是科研完成
证明，后台远端计算的取消/重连语义与本次变更无关。

- npm run build:desktop：通过，生成 Windows x64 NSIS 安装包
  target/release/bundle/nsis/OmicsOps_0.1.0_x64-setup.exe。未执行安装。
