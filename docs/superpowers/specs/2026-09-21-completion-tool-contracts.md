# Agent V4 完成与软件版本输入契约

> 日期：2026-09-21
> 状态：已实现

一次真实运行在完成阶段先把 `agent.complete.criteria` 写成字符串，随后又用
改写过的条件名称提交。Host 仍按冻结计划中的条件字符串精确验证；工具 schema
现在明确要求逐字复制 `frozen_plan.completion_criteria`，避免模型把条件摘要
误当成稳定标识。该改动不放宽 completion gate 或证据校验。

同一运行的三个 analysis 还把 `python=3.11`、标准库名和组合字符串放进
`software_requirements`，而 Host 会把每一项作为包名查询版本。工具 schema
现在要求每项只放一个已安装的 Python distribution 或 R package 裸名；版本约束、
组合字符串、标准库和解释器不进入该数组，纯标准库分析使用空数组。解释器版本
仍由 Host 自动记录。该说明不会修补既有 provenance，也不放宽缺失版本的验证。
