# Copilot 兼容导入与后续设计

[文档索引](README.md) · [使用指南](USAGE.md)

本设计约定两个独立的来源 ID：`copliot-deprecated` 和 `copliot`，为兼容已有数据保留该拼写；当前历史来源界面名称为 `copliot-deprecated`，使用灰色 Copilot 图标，现代来源显示「Github Copliot」，使用黑色 Copilot 图标。旧会话不自动归入新来源，同一会话的来源在首次导入时固定。缺失正文不会使已有请求元数据失效。

当前已实现 `copliot-deprecated` 的兼容导入：复用现有数据库字段和同步格式，未知 Token 填 `0`，未知模型填 `unknown`，未知 provider 填 `github-copliot`，其他已有可空字段继续使用其未知表示。接受历史 Token 不完整、未知与真实零无法区分的限制。现代 `copliot` 也已接入 VS Code chatSessions 的兼容统计；原生 Credits、计量覆盖率、Agent Host 和 CLI 的完整语义设计保留为后续阶段。

两个适配器沿用现有数据库和同步格式。旧版读取选定历史会话，现代来源读取历史清单之外符合 Copilot 身份的会话；均支持 JSON 快照、JSONL 对象操作日志、元数据及可恢复的文本阅读。

## 已实现：旧版兼容导入

兼容记录接入现有请求数、会话、模型和按日统计，不增加数据库字段。每个可恢复的用户请求生成一条现有 `UsageRecord`，`harness=copliot-deprecated`、`grain=event`、`count=1`、`recordSource=session`。Token 全为零也必须保留该记录。

| 现有字段 | 旧版兼容映射 |
| --- | --- |
| `id` | `copliot-session:{sessionId}:{requestId}`，两个来源共用，不包含路径、模型或来源分类 |
| `sessionId` / `nativeEventId` | 原生会话 ID / 请求 ID；transcript 只关联，不另计请求 |
| `occurredAt` | 原生请求时间由毫秒转换为秒；不能用扫描时间补齐 |
| `model` | 优先实际解析模型，其次明确的选择模型；缺失或只有未解析 `auto` 时为 `unknown` |
| `tokens.output` | 首阶段只取原生顶层累计 `completionTokens`，没有该字段时填 `0`；不回退到 `result.usage` 或 metadata 中的最后一次调用输出 |
| `tokens.input` / 缓存 / 推理 | 只有与当前字段含义一致且范围已确认时才保留；其余填 `0`。旧 `promptTokens` 通常为最后一次调用或上下文，且不明确是否含缓存，首阶段不直接搬入表示未缓存输入的 `tokens.input` |
| `cost` | 可确认 Token 非零且价表匹配时，以已知维度估算并标为 `method=estimated`、`billingBasis=model_reference`；否则 `amountUsd=null`、`method=unknown`。不补算缺失 Token，不把请求次数、Credits 或 PRU 当作 USD |
| `provider` | 优先原生请求或响应 metadata 中的有效 `providerId`；未知时为 `github-copliot`，不由模型厂商猜测 |
| `statusCode` / `timing` / `requestMetadata` | 缺失或不兼容时沿用现有空值，不添加新枚举或时间来源 |
| `deviceId` / `revision` / `deleted` | 使用现有设备归属、修订和同步规则；重复扫描不生成新身份 |

`0/unknown` 是兼容默认值，不覆盖可验证的已有值。HTTP 状态、延迟和金额已有可空类型，应使用现有空值表达未知；填 `statusCode=0` 或「已报告 0 USD」会使现有统计误认为数据已知。身份和时间是去重与按日统计的必要条件，无法恢复时应暂缓并诊断，不填同一个 `unknown` ID 或 1970 年时间。非法类型、负数和溢出同样应诊断，不能作为缺失值吞掉。

本机会话列表复用现有 `Session` 投影：会话 ID、原生标题、创建／最后活动时间，以及可验证的项目目录；缺少的可空信息沿用未知表示。来源路径供本机阅读器定位，标题、目录和正文不加入 `UsageRecord` 或同步负载。原生日志没有恢复命令时不编造命令。第一阶段只读取历史会话清单中的来源，新建的现代会话留给 `copliot`，避免两个来源重复扫描入账。

