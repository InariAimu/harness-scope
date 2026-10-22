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

使用已有 Certbot 的 webroot 插件签发独立证书，续期成功后检查并重载 Nginx。Nginx 仅转发 `/health`、`/v1/push`、`/v1/changes`、`/v1/subscriptions/push` 和 `/v1/subscriptions/changes`，请求体上限 5 MiB、每 IP 限流 20 次/秒并允许短时突发，其他路径返回 404。

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

### v6 订阅历史同步

先备份数据库，部署新版服务及 `deploy/nginx.conf` 中两条 `/v1/subscriptions/*` 路由，再升级各客户端。`/health` 的 `recordCapabilities` 包含 `subscription_history_v1` 才支持此功能。两个接口沿用同一 Bearer token、gzip 及解压后的请求上限；请求用量的 v4 接口保持兼容。订阅采样为不可变记录，同一 ID 的不同内容会被拒绝，不采用覆盖合并。共享订阅目录只包含 ID、供应商类型和初次创建时的显示名称，不包含凭据或查询脚本。

客户端明确关联共享订阅后补传已有真实采样。允许多台设备同时使用并独立采样；按采样时间合并，同一身份、套餐、额度桶和重置窗口内已用比例回退的读数不参与趋势和测算，原始记录保留。请求 Token 仍按设备、事件 ID 和修订精确同步，不参与额度筛选；未上传数据无法计入。服务器恢复时更换 `server_instance_id`，各来源设备下一次连接会补传其本机采样和订阅目录；已下载的历史在客户端保留。原有请求记录的恢复规则不变。v6 升级为追加表及索引，不改写已有请求或价格事实；在独立副本运行新版 `--compact` 后，使用 `python3 server/deploy/audit_v6.py BEFORE AFTER` 逐表验证原始数据、元信息和游标完全一致。`audit_v4.py` 是旧 v4 计时迁移专用工具，不用于验证本次升级。

### v4 升级及空间回收

当前数据库为 v6，请求用量同步协议仍为 v4。先部署服务器，再升级每台客户端；v4 计时来自来源设备，服务器无法凭 Token 数反推历史耗时。客户端首次启动自动补传已有计时，保留用量和冻结费用。

变更表只保存每条记录最新的 `(seq, device_id, id)`，关联当前记录生成下载页，不再保存每次修订的完整 JSON。旧游标仍可追赶最新修订，新设备从零下载全部当前记录和删除墓碑，不需重置设备身份或重新上传全部事实。只有实际接受的新修订才推进序号，重试和低修订记录不产生日志。墓碑及每条记录的最新位置不能按日期直接删除。

首次升级先通过 SQLite backup 生成一致性快照，在独立副本运行新二进制 `--db COPY --compact`，再以 `python3 server/deploy/audit_v4.py BEFORE COPY` 验证身份、用量、费用、源路径及序号。正式切换时暂停服务和备份计时器，重新生成最终快照，运行 `--db /var/lib/harness-scope/server.db --compact` 并审计，成功后启动新版本。`--compact` 执行迁移、VACUUM 和 WAL 截断后退出，不启动监听，只能在该数据库的服务停止时执行。失败时保留原快照与旧二进制；回滚需恢复对应版本的数据快照。

上传使用 gzip，下载按 Accept-Encoding 协商 gzip，仍接收未压缩请求。5 MiB 请求上限作用于解压后内容；Nginx 保留传输体限制并透传编码。

`harness-scope-backup.timer` 每天运行一次，随机延迟至多 30 分钟，保留最近 14 份快照。使用 Python `sqlite3.Connection.backup`，验证 `quick_check` 后才发布备份文件。手动验证：

```bash
systemctl start harness-scope-backup.service
journalctl -u harness-scope-backup.service -n 10 --no-pager
```

目前备份只在本服务器，异机备份需要另行配置目的地。恢复旧快照前应停止主服务和备份任务，完整保留原数据库及 WAL/SHM 文件；恢复完成后更新 `meta.server_instance_id` 为 JSON 字符串编码的新 UUID，再启动服务，促使所有设备重建镜像并重新上传本机记录。只有仍持有原始本机数据的设备上线后，才能补齐其记录。历史变更在 v4 中自动合并为每条记录的最新位置。

## 日常检查

```bash
systemctl status harness-scope.service
journalctl -u harness-scope.service -n 50 --no-pager
systemctl list-timers harness-scope-backup.timer
curl --fail https://hs.amu.moe/health
```

服务器重启后服务与备份计时器自动启动。令牌不应写入仓库、命令参数或测试报告。
