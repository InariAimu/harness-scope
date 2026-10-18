# v0.1 验证记录

日期：2026-10-16；平台：Windows x86_64；Node.js 25.8.1，Rust 1.91.1。

## 已执行

| 检查 | 结果 |
| --- | --- |
| `npm test` | 4 项通过：models.dev 来源／过滤、归一化输入、未知与零费用 |
| `npm run build` | TypeScript 检查及 Vite 生产构建通过 |
| `cargo test --workspace --locked` | 21 项通过：19 项核心集成＋2 项 HTTP 集成 |
| `cargo fmt --all --check` | 通过 |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | 通过 |
| `npm run tauri -- build --debug --no-bundle` | 生成内嵌前端的 Windows 调试可执行文件 |

核心用量对账：合成 Claude 样本 1 条用量、220 Tokens；Codex 样本 2 条用量、190 Tokens；合计 3 条、410 Tokens。Codex 缓存与推理分类不重复计入总量。

HTTP 集成使用真实 loopback TCP、两台独立设备数据库及独立服务器。覆盖重复同步、服务重建后重新上传、设备镜像及总量一致。

扩展合约测试注册一个独立 SQLite 样例 harness；覆盖发现、读取、2 个原生会话、指定 sessionId 阅读及用量入队，无需修改存储或同步代码。

## 浏览器验收

使用 `playwright` 技能的 Playwright CLI 驱动本机 Vite 页面和真实 Rust 开发 API。数据库由 `scripts/seed_demo.py` 创建，未读取个人日志，未连接外部同步服务。

* 用量页：指标、趋势、3 条请求明细正确；新增输入分别为 Claude 100、Codex 90 / 30。
* 会话页：2 个会话，项目分组、列表、正文、工具步骤及用量摘要可见；时间单位修正为毫秒。
* 应用页：显示注册表中的两个 harness、真实合成目录和采集诊断。
* 设置页：保存主题设置成功，深浅色切换及设备配置通过实际 API 持久化。
* 最终浏览器会话无应用运行时错误。开发期间的 favicon 和热更新重启日志不计为最终运行时错误。

本机截图保存在 `output/playwright/usage.png` 和 `output/playwright/session-reader.png`，该目录已忽略，不提交样本数据库和二进制。

## 产物与未验证范围

* 可执行文件：`target/debug/harness-scope-desktop.exe`，未签名的调试构建，非安装器。
* 原生 Tauri 窗口交互未做自动化视觉检查；同一前端及核心通过浏览器 API 流程验证。
* Docker 配置已提供，尚未实际构建／启动；Linux 服务端及其他桌面平台由后续 CI／实机检查覆盖。
* models.dev 导入算法和来源信息通过合成数据测试；最终界面验收关闭自动导入，没有以真实远端价格变化作断言。
* Vite 提示主包约 1.94 MB；后续可分拆阅读器、图表和翻译，当前不阻止桌面构建。
* 支持范围和格式限制以 README「当前边界」为准，不能把样本回归通过解释为覆盖全部 harness 版本。

## 2026-10-17 增量验证

平台：Windows x86_64；Node.js 24.11.1，Rust 1.91.1，Python 3.10.6；本机 Codex CLI `0.162.0-alpha.17.2`，隔离安装 OpenCode CLI `2.0.31`。

### 自动化回归

* Rust workspace：26 项通过，包含 20 项核心集成、4 项 OpenCode 集成、2 项真实 HTTP 集成。
* 前端：6 项通过，新增同一 SQLite 文件中多个会话身份和后端删除能力测试；TypeScript / Vite 生产构建通过。
* 对账脚本：4 项标准库 unittest 通过，覆盖十进制费用、缺失用量、未知费用、CC Switch 输入语义及 OpenCode 迁移去重。
* `cargo fmt --all --check`、`cargo clippy --workspace --all-targets --locked -- -D warnings` 通过。
* `npm run tauri -- build --debug --no-bundle` 通过，生成包含本次前端和适配器的 `target/debug/harness-scope-desktop.exe`，仍为未签名调试构建。

OpenCode 回归通过实际 SQLite 文件与未 checkpoint 的 WAL 验证只读采集；覆盖 V1→V2 迁移保持事件 ID、同库去重、纯 V2 无旧表时的会话发现、reasoning 归一化、已知零费用、科学计数金额、损坏与未完成消息保留旧记录、费用修正和 outbox 修订。新 Codex 样本覆盖 `cache_write_input_tokens` 和逐请求记录与 token_count 成对出现时不重复入账。

### 真实请求对账

使用 SQLite Backup API 生成 OpenCode 一致性副本，在副本上运行 v2 standalone，新建并续写同一会话。Codex 使用本机认证和原生日志目录，也新建并续写同一会话。提示词仅要求返回验证标记，不操作项目文件。Harness Scope 使用独立数据库和配置，Codex 仅采集测试日志副本，避免正在进行的聊天影响总量。

