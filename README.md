# Harness Scope

模块化的 AI 编程工具用量统计与本机会话浏览器。React + Tauri 桌面端、Rust 核心、SQLite 存储，以及可自部署的多设备用量同步服务。

详细设计见 [实现计划](docs/IMPLEMENTATION_PLAN.md)，扩展方法见 [新增 harness](docs/ADDING_A_HARNESS.md)。
「速度」「首字」的数据来源、计算门槛与当前未知值行为见 [计时说明](docs/TIMING.md)。

## v0.1 能力

* Claude Code / Codex JSONL、Codex `.jsonl.zst`；累计 Token 差分、重复扫描、Claude 分块补全和显式父会话去重。
* OpenCode V1/V2 SQLite：只读一致性读取 `message` / `part` 和 `session_message` / `session_v2`，迁移后的同会话不重复计费，保留原生报告费用、已知零费用及推理 Token。
* DeepSeek Harness（dsh）：读取 `$DSH_HOME/sessions`（默认 `~/.dsh/sessions`）的 v0/v4 JSONL 与多帧 Zstandard 日志，支持缓存／推理 Token、模型与会话阅读、日志计时估算及迁移副本去重。配置和边界见 [DSH 兼容说明](docs/DSH.md)。
* 复用 CC Switch 的统计组件、视觉 token 和会话阅读器；提供应用、用量、会话、供应商、设置五个页面。
* 供应商账号页：DeepSeek 官方余额、OpenCode Go 五小时／每周／每月额度，手动订阅信息和关联路由的本机用量；支持多个账号，官方数据与日志估算分别显示。
* 对齐 CC Switch 4.0.15 用量页显示设置：六种显示币种、手填汇率、侧栏今日指标、请求日志列显隐、柱形图／曲线面积图切换及原模型定价；显示偏好在本机持久化，原始费用仍保存 USD。
* 本机会话搜索、项目／时间分组、正文和工具步骤、内联图片、Markdown 导出、复制恢复命令。
* 删除已稳定至少 60 秒的会话前先采集用量；正文删除后保留历史统计。无法完成采集则取消删除。
* 固定上游内置价表、模型匹配、models.dev 手动／自动导入，以及手动定价。费用保存来源和实际单价快照；未知与已知零费用分开。
* CC Switch schema v20 数据库的一致性快照预览、幂等迁移和分界日；保留历史费用、旧汇总日期及指定时区。
* 持久化 outbox、修订上传、分页下载、其他设备缓存、服务器重建补传；完整正文、项目路径和工具参数不进入同步 DTO。

## 桌面开发

