# 自部署同步服务器

[项目首页](../README.md) · [文档索引](../docs/README.md) · [同步契约](../docs/ARCHITECTURE.md)

Rust／Axum 服务与桌面共用核心数据模型，采用单用户 Bearer token。以下 Cargo 和打包命令在仓库根目录执行。生产启动不加 `--local`；该参数用于具有本机文件能力的开发 API。

## 本机启动

设置至少 24 字符的随机令牌，不要提交到仓库：

```powershell
$env:HARNESS_SCOPE_TOKEN = python -c "import secrets; print(secrets.token_urlsafe(48))"
cargo run --release --locked -p harness-scope-service -- --db data/server.db --bind 127.0.0.1:8787
```

保留生成的令牌供客户端配置，远程访问通过 HTTPS 反向代理。在桌面「设置 → 多设备同步」填写服务器地址和相同令牌。

```powershell
cargo test -p harness-scope-service --locked
cargo build -p harness-scope-service --release --locked
```

## Docker Compose

仓库中的 [compose.yaml](../compose.yaml) 使用同一个环境变量：

```powershell
# 当前终端已设置 HARNESS_SCOPE_TOKEN
# 后续重启也须使用同一个令牌
docker compose up --build -d
```

默认只向宿主机 `127.0.0.1:8787` 发布端口，数据库保存在命名卷 `scope-data`。HTTPS 代理需要能连接此端口。删除容器不等于备份数据；不要在需要保留数据库时删除命名卷。

## 接口与兼容

| 接口 | 用途 |
| --- | --- |
| `GET /health` | 无认证的版本、schema、协议和能力检查 |
| `POST /v1/push` | 上传请求事实和设备名称 |
| `GET /v1/changes?after=0&limit=500&protocolVersion=4` | 分页下载请求变化 |
| `POST /v1/subscriptions/push` | 上传共享订阅和真实采样 |
| `GET /v1/subscriptions/changes` | 分页下载共享订阅历史 |

健康检查以外的接口需要 Bearer token。当前 schema 为 v6，请求协议为 v4；健康检查声明 `claude_request_timing_v2` 和 `subscription_history_v1` 能力。升级时先更新服务端及代理路由，再更新各客户端。

请求上传支持 gzip，下载按 Accept-Encoding 协商 gzip；解压后上传上限为 5 MiB。代理也应限制传输体，并仅开放上述同步接口。正文、项目路径、API Key 和自定义查询脚本不在同步负载中。

## Debian 部署模板

[deploy/](deploy/) 提供构建、systemd、Nginx、证书和定时备份模板。**这些脚本保留了原部署环境的固定域名、IP 校验及站点文件名，首次部署必须修改为自己的配置。** 重点检查：

- [install.sh](deploy/install.sh)：Nginx 站点文件名与启用链接。
- [enable-https.sh](deploy/enable-https.sh)：域名、DNS 地址校验、证书和站点路径。
- [nginx-http.conf](deploy/nginx-http.conf)、[nginx.conf](deploy/nginx.conf)：server_name 与证书路径。
- [build.sh](deploy/build.sh) 和 systemd 单元：安装根目录、工具链及资源限制是否适合目标机器。

部署机需准备 Python 3、Nginx、Certbot、curl、tar 及 Rust 构建所需系统依赖。模板的默认布局为：

| 路径 | 内容 |
| --- | --- |
| `/opt/harness-scope/src/` | 源码快照 |
| `/opt/harness-scope/build-tools/` | 脚本隔离安装的工具链与 Cargo 缓存 |
| `/opt/harness-scope/build-target/` | Linux 构建产物 |
| `/opt/harness-scope/releases/`、`current` | 发布目录与当前版本链接 |
| `/etc/harness-scope/server.env` | 同步令牌，root 所有、0600 |
| `/var/lib/harness-scope/server.db` | 服务端数据库 |
| `/var/backups/harness-scope/` | 一致性备份 |

先生成源码包；下例 SSH 别名 `scope-server` 需替换为自己的主机配置：

