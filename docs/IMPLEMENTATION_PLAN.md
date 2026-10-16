# Harness Scope 详细实现计划

版本：0.1；日期：2026-10-16。上游参考：CC Switch `e8880e6f57e25bce1bf3125cc6e33d9fb2b0d806`。

## 1. 产品范围与约束

独立的 AI 编程工具用量统计及本机会话浏览器。不同设备的原生会话互不重叠；服务器仅同步用量、费用来源和必要身份字段。完整对话、工具参数、项目路径、图片及恢复命令留在本机。

首版支持 Claude Code、Codex；Windows 为主要验证平台。React + Tauri 桌面应用，Rust 统计核心及 HTTP 服务，SQLite 存储。开发时可通过本机 HTTP 服务在浏览器验证同一前端。

保留 CC Switch 的页面结构、视觉 token、用量组件和会话阅读器。应用页管理 harness、日志目录及采集开关；设置页提供主题、语言、服务器、迁移和数据设置。移除模型路由、供应商切换、认证接管等业务依赖。

费用计算沿用固定上游版本的内置价表、模型匹配及 models.dev 导入规则。记录实际使用的价格快照；不声称估算费用等于账单，也不声称导入价等于请求发生时的历史价。

## 2. 模块与依赖方向

```text
src/                              React 页面、复用组件、IPC/HTTP 适配
src-tauri/                        桌面启动、命令桥接、系统对话框
crates/core/src/
  model.rs                        用量事实、费用来源、配置、同步 DTO
  harness/mod.rs                  HarnessAdapter trait 与注册表
  harness/claude.rs               Claude 日志解释
  harness/codex.rs                Codex 累计、重复快照与祖先处理
  harness/files.rs                文件发现、稳定指纹、JSONL 读取
  pricing.rs                     内置价表、Decimal 计算及价格快照
  store.rs                       SQLite、事务、身份、修订、上传队列
  migration.rs                   CC Switch 只读快照迁移
  query.rs                       范围、设备、模型和应用统计
  engine.rs                      本机业务编排与稳定命令接口
crates/service/                   本机开发 API、自部署同步 API、CLI
tests/fixtures/                   合成且不含真实对话的兼容样本
docs/                            协议、阶段、验收与新增 harness 指南
third_party/                     上游许可证、来源及修改记录
```

核心 crate 不引用 Tauri、React 或服务器路由。读取器不直接访问数据库、价格服务及网络；返回事实、诊断和源范围，由存储层统一计价、修订和同步。

会话发现与用量采集共享配置和目录。会话阅读与用量解释分开，避免按用于显示的截断文本反推用量。新增 harness 只需实现适配器、注册描述和样本，不应修改 SQLite 或同步协议。

### 2.1 HarnessAdapter 合约

每个适配器公开稳定 ID、名称、解析器版本、默认目录和能力。入口接收已配置目录及扫描上下文，返回：

* `ParsedUsage` 列表：原生身份、发生时间、模型、Token 分类、原生费用及来源。
* 本机会话列表和阅读消息；正文不进入同步 DTO。
* 文件身份及是否完整读取；诊断包含来源、类型、说明及是否可重试。

第一版采用变更文件全量重读和稳定事件 ID 的幂等 upsert，优先正确性；后续可在不改变事件身份的前提下增加字节游标。完整文件成功重解析后，才允许撤销该文件中已经不存在的旧记录。半行、无法解释的累计基线或缺失父会话不得静默按零成功处理。

### 2.2 能力表

能力分别标注 `usage`、`transcript`、`delete`、`resume_command`。会话可读不代表用量可采集；不支持的能力不展示可执行按钮。当前 Windows 支持复制恢复命令，一键外部终端恢复列入后续阶段。

## 3. 数据结构

所有同步字段使用 camelCase；时间为 UTC Unix 秒，扫描与同步状态时间可使用毫秒但字段名称必须明确。金额用十进制字符串，数据库不使用浮点计算费用。

### 3.1 用量事实与记录

