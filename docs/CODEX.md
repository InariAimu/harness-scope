# Codex 会话统计与请求档位

[文档索引](README.md) · [速度与首字](TIMING.md) · [订阅预测](PROVIDERS.md#codex-参考预测与-fast)

## 来源与身份

默认发现 `CODEX_HOME`（未设置时为 `~/.codex`）下的 `sessions` 和 `archived_sessions`。统计采集只扫描未压缩 JSONL；会话列表和正文仍可读取 `.jsonl.zst`。已配置来源以「应用」页为准。

压缩前已采集的用量和父会话缓存保留。此前未采集且只剩压缩文件的历史不会自动导入；缺少必要父会话基线时，续写段可能暂缓统计。由于采集不会读取压缩文件，应用内不提供删除压缩 Codex 会话，以免删除未采集的用量。

采集区分逻辑会话和物理 rollout 分段，处理分页续写、父会话继承、重复限额快照、累计高水位和远程压缩补记。原生模型与 provider 文本保留，不用当前配置反推历史路由。

fork 的 `history_base` 若提供父线程 ID、结束字节偏移和行序号，采集器会校验父文件身份、完整行边界、字节与行数一致、来源在读取时稳定，以及多个副本的前缀一致，再使用该父历史快照去重和计算累计基线；父会话停止时间早于 fork 创建时间不再单独阻止采集。边界不完整、父文件不可读、父线程分段无法唯一确认或校验失败时暂缓并保留已有事实。压缩文件仍不参与统计扫描，因此只有压缩父文件且无法验证原始边界时也会暂缓。无明确边界的旧格式继续使用原有时间线校验。

明确对应的历史事件沿用已有记录 ID 与冻结费用；分段遗漏的事件可补回，确认重复或继承的旧事实通过删除修订撤销。日志缺失、截断或父会话基线不足时保留已有事实，并暂缓无法确认的部分。新补回请求只能使用本机可用价格，不能恢复从未保存的历史单价。

分段映射、来源关联、读取缓存仅保存在本机。修订和删除沿用请求协议 v4，不重置设备身份、下载游标或 CC Switch 迁移分界。

## Standard、Fast 与未知

优先读取 rollout 中 `thread_settings_applied.thread_settings.service_tier` 和 `turn_context` 的明确设置，并将其关联到对应轮次。`priority`／`fast` 显示 Fast，`default` 显示 Standard；其他明确档位保留原值，缺少证据则未知。推理强度从对应日志设置读取。

部分版本的首轮 `turn_context` 不写档位。采集器可从**该来源目录**对应的 `logs_2.sqlite` 只读查询 `feedback_tags`，严格匹配会话 ID、轮次 ID、模型和时间，只补缺失设置。当前机器配置、其他来源数据库、后续轮次或速度都不能用来反推首轮。

匹配证据在本机缓存，仅保存轮次、模型、档位和时间，不保存或上传完整运行日志。延迟写入的证据会触发历史回填；日志数据库缺失、不可读或证据冲突时保持未知，不阻断 Token 采集。

这些字段表示本地有效请求设置，不保证服务端实际执行或结算档位。运行日志证据沿用同步中的 `thread_settings` 来源。只有 Codex 记录参加模型的档位分组，其他工具不会因使用相同模型名而归入 Fast／Standard。

## 回填与升级

解析器升级会重读可用的未压缩来源并沿原 ID 修订解析版本和请求元数据；压缩来源不会回填。仅补档位时 Token、时间、模型、计时及冻结费用不变，修订进入已有 outbox；重复扫描保持幂等。

已删除且从未缓存的首轮证据无法恢复，其他设备产生的记录也需要来源设备使用新版采集器补齐。数据库升级与应用回退应保留成对的程序、数据库备份，见[使用指南](USAGE.md#数据与备份)。

## 会话与计时

标题优先使用同一来源的 `session_index.jsonl` 中最后一个有效名称；缺失时回退真实用户提问或会话 ID。列表摘要跳过环境、项目指令及注入上下文，正文保留原始内容。

计时为日志 E2E 估算；首字依据不等同于网络首 Token。采集来源、样本过滤和模型分组见[计时说明](TIMING.md)。Fast 额度倍率和预测见[供应商说明](PROVIDERS.md#codex-参考预测与-fast)，不在解析阶段乘入 Token 或历史费用。

## 维护入口

实现见 [codex_usage.rs](../crates/core/src/harness/codex_usage.rs)、[codex_scan.rs](../crates/core/src/codex_scan.rs) 和 [codex_runtime.rs](../crates/core/src/codex_runtime.rs)。回归覆盖 [rollout 身份](../crates/core/tests/codex_rollouts.rs)、[请求设置](../crates/core/tests/codex_requests.rs)、[运行日志证据](../crates/core/tests/codex_runtime.rs) 及[标题](../crates/core/tests/codex_titles.rs)。解析规则变更必须检查历史身份、费用冻结和重复扫描行为。