只统计保存下来的用户请求；显式 `isSystemInitiated=true` 的条目不计数，工具轮数不增加请求数。取消、失败和重试保留原生请求身份，不能从用户请求次数反推 Premium Request 实际账单。空会话不产生虚构请求。

适配器通过 `allows_price_estimates(has_tokens)` 允许对可确认的非零 Token 估价，扫描、重新计价及待补价格模型列表都遵循同一限制。只按已知维度计算，缺失维度不推算；费用是模型 API 参考价，不代表 Copilot 的 Premium Request／Credits 账单。价表快照、来源和 `model_reference` 口径复用现有字段。未知 Token 全为 0 的请求即使能匹配模型价表仍不定价，避免「已估价 0 USD」覆盖未知；它们也不导致已定价模型反复补价。当前摘要会将未知费用合计显示为 `0`，并增加 `unpricedCount`；该零值不代表免费。

模型统计的 Standard／Fast／档位未知仅适用于 `codex` harness。其他来源即使使用 GPT、Codex 命名的模型或 OpenAI provider，也不进入档位分组。同一模型跨来源统计时，Codex 档位行与其他来源的无档位行分别保留，总请求数和 Token 总量不变。

首阶段不保存选择模型与实际模型两套字段、Token 覆盖率、原生计费用量或最后一次调用的独立用量，也不写入无法兼容当前白名单的计时信息。这些信息仍留在原生文件中，后续完整适配可以按同一身份修订。旧会话被新版续写时仍属于旧类，新增 Credits 暂不写入 USD，也不因此切换来源。

2026-10-22 的本机旧来源盘点中，398 个可完整重放的文件包含 480 条请求条目；排除 9 条显式系统发起条目后，176 个非空会话中有 471 条可计数的用户请求。这些请求均有可恢复的身份和时间；15 条模型需填 `unknown`。只读试映射保留 161 条请求的累计输出，共 1,205,854 Token，其余 310 条输出填 0；77 条请求虽有输入字段，但口径不兼容，首阶段输入均填 0。另一个损坏来源暂缓处理，刚新建的现代请求不包含在这批旧来源中。这里的数量只代表本机现存日志，不代表完整账单。试映射未写入应用数据库或执行同步。

兼容样本见 [compatible-records.json](../tests/fixtures/copliot-deprecated/compatible-records.json)。[Rust 兼容测试](../crates/core/tests/copilot_deprecated_contract.rs)直接调用现有 `Store`、查询和 v4 同步，验证零 Token 仍计数、重复导入与重启后幂等、同步往返、数据库结构和 schema 版本不变。[适配器测试](../crates/core/tests/copliot_deprecated.rs)另验证原生操作重放、来源边界、异常文件不撤销记录和价格更新保护。

### 选择历史范围和执行导入

首次导入需明确选择原生 session ID，保存为 JSON 字符串数组。导入工具在日志根目录写入 `.harness-scope-copliot-deprecated.json` 作为本机发现清单；原生会话文件不修改。清单固定归属，普通扫描不将新会话追加进来，也不因旧会话追加 Credits 改归。根目录可以选 VS Code 的 `Code/User`，发现仅匹配清单中的 `chatSessions/*.{json,jsonl}`，同目录同 ID 优先 JSONL；关联 transcript 不重复入账。

关闭桌面应用后，使用绝对数据库路径、VS Code User 根目录、ID 数组文件和一个尚不存在的备份路径：

```powershell
cargo run -p harness-scope-core --example import_copliot_deprecated --locked -- <DB> <CODE_USER_ROOT> <SESSION_IDS_JSON> <BACKUP_DB>
```

工具先制作 SQLite 一致性备份，再注册启用的本机来源，只扫描此 harness，并重复扫描验证幂等；输出导入数、未知费用和异常文件诊断。之后启动新版桌面应用，「应用」页可查看目录和开关，普通采集沿用此清单。使用已有来源配置和元数据存储，不新增数据库字段或版本。同步沿用当前已配置的后台行为。清单不携带正文、账号信息或密钥，不进入同步。

## 现代 VS Code 兼容接入