| 字段 | 含义 |
| --- | --- |
| deviceId | 数据所属设备的持久 UUID；随数据库备份恢复 |
| id | 设备内稳定事件 ID，包含 harness 命名空间 |
| harness / parserVersion | 适配器标识与实际解析规则版本 |
| sessionId / nativeEventId | 原生身份；历史日汇总允许缺失 |
| occurredAt | UTC 发生时间；无法恢复时记录诊断，不伪装当前时间 |
| model / rawModel | 实际分组模型及原始名称 |
| grain / count | event 或 legacy_day；原始记录数 |
| tokens | 归一化的互斥 Token 维度，见下 |
| cost | 金额及来源，见下 |
| recordSource | session、ccswitch_detail、ccswitch_rollup |
| revision | 设备内该记录单调递增的修订号 |
| deleted | 撤销标记；同步保留 tombstone |
| legacyDate / legacyTimezone | 旧汇总的原日期和用户确认的时区 |

本地还保存 sourcePath、文件指纹和原始解析辅助信息；同步 DTO 明确排除路径和正文。记录 ID 不包含费用、Token 数值或解析器版本，保证补全和修复更新原记录。

### 3.2 Token 分类

`input` 为新增输入，`output` 为输出，`cacheRead` 为缓存读取，`cacheWrite` 为缓存写入；总量只将这四项相加。`cacheWrite1h` 是 cacheWrite 子集，`reasoning` 是 output 子集，均不重复计入总量。

适配器保留原始输入语义：Claude 通常报告新增输入；Codex 的输入包含缓存，必须扣除缓存后再进入统一统计。负值、溢出、缓存大于输入等异常必须诊断。

### 3.3 费用

`CostRecord` 至少包含：

* `amountUsd: string | null`：未知与已知零费用分开。
* `method`：reported、estimated、legacy、unknown。
* `priceSource`：builtin、models_dev、manual、unknown。
* `pricingModel`、`pricedAt`、`ruleVersion`。
* `snapshot`：实际输入／输出／缓存单价和倍率；models.dev 的 provider/model 原始键及抓取时间。

内置价表从固定上游提取，并记录来源。models.dev 模型过滤、归一化、常用模型选择及同名覆盖规则沿用上游；在转换丢失 provider/model 键之前附加元数据。手动修改保存 manual 来源。

已知费用随记录冻结；价格更新仅补全明确未知的费用。Token 修订需要重新计算时优先使用原记录快照；重估使用当前价格必须显式执行并产生新修订。历史导入保留已存金额，来源无法证明时使用 unknown。各设备可以采用不同快照，服务器原样保存并汇总，不自动重算。

### 3.4 SQLite

* `meta(key,value)`：schemaVersion、deviceId、serverInstanceId 等。
* `config(key,value)`：应用配置；令牌不返回给远端，也不写入同步记录。
* `records(device_id,id,revision,deleted,payload,source_path)`：用量记录；复合主键。
* `outbox(id,revision,payload)`：本设备待传修订；同一记录仅保留最新待传版本。
* `remote_records(device_id,id,revision,payload)`：其他设备的用量镜像。
* `file_state(path,fingerprint,parser_version)`：变更检测。
* `migration_sources(id,cutover,timezone,manifest)`：旧数据库身份、覆盖范围及摘要。
* `server_changes(seq,device_id,id,payload)`：服务器持久变化序列。

用量变更与 outbox 入队必须同一事务提交；确认上传时仅清理被确认的具体修订，不能误清理上传期间生成的新修订。扫描进度仅在记录提交后推进。首版不自动删除用量明细和待传记录。

## 4. CC Switch 迁移协议

### 4.1 支持范围

第一版支持固定提交对应的 schema v20 SQLite 数据库／一致性备份。使用 SQLite Backup API 从只读连接生成快照，不能仅复制正在 WAL 写入的主文件。其他 schema 明确拒绝并报告支持范围，不在源数据库执行迁移或写入。

普通完整 SQL 备份包含用量；WebDAV/S3 同步快照跳过用量。SQL 文件执行导入不纳入第一版，可先用原应用恢复为独立数据库快照。不能把 CC Switch 整库恢复代码用于替换 Harness Scope 主库。

