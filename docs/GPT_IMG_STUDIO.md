# Image Studio 用量

Image Studio 按模型上报 Token 用量。一次 Responses 调用可以产生一条语言模型用量和一条或多条图像模型用量；自动命名、提示词润色也会产生各自的模型记录。Scope 累加这些模型的 Token 和来源估算费用，不统计图片张数，不把模型记录算作 HTTP 请求，也不用于请求均价或模型速度。

## 数据来源

更新并启动 Image Studio 后，原有 `usage_events` 会回填到同一数据库的 `model_usage` 表。每次新请求的原始事件和模型记录在同一事务提交，历史报价直接保留，不随升级重新计算。`usage/models.jsonl` 是可重建的镜像，`usage/usage.jsonl` 仍保留原始请求事件。

Scope 首次升级时会添加本机 Image Studio 来源；如果数据库尚不存在，来源默认关闭，可在应用设置启用并指定路径。默认读取应用数据目录的 `studio.sqlite3`，支持 `storage-location.json` 中的迁移目录。选择数据目录时优先读取 SQLite（含 WAL），不会同时扫描镜像。也可以显式选择 `models.jsonl`。

## 字段和统计口径

- 模型记录 schema 为 `gpt-img-studio.model-usage`，`schemaVersion: 1`，稳定 ID 为 `请求 UUID:component:序号`。Scope 再加入 Studio 安装身份，重复扫描和同机副本使用相同身份。
- `modelUsage` 保存原始请求 ID、操作、模型类型、可空 Token、缓存可用性和来源价格快照。它不包含提示词、图片、密钥或响应正文。
- `totalInput + output` 是总 Token。缓存细分已知时使用新增输入、缓存命中、缓存写入三部分；未知时只累计总输入，细分显示未上报，不推算该条记录的缓存命中率。总览计算命中率时排除缓存未知的记录，其余已知记录按 Token 总量加权。推理 Token 已包含在输出中。
- 金额是 Studio 在调用时计算的参考估算，`source_estimated` 不等于供应商实际扣款。`estimated` 表示可计算，`upper_bound` 表示缺少缓存折扣细分的费用上界，`unknown` 表示无法计价。Scope 的改价和重新计价不会覆盖来源报价。
- `modelUsageCount` 单独计数，HTTP `totalRequests` / `requestCount` 对这类记录为 0。费用覆盖的 `pricedCount` / `unpricedCount` 包含请求和模型记录，缺失用量另计 `incompleteUsageCount`。

## 升级与同步

本地 schema v8 仅给查询投影增加 `model_usage` 列并重建投影；不改写既有事实载荷、冻结费用和同步游标。正式升级前应保留一致性数据库副本和匹配旧程序。

同步协议仍为 v4，增加 `model_usage_v2` 能力。旧服务器不支持时，新模型记录保留在本机待传队列，其他来源继续同步。新版服务器拒绝向不声明该能力的旧客户端下载含模型记录的批次，避免丢字段后推进游标。需要跨设备查看时，先升级服务器，再升级各客户端。

## 验证

`tests/fixtures/gpt-img-studio/model-usage.jsonl` 与 Studio 的导出契约样本一致。核心回归覆盖 SQLite WAL、JSONL、未知值、重复扫描、冻结费用、混合请求均价和 v7→v8 投影升级；服务端回归使用实际 loopback HTTP 验证延后上传和无损往返。

```powershell
cargo test --release --locked -p harness-scope-core --test gpt_img_studio
cargo test --release --locked -p harness-scope-service --test model_usage_sync
```

实际调用后可用 `scripts/compare_img_studio.py --studio-db <studio.sqlite3> --scope-db <harness-scope.db> --report <报告.json>` 只读对账。脚本核对原始组件、模型表、Scope 身份、可空 Token 和冻结报价；可选 `--baseline <升级前 metadata.json>` 只汇总本次新增请求，但仍核对全部历史模型记录。对账通过仅表示两侧采集一致，不表示估算与供应商账单一致。

## 供应商身份

GPT Image Studio 默认使用 OpenAI 像素图标。供应商名称取自请求发生时的配置快照，连接 ID（如 `legacy-image`）只作内部路由身份。Studio 新日志仅导出供应商 ID、名称、类型、API 计费渠道与端点分类，不导出完整 URL 或凭据。配置为 HTTPS `api.openai.com:443` 时显示 OpenAI Platform (API)，与 Codex 的 `openai` / ChatGPT Pro 订阅路由分开。

旧日志没有端点证据，显示原名称加 `(API)`，不根据今天的配置改写历史来源。重新扫描只补充已有日志中的供应商元数据，保留记录 ID、Token 和历史费用。携带供应商快照的同步数据要求 `model_usage_v2`；旧服务器继续保留这些记录在本地，不丢弃字段。