`copliot` 的界面名称为「Github Copliot」。它读取配置的 VS Code User 根目录下、不在 `.harness-scope-copliot-deprecated.json` 历史清单中的原生 chatSessions JSON／JSONL。历史清单固定旧会话归属，其余符合身份条件的新会话可持续采集，普通重扫无需逐次选择。缺少或损坏的历史清单会使发现失败，不会把整个历史目录当成现代来源。手动注册新的 User 根目录前，需先完成其历史清单；空清单表示显式选择该根目录下所有 Copilot 会话为现代来源。

请求必须具有 Copilot 扩展／agent 身份或 `copilot/` 模型标识；显式其他扩展优先排除。会话文件名与原生 ID 不一致时暂缓，避免绕过历史边界。两类使用相同的 `copliot-session:{sessionId}:{requestId}` 身份；历史会话被新版续写后仍归旧类。新建会话和已有会话的新请求随普通扫描持续接入，关联 transcripts 不再计量。

当前兼容字段与旧版相同：每个保存的用户请求为一条 `count=1` 记录，实际模型优先，provider 未知时为 `github-copliot`。取顶层累计 `completionTokens`；有有效 `modelTotals` 时改用其输出总和，不再叠加顶层输出。同一请求涉及多个模型时，兼容记录使用 `model=unknown`，保留总输出和一次请求，避免把多模型费用归到最后一个模型。完整的按模型拆分留待扩展计量粒度。损坏 totals 暂缓该条请求，保留之前的记录等待修复。

`promptTokens`、`promptTokenDetails` 表示最后一次调用或上下文，不代表当前数据库的未缓存整轮输入。此阶段输入、缓存和推理填 0，未知费用保持空值。`copilotCredits`／`sessionCopilotCredits` 留在原生文件中，不写入 USD 或伪装为价表费用。可靠的非零输出可按已有价表估算 `model_reference` 参考费用，仍不代表 Copilot 实际账单。模型速度档位继续仅适用于 Codex。

关闭桌面应用后，可运行下面的导入工具。它校验历史边界、创建 SQLite 一致性备份、保留其他配置并注册启用 `copliot`，重复扫描检查幂等；不修改原生会话或历史清单。

```powershell
cargo run -p harness-scope-core --example import_copliot --locked -- <DB> <CODE_USER_ROOT> <BACKUP_DB>
```

[现代适配器回归](../crates/core/tests/copliot.rs)验证来源互斥、未来会话发现、其他 agent 排除、计数、价表、损坏文件保留、重启幂等与 v4 同步。该阶段不改变数据库结构或同步字段；Agent Host 数据库、CLI 和 OTel 仍按下文完整语义设计推进，尚未注册为可采集来源。

可按工具限定重新计价范围，带一致性备份的辅助工具见 [reprice_harness.rs](../crates/core/examples/reprice_harness.rs)。它生成模型参考估算，不恢复缺失 Token 或实际订阅账单。

## 两类来源的边界

| 来源 ID | 主要用途 | 默认展示 | 缺失数据的处理 |
| --- | --- | --- | --- |
| `copliot-deprecated` | 旧版 VS Code/Copilot 会话及其关联 transcript | 观察到的用户请求次数、会话和模型活动 | 首阶段只保存兼容 Token，缺失填 0，模型未知填 `unknown`，费用沿用未知表示 |
| `copliot` | VS Code chatSessions v3；Agent Host 和 CLI 仅列入后续设计 | 当前为用户请求、会话和累计输出 Token；完整方案包括 Credits 与覆盖率 | 当前未知 Token 填 0；完整方案按实际字段保留计量范围和缺失项 |

来源分类、计费制度、统计粒度、正文完整性是四个独立属性。`copliot-deprecated` 中也可能存在 Token 或后续新增的 Credits；`copliot` 中也可能存在没有 Token 的请求。不能以「新版」直接推定 AI Credits 计费，或以「旧版」直接推定每次消耗一个 Premium Request 单位。

## 后续设计：扩展来源发现与归属

以下跨存储发现与预览是待实现设计；当前仅使用前文的历史清单和 VS Code chatSessions。完整导入器拟在同一次发现中收集下列候选，先建立原生身份索引，再分配两个来源；不能让两个适配器分别全量读取同一目录并各自入账。

