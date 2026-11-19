# Gemini CLI 与 Antigravity

两个只读适配器分别注册为 `gemini` 和 `antigravity`。已有安装首次打开新版时添加默认来源；之后删除或禁用来源不会自动恢复。同步服务器不添加本机来源。

- Gemini CLI：`~/.gemini/tmp/**/session-*.json` 和 `session-*.jsonl`，默认会话通常位于 `<project>/chats/`。同名 JSONL 优先于旧 JSON。可指定单个会话文件或目录。
- Antigravity：`~/.gemini/antigravity/conversations/*.db`，当前验证格式为 Antigravity 44.9.1。只读 SQLite 事务包含 WAL；`conversation_summaries.db` 只用于补充标题。旧版 `.pb` 会话不解密、不猜测字段，需要先由 Antigravity 迁移。

## 用量口径

Gemini CLI 使用原生消息 ID、时间和模型名。消息追加更新按同一 ID 合并，JSONL rewind 只影响会话阅读，已经发生的生成用量保留。输入为 `input - cached + tool`，输出为 `output + thoughts`，思考 Token 是输出子集；与原生 `total` 核对。没有 Token、字段非法、累计不一致或 JSONL 尾行未写完时提示延后处理，保留已有事实。旧 JSON 快照中已经被原工具移除的消息无法恢复。

Antigravity 只累计 `gen_metadata` 的 `CortexStepGeneratorMetadata.chat_model.usage`，不重复累计 `steps.metadata.model_usage`。原生输入已经排除缓存；输出包含思考。Proto3 的已存在 usage 消息内省略标量表示真实零，整个 usage 消息缺失表示未知。时间取原生关联步骤的创建时间，复制／分支保留原始来源身份和请求身份。模型优先使用 `response_model_full`，再取 `response_model`、`model_display_name`，不从数字枚举或当前设置猜测模型。未知模型照常统计 Token，未匹配到价表时显示未计价。

可匹配的价表只估算模型参考费用，不代表订阅扣款或 Credits。两个来源不从模型厂商、当前登录、导入时间推断供应商和历史时间。正文、思考、工具参数和结果只进入本机阅读器，不进入持久用量缓存与同步 DTO。

Antigravity 的首字时间使用原生 `chat_model.time_to_first_token`，输出阶段使用 `chat_model.streaming_duration`，总耗时为两者相加。读取器和解析器版本升级会回填现有文件，保留生成身份、Token 与费用快照。计时仅保留本机，不向尚不支持该计时来源的同步服务器发送。缺失、负数、非法纳秒、过长时间都保持未知，不补造零；速度仍遵守现有的最少输出 Token 和有效时长门槛。Gemini CLI 会话文件未保存这两个原生指标，不从用户消息间隔猜测首字或速度。

## Google 供应商分类

「供应商」新增 **Google / Gemini / Antigravity**，无需 API Key。新增账号默认关联这两个来源的会话路由，可取消或修改关联，并填写计划名称、月费及订阅周期。查询与筛选保留 `gemini (Session)` / `antigravity (Session)` 路由键，供应商列按工具来源默认显示 Google；已关联账号名称优先。提示说明来源分类不等于已验证的上游或订阅扣费，不改写历史日志的原生路由。

本次不接入官方剩余额度查询、OAuth 凭据和订阅扣费换算。Google 账号不轮询网络，也不显示虚构额度；模型用量按关联的本机及已同步设备记录汇总。一条来源路由只能关联一个账号，日志无法可靠区分同一路由的多个 Google 账号。

Antigravity 阅读器投影用户消息、模型响应、思考和工具调用；未知插件的结果不伪装为空或成功。当前不支持删除原会话、恢复命令、媒体解码或旧版加密 `.pb`。

## 格式依据与验证

- [Gemini CLI 原生记录类型](https://github.com/google-gemini/gemini-cli/blob/main/packages/core/src/services/chatRecordingTypes.ts)
- [Gemini CLI 原生记录读写](https://github.com/google-gemini/gemini-cli/blob/main/packages/core/src/services/chatRecordingService.ts)
- Antigravity 会话的 SQLite 模式，以及安装包中 `cortex.proto`、`codeium_common.proto` 和 `gemini_coder/proto/trajectory.proto` 的字段描述。解析器直接读取会话数据库，无需安装包、守护进程、端口或登录凭据。

合成样本位于 `tests/fixtures/gemini/` 和 `tests/fixtures/antigravity/`。运行 `cargo test --release --locked -p harness-scope-core --test google_clients`；原生日志只读验证为显式忽略的 `inspect_local_antigravity_read_only`，通过 `HARNESS_SCOPE_ANTIGRAVITY_PROBE` 指定目录后单独运行，不导入正式数据库。