| 检查 | Codex ↔ cc-switch | OpenCode ↔ 原生 SQLite |
| --- | --- | --- |
| 已完成请求数 | 2 = 2 | 2 = 2 |
| 新增输入 | 2,900 = 2,900 | 5,447 = 5,447 |
| 输出（包含推理） | 22 = 22 | 24 = 24 |
| 缓存读取 | 27,520 = 27,520 | 5,120 = 5,120 |
| 总 Tokens | 30,442 = 30,442 | 10,591 = 10,591 |
| USD | 0.008772 = 0.008772 | 0.00084681 = 0.00084681 |

Codex 原生输入包含缓存：cc-switch 的原始 input 需按 `input_token_semantics` 归一化后比较。费用使用固定内置价表，与本机 cc-switch 本次会话记录一致。OpenCode 保留原生 reported 费用；本机 cc-switch 没有 OpenCode 会话统计，对账报告明确标记 `ccswitch: unavailable`，原生对账单独标记通过。

OpenCode 完整副本另外核对了 1,062 条助手用量、26 个会话／模型组，所有 Token 分类、请求数和报告费用一致。Harness Scope 合计采集 1,064 条；强制重扫新增和更新均为零，服务重启后继续保持幂等。真实测试数据和详细报告位于 `output/live-validation/`，已被 Git 忽略；仓库仅提交合成样本及不含正文的汇总结果。

### 本次边界

本机 Vite＋Rust API 通过浏览器验证：应用页显示 OpenCode 解析器及删除限制；新会话与续写正文均可阅读，应用名、模型、两轮提问和用量摘要可见；菜单中的删除项禁用并提示在原应用删除。最终重新加载后的浏览器控制台无错误。原生 Tauri 窗口仍未进行自动化交互验收。

OpenCode 真实请求验证覆盖 v2.0.31 及从本机 V1 SQLite 迁移出的副本；旧式 JSON 存储、分叉会话继承正文和附件图片不在本次验收范围。cc-switch 参照仅对成功的原生会话明细进行比较，未把代理与旧日汇总混入同一会话。原版 cc-switch / OpenCode 数据库只读，未升级桌面应用或覆盖全局配置。安装器、签名、其他平台和大型目录的持续采集仍沿用上文未验证边界。

