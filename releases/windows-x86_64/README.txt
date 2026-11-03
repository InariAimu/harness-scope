Harness Scope — Windows x64 桌面版

启动：双击 harness-scope-desktop.exe。
程序已内嵌前端，不需要 Node.js、Rust 或另起前端服务。
需要 Microsoft Edge WebView2 Runtime；其他运行依赖见 releases/manifest.json。
当前机器上的原始 release 已完成窗口、托盘和真实同步验收；本文件与该版本 SHA-256 一致。

应用仍使用当前 Windows 用户的标准数据目录：
%APPDATA%\dev.inariaimu.harness-scope
把程序复制到这个目录不代表便携数据模式；已有设置和数据库继续使用原位置。
程序未签名，未包含安装器或自动更新功能。
关闭到托盘等行为沿用现有设置。

实际构建版本及 SHA-256 见上级 manifest.json 和 SHA256SUMS.txt。
数据库 schema v7；同步协议 v4；支持 request_tariff_v1 条件价格快照。
