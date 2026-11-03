# 最新二进制构建

本目录按平台保存最新已验证的可执行程序，每个平台保留一份。当前范围为 Windows x64 桌面版和 Linux x64 同步服务端。

| 平台目录 | 程序 | 用途 |
| --- | --- | --- |
| [windows-x86_64](windows-x86_64/) | `harness-scope-desktop.exe` | Windows 桌面应用，内嵌前端 |
| [linux-x86_64](linux-x86_64/) | `harness-scope-service` | Linux 同步服务器，命令行程序 |

各目录中的 `README.txt` 包含运行步骤和系统依赖。无需安装 Node.js 或 Rust，不包含用户数据库、账号配置或凭据。Linux 产物为服务端，不包含桌面界面；macOS、Linux 桌面版和 ARM64 本次未收录。

`manifest.json` 记录各程序实际内嵌的构建身份、大小、SHA-256 和验证来源；`SHA256SUMS.txt` 用于检查文件完整性。当前应用版本为 0.2.0，数据库 schema v7，同步协议 v4。两份程序均为 release 优化构建；正式标签版本与开发构建以各自 `channel`、`displayVersion` 为准，不能仅凭 release 配置称为正式版本。

## 校验

在本目录运行：

```powershell
Get-FileHash .\windows-x86_64\harness-scope-desktop.exe -Algorithm SHA256
Get-FileHash .\linux-x86_64\harness-scope-service -Algorithm SHA256
```

Linux 可在本目录运行 `sha256sum --check SHA256SUMS.txt`。

## 更新约定

完成对应平台的构建和验收后，再更新同名程序及清单。不得用旧版本或其他平台程序代替缺失产物，也不在这里存放测试 EXE、PDB、编译缓存或数据库。替换旧程序时遵循仓库的可恢复清理规则。

当前二进制及生成清单保留在本机并由 `.gitignore` 排除；说明文件可以提交。更新目录不会自动创建 Git 标签、上传 GitHub Release 或部署服务器。