### 4.2 输入与两阶段流程

输入：绝对数据库路径、稳定 `legacySourceId`、原设备归属、分界自然日 `D`、用户确认的历史 IANA 时区。

1. preview：读取一致性快照，识别 schema、所需列、明细／日汇总数量、Token 和费用；检查已有迁移 manifest；显示覆盖范围与限制。
2. commit：对同一快照摘要确认后，在单一事务导入记录、覆盖规则和 outbox；任何错误回滚，源库不变。

旧数据只计入 D 日之前；新读取器只计入 D 日及之后。分界前日志仍可用于恢复累计基线及祖先关系。coverage 按来源和 harness 生效，不应全局屏蔽没有历史数据的应用。

### 4.3 明细与日汇总

明细 ID 为 `legacy:<sourceId>:detail:<request_id>`；日汇总 ID 由来源、原日期、app、provider、model、request_model、pricing_model 维度生成。相同来源及主键重导是 upsert，不能累加。

读取 input_token_semantics 并归一化；按上游有效记录过滤规则处理代理与会话重复，再与源 UI 对账。保留历史金额及已有状态字段；未知来源不补造。

日汇总无原生事件身份和时区，必须保留用户指定时区及原日期。不能恢复正文、小时明细或重新精确划分日界线。查询部分覆盖一个汇总日时，应排除该汇总并明确提示覆盖不完整，不能按比例猜测。

同一旧来源的持续追加导入不属于第一版；迁移是固定快照切换。若以后增加按区间重建，必须先验证新结果，再原子替换旧区间，并同步撤销旧记录。

## 5. 同步协议 v1

单用户服务；HTTPS 由反向代理提供。服务器启动必须配置 Bearer token，默认监听 localhost；生产可显式指定绑定地址。正文、路径、API 密钥不进入协议。消息设置数量和字节上限。

### 5.1 上传

`POST /v1/push`，`Authorization: Bearer <token>`：

```json
{
  "protocolVersion": 1,
  "deviceId": "persistent-uuid",
  "deviceName": "desktop",
  "records": [{"id":"claude:session:message","revision":2,"deleted":false}]
}
```

示例省略记录其他必填字段。服务器验证每条记录 deviceId 与请求一致，金额／Token 合法，身份和修订有效。一个 batch 原子处理。较高修订替换旧值；相同修订与内容重复返回成功；相同修订内容不同返回冲突；较低修订不覆盖较新记录。

响应包含 `serverInstanceId`、确认的 `(id,revision)` 列表和服务器变化游标。响应丢失时重试同样的记录，不会重复记账。server_changes 只在内容变更时追加，并与记录更新同一事务提交。

### 5.2 下载

`GET /v1/changes?after=<seq>&limit=<n>`：按服务器 seq 升序返回记录变化、nextCursor、hasMore 和 serverInstanceId。客户端在同一事务应用变化并推进游标。其他设备记录写入 remote_records；本机记录继续由本地事实负责，避免本机未上传修订被旧镜像覆盖。

服务器重建后 instanceId 改变：清空远端镜像和游标，并将所有本机记录重新加入 outbox，包括 tombstone。首版服务器不裁剪变化序列；未来裁剪必须增加 full snapshot 协议。

### 5.3 查询与离线

本机视图读取本地记录；全部设备视图为本地记录＋其他设备镜像，按完整身份去重；指定设备视图只取所属设备。展示上次成功同步和待传数量，离线汇总明确为最近缓存。设备当前价表不同不会覆盖其他设备原费用。

后台定时采集及同步由后端调度，离开用量页仍运行。操作超时、服务器暂时不可用不阻塞本机浏览，失败保留 outbox。服务重启和客户端重启均不能丢失进度。

## 6. 会话与删除语义

迁入本机列表、搜索、按项目／时间分组、消息阅读、工具步骤、Markdown 导出和恢复命令。读取器只开放已配置 harness 根目录，支持自定义目录时同时更新用量和会话的根目录配置。

