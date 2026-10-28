# 验证指南

[文档索引](README.md) · [开发环境](DEVELOPMENT.md) · [服务器验证](../server/README.md#同步验证)

这里维护可重复执行的检查方法。旧文档中的逐次开发流水、PID、个人数据量和临时产物路径已移除，历史记录可在 Git 中查阅。一次旧测试通过不代表当前工作区已验证。

## 自动化检查

以下是与 [CI 工作流](../.github/workflows/ci.yml) 对齐的完整检查，在仓库根目录运行。日常局部修改按根目录 [AGENTS.md](../AGENTS.md) 选择受影响的目标，避免每次重复完整构建；验证完成后及时将不再需要的中间产物移入回收站。

```powershell
npm ci
npm run version:check
npm run test:version
npm test
python -m unittest discover -s scripts -p "test_*.py"
npm run build
cargo fmt --all --check
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
```

Linux 服务端的独立检查：

```bash
cargo test -p harness-scope-core -p harness-scope-service --locked
```

CI 分别在 Windows 和 Linux 执行这些检查；原生窗口交互、安装器和真实账号行为需另行验证。测试数量以本次输出为准，不在文档中维护易过时的累计数字。

## 按变更选择重点

| 变更 | 重点检查 |
| --- | --- |
| 适配器／解析器 | 合成格式、准确总量、稳定 ID、重扫、追加、半行、继承／副本去重、损坏来源保留事实 |
| 费用／查询 | 缓存语义、十进制金额、未知与零、冻结单价、时区、日汇总边界、分页与各筛选组合 |
| 同步 | 真实 loopback HTTP 双客户端、重试、乱序、冲突、鉴权、游标、服务器重建与墓碑 |
| 订阅历史 | 绑定边界、并发采样、同 ID 冲突、迟到数据、重置、回退读数、配置／身份变化 |
| 界面 | 中英文、明暗主题、窗口尺寸、空值、长内容、键盘操作、请求失败与重试 |
| 版本／发布 | 清单一致性、开发／正式标签条件、关于页、实际可执行文件的构建标识 |
| 纯文档 | 相对链接、标题锚点、示例命令与当前实现、过时版本描述；无需为此重建应用 |

Rust 测试位于 [core/tests](../crates/core/tests) 和 [server/tests](../server/tests)，前端测试与被测模块相邻；Python 检查位于 [scripts](../scripts)。Copilot 的 Python 样本契约包含未来语义，不能替代已实现适配器的 Rust 验收。

## 隔离界面验证

按[合成演示](DEVELOPMENT.md#浏览器开发与合成演示)创建新数据库并启动本机服务。脚本仅指向仓库 fixture，不连接真实账号或同步服务器。额外的浏览器 fixture 应使用合成 API，并说明与真实后端验证的区别。

Windows 可执行文件验收按[构建步骤](DEVELOPMENT.md#windows-可执行文件)生成程序，确认路径、进程、窗口响应和关于页信息。进程正常不等于所有交互通过；按实际改动补查筛选、列表、图表或账号操作。

托盘和设置验收还需检查：默认开关值、右键打开／退出、左键恢复、X／Alt+F4 隐藏或退出、隐藏后的页面保留和后台采集、静默启动无主窗口、重复启动保持单进程、自启注册启停，以及设置重启后保留。普通选择自动保存；文字提交与无效值回退、中英文、保存失败、连续改动和页签切换应分别覆盖。验收后恢复原用户设置及自启状态，不通过注销或重启 Windows 中断用户工作。

## 真实日志只读对账

[compare_usage.py](../scripts/compare_usage.py) 只读数据库和指定来源，以会话／模型归一化 Token，并用 Decimal 比较可用金额。先查看参数：

```powershell
python scripts/compare_usage.py --help
```

用实际路径和会话 ID 替换以下占位符：

```powershell
python scripts/compare_usage.py --scope-db <SCOPE_DB> --ccswitch-db <CCSWITCH_DB> --harness codex --session-id <SESSION_ID> --report output/codex-comparison.json
python scripts/compare_usage.py --scope-db <SCOPE_DB> --ccswitch-db <CCSWITCH_DB> --opencode-db <OPENCODE_DB> --harness opencode --session-id <SESSION_ID> --report output/opencode-comparison.json
python scripts/compare_usage.py --scope-db <SCOPE_DB> --harness claude --claude-projects <PROJECTS_OR_JSONL> --session-id <SESSION_ID> --report output/claude-comparison.json
```

差异、未知费用或缺少可用参照可能返回非零；结合报告区分原因，不能将未比较误报为账单一致。Claude 的累计 `cost-state` 不等于可见请求总量，详见 [Claude 说明](CLAUDE.md)。

## 升级与回填验收

1. 通过 SQLite Backup API 生成一致性副本，保留对应旧程序；正式库仍有 WAL 时不能只复制主文件作为完整备份。
2. 在隔离副本执行升级或扫描，关闭副本的账号刷新、真实同步及不相关来源。
3. 按稳定 ID 比较既有 Token、模型、时间、冻结费用、修订、设备身份、设置和游标；明确哪些字段允许变更。
4. 对回填再次扫描，检查幂等；区分写入中的来源产生的新事件与重复导入。
5. 检查 `PRAGMA quick_check`、诊断及两类待传队列。服务器 schema 升级另按部署文档使用对应审计工具。

删除正文、修改正式账号、重新计价和向真实服务器写入合成数据都不是只读对账的一部分，应在相应操作的明确范围内执行。

## 验证产物

日志、截图、报告、数据库副本和旧程序放入被忽略的 `output/`。报告记录提交／构建标识、命令、结果、数据来源和验证限制，不依赖个人机器上的永久路径。真实日志、凭据和包含个人会话的数据库不提交到仓库。

验收结束后区分临时数据与回滚备份：可移除已完成实验的数据库和复制日志，保留需要的证据与成对回滚材料。清理后不要继续在文档中宣称该临时文件仍可下载或复现。
