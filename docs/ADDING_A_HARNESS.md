# 新增工具适配器

[文档索引](README.md) · [架构与数据契约](ARCHITECTURE.md)

仓库用 harness 表示一种编程工具的数据来源。接口定义在 [harness/mod.rs](../crates/core/src/harness/mod.rs)，已有适配器由 `Registry::default()` 注册。

## 接入步骤

1. 在 `crates/core/src/harness/` 添加独立模块，实现 `HarnessAdapter`。
2. 在注册表加入适配器，声明稳定 ID、名称、默认目录、解析器版本及 usage／transcript／delete 能力。
3. `parse()` 返回 `ParseResult`：包含 `Vec<ParsedUsage>`、诊断及完整性标记；解析不调用数据库写入、价表、Tauri 或网络。
4. 实现会话与消息投影，正文不进入 `UsageRecord`。
5. 在 `tests/fixtures/<harness>/` 增加合成样本，在 `crates/core/tests/` 验证采集、存储和同步；补前端图标或阅读风格。

前端工具注册表来自后端，未知品牌可使用通用图标。不要让普通新增来源要求改写同步协议或数据库结构；确实新增计量语义时另行设计兼容策略。

## 发现与读取接口

| 接口 | 用途 |
| --- | --- |
| `discover()` | 发现本来源的规范文件，避免迁移副本和临时文件重复入账 |
| `read_source()` / `read_usage_source()` | 将 JSONL、压缩日志或原生存储转成用量读取输入 |
| `parse()` | 生成稳定用量事实、诊断与 `complete` 状态 |
| `context_id()` / `context_events()` / `parent_context()` | 提供祖先身份与最小用量上下文 |
| `session()` / `sessions()` | 单文件或多会话来源的列表投影 |
| `read_metadata()` / `enrich_metadata()` | 摘要读取及来源级外部标题等补充 |
| `read_messages()` / `messages_for_session()` | 按原生会话读取正文 |
| `delete_session()` / `delete_source()` | 在能力允许时执行删除 |
| `allows_price_estimates(has_tokens)` | 限制不具备可靠计费维度的来源参与估价 |

SQLite 来源应使用只读一致性事务，同时考虑 WAL。一个数据库包含多个会话时，发现、列表、正文与删除均须保留原生 session ID。摘要采样只用于展示，不能替代完整用量计数。

会话缓存由 [sessions.rs](../crates/core/src/sessions.rs) 管理：摘要按文件元数据失效，SQLite 考虑 WAL；正文按工具、规范路径和 session ID 区分，最多 8 个快照／96 MiB。`session_transcript` 按最多 150 条／约 256 KiB 分页，长块通过 `session_block` 按 Unicode 字符读取。引用绑定快照指纹，续写不能将旧引用解释为新正文。

## 身份与计量规则

- 优先原生事件 ID；无原生 ID 时使用不依赖 Token、价格和路径变化的稳定身份。
- 累计计数必须先变成事件增量，祖先历史、重放和副本不产生新用量。
- `input` 是新增输入，与缓存读写互斥；推理是输出子集，1h 缓存写入是全部缓存写入的子集。
- 时间和 provider 必须有原生日志依据，不能使用导入时间、当前配置或模型厂商补造。
- 缺失、损坏和真实零分别处理；异常来源返回诊断及 `complete=false`，保留已有记录并重试，禁止全范围撤销。
- 解释规则改变时更新 `parser_version`，并验证历史 ID、费用快照和重复扫描。新用量投影字段要同步更新读取白名单。
- 删除入口由 `Descriptor.delete` 和会话 `canDelete` 控制；不能只在前端放开按钮。

Copilot 兼容层受现有非空 Token 字段约束，缺失维度填 0；这是明确记录的例外，不应成为新适配器默认做法。模型档位分组只应用于 Codex，不从模型名为其他工具补造档位。

## 可参考的实现

- [OpenCode](../crates/core/src/harness/opencode.rs)：V1／V2 SQLite，同会话优先新表；纯 V2 会话不能只靠旧 `session` 表发现，列表和用量来自一致性读取。
- [Codex](CODEX.md)：逻辑会话与物理分段、祖先基线、运行日志档位证据和独立标题来源。
- [DSH](DSH.md)：版本文件选择、多帧 Zstandard、稳定消息身份和继承边界。
- [Claude](CLAUDE.md)：同请求内容块合并、请求计时关联和推理子集。
- [Copilot](COPILOT_IMPORT_DESIGN.md)：固定历史清单划分两个已注册来源，未知用量不能伪装成已确认免费。

用量扫描对发生变化的来源重读完整用量投影，并持久缓存可复用的读取结果。若增加字节游标或其他增量策略，仍须保持身份、修订、完整性和父会话基线语义。检查命令见[验证指南](VALIDATION.md)。