删除会话前先采集其用量并提交本地事务及 outbox，再执行原生删除；无法完成采集则不删除。删除正文不删除已发生用量，远端总量不回退。若源正在写入或当前适配器不支持可靠删除，应明确拒绝。统计修正通过独立的撤销记录实现。

## 7. 开发阶段与验收

| 阶段 | 交付 | 必须通过 |
| --- | --- | --- |
| P0 合约和骨架 | Rust workspace、前端、Tauri、文档和来源声明 | 核心脱离桌面编译；目录和命名空间独立 |
| P1 本地事实 | Claude/Codex 适配器、存储、费用来源 | 重扫幂等；半行补全；累计差分；缓存不重算；未知格式诊断 |
| P2 本机产品 | 应用、设置、用量、会话 | 真实目录配置；主题语言；统计筛选；阅读导出；删除保留已采集用量 |
| P3 历史迁移 | v20 快照预览与提交、分界 | 源库不变；对账；重导幂等；边界与累计基线；错误原子回滚 |
| P4 同步 | 独立服务器、队列、修订及远端镜像 | 两设备汇总；断网重试；响应丢失；旧修订保护；重启和服务重建 |
| P5 发布 | 安装与部署说明、CI、构建产物 | Rust 测试、前端类型检查／构建、浏览器真实 API 冒烟、桌面编译 |

### 7.1 首版测试矩阵

* Claude：相同 message 多块、输出补全、缓存 5m/1h、子代理、同一事件重扫。
* Codex：last/total、重复快照、模型变化、累计重置、缺失父会话、fork/replay。
* 存储：不变记录不增 revision；费用来源冻结；修订与 outbox 原子；旧 ACK 不清新 revision。
* 迁移：明细＋日汇总、归一化、cutover、重复导入、不支持 schema、失败回滚。
* 同步：身份隔离、错误令牌、跨设备伪造、重复／过时／冲突修订、分页游标和重建恢复。
* 产品：四个主页面可用；配置后采集真实文件；请求未知状态不显示成功；旧汇总不伪造明细。

每个新 harness 必须提供合成 fixtures 和可人工核对的预期总量。上游解析结果用于差异参考，不能作为唯一正确性依据。版本升级先回归样本，涉及事件身份变化时必须提供迁移或替换方案。

## 8. 首版交付状态记录

实现完成后在 README 和测试记录中列出已实现能力、实际执行的检查、边界及后续工作。未通过的平台能力不宣称已验证。预计首个可靠版本 20–35 人日，估算不构成自动完成全部后续阶段的承诺。

### 8.1 本次 v0.1 实际交付

P0–P4 的首版范围已实现：两个原生适配器、四个页面、内置／models.dev 费用来源、v20 快照迁移、单用户同步服务和持久化队列。P5 已完成自动化回归及 Windows 调试可执行构建；安装器、签名、其他桌面平台和长期大数据验证仍属后续工作。实际结果见 [VALIDATION.md](VALIDATION.md)。

实现细节：配置、价表、扫描诊断及最小祖先用量上下文暂存于 `meta` 表；无需另建配置表。当前 `model` 保留原始模型名，实际计价模型和单价快照保存在 `cost` 中。会话消息首版整份加载，内联图片按需读取；SQLite 等其他格式通过适配器的 `read_source()` 覆盖，多个会话通过 `sessions()` / `messages_for_session()` 分派。源读取和祖先解释不再写死在通用引擎中。

价格数值和匹配／导入规则参考固定上游；对费用记录增加明确的 null 与来源信息，因此缺价回填只处理 unknown，而不会覆盖已知免费零值。时间范围控件沿用系统本地时间，分组时区独立配置；跨时区控件统一与 Codex 分页续写需要后续专项验证。

## 9. 上游与维护

复制代码保存原版权声明及 MIT 许可证，并在 third_party 记录提交、文件和改动。上游 UI、会话和计价更新分别审查；自行维护的用量读取器不需要合并整个 CC Switch 后端。

参考：

* https://github.com/farion1231/cc-switch/tree/e8880e6f57e25bce1bf3125cc6e33d9fb2b0d806
* https://v2.tauri.app/start/frontend/vite/
* https://www.sqlite.org/backup.html
