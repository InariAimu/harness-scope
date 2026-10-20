# 新增 harness

1. 在 `crates/core/src/harness/` 添加独立适配器，实现 `HarnessAdapter`。
2. 在 `Registry::default()` 注册。适配器描述包括 ID、名称、默认目录、解析器版本和能力。
3. `parse()` 只返回 `ParsedUsage`，不调用 SQLite、价表、Tauri 或网络。
4. `session()` / `messages()` 输出本机列表及阅读器的消息块。正文不能进入 `UsageRecord`。
5. 在 `tests/fixtures/<harness>/` 添加合成样本；在 `crates/core/tests` 验证准确总量、重扫、追加、修订和异常格式。

非 JSONL 格式可覆盖 `discover()` 和 `read_source()`，将原生存储转换为本适配器的逻辑事件。一个数据库包含多个会话时覆盖 `sessions()`、`messages_for_session()` 和 `delete_session()`；后端与前端会传递原生 sessionId。祖先解析通过 `context_id()` / `context_events()` / `uses_ancestry()` 提供，只保存不含正文的最小用量上下文。通用编排器不识别 Codex 或其他 harness 的字段。

会话阅读使用独立的 `read_metadata()` / `read_messages(path, session_id)` 投影：前者只读列表摘要，后者只读所选会话。默认回退到完整日志；大日志或多会话数据库应覆盖这两个入口。Codex／Claude 使用有上限的头尾采样，压缩日志仅读前缀；OpenCode 在只读事务内查询会话头或限定 `session_id` 的正文。不要把这些摘要用于用量计数。

独立于日志更新的原生列表信息可通过 `enrich_metadata(root, sessions)` 按来源批量补充，此入口在日志摘要缓存之外每次刷新执行。Codex 按当前来源查找 `session_index.jsonl`，以最后一条有效 `thread_name` 覆盖标题，支持活动／归档目录及日期子目录，不混用其他来源或当前 `CODEX_HOME` 的索引。索引缺失或损坏时保留日志回退：跳过环境、项目指令和 Page 上下文等注入消息，取真实提问；附件包装取 `My request` 部分，完全没有提问则显示 session ID。正文保留原始消息。

阅读缓存位于 `crates/core/src/sessions.rs`。摘要按修改时间、大小与创建时间失效，SQLite 还合并 WAL；正文按 harness、规范源路径及 session ID 区分，LRU 限制为 8 个快照／96 MiB。分页和长块引用绑定快照指纹，日志续写不会把旧引用解释为新正文。`session_messages` 保留完整兼容读取；`session_transcript` 每批最多 150 条／约 256 KiB，`session_block` 按 Unicode 字符分页取完整工具参数、输出及思考。正文缓存和引用不进入数据库或同步协议。

## 身份与统计规则

* 原生事件 ID 优先。若源不提供事件 ID，使用原生会话身份＋不依赖 Token/价格的稳定原始序号。解析规则修复不能让旧事件变成新事件。
* 累计字段必须先转换为事件增量；祖先历史和重复快照不计入新用量。无法恢复基线时返回诊断和 `complete=false`。
* `Tokens.input` 为新增输入。缓存读写与输入互斥；推理 Token 为输出子集，1h 缓存写入为缓存写入子集。
* 时间必须来自可验证的原始记录，不能回落到导入时刻。
* `complete=false` 保留原记录并重试，不允许执行全范围撤销。真正的零用量和无法解释的用量必须区分。
* `parserVersion` 在解释规则改变时升级，使文件指纹缓存失效。
* 正文读取和用量读取共享目录配置，但可以采用不同的解析投影。
* `ParsedUsage.provider` 仅取原生持久化路由 ID；缺失时保持 `None`。不要以当前配置或模型厂商代替历史供应商。新增投影字段时同步更新流式用量白名单，并提升解析器版本以回填历史记录。

## 维护边界

第一版用量扫描对变更文件全量读取；Codex 为恢复父会话基线建立当轮时间线索引。将来增加字节游标时必须维持相同的事件身份、修订和完整性语义。

UI 的注册表来自后端；新增适配器后应补充品牌图标和可选阅读风格。未识别品牌使用通用图标。按未知 harness 分组和同步不应要求修改协议或数据库 schema。

OpenCode 已作为原生 SQLite 适配器实现这一扩展方式，见 `crates/core/src/harness/opencode.rs`。V1 和 V2 同库存放时按会话选择新表，避免重复；不要仅依据旧 `session` 表判定 V2 会话是否存在，V2 新会话写入 `session_v2`。统计和会话列表必须来自同一读取事务。适配器描述的 `delete` 由引擎注入会话 `canDelete`，前端据此决定是否允许删除。

DeepSeek Harness 的 JSONL／多帧 Zstandard 适配器见 `crates/core/src/harness/dsh.rs`，版本差异见 [DSH.md](DSH.md)。目录发现限定原生规范 session 文件，按同目录最高版本选择并检查头部版本；使用稳定的 `message.id`，不把迁移后变化的事件序号作为请求身份。流片段不计用量；v0 继承边界使用 `seedLength`，App v4 使用最后一个 `session/end-seed.data.inherited=true`，普通恢复的空标记不能当成分叉边界。没有继承历史的新建子代理保留自己的全部用量。阅读器通过 `injected` 标记隐藏运行时上下文等注入消息，避免多算提问。

当前解析器覆盖测试样本里的 JSONL 和 Zstandard 压缩 JSONL。分页续写及其他版本日志格式应增加对应样本和适配，遇到不支持的来源必须呈现诊断，不把缺少数据当成零使用。
