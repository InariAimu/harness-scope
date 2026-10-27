# Claude Code 会话统计

[文档索引](README.md) · [速度与首字](TIMING.md)

Claude Code 适配器读取 `~/.claude/projects` 的 JSONL；新配置会优先使用 `$CLAUDE_CONFIG_DIR/projects`。已保存的来源目录继续以「应用」页配置为准。支持范围以回归样本及本机实测版本为准。

## 请求与 Tokens

只将 `type=assistant` 且含有效 `message.usage` 的记录作为用量观察。同一 `(sessionId, message.id)` 的多个内容块保持同一个事件 ID，选择输出最多的快照，相同输出时选择后出现的记录；重扫与追加修订不会产生新的请求身份。

输入、输出、缓存读取和缓存写入分别使用原生字段。`output_tokens_details.thinking_tokens` 为输出子集，单独保存在 `tokens.reasoning`，不会再次加入总 Tokens 或费用。非法计数、推理大于输出、1h 缓存写入大于全部缓存写入均报告未完成，保留原有记录等待重试。

`cost-state` 是 Claude Code 的累计状态，可能包含可见助手消息以外的请求或不同计费口径，不作为额外用量或逐请求费用入库。累计输入／输出可能与可见请求存在差额；`hasUnknownModelCost=true` 时 Claude 自报费用也无法作为供应商账单参照。

## 请求耗时与元数据

`claude-v2-request-timing` 使用同一会话的 `api-request.id` 与助手块的 `requestRef` 关联。只有请求引用唯一、一个请求仅归属一个原生消息、起止时间有效且存在完成标记时，才用请求时间到该消息最后助手内容块时间生成 `claude_api_request_blocks` 估算。范围沿用 100 ms–1 小时；后续工具结果、用户等待、其他请求和整轮耗时不参与计算。

这表示客户端日志 E2E 窗口，包含客户端处理开销，不是精确网络耗时或纯模型生成耗时。由于缺少首字时间，速度回退到 E2E 口径，采用至少 200 输出 Tokens／1 秒门槛并显示 `≈`。助手块时间和 `thinkingDurationMs` 不能证明首 Token 到达时刻，首字保持未知。缺少请求关联的旧日志仍可统计 Tokens，耗时保持未知。

推理强度优先取关联请求的 `params.output_config.effort`，再使用该助手记录的 `perTurnEffort`／`effort`，不向其他请求继承。服务档位取同一条助手用量中的 `service_tier`，来源为 `claude_usage`，不代表供应商、账号或套餐；`usage.speed` 也不作为 tok/s。

升级解析器会重读已有来源，沿原 ID 回填计时、推理细分及请求元数据。计费 Token 分类不变时，已有费用及完整单价快照保持原样，修订和 outbox 按现有机制更新。同步仍使用协议 v4，但服务器及读取这些记录的客户端需要更新 Claude 来源白名单。

新服务器的健康检查通过 `recordCapabilities: ["claude_request_timing_v2"]` 声明支持。服务器尚未声明时，客户端将含新 Claude 计时／请求元数据的修订保留在本机 outbox 并显示提示，继续上传其他兼容记录；筛选发生在上传批量截取之前，避免大量待传 Claude 记录阻塞后续请求。不会剥离元数据或提前确认这些修订。服务器更新并声明能力后自动补传；升级读取这些记录的其他客户端也需包含新来源白名单。

## 供应商与会话标题

已验证的 2.1.306 样本没有供应商路由 ID 或请求 endpoint。`provider` 保持未知，界面显示「会话日志」；不能以模型名、当前环境变量或 API Key 反推历史路由。不能用个人会话 ID 的特例替代通用来源识别。

会话标题优先使用同一会话的 `custom-title`，再取 `ai-title`，最后回退实际用户提问或会话 ID。摘要忽略运行时命令包装、工具结果和 `isMeta` 注入上下文。列表仍使用有上限的头尾采样；未落入采样范围的标题可能回退，不为读取标题完整扫描大日志。

## 本机对账

```powershell
python scripts/compare_usage.py --scope-db <统计库> --harness claude --claude-projects <projects目录或单个JSONL> --session-id <会话ID> --report output/claude-comparison.json
```

原生对账检查可见请求数及各计费 Token 分类，不将累计 `cost-state` 混入参照；报告明确标记费用未对比。可同时提供 `--ccswitch-db` 比较存在的原生会话记录。未知费用、缺少参照和与实际账单一致是不同状态。
