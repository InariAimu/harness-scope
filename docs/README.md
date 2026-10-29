# 文档索引

[返回项目首页](../README.md)

项目首页介绍用途和支持范围。原 README 中的操作说明、开发命令和实现细节已按用途整理到下列文档。

## 使用与部署

| 文档 | 内容 |
| --- | --- |
| [使用指南](USAGE.md) | 数据来源、统计口径、会话、迁移、同步、网络与备份 |
| [供应商与订阅](PROVIDERS.md) | 账号配置、官方余额、参考预测、自定义查询、跨终端历史 |
| [同步服务器](../server/README.md) | 本机启动、Docker Compose、Debian 部署、升级与恢复 |

## 开发与维护

| 文档 | 内容 |
| --- | --- |
| [开发与构建](DEVELOPMENT.md) | 环境、桌面与浏览器开发、Windows 构建、版本和产物管理 |
| [架构与数据契约](ARCHITECTURE.md) | 模块边界、事实与投影、价格快照、同步和数据版本 |
| [新增工具适配器](ADDING_A_HARNESS.md) | HarnessAdapter 接口、读取投影、身份和回归要求 |
| [验证指南](VALIDATION.md) | 自动化检查、隔离演示、对账及升级验收 |
| [第三方来源与许可](../third_party/UPSTREAM.md) | 上游提交、复用范围和素材授权 |

## 工具兼容与计量

| 文档 | 内容 |
| --- | --- |
| [Claude Code](CLAUDE.md) | 请求合并、计时、推理细分和供应商边界 |
| [Codex](CODEX.md) | rollout 身份、分段与继承、初始 Fast 档位和回填 |
| [DeepSeek Harness](DSH.md) | v0／v4 格式、流式日志和供应商计价 |
| [Cherry Studio](CHERRY.md) | SQLite 会话、可确认用量和旧汇总限制 |
| [LM Studio](LMSTUDIO.md) | 本机桌面会话、生成版本、增量用量与未计价口径 |
| [Copilot 兼容导入与后续设计](COPILOT_IMPORT_DESIGN.md) | 已实现的 VS Code 导入，以及尚未实现的完整原生计量契约 |
| [速度与首字](TIMING.md) | 估算来源、样本门槛、汇总和未知值 |

OpenCode 的路径、V1／V2 边界和只读行为见[使用指南](USAGE.md#数据来源)及[适配器说明](ADDING_A_HARNESS.md)。

## 文档维护

- 当前行为以代码、配置和回归样本为依据；应用版本、数据库版本及协议版本分别说明。
- 功能说明写入对应专题；未实现的设计明确标注，不混入已支持能力。
- 临时测试日志、真实数据库副本、截图和部署回执放入被忽略的 `output/`，不作为新克隆仓库的文档依赖。
- 不在指南中维护 PID、个人服务器地址、一次性测试数量或重复的动态价表。历史实施记录可通过 Git 历史查阅。
