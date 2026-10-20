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
| DSH 输出阶段估算 | 输出 Tokens ÷ ((耗时 − 首字等待) / 1000)；耗时依据为 dsh_step_message、首字依据为 dsh_stream_delta；输出至少 200、输出窗口至少 1000 ms |
| E2E 端到端吞吐估算 | 其他日志或缺少有效流式首字依据的 DSH，输出 Tokens ÷ (估算耗时 / 1000)，包含首字等待；输出至少 200、耗时至少 1000 ms；显示 E2E 标记 |
| 汇总估算速度 | 同一口径有效样本 Σ输出 ÷ (Σ有效窗口 / 1000)，不平均逐条速度；优先输出阶段样本，无此类有效样本才显示 E2E。两种口径不混算，悬停展示口径及采用／未混入的 E2E 样本数 |
| 首字 | firstTokenMs / 1000，保留一位小数 |
| 汇总首字 | 当前日期、设备、应用和模型筛选下有效首字样本的中位数；偶数样本取中间两项均值 |
| 精确代理速度（预留显示） | 输出至少 100，latencyMs − firstTokenMs 至少 100 ms；本机日志不会填入此统计 |

输出沿用统一 output 维度，包含 reasoning 子集。耗时只接受 100 ms 至 1 h；倒挂、缺失、过短或过长的时间戳不影响用量采集，仅使计时未知。首字必须位于起止区间内。DSH 流输出窗口过短或输出不足时显示未知，不能为了显示速度而改用整段耗时。没有流式首字依据时仍可估算 E2E，小回复不算速度但仍可统计首字。缺失计时的输出不能混入速度分子。

DSH 的客户端流时间反映片段到达，无法保证等同于模型逐 Token 生成。2026-10-20 本机只读核对发现，`opencode-go / deepseek-v4.1-flash` 一次工具调用输出 124 Tokens，总耗时 6491 ms，首片段等待 6331 ms；78 个参数片段集中在余下 160 ms 到达，旧门槛得到 775 tok/s。同批还发现 1311 Tokens / 778 ms 的集中输出，仅提高 Token 门槛仍不足以过滤。现将输出量和输出窗口门槛同时提高到 200 Tokens / 1 s，列表、详情和汇总使用同一查询策略；不截断高速度数值，也不将过短窗口改作 E2E。该过滤减少短样本偏差，保留下来的值仍是客户端接收速度估算，不能排除较长时间的缓冲。

API 的 models/providers 用 estDecodeSpeedOutputTokens、estDecodeSpeedGenerationMs、estDecodeSpeedSampleCount 表示输出阶段样本；estSpeedOutputTokens、estSpeedDurationMs、estSpeedSampleCount 仅表示 E2E 回退样本。另返回 estFirstTokenMedianMs、estFirstTokenSampleCount、timingSampleCount。请求列表与详情包含 timingMethod=estimated、timingSource、firstTokenSource，以及查询时派生的 estimatedSpeed（basis、durationMs、eligible、minOutputTokens、minDurationMs）。前端依后端策略显示，避免按不同门槛重算；兼容旧服务未返回策略时使用原 E2E 估算。未知状态码的成功率显示 `—`。

## 统一其他 harness 的评估（2026-10-20）

建议统一“明确口径、按同口径汇总、有效窗口过滤”的规则，不能直接从所有日志的总耗时减去当前首字估计。DSH 的起点、流片段与结束属于同一 turn/step，已在本机 App 0.2.1-rc.2 的实际会话逐请求验证，适合启用输出阶段估算。

| Harness | 当前条件 | 本次处理与后续条件 |
| --- | --- | --- |
| DSH | step/start、首个非空流 delta、assistant/message 组成完整边界，重试首输出保持 | 启用输出阶段估算，保留 ≈ 与短窗口过滤 |
| Codex | 首字来自输出项 started_at_ms，约 19% 本机历史请求有该字段；它是 item 生命周期开始，缺失时上游还可回退完成时刻，不是流 Token 到达事件 | 保留 E2E。要采用输出阶段口径，需要验证 item 与具体请求的对应关系及实际首输出语义，或采集请求级流式首字时间，不能将缺失字段补为零 |
| OpenCode | 首字来自内容创建；仅约 6% 本机记录带独立 streamed 结束字段，其余 completed 可能包含工具执行 | 保留 E2E。可后续仅对具备可靠首次输出和独立流结束的版本启用，旧记录继续回退并明确来源 |
| Claude Code | 当前原生日志解析没有完整请求起止和首输出边界 | 继续显示未知；不能用相邻消息间隔代替网络或模型生成耗时 |

上述比例为本机只读审计快照，不代表这些产品的全部版本能力。Codex 上游可参考 [item 计时实现](https://github.com/openai/codex/blob/main/codex-rs/core/src/session/mod.rs)。后续适配集中在后端计时来源策略，不需要改变 Token、费用或同步身份。

## 持久化与回填

计时保存在本机 SQLite 新增的 record_timings 表，使用完整 (device_id, id) 键。它与用量和文件游标在同一事务提交，保留原有费用快照和计数。该表是兼容旧 schema v1 的附加本机数据，不进入 v1 同步 DTO、outbox 或服务器；远程设备记录暂不携带计时，也不会错误套用本机同 ID 的计时。

解析器版本更新为 codex-v3-timing / opencode-sqlite-v2-timing；启动扫描会重读现有源日志并回填，无需重新定价。历史已删源日志不能回填；删除会话后已有计时随用量保留。若源记录仍未完成，继续重试，不覆盖已完成用量。

2026-10-20 的 DSH 速度调整仅在查询时消费现有 record_timings，没有增加 schema、修改原始计时、提高解析器版本或重新扫描历史日志；不更改费用、用量修订与 outbox。速度策略 DTO 也不进入同步记录。

测试见 crates/core/tests/timing.rs、src/components/usage/format.test.ts 和 statsColumns.test.ts。

## 默认窗口尺寸

默认客户端尺寸为 1000×667 **逻辑像素**，对应原先 150% 缩放下约 1500×1000 的设计视口。Tauri 配置和 Windows 关闭系统装饰后的 `LogicalSize` 使用同一尺寸，随后居中；100% 缩放时客户端为 1000×667 物理像素，150% 时约为 1500×1000（高度有取整差异）。系统 DPI 负责缩放，避免在 100% 下仍强制打开 1500×1000 的大窗口。最小尺寸维持 900×620 逻辑像素，阴影／外框不计入客户端尺寸。