OpenCode CLI 与 V1/V2 迁移说明参考官方 [命令说明](https://dev.opencode.ai/v2/docs/cli/commands/) 和 [迁移说明](https://dev.opencode.ai/v2/docs/migrate-v1/)，实际格式断言以本机数据与合成回归为准。

## 2026-10-17 界面反馈修正

* 移除固定底部状态条及占位；外壳固定视口，用量页仅正文区滚动。设备筛选移到页面标题下方，应用、设置、会话列表和阅读器分别由自身内容区负责滚动。
* 从固定上游 `src/icons/extracted/index.ts` 引入 Claude、OpenAI/Codex 和 OpenCode SVG，并沿用 Claude 终端／桌面角标。应用卡片、筛选、请求表与会话头像统一使用同一图标组件；来源已补入 `third_party/UPSTREAM.md`。
* Windows 启动时关闭系统装饰标题栏，页头提供 cc-switch 同样布局的最小化、最大化／还原、关闭按钮，补齐 Tauri 窗口权限及拖动区域。
* 未采集的耗时、首字、生成窗口由 API 明确返回 null；前端继续显示 `—`，不把缺失值解释为 0 ms。公式、门槛及当前数据限制见 [计时说明](TIMING.md)。

检查结果：27 项 Rust 测试、9 项前端测试、TypeScript／Vite 构建、fmt、clippy 和 Windows 内嵌前端调试构建通过。浏览器使用真实统计数据库的一致性副本，关闭采集，并仅用合成日志验证会话阅读；没有修改桌面原库或源日志。1280×860、900×620 下均仅 `main-content` 存在纵向滚动，外壳不滚动，底部状态条不存在；应用、设置、会话列表／阅读器没有外层滚动，品牌图标和未知计时展示正确，页面无运行时异常。截图和报告位于已忽略的 `output/ui-shell-*.png` / `output/ui-shell-report.json`。

已重启桌面程序继续显示本机真实数据，进程响应正常。只读检查主窗口的客户端顶部与窗口顶部相差 2 px，确认没有系统标题栏的高度占位。自动审批拒绝带远程调试参数启动桌面的操作，仅返回 `blocked by policy`；因此原生按钮的自动点击、拖动和最大化交互未验收，不能以浏览器布局检查代替原生交互验证。

## 2026-10-17 本机速度、首字与窗口尺寸

已实现 Codex / OpenCode v2 的请求耗时与首字估算、模型／应用分组的加权速度和首字中位数。请求及统计使用 `≈`，详情展示计时来源；缺失样本为 `—`。口径与本机存储／同步边界见 [计时说明](TIMING.md)。

真实请求使用本机 Codex CLI 0.162.0-alpha.17.2 和 OpenCode v2.0.31，要求只返回说明文字且不调用工具、不修改项目文件。CLI 正常完成并由桌面扫描导入；未把客户端命令运行总时间作为模型耗时。Codex 初始化出现模型列表刷新超时／本机 MCP 启动警告，但请求实际成功，有 turn.completed 和原生用量记录。

| 本次真实请求 | 输出 Tokens（含推理） | 估算耗时 | 估算首字 | 显示估算速度 |
| --- | --- | --- | --- | --- |
| Codex | 1361 | 29133 ms | 12666 ms | ≈47 tok/s |
| OpenCode v2 | 471 | 3859 ms | 715 ms | ≈122 tok/s |

Codex 耗时及首字与本机 cc-switch 记录逐毫秒一致；请求计数、各 Token 维度和费用对账通过。OpenCode 时间字段与原生 session_message / content.time 对齐；全量 1062 条本机 OpenCode 用量的各 Token 维度和费用对账通过。回填前已有 30769 条记录的 Tokens、模型、计数、费用金额及单价快照均未变化；其中一条旧记录的 pricedAt 元数据刷新，金额和价表不变。校验时共回填 Codex 耗时 27719 条、首字 4452 条，OpenCode 耗时 1062 条、首字 903 条；活动日志继续增长，数量是检查时的快照。

36 项 Rust 测试（含 9 项新增计时测试）、11 项前端测试、4 项 Python 对账测试通过；TypeScript、Vite、fmt、clippy 和 Windows 调试桌面构建通过。新增回归覆盖重复快照、延迟用量、工具耗时、中断、旧布局补写、异常／缺失时间戳、V1/V2 内容计时、事务回滚、重启与回填、价格保留、加权汇总／偶数中位数以及跨设备同 ID 隔离。

浏览器以桌面真实统计库的一致性副本验证请求表、来源详情、模型／应用统计以及筛选后的单请求统计。1500×1000 与本机缩放对应的 1000×667 CSS 视口显示正确，外壳不滚动，无运行时异常；计时详情标记和两种来源文案均可见。原生桌面已重启，响应正常；通过 DPI aware 的只读 Win32 查询确认本机 DPI 为 144（150%），实际客户端 1500×1000 物理像素，外框／阴影 1522×1013。未进行原生窗口按钮自动点击或拖动验收。

详细报告和截图位于忽略目录 output/timing-validation/（report.json、ui-report.json、window-physical.json、对账 JSON 及 ui-*.png）。临时 API / Vite 验证进程在验证后关闭，保留桌面程序运行。

## 2026-10-18 对齐 CC Switch 4.0.14 用量页设置

参照 v4.0.14 / c1caee80d7f047e0d671292a0cc0502cc07ea1e5，将「定价」页签并入「设置」，保留 models.dev 和手动模型定价，新增：

* 显示币种：USD、CNY、EUR、JPY、TWD、TRY，按币种保存手填汇率；预置汇率只作为初始值，不联网更新。总览、请求表、模型／供应商统计、趋势与热力图、请求详情和会话摘要使用同一显示方式；换算后可悬停查看美元原值，定价输入始终为 USD。
* 侧栏「今日」：金额、Token、请求数、缓存命中率，最多两项，可全部隐藏。按配置时区读取本机今日数据，不随用量页面的设备／模型等筛选改变。
* 请求日志列：时间固定可见，其余 13 列可选；新增可显示缓存写入、命中率、总耗时与状态码，支持恢复默认。原生日志未知状态码显示 —，估算耗时保留 ≈，已采集的 Codex 缓存写入计数仍可见。

偏好通过独立的 usage_display / save_usage_display 命令保存在本机 meta.usage_display。保存使用字段补丁、汇率合并和串行请求，连续修改不会拿旧的整份配置覆盖其他设置。它不写入用量、价格快照、outbox 或同步协议。

验证通过：41 项 Rust 测试（新增 5 项显示设置测试）、16 项前端测试（新增 5 项显示格式及列配置测试）、4 项 Python 对账测试，以及 TypeScript、fmt、clippy、Vite 和 Windows 内嵌前端调试构建。测试覆盖旧库默认、跨重启持久化、汇率独立保存、非法值原子拒绝、显示变更不改费用／修订／上传队列、配置时区与本机今日范围、极小非零金额和锁定时间列。

浏览器使用桌面真实统计库的一致性副本，禁用副本采集及服务器同步；实际操作验证 CNY / EUR 切换和独立汇率、非法 0 汇率、连续列勾选、侧栏最多两项／全部隐藏、刷新后保留、恢复默认、保存失败回退、换算金额的美元提示，以及模型／供应商统计。操作前后的全量原始 USD 汇总一致，页面无运行时异常。1500×1000 与本机 150% 缩放对应的 1000×667 CSS 视口外壳不滚动，设置页只有 main-content 纵向滚动。空用量响应下仍可从空状态进入显示与模型定价设置。详细报告、截图及副本在忽略目录 output/usage-settings-validation/。

已重启桌面程序供本机查看；保留 1500×1000 物理客户端默认尺寸。原生按钮点击、拖动和原生控件自动化仍未验收，浏览器检查不替代这些操作。临时验证 API / Vite 在完成后关闭，桌面进程保留运行。