| 存储 | 读取方式 | 归属依据 |
| --- | --- | --- |
| `Code/User/workspaceStorage/*/chatSessions/*.{json,jsonl}`，以及配置的用户数据根目录 | JSON 快照或对象操作日志重放 | 原生 `sessionId`、请求元数据、来源清单 |
| 同工作区 `GitHub.copilot-chat/transcripts/*.jsonl` | 按事件读取正文，关联已有会话 | 关联原生 `sessionId`，不能再计一个会话或一组请求 |
| `Code/agentSessionData/<id>/session.db` | 只读 SQLite 事务，包含 WAL | 注册表／原生元数据证明 provider 后，关联 `turns` 和 `turn_usage` |
| Copilot home 下 `session-state/<id>/events.jsonl` 及原生元数据旁文件 | 按 producer、事件版本读取 | SDK 原生会话与事件 ID；home 可由用户配置 |
| 已存在的 Copilot OTel 文件 | 仅选取适用的模型调用 span | 原生会话关联、trace ID 与 span ID |

新旧 chatSessions 都可能是 `version=3` 和 `.jsonl`。现代请求可以有 `copilotCredits`、`modelTotals`；Agent Host 可以有 `_meta.turnTokenTotals`。这些是能力证据，不是自动迁移命令。结构化 transcript 的 `session.start` 也不足以证明用量完整。

首次预览建立来源清单，保存 `(来源实例, 原生 sessionId) → harness ID`、原生格式、采样摘要及判定依据：

1. 已在历史导入清单中的会话继续使用 `copliot-deprecated`。
2. 初次导入时，旧来源盘点可以批量归入 `copliot-deprecated`；有现代用量证据的新建会话建议为 `copliot`。没有证据或混合来源的会话显示「待归类」，不按时间或软件版本猜测。
3. 用户在预览中确定两类归属后，与导入事务一起保存清单；普通重扫不需要再次确认，也不能覆盖既有归属。
4. 空会话先留在发现结果中，不产生请求或用量事实。未完成请求可更新会话列表，尚不作为完整计量样本。
5. 已归入旧类的会话被新版续写时，新请求仍属于该旧会话。首阶段不导入 Credits；后续完整方案才保留其原生单位。若未来提供显式改归，必须按原身份原子替换并处理镜像／墓碑，不能新增一套事实。

来源清单是本机导入控制信息；同步记录直接携带确定的 harness ID，接收端不得重新猜测归属。两个来源共用格式读取器和身份协调器，最后按来源清单路由到各自的适配器结果。

## 后续完整语义的数据契约

下面是后续完整适配拟增加的语义，不是当前两类兼容导入的入库字段，也不是兼容导入的前置条件。可执行样本契约位于 [copliot-import-contract.json](../tests/fixtures/copliot-import-contract.json)：`legacyCompatibilityProfile` 描述第一阶段；`cases` 保留完整语义下区分未知与零等样本。

| 信息 | 约定 |
| --- | --- |
| 身份 | 来源实例、原生 session ID、原生 turn/request ID、原生 event ID；不存在可靠 ID 时才采用稳定序号并报告 |
| 模型 | 选择模型和实际解析模型分别保存；`auto` 没有实际模型时保持未知，不套某个模型的单价 |
| 请求计数 | `observedUserRequests`、`apiCalls` 分开；系统发起请求、工具步骤、本地提示不增加用户请求数 |
| 计费用量 | `premiumRequestUnits`、`copilotCredits`、`nanoAiUnits` 分别保存为十进制数值和原生来源，允许明确零；不能填入 `amountUsd` |
| 计费制度 | `premium_request`、`ai_credit`、`byok` 或 `unknown`；依据原生计费证据或明确的账户历史，不由 harness ID 推断 |
| Token | 各维度可未知，保存原生输入口径、缓存／推理子集及来源；未知与已知零分开 |
| 范围 | `last_api_call`、`api_call`、`user_turn`、`session_model`；没有依据时为 `unknown` |
| 覆盖 | 正文覆盖 `available/partial/missing/unknown`；用量覆盖按字段及范围保存，二者互不代替 |
| 时间 | 原生日志时间和时间范围；缺少原生时间时不使用扫描时间替代 |

所有计数都必须是非负整数，十进制计费用量必须有限且非负；非法类型、负数、溢出、缓存大于其父输入等异常返回诊断并暂缓相关事实。不能静默截断或钳成零。缺少字段不等于数据损坏：可以合法导入「有请求次数、Token 未知」的记录。