```powershell
python server/deploy/package_source.py output/deploy/harness-scope-source.tar.gz
ssh scope-server "install -d -m 0755 /opt/harness-scope/src"
scp output/deploy/harness-scope-source.tar.gz scope-server:/opt/harness-scope/source.tar.gz
ssh scope-server "tar -xzf /opt/harness-scope/source.tar.gz -C /opt/harness-scope/src"
```

升级时等旧构建结束再更新源码；在服务器执行：

```bash
bash /opt/harness-scope/src/server/deploy/build.sh
# 安装前完成域名等配置调整；升级现有服务时先做一致性备份
bash /opt/harness-scope/src/server/deploy/install.sh
```

构建脚本当前固定 Rust 1.91.0、单构建任务，不修改全局 PATH。安装脚本要求 root，创建专用无登录用户、发布目录、服务和备份计时器；首次生成令牌，后续保留现有令牌与数据库。服务仅监听 loopback，以 SIGINT 优雅停机。

配置自己的 DNS 与 80／443 访问后，使用已调整的 `enable-https.sh` 申请证书。它使用 Certbot webroot，续期成功后检查并重载 Nginx。客户端填写自己的 HTTPS 域名。

## 同步验证

对已有数据的服务使用只读探测。令牌文件格式为 `HARNESS_SCOPE_TOKEN=...`，存放在被忽略的位置：

```powershell
cargo run --locked -p harness-scope-service --example sync_probe -- --url https://scope.example.com --token-file output/deploy/server.env --read-only
```

将示例域名改为自己的地址。该模式创建临时客户端并验证认证及下载，不上传合成记录。

不带 `--read-only` 的探测会写入合成设备和用量，只能对独立空测试服务器运行：

```powershell
cargo run --locked -p harness-scope-service --example sync_probe -- --url http://127.0.0.1:8787 --token-file output/deploy/test-server.env
```

测试服务器应使用独立数据库和令牌。工具验证重试、双向同步、幂等、修订、分页和重启；订阅历史的并发与冲突另由 [subscription_sync.rs](tests/subscription_sync.rs) 回归覆盖。

## 升级、备份与恢复

日常备份由 [backup.py](deploy/backup.py) 使用 SQLite Backup API 生成，`quick_check` 通过后发布。计时器每日运行，随机延迟至多 30 分钟，保留最近 14 份；异机备份需另行配置。

```bash
systemctl start harness-scope-backup.service
journalctl -u harness-scope-backup.service -n 10 --no-pager
```

数据库升级先在一致性副本上验证。`--db COPY --compact` 执行迁移、VACUUM 和 WAL 截断后退出，不启动监听，只能在该数据库未被服务使用时执行。v6 升级用 [audit_v6.py](deploy/audit_v6.py) 比较升级前后副本；[audit_v4.py](deploy/audit_v4.py) 仅用于对应旧版迁移审计，不混用。

正式切换前暂停主服务与备份任务，生成最终备份并保留旧二进制，按副本上验证的流程升级。回滚需恢复匹配的旧数据库和程序。不能只替换程序，也不能在未处理 WAL 状态时用旧主文件覆盖在线数据库。

恢复历史快照时，为 `meta.server_instance_id` 写入 JSON 字符串编码的新 UUID，再启动服务。来源设备下一次连接会重建请求镜像并补传本机事实、墓碑及共享采样；订阅历史的已下载缓存保留。只有仍持有本机数据且重新上线的设备能补齐缺失记录。

服务端请求变更表只保存各记录最新序号位置，不保存每次修订的完整副本。墓碑和最新位置不能按日期直接删除；正常空间回收使用停机后的 `--compact`。

## 日常检查

```bash
systemctl status harness-scope.service
journalctl -u harness-scope.service -n 50 --no-pager
systemctl list-timers harness-scope-backup.timer
curl --fail http://127.0.0.1:8787/health
```

同时检查公网 HTTPS 健康接口和需要认证的下载接口，避免只验证本机端口。令牌不写入仓库、命令参数或报告；升级共享历史时确认代理已开放两条订阅路由。
