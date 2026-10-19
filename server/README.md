# 同步服务器

`src/` 为 Rust/Axum 服务，`tests/` 为 HTTP 集成测试，`examples/sync_probe.rs` 为使用真实客户端引擎的部署验证工具，`deploy/` 为 Debian 部署文件。此 crate 与 `../crates/core` 共用同步协议和 SQLite 实现；以下 Cargo 命令均在仓库根目录执行。

```powershell
cargo test -p harness-scope-service --locked
cargo build -p harness-scope-service --release --locked
```

生产启动不加 `--local`。`--local` 提供本机命令 API，仅供桌面前端开发。

## Debian 目录与进程

| 路径 | 内容 |
| --- | --- |
| `/opt/harness-scope/src/` | 仓库源码快照，服务器代码在其 `server/` 子目录 |
| `/opt/harness-scope/build-tools/` | 隔离安装的 Rust 1.91.0 和 Cargo 缓存 |
| `/opt/harness-scope/build-target/` | Linux 构建产物 |
| `/opt/harness-scope/releases/` | 带时间戳的可执行文件与备份脚本 |
| `/opt/harness-scope/current` | 当前发布版本的符号链接 |
| `/etc/harness-scope/server.env` | 随机同步令牌，root 所有、0600 |
| `/var/lib/harness-scope/server.db` | 生产 SQLite 数据库 |
| `/var/backups/harness-scope/` | 最近 14 份一致性备份 |

`harness-scope.service` 以专用无登录用户运行，只监听 `127.0.0.1:8787`，设置内存上限、文件系统保护与自动重启。退出信号使用 SIGINT，与程序的优雅停机处理一致。

## 上传与构建

先在本机生成不含数据库、密钥和构建缓存的源码包，再上传到服务器：

```powershell
python server/deploy/package_source.py output/deploy/harness-scope-source.tar.gz
ssh harness-scope-debian "install -d -m 0755 /opt/harness-scope/src"
scp output/deploy/harness-scope-source.tar.gz harness-scope-debian:/opt/harness-scope/source.tar.gz
ssh harness-scope-debian "tar -xzf /opt/harness-scope/source.tar.gz -C /opt/harness-scope/src"
```

升级时应在旧构建结束后再更新源码。首次在服务器构建：

```bash
systemd-run --unit=harness-scope-build --property=RemainAfterExit=yes \
  --property=MemoryMax=1300M --property=CPUQuota=150% \
  --property=TasksMax=256 \
  /bin/bash /opt/harness-scope/src/server/deploy/build.sh
journalctl -fu harness-scope-build.service
```

构建使用单任务和固定工具链，不更改 root 的 PATH。再次使用同一临时构建单元前，执行 `systemctl stop harness-scope-build.service`；若此前失败，再执行 `systemctl reset-failed harness-scope-build.service`。

构建成功后安装，首次生成令牌；后续安装保留令牌和数据库：

```bash
bash /opt/harness-scope/src/server/deploy/install.sh
```

脚本安装服务、备份计时器和独立 Nginx 站点，保留其他站点配置。之前的发布文件会保留在 `releases/`；升级前先备份数据库，因为数据库迁移后仅切换旧二进制未必能回滚。

## HTTPS

在 DNS 添加 `hs.amu.moe` 的 A 记录 `49.51.191.33`，并确保云安全组可访问 80/443。解析生效后：

```bash
bash /opt/harness-scope/src/server/deploy/enable-https.sh
```

使用已有 Certbot 的 webroot 插件签发独立证书，续期成功后检查并重载 Nginx。Nginx 仅转发 `/health`、`/v1/push`、`/v1/changes`，请求体上限 5 MiB、每 IP 限流 20 次/秒并允许短时突发，其他路径返回 404。

桌面设置中填写 `https://hs.amu.moe` 和 `server.env` 内的令牌值。`/health` 无需认证，用量接口要求 Bearer token；当前是单用户共享令牌模型。

## 同步验证

对生产服务可使用只读检查：建立空的临时客户端，通过真实同步引擎验证认证和下载，不上传记录：

```powershell
cargo run --locked -p harness-scope-service --example sync_probe -- --url https://hs.amu.moe --token-file output/deploy/hs.amu.moe.env --read-only
```

先使用独立服务器数据库进行探测。探测会写入两台合成设备、三条记录，需要空的测试库，不能对已有用户数据的服务执行。

```powershell
cargo run --locked -p harness-scope-service --example sync_probe -- --url https://hs.amu.moe --token-file output/deploy/hs.amu.moe.env
```

探测使用临时目录与合成 Claude/Codex 日志，不扫描真实会话。它检查错误令牌保留 outbox、双向同步、重复上传幂等、供应商字段、修订替换、游标分页和客户端重启。初始为 3 条记录、410 Tokens，修订后为 417 Tokens，待传应为 0。测试完成后归档测试服务器数据库，生产启动使用新库和新令牌。

## 备份与恢复

`harness-scope-backup.timer` 每天运行一次，随机延迟至多 30 分钟，保留最近 14 份快照。使用 Python `sqlite3.Connection.backup`，验证 `quick_check` 后才发布备份文件。手动验证：

```bash
systemctl start harness-scope-backup.service
journalctl -u harness-scope-backup.service -n 10 --no-pager
```

目前备份只在本服务器，异机备份需要另行配置目的地。恢复旧快照前应停止主服务和备份任务，完整保留原数据库及 WAL/SHM 文件；恢复完成后更新 `meta.server_instance_id` 为 JSON 字符串编码的新 UUID，再启动服务，促使所有设备重建镜像并重新上传本机记录。只有仍持有原始本机数据的设备上线后，才能补齐其记录。当前变化日志没有裁剪机制。

## 日常检查

```bash
systemctl status harness-scope.service
journalctl -u harness-scope.service -n 50 --no-pager
systemctl list-timers harness-scope-backup.timer
curl --fail https://hs.amu.moe/health
```

服务器重启后服务与备份计时器自动启动。令牌不应写入仓库、命令参数或测试报告。
