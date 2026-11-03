Harness Scope — Linux x64 同步服务端

这是同步服务器的 ELF 可执行程序，不是桌面 GUI。
依赖：x86_64 Linux、glibc 2.39 或以上、libgcc_s.so.1 及标准 glibc 运行库。
已在 Debian 13 x86_64 部署并完成 HTTPS 真实同步对账；不适用于 Alpine/musl 或 ARM64。
无需 Node.js、Rust；不包含令牌、用户数据库或生产服务器配置。

从 Windows 复制到 Linux 后，在此目录运行：
chmod +x ./harness-scope-service
./harness-scope-service --help

前台启动示例（bash，交互输入自己的至少 24 字符随机令牌）：
read -rsp 'HARNESS_SCOPE_TOKEN: ' HARNESS_SCOPE_TOKEN; printf '\n'
export HARNESS_SCOPE_TOKEN
./harness-scope-service --db ./data/server.db --bind 127.0.0.1:8787

保持这个终端运行，Ctrl+C 停止服务；重启仍需使用相同令牌和数据库路径。
远程客户端通过 HTTPS 反向代理连接。不要给公网同步服务添加 --local 参数。
systemd、反向代理和一致性备份配置见仓库 server/README.md。

已有数据库升级前保留一致性备份及匹配旧程序。不要把新版数据库交给旧程序运行。
实际构建版本及 SHA-256 见上级 manifest.json 和 SHA256SUMS.txt。
数据库 schema v7；同步协议 v4；支持 request_tariff_v1 条件价格快照。
