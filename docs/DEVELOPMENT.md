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

本机最新已验证产物集中在 [releases/](../releases/README.md)，按 `windows-x86_64/`（桌面版）和 `linux-x86_64/`（同步服务端）区分，并附构建清单与 SHA-256。可执行文件和生成清单不进入 Git；该目录不是独立便携数据目录，也不会自动发布到 GitHub。

## GitHub Actions 构建

[Build binaries](../.github/workflows/build.yml) 在推送、提交拉取请求及手动触发时构建优化版。登录 GitHub 后，在仓库的 **Actions → Build binaries** 中打开对应的运行记录，在 **Artifacts** 下载：

| 产物 | 用途 |
| --- | --- |
| `harness-scope-desktop.exe` | Windows x64 桌面版，内嵌前端，需要 WebView2 |
| `harness-scope-service-linux-x86_64.tar.gz` | Linux x64 同步服务端，使用 `tar -xzf` 解包，需要 glibc 2.39 或以上 |

产物保留 14 天，不会创建 GitHub Release。手动构建时，在 **Run workflow** 中选择要编译的分支。

## 三平台 Release

[Release packages](../.github/workflows/release.yml) 是手动触发的正式版本打包流程。它在 Windows、Ubuntu 22.04 和 macOS runner 上分别生成 Windows x64 EXE、Linux x64 AppImage/DEB 与同步服务端压缩包，以及兼容 Apple Silicon/Intel 的 macOS 通用 DMG。macOS 最低版本为 11.0；Linux 构建要求 glibc 2.35+，DEB 还需要 WebKitGTK 4.1。

工作流以选定提交为源，在 runner 内创建与应用版本一致的本地标签来生成正式构建标识。已有标签若指向其他提交会拒绝构建。流程仅上传 Actions artifacts，不修改远程标签或发布 Release；应先完成所有平台构建及同一提交的 `checks`，再发布：

```powershell
gh workflow run release.yml --ref main -f tag=v0.2.0
gh run list --workflow release.yml
# 使用上一步实际成功的运行 ID。
gh run download <RUN_ID> -n release-assets -D output/release-v0.2.0/assets
# 按 build-manifest.json 核对提交、平台和版本，并校验 SHA256SUMS.txt。
git tag -a v0.2.0 <VERIFIED_COMMIT> -m "Harness Scope v0.2.0"
git push origin refs/tags/v0.2.0
gh release create v0.2.0 'output/release-v0.2.0/assets/*' --verify-tag --draft --title 'Harness Scope v0.2.0' --notes-file .github/release-notes/v0.2.0.md
gh release view v0.2.0
gh release edit v0.2.0 --draft=false --latest
```

下载文件的完整性由 `SHA256SUMS.txt` 和 `build-manifest.json` 记录，后者也保留各架构的源代码提交及实际构建身份。缺少平台、混入开发构建或校验和不符时，汇总步骤失败。

发布配置 [tauri.release.conf.json](../src-tauri/tauri.release.conf.json) 仅在打包时启用，不改变日常 Windows EXE 构建。当前 Windows 未签名；macOS 使用 ad-hoc 签名，未配置 Developer ID 或 Apple 公证，首次安装可能需要用户在系统安全设置中确认。自动更新未配置。

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