需要 Node.js 22.12+、Rust 及 [Tauri Windows 开发依赖](https://v2.tauri.app/start/prerequisites/#windows)（MSVC / WebView2）。

```powershell
npm ci
npm run desktop
```

Windows 数据库默认位于 `%APPDATA%/dev.inariaimu.harness-scope/harness-scope.db`，与 CC Switch 独立。设备身份保存在数据库内，换机／恢复时应保留它，避免重新上传为另一台设备。

本地 schema v2 保留原始用量 JSON，使用事务内自动维护的查询投影和应用／供应商／时间索引，分页只读取当前页的价格快照与计时。首次升级会建立索引，此后随记录增量维护；不改写历史费用或同步修订。查询使用独立只读 WAL 连接，后台扫描不会阻塞已有统计读取。界面按实际用量版本刷新，短时间重复切换复用缓存，悬停或聚焦 harness 时预取图表和请求页。采集由后台统一调度，修改扫描间隔会保存并唤醒采集线程。

生成内嵌前端的本机调试可执行文件：

```powershell
npm run tauri -- build --debug --no-bundle
```

输出为 `target/debug/harness-scope-desktop.exe`。发布构建用 `npm run desktop:build`；安装器与签名尚未配置。

## 供应商账号

在「供应商 → 添加账号」选择 DeepSeek 或 OpenCode Go，填写名称与 API Key。DeepSeek 按官方返回的原币显示总余额、充值余额和赠金；OpenCode Go 显示三个窗口的剩余比例及重置时间。页面打开时查询，之后每分钟刷新，也可手动刷新；失败时保留上次成功的数据并标记错误。到期的额度缓存显示等待刷新，不推算为满额。

套餐名称、每月费用、币种、本期开始和下次续费由用户手动记录，也可以不配置 Key，仅管理订阅和查看本机用量。「关联本机用量」按日志中的 harness／供应商原始 ID 选择路由；一条路由只能关联一个账号，历史日志不能区分同一路由的不同 Key。今天、本月和手动账期都使用设置中的统计时区；账期在续费日零点截止。折算金额复用现有价表和汇率，仅代表日志估算，不从官方余额扣减，也不据此推算套餐剩余额度。

账号、订阅和查询缓存仅保存在本机数据库，不参与用量同步。API Key 单独保存在数据库旁的 `.provider-keys` 目录（例如 `harness-scope.provider-keys`），不会出现在账号查询返回值或用量记录内。Windows 使用当前用户的 DPAPI 加密；Unix 使用 0700 目录和 0600 文件权限，尚未接入系统钥匙串。仅备份数据库不包含 Key，换 Windows 用户或设备后需要重新填写。编辑时 Key 留空保留原值，可显式清除；删除账号及 Key 会保留历史用量日志。

## 浏览器开发与合成演示

开发 API 只能监听 loopback；它提供本机文件访问能力，不应暴露到公网。同步服务器不提供该命令 API。

```powershell
# 首次创建独立演示数据库；脚本拒绝覆盖已有数据库
python scripts/seed_demo.py
cargo run -p harness-scope-service -- --local --db output/playwright/demo.db --bind 127.0.0.1:8788
```

另开终端：

```powershell
npm run dev
```

打开 [本机界面](http://127.0.0.1:1420)。演示只读取仓库内合成样本，显示 2 个会话、3 条用量记录、410 Tokens。关闭 models.dev 自动导入，未连接同步服务器。

OpenCode 默认读取 `~/.local/share/opencode/opencode.db`，支持 `XDG_DATA_HOME` 和 `OPENCODE_DB`。在「应用」页可填写包含 `opencode.db` 的目录，或直接填写数据库绝对路径。V1 升级 V2 后优先读取新表，旧表中的相同会话只计一次。会话删除由原应用执行，界面按后端能力关闭删除入口。

浏览自己的数据时，使用独立数据库启动本机服务，并在「应用」页设置日志目录。桌面端无需另外运行本机 HTTP 服务。

## 自部署同步服务器

```powershell
# 设置一个随机长令牌，至少 24 字符；不要提交到仓库
$env:HARNESS_SCOPE_TOKEN = '<your-random-token>'
cargo run --release -p harness-scope-service -- --db data/server.db --bind 127.0.0.1:8787
```

通过 HTTPS 反向代理暴露该服务，再在桌面「设置 → 多设备同步」填写地址和相同令牌。仅允许 loopback 地址使用明文 HTTP。

也可使用 Docker Compose：设置 `HARNESS_SCOPE_TOKEN` 后运行 `docker compose up --build -d`。Compose 默认仅向宿主机 loopback 发布 8787，数据存于命名卷。首次交付未实际运行容器；Windows 服务端和 HTTP 协议有自动化测试。

接口：`GET /health`、`POST /v1/push`、`GET /v1/changes?after=0&limit=500`。除健康检查外均需 Bearer token；当前客户端使用协议 v2 保存供应商信息，服务端兼容接收 v1。升级客户端时请同步升级服务器和其他客户端；遇到 v1 服务器会提示升级并保留待同步记录。协议和事务语义见详细计划第 5 节。

## CC Switch 历史迁移

在「设置 → 数据迁移」选择 v20 数据库／SQLite 备份，填写稳定旧来源 ID、分界自然日和旧系统时区。先预览对账，再提交同一快照摘要。源库只读；导入与覆盖规则原子提交。

* 分界日前使用旧统计；分界日及之后使用原生日志。早期日志仍用于累计基线和祖先关系。
* 重导相同快照不累加；同一来源换快照或同设备重叠 harness 来源会明确拒绝。
* 日汇总不能还原请求明细或精确重新分配到其他时区；部分覆盖一个旧汇总日的查询排除该日汇总。
* WebDAV/S3 同步快照没有用量数据。首版不直接执行外部 SQL 备份文件，也不恢复整个 CC Switch 数据库到本应用。

## 验证

```powershell
npm test
python -m unittest discover -s scripts -p "test_*.py"
npm run build
cargo test --workspace --locked
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
```

测试覆盖解析、缓存语义、费用快照、重启幂等、半行补全、迁移、删除保留统计、上传重试／乱序／冲突、认证、分页镜像及服务器重建。端到端同步测试使用真实 loopback HTTP 和两个独立设备数据库。

真实 CLI 对账可使用 `scripts/compare_usage.py`。脚本只读两边数据库，以会话和模型汇总，归一化输入缓存语义并用 Decimal 比较费用；差异、未知费用或没有可用参照时返回非零。OpenCode 可同时检查原生数据库，并单独标注 cc-switch 中没有记录的情况：

```powershell
python scripts/compare_usage.py --scope-db <HarnessScope数据库> --ccswitch-db <CCSwitch数据库> --harness codex --session-id <Codex会话ID> --report output/codex-comparison.json
python scripts/compare_usage.py --scope-db <HarnessScope数据库> --ccswitch-db <CCSwitch数据库> --opencode-db <OpenCode数据库> --harness opencode --session-id <OpenCode会话ID> --report output/opencode-comparison.json
```

2026-10-17 已使用本机 Codex 和隔离安装的 OpenCode v2.0.31 发出新建／续写请求，具体对账结果见 [验证记录](docs/VALIDATION.md)。真实日志、数据库副本与详细报告均保存在已忽略的 `output/`，不提交到仓库。

## 当前边界

* 首版验证范围为所附 Claude/Codex 样本及 Windows；macOS/Linux 桌面尚未实机验证。
* OpenCode 覆盖 V1/V2 SQLite 样本和本机 v2.0.31 实际请求；早期 JSON 文件存储、分叉会话继承正文和 SQLite 附件图片未验证。未完成或损坏的助手记录会诊断并重试，暂缓该来源的完整撤销处理。
* Codex 分页续写及未覆盖的日志版本需要补充适配；未知基线或异常字段会诊断并暂缓入账。解析器升级必须带样本回归和身份迁移策略。
* 文件变更检测使用完整内容指纹；变更文件重解析。大规模会话目录的扫描和阅读性能仍需专项优化。
* 会话阅读正文目前整份加载，内联图片按需加载；暂不支持外部图片文件引用和工具附件图片。
* Windows 恢复会话提供命令复制；一键外部终端恢复、托盘、自动更新及安装器列入后续阶段。
* 时间范围控件使用查看设备的系统时区；统计分组按设置的 IANA 时区。跨时区查看时需注意这一差异。
* 同步仅面向单用户及互不重叠的设备会话；完整会话跨设备同步、多用户权限及远端网页登录不在本版范围。

## 来源与许可

项目为 MIT。复用的 CC Switch 代码、翻译、视觉 token 和内置价格保留原作者 Jason Young 的 MIT 声明，来源提交和修改范围见 [third_party/UPSTREAM.md](third_party/UPSTREAM.md) 与 [原许可证](third_party/CC-Switch-LICENSE)。
