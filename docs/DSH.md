# DeepSeek Harness 兼容

应用 ID 为 `dsh`，全名为 DeepSeek Harness，请求列表短名为 DSH。用量、模型／应用筛选和本机会话阅读使用已有接口；同步事实兼容旧版美元费用，并扩展可选的原币／汇率及分时计价快照。

## 日志位置

默认读取 `$DSH_HOME/sessions`；未配置环境变量时读取 `~/.dsh/sessions`。在「应用 → DeepSeek Harness」添加目录并启用采集；支持填写 Harness 主目录、sessions 目录、单个会话目录或具体日志文件。本机路径可填写 `E:\DeepSeekHarness` 或 `E:\DeepSeekHarness\sessions`。

只发现 `session.jsonl`、`session.jsonl.zstd`／`.zst` 和 `session.vN.jsonl` 对应的压缩文件，不读取凭据、插件或统计缓存。版本文件名必须为规范数字形式，忽略 `.v0`、前导零、大小写变体和临时文件。一个会话目录存在多个版本时只选最高版本，并验证文件名与头部版本一致。相同版本有多种物理编码时报告冲突，未知版本报告不支持，避免悄悄回退到旧历史。

本机验证覆盖两种独立安装：

| 安装 | 实际版本 | 存储格式 |
| --- | --- | --- |
| 全局 `dsh` CLI | `0.1.0-rc.7` | v0，独立流事件／打包片段 |
| DeepSeek Harness App 自带 CLI | `0.2.0-rc.2` | v4，`session.v4.jsonl.zstd`，最终助手消息内嵌 `data.stream` |

App 迁移保留的 v0 和新 v4 文件不会重复计数。v1–v3 中间代文件暂未支持；如它们是目录中最高版本，会明确诊断。

## 用量与会话

* 以 `assistant/message.data.usage` 为一次模型调用的计数，原生 `message.id` 提供稳定身份。流片段、重放替换节点和迁移副本不增加用量；空正文但携带用量的消息仍入账。
* `inputTokens` 已是新增输入，不再减去缓存。缓存读写单独入账；`reasoningTokens` 是 `outputTokens` 的子集，不重复相加。未报告的可选缓存／推理维度为零；缺少必需用量、负数或异常子集会诊断并暂缓完整性确认。
* v0 按 `seedLength` 排除继承历史；v4 已移除此头字段，改用 `isSeeded` 和最后一个 `session/end-seed.data.inherited=true` 的序号作为边界。嵌套分叉的更早标记不覆盖最终边界；普通恢复的空 `data` 标记不排除历史。头部与边界矛盾时诊断并暂缓。没有继承历史的新建子代理保留自己的用量。
* 从每个 Zstandard 帧读取完整日志。截断压缩帧不会撤销已采集记录，完整写入后重试；JSON 半行、事件序号缺口和损坏的打包片段也不会触发全范围撤销。
* 会话阅读支持用户正文、思考、最终回复和工具调用／结果；运行时上下文、技能目录等注入消息默认隐藏，不作为用户提问。日志计时按步骤起点、模型消息完成时间和首个非空流片段估算，显示 `≈`；v4 重试的 `assistant/attempt` 流可提供首次输出时间，但不会额外计一次用量。速度扣除首 Token 等待，以输出阶段计时，过滤不足 100 输出 Tokens 或 100ms 的窗口；缺少流式首字依据时改为明确标注 E2E 的端到端估算，两种口径不混合汇总。见[计时说明](TIMING.md)。

日志没有原生费用时沿用模型价表估算；没有匹配价格则保留未知，明细和会话摘要显示“未定价”。不从 Token 总量推测真实账单。删除、恢复命令和外部附件／图片读取暂未启用；删除及继续会话在 dsh 原应用中操作。

## DeepSeek 官方计价与汇率