原始 Credits 与 nano-AIU 可同时存在。没有明确单位和版本依据时不相互换算，不能累加两个描述同一消耗的值。Premium Request 倍率的估算需要当时适用的模型和规则快照，并单独标为估算；本期账户余额和当前模型价表不能反推历史实际账单。

## 后续完整语义的用量读取与优先级

### 历史会话

从已保存请求的稳定 ID 与原生时间恢复观察到的用户请求次数。没有正文但有上述身份的请求可以计入；显式 `isSystemInitiated` 单独处理，取消／失败／重试保留状态，不声称观察到的请求都已计费。旧工具调用轮数不等于用户请求数。

`result.usage`、`result.metadata.promptTokens/outputTokens` 和顶层 `completionTokens` 按来源及范围保存。缺少整轮证据时不拼成完整的整轮 Tokens，特别不能将「最后一次调用的输入」与「整轮累计输出」当成同一次完整 API 请求报价。没有任何 Token 的请求仍可进入请求次数与会话活动统计。

### 现代 chatSessions

`copilotCredits` 保存为该响应报告的原生计费用量。`sessionCopilotCredits` 是会话累计口径，不能逐响应相加。存在有效 `modelTotals` 时，按原模型保存整轮累计 Tokens；否则保留顶层输入／输出的原生范围和缺失信息。已知零 Credits 不能触发「字段缺失」回退。

同一请求在 JSONL 操作中多次更新，只更新同一事实的修订。重放必须支持初始对象、设置、数组追加／截断和删除；路径错误、未知操作或尾部半行均标记来源不完整，保留已有事实等待重试，不执行整文件撤销。

### Agent Host

优先读取 `turn_usage.usage._meta.turnTokenTotals`，或有明确范围的 `directTurnTokenTotals`。顶层 `usage.inputTokens/outputTokens` 通常是最后一次调用或当前上下文，不能再与整轮总量相加。父轮的整轮总量包含子代理时，不再累加子代理的同一部分；若按子代理展示，使用 direct 口径并证明其树的覆盖与对账关系。

