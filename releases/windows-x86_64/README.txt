Harness Scope — Windows x64 桌面版

启动：双击 harness-scope-desktop.exe。
程序已内嵌前端，不需要 Node.js、Rust 或另起前端服务。
需要 Microsoft Edge WebView2 Runtime；其他运行依赖见 releases/manifest.json。
校验下载或复制的程序时，在 PowerShell 中运行 Get-FileHash .\harness-scope-desktop.exe -Algorithm SHA256，并与上级目录 SHA256SUMS.txt 中该程序的值比对。

应用仍使用当前 Windows 用户的标准数据目录：
%APPDATA%\dev.inariaimu.harness-scope
把程序复制到这个目录不代表便携数据模式；已有设置和数据库继续使用原位置。
程序未签名，未包含安装器或自动更新功能。
关闭到托盘等行为沿用现有设置。

构建版本见上级 manifest.json。跨版本升级前请核对对应版本的数据库和同步协议兼容说明。
