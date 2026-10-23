# DeepSeek Harness 兼容

[文档索引](README.md) · [速度与首字](TIMING.md)

应用 ID 为 `dsh`，读取 DeepSeek Harness 的原生日志。当前适配 v0 和 v4；支持范围由 [Rust 适配器](../crates/core/src/harness/dsh.rs) 和[回归样本](../crates/core/tests/dsh.rs)定义。

## 日志位置与版本

默认读取 `DSH_HOME/sessions`，未配置时为 `~/.dsh/sessions`。在「应用 → DeepSeek Harness」添加主目录、sessions 目录、单个会话目录或日志文件并启用采集。

发现仅匹配规范的 `session.jsonl`、对应 `.zstd`／`.zst` 及版本化 `session.vN.jsonl` 日志，不读取凭据、插件和统计缓存。同目录只选最高版本，并核对文件名与头部；同版本多个编码或未知版本会诊断，避免回退旧副本重复计数。v1–v3 尚未适配。

| 格式 | 主要特征 |
| --- | --- |
| v0 | 独立流事件或打包片段，`seedLength` 指定继承边界 |
| v4 | 版本化文件，可使用多帧 Zstandard；助手消息和重试 attempt 内含 stream，使用继承结束标记 |

## 用量与会话

- 以 `assistant/message.data.usage` 计量模型调用，原生 `message.id` 提供稳定身份；流片段、重放替换和迁移副本不增加请求。
- `inputTokens` 已是新增输入，不再减缓存；缓存读写单列，`reasoningTokens` 是输出子集。
- v0 按 `seedLength` 排除继承历史；v4 使用 `isSeeded` 和最后一个 `session/end-seed.data.inherited=true` 边界，普通恢复的空标记不排除历史。没有继承历史的子代理保留自身用量。
- 多帧压缩均读取；截断帧、半行、序号缺口或损坏片段会暂缓完整性确认，保留既有事实等待重试。
- 会话阅读包含用户正文、思考、最终回复与工具步骤，注入上下文默认隐藏，不计为用户提问。

有可靠流式首字时估算输出阶段速度，否则回退明确标注的 E2E 口径。缺少计时不影响 Token 采集，门槛和汇总规则见[计时说明](TIMING.md)。本应用尚未启用 DSH 会话删除、恢复命令和外部附件读取。

## 原生供应商与价格

供应商取 `assistant/message.data.message.source.provider`，模型保留日志原文。路由字段表明客户端选择，不以当前配置或模型厂商反推历史上游。

价格按供应商和模型匹配：原生报告费用优先，已配置供应商使用专属价表；通用 models.dev 价格作为 `model_reference` 参考。DeepSeek 官方与 OpenCode Go 的分时规则各自独立，不能因模型相同而混用。

当前代码中的规则入口：

| 规则 | 定义 |
| --- | --- |
| DeepSeek 官方原币与节假日 | [pricing/deepseek.rs](../crates/core/src/pricing/deepseek.rs) |
| OpenCode Go 的 DeepSeek 计价 | [pricing/opencode_go.rs](../crates/core/src/pricing/opencode_go.rs) |
| Go 模型、上下文与额度配置 | [opencode-go-catalog.json](../crates/core/resources/opencode-go-catalog.json) |

规则保留来源、版本和适用时间。本文不重复维护单价表；它们是应用中的参考规则，不是实时账单查询。DeepSeek 官方规则使用自身时区和年度节假日，缺少对应日历时不猜测；Go 使用独立峰谷规则，不套用官方人民币汇率或日历。

## 原币、汇率与历史

「用量 → 设置 → 定价」中的计价汇率方向为 `1 USD = X 原币`，美元费用为原币费用除以该值。计价汇率影响新记录及未定价记录，显示汇率只影响呈现。缺少计价汇率时可保留原币估算，美元费用仍未知。

`cost.snapshot` 保存单价与币种，`cost.conversion` 保存原币金额和汇率，`cost.context` 保存时段、日志时间依据及日历版本。更新价格不自动改写已确认的费用；更正同一请求的 Token 时沿用原快照。显式重算仅处理符合条件的本机事件估算，跳过原生报告、迁移日汇总和远端记录。

Go 的 `subscription_usage` 表示套餐额度折算，不是分摊月费或现金扣款。Search／Fetch 独立工具费尚未采集，不能根据工具名称或 Token 数补算。

供应商与价格快照通过当前请求协议 v4 传输，读写这些字段的设备需有相应版本支持；版本与兼容统一见[架构说明](ARCHITECTURE.md#版本与兼容)。上游格式参考 [DeepSeek Harness](https://github.com/deepseek-ai/deepseek-harness)，仓库只保留合成日志样本。
