# 新增 harness

1. 在 `crates/core/src/harness/` 添加独立适配器，实现 `HarnessAdapter`。
2. 在 `Registry::default()` 注册。适配器描述包括 ID、名称、默认目录、解析器版本和能力。
3. `parse()` 只返回 `ParsedUsage`，不调用 SQLite、价表、Tauri 或网络。
4. `session()` / `messages()` 输出本机列表及阅读器的消息块。正文不能进入 `UsageRecord`。
5. 在 `tests/fixtures/<harness>/` 添加合成样本；在 `crates/core/tests` 验证准确总量、重扫、追加、修订和异常格式。

非 JSONL 格式可覆盖 `discover()` 和 `read_source()`，将原生存储转换为本适配器的逻辑事件。一个数据库包含多个会话时覆盖 `sessions()`、`messages_for_session()` 和 `delete_session()`；后端与前端会传递原生 sessionId。祖先解析通过 `context_id()` / `context_events()` / `uses_ancestry()` 提供，只保存不含正文的最小用量上下文。通用编排器不识别 Codex 或其他 harness 的字段。

## 身份与统计规则

* 原生事件 ID 优先。若源不提供事件 ID，使用原生会话身份＋不依赖 Token/价格的稳定原始序号。解析规则修复不能让旧事件变成新事件。
* 累计字段必须先转换为事件增量；祖先历史和重复快照不计入新用量。无法恢复基线时返回诊断和 `complete=false`。
* `Tokens.input` 为新增输入。缓存读写与输入互斥；推理 Token 为输出子集，1h 缓存写入为缓存写入子集。
* 时间必须来自可验证的原始记录，不能回落到导入时刻。
* `complete=false` 保留原记录并重试，不允许执行全范围撤销。真正的零用量和无法解释的用量必须区分。
* `parserVersion` 在解释规则改变时升级，使文件指纹缓存失效。
* 正文读取和用量读取共享目录配置，但可以采用不同的解析投影。

## 维护边界

第一版用量扫描对变更文件全量读取；Codex 为恢复父会话基线建立当轮时间线索引。将来增加字节游标时必须维持相同的事件身份、修订和完整性语义。

UI 的注册表来自后端；新增适配器后应补充品牌图标和可选阅读风格。未识别品牌使用通用图标。按未知 harness 分组和同步不应要求修改协议或数据库 schema。

OpenCode 已作为原生 SQLite 适配器实现这一扩展方式，见 `crates/core/src/harness/opencode.rs`。V1 和 V2 同库存放时按会话选择新表，避免重复；不要仅依据旧 `session` 表判定 V2 会话是否存在，V2 新会话写入 `session_v2`。统计和会话列表必须来自同一读取事务。适配器描述的 `delete` 由引擎注入会话 `canDelete`，前端据此决定是否允许删除。

当前解析器覆盖测试样本里的 JSONL 和 Zstandard 压缩 JSONL。分页续写及其他版本日志格式应增加对应样本和适配，遇到不支持的来源必须呈现诊断，不把缺少数据当成零使用。
