# DeepSeek Harness 兼容

应用 ID 为 `dsh`，显示名称为 DeepSeek Harness。用量、模型／应用筛选、同步和本机会话阅读使用已有接口，不新增数据库或同步协议字段。

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
* 会话阅读支持用户正文、思考、最终回复和工具调用／结果；运行时上下文、技能目录等注入消息默认隐藏，不作为用户提问。日志计时按步骤起点、模型消息完成时间和首个非空流片段估算，显示 `≈`；v4 重试的 `assistant/attempt` 流可提供首次输出时间，但不会额外计一次用量。

日志没有原生费用时沿用模型价表估算；没有匹配价格则保留未知，明细和会话摘要显示“未定价”。不从 Token 总量推测真实账单。删除、恢复命令和外部附件／图片读取暂未启用；删除及继续会话在 dsh 原应用中操作。

格式依据为两种本机安装的 `@deepseek-ai/dsh-session`、`dsh-session-persistence-jsonl`、`dsh-session-format-*`、`dsh-session-stats` 和 `dsh-llm` 类型／持久化实现，以及只读历史快照和分别发起的 CLI／App 真实请求日志。Rust 适配器独立实现；仓库仅保留合成样本。上游项目：[deepseek-ai/deepseek-harness](https://github.com/deepseek-ai/deepseek-harness)。
