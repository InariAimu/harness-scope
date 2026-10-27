# 速度与首字

[文档索引](README.md) · [架构与数据契约](ARCHITECTURE.md)

原生日志计时均为估算，界面显示 `≈`；缺失依据显示 `—`。请求详情提供耗时与首字来源，不应把这些值当作精确网络延迟。

## 采集来源

| 来源 | 起止依据 | 首字依据 |
| --- | --- | --- |
| Codex | 最近的有效用户／轮次／工具边界至本次用量事件；排除后续工具执行 | 对应输出项 `item_completed.started_at_ms`，无有效字段则未知 |
| OpenCode V1／V2 | 助手 `time.created` 至有效 `time.streamed`，缺失时回退 `completed` | 同一消息 text／reasoning part 的最早创建／开始时间 |
| DeepSeek Harness v0／v4 | 同一 turn／step 的 `step/start` 至 `assistant/message` | 首个非空 text／reasoning／tool-call delta，含支持的重试 stream |
| Claude Code | `api-request.id` 与助手 `requestRef` 唯一关联后的请求窗口 | 未提供可靠首 Token 时间，保持未知 |

首字表示最早模型输出，可能包含推理，不一定是最终可见答案。Codex 输出项生命周期和 OpenCode 内容创建时间不等于流 Token 到达；Claude 的思考块时长也不能用于补造首字。

Codex 重复累计快照不切分请求，取消等边界清除未结算计时。OpenCode 的 completed 回退可能包含工具耗时；DSH 流片段可能缓冲后集中到达。计时只影响估算可用性，不改变用量身份和去重。

## 计算与样本门槛

| 指标 | 规则 |
| --- | --- |
| 输出阶段速度 | 输出 Tokens ÷（总耗时 − 首字等待）；输出至少 200，输出阶段至少 1 秒 |
| E2E 回退 | 缺少首字时间时，输出 Tokens ÷ 端到端耗时；输出至少 200，耗时至少 1 秒 |
| 汇总速度 | 同口径的输出总和 ÷ 有效时间总和，不平均逐条速度；优先输出阶段样本，无此类样本才显示 E2E |
| 单条首字 | `firstTokenMs / 1000`，显示秒 |
| 汇总首字 | 有效样本中位数，偶数样本取中间两项均值 |

耗时只接受 100 ms–1 小时，首字必须在起止区间内；输出包含 reasoning 子集，不重复累加。短回复仍可有首字，但不进入速度样本。缺失计时的输出不能加入速度分子。

所有来源有首字时间时均扣除首字等待；输出窗口过短或无效时保持未知，只有缺少首字时间才采用 E2E 回退。Codex 和 OpenCode 的首字时间为日志估算，未必等于实际流 Token 到达时间。两种速度口径不混算，界面展示采用的样本数。门槛用于减少短窗口偏差，不能保证消除所有客户端缓冲影响。

## 模型档位比较

仅 `harness=codex` 的模型统计按原始模型与请求档位分组。可信 `thread_settings`／`turn_context` 的 `default` 为 Standard，`priority`／`fast` 为 Fast；缺失为档位未知。其他工具的同名模型保留无档位分组，不推断其服务档位。

各组独立计算请求、Tokens、费用、加权速度和首字中位数，同模型相邻展示并一起分页。档位来自请求设置，不证明服务端实际执行或计费；任务、上下文和输出长度差异仍会影响比较。首轮证据、历史回填见 [Codex 说明](CODEX.md)。

## 查询接口

后端查询统一提供估算策略，前端按策略展示，不自行使用另一套门槛：

- 输出阶段：`estDecodeSpeedOutputTokens`、`estDecodeSpeedGenerationMs`、`estDecodeSpeedSampleCount`。
- E2E：`estSpeedOutputTokens`、`estSpeedDurationMs`、`estSpeedSampleCount`。
- 首字与覆盖：`estFirstTokenMedianMs`、`estFirstTokenSampleCount`、`timingSampleCount`。
- 单请求：`timingMethod`、`timingSource`、`firstTokenSource` 和派生的 `estimatedSpeed`（依据、时间、门槛及有效性）。

未知状态码的成功率保持未知，不用计时是否存在代替请求成功状态。

## 持久化与同步

可选 `timing` 随用量协议 v4 传输，仅包含耗时、首字及白名单来源。schema 版本独立演进，当前见[版本说明](ARCHITECTURE.md#版本与兼容)。计算后的速度、正文、路径和凭据不进入计时负载。

`record_timings` 是按 `(device_id, id)` 维护的查询投影，与事实及下载游标在事务中更新，旧修订不会覆盖新样本。服务器镜像重建时清除远程投影，保留本机计时；远端样本仍表示来源设备的日志估算。

解析器升级可从仍存在的日志补齐证据；源日志已删除且未保存的计时不能恢复。已有本机计时可通过原 ID 修订和 outbox 补传，Token 与冻结费用保持原样。服务端先升级相应能力，未知来源不能通过剥离字段提前确认。

实现见 [timing.rs](../crates/core/src/timing.rs)；回归见 [timing 测试](../crates/core/tests/timing.rs)、[格式化测试](../src/components/usage/format.test.ts)和[列配置测试](../src/components/usage/statsColumns.test.ts)。