计价规则依据 [DeepSeek 官方人民币价表](https://api-docs.deepseek.com/zh-cn/quick_start/pricing/)，版本 `deepseek-cn-2026-09-10`。当前规则只用于 2026-09-10 北京时间 12:00 起的请求，不将新价表回套到更早的未知费用。计价按日志事件时间估算，详情明确标出该时间依据；它不是供应商账单确认时间，跨高峰边界的结果可能与实际账单有差别。

请求／模型列表、筛选和同步记录的 `model` 保留日志原文。计价模型归一为 `deepseek-flash`、`deepseek-v4-pro`：Flash 兼容 `deepseek-v4-flash`、`deepseek-v4-flash-vision-exp`、`deepseek-v4.1-flash`、`deepseek-v4-1-flash`；两类支持模型命名空间、版本日期、`[1m]` 和已支持的推理档位后缀。不会将 V3、`deepseek-chat`、`deepseek-reasoner` 或未知自定义后缀映射为当前 Flash。

单价单位是人民币／百万 Tokens，以下按空闲／高峰排列：

| 模型 | 未命中输入 | 缓存命中 | 输出 |
| --- | --- | --- | --- |
| deepseek-flash | 1 / 2 | 0.02 / 0.04 | 4 / 8 |
| deepseek-v4-pro | 4.5 / 9 | 0.15 / 0.30 | 13.5 / 27 |

高峰固定为北京时间周一至周五 09:00–12:00、14:00–18:00，节假日除外；周末始终为空闲，包含调休上班的周末。内置完整的 [2026 年节假日安排](https://www.gov.cn/zhengce/content/202511/content_7047090.htm)，可在定价界面保存其他年度的完整日期列表。没有对应年度日历时保持未定价，避免默认按普通工作日估算。统计显示时区不参与选价。

「用量统计 → 设置 → 定价」可配置计价汇率，方向为 **1 USD = X 原币**；价格计算使用十进制字符串，美元费用 = 原币费用 ÷ 汇率。计价汇率与「显示汇率」独立：前者影响新记录和未定价记录入账，后者只改变界面显示。非美元手动价表也采用同一换算过程。未配置所需汇率时仍保存原币估算与单价快照，美元费用保持未知。

`cost.snapshot` 保存原币单价和币种；`cost.conversion` 保存原币金额、汇率及配置时间；`cost.context` 保存高峰／空闲、计价时间依据及节假日日历哈希。内部汇总和同步金额继续使用 `amountUsd`。通过原币入账或显示币种换算的单条成本显示 `≈`，明细提供原币金额和汇率；总成本卡片与侧栏今日成本仅显示配置的币种符号，不加 `≈`。旧记录缺省币种为 USD，新增字段可选，既有用量身份不变；同步双方升级后可完整保留新增元数据。

保存价表／计价设置仅补齐未知费用，已有费用冻结。更正同一次请求的 Token 量时沿用其原单价、时段和汇率。定价界面的重算按钮只更新本机事件级会话估算：跳过原生上报费用、迁移汇总、远端记录和删除记录，无法可靠计价时不覆盖已有金额。models.dev 不会覆盖官方 DeepSeek 分时规则或手动设置的其他模型价格。

当前仅统计模型 Token 费用。Search／Fetch 的独立成功次数尚未采集，因此界面明确提示工具费未统计；不根据工具名称、返回条数或 Token 数推算调用费用。官方说明 Harness 内相关调用从北京时间 2026-11-01 00:00 起收费，计费条件见[网页搜索与抓取](https://api-docs.deepseek.com/zh-cn/guides/search/#计费)。

## 原生供应商与 BYOK 计价

DSH 从每条 `assistant/message.data.message.source.provider` 识别供应商；OpenCode 读取 V1 的 `providerID` 或 V2 的 `model.providerID`；Codex 读取 `session_meta.payload.model_provider`，并保留显式的逐轮 `turn_context` 覆盖。日志原始路由 ID 写入可选的 `UsageRecord.provider`，用于请求明细、供应商分组与所有用量筛选。它说明客户端选择的路由，不从模型名或当前配置反推历史上游身份。没有可靠字段、原日志已丢失或迁移汇总记录继续显示会话日志占位名。

价表以 `(provider, modelId)` 区分，同一模型可保存多套手动价格；配置中的供应商 ID 应与日志一致。`deepseek` 为内置 DeepSeek 官方供应商的计价别名，列表仍显示原始 ID 对应标签。已配置供应商优先匹配专属价表；未配置的供应商按模型名称匹配通用 models.dev 参考价，快照记录 `sourceKey`，金额口径为 `model_reference`。模型匹配仍支持命名空间、日期、推理档位以及上述 DeepSeek 名称归一，列表中的原始模型文本不会改变。models.dev 没有可匹配条目时保持未定价。原有供应商未知的非 DeepSeek 记录可继续使用兼容的通用内置／手动价表。

开启 models.dev 自动同步后，会将日志中尚缺价格的模型纳入匹配，而不只导入常用模型；相同未匹配模型在当前渲染进程内六小时最多尝试一次，避免每轮扫描反复请求。手动同步仍可立即重试。关闭自动同步后可用 models.dev 选择器手动导入。导入的通用参考价与官方／Go 专属价表并存。

`opencode-go` 的 DeepSeek 价表独立依据 [OpenCode Go 官方说明](https://opencode.ai/v2/docs/console/go/)，版本 `opencode-go-deepseek-2026-10-19`。单价为 USD／百万 Tokens，按空闲／高峰排列：

| 计价模型 | 输入 | 缓存读取 | 输出 |
| --- | --- | --- | --- |
| deepseek-flash | 0.15 / 0.30 | 0.003 / 0.006 | 0.60 / 1.20 |
| deepseek-v4-pro | 0.66 / 1.32 | 0.022 / 0.044 | 1.98 / 3.96 |

高峰为 UTC 周一至周五 01:00–04:00、06:00–10:00，结束边界不包含在内；官方 Go 页面未列中国节假日豁免，因此不复用 DeepSeek 官方日历或人民币汇率。这里计算的是 `subscription_usage`（套餐额度折算），不分摊 Go／Go Plus 月费，也无法从日志判断余额扣费、剩余额度或具体套餐。其他 Go 模型须另行配置供应商价格，不能套用 DeepSeek 官方价表。历史 Go 估算使用本次核对版本，不能视为历史账单确认金额。

升级解析器版本后，正常扫描会一次性重读可用日志并回填供应商，保持稳定请求身份和 Token 口径；后续扫描继续使用持久化读取缓存。旧模型通用价表错误用于其他供应商的估算会按正确供应商重新选择，原生日志直接报告的费用优先保留。新增字段不需要 SQLite schema 迁移。同步协议升级为 v2，服务端保留 v1 接收能力；旧服务端会被客户端预检拦截，保留待同步队列并提示升级，所有读写这些记录的客户端也应升级，以完整保留供应商元数据。

格式依据为两种本机安装的 `@deepseek-ai/dsh-session`、`dsh-session-persistence-jsonl`、`dsh-session-format-*`、`dsh-session-stats` 和 `dsh-llm` 类型／持久化实现，以及只读历史快照和分别发起的 CLI／App 真实请求日志。Rust 适配器独立实现；仓库仅保留合成样本。上游项目：[deepseek-ai/deepseek-harness](https://github.com/deepseek-ai/deepseek-harness)。
