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
