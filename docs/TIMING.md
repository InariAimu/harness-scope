# 速度与首字

## 采集来源

参考 [CC Switch e8880e6](https://github.com/farion1231/cc-switch/tree/e8880e6f57e25bce1bf3125cc6e33d9fb2b0d806/src-tauri/src/services) 的请求计时器、速度门槛和中位数口径。所有原生日志计时均为**估算**，请求列表和模型／应用统计显示 `≈`；未知显示 `—`。请求详情展示耗时依据、首字依据以及估算说明。

| 来源 | 开始 | 结束 | 首字估算 |
| --- | --- | --- | --- |
| Codex JSONL / zst | 首个模型输出之前最近的用户消息、turn_context、工具结果或上次用量边界 | 优先匹配本次用量的 token_usage_record；否则 token_count；若它紧跟工具结果写出，回退到工具执行前最后模型输出 | 本次最早 Reasoning / AgentMessage 的 item_completed.started_at_ms 减去起点；没有有效字段则未知 |
| OpenCode v2 SQLite | 助手消息 time.created | 优先有效 time.streamed；缺失或早于起点时回退 time.completed | 本条消息 text / reasoning 内容中最早 time.created（兼容 time.start）减去起点；缺失则未知 |
| OpenCode v1 SQLite | 同上 | 同上，通常只有 completed | 使用同一 session/message 下 text / reasoning part 的 time.start（兼容 created） |
| DeepSeek Harness v0/v4 | 对应 turn/step 的 step/start.time | assistant/message.time（不含后续工具执行） | 首个非空 text / reasoning / tool-call delta；v0 支持打包流片段，v4 读取消息／重试 attempt 内 stream；空片段跳过 |

Codex 重复累计快照不切分请求；task_started / turn_aborted 清除未结算请求；旧布局在 token_count 后补写的同批输出不归到下一次请求。沿用原有用量去重、分叉和稳定事件 ID。OpenCode v2 的流结束时间不包含后续工具执行；completed 回退可能包含工具执行、使估算速度偏低，详情中会明确展示。

首字指**最早模型输出（包含推理）**，不是最终可见答案的首字。日志起点不是网络请求发出的精确时刻，包含客户端组装开销；OpenCode 内容创建时间也可能晚于首个流片段。不要将估算值当成网络 TTFT。CC Switch 固定版本未从 OpenCode 原生日志推首字；内容时间戳估算是本项目的补充。

## 计算与统计

| 指标 | 计算与条件 |
| --- | --- |
| 日志估算速度 | 输出 Tokens ÷ (估算耗时 / 1000)，包含首字等待；输出至少 200，耗时至少 1000 ms |
| 汇总估算速度 | 同一批满足上述门槛的样本 Σ输出 ÷ (Σ耗时 / 1000)，不平均逐条速度 |
| 首字 | firstTokenMs / 1000，保留一位小数 |
| 汇总首字 | 当前日期、设备、应用和模型筛选下有效首字样本的中位数；偶数样本取中间两项均值 |
| 精确代理速度（预留显示） | 输出至少 100，latencyMs − firstTokenMs 至少 100 ms；本机日志不会填入此统计 |

输出沿用统一 output 维度，包含 reasoning 子集。耗时只接受 100 ms 至 1 h；倒挂、缺失、过短或过长的时间戳不影响用量采集，仅使计时未知。首字必须位于起止区间内。没有首字也能估速度，小回复不能估速度但仍可统计首字。缺失计时的输出不能混入速度分子。

API 的 models/providers 返回 estSpeedOutputTokens、estSpeedDurationMs、estSpeedSampleCount、estFirstTokenMedianMs、estFirstTokenSampleCount、timingSampleCount；详情包含 timingMethod=estimated、timingSource、firstTokenSource。未知状态码的成功率显示 `—`。

## 持久化与回填

计时保存在本机 SQLite 新增的 record_timings 表，使用完整 (device_id, id) 键。它与用量和文件游标在同一事务提交，保留原有费用快照和计数。该表是兼容旧 schema v1 的附加本机数据，不进入 v1 同步 DTO、outbox 或服务器；远程设备记录暂不携带计时，也不会错误套用本机同 ID 的计时。

解析器版本更新为 codex-v3-timing / opencode-sqlite-v2-timing；启动扫描会重读现有源日志并回填，无需重新定价。历史已删源日志不能回填；删除会话后已有计时随用量保留。若源记录仍未完成，继续重试，不覆盖已完成用量。

测试见 crates/core/tests/timing.rs、src/components/usage/format.test.ts 和 statsColumns.test.ts。

## 默认窗口尺寸

配置默认 1500×1000。Windows 在关闭系统装饰后调用 Tauri PhysicalSize(1500, 1000)，以本机 DPI 下的实际客户端像素为准，随后居中；不是将配置的逻辑像素再乘以缩放比例。阴影／外框不计入客户端尺寸。
