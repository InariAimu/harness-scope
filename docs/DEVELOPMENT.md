# 开发与构建

[文档索引](README.md) · [验证指南](VALIDATION.md)

本页整理自原项目 README 的开发、构建和版本管理说明。以下命令均在仓库根目录执行。

## 环境

- Node.js 22.12+ 与 npm；CI 使用 Node.js 22。
- Rust stable、Cargo；格式和静态检查需要 rustfmt、Clippy。
- 对应平台的 [Tauri 开发依赖](https://v2.tauri.app/start/prerequisites/)。Windows 需要 MSVC 构建工具和 WebView2。
- Python 3 用于合成演示、对账和部署辅助脚本。

```powershell
npm ci
npm run version:check
```

Rust 依赖使用仓库中的 `Cargo.lock`。下列构建命令传入 `--locked`，避免构建时更新锁文件。

## 桌面开发

```powershell
npm run desktop
```

Tauri 会启动 Vite，并通过 Rust 命令访问本机数据，不需要额外启动 HTTP 服务。默认窗口为 1000×667 逻辑像素，最小为 900×620；配置见 [tauri.conf.json](../src-tauri/tauri.conf.json)。

桌面开发会使用应用的数据目录。Windows 默认数据库为 `%APPDATA%/dev.inariaimu.harness-scope/harness-scope.db`；试验解析器或数据库迁移时使用独立副本，步骤见[验证指南](VALIDATION.md)。

## Windows 可执行文件

编译前关闭同一路径下正在运行的应用，以便链接器替换可执行文件。

```powershell
# 优化构建，内嵌前端
npm run desktop:build -- -- --locked

# 需要调试符号时使用
npm run tauri -- build --debug --no-bundle -- --locked
```

| 构建 | 输出 |
| --- | --- |
| release | `target/release/harness-scope-desktop.exe` |
| debug | `target/debug/harness-scope-desktop.exe` |

两个命令都会执行前端版本检查、TypeScript 检查和 Vite 构建。当前 `bundle.active=false`，不生成安装器；签名和自动更新未配置。其他平台需要在对应系统安装依赖并验证构建。

## 浏览器开发与合成演示

浏览器通过 Vite 的 `/api` 代理访问本机 Rust 服务。`--local` 服务具有本机文件访问能力，强制限制在 loopback；生产同步服务不提供该命令 API。

```powershell
# 创建独立演示库；脚本拒绝覆盖已有文件
python scripts/seed_demo.py
cargo run --locked -p harness-scope-service -- --local --db output/playwright/demo.db --bind 127.0.0.1:8788
```

另开终端：

```powershell
npm run dev
```

打开 `http://127.0.0.1:1420`。初始演示来源为仓库中的 Claude／Codex 合成样本，关闭 models.dev 自动导入，未配置同步服务器。测试真实来源时另建数据库，不复用正式桌面库。

## 版本与构建标识

应用版本的唯一编辑入口是根 [Cargo.toml](../Cargo.toml) 的 `[workspace.package].version`。Rust 工作区包继承此值；修改后同步 npm、Tauri 和锁文件：

```powershell
npm run version:sync
npm run version:check
npm run test:version
```

前端构建、Rust 构建和 CI 检查版本一致性。功能增加通常提升次版本，修复提升补丁版本。应用版本、数据库 schema 和同步协议独立演进，当前数据契约见[架构说明](ARCHITECTURE.md#版本与兼容)。

普通构建显示 `<version>-dev.<UTC构建时间>`；侧栏简写开发标识，「设置 → 关于」显示完整版本、构建时间、Git 提交、工作区修改状态、schema 和协议版本。桌面显示实际 Rust 程序的信息，独立浏览器显示 Vite 构建信息。

发布时先完成检查并提交版本变更，再为对应提交创建匹配的 `v<version>` 标签。只有工作区干净且 HEAD 上有匹配标签，构建才标为正式版本。Rust 的 release 优化配置本身不决定发布身份；构建脚本不自动提交、打标签或上传产物。

## 目录与入口

| 路径 | 职责 |
| --- | --- |
| `src/` | React 页面、组件、查询缓存和客户端接口 |
| `src-tauri/` | 桌面窗口与命令桥接 |
| `crates/core/` | 解析、存储、计价、查询、账号及同步 |
| `server/` | Axum 同步服务、本机开发 API 和部署工具 |
| `tests/fixtures/` | 可提交的合成来源样本 |
| `scripts/` | 版本、演示和对账工具 |

新增来源见[适配器指南](ADDING_A_HARNESS.md)，检查命令集中在[验证指南](VALIDATION.md)，服务端部署集中在[服务器说明](../server/README.md)。

## 产物与空间

Agent 执行构建、测试及收尾时遵循根目录 [AGENTS.md](../AGENTS.md)：按变更选择目标，验证完成后及时清理中间产物。Windows 清理使用回收站，核验完成后由用户决定何时清空，不直接永久删除。

`target/`、`node_modules/`、`dist/` 和 `output/` 均不进入 Git。Rust 的 debug 依赖、PDB 和增量缓存会随多次构建增长；停止相关构建和程序后可以清理 `target/debug`，下次 debug／测试构建会重新生成。仅运行 release 构建通常不会生成 debug 缓存。

`output/` 同时容纳临时测试数据和升级备份，不能整体当作缓存删除。完成验收后可清理对应测试库、WAL／SHM、复制日志和旧程序；保留需要的报告、截图，以及成对的升级前数据库与可执行文件。清理目录链接时不要跟随到外部数据。依赖和产物清理不应触碰 AppData 中的正式数据库。