`_meta.copilotUsage.totalNanoAiu` 是该轮计费用量；`sessionTotalNanoAiu` 作为会话累计观察另存，不能逐轮相加。[VS Code 1.139.1 类型及范围](https://github.com/microsoft/vscode/blob/1.139.1/src/vs/platform/agentHost/common/state/sessionState.ts)和[持久化接口](https://github.com/microsoft/vscode/blob/1.139.1/src/vs/platform/agentHost/common/sessionDataService.ts)给出了这些边界。通用 `session.db` 也可能属于其他 harness，仅有表结构不能证明它是 Copilot 会话。

### CLI 与 OTel

CLI 的 `assistant.usage` 是临时事件，不能假设历史文件中一定存在。历史文件可以提供 `session.shutdown.modelMetrics` 或某些版本的 usage checkpoint；不同版本的 checkpoint 字段并不相同，应按实际字段检测能力。[SDK 用量说明](https://docs.github.com/en/copilot/how-tos/copilot-sdk/features/usage-and-billing)区分了逐调用事件和会话累计指标。

汇总快照按已验证的会话／计量窗口更新，不能把每个 checkpoint、shutdown 及逐调用数据同时相加。恢复、分叉或计数下降时先辨明窗口与继承边界；无法确定就暂缓，不将差额当作负消耗，也不重新加入父历史。只有会话／模型汇总时保留该粒度，不伪造请求时间、按天均摊或用结束时间代替历史活动分布。

已有 OTel 模型调用 span 可以补充逐调用数据，使用 trace/span ID 去重，并与会话总量对账。不能把 `invoke_agent` 父 span 与子模型调用同时累加。启用将来的采集需要单独配置；本设计不更改 VS Code 或 CLI 设置。

## 后续完整语义与当前统计的衔接

当前 `Tokens` 的所有维度是 `u64`；`ParsedUsage.reported_cost` 会进入 USD；扫描器把每个解析结果保存为 `count=1` 的 event。这可以承载第一阶段的兼容记录；若要完整支持未知 Token、不同计量范围及 Credits 等语义，仅添加两个适配器还不够。

后续实现需要一并完成以下工作：

1. 在模型和查询投影中表达未知 Token、观察范围和原生计费用量；旧记录默认保留原有完整语义，不将历史全部改为未知。原生输入是否包含缓存需要按具体格式验证后再转换为当前互斥维度，推理始终作为输出子集。
2. 用户请求身份与各模型用量行分开。一个用户轮可以使用多个模型，拆成模型行时不能让全局请求数翻倍；全局用户请求去重后为 1，模型相关请求数可以分别为 1，但不能把各模型数直接相加作为全局用户请求数。
3. 汇总同时返回已知值和覆盖情况。全局用量缺失时显示未知／已知下限；热力图请求指标使用用户请求次数，Token 指标只使用口径兼容的已知部分。会话级汇总不出现在伪造的逐请求列表中。
4. 原生 Credits／PRU 与 USD 分别显示、筛选和汇总。缺失 Token 或非现金计费用量禁止自动调用 API 价表补成实际费用；完整 Token 的 API 参考价如需展示，须是独立估算。
5. 费用、覆盖、计量范围及新增时间来源均须进入同步白名单。新客户端先检查服务器能力，再上传含新语义的记录；旧服务器下保留待传修订，不能剥离字段或提前确认。具体 schema／协议版本留待实现时确定。

详情页同时显示「Copilot 历史／GitHub Copilot」、实际模型、观察范围、缺失项和计费用量单位。正文阅读与用量采集共用来源配置，各自处理不完整性；没有正文仍可显示元数据与用量。

## 后续设计：跨来源身份与重复导入

同一来源实例中，以原生 session/request/event ID 关联 chatSessions、transcript、SDK events 和 SQLite。`turns.event_id` 可以作为 turn ID 的别名映射，不能各自新增一个请求。文件移动或由 JSON 改为 JSONL 不改变身份；路径、Tokens、Credits、模型标签和来源分类不参与稳定事件身份计算。

单独保存事实的稳定身份及 `harness` 归属。多次导入或解析器升级沿原身份修改修订，删除源正文不清除历史统计。不同来源实例不凭相同文件名直接合并；发现完整复制的原生会话时先提示重复来源，不能默认当成另一组新用量。

本机与其他设备仍使用现有设备身份及同步修订机制。尚无跨设备原生副本的全局去重协议时，预览应提示相同会话副本的风险，不绕过现有设备边界擅自合并其他设备记录。

## 后续完整语义的导入预览与验收

预览分别展示两个来源的会话／用户请求数、已知 Token 的范围与覆盖、原生 Credits／PRU、缺少正文及不完整来源数。扫描后先对账，再允许写入；沿用既有授权的普通增量扫描无需重复确认。任何应用阶段都应支持事务回滚并保留原生文件。

| 样本 | 必须验证的行为 |
| --- | --- |
| 旧会话缺少正文和 Tokens | 仍保留用户请求；Token 和费用为未知 |
| 旧会话被现代客户端续写 | 继续归入 `copliot-deprecated`，新 Credits 不丢失，不产生第二套身份 |
| 新版 JSONL v3 | 操作重放后只保留每个请求最新状态，归入 `copliot`；与旧 v3 不混淆 |
| 缺失与零值 | 已知零 Credits、已知零输出与缺失字段有不同结果 |
| 多轮调用 | 最后一次输入与累计输出不冒充整轮完整用量 |
| Agent Host 父轮与子代理 | 优先整轮模型总量，不重复加入顶层上下文或子代理 |
| CLI 计量快照 | 明确区分用户请求、API 调用、PRU、Credits；快照不逐条累加 |
| transcript 副本 | 补充正文，不增加会话或请求次数 |
| 半行／非法操作路径 | 诊断并暂缓，保留旧事实，禁止完整撤销 |

合成样本和期望结果见 `tests/fixtures/copliot-import-contract.json`；校验命令为 `python -m unittest discover -s scripts -p "test_copilot_import_design.py"`。Python 检查完整语义的原生样本及契约；已实现的兼容层由 Rust 适配器及兼容测试验证原生读取、现有记录格式、存储和同步。两个 VS Code 适配器均已注册并有 Rust 回归；Agent Host、CLI、OTel 和完整原生计量仍未实现，设计样本不能代替生产支持。
